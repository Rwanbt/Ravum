//! Builtin free functions, constants and method lowering tables.
//!
//! Extracted from `emit` by E4 (P-6), no behaviour change.

use super::*;

/// A free function of the Ravum standard library and how it is emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FreeBuiltin {
    Println,
    Print,
    Eprintln,
    Eprint,
    ReadLine,
    Args,
    EnvVar,
    FsReadToString,
    FsWriteString,
    FsExists,
    Exit,
    Panic,
    Assert,
    AssertEq,
    Drop,
    StringFrom,
    ErrorNew,
    /// Emit `path(args)` verbatim: associated constructors of prelude types
    /// (`Vec::new`, `Vec::with_capacity`, `String::new`).
    RustPath(&'static str),
    ParseI64,
    ParseF64,
    MathUnary(&'static str),
    MathBinary(&'static str),
    /// `Option::Some` / `Result::Ok` ... (variant constructors that arrive as
    /// builtin free functions because `Option`/`Result` are prelude types).
    VariantCtor,
}

pub(super) fn builtin_free(path: &str) -> Option<FreeBuiltin> {
    let path = path.strip_prefix("std::").unwrap_or(path);
    Some(match path {
        "io::println" | "println" => FreeBuiltin::Println,
        "io::print" | "print" => FreeBuiltin::Print,
        "io::eprintln" | "eprintln" => FreeBuiltin::Eprintln,
        "io::eprint" | "eprint" => FreeBuiltin::Eprint,
        "io::read_line" | "read_line" => FreeBuiltin::ReadLine,
        "env::args" | "args" => FreeBuiltin::Args,
        "env::var" | "env_var" => FreeBuiltin::EnvVar,
        "fs::read_to_string" | "read_to_string" => FreeBuiltin::FsReadToString,
        "fs::write_string" | "write_string" => FreeBuiltin::FsWriteString,
        "fs::exists" | "exists" => FreeBuiltin::FsExists,
        "process::exit" | "exit" => FreeBuiltin::Exit,
        "panic::panic" | "panic" => FreeBuiltin::Panic,
        "panic::assert" | "assert" => FreeBuiltin::Assert,
        "panic::assert_eq" | "assert_eq" => FreeBuiltin::AssertEq,
        "mem::drop" | "drop" => FreeBuiltin::Drop,
        "string::from" | "String::from" | "str::from" => FreeBuiltin::StringFrom,
        "Error::new" | "error::new" => FreeBuiltin::ErrorNew,
        "Vec::new" | "vec::new" => FreeBuiltin::RustPath("Vec::new"),
        "Vec::with_capacity" | "vec::with_capacity" => FreeBuiltin::RustPath("Vec::with_capacity"),
        "String::new" | "string::new" => FreeBuiltin::RustPath("String::new"),
        "string::parse_i64" | "parse_i64" | "str::parse_i64" => FreeBuiltin::ParseI64,
        "string::parse_f64" | "parse_f64" | "str::parse_f64" => FreeBuiltin::ParseF64,
        "math::abs" | "abs" => FreeBuiltin::MathUnary("abs"),
        "math::sqrt" | "sqrt" => FreeBuiltin::MathUnary("sqrt"),
        "math::floor" | "floor" => FreeBuiltin::MathUnary("floor"),
        "math::ceil" | "ceil" => FreeBuiltin::MathUnary("ceil"),
        "math::round" | "round" => FreeBuiltin::MathUnary("round"),
        "math::sin" | "sin" => FreeBuiltin::MathUnary("sin"),
        "math::cos" | "cos" => FreeBuiltin::MathUnary("cos"),
        "math::tan" | "tan" => FreeBuiltin::MathUnary("tan"),
        "math::exp" | "exp" => FreeBuiltin::MathUnary("exp"),
        "math::ln" | "ln" => FreeBuiltin::MathUnary("ln"),
        "math::log10" | "log10" => FreeBuiltin::MathUnary("log10"),
        "math::pow" | "pow" => FreeBuiltin::MathBinary("powf"),
        "math::min" | "min" => FreeBuiltin::MathBinary("min"),
        "math::max" | "max" => FreeBuiltin::MathBinary("max"),
        _ => {
            if is_option_or_result_ctor(path) {
                FreeBuiltin::VariantCtor
            } else {
                return None;
            }
        }
    })
}

/// `Option::Some`, `Result::Ok`, `Some`, `Ok` ... (unqualified accepted).
pub(super) fn is_option_or_result_ctor(path: &str) -> bool {
    let segments: Vec<&str> = path.split("::").collect();
    let Some(last) = segments.last() else {
        return false;
    };
    let accepted = ["Some", "None", "Ok", "Err"];
    if !accepted.contains(last) {
        return false;
    }
    segments.len() == 1
        || matches!(
            segments.get(segments.len() - 2).copied(),
            Some("Option") | Some("Result")
        )
}

pub(super) fn builtin_const(path: &str) -> Option<String> {
    let path = path.strip_prefix("std::").unwrap_or(path);
    let direct = match path {
        "math::PI" | "math::pi" => Some("std::f64::consts::PI"),
        "math::E" | "math::e" => Some("std::f64::consts::E"),
        "math::TAU" | "math::tau" => Some("std::f64::consts::TAU"),
        // `Option::None` is a constant, not a constructor call: the canonical
        // representation decided by the checker.
        "Option::None" | "None" => Some("None"),
        _ => None,
    };
    if let Some(found) = direct {
        return Some(found.to_string());
    }
    // `<primitive>::MIN` / `MAX` / `EPSILON` ... are already valid Rust paths.
    let mut segments = path.split("::");
    let owner = segments.next()?;
    let member = segments.next()?;
    if segments.next().is_some() {
        return None;
    }
    let owner_ok = builtin_type_name(owner).is_some();
    let member_ok = matches!(
        member,
        "MIN" | "MAX" | "EPSILON" | "NAN" | "INFINITY" | "NEG_INFINITY" | "DIGITS" | "BITS"
    );
    if owner_ok && member_ok {
        Some(format!("{owner}::{member}"))
    } else {
        None
    }
}

/// Builtin methods that need more than a direct `.name(args)` call.
pub(super) fn builtin_method_special(
    owner: &str,
    method: &str,
    receiver: &str,
    args: &str,
) -> Option<String> {
    let rendered = match (owner, method) {
        (_, "to_string") => format!("format!(\"{{}}\", {receiver})"),
        // `str::as_str` (STD-11.4) is the identity on a text reference: the
        // receiver is already a `&str`, and Rust's `str` has no such method.
        ("str", "as_str") => format!("({receiver})"),
        ("String", "trim") | ("str", "trim") => format!("{receiver}.trim().to_string()"),
        ("String", "lines") | ("str", "lines") => {
            format!("{receiver}.lines().map(|rv_line| rv_line.to_string()).collect::<Vec<String>>()")
        }
        ("String", "parse_i64") | ("str", "parse_i64") => format!(
            "{receiver}.trim().parse::<i64>().map_err(|rv_error| crate::Error::new(&rv_error.to_string()))"
        ),
        ("String", "parse_f64") | ("str", "parse_f64") => format!(
            "{receiver}.trim().parse::<f64>().map_err(|rv_error| crate::Error::new(&rv_error.to_string()))"
        ),
        _ => return None,
    };
    if args.is_empty() {
        Some(rendered)
    } else {
        // These special forms take no argument in Core 1.0.
        Some(rendered)
    }
}

/// Rust method name that exists on a builtin owner, exactly as listed by the
/// Ravum Core 1.0 stdlib. Anything else is reported as an explicit
/// [`CodegenError`]: the generator never guesses a Rust method name.
pub(super) fn builtin_method_is_direct(owner: &str, method: &str) -> bool {
    const STRING: &[&str] = &[
        "len",
        "is_empty",
        "as_str",
        "push_str",
        "push",
        "clone",
        "to_uppercase",
        "to_lowercase",
        "contains",
        "replace",
        "starts_with",
        "ends_with",
    ];
    const STR: &[&str] = &[
        "len",
        "is_empty",
        "to_uppercase",
        "to_lowercase",
        "contains",
        "replace",
        "starts_with",
        "ends_with",
    ];
    const VEC: &[&str] = &[
        "len", "is_empty", "push", "pop", "clear", "clone", "as_slice",
    ];
    const SLICE: &[&str] = &["len", "is_empty"];
    const OPTION: &[&str] = &["is_some", "is_none", "unwrap", "unwrap_or", "ok_or"];
    const RESULT: &[&str] = &["is_ok", "is_err", "unwrap", "unwrap_or", "ok", "err"];
    const FLOAT: &[&str] = &[
        "abs", "sqrt", "floor", "ceil", "round", "powf", "min", "max", "sin", "cos", "tan", "exp",
        "ln", "log10",
    ];
    const SIGNED: &[&str] = &[
        "abs",
        // RFC-RT-TRAPS-1 explicit forms (Rust methods exist on every integer
        // type; `checked_*` returns `Option`, which the type checker enforced).
        "wrapping_add",
        "wrapping_sub",
        "wrapping_mul",
        "wrapping_neg",
        "saturating_add",
        "saturating_sub",
        "saturating_mul",
        "saturating_neg",
        "checked_add",
        "checked_sub",
        "checked_mul",
        "checked_div",
        "checked_rem",
        "checked_neg",
    ];
    match owner {
        "String" => STRING.contains(&method),
        "str" => STR.contains(&method),
        "Vec" => VEC.contains(&method),
        "slice" | "Slice" | "[T]" => SLICE.contains(&method),
        "Option" => OPTION.contains(&method),
        "Result" => RESULT.contains(&method),
        "f32" | "f64" => FLOAT.contains(&method),
        "int" | "i8" | "i16" | "i32" | "i64" | "i128" | "isize" => SIGNED.contains(&method),
        _ => false,
    }
}

/// Split a canonical builtin method path into `(owner, method)`.
pub(super) fn split_builtin_method(path: &str) -> Option<(&str, &str)> {
    let path = path.strip_prefix("std::").unwrap_or(path);
    let segments: Vec<&str> = path.split("::").filter(|s| !s.is_empty()).collect();
    if segments.len() < 2 {
        return None;
    }
    let method = segments[segments.len() - 1];
    let owner = segments[segments.len() - 2];
    Some((owner, method))
}

/// Normalize `&String`, `&mut Vec<T>`, `Vec<T>` ... to the bare builtin owner.
pub(super) fn normalize_owner(owner: &str) -> String {
    let mut text = owner.trim();
    if let Some(rest) = text.strip_prefix('&') {
        text = rest.trim();
    }
    if let Some(rest) = text.strip_prefix("mut ") {
        text = rest.trim();
    }
    if let Some(position) = text.find('<') {
        text = &text[..position];
    }
    text.trim().to_string()
}
