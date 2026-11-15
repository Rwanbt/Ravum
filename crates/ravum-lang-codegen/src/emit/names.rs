//! Identifier mangling, escaping and type-name spelling.
//!
//! Extracted from `emit` by E4 (P-6), no behaviour change.

use super::*;

/// Rust 2015/2018/2021 keywords plus reserved words. Ravum identifiers are
/// `[A-Za-z_][A-Za-z0-9_]*` and exclude Ravum keywords, so most of these can
/// only appear through names the Ravum keyword list does not contain (`type`,
/// `ref`, `dyn`, `static`, `box`, ...).
pub(super) const RUST_KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate",
    "do", "dyn", "else", "enum", "extern", "false", "final", "fn", "for", "if", "impl", "in",
    "let", "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref",
    "return", "self", "Self", "static", "struct", "super", "trait", "true", "try", "type",
    "typeof", "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
];

/// Mangle a Ravum identifier into an always-legal Rust identifier.
///
/// `self`, `Self`, `super`, `crate` and `_` cannot be raw identifiers, so they
/// are renamed deterministically; other keywords use `r#`.
pub(super) fn mangle_ident(name: &str) -> String {
    if matches!(name, "self" | "Self" | "super" | "crate" | "_" | "") {
        let cleaned = sanitize_segment(name);
        return if cleaned.is_empty() {
            "rv_unnamed".to_string()
        } else {
            format!("rv_kw_{cleaned}")
        };
    }
    if RUST_KEYWORDS.contains(&name) {
        format!("r#{name}")
    } else {
        name.to_string()
    }
}

/// Keep only characters legal in a Rust identifier; used for module segments
/// taken from fact paths.
pub(super) fn sanitize_segment(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for (position, ch) in name.chars().enumerate() {
        let ok = if position == 0 {
            ch.is_ascii_alphabetic() || ch == '_'
        } else {
            ch.is_ascii_alphanumeric() || ch == '_'
        };
        out.push(if ok { ch } else { '_' });
    }
    out
}

pub(super) fn indent_text(level: usize) -> String {
    INDENT.repeat(level)
}

// ---------------------------------------------------------------------------
// Literals and escaping.
// ---------------------------------------------------------------------------

pub(super) fn escape_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            other if (other as u32) < 0x20 || other as u32 == 0x7f => {
                out.push_str(&format!("\\u{{{:x}}}", other as u32));
            }
            other => out.push(other),
        }
    }
    out
}

pub(super) fn string_literal(text: &str) -> String {
    format!("\"{}\"", escape_string(text))
}

pub(super) fn escape_char(ch: char) -> String {
    match ch {
        '\\' => "\\\\".to_string(),
        '\'' => "\\'".to_string(),
        '\n' => "\\n".to_string(),
        '\r' => "\\r".to_string(),
        '\t' => "\\t".to_string(),
        '\0' => "\\0".to_string(),
        other if (other as u32) < 0x20 || other as u32 == 0x7f => {
            format!("\\u{{{:x}}}", other as u32)
        }
        other => other.to_string(),
    }
}

pub(super) fn char_literal(ch: char) -> String {
    format!("'{}'", escape_char(ch))
}

pub(super) fn int_type_name(ty: &Type) -> Option<&'static str> {
    Some(match ty {
        Type::I8 => "i8",
        Type::I16 => "i16",
        Type::I32 => "i32",
        Type::I64 => "i64",
        Type::I128 => "i128",
        Type::U8 => "u8",
        Type::U16 => "u16",
        Type::U32 => "u32",
        Type::U64 => "u64",
        Type::U128 => "u128",
        Type::Isize => "isize",
        Type::Usize => "usize",
        _ => return None,
    })
}

pub(super) fn builtin_type_name(name: &str) -> Option<&'static str> {
    Some(match name {
        "i8" => "i8",
        "i16" => "i16",
        "i32" => "i32",
        "i64" => "i64",
        "i128" => "i128",
        "u8" => "u8",
        "u16" => "u16",
        "u32" => "u32",
        "u64" => "u64",
        "u128" => "u128",
        "isize" => "isize",
        "usize" => "usize",
        "f32" => "f32",
        "f64" => "f64",
        "bool" => "bool",
        "char" => "char",
        "String" => "String",
        "str" => "str",
        "Vec" => "Vec",
        "Option" => "Option",
        "Result" => "Result",
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Builtin stdlib mapping.
// ---------------------------------------------------------------------------
