use super::*;

pub(super) fn normalize_split_angles(tokens: Vec<Token>) -> Vec<Token> {
    let mut out = Vec::with_capacity(tokens.len() + 8);
    for token in tokens {
        let span = token.span;
        let file = span.file;
        match token.kind {
            TokenKind::Punct(Punct::Shr) => {
                out.push(Token::new(
                    TokenKind::Punct(Punct::Gt),
                    Span::new(file, span.start, span.start + 1),
                ));
                out.push(Token::new(
                    TokenKind::Punct(Punct::Gt),
                    Span::new(file, span.start + 1, span.end),
                ));
            }
            TokenKind::Punct(Punct::ShrEq) => {
                out.push(Token::new(
                    TokenKind::Punct(Punct::Gt),
                    Span::new(file, span.start, span.start + 1),
                ));
                out.push(Token::new(
                    TokenKind::Punct(Punct::Gt),
                    Span::new(file, span.start + 1, span.start + 2),
                ));
                out.push(Token::new(
                    TokenKind::Punct(Punct::Eq),
                    Span::new(file, span.start + 2, span.end),
                ));
            }
            TokenKind::Punct(Punct::Ge) => {
                out.push(Token::new(
                    TokenKind::Punct(Punct::Gt),
                    Span::new(file, span.start, span.start + 1),
                ));
                out.push(Token::new(
                    TokenKind::Punct(Punct::Eq),
                    Span::new(file, span.start + 1, span.end),
                ));
            }
            _ => out.push(token),
        }
    }
    out
}

/// The lexer cannot tokenize `#` (frozen token set), so it emits `E-LEX-CHAR`
/// and drops it. For an attribute (`#` followed by `[`) the parser is the owner
/// of the canonical `E-UNSUPPORTED-ATTR`; remove the redundant lexer error.
pub(super) fn strip_attribute_hash_errors(diags: DiagnosticBag, src: &str) -> DiagnosticBag {
    let mut kept = DiagnosticBag::new();
    for diagnostic in diags.into_vec() {
        if diagnostic.code == "E-LEX-CHAR" && is_attribute_hash(&diagnostic.span, src) {
            continue;
        }
        kept.push(diagnostic);
    }
    kept
}

pub(super) fn is_attribute_hash(span: &Span, src: &str) -> bool {
    let bytes = src.as_bytes();
    let start = span.start as usize;
    if bytes.get(start) != Some(&b'#') {
        return false;
    }
    let mut i = start + 1;
    while let Some(&byte) = bytes.get(i) {
        if byte.is_ascii_whitespace() {
            i += 1;
        } else {
            return byte == b'[';
        }
    }
    false
}

pub(super) fn is_comparison_binary(expr: &Expr) -> bool {
    matches!(&expr.kind, ExprKind::Binary { op, .. } if op.is_comparison())
}

pub(super) fn punct_str(p: Punct) -> &'static str {
    match p {
        Punct::LParen => "(",
        Punct::RParen => ")",
        Punct::LBrace => "{",
        Punct::RBrace => "}",
        Punct::LBracket => "[",
        Punct::RBracket => "]",
        Punct::Comma => ",",
        Punct::Semi => ";",
        Punct::Colon => ":",
        Punct::ColonColon => "::",
        Punct::Dot => ".",
        Punct::DotDot => "..",
        Punct::DotDotEq => "..=",
        Punct::Arrow => "->",
        Punct::FatArrow => "=>",
        Punct::Plus => "+",
        Punct::Minus => "-",
        Punct::Star => "*",
        Punct::Slash => "/",
        Punct::Percent => "%",
        Punct::Amp => "&",
        Punct::AmpAmp => "&&",
        Punct::Pipe => "|",
        Punct::PipePipe => "||",
        Punct::Caret => "^",
        Punct::Bang => "!",
        Punct::Eq => "=",
        Punct::EqEq => "==",
        Punct::Ne => "!=",
        Punct::Lt => "<",
        Punct::Le => "<=",
        Punct::Gt => ">",
        Punct::Ge => ">=",
        Punct::Shl => "<<",
        Punct::Shr => ">>",
        Punct::PlusEq => "+=",
        Punct::MinusEq => "-=",
        Punct::StarEq => "*=",
        Punct::SlashEq => "/=",
        Punct::PercentEq => "%=",
        Punct::AmpEq => "&=",
        Punct::PipeEq => "|=",
        Punct::CaretEq => "^=",
        Punct::ShlEq => "<<=",
        Punct::ShrEq => ">>=",
        Punct::Question => "?",
    }
}

pub(super) fn opener_close(p: Punct) -> Option<Punct> {
    match p {
        Punct::LParen => Some(Punct::RParen),
        Punct::LBracket => Some(Punct::RBracket),
        Punct::LBrace => Some(Punct::RBrace),
        _ => None,
    }
}
