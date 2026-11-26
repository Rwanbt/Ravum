//! IR v1 data model: a control-flow graph of basic blocks over typed,
//! mutable local slots. Every operation that can trap says so in its
//! semantics; backends must implement the exact trap conditions and messages.

use serde::{Deserialize, Serialize};

/// Version of the IR contract. Any change to the data model or to the
/// semantics of an operation bumps it; a consumer refuses an unknown version.
pub const IR_VERSION: &str = "ravum-ir-1.8";

/// Scalar types of IR v1. `isize`/`usize` are 64-bit (the only supported
/// targets are 64-bit).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Ty {
    I8,
    I16,
    I32,
    I64,
    Isize,
    U8,
    U16,
    U32,
    U64,
    Usize,
    F32,
    F64,
    Bool,
    Unit,
    /// A 64-bit address into memory the program may access (a stack slot of
    /// the current or a calling frame, or host-owned state). IR v1.1.
    Ptr,
    /// IR v1.3: handle of a run-heap object (`String`, `Vec<T>`, and the
    /// borrows of them), see `heap.rs`. 0 is never an object.
    Obj,
}

impl Ty {
    /// Size (= alignment) in memory, in bytes.
    pub fn size(self) -> u32 {
        match self {
            Ty::I8 | Ty::U8 | Ty::Bool => 1,
            Ty::I16 | Ty::U16 => 2,
            Ty::I32 | Ty::U32 | Ty::F32 => 4,
            Ty::I64 | Ty::U64 | Ty::Isize | Ty::Usize | Ty::F64 | Ty::Ptr | Ty::Obj => 8,
            Ty::Unit => 0,
        }
    }

    pub fn is_int(self) -> bool {
        self.int_bits().is_some()
    }
    pub fn is_float(self) -> bool {
        matches!(self, Ty::F32 | Ty::F64)
    }
    pub fn is_signed(self) -> bool {
        matches!(self, Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64 | Ty::Isize)
    }
    pub fn int_bits(self) -> Option<u32> {
        Some(match self {
            Ty::I8 | Ty::U8 => 8,
            Ty::I16 | Ty::U16 => 16,
            Ty::I32 | Ty::U32 => 32,
            Ty::I64 | Ty::U64 | Ty::Isize | Ty::Usize => 64,
            _ => return None,
        })
    }
    /// Source-level name, as `Outcome` renders it.
    pub fn name(self) -> &'static str {
        match self {
            Ty::I8 => "i8",
            Ty::I16 => "i16",
            Ty::I32 => "i32",
            Ty::I64 => "i64",
            Ty::Isize => "isize",
            Ty::U8 => "u8",
            Ty::U16 => "u16",
            Ty::U32 => "u32",
            Ty::U64 => "u64",
            Ty::Usize => "usize",
            Ty::F32 => "f32",
            Ty::F64 => "f64",
            Ty::Bool => "bool",
            Ty::Unit => "()",
            Ty::Ptr => "ptr",
            Ty::Obj => "obj",
        }
    }
}

/// Index of a local slot in its function. Parameters are slots `0..params`.
pub type Local = u32;
/// Index of a basic block in its function. Block 0 is the entry.
pub type BlockId = u32;
/// Index of a function in the module.
pub type FuncId = u32;

/// A constant. Integers are stored as the two's-complement bit pattern of
/// their type (sign-extended into `i128` for signed types); floats as bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Const {
    Int { ty: Ty, value: i128 },
    F32Bits(u32),
    F64Bits(u64),
    Bool(bool),
    Unit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinOp {
    /// Checked: traps `attempt to add with overflow`.
    Add,
    /// Checked: traps `attempt to subtract with overflow`.
    Sub,
    /// Checked: traps `attempt to multiply with overflow`.
    Mul,
    /// Ints: traps `division by zero` (EXPR-3.2) / `attempt to divide with
    /// overflow` (MIN / -1, EXPR-3.1); truncates toward zero. Floats: IEEE.
    Div,
    /// Ints: traps `remainder by zero` (EXPR-3.2) / `attempt to calculate
    /// the remainder with overflow` (MIN % -1, D-19-2); sign of the dividend.
    /// Floats: IEEE `fmod`.
    Rem,
    BitAnd,
    BitOr,
    BitXor,
    /// Traps `shift out of range` when the amount is >= bits (EXPR-3.3).
    /// The amount operand is always a `U64` slot.
    Shl,
    /// Traps `shift out of range` when the amount is >= bits (EXPR-3.3).
    /// Arithmetic for signed types, logical for unsigned.
    Shr,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    /// Ints only (IR v1.2, `int::wrapping_*`): two's-complement wrap, no trap.
    WrappingAdd,
    WrappingSub,
    WrappingMul,
    /// Ints only (IR v1.2, `int::saturating_*`): clamp to the type range.
    SaturatingAdd,
    SaturatingSub,
    SaturatingMul,
    /// Ints only (IR v1.2): `Bool`, true when the checked `+`/`-`/`*` would
    /// overflow (used by `int::checked_*`; never traps).
    OverflowsAdd,
    OverflowsSub,
    OverflowsMul,
    /// `f64` only (IR v1.2, `math::min`/`math::max`, STD-6.5): a NaN operand
    /// yields the other; `min(-0.0, +0.0) = -0.0`, `max(-0.0, +0.0) = +0.0`.
    MathMin,
    MathMax,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnOp {
    /// Ints: checked, traps `attempt to negate with overflow`. Floats: sign flip.
    Neg,
    /// Bool: logical not. Ints: bitwise not.
    Not,
    /// IR v1.2. Signed ints: checked (`int::abs`), traps
    /// `attempt to negate with overflow` on MIN. `f64`: clears the sign bit.
    Abs,
    /// IR v1.2, signed ints: `0 - x` with wrap (`int::wrapping_neg`).
    WrappingNeg,
    /// IR v1.2, signed ints: MIN → MAX (`int::saturating_neg`).
    SaturatingNeg,
    /// IR v1.2, `f64` only: IEEE-754 correctly rounded square root.
    Sqrt,
    /// IR v1.2, `f64` only: round toward -inf.
    Floor,
    /// IR v1.2, `f64` only: round toward +inf.
    Ceil,
    /// IR v1.2, `f64` only (`math::round`, STD-6.3): nearest, ties away
    /// from zero.
    Round,
}

/// IR v1.3 run-heap operations (`heap::Heap::op`): two raw 64-bit operands
/// `a`, `b` (unused ones are 0), one raw result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HeapOp {
    StrNew,
    StrClone,
    /// `a.push_str(b)` (both strings).
    StrPush,
    StrLen,
    StrEq,
    VecNew,
    /// `a.push(b)`, `b` the element's raw word.
    VecPush,
    VecLen,
    /// `a[b]`: traps `index out of bounds`.
    VecGet,
    VecClone,
    /// `*a = b` through a `&mut String`/`&mut Vec<T>` borrow: `a`'s object
    /// takes `b`'s contents (`b` is moved, the checker forbids its reuse).
    /// Both must be of the same kind. Additive in IR v1.4.
    Replace,
    /// IR v1.6 (RFC-UI-LANG-1 P3): a new empty map; `a` is its
    /// `heap::MapKind` code.
    MapNew,
    MapLen,
    /// A new vector of the keys of map `a`, in key order.
    MapKeys,
    /// A new vector of the values of map `a`, in key order.
    MapVals,
    /// The values of map `a`, in key order, take those of vector `b`.
    MapSetVals,
    /// A shallow copy of map `a` (values copied as words).
    MapClone,
    /// IR v1.7 (RFC-UI-LANG-1 P4): byte index of the first occurrence of
    /// string `b` in string `a`, or `u64::MAX` when absent.
    StrFind,
    /// Whether string `a` starts with string `b`.
    StrStartsWith,
    /// Whether string `a` ends with string `b`.
    StrEndsWith,
    /// A new vector of new strings: `a` split on the non-empty separator
    /// `b` (an empty one is a fault; the lowering traps before).
    StrSplit,
    /// A new string `a[b..]` (`b` a char boundary within `a`, else a fault;
    /// the lowering traps before).
    StrTail,
    /// `a.truncate(b)` in place (`b` a char boundary within `a`, else a
    /// fault).
    StrTruncate,
    /// A new vector of the `char`s of `a` (code points as words).
    StrChars,
    /// Whether `b` is a char boundary of `a` (`b == a.len()` included).
    StrIsCharBoundary,
    /// A new string: the strings of vector `a` joined by string `b`.
    StrJoin,
    /// IR v1.8 (RFC-UI-LANG-2, STD-21): a new empty arena with the next
    /// identifier of the run (panic `too many arenas in this run`). The
    /// `Map*` operations and instructions also apply to an arena, keyed by
    /// handles: `MapLen`, `MapKeys` (live handles in slot order), `MapVals`,
    /// `MapSetVals`, `MapClone` (a new identifier), `MapGet`, `MapRemove`,
    /// and `MapInsert` (write back into a live slot only).
    ArenaNew,
    /// `a.insert(b)`, `b` the element's raw word: the packed handle (panic
    /// `arena is full`).
    ArenaInsert,
}

impl HeapOp {
    /// Every operation, in declaration order (`op as u32` is its index).
    pub const ALL: [HeapOp; 28] = [
        HeapOp::StrNew,
        HeapOp::StrClone,
        HeapOp::StrPush,
        HeapOp::StrLen,
        HeapOp::StrEq,
        HeapOp::VecNew,
        HeapOp::VecPush,
        HeapOp::VecLen,
        HeapOp::VecGet,
        HeapOp::VecClone,
        HeapOp::Replace,
        HeapOp::MapNew,
        HeapOp::MapLen,
        HeapOp::MapKeys,
        HeapOp::MapVals,
        HeapOp::MapSetVals,
        HeapOp::MapClone,
        HeapOp::StrFind,
        HeapOp::StrStartsWith,
        HeapOp::StrEndsWith,
        HeapOp::StrSplit,
        HeapOp::StrTail,
        HeapOp::StrTruncate,
        HeapOp::StrChars,
        HeapOp::StrIsCharBoundary,
        HeapOp::StrJoin,
        HeapOp::ArenaNew,
        HeapOp::ArenaInsert,
    ];
}

/// Output stream of a [`Inst::Print`] (IR v1.2, `io::print`/`println`/`eprintln`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stream {
    Stdout,
    Stderr,
    /// IR v1.4: the note of the next [`Term::Trap`] (RFC-CONTRACTS-1
    /// amendment 1, the values of a violated contract): appended to the
    /// trap's message, never printed.
    Note,
}

/// What a [`Inst::Print`] writes: a literal text, or a scalar formatted like
/// its `to_string` (Rust `Display`: the same text as the `rustc` backend).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PrintArg {
    Text(String),
    Value {
        ty: Ty,
        src: Local,
    },
    /// IR v1.3: the contents of a heap string.
    Obj(Local),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Inst {
    /// IR v1.2: append to the run's output buffer (effect `io`). Never
    /// allowed in a real-time entry (the hot-swap engine refuses it).
    Print {
        stream: Stream,
        arg: PrintArg,
    },
    /// IR v1.3: `dst = heap.op(a, b)` (`a`/`b` absent = 0). `dst`'s type is
    /// the result's (a handle, a length, a bool or an element word).
    Heap {
        dst: Option<Local>,
        op: HeapOp,
        a: Option<Local>,
        b: Option<Local>,
    },
    /// IR v1.3: a new heap string holding `text` (a literal).
    HeapText {
        dst: Local,
        text: String,
    },
    /// IR v1.3: a new heap string holding the `Display` text of `src`.
    HeapOfValue {
        dst: Local,
        ty: Ty,
        src: Local,
    },
    /// IR v1.3: `h[i] = x` (traps `index out of bounds`).
    VecSet {
        h: Local,
        i: Local,
        x: Local,
    },
    /// IR v1.6: `value` (raw word, 0 when absent) of key `key` in map `h`,
    /// and whether it is `present`.
    MapGet {
        h: Local,
        key: Local,
        present: Local,
        value: Local,
    },
    /// IR v1.6: insert or replace `key -> val` in map `h`; the previous
    /// value, if any (`present`, `old`).
    MapInsert {
        h: Local,
        key: Local,
        val: Local,
        present: Local,
        old: Local,
    },
    /// IR v1.6: remove `key` from map `h`; the removed value, if any.
    MapRemove {
        h: Local,
        key: Local,
        present: Local,
        old: Local,
    },
    /// IR v1.3: `h.pop()`: `present` and the raw word (0 when absent).
    VecPop {
        present: Local,
        value: Local,
        h: Local,
    },
    Const {
        dst: Local,
        value: Const,
    },
    Copy {
        dst: Local,
        src: Local,
    },
    /// `ty` is the operand type; comparisons produce `Bool`.
    Bin {
        dst: Local,
        op: BinOp,
        ty: Ty,
        lhs: Local,
        rhs: Local,
    },
    Un {
        dst: Local,
        op: UnOp,
        ty: Ty,
        src: Local,
    },
    /// `as` cast with Rust semantics: int↔int wraps/extends, float→int
    /// saturates (NaN → 0), int→float rounds to nearest, bool→int is 0/1.
    Cast {
        dst: Local,
        from: Ty,
        to: Ty,
        src: Local,
    },
    /// Call; a trap inside the callee propagates.
    Call {
        dst: Option<Local>,
        func: FuncId,
        args: Vec<Local>,
    },
    /// Address of stack slot `slot` of the current frame (IR v1.1).
    StackAddr {
        dst: Local,
        slot: u32,
    },
    /// Read a scalar of type `ty` at `addr + offset`. The lowering only emits
    /// accesses that stay inside the object `addr` points into.
    Load {
        dst: Local,
        ty: Ty,
        addr: Local,
        offset: u32,
    },
    /// Write a scalar of type `ty` at `addr + offset` (same discipline).
    Store {
        ty: Ty,
        addr: Local,
        offset: u32,
        src: Local,
    },
    /// `dst = base + index * scale`; `index` is a `U64` slot already checked
    /// against the bounds of the array by the lowering.
    PtrAdd {
        dst: Local,
        base: Local,
        index: Local,
        scale: u32,
    },
}

impl Inst {
    /// Whether this instruction prints or touches the run heap (allocation
    /// or heap object access). Real-time modules refuse every such
    /// instruction (INV-05). Exhaustive on purpose: a new instruction must
    /// say which side it is on.
    pub fn touches_heap_or_io(&self) -> bool {
        match self {
            Inst::Print { .. }
            | Inst::Heap { .. }
            | Inst::HeapText { .. }
            | Inst::HeapOfValue { .. }
            | Inst::VecSet { .. }
            | Inst::VecPop { .. }
            | Inst::MapGet { .. }
            | Inst::MapInsert { .. }
            | Inst::MapRemove { .. } => true,
            Inst::Const { .. }
            | Inst::Copy { .. }
            | Inst::Bin { .. }
            | Inst::Un { .. }
            | Inst::Cast { .. }
            | Inst::Call { .. }
            | Inst::StackAddr { .. }
            | Inst::Load { .. }
            | Inst::Store { .. }
            | Inst::PtrAdd { .. } => false,
        }
    }
}

/// A stack slot of a function frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StackSlot {
    pub size: u32,
    pub align: u32,
}

/// Host-owned state of a DSP block (RFC-E11-DSP-BLOCK): its canonical schema
/// (HR-1.1, computed by `ravum_lang_sema::reload::schema`) and its layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateInfo {
    pub text: String,
    pub digest: String,
    pub size: u32,
    pub align: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Term {
    Jump(BlockId),
    Branch {
        cond: Local,
        then_to: BlockId,
        else_to: BlockId,
    },
    Return(Option<Local>),
    /// Controlled stop with a message (`panic` or a language trap).
    Trap(String),
    /// IR v1.5: out-of-bounds stop of a fixed array (EXPR-7.6), with the
    /// spec message `index out of bounds: the len is {len} but the index is
    /// {index}` built from two `u64` locals at stop time. It prints nothing
    /// and allocates nothing, so real-time modules may use it (INV-05).
    TrapBounds {
        len: Local,
        index: Local,
    },
    /// Never reached when the program was accepted by the checker.
    Unreachable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Block {
    pub insts: Vec<Inst>,
    pub term: Term,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Function {
    pub name: String,
    pub params: u32,
    pub ret: Ty,
    pub locals: Vec<Ty>,
    pub blocks: Vec<Block>,
    #[serde(default)]
    pub stack: Vec<StackSlot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Module {
    pub version: String,
    pub functions: Vec<Function>,
    /// `struct State` of the root module, when there is one.
    #[serde(default)]
    pub state: Option<StateInfo>,
    /// Schema of the parameter `T` of `fn migrate(old: T) -> State`
    /// (HR-1.7), when the module declares one.
    #[serde(default)]
    pub migrate_from: Option<StateInfo>,
}

impl Module {
    pub fn func_id(&self, name: &str) -> Option<FuncId> {
        self.functions
            .iter()
            .position(|f| f.name == name)
            .map(|i| i as FuncId)
    }

    /// Serialize to the canonical JSON form.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("IR serialization cannot fail")
    }

    /// Parse JSON, refusing an unknown version (fail-closed).
    pub fn from_json(text: &str) -> Result<Module, String> {
        let m: Module = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if m.version != IR_VERSION {
            return Err(format!(
                "unsupported IR version `{}` (expected `{IR_VERSION}`)",
                m.version
            ));
        }
        m.validate()?;
        Ok(m)
    }

    /// Structural validation: every index in range, every block terminated.
    pub fn validate(&self) -> Result<(), String> {
        for f in &self.functions {
            let nl = f.locals.len() as u32;
            let nb = f.blocks.len() as u32;
            if f.params > nl || nb == 0 {
                return Err(format!("{}: bad params or no block", f.name));
            }
            let l = |x: Local| {
                if x < nl {
                    Ok(())
                } else {
                    Err(format!("{}: local {x} out of range", f.name))
                }
            };
            let b = |x: BlockId| {
                if x < nb {
                    Ok(())
                } else {
                    Err(format!("{}: block {x} out of range", f.name))
                }
            };
            for blk in &f.blocks {
                for i in &blk.insts {
                    match i {
                        Inst::Const { dst, .. } => l(*dst)?,
                        Inst::Heap { dst, a, b, .. } => {
                            for x in [dst, a, b].into_iter().flatten() {
                                l(*x)?;
                            }
                        }
                        Inst::HeapText { dst, .. } => l(*dst)?,
                        Inst::HeapOfValue { dst, src, ty } => {
                            l(*dst)?;
                            l(*src)?;
                            if !(ty.is_int() || ty.is_float() || *ty == Ty::Bool) {
                                return Err(format!("{}: to_string of {ty:?}", f.name));
                            }
                        }
                        Inst::VecSet { h, i, x } => {
                            l(*h)?;
                            l(*i)?;
                            l(*x)?
                        }
                        Inst::VecPop { present, value, h } => {
                            l(*present)?;
                            l(*value)?;
                            l(*h)?
                        }
                        Inst::MapGet {
                            h,
                            key,
                            present,
                            value: out,
                        }
                        | Inst::MapRemove {
                            h,
                            key,
                            present,
                            old: out,
                        } => {
                            for x in [*h, *key, *present, *out] {
                                l(x)?;
                            }
                            if f.locals[*h as usize] != Ty::Obj
                                || f.locals[*present as usize] != Ty::Bool
                            {
                                return Err(format!(
                                    "{}: map operation on mistyped locals",
                                    f.name
                                ));
                            }
                        }
                        Inst::MapInsert {
                            h,
                            key,
                            val,
                            present,
                            old,
                        } => {
                            for x in [*h, *key, *val, *present, *old] {
                                l(x)?;
                            }
                            if f.locals[*h as usize] != Ty::Obj
                                || f.locals[*present as usize] != Ty::Bool
                            {
                                return Err(format!(
                                    "{}: map operation on mistyped locals",
                                    f.name
                                ));
                            }
                        }
                        Inst::Print { arg, .. } => {
                            if let PrintArg::Obj(src) = arg {
                                l(*src)?;
                            }
                            if let PrintArg::Value { ty, src } = arg {
                                l(*src)?;
                                if !(ty.is_int() || ty.is_float() || *ty == Ty::Bool) {
                                    return Err(format!("{}: print of {ty:?}", f.name));
                                }
                            }
                        }
                        Inst::Copy { dst, src } | Inst::Cast { dst, src, .. } => {
                            l(*dst)?;
                            l(*src)?
                        }
                        Inst::Un { dst, src, op, ty } => {
                            l(*dst)?;
                            l(*src)?;
                            if !un_op_accepts(*op, *ty) {
                                return Err(format!("{}: {op:?} on {ty:?}", f.name));
                            }
                        }
                        Inst::Bin {
                            dst,
                            lhs,
                            rhs,
                            op,
                            ty,
                        } => {
                            if !bin_op_accepts(*op, *ty) {
                                return Err(format!("{}: {op:?} on {ty:?}", f.name));
                            }
                            l(*dst)?;
                            l(*lhs)?;
                            l(*rhs)?
                        }
                        Inst::StackAddr { dst, slot } => {
                            l(*dst)?;
                            if *slot as usize >= f.stack.len() {
                                return Err(format!("{}: stack slot {slot} out of range", f.name));
                            }
                        }
                        Inst::Load { dst, addr, .. } => {
                            l(*dst)?;
                            l(*addr)?
                        }
                        Inst::Store { addr, src, .. } => {
                            l(*addr)?;
                            l(*src)?
                        }
                        Inst::PtrAdd {
                            dst, base, index, ..
                        } => {
                            l(*dst)?;
                            l(*base)?;
                            l(*index)?
                        }
                        Inst::Call { dst, func, args } => {
                            if let Some(d) = dst {
                                l(*d)?
                            }
                            for a in args {
                                l(*a)?
                            }
                            let callee = self
                                .functions
                                .get(*func as usize)
                                .ok_or_else(|| format!("{}: func {func} out of range", f.name))?;
                            if callee.params as usize != args.len() {
                                return Err(format!(
                                    "{}: arity mismatch calling {}",
                                    f.name, callee.name
                                ));
                            }
                        }
                    }
                }
                match &blk.term {
                    Term::Jump(t) => b(*t)?,
                    Term::Branch {
                        cond,
                        then_to,
                        else_to,
                    } => {
                        l(*cond)?;
                        b(*then_to)?;
                        b(*else_to)?
                    }
                    Term::Return(Some(x)) => l(*x)?,
                    Term::TrapBounds { len, index } => {
                        for x in [*len, *index] {
                            l(x)?;
                            if f.locals[x as usize] != Ty::U64 {
                                return Err(format!(
                                    "{}: bounds trap operand {x} is not u64",
                                    f.name
                                ));
                            }
                        }
                    }
                    // Exhaustive on purpose: a new terminator must state
                    // what it validates (fail-closed).
                    Term::Return(None) | Term::Trap(_) | Term::Unreachable => {}
                }
            }
        }
        Ok(())
    }
}

/// Operand types an IR v1.2 unary operation accepts (fail-closed: a backend
/// never sees, say, `Sqrt` on an integer).
pub fn un_op_accepts(op: UnOp, ty: Ty) -> bool {
    match op {
        UnOp::Neg => ty.is_int() || ty.is_float(),
        UnOp::Not => ty.is_int() || ty == Ty::Bool,
        UnOp::Abs => (ty.is_int() && ty.is_signed()) || ty == Ty::F64,
        UnOp::WrappingNeg | UnOp::SaturatingNeg => ty.is_int() && ty.is_signed(),
        UnOp::Sqrt | UnOp::Floor | UnOp::Ceil | UnOp::Round => ty == Ty::F64,
    }
}

/// Operand types the IR v1.2 integer-only binary operations accept; the
/// v1/v1.1 operations are unchanged (checked by the lowering).
pub fn bin_op_accepts(op: BinOp, ty: Ty) -> bool {
    use BinOp::*;
    match op {
        WrappingAdd | WrappingSub | WrappingMul | SaturatingAdd | SaturatingSub | SaturatingMul
        | OverflowsAdd | OverflowsSub | OverflowsMul => ty.is_int(),
        MathMin | MathMax => ty == Ty::F64,
        _ => true,
    }
}

#[cfg(test)]
mod heap_op_tests {
    use super::HeapOp;

    /// `HeapOp::ALL` is complete and in declaration order: the native
    /// backend decodes `op as u32` by indexing it (`rv_heap`), below 64.
    #[test]
    fn all_lists_every_heap_op_in_order() {
        for (i, op) in HeapOp::ALL.iter().enumerate() {
            assert_eq!(*op as usize, i, "{op:?}");
        }
        assert_eq!(HeapOp::ArenaInsert as usize + 1, HeapOp::ALL.len());
        assert!(HeapOp::ALL.len() < 64);
    }
}
