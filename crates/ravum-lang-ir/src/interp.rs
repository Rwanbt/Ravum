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
    let Some(id) = module.func_id(entry) else {
        return RunOutcome::Fault(format!("no function `{entry}`"));
    };
    if module.functions[id as usize].params != 0 {
        return RunOutcome::Fault(format!("`{entry}` takes arguments"));
    }
    let mut m = Machine { module, steps };
    match m.call(id, &[], 0) {
        Ok(v) => RunOutcome::Value(v),
        Err(Stop::Panic(s)) => RunOutcome::Panic(s),
        Err(Stop::Diverge) => RunOutcome::Diverge,
        Err(Stop::Fault(s)) => RunOutcome::Fault(s),
    }
}

struct Machine<'m> {
    module: &'m Module,
    steps: u64,
}

impl Machine<'_> {
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
        },
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
