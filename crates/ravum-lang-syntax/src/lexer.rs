//! Ravum lexer.
//!
//! Robust on arbitrary input: every failure produces a diagnostic and the lexer
//! keeps going, so the parser can report several errors. No panics, no slicing on
//! non-char boundaries. `\r\n` is accepted (offsets stay byte-accurate).

use crate::diag::{Diagnostic, DiagnosticBag};
use crate::span::{FileId, Span};
use crate::token::{Kw, Punct, Token, TokenKind};

pub struct Lexer<'a> {
    file: FileId,
    src: &'a str,
    pos: usize,
    tokens: Vec<Token>,
    diags: DiagnosticBag,
    keep_trivia: bool,
}

impl<'a> Lexer<'a> {
    pub fn new(file: FileId, src: &'a str, keep_trivia: bool) -> Self {
        Self {
            file,
            src,
            pos: 0,
            tokens: Vec::new(),
            diags: DiagnosticBag::new(),
            keep_trivia,
        }
    }

    pub fn run(mut self) -> (Vec<Token>, DiagnosticBag) {
        if self.src.starts_with('\u{feff}') {
            self.pos = '\u{feff}'.len_utf8();
        }
        while self.pos < self.src.len() {
            self.skip_whitespace();
            if self.pos >= self.src.len() {
                break;
            }
            let start = self.pos;
            let ch = self.peek_char().unwrap_or('\0');
            match ch {
                '/' if self.peek_second() == Some('/') => self.line_comment(start),
                '/' if self.peek_second() == Some('*') => self.block_comment(start),
                '"' => self.string_literal(start),
                '\'' => self.char_literal(start),
                '0'..='9' => self.number(start),
                c if c == '_' || c.is_ascii_alphabetic() => self.ident_or_kw(start),
                _ => self.punct_or_error(start),
            }
        }
        let end = self.src.len() as u32;
        self.tokens
            .push(Token::new(TokenKind::Eof, Span::new(self.file, end, end)));
        (self.tokens, self.diags)
    }

    fn peek_char(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn peek_second(&self) -> Option<char> {
        let mut it = self.src[self.pos..].chars();
        it.next();
        it.next()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek_char()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn span(&self, start: usize) -> Span {
        Span::new(self.file, start as u32, self.pos as u32)
    }

    fn error(&mut self, code: &str, start: usize, message: impl Into<String>) {
        self.diags
            .push(Diagnostic::error(code, self.span(start), message));
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek_char() {
            if ch.is_whitespace() {
                self.bump();
            } else {
                break;
            }
        }
    }

    fn line_comment(&mut self, start: usize) {
        self.pos += 2;
        while let Some(ch) = self.peek_char() {
            if ch == '\n' {
                break;
            }
            self.bump();
        }
        if self.keep_trivia {
            let text = self.src[start..self.pos].to_string();
            self.tokens
                .push(Token::new(TokenKind::Comment(text), self.span(start)));
        }
    }

    fn block_comment(&mut self, start: usize) {
        self.pos += 2;
        let mut closed = false;
        while self.pos < self.src.len() {
            if self.src[self.pos..].starts_with("*/") {
                self.pos += 2;
                closed = true;
                break;
            }
            self.bump();
        }
        if !closed {
            self.error("E-LEX-COMMENT", start, "unterminated block comment");
        }
        if self.keep_trivia {
            let text = self.src[start..self.pos].to_string();
            self.tokens
                .push(Token::new(TokenKind::Comment(text), self.span(start)));
        }
    }

    fn ident_or_kw(&mut self, start: usize) {
        while let Some(ch) = self.peek_char() {
            if ch == '_' || ch.is_ascii_alphanumeric() {
                self.bump();
            } else {
                break;
            }
        }
        let text = &self.src[start..self.pos];
        // `r"…"`, `b"…"`, `br"…"` are raw/byte string prefixes: recognised by the
        // lexer (so the meaning is unambiguous) and refused with the documented
        // lexical code instead of a parse cascade. The prefix token is not
        // pushed, so the following string literal is lexed normally and the
        // parser sees a plain string expression.
        if matches!(text, "r" | "b" | "br") && self.peek_char() == Some('"') {
            self.error(
                "E-UNSUPPORTED-TOKEN",
                start,
                format!("`{text}\"…\"` strings are not supported in Ravum Core 1.0"),
            );
            return;
        }
        let kind = match Kw::from_str(text) {
            Some(kw) => TokenKind::Kw(kw),
            None => TokenKind::Ident(text.to_string()),
        };
        self.tokens.push(Token::new(kind, self.span(start)));
    }

    /// True when the last non-comment token is `.`: per LEX-8.9, digits after a
    /// field-access dot never fuse into a float (`t.0.1` is `t . 0 . 1`).
    fn prev_significant_is_dot(&self) -> bool {
        let mut index = self.tokens.len();
        while index > 0 {
            index -= 1;
            match &self.tokens[index].kind {
                TokenKind::Comment(_) => continue,
                kind => return matches!(kind, TokenKind::Punct(Punct::Dot)),
            }
        }
        false
    }

    fn number(&mut self, start: usize) {
        let prev_is_dot = self.prev_significant_is_dot();
        let mut base = 10u32;
        let mut prefixed = false;
        if self.src[start..].starts_with("0x") || self.src[start..].starts_with("0X") {
            base = 16;
            prefixed = true;
            self.pos += 2;
        } else if self.src[start..].starts_with("0o") || self.src[start..].starts_with("0O") {
            base = 8;
            prefixed = true;
            self.pos += 2;
        } else if self.src[start..].starts_with("0b") || self.src[start..].starts_with("0B") {
            base = 2;
            prefixed = true;
            self.pos += 2;
        }

        // Integer digits. A separator `_` is valid only between two digits of
        // the same literal (LEX-8.1); anything else is E-LEX-NUMBER.
        let digits_start = self.pos;
        let mut saw_digit = false;
        let mut last_was_digit = false;
        let mut bad_separator = false;
        while let Some(ch) = self.peek_char() {
            if digit_for_base(ch, base) {
                saw_digit = true;
                last_was_digit = true;
                self.bump();
            } else if ch == '_' {
                let next_is_digit =
                    matches!(self.peek_second(), Some(c) if digit_for_base(c, base));
                if last_was_digit && next_is_digit {
                    last_was_digit = false;
                    self.bump();
                } else {
                    bad_separator = true;
                    last_was_digit = false;
                    self.bump();
                }
            } else {
                break;
            }
        }

        // A digit outside the base (`0b12`, `0o8`) is malformed (LEX-8.2).
        let mut bad_base_digit = false;
        if prefixed {
            if let Some(ch) = self.peek_char() {
                if ch.is_ascii_digit() && !digit_for_base(ch, base) {
                    bad_base_digit = true;
                }
            }
        }

        if !saw_digit || bad_separator || bad_base_digit {
            // Consume the malformed tail so one diagnostic covers the literal.
            while let Some(ch) = self.peek_char() {
                if ch.is_ascii_alphanumeric() || ch == '_' {
                    self.bump();
                } else {
                    break;
                }
            }
            let message = if !saw_digit {
                "numeric literal requires at least one digit"
            } else if bad_base_digit {
                "digit out of range for the numeric base"
            } else {
                "digit separator must be between two digits"
            };
            self.error("E-LEX-NUMBER", start, message);
            self.tokens.push(Token::new(
                TokenKind::Int {
                    value: 0,
                    suffix: None,
                },
                self.span(start),
            ));
            return;
        }

        let digits_raw = &self.src[digits_start..self.pos];
        let digits: String = digits_raw.chars().filter(|c| *c != '_').collect();
        let value = u128::from_str_radix(&digits, base).unwrap_or_else(|_| {
            self.diags.push(Diagnostic::error(
                "E-LEX-INT-OVERFLOW",
                Span::new(self.file, start as u32, self.pos as u32),
                "integer literal does not fit in 128 bits",
            ));
            0
        });

        // Decimal floats: `DEC "." DEC [EXP]` or `DEC EXP` (LEX-8.2).
        let mut is_float = false;
        if base == 10 && !prev_is_dot {
            if self.peek_char() == Some('.') {
                // `0..3` is an integer followed by the range operator: a dot
                // followed by another dot never starts a float (LEX-8.8/8.9).
                if self.peek_second() == Some('.') {
                    // Leave both dots to the punctuation lexer.
                } else if matches!(self.peek_second(), Some(c) if c.is_ascii_digit()) {
                    is_float = true;
                    self.bump(); // `.`
                    self.consume_fraction_digits();
                } else {
                    // `1.` (or `1.f32`) is invalid, not a field access (LEX-8.8).
                    self.bump(); // `.`
                    while let Some(ch) = self.peek_char() {
                        if ch.is_ascii_alphanumeric() || ch == '_' {
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    let span = self.span(start);
                    self.diags.push(Diagnostic::error(
                        "E-LEX-NUMBER",
                        span,
                        "a float literal requires at least one digit after the decimal point",
                    ));
                    let text: String = self.src[start..self.pos]
                        .chars()
                        .filter(|c| *c != '_')
                        .collect();
                    self.tokens.push(Token::new(
                        TokenKind::Float { text, suffix: None },
                        self.span(start),
                    ));
                    return;
                }
            }
            if self.exponent_follows() {
                self.consume_exponent();
                is_float = true;
            }
        }

        if is_float {
            let raw = &self.src[start..self.pos];
            let text: String = raw.chars().filter(|c| *c != '_').collect();
            let suffix = self.float_suffix();
            let suffix = match suffix {
                None => None,
                Some(name) if name == "f32" || name == "f64" => Some(name),
                Some(name) => {
                    let span =
                        Span::new(self.file, (self.pos - name.len()) as u32, self.pos as u32);
                    self.diags.push(Diagnostic::error(
                        "E-LEX-NUMBER",
                        span,
                        format!("unknown float literal suffix `{name}`"),
                    ));
                    None
                }
            };
            // A float literal MUST be finite; overflow to +/-inf is an error and
            // underflow to 0.0 is accepted silently (LEX-8.10).
            match text.parse::<f64>() {
                Ok(value) if value.is_finite() => {}
                Ok(_) => {
                    let span = self.span(start);
                    self.diags.push(Diagnostic::error(
                        "E-LEX-NUMBER",
                        span,
                        "float literal overflows to infinity",
                    ));
                }
                Err(_) => {
                    let span = self.span(start);
                    self.diags.push(Diagnostic::error(
                        "E-LEX-NUMBER",
                        span,
                        "malformed float literal",
                    ));
                }
            }
            self.tokens.push(Token::new(
                TokenKind::Float { text, suffix },
                self.span(start),
            ));
            return;
        }

        // Integer suffix: attached and without separators (LEX-8.3).
        let suffix_start = self.pos;
        if matches!(self.peek_char(), Some(c) if c.is_ascii_alphabetic()) {
            while let Some(ch) = self.peek_char() {
                if ch.is_ascii_alphanumeric() || ch == '_' {
                    self.bump();
                } else {
                    break;
                }
            }
        }
        let suffix_text = if suffix_start == self.pos {
            None
        } else {
            Some(self.src[suffix_start..self.pos].to_string())
        };
        let suffix = match suffix_text {
            None => None,
            Some(name) if valid_int_suffix(&name) => Some(name),
            Some(name) => {
                let span = Span::new(self.file, suffix_start as u32, self.pos as u32);
                self.diags.push(Diagnostic::error(
                    "E-LEX-INT-SUFFIX",
                    span,
                    format!("unknown integer literal suffix `{name}`"),
                ));
                None
            }
        };
        self.tokens.push(Token::new(
            TokenKind::Int { value, suffix },
            self.span(start),
        ));
    }

    fn consume_fraction_digits(&mut self) {
        let mut last_was_digit = false;
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_digit() {
                last_was_digit = true;
                self.bump();
            } else if ch == '_'
                && last_was_digit
                && matches!(self.peek_second(), Some(c) if c.is_ascii_digit())
            {
                last_was_digit = false;
                self.bump();
            } else {
                break;
            }
        }
    }

    /// True when an exponent (`e`/`E`, optional sign, at least one digit) starts
    /// at the current position.
    fn exponent_follows(&self) -> bool {
        if !matches!(self.peek_char(), Some('e') | Some('E')) {
            return false;
        }
        let mut chars = self.src[self.pos..].chars();
        let _ = chars.next();
        let mut next = chars.next();
        if matches!(next, Some('+') | Some('-')) {
            next = chars.next();
        }
        matches!(next, Some(c) if c.is_ascii_digit())
    }

    fn consume_exponent(&mut self) {
        self.bump(); // `e` / `E`
        if matches!(self.peek_char(), Some('+') | Some('-')) {
            self.bump();
        }
        let mut last_was_digit = false;
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_digit() {
                last_was_digit = true;
                self.bump();
            } else if ch == '_'
                && last_was_digit
                && matches!(self.peek_second(), Some(c) if c.is_ascii_digit())
            {
                last_was_digit = false;
                self.bump();
            } else {
                break;
            }
        }
    }

    fn float_suffix(&mut self) -> Option<String> {
        if default_float_suffix(self.peek_char()) {
            let start = self.pos;
            let mut chars = String::new();
            while let Some(ch) = self.peek_char() {
                if ch.is_ascii_alphanumeric() || ch == '_' {
                    chars.push(ch);
                    self.bump();
                } else {
                    break;
                }
            }
            Some(self.src[start..self.pos].to_string())
        } else {
            None
        }
    }

    fn string_literal(&mut self, start: usize) {
        self.bump(); // opening quote
        let mut value = String::new();
        let mut terminated = false;
        while let Some(ch) = self.peek_char() {
            if ch == '"' {
                self.bump();
                terminated = true;
                break;
            }
            if ch == '\n' {
                // Keep the newline for the next token; report one error below.
                break;
            }
            if ch == '\\' {
                let esc_start = self.pos;
                self.bump();
                match self.bump() {
                    Some('n') => value.push('\n'),
                    Some('t') => value.push('\t'),
                    Some('r') => value.push('\r'),
                    Some('0') => value.push('\0'),
                    Some('\\') => value.push('\\'),
                    Some('"') => value.push('"'),
                    Some('\'') => value.push('\''),
                    Some('x') => match self.hex_escape(2) {
                        Some(c) => value.push(c),
                        None => self.error(
                            "E-LEX-ESCAPE",
                            esc_start,
                            "invalid \\x escape (two hex digits expected)",
                        ),
                    },
                    Some('u') => match self.unicode_escape() {
                        Some(c) => value.push(c),
                        None => self.error(
                            "E-LEX-ESCAPE",
                            esc_start,
                            "invalid \\u{...} escape (1 to 6 hex digits, valid scalar value)",
                        ),
                    },
                    Some(other) => {
                        self.error(
                            "E-LEX-ESCAPE",
                            esc_start,
                            format!("unknown escape sequence \\{other}"),
                        );
                        value.push(other);
                    }
                    None => {
                        self.error("E-LEX-STRING", start, "unterminated escape sequence");
                        break;
                    }
                }
                continue;
            }
            self.bump();
            value.push(ch);
        }
        if !terminated {
            self.error("E-LEX-STRING", start, "unterminated string literal");
        }
        self.tokens
            .push(Token::new(TokenKind::Str(value), self.span(start)));
    }

    fn char_literal(&mut self, start: usize) {
        self.bump(); // opening quote
                     // `'a` with no closing quote is a lifetime. Core 1.0 refuses lifetimes;
                     // the token is still emitted so the parser can skip it and report the
                     // rest of the item normally (one diagnostic, no cascade).
        if let Some(first) = self.peek_char() {
            if first.is_alphabetic() || first == '_' {
                let rest = &self.src[self.pos..];
                let mut end = rest.len();
                let mut terminator = None;
                for (index, ch) in rest.char_indices().skip(1) {
                    if ch.is_alphanumeric() || ch == '_' {
                        continue;
                    }
                    terminator = Some(ch);
                    end = index;
                    break;
                }
                if terminator != Some('\'') {
                    let name = rest[..end].to_string();
                    for _ in rest[..end].chars() {
                        self.bump();
                    }
                    self.error(
                        "E-UNSUPPORTED-TOKEN",
                        start,
                        "lifetime parameters (`'a`) are not supported in Ravum Core 1.0",
                    );
                    self.tokens
                        .push(Token::new(TokenKind::Lifetime(name), self.span(start)));
                    return;
                }
            }
        }
        let ch_start = self.pos;
        let value = match self.peek_char() {
            None => {
                self.error("E-LEX-CHAR", start, "unterminated character literal");
                return;
            }
            Some('\'') => {
                self.bump();
                self.error("E-LEX-CHAR", start, "empty character literal");
                self.tokens
                    .push(Token::new(TokenKind::Char('\0'), self.span(start)));
                return;
            }
            Some('\n') => {
                self.error("E-LEX-CHAR", start, "newline in character literal");
                return;
            }
            Some('\\') => {
                self.bump();
                match self.bump() {
                    Some('n') => '\n',
                    Some('t') => '\t',
                    Some('r') => '\r',
                    Some('0') => '\0',
                    Some('\\') => '\\',
                    Some('\'') => '\'',
                    Some('"') => '"',
                    Some('x') => self.hex_escape(2).unwrap_or_else(|| {
                        self.error("E-LEX-ESCAPE", ch_start, "invalid \\x escape");
                        '\0'
                    }),
                    Some('u') => self.unicode_escape().unwrap_or_else(|| {
                        self.error("E-LEX-ESCAPE", ch_start, "invalid \\u{...} escape");
                        '\0'
                    }),
                    Some(other) => {
                        self.error(
                            "E-LEX-ESCAPE",
                            ch_start,
                            format!("unknown escape sequence \\{other}"),
                        );
                        other
                    }
                    None => {
                        self.error("E-LEX-CHAR", start, "unterminated character literal");
                        return;
                    }
                }
            }
            Some(c) => {
                self.bump();
                c
            }
        };
        if self.peek_char() == Some('\'') {
            self.bump();
            self.tokens
                .push(Token::new(TokenKind::Char(value), self.span(start)));
        } else {
            self.error(
                "E-LEX-CHAR",
                start,
                "character literal must contain exactly one character",
            );
            // Recover: consume until closing quote or newline.
            while let Some(c) = self.peek_char() {
                if c == '\'' || c == '\n' {
                    break;
                }
                self.bump();
            }
            if self.peek_char() == Some('\'') {
                self.bump();
            }
            self.tokens
                .push(Token::new(TokenKind::Char(value), self.span(start)));
        }
    }

    fn hex_escape(&mut self, digits: usize) -> Option<char> {
        let mut code = 0u32;
        for _ in 0..digits {
            let ch = self.peek_char()?;
            let digit = ch.to_digit(16)?;
            self.bump();
            code = code * 16 + digit;
        }
        char::from_u32(code)
    }

    fn unicode_escape(&mut self) -> Option<char> {
        if self.peek_char() != Some('{') {
            return None;
        }
        self.bump();
        let mut code = 0u32;
        let mut count = 0;
        while let Some(ch) = self.peek_char() {
            if ch == '}' {
                self.bump();
                if count == 0 {
                    return None;
                }
                return char::from_u32(code);
            }
            let digit = ch.to_digit(16)?;
            self.bump();
            code = code.checked_mul(16)?.checked_add(digit)?;
            count += 1;
            if count > 6 {
                return None;
            }
        }
        None
    }

    fn punct_or_error(&mut self, start: usize) {
        // Three-character operators first, then two, then one.
        const PUNCTS: &[(&str, Punct)] = &[
            ("<<=", Punct::ShlEq),
            (">>=", Punct::ShrEq),
            ("..=", Punct::DotDotEq),
            ("::", Punct::ColonColon),
            ("..", Punct::DotDot),
            ("->", Punct::Arrow),
            ("=>", Punct::FatArrow),
            ("==", Punct::EqEq),
            ("!=", Punct::Ne),
            ("<=", Punct::Le),
            (">=", Punct::Ge),
            ("&&", Punct::AmpAmp),
            ("||", Punct::PipePipe),
            ("<<", Punct::Shl),
            (">>", Punct::Shr),
            ("+=", Punct::PlusEq),
            ("-=", Punct::MinusEq),
            ("*=", Punct::StarEq),
            ("/=", Punct::SlashEq),
            ("%=", Punct::PercentEq),
            ("&=", Punct::AmpEq),
            ("|=", Punct::PipeEq),
            ("^=", Punct::CaretEq),
        ];
        for (text, punct) in PUNCTS {
            if self.src[start..].starts_with(text) {
                self.pos += text.len();
                self.tokens
                    .push(Token::new(TokenKind::Punct(*punct), self.span(start)));
                return;
            }
        }
        let ch = self.bump().unwrap_or('\0');
        let punct = match ch {
            '(' => Some(Punct::LParen),
            ')' => Some(Punct::RParen),
            '{' => Some(Punct::LBrace),
            '}' => Some(Punct::RBrace),
            '[' => Some(Punct::LBracket),
            ']' => Some(Punct::RBracket),
            ',' => Some(Punct::Comma),
            ';' => Some(Punct::Semi),
            ':' => Some(Punct::Colon),
            '.' => Some(Punct::Dot),
            '+' => Some(Punct::Plus),
            '-' => Some(Punct::Minus),
            '*' => Some(Punct::Star),
            '/' => Some(Punct::Slash),
            '%' => Some(Punct::Percent),
            '&' => Some(Punct::Amp),
            '|' => Some(Punct::Pipe),
            '^' => Some(Punct::Caret),
            '!' => Some(Punct::Bang),
            '=' => Some(Punct::Eq),
            '<' => Some(Punct::Lt),
            '>' => Some(Punct::Gt),
            '?' => Some(Punct::Question),
            _ => None,
        };
        match punct {
            Some(p) => self
                .tokens
                .push(Token::new(TokenKind::Punct(p), self.span(start))),
            None => {
                if ch.is_ascii() {
                    self.error(
                        "E-LEX-CHAR",
                        start,
                        format!("unexpected character '{}'", ch.escape_default()),
                    );
                } else {
                    self.error(
                        "E-UNSUPPORTED-IDENT",
                        start,
                        format!(
                            "non-ASCII character '{}' is not supported in Ravum Core 1.0 identifiers or syntax",
                            ch
                        ),
                    );
                }
            }
        }
    }
}

fn default_float_suffix(ch: Option<char>) -> bool {
    matches!(ch, Some('f'))
}

fn valid_int_suffix(name: &str) -> bool {
    matches!(
        name,
        "i8" | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "isize"
            | "usize"
    )
}

fn digit_for_base(ch: char, base: u32) -> bool {
    match base {
        16 => ch.is_ascii_hexdigit(),
        8 => ('0'..='7').contains(&ch),
        2 => ch == '0' || ch == '1',
        _ => ch.is_ascii_digit(),
    }
}

/// Tokenize a source file. `keep_trivia` keeps comments as tokens (formatter).
pub fn lex(file: FileId, src: &str, keep_trivia: bool) -> (Vec<Token>, DiagnosticBag) {
    Lexer::new(file, src, keep_trivia).run()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<TokenKind> {
        let (tokens, diags) = lex(0, src, false);
        assert!(diags.is_empty(), "unexpected diagnostics: {:?}", diags);
        tokens.into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn empty_file_has_eof() {
        assert_eq!(kinds(""), vec![TokenKind::Eof]);
    }

    #[test]
    fn identifiers_and_keywords() {
        assert_eq!(
            kinds("let mut x = foo"),
            vec![
                TokenKind::Kw(Kw::Let),
                TokenKind::Kw(Kw::Mut),
                TokenKind::Ident("x".into()),
                TokenKind::Punct(Punct::Eq),
                TokenKind::Ident("foo".into()),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn integers_bases_and_suffixes() {
        assert_eq!(
            kinds("0 42 1_000 0xff 0o17 0b1010 7u8 42usize"),
            vec![
                TokenKind::Int {
                    value: 0,
                    suffix: None
                },
                TokenKind::Int {
                    value: 42,
                    suffix: None
                },
                TokenKind::Int {
                    value: 1000,
                    suffix: None
                },
                TokenKind::Int {
                    value: 255,
                    suffix: None
                },
                TokenKind::Int {
                    value: 15,
                    suffix: None
                },
                TokenKind::Int {
                    value: 10,
                    suffix: None
                },
                TokenKind::Int {
                    value: 7,
                    suffix: Some("u8".into())
                },
                TokenKind::Int {
                    value: 42,
                    suffix: Some("usize".into())
                },
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn exponent_floats_without_decimal_point() {
        assert_eq!(
            kinds("1e9 1E9 1e+9 1e-9 1.5e-3f64 2e0"),
            vec![
                TokenKind::Float {
                    text: "1e9".into(),
                    suffix: None
                },
                TokenKind::Float {
                    text: "1E9".into(),
                    suffix: None
                },
                TokenKind::Float {
                    text: "1e+9".into(),
                    suffix: None
                },
                TokenKind::Float {
                    text: "1e-9".into(),
                    suffix: None
                },
                TokenKind::Float {
                    text: "1.5e-3".into(),
                    suffix: Some("f64".into())
                },
                TokenKind::Float {
                    text: "2e0".into(),
                    suffix: None
                },
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn range_dots_are_not_float_fragments() {
        assert_eq!(
            kinds("0..3"),
            vec![
                TokenKind::Int {
                    value: 0,
                    suffix: None
                },
                TokenKind::Punct(Punct::DotDot),
                TokenKind::Int {
                    value: 3,
                    suffix: None
                },
                TokenKind::Eof
            ]
        );
        assert_eq!(
            kinds("0..=3"),
            vec![
                TokenKind::Int {
                    value: 0,
                    suffix: None
                },
                TokenKind::Punct(Punct::DotDotEq),
                TokenKind::Int {
                    value: 3,
                    suffix: None
                },
                TokenKind::Eof
            ]
        );
        let (_, diags) = lex(0, "for i in 0..10 { }", false);
        assert!(diags.is_empty(), "{:?}", diags.as_slice());
    }

    #[test]
    fn dot_context_does_not_fuse_field_indices_into_floats() {
        assert_eq!(
            kinds("t.0.1"),
            vec![
                TokenKind::Ident("t".into()),
                TokenKind::Punct(Punct::Dot),
                TokenKind::Int {
                    value: 0,
                    suffix: None
                },
                TokenKind::Punct(Punct::Dot),
                TokenKind::Int {
                    value: 1,
                    suffix: None
                },
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn invalid_int_suffix_is_reported_by_the_lexer() {
        let (tokens, diags) = lex(0, "10z", false);
        assert_eq!(diags.error_count(), 1);
        assert_eq!(diags.as_slice()[0].code, "E-LEX-INT-SUFFIX");
        // The suffix span is exactly the offending suffix.
        assert_eq!(diags.as_slice()[0].span.start, 2);
        assert_eq!(diags.as_slice()[0].span.end, 3);
        assert_eq!(
            tokens[0].kind,
            TokenKind::Int {
                value: 10,
                suffix: None
            }
        );
    }

    #[test]
    fn malformed_numbers_are_e_lex_number() {
        for (src, count) in [
            ("0x", 1),
            ("0x_FF", 1),
            ("1__0", 1),
            ("0b12", 1),
            ("0o8", 1),
            ("1.", 1),
            ("1e999", 1),
            ("0xff_u8", 1),
        ] {
            let (_, diags) = lex(0, src, false);
            assert!(
                diags.as_slice().iter().any(|d| d.code == "E-LEX-NUMBER"),
                "{src:?} should be E-LEX-NUMBER, got {:?}",
                diags.as_slice().iter().map(|d| &d.code).collect::<Vec<_>>()
            );
            assert_eq!(diags.error_count(), count, "{src:?}");
        }
        // Underflow to 0.0 is accepted silently.
        let (_, diags) = lex(0, "1e-999", false);
        assert!(diags.is_empty(), "{:?}", diags.as_slice());
    }

    #[test]
    fn floats() {
        assert_eq!(
            kinds("1.0 1.5e3 2.0f32 .5"),
            vec![
                TokenKind::Float {
                    text: "1.0".into(),
                    suffix: None
                },
                TokenKind::Float {
                    text: "1.5e3".into(),
                    suffix: None
                },
                TokenKind::Float {
                    text: "2.0".into(),
                    suffix: Some("f32".into())
                },
                TokenKind::Punct(Punct::Dot),
                TokenKind::Int {
                    value: 5,
                    suffix: None
                },
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn string_and_char_escapes() {
        assert_eq!(
            kinds(r#" "a\n\t\"\\\u{41}" 'x' '\n' '\u{1F600}' "#),
            vec![
                TokenKind::Str("a\n\t\"\\A".into()),
                TokenKind::Char('x'),
                TokenKind::Char('\n'),
                TokenKind::Char('😀'),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn comments_are_skipped_or_kept() {
        let (tokens, diags) = lex(0, "// hi\nfn /* mid */ x", true);
        assert!(diags.is_empty());
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind.clone()).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Comment("// hi".into()),
                TokenKind::Kw(Kw::Fn),
                TokenKind::Comment("/* mid */".into()),
                TokenKind::Ident("x".into()),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn unterminated_comment_reports_error() {
        let (_, diags) = lex(0, "/* nope", false);
        assert_eq!(diags.error_count(), 1);
        assert_eq!(diags.as_slice()[0].code, "E-LEX-COMMENT");
    }

    #[test]
    fn unterminated_string_reports_error() {
        let (_, diags) = lex(0, "\"abc", false);
        assert_eq!(diags.error_count(), 1);
    }

    #[test]
    fn unicode_identifier_is_rejected_explicitly() {
        let (_, diags) = lex(0, "let é = 1", false);
        assert_eq!(diags.error_count(), 1);
        assert_eq!(diags.as_slice()[0].code, "E-UNSUPPORTED-IDENT");
    }

    #[test]
    fn arbitrary_bytes_do_not_panic() {
        for src in [
            "\u{0}\u{1}\u{2}",
            "fn x() { \"\\u{d800}\" }",
            "0x",
            "0b2",
            "'",
            "\"\\",
            "/*/",
        ] {
            let _ = lex(0, src, true);
        }
    }

    #[test]
    fn spans_are_byte_accurate() {
        let (tokens, _) = lex(0, "fn é-x", false);
        // `é` is 2 bytes: the error token must not overlap the identifier.
        assert_eq!(tokens[0].span.start, 0);
        assert_eq!(tokens[0].span.end, 2);
        assert!(tokens.iter().all(|t| t.span.start <= t.span.end));
    }
}
