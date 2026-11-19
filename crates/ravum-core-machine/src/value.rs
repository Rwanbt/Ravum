//! Values of the machine (§2) and the arithmetic of §5.

use crate::ast::{FloatTy, IntTy};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A typed integer. `u128` values above `i128::MAX` use [`Value::U128`].
    Int(IntTy, i128),
    U128(u128),
    /// An integer literal whose type is not yet fixed by its context
    /// (TYP-13.2): it takes the type of the first typed value it meets.
    IntLit(u128),
    Float(FloatTy, f64),
    FloatLit(f64),
    Bool(bool),
    Char(char),
    Str(String),
    Unit,
    Tuple(Vec<Value>),
    Array(Vec<Value>),
    Struct(String, Vec<(String, Value)>),
    Enum(String, String, Vec<Value>),
}

/// Why an evaluation stopped before producing a value.
#[derive(Debug, Clone, PartialEq)]
pub enum Stop {
    Panic(String),
    Fault(String),
    Diverge,
    Break,
    Continue,
    Return(Value),
}

pub type Eval<T> = Result<T, Stop>;

pub fn fault<T>(msg: impl Into<String>) -> Eval<T> {
    Err(Stop::Fault(msg.into()))
}

pub fn panic<T>(msg: impl Into<String>) -> Eval<T> {
    Err(Stop::Panic(msg.into()))
}

/// Inclusive range of a non-`u128` integer type, as `i128`.
pub fn bounds(ty: IntTy) -> (i128, i128) {
    let bits = ty.bits();
    if ty.signed() {
        if bits == 128 {
            (i128::MIN, i128::MAX)
        } else {
            (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
        }
    } else {
        (0, (1i128 << bits) - 1)
    }
}

/// A typed integer from an exact mathematical value, or `None` when it does
/// not fit the type.
pub fn make_int(ty: IntTy, v: i128) -> Option<Value> {
    if ty == IntTy::U128 {
        return (v >= 0).then_some(Value::U128(v as u128));
    }
    let (lo, hi) = bounds(ty);
    (lo..=hi).contains(&v).then_some(Value::Int(ty, v))
}

pub fn make_int_u128(ty: IntTy, v: u128) -> Option<Value> {
    if ty == IntTy::U128 {
        return Some(Value::U128(v));
    }
    i128::try_from(v).ok().and_then(|v| make_int(ty, v))
}

/// Two's complement wrap of an exact value to `ty` (casts, shifts).
pub fn wrap(ty: IntTy, bits_value: u128) -> Value {
    let bits = ty.bits();
    let masked = if bits == 128 {
        bits_value
    } else {
        bits_value & ((1u128 << bits) - 1)
    };
    if ty == IntTy::U128 {
        return Value::U128(masked);
    }
    if ty.signed() && bits < 128 && masked >> (bits - 1) & 1 == 1 {
        return Value::Int(ty, masked as i128 - (1i128 << bits));
    }
    Value::Int(ty, masked as i128)
}

/// The two's complement bit pattern of an integer value.
pub fn raw_bits(v: &Value) -> u128 {
    match v {
        Value::Int(_, n) => *n as u128,
        Value::U128(n) => *n,
        _ => 0,
    }
}

pub fn int_ty(v: &Value) -> Option<IntTy> {
    match v {
        Value::Int(t, _) => Some(*t),
        Value::U128(_) => Some(IntTy::U128),
        _ => None,
    }
}

/// Exact value of a non-`u128` integer, or of a `u128` that fits `i128`.
pub fn as_i128(v: &Value) -> Option<i128> {
    match v {
        Value::Int(_, n) => Some(*n),
        Value::U128(n) => i128::try_from(*n).ok(),
        _ => None,
    }
}

pub fn round_float(ty: FloatTy, v: f64) -> f64 {
    match ty {
        FloatTy::F64 => v,
        // Every f32 operation is computed in f64 and rounded once: for
        // `+ - * / sqrt` this equals the correctly rounded f32 result.
        FloatTy::F32 => v as f32 as f64,
    }
}

/// `+ - * / %` on two integers of the same type; traps per §5.
pub fn int_arith(op: &str, a: &Value, b: &Value) -> Eval<Value> {
    let ty = int_ty(a).expect("integer");
    if ty == IntTy::U128 {
        let (x, y) = (raw_bits(a), raw_bits(b));
        let r = match op {
            "+" => x.checked_add(y),
            "-" => x.checked_sub(y),
            "*" => x.checked_mul(y),
            "/" if y == 0 => return panic("division by zero"),
            "%" if y == 0 => return panic("remainder by zero"),
            "/" => Some(x / y),
            _ => Some(x % y),
        };
        return r.map(Value::U128).map_or_else(|| overflow(op), Ok);
    }
    let (x, y) = (as_i128(a).unwrap(), as_i128(b).unwrap());
    let exact = match op {
        "+" => x.checked_add(y),
        "-" => x.checked_sub(y),
        "*" => x.checked_mul(y),
        "/" if y == 0 => return panic("division by zero"),
        "%" if y == 0 => return panic("remainder by zero"),
        // Truncation toward zero; `MIN / -1` overflows (§5).
        "/" => x.checked_div(y),
        // Sign of the dividend; `MIN % -1` overflows like `MIN / -1`
        // (§5, decision D-19-2).
        _ if y == -1 && x == bounds(ty).0 && ty.signed() => None,
        _ => x.checked_rem(y),
    };
    match exact.and_then(|v| make_int(ty, v)) {
        Some(v) => Ok(v),
        None => overflow(op),
    }
}

fn overflow<T>(op: &str) -> Eval<T> {
    let verb = match op {
        "+" => "add",
        "-" => "subtract",
        "*" => "multiply",
        "/" => "divide",
        _ => "calculate the remainder",
    };
    panic(format!("attempt to {verb} with overflow"))
}

pub fn display(v: &Value) -> String {
    match v {
        Value::Int(_, n) => n.to_string(),
        Value::U128(n) | Value::IntLit(n) => n.to_string(),
        Value::Float(FloatTy::F32, f) => (*f as f32).to_string(),
        Value::Float(FloatTy::F64, f) | Value::FloatLit(f) => f.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Char(c) => c.to_string(),
        Value::Str(s) => s.clone(),
        Value::Unit => "()".into(),
        Value::Tuple(vs) => format!(
            "({})",
            vs.iter().map(display).collect::<Vec<_>>().join(", ")
        ),
        Value::Array(vs) => format!(
            "[{}]",
            vs.iter().map(display).collect::<Vec<_>>().join(", ")
        ),
        Value::Struct(n, fs) => format!(
            "{n} {{ {} }}",
            fs.iter()
                .map(|(f, v)| format!("{f}: {}", display(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Enum(_, var, vs) if vs.is_empty() => var.clone(),
        Value::Enum(_, var, vs) => format!(
            "{var}({})",
            vs.iter().map(display).collect::<Vec<_>>().join(", ")
        ),
    }
}

/// The type name of a value, for a comparable result (MACH-8.2).
pub fn type_name(v: &Value) -> String {
    match v {
        Value::Int(t, _) => t.name().into(),
        Value::U128(_) => "u128".into(),
        Value::IntLit(_) => "i32".into(),
        Value::Float(FloatTy::F32, _) => "f32".into(),
        Value::Float(FloatTy::F64, _) | Value::FloatLit(_) => "f64".into(),
        Value::Bool(_) => "bool".into(),
        Value::Char(_) => "char".into(),
        Value::Str(_) => "&str".into(),
        Value::Unit => "()".into(),
        Value::Tuple(_) => "tuple".into(),
        Value::Array(_) => "array".into(),
        Value::Struct(n, _) => n.clone(),
        Value::Enum(e, _, _) => e.clone(),
    }
}
