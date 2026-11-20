//! Builtin free functions, constants and method lowering tables.
//!
//! Extracted from `emit` by E4 (P-6), no behaviour change.

/// A free function of the Ravum standard library and how it is emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FreeBuiltin {
    Println,
    Print,
    Eprintln,
    ReadLine,
    Args,
    EnvVar,
    FsReadToString,
    FsWriteString,
    FsExists,
    /// `std::net::<name>`, emitted as `crate::rv_net_<name>(args)`
    /// (RFC-E11-NET-HTTP §3).
    Net(&'static str),
    /// `std::thread::<name>` / `std::sync::<name>`, emitted as
    /// `crate::rv_<module>_<name>(args)` (RFC-E11-CONCURRENCY §3).
    Conc(&'static str),
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
        "io::read_line" | "read_line" => FreeBuiltin::ReadLine,
        "env::args" | "args" => FreeBuiltin::Args,
        "env::var" | "env_var" => FreeBuiltin::EnvVar,
        "fs::read_to_string" | "read_to_string" => FreeBuiltin::FsReadToString,
        "fs::write_string" | "write_string" => FreeBuiltin::FsWriteString,
        "fs::exists" | "exists" => FreeBuiltin::FsExists,
        "net::listen" => FreeBuiltin::Net("listen"),
        "net::local_port" => FreeBuiltin::Net("local_port"),
        "net::accept" => FreeBuiltin::Net("accept"),
        "net::connect" => FreeBuiltin::Net("connect"),
        "net::read_line" => FreeBuiltin::Net("read_line"),
        "net::read_exact" => FreeBuiltin::Net("read_exact"),
        "net::write_str" => FreeBuiltin::Net("write_str"),
        "net::accept_async" => FreeBuiltin::Net("accept_async"),
        "net::connect_async" => FreeBuiltin::Net("connect_async"),
        "net::read_line_async" => FreeBuiltin::Net("read_line_async"),
        "net::write_str_async" => FreeBuiltin::Net("write_str_async"),
        "thread::spawn" => FreeBuiltin::Conc("thread_spawn"),
        "task::block_on" => FreeBuiltin::Conc("task_block_on"),
        "task::spawn" => FreeBuiltin::Conc("task_spawn"),
        "task::yield_now" => FreeBuiltin::Conc("task_yield_now"),
        "task::timeout" => FreeBuiltin::Conc("task_timeout"),
        "http::read_request" => FreeBuiltin::Conc("http_read_request"),
        "http::method" => FreeBuiltin::Conc("http_method"),
        "http::path" => FreeBuiltin::Conc("http_path"),
        "http::header" => FreeBuiltin::Conc("http_header"),
        "http::body" => FreeBuiltin::Conc("http_body"),
        "http::respond" => FreeBuiltin::Conc("http_respond"),
        "http::read_request_async" => FreeBuiltin::Conc("http_read_request_async"),
        "http::respond_async" => FreeBuiltin::Conc("http_respond_async"),
        "http::get" => FreeBuiltin::Conc("http_get"),
        "http::post" => FreeBuiltin::Conc("http_post"),
        "http::status" => FreeBuiltin::Conc("http_status"),
        "http::response_body" => FreeBuiltin::Conc("http_response_body"),
        "ffi::null" => FreeBuiltin::Conc("ffi_null"),
        "ffi::is_null" => FreeBuiltin::Conc("ffi_is_null"),
        "thread::join" => FreeBuiltin::Conc("thread_join"),
        "sync::channel" => FreeBuiltin::Conc("sync_channel"),
        "sync::sender" => FreeBuiltin::Conc("sync_sender"),
        "sync::clone_sender" => FreeBuiltin::Conc("sync_clone_sender"),
        "sync::receiver" => FreeBuiltin::Conc("sync_receiver"),
        "sync::send" => FreeBuiltin::Conc("sync_send"),
        "sync::recv" => FreeBuiltin::Conc("sync_recv"),
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
    // `split` yields at least one (possibly empty) segment.
    let last = &segments[segments.len() - 1];
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

/// The Rust spelling of a builtin constant. The resolver produces exactly one,
/// `Option::None` (resolve/paths.rs); `math::PI` and the primitive associated
/// constants are `E-RES-UNKNOWN` in Core 1.0, so any other path is a defect
/// upstream and the caller refuses it.
pub(super) fn builtin_const(path: &str) -> Option<String> {
    match path.strip_prefix("std::").unwrap_or(path) {
        "Option::None" | "None" => Some("None".to_string()),
        _ => None,
    }
}

/// Builtin methods that need more than a direct `.name(args)` call.
pub(super) fn builtin_method_special(owner: &str, method: &str, receiver: &str) -> Option<String> {
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
    // These special forms take no argument in Core 1.0 (typeck checks the
    // arity), so the rendered arguments are never needed.
    Some(rendered)
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
