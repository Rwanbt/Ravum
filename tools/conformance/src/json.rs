//! Minimal JSON reader and writer.
//!
//! The conformance harness deliberately has **no external dependency**: the
//! oracle must be buildable in a minimal environment and must not inherit the
//! dependency graph of the toolchain it judges. This module implements the
//! subset of RFC 8259 needed by `case.json` and by the report writer, with a
//! bounded recursion depth so that a hostile input cannot overflow the stack.
//!
//! Objects keep their insertion order, which makes the generated report
//! byte-deterministic for a given run.

use std::fmt::Write as _;

/// Maximum nesting depth accepted while parsing. `case.json` and diagnostics
/// reports are shallow; the limit only matters for hostile input.
const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    /// Raw JSON number text (kept verbatim so no precision is silently lost).
    Number(String),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonError {
    pub message: String,
    pub offset: usize,
}

impl std::fmt::Display for JsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (at byte {})", self.message, self.offset)
    }
}

impl std::error::Error for JsonError {}

impl Json {
    // -- constructors -----------------------------------------------------

    pub fn int(value: i64) -> Json {
        Json::Number(value.to_string())
    }

    pub fn uint(value: usize) -> Json {
        Json::Number(value.to_string())
    }

    pub fn string(value: impl Into<String>) -> Json {
        Json::String(value.into())
    }

    pub fn bool(value: bool) -> Json {
        Json::Bool(value)
    }

    /// Builds an object from ordered `(key, value)` pairs.
    pub fn object(entries: Vec<(&str, Json)>) -> Json {
        Json::Object(
            entries
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        )
    }

    // -- accessors --------------------------------------------------------

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&[(String, Json)]> {
        match self {
            Json::Object(entries) => Some(entries),
            _ => None,
        }
    }

    /// Returns the first value for `key` (duplicate keys are rejected by the
    /// caller when strictness matters).
    pub fn get(&self, key: &str) -> Option<&Json> {
        self.as_object()?
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }

    /// Integer accessor restricted to values that fit in `i64` and are written
    /// without a fraction or exponent.
    pub fn as_i64(&self) -> Option<i64> {
        let Json::Number(raw) = self else {
            return None;
        };
        if raw.contains('.') || raw.contains('e') || raw.contains('E') {
            return None;
        }
        raw.parse::<i64>().ok()
    }

    // -- parsing ----------------------------------------------------------

    pub fn parse(src: &str) -> Result<Json, JsonError> {
        let mut parser = Parser {
            src,
            bytes: src.as_bytes(),
            pos: 0,
        };
        let value = parser.value(0)?;
        parser.skip_whitespace();
        if parser.pos != parser.bytes.len() {
            return Err(parser.error("trailing characters after JSON value"));
        }
        Ok(value)
    }

    // -- writing ----------------------------------------------------------

    /// Two-space indented representation, used for the report file.
    pub fn write_pretty(&self) -> String {
        let mut out = String::new();
        self.write_into(&mut out, Some(2), 0);
        out.push('\n');
        out
    }

    fn write_into(&self, out: &mut String, indent: Option<usize>, level: usize) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(true) => out.push_str("true"),
            Json::Bool(false) => out.push_str("false"),
            Json::Number(raw) => out.push_str(raw),
            Json::String(s) => write_json_string(s, out),
            Json::Array(items) => {
                if items.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    newline_indent(out, indent, level + 1);
                    item.write_into(out, indent, level + 1);
                }
                newline_indent(out, indent, level);
                out.push(']');
            }
            Json::Object(entries) => {
                if entries.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push('{');
                for (index, (key, value)) in entries.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    newline_indent(out, indent, level + 1);
                    write_json_string(key, out);
                    out.push(':');
                    if indent.is_some() {
                        out.push(' ');
                    }
                    value.write_into(out, indent, level + 1);
                }
                newline_indent(out, indent, level);
                out.push('}');
            }
        }
    }
}

fn newline_indent(out: &mut String, indent: Option<usize>, level: usize) {
    if let Some(width) = indent {
        out.push('\n');
        for _ in 0..width * level {
            out.push(' ');
        }
    }
}

fn write_json_string(value: &str, out: &mut String) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

struct Parser<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn error(&self, message: impl Into<String>) -> JsonError {
        JsonError {
            message: message.into(),
            offset: self.pos,
        }
    }

    fn skip_whitespace(&mut self) {
        while let Some(&byte) = self.bytes.get(self.pos) {
            match byte {
                b' ' | b'\t' | b'\n' | b'\r' => self.pos += 1,
                _ => break,
            }
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, JsonError> {
        if depth > MAX_DEPTH {
            return Err(self.error("JSON nesting is too deep"));
        }
        self.skip_whitespace();
        let byte = *self
            .bytes
            .get(self.pos)
            .ok_or_else(|| self.error("unexpected end of JSON input"))?;
        match byte {
            b'n' => {
                self.expect_literal("null")?;
                Ok(Json::Null)
            }
            b't' => {
                self.expect_literal("true")?;
                Ok(Json::Bool(true))
            }
            b'f' => {
                self.expect_literal("false")?;
                Ok(Json::Bool(false))
            }
            b'"' => Ok(Json::String(self.string()?)),
            b'[' => self.array(depth),
            b'{' => self.object(depth),
            b'-' | b'0'..=b'9' => self.number(),
            _ => Err(self.error("unexpected character while parsing JSON")),
        }
    }

    fn expect_literal(&mut self, literal: &str) -> Result<(), JsonError> {
        if self.src[self.pos..].starts_with(literal) {
            self.pos += literal.len();
            Ok(())
        } else {
            Err(self.error(format!("expected JSON literal `{literal}`")))
        }
    }

    fn number(&mut self) -> Result<Json, JsonError> {
        let start = self.pos;
        if self.bytes.get(self.pos) == Some(&b'-') {
            self.pos += 1;
        }
        match self.bytes.get(self.pos) {
            Some(b'0') => {
                self.pos += 1;
                if matches!(self.bytes.get(self.pos), Some(b'0'..=b'9')) {
                    return Err(self.error("leading zero in JSON number"));
                }
            }
            Some(b'1'..=b'9') => {
                while matches!(self.bytes.get(self.pos), Some(b'0'..=b'9')) {
                    self.pos += 1;
                }
            }
            _ => return Err(self.error("expected digits in JSON number")),
        }
        if self.bytes.get(self.pos) == Some(&b'.') {
            self.pos += 1;
            if !matches!(self.bytes.get(self.pos), Some(b'0'..=b'9')) {
                return Err(self.error("expected digits after decimal point"));
            }
            while matches!(self.bytes.get(self.pos), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        if matches!(self.bytes.get(self.pos), Some(b'e') | Some(b'E')) {
            self.pos += 1;
            if matches!(self.bytes.get(self.pos), Some(b'+') | Some(b'-')) {
                self.pos += 1;
            }
            if !matches!(self.bytes.get(self.pos), Some(b'0'..=b'9')) {
                return Err(self.error("expected digits in JSON exponent"));
            }
            while matches!(self.bytes.get(self.pos), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        Ok(Json::Number(self.src[start..self.pos].to_string()))
    }

    fn string(&mut self) -> Result<String, JsonError> {
        debug_assert_eq!(self.bytes.get(self.pos), Some(&b'"'));
        self.pos += 1;
        let mut out = String::new();
        loop {
            let byte = *self
                .bytes
                .get(self.pos)
                .ok_or_else(|| self.error("unterminated JSON string"))?;
            match byte {
                b'"' => {
                    self.pos += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.pos += 1;
                    let escape = *self
                        .bytes
                        .get(self.pos)
                        .ok_or_else(|| self.error("unterminated JSON escape"))?;
                    self.pos += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{08}'),
                        b'f' => out.push('\u{0c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let first = self.hex4()?;
                            let code = if (0xD800..=0xDBFF).contains(&first) {
                                if self.bytes.get(self.pos) != Some(&b'\\')
                                    || self.bytes.get(self.pos + 1) != Some(&b'u')
                                {
                                    return Err(self.error(
                                        "high surrogate without a following low surrogate",
                                    ));
                                }
                                self.pos += 2;
                                let second = self.hex4()?;
                                if !(0xDC00..=0xDFFF).contains(&second) {
                                    return Err(self.error("invalid low surrogate"));
                                }
                                0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                            } else if (0xDC00..=0xDFFF).contains(&first) {
                                return Err(self.error("lone low surrogate"));
                            } else {
                                first
                            };
                            let ch = char::from_u32(code)
                                .ok_or_else(|| self.error("invalid Unicode scalar value"))?;
                            out.push(ch);
                        }
                        other => {
                            return Err(
                                self.error(format!("unknown JSON escape `\\{}`", other as char))
                            )
                        }
                    }
                }
                0x00..=0x1F => return Err(self.error("raw control character in JSON string")),
                _ => {
                    let ch = self.src[self.pos..]
                        .chars()
                        .next()
                        .ok_or_else(|| self.error("invalid UTF-8 in JSON string"))?;
                    self.pos += ch.len_utf8();
                    out.push(ch);
                }
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let mut value = 0u32;
        for _ in 0..4 {
            let byte = *self
                .bytes
                .get(self.pos)
                .ok_or_else(|| self.error("truncated \\u escape"))?;
            let digit = (byte as char)
                .to_digit(16)
                .ok_or_else(|| self.error("invalid hexadecimal digit in \\u escape"))?;
            value = value * 16 + digit;
            self.pos += 1;
        }
        Ok(value)
    }

    fn array(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.pos += 1; // '['
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.bytes.get(self.pos) == Some(&b']') {
            self.pos += 1;
            return Ok(Json::Array(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.skip_whitespace();
            match self.bytes.get(self.pos) {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Json::Array(items));
                }
                _ => return Err(self.error("expected `,` or `]` in JSON array")),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.pos += 1; // '{'
        let mut entries = Vec::new();
        self.skip_whitespace();
        if self.bytes.get(self.pos) == Some(&b'}') {
            self.pos += 1;
            return Ok(Json::Object(entries));
        }
        loop {
            self.skip_whitespace();
            if self.bytes.get(self.pos) != Some(&b'"') {
                return Err(self.error("expected a JSON object key"));
            }
            let key = self.string()?;
            self.skip_whitespace();
            if self.bytes.get(self.pos) != Some(&b':') {
                return Err(self.error("expected `:` after JSON object key"));
            }
            self.pos += 1;
            let value = self.value(depth + 1)?;
            entries.push((key, value));
            self.skip_whitespace();
            match self.bytes.get(self.pos) {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Json::Object(entries));
                }
                _ => return Err(self.error("expected `,` or `}` in JSON object")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scalars_and_nesting() {
        let value = Json::parse(r#"{"a":1,"b":[true,false,null,"x"],"c":{"d":-2.5e3}}"#).unwrap();
        assert_eq!(value.get("a").and_then(Json::as_i64), Some(1));
        let b = value.get("b").and_then(Json::as_array).unwrap();
        assert_eq!(b[0].as_bool(), Some(true));
        assert_eq!(b[2], Json::Null);
        assert_eq!(b[3].as_str(), Some("x"));
        assert_eq!(
            value.get("c").and_then(|c| c.get("d")),
            Some(&Json::Number("-2.5e3".into()))
        );
    }

    #[test]
    fn parses_escapes_and_surrogate_pairs() {
        let value =
            Json::parse(r#""a\n\t\"\\\/\u{41}""#.replace(r"\u{41}", r"\u0041").as_str()).unwrap();
        assert_eq!(value.as_str(), Some("a\n\t\"\\/A"));
        let emoji = Json::parse(r#""\ud83d\ude00""#).unwrap();
        assert_eq!(emoji.as_str(), Some("😀"));
    }

    #[test]
    fn rejects_invalid_json() {
        for src in [
            "",
            "{",
            "{\"a\":}",
            "[1,]",
            "01",
            "1.",
            "\"\\q\"",
            "\"a\nb\"",
            "{} trailing",
            "\"\\ud800\"",
        ] {
            assert!(Json::parse(src).is_err(), "should reject {src:?}");
        }
    }

    #[test]
    fn rejects_excessive_nesting() {
        let deep = "[".repeat(MAX_DEPTH + 2) + &"]".repeat(MAX_DEPTH + 2);
        assert!(Json::parse(&deep).is_err());
    }

    #[test]
    fn pretty_writer_is_stable_and_escapes_control_chars() {
        let value = Json::object(vec![
            ("schema_version", Json::int(1)),
            ("detail", Json::string("line\n\u{1}\t\"quoted\"")),
            ("cases", Json::Array(vec![])),
        ]);
        let rendered = value.write_pretty();
        assert_eq!(
            rendered,
            "{\n  \"schema_version\": 1,\n  \"detail\": \"line\\n\\u0001\\t\\\"quoted\\\"\",\n  \"cases\": []\n}\n"
        );
        // Re-parsing our own output must succeed (round-trip of the grammar).
        assert_eq!(Json::parse(&rendered).unwrap(), value);
    }

    #[test]
    fn integer_accessor_rejects_fractions() {
        assert_eq!(Json::Number("42".into()).as_i64(), Some(42));
        assert_eq!(Json::Number("42.0".into()).as_i64(), None);
        assert_eq!(Json::Number("1e3".into()).as_i64(), None);
        assert_eq!(Json::String("42".into()).as_i64(), None);
    }
}
