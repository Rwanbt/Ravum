//! Reference interpreter of IR v1 (first consumer of the IR, gate T14).
//!
//! Integer arithmetic is computed exactly in `i128` and then range-checked
//! against the operand type, so overflow detection does not depend on host
//! wrapping behavior. A step budget and a call-depth limit make every run
//! terminate; running out of either is reported, never turned into a value.

use crate::ir::*;

/// A runtime value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Val {
    Int(Ty, i128),
    F32(f32),
    F64(f64),
    Bool(bool),
    Unit,
}

impl Val {
    pub fn ty(&self) -> Ty {
        match self {
            Val::Int(t, _) => *t,
            Val::F32(_) => Ty::F32,
            Val::F64(_) => Ty::F64,
            Val::Bool(_) => Ty::Bool,
            Val::Unit => Ty::Unit,
        }
    }

    /// `to_string` text (STD-12.1), as `ravum-core-machine` renders it.
    pub fn text(&self) -> String {
        match self {
            Val::Int(_, v) => v.to_string(),
            Val::F32(v) => v.to_string(),
            Val::F64(v) => v.to_string(),
            Val::Bool(b) => b.to_string(),
            Val::Unit => "()".to_string(),
        }
    }
}

/// Result of running an entry function.
#[derive(Debug, Clone, PartialEq)]
pub enum RunOutcome {
    Value(Val),
    Panic(String),
    /// Step budget exhausted: inconclusive.
    Diverge,
    /// Call depth limit, missing entry, bad arguments: no result.
    Fault(String),
}

impl RunOutcome {
    /// Same rendering as `ravum_core_machine::Outcome::render` for values and
    /// panics, so the two oracles can be compared textually.
    pub fn render(&self) -> String {
        match self {
            RunOutcome::Value(v) => format!("value {} {}", v.ty().name(), v.text()),
            RunOutcome::Panic(m) => format!("panic: {m}"),
            RunOutcome::Diverge => "diverge".to_string(),
            RunOutcome::Fault(m) => format!("fault: {m}"),
        }
    }
}

pub const DEFAULT_STEPS: u64 = 10_000_000;
pub const MAX_DEPTH: u32 = 512;

enum Stop {
    Panic(String),
    Diverge,
    Fault(String),
}

pub fn range(ty: Ty) -> (i128, i128) {
    let bits = ty.int_bits().expect("integer type");
    if ty.is_signed() {
        (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
    } else {
        (0, (1i128 << bits) - 1)
    }
}

/// Wrap an exact integer to the bit pattern of `ty` (used by `as` casts).
pub fn wrap(ty: Ty, v: i128) -> i128 {
    let bits = ty.int_bits().expect("integer type");
    let mask: u128 = if bits == 128 {
        u128::MAX
    } else {
        (1u128 << bits) - 1
    };
    let u = (v as u128) & mask;
    if ty.is_signed() && (u >> (bits - 1)) & 1 == 1 {
        (u as i128) - (1i128 << bits)
    } else {
        u as i128
    }
}

/// Run `entry` with no argument.
pub fn run(module: &Module, entry: &str, steps: u64) -> RunOutcome {
    run_with_output(module, entry, steps).0
}

/// [`run`], also returning what the program printed (up to the stop).
pub fn run_with_output(module: &Module, entry: &str, steps: u64) -> (RunOutcome, Output) {
    let Some(id) = module.func_id(entry) else {
        return (
            RunOutcome::Fault(format!("no function `{entry}`")),
            Output::default(),
        );
    };
    if module.functions[id as usize].params != 0 {
        return (
            RunOutcome::Fault(format!("`{entry}` takes arguments")),
            Output::default(),
        );
    }
    let mut m = Machine::new(module, steps, 0);
    let r = match m.call(id, &[], 0) {
        Ok(v) => RunOutcome::Value(v),
        Err(Stop::Panic(s)) => RunOutcome::Panic(s),
        Err(Stop::Diverge) => RunOutcome::Diverge,
        Err(Stop::Fault(s)) => RunOutcome::Fault(s),
    };
    (r, m.out)
}

/// Bytes of memory the interpreter gives a run (stack + host state).
pub const MEMORY: usize = 1 << 20;
/// Addresses below this one are never valid (null guard). Host state, when
/// present, starts here.
pub const STATE_BASE: usize = 16;

/// Run a DSP block: `init` (if any, `fn() -> State` lowered with its result
/// address) writes the host state once, then `process(&mut State, x)` runs for
/// each input. Returns the outputs, or the outcome that stopped the run.
pub fn run_dsp(
    module: &Module,
    init: Option<&str>,
    process: &str,
    inputs: &[f32],
    steps: u64,
) -> Result<Vec<f32>, RunOutcome> {
    let size = module.state.as_ref().map(|s| s.size as usize).unwrap_or(0);
    let mut m = Machine::new(module, steps, size);
    let state = Val::Int(Ty::Ptr, STATE_BASE as i128);
    let stop = |e: Stop| match e {
        Stop::Panic(s) => RunOutcome::Panic(s),
        Stop::Diverge => RunOutcome::Diverge,
        Stop::Fault(s) => RunOutcome::Fault(s),
    };
    if let Some(init) = init {
        let id = module
            .func_id(init)
            .ok_or_else(|| RunOutcome::Fault(format!("no function `{init}`")))?;
        m.call(id, &[state], 0).map_err(stop)?;
    }
    let id = module
        .func_id(process)
        .ok_or_else(|| RunOutcome::Fault(format!("no function `{process}`")))?;
    let mut out = Vec::with_capacity(inputs.len());
    for x in inputs {
        match m.call(id, &[state, Val::F32(*x)], 0).map_err(stop)? {
            Val::F32(y) => out.push(y),
            other => return Err(RunOutcome::Fault(format!("`{process}` returned {other:?}"))),
        }
    }
    Ok(out)
}

struct Machine<'m> {
    module: &'m Module,
    steps: u64,
    mem: Vec<u8>,
    sp: usize,
    /// `io::print*` output, in program order.
    out: Output,
}

/// Text written by a run (`io::print`/`println` → `stdout`, `eprintln` →
/// `stderr`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
}

impl<'m> Machine<'m> {
    fn new(module: &'m Module, steps: u64, state_size: usize) -> Self {
        Machine {
            module,
            steps,
            mem: vec![0; MEMORY],
            sp: (STATE_BASE + state_size).div_ceil(16) * 16,
            out: Output::default(),
        }
    }

    fn check(&self, addr: i128, size: u32) -> Result<usize, Stop> {
        let end = addr + size as i128;
        if addr < STATE_BASE as i128 || end > self.mem.len() as i128 {
            return Err(Stop::Fault(format!("memory access out of range at {addr}")));
        }
        Ok(addr as usize)
    }

    fn load(&self, ty: Ty, addr: i128) -> Result<Val, Stop> {
        let a = self.check(addr, ty.size())?;
        let b = &self.mem[a..a + ty.size() as usize];
        let mut raw = [0u8; 8];
        raw[..b.len()].copy_from_slice(b);
        let u = u64::from_le_bytes(raw);
        Ok(match ty {
            Ty::F32 => Val::F32(f32::from_bits(u as u32)),
            Ty::F64 => Val::F64(f64::from_bits(u)),
            Ty::Bool => Val::Bool(u & 1 == 1),
            Ty::Ptr => Val::Int(Ty::Ptr, u as i128),
            Ty::Unit => Val::Unit,
            t => Val::Int(t, wrap(t, u as i128)),
        })
    }

    fn store(&mut self, ty: Ty, addr: i128, v: Val) -> Result<(), Stop> {
        let a = self.check(addr, ty.size())?;
        let u: u64 = match v {
            Val::F32(x) => x.to_bits() as u64,
            Val::F64(x) => x.to_bits(),
            Val::Bool(b) => b as u64,
            Val::Int(_, x) => x as u64,
            Val::Unit => 0,
        };
        let n = ty.size() as usize;
        self.mem[a..a + n].copy_from_slice(&u.to_le_bytes()[..n]);
        Ok(())
    }

    fn tick(&mut self) -> Result<(), Stop> {
        if self.steps == 0 {
            return Err(Stop::Diverge);
        }
        self.steps -= 1;
        Ok(())
    }

    fn call(&mut self, id: FuncId, args: &[Val], depth: u32) -> Result<Val, Stop> {
        if depth >= MAX_DEPTH {
            return Err(Stop::Fault("call depth limit".into()));
        }
        let f = &self.module.functions[id as usize];
        let mut slots: Vec<Val> = f.locals.iter().map(|t| zero(*t)).collect();
        slots[..args.len()].copy_from_slice(args);
        // Frame: stack slots at increasing aligned addresses; released on
        // return (the caller's `sp` is restored).
        let saved_sp = self.sp;
        let mut frame = Vec::with_capacity(f.stack.len());
        let mut sp = self.sp;
        for s in &f.stack {
            sp = sp.div_ceil(s.align.max(1) as usize) * s.align.max(1) as usize;
            frame.push(sp);
            sp += s.size as usize;
        }
        if sp > self.mem.len() {
            return Err(Stop::Fault("stack overflow".into()));
        }
        self.sp = sp;
        let r = self.exec(f, &mut slots, &frame, depth);
        self.sp = saved_sp;
        r
    }

    fn exec(
        &mut self,
        f: &'m Function,
        slots: &mut [Val],
        frame: &[usize],
        depth: u32,
    ) -> Result<Val, Stop> {
        let mut b: BlockId = 0;
        loop {
            let blk = &f.blocks[b as usize];
            for i in &blk.insts {
                self.tick()?;
                match i {
                    Inst::Const { dst, value } => slots[*dst as usize] = konst(*value),
                    Inst::Copy { dst, src } => slots[*dst as usize] = slots[*src as usize],
                    Inst::Bin {
                        dst,
                        op,
                        ty,
                        lhs,
                        rhs,
                    } => {
                        slots[*dst as usize] =
                            bin(*op, *ty, slots[*lhs as usize], slots[*rhs as usize])?
                    }
                    Inst::Un { dst, op, ty, src } => {
                        slots[*dst as usize] = un(*op, *ty, slots[*src as usize])?
                    }
                    Inst::Cast { dst, from, to, src } => {
                        slots[*dst as usize] = cast(*from, *to, slots[*src as usize])
                    }
                    Inst::StackAddr { dst, slot } => {
                        slots[*dst as usize] = Val::Int(Ty::Ptr, frame[*slot as usize] as i128)
                    }
                    Inst::Load {
                        dst,
                        ty,
                        addr,
                        offset,
                    } => {
                        let a = ptr(slots[*addr as usize])? + *offset as i128;
                        slots[*dst as usize] = self.load(*ty, a)?;
                    }
                    Inst::Store {
                        ty,
                        addr,
                        offset,
                        src,
                    } => {
                        let a = ptr(slots[*addr as usize])? + *offset as i128;
                        self.store(*ty, a, slots[*src as usize])?;
                    }
                    Inst::PtrAdd {
                        dst,
                        base,
                        index,
                        scale,
                    } => {
                        let Val::Int(_, i) = slots[*index as usize] else {
                            return Err(Stop::Fault("non-integer index".into()));
                        };
                        let a = ptr(slots[*base as usize])? + i * *scale as i128;
                        slots[*dst as usize] = Val::Int(Ty::Ptr, a);
                    }
                    Inst::Print { stream, arg } => {
                        let text = match arg {
                            PrintArg::Text(t) => t.clone(),
                            PrintArg::Value { src, .. } => slots[*src as usize].text(),
                        };
                        match stream {
                            Stream::Stdout => self.out.stdout.push_str(&text),
                            Stream::Stderr => self.out.stderr.push_str(&text),
                        }
                    }
                    Inst::Call { dst, func, args } => {
                        let vals: Vec<Val> = args.iter().map(|a| slots[*a as usize]).collect();
                        let r = self.call(*func, &vals, depth + 1)?;
                        if let Some(d) = dst {
                            slots[*d as usize] = r;
                        }
                    }
                }
            }
            self.tick()?;
            match &blk.term {
                Term::Jump(t) => b = *t,
                Term::Branch {
                    cond,
                    then_to,
                    else_to,
                } => {
                    b = match slots[*cond as usize] {
                        Val::Bool(true) => *then_to,
                        Val::Bool(false) => *else_to,
                        _ => return Err(Stop::Fault("non-bool condition".into())),
                    }
                }
                Term::Return(Some(x)) => return Ok(slots[*x as usize]),
                Term::Return(None) => return Ok(Val::Unit),
                Term::Trap(m) => return Err(Stop::Panic(m.clone())),
                Term::Unreachable => return Err(Stop::Fault("reached unreachable block".into())),
            }
        }
    }
}

fn ptr(v: Val) -> Result<i128, Stop> {
    match v {
        Val::Int(Ty::Ptr, a) => Ok(a),
        _ => Err(Stop::Fault("not an address".into())),
    }
}

fn zero(t: Ty) -> Val {
    match t {
        Ty::F32 => Val::F32(0.0),
        Ty::F64 => Val::F64(0.0),
        Ty::Bool => Val::Bool(false),
        Ty::Unit => Val::Unit,
        t => Val::Int(t, 0),
    }
}

fn konst(c: Const) -> Val {
    match c {
        Const::Int { ty, value } => Val::Int(ty, value),
        Const::F32Bits(b) => Val::F32(f32::from_bits(b)),
        Const::F64Bits(b) => Val::F64(f64::from_bits(b)),
        Const::Bool(b) => Val::Bool(b),
        Const::Unit => Val::Unit,
    }
}

fn trap<T>(m: &str) -> Result<T, Stop> {
    Err(Stop::Panic(m.to_string()))
}

fn checked(ty: Ty, v: i128, msg: &str) -> Result<Val, Stop> {
    let (lo, hi) = range(ty);
    if v < lo || v > hi {
        trap(msg)
    } else {
        Ok(Val::Int(ty, v))
    }
}

fn bin(op: BinOp, ty: Ty, a: Val, b: Val) -> Result<Val, Stop> {
    use BinOp::*;
    match (a, b) {
        (Val::Int(_, x), Val::Int(_, y)) => match op {
            Add => checked(ty, x + y, "attempt to add with overflow"),
            Sub => checked(ty, x - y, "attempt to subtract with overflow"),
            Mul => match x.checked_mul(y) {
                Some(p) => checked(ty, p, "attempt to multiply with overflow"),
                None => trap("attempt to multiply with overflow"),
            },
            Div => {
                if y == 0 {
                    trap("division by zero")
                } else {
                    checked(ty, x / y, "attempt to divide with overflow")
                }
            }
            Rem => {
                if y == 0 {
                    trap("remainder by zero")
                } else if ty.is_signed() && x == range(ty).0 && y == -1 {
                    trap("attempt to calculate the remainder with overflow")
                } else {
                    Ok(Val::Int(ty, x % y))
                }
            }
            BitAnd => Ok(Val::Int(ty, wrap(ty, x & y))),
            BitOr => Ok(Val::Int(ty, wrap(ty, x | y))),
            BitXor => Ok(Val::Int(ty, wrap(ty, x ^ y))),
            Shl | Shr => {
                let bits = ty.int_bits().unwrap() as i128;
                if y < 0 || y >= bits {
                    return trap("shift out of range");
                }
                if op == Shl {
                    Ok(Val::Int(ty, wrap(ty, x << y)))
                } else {
                    // `x` is exact: `>>` on i128 is arithmetic, and an
                    // unsigned `x` is non-negative, so it is logical.
                    Ok(Val::Int(ty, x >> y))
                }
            }
            Eq => Ok(Val::Bool(x == y)),
            Ne => Ok(Val::Bool(x != y)),
            Lt => Ok(Val::Bool(x < y)),
            Le => Ok(Val::Bool(x <= y)),
            Gt => Ok(Val::Bool(x > y)),
            Ge => Ok(Val::Bool(x >= y)),
            // `x`, `y` are exact; the i128 product can only overflow for
            // 64-bit operands, where wrapping in 2^128 keeps the low bits.
            WrappingAdd => Ok(Val::Int(ty, wrap(ty, x.wrapping_add(y)))),
            WrappingSub => Ok(Val::Int(ty, wrap(ty, x.wrapping_sub(y)))),
            WrappingMul => Ok(Val::Int(ty, wrap(ty, x.wrapping_mul(y)))),
            MathMin | MathMax => Err(Stop::Fault("math::min/max on an integer".into())),
            OverflowsAdd | OverflowsSub | OverflowsMul => {
                let (lo, hi) = range(ty);
                let exact = match op {
                    OverflowsAdd => Some(x + y),
                    OverflowsSub => Some(x - y),
                    _ => x.checked_mul(y),
                };
                Ok(Val::Bool(!exact.is_some_and(|v| (lo..=hi).contains(&v))))
            }
            SaturatingAdd | SaturatingSub | SaturatingMul => {
                let (lo, hi) = range(ty);
                let exact = match op {
                    SaturatingAdd => Some(x + y),
                    SaturatingSub => Some(x - y),
                    _ => x.checked_mul(y),
                };
                // An i128 product overflow means a magnitude beyond any
                // 64-bit range: its sign is the sign of `x * y`.
                let v = exact.unwrap_or(if (x < 0) != (y < 0) { lo } else { hi });
                Ok(Val::Int(ty, v.clamp(lo, hi)))
            }
        },
        (Val::F64(x), Val::F64(y)) if op == MathMin || op == MathMax => {
            // STD-6.5, written independently of the machine's code.
            Ok(Val::F64(if x.is_nan() {
                y
            } else if y.is_nan() {
                x
            } else if x == y {
                if (op == MathMin) == x.is_sign_negative() {
                    x
                } else {
                    y
                }
            } else if (op == MathMin) == (x < y) {
                x
            } else {
                y
            }))
        }
        (Val::F64(x), Val::F64(y)) => fbin(op, x, y, Val::F64),
        (Val::F32(x), Val::F32(y)) => fbin(op, x, y, Val::F32),
        (Val::Bool(x), Val::Bool(y)) => Ok(Val::Bool(match op {
            Eq => x == y,
            Ne => x != y,
            BitAnd => x & y,
            BitOr => x | y,
            BitXor => x ^ y,
            Lt => !x & y,
            Le => x <= y,
            Gt => x & !y,
            Ge => x >= y,
            _ => return Err(Stop::Fault("arithmetic on bool".into())),
        })),
        (Val::Unit, Val::Unit) => Ok(Val::Bool(matches!(op, Eq | Le | Ge))),
        _ => Err(Stop::Fault("operand type mismatch".into())),
    }
}

fn fbin<F>(op: BinOp, x: F, y: F, wrapf: fn(F) -> Val) -> Result<Val, Stop>
where
    F: Copy
        + PartialOrd
        + std::ops::Add<Output = F>
        + std::ops::Sub<Output = F>
        + std::ops::Mul<Output = F>
        + std::ops::Div<Output = F>
        + std::ops::Rem<Output = F>,
{
    use BinOp::*;
    Ok(match op {
        Add => wrapf(x + y),
        Sub => wrapf(x - y),
        Mul => wrapf(x * y),
        Div => wrapf(x / y),
        Rem => wrapf(x % y),
        Eq => Val::Bool(x == y),
        Ne => Val::Bool(x != y),
        Lt => Val::Bool(x < y),
        Le => Val::Bool(x <= y),
        Gt => Val::Bool(x > y),
        Ge => Val::Bool(x >= y),
        _ => return Err(Stop::Fault("bit operation on float".into())),
    })
}

fn un(op: UnOp, ty: Ty, a: Val) -> Result<Val, Stop> {
    match (op, a) {
        (UnOp::Neg, Val::Int(_, x)) => checked(ty, -x, "attempt to negate with overflow"),
        (UnOp::Neg, Val::F64(x)) => Ok(Val::F64(-x)),
        (UnOp::Neg, Val::F32(x)) => Ok(Val::F32(-x)),
        (UnOp::Not, Val::Bool(b)) => Ok(Val::Bool(!b)),
        (UnOp::Not, Val::Int(_, x)) => Ok(Val::Int(ty, wrap(ty, !x))),
        (UnOp::Abs, Val::Int(_, x)) => checked(ty, x.abs(), "attempt to negate with overflow"),
        (UnOp::WrappingNeg, Val::Int(_, x)) => Ok(Val::Int(ty, wrap(ty, -x))),
        (UnOp::SaturatingNeg, Val::Int(_, x)) => {
            let (lo, hi) = range(ty);
            Ok(Val::Int(ty, (-x).clamp(lo, hi)))
        }
        (UnOp::Abs, Val::F64(x)) => Ok(Val::F64(x.abs())),
        (UnOp::Sqrt, Val::F64(x)) => Ok(Val::F64(x.sqrt())),
        (UnOp::Floor, Val::F64(x)) => Ok(Val::F64(x.floor())),
        (UnOp::Ceil, Val::F64(x)) => Ok(Val::F64(x.ceil())),
        (UnOp::Round, Val::F64(x)) => Ok(Val::F64(x.round())),
        _ => Err(Stop::Fault("bad unary operand".into())),
    }
}

/// `as` with Rust semantics.
pub fn cast(from: Ty, to: Ty, v: Val) -> Val {
    let _ = from;
    match (v, to) {
        (Val::Int(_, x), t) if t.is_int() => Val::Int(t, wrap(t, x)),
        (Val::Int(_, x), Ty::F64) => Val::F64(x as f64),
        (Val::Int(_, x), Ty::F32) => Val::F32(x as f32),
        (Val::Bool(b), t) if t.is_int() => Val::Int(t, b as i128),
        (Val::Bool(b), Ty::Bool) => Val::Bool(b),
        (Val::F64(x), t) if t.is_int() => Val::Int(t, sat(t, x)),
        (Val::F32(x), t) if t.is_int() => Val::Int(t, sat(t, x as f64)),
        (Val::F64(x), Ty::F32) => Val::F32(x as f32),
        (Val::F64(x), Ty::F64) => Val::F64(x),
        (Val::F32(x), Ty::F64) => Val::F64(x as f64),
        (Val::F32(x), Ty::F32) => Val::F32(x),
        (other, _) => other,
    }
}

/// Saturating float → int (`as`): NaN → 0, out of range → bound, truncation.
fn sat(t: Ty, x: f64) -> i128 {
    let (lo, hi) = range(t);
    if x.is_nan() {
        0
    } else if x <= lo as f64 {
        lo
    } else if x >= hi as f64 {
        hi
    } else {
        x.trunc() as i128
    }
}
