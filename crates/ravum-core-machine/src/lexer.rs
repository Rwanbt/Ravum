//! Tokens of the pure core. Written from `spec/language/lexical.md`, without
//! reusing `ravum-lang-syntax`.

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String),
    /// Magnitude and optional type suffix (`u8`, `i64`, ...).
    Int(u128, Option<String>),
    /// Value and optional suffix (`f32`, `f64`).
    Float(f64, Option<String>),
    Str(String),
    Char(char),
    Punct(&'static str),
    Eof,
}

const PUNCTS: [&str; 44] = [
    "<<=", ">>=", "..=", "::", "->", "=>", "==", "!=", "<=", ">=", "&&", "||", "+=", "-=", "*=",
    "/=", "%=", "&=", "|=", "^=", "<<", ">>", "..", "+", "-", "*", "/", "%", "&", "|", "^", "!",
    "=", "<", ">", "(", ")", "{", "}", "[", "]", ",", ";", ":",
];
const PUNCT_TAIL: [&str; 3] = [".", "#", "_"];

pub fn lex(src: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            let mut depth = 0usize;
            loop {
                if i + 1 >= chars.len() {
                    return Err("unterminated block comment".into());
                }
                if chars[i] == '/' && chars[i + 1] == '*' {
                    depth += 1;
                    i += 2;
                } else if chars[i] == '*' && chars[i + 1] == '/' {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            if word == "_" {
                out.push(Tok::Punct("_"));
            } else {
                out.push(Tok::Ident(word));
            }
            continue;
        }
        if c.is_ascii_digit() {
            let (tok, next) = number(&chars, i)?;
            out.push(tok);
            i = next;
            continue;
        }
        if c == '"' {
            let mut s = String::new();
            i += 1;
            loop {
                match chars.get(i) {
                    None => return Err("unterminated string".into()),
                    Some('"') => {
                        i += 1;
                        break;
                    }
                    Some('\\') => {
                        let (ch, next) = escape(&chars, i)?;
                        s.push(ch);
                        i = next;
                    }
                    Some(&ch) => {
                        s.push(ch);
                        i += 1;
                    }
                }
            }
            out.push(Tok::Str(s));
            continue;
        }
        if c == '\'' {
            let (ch, next) = if chars.get(i + 1) == Some(&'\\') {
                escape(&chars, i + 1)?
            } else {
                (*chars.get(i + 1).ok_or("unterminated char")?, i + 2)
            };
            if chars.get(next) != Some(&'\'') {
                return Err("unterminated char".into());
            }
            out.push(Tok::Char(ch));
            i = next + 1;
            continue;
        }
        let rest: String = chars[i..chars.len().min(i + 3)].iter().collect();
        if let Some(p) = PUNCTS
            .iter()
            .chain(PUNCT_TAIL.iter())
            .find(|p| rest.starts_with(**p))
        {
            out.push(Tok::Punct(p));
            i += p.chars().count();
            continue;
        }
        return Err(format!("unexpected character `{c}`"));
    }
    out.push(Tok::Eof);
    Ok(out)
}

fn escape(chars: &[char], i: usize) -> Result<(char, usize), String> {
    let ch = match chars.get(i + 1) {
        Some('n') => '\n',
        Some('t') => '\t',
        Some('r') => '\r',
        Some('0') => '\0',
        Some('\\') => '\\',
        Some('\'') => '\'',
        Some('"') => '"',
        _ => return Err("unsupported escape".into()),
    };
    Ok((ch, i + 2))
}

const SUFFIXES: [&str; 14] = [
    "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize", "f32",
    "f64",
];

fn number(chars: &[char], start: usize) -> Result<(Tok, usize), String> {
    let mut i = start;
    let radix = if chars[i] == '0' {
        match chars.get(i + 1) {
            Some('x') => 16,
            Some('b') => 2,
            Some('o') => 8,
            _ => 10,
        }
    } else {
        10
    };
    if radix != 10 {
        i += 2;
    }
    let mut digits = String::new();
    while i < chars.len() && (chars[i].is_digit(radix) || chars[i] == '_') {
        if chars[i] != '_' {
            digits.push(chars[i]);
        }
        i += 1;
    }
    let mut is_float = false;
    if radix == 10
        && chars.get(i) == Some(&'.')
        && chars.get(i + 1).is_some_and(|c| c.is_ascii_digit())
    {
        is_float = true;
        digits.push('.');
        i += 1;
        while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '_') {
            if chars[i] != '_' {
                digits.push(chars[i]);
            }
            i += 1;
        }
    }
    if radix == 10 && matches!(chars.get(i), Some('e') | Some('E')) {
        let mut j = i + 1;
        let mut exp = String::from("e");
        if matches!(chars.get(j), Some('+') | Some('-')) {
            exp.push(chars[j]);
            j += 1;
        }
        if chars.get(j).is_some_and(|c| c.is_ascii_digit()) {
            while j < chars.len() && chars[j].is_ascii_digit() {
                exp.push(chars[j]);
                j += 1;
            }
            is_float = true;
            digits.push_str(&exp);
            i = j;
        }
    }
    let mut suffix = None;
    for s in SUFFIXES {
        let end = i + s.len();
        if end <= chars.len() && chars[i..end].iter().collect::<String>() == s {
            suffix = Some(s.to_string());
            i = end;
            break;
        }
    }
    let float_suffix = matches!(suffix.as_deref(), Some("f32") | Some("f64"));
    if is_float || float_suffix {
        let v: f64 = digits.parse().map_err(|_| "bad float literal")?;
        Ok((Tok::Float(v, suffix), i))
    } else {
        let v = u128::from_str_radix(&digits, radix).map_err(|_| "integer literal too large")?;
        Ok((Tok::Int(v, suffix), i))
    }
}
