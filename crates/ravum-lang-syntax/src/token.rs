//! Lexical tokens. The lexer keeps byte spans; the parser keeps the token text
//! needed for diagnostics (numeric literals are parsed early so overflow is an
//! error at the right place).

use crate::span::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kw {
    Fn,
    Let,
    Mut,
    Const,
    Struct,
    Enum,
    Impl,
    Use,
    Pub,
    Mod,
    Match,
    If,
    Else,
    While,
    Loop,
    For,
    In,
    Break,
    Continue,
    Return,
    True,
    False,
    As,
    Test,
    Unsafe,
    SelfValue,
    SelfType,
    Crate,
    Super,
    Trait,
    Where,
    Extern,
    Async,
    Move,
    Type,
    Static,
    Effects,
    Underscore,
}

impl Kw {
    pub fn from_str(s: &str) -> Option<Kw> {
        Some(match s {
            "fn" => Kw::Fn,
            "let" => Kw::Let,
            "mut" => Kw::Mut,
            "const" => Kw::Const,
            "struct" => Kw::Struct,
            "enum" => Kw::Enum,
            "impl" => Kw::Impl,
            "use" => Kw::Use,
            "pub" => Kw::Pub,
            "mod" => Kw::Mod,
            "match" => Kw::Match,
            "if" => Kw::If,
            "else" => Kw::Else,
            "while" => Kw::While,
            "loop" => Kw::Loop,
            "for" => Kw::For,
            "in" => Kw::In,
            "break" => Kw::Break,
            "continue" => Kw::Continue,
            "return" => Kw::Return,
            "true" => Kw::True,
            "false" => Kw::False,
            "as" => Kw::As,
            "test" => Kw::Test,
            "unsafe" => Kw::Unsafe,
            "self" => Kw::SelfValue,
            "Self" => Kw::SelfType,
            "crate" => Kw::Crate,
            "super" => Kw::Super,
            "trait" => Kw::Trait,
            "where" => Kw::Where,
            "extern" => Kw::Extern,
            "async" => Kw::Async,
            "move" => Kw::Move,
            "type" => Kw::Type,
            "static" => Kw::Static,
            "effects" => Kw::Effects,
            "_" => Kw::Underscore,
            _ => return None,
        })
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Kw::Fn => "fn",
            Kw::Let => "let",
            Kw::Mut => "mut",
            Kw::Const => "const",
            Kw::Struct => "struct",
            Kw::Enum => "enum",
            Kw::Impl => "impl",
            Kw::Use => "use",
            Kw::Pub => "pub",
            Kw::Mod => "mod",
            Kw::Match => "match",
            Kw::If => "if",
            Kw::Else => "else",
            Kw::While => "while",
            Kw::Loop => "loop",
            Kw::For => "for",
            Kw::In => "in",
            Kw::Break => "break",
            Kw::Continue => "continue",
            Kw::Return => "return",
            Kw::True => "true",
            Kw::False => "false",
            Kw::As => "as",
            Kw::Test => "test",
            Kw::Unsafe => "unsafe",
            Kw::SelfValue => "self",
            Kw::SelfType => "Self",
            Kw::Crate => "crate",
            Kw::Super => "super",
            Kw::Trait => "trait",
            Kw::Where => "where",
            Kw::Extern => "extern",
            Kw::Async => "async",
            Kw::Move => "move",
            Kw::Type => "type",
            Kw::Static => "static",
            Kw::Effects => "effects",
            Kw::Underscore => "_",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Ident(String),
    Kw(Kw),
    /// Integer literal, already parsed; suffix kept verbatim for later typing.
    Int {
        value: u128,
        suffix: Option<String>,
    },
    /// Float literal text (validated range later) plus optional suffix.
    Float {
        text: String,
        suffix: Option<String>,
    },
    Str(String),
    Char(char),
    Punct(Punct),
    /// A lifetime (`'a`). Refused by the lexer with `E-UNSUPPORTED-TOKEN`; the
    /// parser skips the token so the enclosing item still parses and the user
    /// gets one diagnostic instead of a cascade.
    Lifetime(String),
    Comment(String),
    Eof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Punct {
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Semi,
    Colon,
    ColonColon,
    Dot,
    DotDot,
    DotDotEq,
    Arrow,
    FatArrow,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Amp,
    AmpAmp,
    Pipe,
    PipePipe,
    Caret,
    Bang,
    Eq,
    EqEq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Shl,
    Shr,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,
    AmpEq,
    PipeEq,
    CaretEq,
    ShlEq,
    ShrEq,
    Question,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }

    pub fn is_kw(&self, kw: Kw) -> bool {
        matches!(&self.kind, TokenKind::Kw(k) if *k == kw)
    }

    pub fn is_punct(&self, p: Punct) -> bool {
        matches!(&self.kind, TokenKind::Punct(q) if *q == p)
    }

    pub fn is_eof(&self) -> bool {
        matches!(self.kind, TokenKind::Eof)
    }
}
