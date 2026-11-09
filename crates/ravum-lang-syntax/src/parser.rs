//! Recursive-descent parser for Ravum Core 1.0.
//!
//! OWNER: worker `syntax`.
//!
//! Contract:
//!
//! * `parse_file` never panics on arbitrary input and never loops forever;
//!   every loop either consumes a token or leaves because of a stop condition.
//! * node ids (see [`crate::ast`]) are unique across a file: the high 8 bits
//!   carry the [`FileId`], the low 24 bits a per-file counter. Every `Item`,
//!   `Module`, `Stmt`, `Block`, `Expr`, `Pat`, `Param`, `GenericParam`,
//!   `FieldDef`, `VariantDef`, `MatchArm`, `Type` and `Path` gets exactly one id.
//! * every syntax error is a [`crate::diag::Diagnostic`] with a stable
//!   `E-PARSE-*`, `E-UNSUPPORTED-*` or `E-LEX-*` code and an exact span;
//! * recovery keeps parsing after an error so several diagnostics are reported.
//!
//! # Guards
//!
//! * [`MAX_DEPTH`] bounds expression/type/pattern/item nesting; beyond it a
//!   single `E-PARSE-DEPTH` is emitted and parsing unwinds without parsing the
//!   rest of the file (the partial program is returned).
//! * [`MAX_TOKENS`] bounds the token stream; beyond it `E-PARSE-TOO-LARGE` is
//!   emitted and an empty root module is returned.
//! * [`MAX_NODES`] bounds allocated node ids so the 24-bit counter can never
//!   overflow into the file bits; a single `E-PARSE-TOO-LARGE` is emitted.
//!
//! # Known limitations (Core 1.0 draft)
//!
//! * `while`/`loop`/`for` are parsed (their AST nodes were added by the Lead);
//!   `break`/`continue` never take a value and labels are unsupported.
//! * Expression generic arguments require the turbofish form
//!   (`Vec::<i32>::new()`): a bare `<` in expression position is always a
//!   comparison. Type positions accept the plain form (`Vec<i32>`).
//! * `macro_rules!`/`foo!()` are rejected with `E-UNSUPPORTED-MACRO`.
//! * chained tuple indices such as `x.0.1` are rejected with
//!   `E-PARSE-UNEXPECTED`: the lexer tokenizes `0.1` as one float literal, so
//!   the frozen token stream cannot express that chain. `x.0` is supported.
//! * `pub` on an enum variant is accepted and ignored: the frozen AST has no
//!   visibility slot on `VariantDef`, and Core variants inherit the enum's
//!   visibility. `pub` on a parameter binding is *not* part of the grammar.
//! * `mut` on a `let` binding / function parameter is accepted. Parameters have
//!   no mutable flag in the frozen AST, so it is ignored there.
//! * Structs with tuple fields (`struct S(T);`) are rejected with
//!   `E-UNSUPPORTED-TUPLE-STRUCT` (only named-field and unit structs exist in
//!   the frozen AST).

use crate::ast::*;
use crate::diag::{Diagnostic, DiagnosticBag};
use crate::lexer;
use crate::span::{FileId, Span};
use crate::token::{Kw, Punct, Token, TokenKind};
use std::collections::BTreeSet;

/// Deepest expression/type/pattern/item nesting accepted before
/// `E-PARSE-DEPTH`.
pub const MAX_DEPTH: u32 = 256;
/// Largest token stream accepted before `E-PARSE-TOO-LARGE`.
pub const MAX_TOKENS: usize = 300_000;
/// Largest number of nodes a single file may allocate (24-bit counter).
pub const MAX_NODES: u32 = 2_000_000;

pub struct ParseResult {
    pub program: Program,
    pub diags: DiagnosticBag,
}

/// Allocates node ids. The high 8 bits carry the `FileId`, the low 24 bits a
/// per-file counter, so ids are unique across files without a global allocator.
/// This caps a file at ~16.7 M nodes, far below [`MAX_NODES`].
struct NodeIds {
    next: NodeId,
}

impl NodeIds {
    fn new(file: FileId) -> Self {
        Self {
            next: (file & 0xff) << 24 | 1,
        }
    }

    fn next(&mut self) -> NodeId {
        let id = self.next;
        self.next = self.next.saturating_add(1);
        id
    }
}

/// Parse one `.rv` file. Module declarations (`mod foo;`) are kept unresolved
/// for the loader to expand. The parser always returns a program (possibly
/// empty or partial) plus all diagnostics; it never panics.
pub fn parse_file(file: FileId, src: &str) -> ParseResult {
    let (tokens, diags) = lexer::lex(file, src, false);
    // `#` is not a token in the frozen token set. The lexer reports it as
    // E-LEX-CHAR; for `#[...]` / `#![...]` attributes the parser reports the
    // canonical E-UNSUPPORTED-ATTR instead, so drop the redundant lexer error.
    let mut diags = strip_attribute_hash_errors(diags, src);
    let tokens = normalize_split_angles(tokens);

    let mut ids = NodeIds::new(file);
    let root_id = ids.next();

    let end = src.len().min(u32::MAX as usize) as u32;
    let root_span = Span::new(file, 0, end);

    if tokens.len() > MAX_TOKENS {
        diags.push(Diagnostic::error(
            "E-PARSE-TOO-LARGE",
            root_span,
            format!(
                "source contains {} tokens, above the parser limit of {MAX_TOKENS}",
                tokens.len()
            ),
        ));
        return ParseResult {
            program: Program {
                file,
                root: Module {
                    id: root_id,
                    name: None,
                    items: Vec::new(),
                    span: root_span,
                },
            },
            diags,
        };
    }

    let mut parser = Parser::new(file, src, tokens, diags, ids);
    let items = parser.parse_module_items(false);
    let mut diags = parser.finish();

    let program = Program {
        file,
        root: Module {
            id: root_id,
            name: None,
            items,
            span: root_span,
        },
    };
    diags.sort();
    ParseResult { program, diags }
}

/// The lexer emits `>>`, `>=` and `>>=` as single tokens. In type/generic
/// position those close generic lists, so split them once up front; expression
/// parsing recombines adjacent `>`/`=` tokens into the original operator. The
/// split preserves byte spans (`>` and `=` are ASCII, so the halves are valid
/// UTF-8 boundaries).
fn normalize_split_angles(tokens: Vec<Token>) -> Vec<Token> {
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
fn strip_attribute_hash_errors(diags: DiagnosticBag, src: &str) -> DiagnosticBag {
    let mut kept = DiagnosticBag::new();
    for diagnostic in diags.into_vec() {
        if diagnostic.code == "E-LEX-CHAR" && is_attribute_hash(&diagnostic.span, src) {
            continue;
        }
        kept.push(diagnostic);
    }
    kept
}

fn is_attribute_hash(span: &Span, src: &str) -> bool {
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

fn is_comparison_binary(expr: &Expr) -> bool {
    matches!(&expr.kind, ExprKind::Binary { op, .. } if op.is_comparison())
}

fn punct_str(p: Punct) -> &'static str {
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

fn opener_close(p: Punct) -> Option<Punct> {
    match p {
        Punct::LParen => Some(Punct::RParen),
        Punct::LBracket => Some(Punct::RBracket),
        Punct::LBrace => Some(Punct::RBrace),
        _ => None,
    }
}

struct Parser<'a> {
    file: FileId,
    src: &'a str,
    tokens: Vec<Token>,
    pos: usize,
    diags: DiagnosticBag,
    ids: NodeIds,
    node_count: u32,
    depth: u32,
    fatal: bool,
    depth_reported: bool,
    nodes_reported: bool,
    /// Number of enclosing condition contexts where assignment is forbidden.
    forbid_assign: u32,
    /// Expressions that were written between parentheses. Used to distinguish
    /// `(a < b) < c` (allowed) from `a < b < c` (E-PARSE-UNEXPECTED) and
    /// `(t.0).0` (allowed) from `t.0.1` (E-PARSE-UNEXPECTED).
    paren_exprs: BTreeSet<NodeId>,
}

impl<'a> Parser<'a> {
    fn new(
        file: FileId,
        src: &'a str,
        tokens: Vec<Token>,
        diags: DiagnosticBag,
        ids: NodeIds,
    ) -> Self {
        Self {
            file,
            src,
            tokens,
            pos: 0,
            diags,
            ids,
            node_count: 0,
            depth: 0,
            fatal: false,
            depth_reported: false,
            nodes_reported: false,
            forbid_assign: 0,
            paren_exprs: BTreeSet::new(),
        }
    }

    fn finish(mut self) -> DiagnosticBag {
        self.diags.sort();
        self.diags
    }

    // ---------------------------------------------------------------------
    // Token access
    // ---------------------------------------------------------------------

    fn eof_index(&self) -> usize {
        self.tokens.len().saturating_sub(1)
    }

    fn cur(&self) -> &Token {
        let idx = self.pos.min(self.tokens.len().saturating_sub(1));
        &self.tokens[idx]
    }

    fn cur_span(&self) -> Span {
        self.cur().span
    }

    fn prev_end(&self) -> u32 {
        if self.pos == 0 {
            self.cur().span.start
        } else {
            self.tokens
                .get(self.pos - 1)
                .map(|t| t.span.end)
                .unwrap_or(0)
        }
    }

    fn span_from(&self, start: Span) -> Span {
        let end = self.prev_end().max(start.start);
        Span::new(start.file, start.start, end)
    }

    fn zero_span(&self) -> Span {
        let at = self.prev_end();
        Span::new(self.file, at, at)
    }

    fn at_eof(&self) -> bool {
        self.cur().is_eof()
    }

    fn bump(&mut self) {
        if !self.at_eof() {
            self.pos = (self.pos + 1).min(self.eof_index());
        }
    }

    fn bump_n(&mut self, n: usize) {
        for _ in 0..n {
            self.bump();
        }
    }

    fn kind(&self) -> &TokenKind {
        &self.cur().kind
    }

    fn at(&self, p: Punct) -> bool {
        matches!(self.kind(), TokenKind::Punct(q) if *q == p)
    }

    fn at_kw(&self, kw: Kw) -> bool {
        matches!(self.kind(), TokenKind::Kw(k) if *k == kw)
    }

    fn eat(&mut self, p: Punct) -> bool {
        if self.at(p) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn eat_kw(&mut self, kw: Kw) -> bool {
        if self.at_kw(kw) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn peek_at(&self, n: usize) -> &Token {
        let idx = (self.pos + n).min(self.eof_index());
        &self.tokens[idx]
    }

    fn peek_punct(&self, n: usize, p: Punct) -> bool {
        matches!(&self.peek_at(n).kind, TokenKind::Punct(q) if *q == p)
    }

    fn peek_ident(&self) -> bool {
        matches!(&self.peek_at(1).kind, TokenKind::Ident(_))
    }

    // ---------------------------------------------------------------------
    // Ids and guards
    // ---------------------------------------------------------------------

    fn id(&mut self) -> NodeId {
        if self.node_count >= MAX_NODES {
            if !self.nodes_reported {
                self.nodes_reported = true;
                let span = self.cur_span();
                self.diags.error(
                    "E-PARSE-TOO-LARGE",
                    span,
                    format!("file exceeds the parser node limit of {MAX_NODES} nodes"),
                );
            }
            self.fatal = true;
            return 0;
        }
        self.node_count += 1;
        self.ids.next()
    }

    fn fail_depth(&mut self) {
        if !self.depth_reported {
            self.depth_reported = true;
            let span = self.cur_span();
            self.diags.error(
                "E-PARSE-DEPTH",
                span,
                format!("nesting exceeds the maximum parser depth of {MAX_DEPTH}"),
            );
        }
        self.fatal = true;
    }

    // ---------------------------------------------------------------------
    // Diagnostics helpers
    // ---------------------------------------------------------------------

    fn describe_token(&self) -> String {
        match self.kind() {
            TokenKind::Ident(name) => format!("identifier `{name}`"),
            TokenKind::Kw(kw) => format!("keyword `{}`", kw.as_str()),
            TokenKind::Int { .. } => "integer literal".to_string(),
            TokenKind::Float { .. } => "float literal".to_string(),
            TokenKind::Str(_) => "string literal".to_string(),
            TokenKind::Char(_) => "character literal".to_string(),
            TokenKind::Punct(p) => format!("`{}`", punct_str(*p)),
            TokenKind::Comment(_) => "comment".to_string(),
            TokenKind::Eof => "end of file".to_string(),
        }
    }

    fn expected(&mut self, what: &str) {
        let span = self.cur_span();
        let found = self.describe_token();
        self.diags.error(
            "E-PARSE-EXPECTED",
            span,
            format!("expected {what}, found {found}"),
        );
    }

    fn unexpected(&mut self, message: impl Into<String>) {
        let span = self.cur_span();
        self.diags.error("E-PARSE-UNEXPECTED", span, message.into());
    }

    fn unclosed(&mut self, opener: Span, what: &str) {
        let at = self.cur_span();
        self.diags.push(
            Diagnostic::error(
                "E-PARSE-UNCLOSED",
                opener,
                format!("unclosed delimiter: expected {what}"),
            )
            .with_note(
                format!(
                    "reached {} before the delimiter was closed",
                    self.describe_token()
                ),
                Some(at),
            ),
        );
    }

    fn expect_close(&mut self, close: Punct, opener: Span, what: &str) -> bool {
        if self.eat(close) {
            true
        } else {
            self.unclosed(opener, what);
            false
        }
    }

    fn recover_and_bump(&mut self, code: &str, message: impl Into<String>) {
        let span = self.cur_span();
        self.diags.error(code, span, message);
        self.bump();
    }

    // ---------------------------------------------------------------------
    // Structural helpers
    // ---------------------------------------------------------------------

    fn unit_expr(&mut self, span: Span) -> Expr {
        let id = self.id();
        Expr {
            id,
            kind: ExprKind::Unit,
            span,
        }
    }

    fn infer_type(&mut self, span: Span) -> Type {
        let id = self.id();
        Type {
            id,
            kind: TypeKind::Infer,
            span,
        }
    }

    fn unit_type(&mut self, span: Span) -> Type {
        let id = self.id();
        Type {
            id,
            kind: TypeKind::Unit,
            span,
        }
    }

    fn wild_pat(&mut self, span: Span) -> Pat {
        let id = self.id();
        Pat {
            id,
            kind: PatKind::Wild,
            span,
        }
    }

    fn empty_block(&mut self, span: Span) -> Block {
        let id = self.id();
        Block {
            id,
            stmts: Vec::new(),
            tail: None,
            span,
        }
    }

    fn is_expr_stopper(&self) -> bool {
        matches!(
            self.kind(),
            TokenKind::Eof
                | TokenKind::Punct(
                    Punct::Semi
                        | Punct::RBrace
                        | Punct::RParen
                        | Punct::RBracket
                        | Punct::Comma
                        | Punct::FatArrow
                        | Punct::Arrow
                        | Punct::Colon
                        | Punct::Eq
                )
        )
    }

    fn is_type_stopper(&self) -> bool {
        matches!(
            self.kind(),
            TokenKind::Eof
                | TokenKind::Punct(
                    Punct::Semi
                        | Punct::RBrace
                        | Punct::RParen
                        | Punct::RBracket
                        | Punct::Comma
                        | Punct::Lt
                        | Punct::Gt
                        | Punct::Eq
                        | Punct::FatArrow
                        | Punct::Arrow
                        | Punct::Colon
                )
        )
    }

    fn is_pattern_stopper(&self) -> bool {
        matches!(
            self.kind(),
            TokenKind::Eof
                | TokenKind::Punct(
                    Punct::Semi
                        | Punct::RBrace
                        | Punct::RParen
                        | Punct::RBracket
                        | Punct::Comma
                        | Punct::FatArrow
                        | Punct::Colon
                        | Punct::Eq
                )
        )
    }

    fn starts_expr(&self) -> bool {
        match self.kind() {
            TokenKind::Ident(_)
            | TokenKind::Int { .. }
            | TokenKind::Float { .. }
            | TokenKind::Str(_)
            | TokenKind::Char(_) => true,
            TokenKind::Kw(kw) => matches!(
                kw,
                Kw::True
                    | Kw::False
                    | Kw::SelfValue
                    | Kw::If
                    | Kw::Match
                    | Kw::While
                    | Kw::Loop
                    | Kw::For
                    | Kw::Return
                    | Kw::Break
                    | Kw::Continue
                    | Kw::Unsafe
                    | Kw::Async
                    | Kw::Move
                    | Kw::Crate
                    | Kw::Super
            ),
            TokenKind::Punct(p) => matches!(
                p,
                Punct::LParen
                    | Punct::LBracket
                    | Punct::LBrace
                    | Punct::Minus
                    | Punct::Bang
                    | Punct::Star
                    | Punct::Amp
            ),
            _ => false,
        }
    }

    fn starts_pattern(&self) -> bool {
        Self::starts_pattern_kind(self.kind())
    }

    fn starts_pattern_at(&self, n: usize) -> bool {
        Self::starts_pattern_kind(&self.peek_at(n).kind)
    }

    fn starts_pattern_kind(kind: &TokenKind) -> bool {
        match kind {
            TokenKind::Ident(_)
            | TokenKind::Int { .. }
            | TokenKind::Str(_)
            | TokenKind::Char(_) => true,
            TokenKind::Kw(kw) => matches!(
                kw,
                Kw::Underscore
                    | Kw::Mut
                    | Kw::True
                    | Kw::False
                    | Kw::SelfType
                    | Kw::SelfValue
                    | Kw::Crate
                    | Kw::Super
            ),
            TokenKind::Punct(p) => {
                matches!(
                    p,
                    Punct::LParen | Punct::Minus | Punct::DotDot | Punct::DotDotEq
                )
            }
            _ => false,
        }
    }

    fn is_item_start(&self) -> bool {
        matches!(
            self.kind(),
            TokenKind::Kw(
                Kw::Fn
                    | Kw::Struct
                    | Kw::Enum
                    | Kw::Impl
                    | Kw::Const
                    | Kw::Use
                    | Kw::Mod
                    | Kw::Test
                    | Kw::Pub
                    | Kw::Trait
                    | Kw::Unsafe
                    | Kw::Async
                    | Kw::Extern
                    | Kw::Type
                    | Kw::Static
                    | Kw::Where
            )
        )
    }

    /// Consume a balanced `(...)`, `[...]` or `{...}` group. The current token
    /// must be the opener; a non-opener is consumed alone (always progresses).
    fn skip_balanced(&mut self) -> Span {
        let start = self.cur_span();
        let opener = match self.kind() {
            TokenKind::Punct(p) => *p,
            _ => {
                self.bump();
                return self.span_from(start);
            }
        };
        let Some(close) = opener_close(opener) else {
            self.bump();
            return self.span_from(start);
        };
        let mut stack = vec![close];
        self.bump();
        while !self.at_eof() && !stack.is_empty() {
            if let TokenKind::Punct(p) = self.kind() {
                if let Some(c) = opener_close(*p) {
                    stack.push(c);
                    self.bump();
                    continue;
                }
                if stack.last().copied() == Some(*p) {
                    stack.pop();
                    self.bump();
                    continue;
                }
            }
            self.bump();
        }
        self.span_from(start)
    }

    // ---------------------------------------------------------------------
    // Recovery
    // ---------------------------------------------------------------------

    /// Skip to an item boundary: after `;` (consumed), before an item-start
    /// keyword, before a closing `}` (not consumed, so the enclosing module can
    /// consume it) or at EOF.
    fn recover_item(&mut self) {
        let mut depth = 0i32;
        while !self.at_eof() && !self.fatal {
            if depth == 0 {
                if self.at(Punct::RBrace) {
                    return;
                }
                if self.at(Punct::Semi) {
                    self.bump();
                    return;
                }
                if self.is_item_start() {
                    return;
                }
            }
            match self.kind() {
                TokenKind::Punct(Punct::LBrace) => {
                    depth += 1;
                    self.bump();
                }
                TokenKind::Punct(Punct::RBrace) => {
                    depth = (depth - 1).max(0);
                    self.bump();
                }
                _ => self.bump(),
            }
        }
    }

    fn recover_block_or_arm(&mut self, stop_at_rbrace: bool) {
        let mut depth = 0i32;
        while !self.at_eof() && !self.fatal {
            if depth == 0 {
                if self.at(Punct::Comma) {
                    self.bump();
                    return;
                }
                if stop_at_rbrace && self.at(Punct::RBrace) {
                    return;
                }
            }
            match self.kind() {
                TokenKind::Punct(Punct::LParen)
                | TokenKind::Punct(Punct::LBracket)
                | TokenKind::Punct(Punct::LBrace) => {
                    depth += 1;
                    self.bump();
                }
                TokenKind::Punct(Punct::RParen)
                | TokenKind::Punct(Punct::RBracket)
                | TokenKind::Punct(Punct::RBrace) => {
                    depth = (depth - 1).max(0);
                    self.bump();
                }
                _ => self.bump(),
            }
        }
    }

    // ---------------------------------------------------------------------
    // Items
    // ---------------------------------------------------------------------

    fn parse_module_items(&mut self, close_with_rbrace: bool) -> Vec<Item> {
        let mut items = Vec::new();
        loop {
            if self.fatal || self.at_eof() {
                break;
            }
            if self.at(Punct::RBrace) {
                if close_with_rbrace {
                    break;
                }
                self.recover_and_bump("E-PARSE-UNEXPECTED", "unexpected `}` at top level");
                continue;
            }
            let before = self.pos;
            match self.parse_item() {
                Some(item) => {
                    items.push(item);
                    if self.pos == before && !self.fatal {
                        self.bump();
                    }
                }
                None => {
                    if self.fatal {
                        break;
                    }
                    self.recover_item();
                    if self.pos == before {
                        self.recover_and_bump(
                            "E-PARSE-UNEXPECTED",
                            "unexpected token; expected an item",
                        );
                    }
                }
            }
        }
        items
    }

    fn parse_item(&mut self) -> Option<Item> {
        if self.fatal {
            return None;
        }
        if self.depth >= MAX_DEPTH {
            self.fail_depth();
            return None;
        }
        self.depth += 1;
        let result = self.parse_item_inner();
        self.depth -= 1;
        result
    }

    fn parse_item_inner(&mut self) -> Option<Item> {
        let start = self.cur_span();
        self.skip_attributes();
        if self.fatal {
            return None;
        }
        let vis = self.parse_vis();
        self.skip_attributes();
        if self.fatal {
            return None;
        }

        let kind: Option<ItemKind> = match self.kind().clone() {
            TokenKind::Kw(Kw::Mod) => self.parse_mod_item(),
            TokenKind::Kw(Kw::Use) => self.parse_use_item(&vis),
            TokenKind::Kw(Kw::Fn) => self.parse_fn_item(false),
            TokenKind::Kw(Kw::Test) => self.parse_fn_item(true),
            TokenKind::Kw(Kw::Struct) => self.parse_struct_item(),
            TokenKind::Kw(Kw::Enum) => self.parse_enum_item(),
            TokenKind::Kw(Kw::Impl) => self.parse_impl_item(),
            TokenKind::Kw(Kw::Const) => self.parse_const_item(),
            TokenKind::Kw(Kw::Trait) => self.unsupported_item("E-UNSUPPORTED-TRAIT", "`trait`"),
            TokenKind::Kw(Kw::Where) => self.unsupported_item("E-UNSUPPORTED-WHERE", "`where`"),
            TokenKind::Kw(Kw::Extern) => self.parse_extern_block(),
            TokenKind::Kw(Kw::Async) => self.unsupported_item("E-UNSUPPORTED-ASYNC", "`async`"),
            TokenKind::Kw(Kw::Unsafe) => self.unsupported_item("E-UNSUPPORTED-UNSAFE", "`unsafe`"),
            TokenKind::Kw(Kw::Move) => self.unsupported_item("E-UNSUPPORTED-MOVE", "`move`"),
            TokenKind::Kw(Kw::Type) => {
                self.unsupported_item("E-UNSUPPORTED-TYPE-ALIAS", "`type` aliases")
            }
            TokenKind::Kw(Kw::Static) => {
                self.unsupported_item("E-UNSUPPORTED-STATIC", "`static` items")
            }
            TokenKind::Ident(_) if self.peek_punct(1, Punct::Bang) => {
                let span = self.cur_span();
                self.bump();
                self.bump();
                self.skip_balanced();
                let span = self.span_from(span);
                self.diags.error(
                    "E-UNSUPPORTED-MACRO",
                    span,
                    "macro definitions and invocations are not supported in Ravum Core 1.0",
                );
                self.recover_item();
                None
            }
            _ => {
                let found = self.describe_token();
                self.unexpected(format!("expected an item, found {found}"));
                self.bump();
                self.recover_item();
                None
            }
        };

        match kind {
            Some(kind) => Some(Item {
                id: self.id(),
                vis,
                kind,
                span: self.span_from(start),
            }),
            None => None,
        }
    }

    fn unsupported_item(&mut self, code: &str, what: &str) -> Option<ItemKind> {
        let span = self.cur_span();
        self.bump();
        self.diags.error(
            code,
            span,
            format!("{what} is not supported in Ravum Core 1.0"),
        );
        self.recover_item();
        None
    }

    fn parse_vis(&mut self) -> Vis {
        if !self.at_kw(Kw::Pub) {
            return Vis::Private;
        }
        let pub_span = self.cur_span();
        self.bump();
        if self.at(Punct::LParen) {
            let open = self.cur_span();
            self.skip_balanced();
            let span = pub_span.join(self.span_from(open));
            self.diags.error(
                "E-UNSUPPORTED-VIS",
                span,
                "restricted visibility `pub(...)` is not supported in Ravum Core 1.0",
            );
            return Vis::Public(pub_span);
        }
        if self.at_kw(Kw::Test) {
            let span = pub_span.join(self.cur_span());
            self.diags.error(
                "E-PARSE-EXPECTED",
                span,
                "`test fn` is private in Ravum Core 1.0; `pub test fn` is not a valid item",
            );
        }
        Vis::Public(pub_span)
    }

    /// Detect `#[...]` even though `#` is not a token: the lexer reports it as
    /// `E-LEX-CHAR` and drops it, leaving `[` at the same position. Look back for
    /// `#` before the `[` so the parser can report the intended
    /// `E-UNSUPPORTED-ATTR` and skip the group.
    fn attribute_hash_pos(&self) -> Option<u32> {
        if !self.at(Punct::LBracket) {
            return None;
        }
        let start = self.cur_span().start as usize;
        let bytes = self.src.as_bytes();
        let mut i = start;
        while i > 0 {
            i -= 1;
            match bytes[i] {
                b' ' | b'\t' | b'\r' | b'\n' => continue,
                b'#' => return Some(i as u32),
                _ => return None,
            }
        }
        None
    }

    fn skip_attributes(&mut self) {
        while let Some(hash) = self.attribute_hash_pos() {
            self.report_attribute(hash);
        }
    }

    /// Consumes one `#[...]`/`#![...]` group (the current token is the opening
    /// `[`) and reports the canonical single diagnostic.
    fn report_attribute(&mut self, hash: u32) {
        self.skip_balanced();
        let span = Span::new(self.file, hash, self.prev_end().max(hash));
        self.diags.error(
            "E-UNSUPPORTED-ATTR",
            span,
            "attributes (`#[...]`) are not supported in Ravum Core 1.0",
        );
    }

    fn parse_ident(&mut self, what: &str) -> Ident {
        match self.kind().clone() {
            TokenKind::Ident(name) => {
                let span = self.cur_span();
                self.bump();
                Ident::new(name, span)
            }
            _ => {
                self.expected(what);
                Ident::new("<error>", self.cur_span())
            }
        }
    }

    fn parse_mod_item(&mut self) -> Option<ItemKind> {
        let start = self.cur_span();
        self.bump(); // `mod`
        let name = self.parse_ident("module name");
        if self.eat(Punct::Semi) {
            return Some(ItemKind::ModDecl(ModDecl { name }));
        }
        if self.at(Punct::LBrace) {
            let module = self.parse_module_body(Some(name), start);
            return Some(ItemKind::Mod(module));
        }
        self.expected("`;` or `{` after the module name");
        self.recover_item();
        None
    }

    fn parse_module_body(&mut self, name: Option<Ident>, start: Span) -> Module {
        let open = self.cur_span();
        if !self.eat(Punct::LBrace) {
            self.expected("`{`");
            return Module {
                id: self.id(),
                name,
                items: Vec::new(),
                span: start,
            };
        }
        let items = self.parse_module_items(true);
        self.expect_close(Punct::RBrace, open, "`}`");
        Module {
            id: self.id(),
            name,
            items,
            span: self.span_from(start),
        }
    }

    fn parse_use_item(&mut self, vis: &Vis) -> Option<ItemKind> {
        let start = self.cur_span();
        self.bump(); // `use`
        if vis.is_public() {
            self.diags.error(
                "E-UNSUPPORTED-REEXPORT",
                vis_span(vis),
                "`pub use` re-exports are not supported in Ravum Core 1.0",
            );
        }
        let path = self.parse_path(false, false);
        if self.at(Punct::ColonColon) && self.peek_punct(1, Punct::Star) {
            let star = self.peek_at(1).span;
            self.bump_n(2);
            self.diags.error(
                "E-UNSUPPORTED-GLOB",
                star,
                "glob imports (`use foo::*;`) are not supported in Ravum Core 1.0",
            );
        } else if self.at(Punct::Star) {
            let star = self.cur_span();
            self.bump();
            self.diags.error(
                "E-UNSUPPORTED-GLOB",
                star,
                "glob imports (`use foo::*;`) are not supported in Ravum Core 1.0",
            );
        }
        if self.at_kw(Kw::As) {
            let span = self.cur_span();
            self.bump();
            self.parse_ident("import alias");
            self.diags.error(
                "E-UNSUPPORTED-ALIAS",
                span,
                "`use ... as ...` aliases are not supported in Ravum Core 1.0",
            );
        }
        if !self.eat(Punct::Semi) {
            self.expected("`;`");
        }
        Some(ItemKind::Use(UseItem {
            path,
            span: self.span_from(start),
        }))
    }

    fn parse_generics(&mut self) -> Vec<GenericParam> {
        let mut out = Vec::new();
        if !self.at(Punct::Lt) {
            return out;
        }
        let opener = self.cur_span();
        self.bump(); // `<`
        if self.at(Punct::Gt) {
            self.expected("generic parameter name");
            self.bump();
            return out;
        }
        loop {
            if self.fatal || self.at_eof() || self.at(Punct::Gt) {
                break;
            }
            let before = self.pos;
            match self.kind().clone() {
                TokenKind::Ident(name) => {
                    let span = self.cur_span();
                    self.bump();
                    out.push(GenericParam {
                        id: self.id(),
                        name: Ident::new(name, span),
                    });
                }
                _ => {
                    self.expected("generic parameter name");
                    if !self.at(Punct::Gt) && !self.at(Punct::Comma) {
                        self.bump();
                    }
                }
            }
            if self.at(Punct::Colon) {
                let span = self.cur_span();
                self.diags.error(
                    "E-UNSUPPORTED-BOUNDS",
                    span,
                    "generic bounds are not supported in Ravum Core 1.0",
                );
                self.skip_bound();
            }
            if self.eat(Punct::Comma) {
                continue;
            }
            if self.pos == before && !self.at(Punct::Gt) {
                self.bump();
                continue;
            }
            break;
        }
        self.expect_gt(opener, "unclosed generic parameter list");
        out
    }

    fn skip_bound(&mut self) {
        // `:` was not consumed by the caller; skip it and the bound tokens.
        self.bump();
        let mut angles = 0i32;
        while !self.at_eof() {
            match self.kind() {
                TokenKind::Punct(Punct::Lt) => {
                    angles += 1;
                    self.bump();
                }
                TokenKind::Punct(Punct::Gt) if angles > 0 => {
                    angles -= 1;
                    self.bump();
                }
                TokenKind::Punct(Punct::Comma | Punct::Gt) if angles == 0 => break,
                TokenKind::Punct(Punct::LBrace | Punct::RBrace | Punct::Semi) => break,
                TokenKind::Punct(Punct::LParen | Punct::LBracket) => {
                    self.skip_balanced();
                }
                _ => self.bump(),
            }
        }
    }

    fn expect_gt(&mut self, opener: Span, message: &str) {
        if self.at(Punct::Gt) {
            self.bump();
            return;
        }
        let at = self.cur_span();
        self.diags.push(
            Diagnostic::error("E-PARSE-UNCLOSED", opener, message.to_string()).with_note(
                format!(
                    "reached {} before the generic list was closed",
                    self.describe_token()
                ),
                Some(at),
            ),
        );
    }

    /// `extern "C" { fn f(x: i32) -> i32; ... }`.
    ///
    /// Core 1.0 accepts only the `"C"` ABI and declarations ending in `;`.
    /// `extern "C" fn f() { ... }` (a definition) remains
    /// `E-UNSUPPORTED-EXTERN`.
    fn parse_extern_block(&mut self) -> Option<ItemKind> {
        let start = self.cur_span();
        self.bump(); // `extern`
        let abi = if let TokenKind::Str(value) = self.kind().clone() {
            let span = self.cur_span();
            self.bump();
            Ident::new(value, span)
        } else {
            self.expected("ABI string after `extern` (only \"C\")");
            Ident::new("C".to_string(), self.zero_span())
        };
        if abi.name != "C" {
            self.diags.error(
                "E-UNSUPPORTED-ABI",
                abi.span,
                format!(
                    "`extern \"{}\"` is not supported in Ravum Core 1.0 (only `extern \"C\"`)",
                    abi.name
                ),
            );
        }
        if self.at_kw(Kw::Fn) {
            self.diags.error(
                "E-UNSUPPORTED-EXTERN",
                start,
                "`extern` function definitions are not supported in Ravum Core 1.0; declare them in an `extern \"C\" { ... }` block",
            );
            self.recover_item();
            return None;
        }
        if !self.at(Punct::LBrace) {
            self.expected("`{`");
            self.recover_item();
            return None;
        }
        self.bump(); // `{`
        let mut fns = Vec::new();
        while !self.at(Punct::RBrace) && !self.at_eof() && !self.fatal {
            if !self.at_kw(Kw::Fn) {
                let found = self.describe_token();
                self.unexpected(format!(
                    "expected `fn` or `}}` in `extern` block, found {found}"
                ));
                self.bump();
                continue;
            }
            let fn_start = self.cur_span();
            self.bump(); // `fn`
            let name = self.parse_ident("function name");
            let mut params = Vec::new();
            if self.at(Punct::LParen) {
                params = self.parse_params();
            } else {
                self.expected("`(`");
            }
            let ret = if self.eat(Punct::Arrow) {
                self.parse_type()
            } else {
                let span = self.zero_span();
                self.unit_type(span)
            };
            if !self.eat(Punct::Semi) {
                self.expected("`;` after `extern` function declaration");
            }
            fns.push(ExternFn {
                id: self.id(),
                name,
                params,
                ret,
                span: self.span_from(fn_start),
            });
        }
        if self.at(Punct::RBrace) {
            self.bump();
        } else {
            self.diags
                .error("E-PARSE-UNCLOSED", start, "unclosed `extern` block");
        }
        Some(ItemKind::Extern(ExternBlock {
            abi,
            fns,
            span: self.span_from(start),
        }))
    }

    fn parse_fn_item(&mut self, is_test: bool) -> Option<ItemKind> {
        let start = self.cur_span();
        if is_test {
            self.bump(); // `test`
            if !self.at_kw(Kw::Fn) {
                self.expected("`fn` after `test`");
                self.recover_item();
                return None;
            }
        }
        self.bump(); // `fn`
        let name = self.parse_ident("function name");
        let generics = self.parse_generics();
        let mut params = Vec::new();
        if self.at(Punct::LParen) {
            params = self.parse_params();
        } else {
            self.expected("`(`");
        }
        let ret = if self.eat(Punct::Arrow) {
            self.parse_type()
        } else {
            let span = self.zero_span();
            self.unit_type(span)
        };
        let effects = self.parse_effects();
        if effects.is_some() && self.at(Punct::Arrow) {
            // `effects(...)` must come after the return type (G-3.2). It is
            // valid after `)` only when the return type is omitted, so a `->`
            // here means the clause was misplaced: one precise diagnostic, then
            // skip the return type so the body still parses.
            let span = self.cur_span();
            self.diags.error(
                "E-PARSE-EXPECTED",
                span,
                "`effects(...)` must follow the return type; write `-> Ty effects(...)`",
            );
            self.bump(); // `->`
            let _ = self.parse_type();
        }
        if self.at_kw(Kw::Where) {
            let span = self.cur_span();
            self.bump();
            self.diags.error(
                "E-UNSUPPORTED-WHERE",
                span,
                "`where` clauses are not supported in Ravum Core 1.0",
            );
            self.skip_where_clause();
        }
        let body = if self.at(Punct::LBrace) {
            self.parse_block()
        } else {
            self.expected("`{`");
            if self.at(Punct::Semi) {
                self.bump();
            }
            let span = self.zero_span();
            self.empty_block(span)
        };
        Some(ItemKind::Fn(FnItem {
            name,
            generics,
            params,
            ret,
            effects,
            body,
            is_test,
            span: self.span_from(start),
        }))
    }

    fn skip_where_clause(&mut self) {
        let mut angles = 0i32;
        while !self.at_eof() {
            match self.kind() {
                TokenKind::Punct(Punct::LBrace) if angles == 0 => break,
                TokenKind::Punct(Punct::Semi) if angles == 0 => break,
                TokenKind::Punct(Punct::Lt) => {
                    angles += 1;
                    self.bump();
                }
                TokenKind::Punct(Punct::Gt) if angles > 0 => {
                    angles -= 1;
                    self.bump();
                }
                _ if angles == 0 && self.is_item_start() => break,
                _ => self.bump(),
            }
        }
    }

    fn parse_params(&mut self) -> Vec<Param> {
        let opener = self.cur_span();
        self.bump(); // `(`
        let mut params = Vec::new();
        loop {
            if self.fatal || self.at_eof() || self.at(Punct::RParen) {
                break;
            }
            if self.is_item_start() {
                break;
            }
            let before = self.pos;
            let p_start = self.cur_span();
            let kind = if self.at(Punct::Amp) {
                self.bump();
                let mutable = self.eat_kw(Kw::Mut);
                if self.at_kw(Kw::SelfValue) {
                    self.bump();
                    ParamKind::Receiver(if mutable {
                        Receiver::Exclusive
                    } else {
                        Receiver::Shared
                    })
                } else {
                    self.expected("`self`");
                    ParamKind::Receiver(Receiver::Shared)
                }
            } else if self.at_kw(Kw::SelfValue) {
                self.bump();
                ParamKind::Receiver(Receiver::ByValue)
            } else if self.at_kw(Kw::Mut) {
                // `mut name: Ty` is accepted; the frozen AST has no mutable flag
                // on parameters, so the modifier is ignored here.
                self.bump();
                let name = self.parse_ident("parameter name");
                if self.eat(Punct::Colon) {
                    let ty = self.parse_type();
                    ParamKind::Binding { name, ty }
                } else {
                    self.expected("`:` and a parameter type");
                    let span = self.zero_span();
                    let ty = self.infer_type(span);
                    ParamKind::Binding { name, ty }
                }
            } else if matches!(self.kind(), TokenKind::Ident(_)) {
                let name = self.parse_ident("parameter name");
                if self.eat(Punct::Colon) {
                    let ty = self.parse_type();
                    ParamKind::Binding { name, ty }
                } else {
                    self.expected("`:` and a parameter type");
                    let span = self.zero_span();
                    let ty = self.infer_type(span);
                    ParamKind::Binding { name, ty }
                }
            } else {
                self.expected("a parameter or `)`");
                if !self.at(Punct::RParen) {
                    self.bump();
                }
                continue;
            };
            params.push(Param {
                id: self.id(),
                kind,
                span: self.span_from(p_start),
            });
            if self.eat(Punct::Comma) {
                continue;
            }
            if self.at(Punct::RParen) {
                break;
            }
            self.expected("`,` or `)`");
            if self.pos == before {
                self.bump();
            }
        }
        self.expect_close(Punct::RParen, opener, "`)`");
        params
    }

    fn parse_effects(&mut self) -> Option<Vec<Ident>> {
        if !self.at_kw(Kw::Effects) {
            return None;
        }
        self.bump(); // `effects`
        let opener = self.cur_span();
        if !self.eat(Punct::LParen) {
            self.expected("`(` after `effects`");
            return Some(Vec::new());
        }
        let mut out = Vec::new();
        loop {
            if self.fatal || self.at_eof() || self.at(Punct::RParen) {
                break;
            }
            let before = self.pos;
            let effect = self.parse_effect_name();
            if let Some(effect) = effect {
                out.push(effect);
            }
            if self.eat(Punct::Comma) {
                continue;
            }
            if self.at(Punct::RParen) {
                break;
            }
            self.expected("`,` or `)`");
            if self.pos == before {
                self.bump();
            }
        }
        self.expect_close(Punct::RParen, opener, "`)`");
        Some(out)
    }

    fn parse_effect_name(&mut self) -> Option<Ident> {
        match self.kind().clone() {
            TokenKind::Ident(name) => {
                let span = self.cur_span();
                self.bump();
                Some(Ident::new(name, span))
            }
            TokenKind::Kw(kw @ (Kw::Unsafe | Kw::Async)) => {
                let span = self.cur_span();
                self.bump();
                Some(Ident::new(kw.as_str(), span))
            }
            _ => {
                self.expected("an effect name");
                None
            }
        }
    }

    fn parse_struct_item(&mut self) -> Option<ItemKind> {
        let start = self.cur_span();
        self.bump(); // `struct`
        let name = self.parse_ident("struct name");
        let generics = self.parse_generics();
        if self.eat(Punct::Semi) {
            return Some(ItemKind::Struct(StructItem {
                name,
                generics,
                fields: Vec::new(),
                span: self.span_from(start),
            }));
        }
        if self.at(Punct::LParen) {
            let span = self.cur_span();
            self.diags.error(
                "E-UNSUPPORTED-TUPLE-STRUCT",
                span,
                "tuple structs (`struct S(...)`) are not supported in Ravum Core 1.0; use named fields",
            );
            self.skip_balanced();
            if self.at(Punct::Semi) {
                self.bump();
            }
            self.recover_item();
            return None;
        }
        if !self.at(Punct::LBrace) {
            self.expected("`{`, `(` or `;`");
            self.recover_item();
            return None;
        }
        let fields = self.parse_fields("struct");
        Some(ItemKind::Struct(StructItem {
            name,
            generics,
            fields,
            span: self.span_from(start),
        }))
    }

    fn parse_fields(&mut self, what: &str) -> Vec<FieldDef> {
        let opener = self.cur_span();
        self.bump(); // `{`
        let mut fields = Vec::new();
        loop {
            if self.fatal || self.at_eof() || self.at(Punct::RBrace) {
                break;
            }
            let before = self.pos;
            let f_start = self.cur_span();
            let vis = self.parse_vis();
            let name = self.parse_ident("field name");
            let ty = if self.eat(Punct::Colon) {
                self.parse_type()
            } else {
                self.expected("`:`");
                let span = self.zero_span();
                self.infer_type(span)
            };
            fields.push(FieldDef {
                id: self.id(),
                vis,
                name,
                ty,
                span: self.span_from(f_start),
            });
            if self.eat(Punct::Comma) {
                continue;
            }
            if self.at(Punct::RBrace) {
                break;
            }
            if self.is_item_start() {
                break;
            }
            self.expected("`,` or `}`");
            if self.pos == before {
                self.bump();
            }
        }
        self.expect_close(Punct::RBrace, opener, &format!("`}}` of the {what}"));
        fields
    }

    fn parse_enum_item(&mut self) -> Option<ItemKind> {
        let start = self.cur_span();
        self.bump(); // `enum`
        let name = self.parse_ident("enum name");
        let generics = self.parse_generics();
        if !self.at(Punct::LBrace) {
            self.expected("`{`");
            self.recover_item();
            return None;
        }
        let opener = self.cur_span();
        self.bump();
        let mut variants = Vec::new();
        loop {
            if self.fatal || self.at_eof() || self.at(Punct::RBrace) {
                break;
            }
            let before = self.pos;
            let v_start = self.cur_span();
            // `pub` on a variant is accepted but has no slot in the frozen AST;
            // Core variants inherit the enum visibility.
            let _ = self.parse_vis();
            let v_name = self.parse_ident("variant name");
            let fields = if self.at(Punct::LParen) {
                let open = self.cur_span();
                self.bump();
                let mut tys = Vec::new();
                loop {
                    if self.fatal || self.at_eof() || self.at(Punct::RParen) {
                        break;
                    }
                    let ty_before = self.pos;
                    tys.push(self.parse_type());
                    if self.eat(Punct::Comma) {
                        if self.at(Punct::RParen) {
                            break;
                        }
                        continue;
                    }
                    if self.pos == ty_before {
                        self.bump();
                        continue;
                    }
                    break;
                }
                self.expect_close(Punct::RParen, open, "`)`");
                VariantDefFields::Tuple(tys)
            } else if self.at(Punct::LBrace) {
                VariantDefFields::Struct(self.parse_fields("variant"))
            } else {
                VariantDefFields::Unit
            };
            variants.push(VariantDef {
                id: self.id(),
                name: v_name,
                fields,
                span: self.span_from(v_start),
            });
            if self.eat(Punct::Comma) {
                continue;
            }
            if self.at(Punct::RBrace) {
                break;
            }
            if self.is_item_start() {
                break;
            }
            self.expected("`,` or `}`");
            if self.pos == before {
                self.bump();
            }
        }
        self.expect_close(Punct::RBrace, opener, "`}` of the enum");
        Some(ItemKind::Enum(EnumItem {
            name,
            generics,
            variants,
            span: self.span_from(start),
        }))
    }

    fn parse_impl_item(&mut self) -> Option<ItemKind> {
        let start = self.cur_span();
        self.bump(); // `impl`
        let generics = self.parse_generics();
        let self_ty = self.parse_type();
        if self.at_kw(Kw::For) {
            let span = self.cur_span();
            self.bump();
            self.diags.error(
                "E-UNSUPPORTED-TRAIT-IMPL",
                span,
                "trait implementations (`impl Trait for Type`) are not supported in Ravum Core 1.0",
            );
            self.skip_to_brace_or_semi();
            if self.at(Punct::LBrace) {
                self.skip_balanced();
            }
            self.recover_item();
            return None;
        }
        if !self.at(Punct::LBrace) {
            self.expected("`{`");
            self.recover_item();
            return None;
        }
        let opener = self.cur_span();
        self.bump();
        let mut items = Vec::new();
        loop {
            if self.fatal || self.at_eof() || self.at(Punct::RBrace) {
                break;
            }
            let before = self.pos;
            let item_start = self.cur_span();
            self.skip_attributes();
            let vis = self.parse_vis();
            let kind = match self.kind().clone() {
                TokenKind::Kw(Kw::Fn) => self.parse_fn_item(false),
                TokenKind::Kw(Kw::Const) => self.parse_const_item(),
                TokenKind::Kw(Kw::Trait) => self.unsupported_item("E-UNSUPPORTED-TRAIT", "`trait`"),
                TokenKind::Kw(Kw::Unsafe) => {
                    self.unsupported_item("E-UNSUPPORTED-UNSAFE", "`unsafe`")
                }
                TokenKind::Kw(Kw::Async) => self.unsupported_item("E-UNSUPPORTED-ASYNC", "`async`"),
                TokenKind::Kw(Kw::Extern) => {
                    self.unsupported_item("E-UNSUPPORTED-EXTERN", "`extern`")
                }
                _ => {
                    let found = self.describe_token();
                    self.unexpected(format!(
                        "expected a method or `}}` in the impl block, found {found}"
                    ));
                    self.recover_impl_item();
                    if self.pos == before {
                        self.bump();
                    }
                    None
                }
            };
            if let Some(kind) = kind {
                items.push(Item {
                    id: self.id(),
                    vis,
                    kind,
                    span: self.span_from(item_start),
                });
            }
            if self.pos == before && !self.fatal {
                self.bump();
            }
        }
        self.expect_close(Punct::RBrace, opener, "`}` of the impl block");
        Some(ItemKind::Impl(ImplItem {
            generics,
            self_ty,
            items,
            span: self.span_from(start),
        }))
    }

    fn skip_to_brace_or_semi(&mut self) {
        let mut angles = 0i32;
        while !self.at_eof() {
            match self.kind() {
                TokenKind::Punct(Punct::LBrace) if angles == 0 => return,
                TokenKind::Punct(Punct::Semi) if angles == 0 => return,
                TokenKind::Punct(Punct::Lt) => {
                    angles += 1;
                    self.bump();
                }
                TokenKind::Punct(Punct::Gt) if angles > 0 => {
                    angles -= 1;
                    self.bump();
                }
                _ => self.bump(),
            }
        }
    }

    fn recover_impl_item(&mut self) {
        let mut depth = 0i32;
        while !self.at_eof() && !self.fatal {
            if depth == 0 {
                if self.at(Punct::RBrace) {
                    return;
                }
                if self.at_kw(Kw::Fn) || self.at_kw(Kw::Const) {
                    return;
                }
            }
            match self.kind() {
                TokenKind::Punct(Punct::LBrace) => {
                    depth += 1;
                    self.bump();
                }
                TokenKind::Punct(Punct::RBrace) => {
                    depth = (depth - 1).max(0);
                    self.bump();
                }
                _ => self.bump(),
            }
        }
    }

    fn parse_const_item(&mut self) -> Option<ItemKind> {
        let start = self.cur_span();
        self.bump(); // `const`
        let name = self.parse_ident("constant name");
        let ty = if self.eat(Punct::Colon) {
            self.parse_type()
        } else {
            self.expected("`:` and a constant type");
            let span = self.zero_span();
            self.infer_type(span)
        };
        let value = if self.eat(Punct::Eq) {
            self.parse_expr(true)
        } else {
            self.expected("`=` and a constant value");
            let span = self.zero_span();
            self.unit_expr(span)
        };
        if !self.eat(Punct::Semi) {
            self.expected("`;`");
        }
        Some(ItemKind::Const(ConstItem {
            name,
            ty,
            value,
            span: self.span_from(start),
        }))
    }

    // ---------------------------------------------------------------------
    // Types
    // ---------------------------------------------------------------------

    fn parse_type(&mut self) -> Type {
        if self.fatal {
            return self.infer_type(self.cur_span());
        }
        if self.depth >= MAX_DEPTH {
            self.fail_depth();
            return self.infer_type(self.cur_span());
        }
        self.depth += 1;
        let ty = self.parse_type_inner();
        self.depth -= 1;
        ty
    }

    fn parse_type_inner(&mut self) -> Type {
        let start = self.cur_span();
        if let Some(hash) = self.attribute_hash_pos() {
            self.report_attribute(hash);
            return self.infer_type(self.span_from(start));
        }
        match self.kind().clone() {
            TokenKind::Punct(Punct::Amp) => {
                self.bump();
                let mutable = self.eat_kw(Kw::Mut);
                let inner = self.parse_type();
                let id = self.id();
                Type {
                    id,
                    kind: TypeKind::Ref {
                        mutable,
                        inner: Box::new(inner),
                    },
                    span: self.span_from(start),
                }
            }
            TokenKind::Punct(Punct::Star) => {
                self.diags.error(
                    "E-UNSUPPORTED-UNSAFE",
                    start,
                    "raw pointer types (`*const T` / `*mut T`) are not supported in Ravum Core 1.0",
                );
                self.bump(); // `*`
                if self.at_kw(Kw::Const) || self.at_kw(Kw::Mut) {
                    self.bump();
                }
                self.skip_type_recovery();
                self.infer_type(self.span_from(start))
            }
            TokenKind::Punct(Punct::LBracket) => {
                let opener = start;
                self.bump();
                let elem = self.parse_type();
                if self.eat(Punct::Semi) {
                    let len = self.parse_expr(false);
                    self.expect_close(Punct::RBracket, opener, "`]`");
                    let id = self.id();
                    Type {
                        id,
                        kind: TypeKind::Array {
                            elem: Box::new(elem),
                            len: Box::new(len),
                        },
                        span: self.span_from(start),
                    }
                } else {
                    self.expect_close(Punct::RBracket, opener, "`]`");
                    let id = self.id();
                    Type {
                        id,
                        kind: TypeKind::Slice(Box::new(elem)),
                        span: self.span_from(start),
                    }
                }
            }
            TokenKind::Punct(Punct::LParen) => {
                self.bump();
                if self.eat(Punct::RParen) {
                    let id = self.id();
                    return Type {
                        id,
                        kind: TypeKind::Unit,
                        span: self.span_from(start),
                    };
                }
                let first = self.parse_type();
                if self.eat(Punct::Comma) {
                    let mut elems = vec![first];
                    loop {
                        if self.fatal || self.at_eof() || self.at(Punct::RParen) {
                            break;
                        }
                        let before = self.pos;
                        elems.push(self.parse_type());
                        if self.eat(Punct::Comma) {
                            continue;
                        }
                        if self.pos == before {
                            self.bump();
                        }
                        break;
                    }
                    self.expect_close(Punct::RParen, start, "`)`");
                    let id = self.id();
                    Type {
                        id,
                        kind: TypeKind::Tuple(elems),
                        span: self.span_from(start),
                    }
                } else {
                    self.expect_close(Punct::RParen, start, "`)`");
                    // Parenthesized type: return the inner node unchanged.
                    first
                }
            }
            TokenKind::Kw(Kw::Underscore) => {
                self.bump();
                let id = self.id();
                Type {
                    id,
                    kind: TypeKind::Infer,
                    span: self.span_from(start),
                }
            }
            TokenKind::Kw(Kw::SelfType) => {
                if self.peek_punct(1, Punct::ColonColon) {
                    self.diags.error(
                        "E-UNSUPPORTED-ASSOC-TYPE",
                        start,
                        "associated types on `Self` are not supported in Ravum Core 1.0",
                    );
                    self.bump_n(2);
                    self.skip_type_recovery();
                    self.infer_type(self.span_from(start))
                } else {
                    self.bump();
                    let id = self.id();
                    Type {
                        id,
                        kind: TypeKind::SelfType,
                        span: self.span_from(start),
                    }
                }
            }
            TokenKind::Kw(Kw::Impl) => {
                self.diags.error(
                    "E-UNSUPPORTED-IMPL-TRAIT",
                    start,
                    "`impl Trait` return/argument types are not supported in Ravum Core 1.0",
                );
                self.bump();
                self.skip_type_recovery();
                self.infer_type(self.span_from(start))
            }
            TokenKind::Kw(Kw::Fn) => {
                self.diags.error(
                    "E-UNSUPPORTED-FN-TYPE",
                    start,
                    "function-pointer types (`fn(...)`) are not supported in Ravum Core 1.0",
                );
                self.bump();
                self.skip_type_recovery();
                self.infer_type(self.span_from(start))
            }
            TokenKind::Kw(Kw::Trait) => {
                self.diags.error(
                    "E-UNSUPPORTED-TRAIT",
                    start,
                    "trait objects are not supported in Ravum Core 1.0",
                );
                self.bump();
                self.skip_type_recovery();
                self.infer_type(self.span_from(start))
            }
            TokenKind::Punct(Punct::Bang) => {
                self.diags.error(
                    "E-UNSUPPORTED-TYPE",
                    start,
                    "the never type (`!`) is not supported in Ravum Core 1.0",
                );
                self.bump();
                self.infer_type(self.span_from(start))
            }
            TokenKind::Ident(ref name) if name == "dyn" => {
                self.diags.error(
                    "E-UNSUPPORTED-TRAIT",
                    start,
                    "trait objects (`dyn Trait`) are not supported in Ravum Core 1.0",
                );
                self.bump();
                self.skip_type_recovery();
                self.infer_type(self.span_from(start))
            }
            TokenKind::Ident(_)
            | TokenKind::Kw(Kw::Crate)
            | TokenKind::Kw(Kw::Super)
            | TokenKind::Kw(Kw::SelfValue) => {
                let path = self.parse_path(true, true);
                let id = self.id();
                Type {
                    id,
                    kind: TypeKind::Path(path),
                    span: self.span_from(start),
                }
            }
            _ => {
                self.expected("a type");
                if !self.is_type_stopper() && !self.at_eof() {
                    self.bump();
                }
                self.infer_type(self.span_from(start))
            }
        }
    }

    fn skip_type_recovery(&mut self) {
        let mut angles = 0i32;
        while !self.at_eof() {
            match self.kind() {
                TokenKind::Punct(Punct::Lt) => {
                    angles += 1;
                    self.bump();
                }
                TokenKind::Punct(Punct::Gt) if angles > 0 => {
                    angles -= 1;
                    self.bump();
                }
                TokenKind::Punct(
                    Punct::Comma
                    | Punct::RParen
                    | Punct::RBracket
                    | Punct::Semi
                    | Punct::LBrace
                    | Punct::RBrace
                    | Punct::Eq
                    | Punct::FatArrow,
                ) if angles == 0 => break,
                TokenKind::Punct(Punct::LParen | Punct::LBracket) if angles == 0 => {
                    self.skip_balanced();
                }
                _ => self.bump(),
            }
        }
    }

    fn parse_type_args(&mut self, opener: Span) -> Vec<Type> {
        let mut out = Vec::new();
        if self.at(Punct::Gt) {
            self.expected("generic argument type");
            self.bump();
            return out;
        }
        loop {
            if self.fatal || self.at_eof() || self.at(Punct::Gt) {
                break;
            }
            let before = self.pos;
            out.push(self.parse_type());
            if self.eat(Punct::Comma) {
                if self.at(Punct::Gt) {
                    break;
                }
                continue;
            }
            if self.pos == before && !self.at(Punct::Gt) {
                self.bump();
                continue;
            }
            break;
        }
        self.expect_gt(opener, "unclosed generic argument list");
        out
    }

    // ---------------------------------------------------------------------
    // Paths
    // ---------------------------------------------------------------------

    fn parse_path(&mut self, allow_generics: bool, type_ctx: bool) -> Path {
        let start = self.cur_span();
        let mut root = PathRoot::Plain;
        if self.at_kw(Kw::Crate) {
            self.bump();
            root = PathRoot::Crate;
            if !self.eat(Punct::ColonColon) {
                self.expected("`::` after `crate`");
            }
        } else if self.at_kw(Kw::Super) {
            self.bump();
            root = PathRoot::Super;
            if !self.eat(Punct::ColonColon) {
                self.expected("`::` after `super`");
            }
        } else if self.at_kw(Kw::SelfValue) && self.peek_punct(1, Punct::ColonColon) {
            self.bump_n(2);
            root = PathRoot::SelfMod;
        }

        let mut segments = Vec::new();
        loop {
            if self.fatal {
                break;
            }
            match self.kind().clone() {
                TokenKind::Ident(name) => {
                    let seg_span = self.cur_span();
                    self.bump();
                    let mut args = Vec::new();
                    if allow_generics {
                        if type_ctx && self.at(Punct::Lt) {
                            let opener = self.cur_span();
                            self.bump();
                            args = self.parse_type_args(opener);
                        } else if self.at(Punct::ColonColon) && self.peek_punct(1, Punct::Lt) {
                            self.bump(); // `::`
                            let opener = self.cur_span();
                            self.bump(); // `<`
                            args = self.parse_type_args(opener);
                        }
                    }
                    segments.push(PathSegment {
                        name: Ident::new(name, seg_span),
                        args,
                    });
                    if self.at(Punct::ColonColon) && self.peek_ident() {
                        self.bump(); // `::`
                        continue;
                    }
                    break;
                }
                _ => {
                    self.expected("path segment");
                    break;
                }
            }
        }
        Path {
            id: self.id(),
            root,
            segments,
            span: self.span_from(start),
        }
    }

    // ---------------------------------------------------------------------
    // Blocks and statements
    // ---------------------------------------------------------------------

    fn parse_block(&mut self) -> Block {
        let start = self.cur_span();
        if !self.eat(Punct::LBrace) {
            self.expected("`{`");
            return self.empty_block(Span::new(self.file, start.start, start.start));
        }
        self.parse_block_after_open(start)
    }

    fn parse_block_after_open(&mut self, start: Span) -> Block {
        // Assignments are statements: inside a block the guard is lifted for the
        // whole body (nested condition contexts re-enable it).
        let saved_forbid_assign = self.forbid_assign;
        self.forbid_assign = 0;
        let mut stmts = Vec::new();
        let mut tail = None;
        loop {
            if self.fatal || self.at_eof() || self.at(Punct::RBrace) {
                break;
            }
            // A new item where a statement is expected means the block's `}` is
            // missing: stop and let the caller report `E-PARSE-UNCLOSED`, so the
            // following item still parses. `unsafe`/`async` are excluded: they
            // start expressions (unsupported, but part of the body) and must not
            // be mistaken for items.
            if self.is_item_start()
                && !self.at_kw(Kw::Let)
                && !self.at_kw(Kw::Unsafe)
                && !self.at_kw(Kw::Async)
            {
                break;
            }
            let before = self.pos;
            if self.at(Punct::Semi) {
                self.bump();
                continue;
            }
            if self.at_kw(Kw::Let) {
                let stmt = self.parse_let_stmt();
                stmts.push(stmt);
            } else {
                let expr = self.parse_expr(true);
                let block_like = is_block_like(&expr);
                if self.at(Punct::Semi) {
                    let semi = self.cur_span();
                    self.bump();
                    let span = expr.span.join(semi);
                    let id = self.id();
                    stmts.push(Stmt {
                        id,
                        kind: StmtKind::Expr { expr, semi: true },
                        span,
                    });
                } else if self.at(Punct::RBrace) || self.at_eof() {
                    tail = Some(Box::new(expr));
                    break;
                } else if block_like {
                    // `if`/`match`/`while`/`loop`/`for`/`{ ... }` used as an
                    // expression statement does not require a trailing `;`.
                    let span = expr.span;
                    let id = self.id();
                    stmts.push(Stmt {
                        id,
                        kind: StmtKind::Expr { expr, semi: false },
                        span,
                    });
                } else {
                    self.expected("`;`");
                    let span = expr.span;
                    let id = self.id();
                    stmts.push(Stmt {
                        id,
                        kind: StmtKind::Expr { expr, semi: false },
                        span,
                    });
                }
            }
            if self.pos == before && !self.fatal {
                self.recover_and_bump("E-PARSE-UNEXPECTED", "unexpected token in block");
            }
        }
        if self.at(Punct::RBrace) {
            self.bump();
        } else {
            self.unclosed(start, "`}`");
        }
        self.forbid_assign = saved_forbid_assign;
        Block {
            id: self.id(),
            stmts,
            tail,
            span: self.span_from(start),
        }
    }

    fn parse_let_stmt(&mut self) -> Stmt {
        let start = self.cur_span();
        self.bump(); // `let`
        let pat = self.parse_pattern();
        let ty = if self.eat(Punct::Colon) {
            Some(self.parse_type())
        } else {
            None
        };
        let init = if self.eat(Punct::Eq) {
            Some(self.parse_expr(true))
        } else {
            None
        };
        if !self.eat(Punct::Semi) {
            self.expected("`;`");
        }
        Stmt {
            id: self.id(),
            kind: StmtKind::Let(LetStmt { pat, ty, init }),
            span: self.span_from(start),
        }
    }

    // ---------------------------------------------------------------------
    // Patterns
    // ---------------------------------------------------------------------

    fn parse_pattern(&mut self) -> Pat {
        if self.fatal {
            return self.wild_pat(self.cur_span());
        }
        if self.depth >= MAX_DEPTH {
            self.fail_depth();
            return self.wild_pat(self.cur_span());
        }
        self.depth += 1;
        let pat = self.parse_pattern_inner();
        self.depth -= 1;

        if self.at(Punct::DotDot) || self.at(Punct::DotDotEq) {
            let op = self.cur_span();
            self.bump();
            self.diags.error(
                "E-UNSUPPORTED-RANGE-PAT",
                op,
                "range patterns (`a..b`, `a..=b`) are not supported in Ravum Core 1.0",
            );
            if self.starts_pattern() {
                let _ = self.parse_pattern();
            }
        }
        while self.at(Punct::Pipe) {
            let op = self.cur_span();
            self.bump();
            self.diags.error(
                "E-UNSUPPORTED-OR-PAT",
                op,
                "or-patterns (`a | b`) are not supported in Ravum Core 1.0",
            );
            if self.starts_pattern() {
                let _ = self.parse_pattern();
            }
        }
        pat
    }

    fn parse_pattern_inner(&mut self) -> Pat {
        let start = self.cur_span();
        match self.kind().clone() {
            TokenKind::Kw(Kw::Underscore) => {
                self.bump();
                let id = self.id();
                Pat {
                    id,
                    kind: PatKind::Wild,
                    span: self.span_from(start),
                }
            }
            TokenKind::Kw(Kw::Mut) => {
                self.bump();
                let name = self.parse_ident("binding name");
                let id = self.id();
                Pat {
                    id,
                    kind: PatKind::Binding {
                        mutable: true,
                        name,
                    },
                    span: self.span_from(start),
                }
            }
            TokenKind::Ident(ref name) if name == "ref" && self.starts_pattern_at(1) => {
                let span = start;
                self.diags.error(
                    "E-UNSUPPORTED-REF-PAT",
                    span,
                    "`ref`/`ref mut` patterns are not supported in Ravum Core 1.0",
                );
                self.bump();
                self.eat_kw(Kw::Mut);
                if self.starts_pattern() {
                    let _ = self.parse_pattern();
                }
                self.wild_pat(self.span_from(start))
            }
            TokenKind::Ident(name) => {
                let binding_span = start;
                if self.peek_punct(1, Punct::ColonColon)
                    || self.peek_punct(1, Punct::LParen)
                    || self.peek_punct(1, Punct::LBrace)
                    || self.peek_punct(1, Punct::DotDot)
                    || self.peek_punct(1, Punct::DotDotEq)
                {
                    let path = self.parse_path(true, false);
                    self.finish_path_pattern(path, start)
                } else {
                    self.bump();
                    let id = self.id();
                    Pat {
                        id,
                        kind: PatKind::Binding {
                            mutable: false,
                            name: Ident::new(name, binding_span),
                        },
                        span: self.span_from(start),
                    }
                }
            }
            // `effects` is contextual (LEX-5.5): outside a signature clause it is
            // an ordinary identifier, so `let effects = 1;` binds it.
            TokenKind::Kw(Kw::Effects) => {
                let binding_span = start;
                self.bump();
                let id = self.id();
                Pat {
                    id,
                    kind: PatKind::Binding {
                        mutable: false,
                        name: Ident::new("effects".to_string(), binding_span),
                    },
                    span: self.span_from(start),
                }
            }
            TokenKind::Kw(Kw::True) => {
                self.bump();
                let id = self.id();
                Pat {
                    id,
                    kind: PatKind::Lit(LitPat::Bool(true)),
                    span: self.span_from(start),
                }
            }
            TokenKind::Kw(Kw::False) => {
                self.bump();
                let id = self.id();
                Pat {
                    id,
                    kind: PatKind::Lit(LitPat::Bool(false)),
                    span: self.span_from(start),
                }
            }
            TokenKind::Int { value, suffix } => {
                let span = self.cur_span();
                self.bump();
                let suffix = self.int_suffix(suffix, span);
                let id = self.id();
                Pat {
                    id,
                    kind: PatKind::Lit(LitPat::Int { value, suffix }),
                    span: self.span_from(start),
                }
            }
            TokenKind::Str(value) => {
                self.bump();
                let id = self.id();
                Pat {
                    id,
                    kind: PatKind::Lit(LitPat::Str(value)),
                    span: self.span_from(start),
                }
            }
            TokenKind::Char(value) => {
                self.bump();
                let id = self.id();
                Pat {
                    id,
                    kind: PatKind::Lit(LitPat::Char(value)),
                    span: self.span_from(start),
                }
            }
            TokenKind::Punct(Punct::LParen) => {
                let opener = start;
                self.bump();
                if self.eat(Punct::RParen) {
                    let id = self.id();
                    return Pat {
                        id,
                        kind: PatKind::Tuple(Vec::new()),
                        span: self.span_from(start),
                    };
                }
                let mut pats = Vec::new();
                loop {
                    if self.fatal || self.at_eof() || self.at(Punct::RParen) {
                        break;
                    }
                    let before = self.pos;
                    pats.push(self.parse_pattern());
                    if self.eat(Punct::Comma) {
                        if self.at(Punct::RParen) {
                            break;
                        }
                        continue;
                    }
                    if self.pos == before {
                        self.bump();
                    }
                    break;
                }
                self.expect_close(Punct::RParen, opener, "`)`");
                let id = self.id();
                Pat {
                    id,
                    kind: PatKind::Tuple(pats),
                    span: self.span_from(start),
                }
            }
            TokenKind::Punct(Punct::Minus) => {
                self.bump();
                self.diags.error(
                    "E-UNSUPPORTED-NEG-PAT",
                    start,
                    "negative literal patterns are not supported in Ravum Core 1.0",
                );
                if matches!(self.kind(), TokenKind::Int { .. }) {
                    self.bump();
                }
                let id = self.id();
                Pat {
                    id,
                    kind: PatKind::Lit(LitPat::Int {
                        value: 0,
                        suffix: None,
                    }),
                    span: self.span_from(start),
                }
            }
            TokenKind::Punct(Punct::DotDot) | TokenKind::Punct(Punct::DotDotEq) => {
                let span = start;
                let eq = self.at(Punct::DotDotEq);
                self.bump();
                self.diags.error(
                    "E-UNSUPPORTED-REST-PAT",
                    span,
                    if eq {
                        "`..=` rest patterns are not supported in Ravum Core 1.0"
                    } else {
                        "`..` rest patterns are not supported in Ravum Core 1.0"
                    },
                );
                self.wild_pat(self.span_from(start))
            }
            TokenKind::Kw(Kw::Crate) | TokenKind::Kw(Kw::Super) => {
                let path = self.parse_path(true, false);
                self.finish_path_pattern(path, start)
            }
            TokenKind::Kw(Kw::SelfValue) if self.peek_punct(1, Punct::ColonColon) => {
                let path = self.parse_path(true, false);
                self.finish_path_pattern(path, start)
            }
            TokenKind::Kw(Kw::SelfType) => {
                self.unexpected("`Self` is not a valid pattern");
                self.bump();
                self.wild_pat(self.span_from(start))
            }
            _ => {
                self.expected("a pattern");
                if !self.is_pattern_stopper() && !self.at_eof() {
                    self.bump();
                }
                self.wild_pat(self.span_from(start))
            }
        }
    }

    fn finish_path_pattern(&mut self, path: Path, start: Span) -> Pat {
        if self.at(Punct::LParen) {
            let opener = self.cur_span();
            self.bump();
            let mut pats = Vec::new();
            loop {
                if self.fatal || self.at_eof() || self.at(Punct::RParen) {
                    break;
                }
                let before = self.pos;
                pats.push(self.parse_pattern());
                if self.eat(Punct::Comma) {
                    if self.at(Punct::RParen) {
                        break;
                    }
                    continue;
                }
                if self.pos == before {
                    self.bump();
                }
                break;
            }
            self.expect_close(Punct::RParen, opener, "`)`");
            let id = self.id();
            Pat {
                id,
                kind: PatKind::Variant {
                    path,
                    fields: VariantPatFields::Tuple(pats),
                },
                span: self.span_from(start),
            }
        } else if self.at(Punct::LBrace) {
            let opener = self.cur_span();
            self.bump();
            let mut fields = Vec::new();
            loop {
                if self.fatal || self.at_eof() || self.at(Punct::RBrace) {
                    break;
                }
                let before = self.pos;
                if self.at(Punct::DotDot) || self.at(Punct::DotDotEq) {
                    let span = self.cur_span();
                    self.bump();
                    self.diags.error(
                        "E-UNSUPPORTED-REST-PAT",
                        span,
                        "struct rest patterns (`..`) are not supported in Ravum Core 1.0",
                    );
                    break;
                }
                let name = self.parse_ident("field name");
                let pat = if self.eat(Punct::Colon) {
                    self.parse_pattern()
                } else {
                    let id = self.id();
                    Pat {
                        id,
                        kind: PatKind::Binding {
                            mutable: false,
                            name: name.clone(),
                        },
                        span: name.span,
                    }
                };
                fields.push(FieldPat { name, pat });
                if self.eat(Punct::Comma) {
                    continue;
                }
                if self.at(Punct::RBrace) {
                    break;
                }
                self.expected("`,` or `}`");
                if self.pos == before {
                    self.bump();
                }
            }
            self.expect_close(Punct::RBrace, opener, "`}`");
            let id = self.id();
            Pat {
                id,
                kind: PatKind::Variant {
                    path,
                    fields: VariantPatFields::Struct(fields),
                },
                span: self.span_from(start),
            }
        } else {
            let id = self.id();
            Pat {
                id,
                kind: PatKind::Path(path),
                span: self.span_from(start),
            }
        }
    }

    // ---------------------------------------------------------------------
    // Expressions
    // ---------------------------------------------------------------------

    fn parse_expr(&mut self, allow_struct: bool) -> Expr {
        if self.fatal {
            return self.unit_expr(self.cur_span());
        }
        if self.depth >= MAX_DEPTH {
            self.fail_depth();
            return self.unit_expr(self.cur_span());
        }
        self.depth += 1;
        let expr = self.parse_assign(allow_struct);
        self.depth -= 1;
        expr
    }

    fn parse_assign(&mut self, allow_struct: bool) -> Expr {
        if self.fatal {
            return self.unit_expr(self.cur_span());
        }
        if self.depth >= MAX_DEPTH {
            self.fail_depth();
            return self.unit_expr(self.cur_span());
        }
        self.depth += 1;
        let expr = self.parse_assign_inner(allow_struct);
        self.depth -= 1;
        expr
    }

    fn parse_assign_inner(&mut self, allow_struct: bool) -> Expr {
        let lhs = self.parse_range(allow_struct);
        if let Some((op, count)) = self.peek_assign() {
            if self.forbid_assign > 0 {
                let span = self.cur_span();
                self.diags.error(
                    "E-PARSE-UNEXPECTED",
                    span,
                    "assignment is not allowed here; assignment is only allowed as a statement",
                );
                self.bump_n(count);
                let _ = self.parse_assign(allow_struct);
                return lhs;
            }
            let start = lhs.span;
            self.bump_n(count);
            let value = self.parse_assign(allow_struct);
            let span = start.join(value.span);
            let id = self.id();
            return match op {
                Some(op) => Expr {
                    id,
                    kind: ExprKind::AssignOp {
                        op,
                        target: Box::new(lhs),
                        value: Box::new(value),
                    },
                    span,
                },
                None => Expr {
                    id,
                    kind: ExprKind::Assign {
                        target: Box::new(lhs),
                        value: Box::new(value),
                    },
                    span,
                },
            };
        }
        lhs
    }

    /// Assignment operators are the lowest precedence level, right-associative.
    fn peek_assign(&self) -> Option<(Option<BinOp>, usize)> {
        match self.kind() {
            TokenKind::Punct(Punct::Eq) => Some((None, 1)),
            TokenKind::Punct(Punct::PlusEq) => Some((Some(BinOp::Add), 1)),
            TokenKind::Punct(Punct::MinusEq) => Some((Some(BinOp::Sub), 1)),
            TokenKind::Punct(Punct::StarEq) => Some((Some(BinOp::Mul), 1)),
            TokenKind::Punct(Punct::SlashEq) => Some((Some(BinOp::Div), 1)),
            TokenKind::Punct(Punct::PercentEq) => Some((Some(BinOp::Rem), 1)),
            TokenKind::Punct(Punct::AmpEq) => Some((Some(BinOp::BitAnd), 1)),
            TokenKind::Punct(Punct::PipeEq) => Some((Some(BinOp::BitOr), 1)),
            TokenKind::Punct(Punct::CaretEq) => Some((Some(BinOp::BitXor), 1)),
            TokenKind::Punct(Punct::ShlEq) => Some((Some(BinOp::Shl), 1)),
            // `>>=` is split into `>` `>` `=`.
            TokenKind::Punct(Punct::Gt)
                if self.peek_punct(1, Punct::Gt) && self.peek_punct(2, Punct::Eq) =>
            {
                Some((Some(BinOp::Shr), 3))
            }
            _ => None,
        }
    }

    fn parse_range(&mut self, allow_struct: bool) -> Expr {
        if self.at(Punct::DotDot) || self.at(Punct::DotDotEq) {
            let start = self.cur_span();
            let inclusive = self.at(Punct::DotDotEq);
            self.bump();
            let end = if self.starts_expr() {
                Some(Box::new(self.parse_binary(1, allow_struct)))
            } else {
                None
            };
            let id = self.id();
            return Expr {
                id,
                kind: ExprKind::Range {
                    start: None,
                    end,
                    inclusive,
                },
                span: self.span_from(start),
            };
        }
        let lhs = self.parse_binary(1, allow_struct);
        if self.at(Punct::DotDot) || self.at(Punct::DotDotEq) {
            let start = lhs.span;
            let inclusive = self.at(Punct::DotDotEq);
            self.bump();
            let end = if self.starts_expr() {
                Some(Box::new(self.parse_binary(1, allow_struct)))
            } else {
                None
            };
            let id = self.id();
            return Expr {
                id,
                kind: ExprKind::Range {
                    start: Some(Box::new(lhs)),
                    end,
                    inclusive,
                },
                span: self.span_from(start),
            };
        }
        lhs
    }

    /// Binding powers for binary operators (design §3.3), low to high:
    /// `||` 1, `&&` 2, comparisons 3, `|` 4, `^` 5, `&` 6, shifts 7,
    /// additive 8, multiplicative 9.
    fn peek_binop(&self) -> Option<(BinOp, u8, usize)> {
        match self.kind() {
            TokenKind::Punct(Punct::PipePipe) => Some((BinOp::Or, 1, 1)),
            TokenKind::Punct(Punct::AmpAmp) => Some((BinOp::And, 2, 1)),
            TokenKind::Punct(Punct::EqEq) => Some((BinOp::Eq, 3, 1)),
            TokenKind::Punct(Punct::Ne) => Some((BinOp::Ne, 3, 1)),
            TokenKind::Punct(Punct::Lt) => Some((BinOp::Lt, 3, 1)),
            TokenKind::Punct(Punct::Le) => Some((BinOp::Le, 3, 1)),
            TokenKind::Punct(Punct::Gt) => {
                if self.peek_punct(1, Punct::Gt) {
                    if self.peek_punct(2, Punct::Eq) {
                        // `>>=` (assignment), not a binary operator.
                        None
                    } else {
                        Some((BinOp::Shr, 7, 2))
                    }
                } else if self.peek_punct(1, Punct::Eq) {
                    Some((BinOp::Ge, 3, 2))
                } else {
                    Some((BinOp::Gt, 3, 1))
                }
            }
            TokenKind::Punct(Punct::Shl) => Some((BinOp::Shl, 7, 1)),
            TokenKind::Punct(Punct::Pipe) => Some((BinOp::BitOr, 4, 1)),
            TokenKind::Punct(Punct::Caret) => Some((BinOp::BitXor, 5, 1)),
            TokenKind::Punct(Punct::Amp) => Some((BinOp::BitAnd, 6, 1)),
            TokenKind::Punct(Punct::Plus) => Some((BinOp::Add, 8, 1)),
            TokenKind::Punct(Punct::Minus) => Some((BinOp::Sub, 8, 1)),
            TokenKind::Punct(Punct::Star) => Some((BinOp::Mul, 9, 1)),
            TokenKind::Punct(Punct::Slash) => Some((BinOp::Div, 9, 1)),
            TokenKind::Punct(Punct::Percent) => Some((BinOp::Rem, 9, 1)),
            _ => None,
        }
    }

    fn parse_binary(&mut self, min_prec: u8, allow_struct: bool) -> Expr {
        let mut lhs = self.parse_cast(allow_struct);
        loop {
            let Some((op, prec, count)) = self.peek_binop() else {
                break;
            };
            if prec < min_prec {
                break;
            }
            // Comparisons are non-associative (G-6.12): `a < b < c` is a syntax
            // error, while `(a < b) < c` is accepted (tracked via paren_exprs).
            if prec == 3 && is_comparison_binary(&lhs) && !self.paren_exprs.contains(&lhs.id) {
                let span = self.cur_span();
                self.diags.error(
                    "E-PARSE-UNEXPECTED",
                    span,
                    "comparison operators cannot be chained; parenthesize the comparison",
                );
            }
            let start = lhs.span;
            self.bump_n(count);
            let rhs = self.parse_binary(prec + 1, allow_struct);
            let span = start.join(rhs.span);
            let id = self.id();
            lhs = Expr {
                id,
                kind: ExprKind::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            };
        }
        lhs
    }

    fn parse_cast(&mut self, allow_struct: bool) -> Expr {
        let mut expr = self.parse_unary(allow_struct);
        while self.at_kw(Kw::As) {
            let start = expr.span;
            self.bump();
            let ty = self.parse_type();
            let span = start.join(ty.span);
            let id = self.id();
            expr = Expr {
                id,
                kind: ExprKind::Cast {
                    expr: Box::new(expr),
                    ty,
                },
                span,
            };
        }
        expr
    }

    fn parse_unary(&mut self, allow_struct: bool) -> Expr {
        if self.fatal {
            return self.unit_expr(self.cur_span());
        }
        if self.depth >= MAX_DEPTH {
            self.fail_depth();
            return self.unit_expr(self.cur_span());
        }
        self.depth += 1;
        let expr = self.parse_unary_inner(allow_struct);
        self.depth -= 1;
        expr
    }

    fn parse_unary_inner(&mut self, allow_struct: bool) -> Expr {
        let start = self.cur_span();
        let op = match self.kind() {
            TokenKind::Punct(Punct::Minus) => Some(UnOp::Neg),
            TokenKind::Punct(Punct::Bang) => Some(UnOp::Not),
            TokenKind::Punct(Punct::Star) => Some(UnOp::Deref),
            TokenKind::Punct(Punct::Amp) => {
                self.bump();
                let op = if self.eat_kw(Kw::Mut) {
                    UnOp::RefMut
                } else {
                    UnOp::Ref
                };
                let inner = self.parse_unary(allow_struct);
                let span = start.join(inner.span);
                let id = self.id();
                return Expr {
                    id,
                    kind: ExprKind::Unary {
                        op,
                        expr: Box::new(inner),
                    },
                    span,
                };
            }
            _ => None,
        };
        match op {
            Some(op) => {
                self.bump();
                let inner = self.parse_unary(allow_struct);
                let span = start.join(inner.span);
                let id = self.id();
                Expr {
                    id,
                    kind: ExprKind::Unary {
                        op,
                        expr: Box::new(inner),
                    },
                    span,
                }
            }
            None => self.parse_postfix(allow_struct),
        }
    }

    fn parse_postfix(&mut self, allow_struct: bool) -> Expr {
        let mut expr = self.parse_primary(allow_struct);
        loop {
            if self.fatal {
                break;
            }
            let before = self.pos;
            if self.at(Punct::LParen) {
                let start = expr.span;
                let opener = self.cur_span();
                self.bump();
                let args = self.parse_call_args(opener);
                let span = self.span_from(start);
                let id = self.id();
                expr = Expr {
                    id,
                    kind: ExprKind::Call {
                        callee: Box::new(expr),
                        args,
                    },
                    span,
                };
            } else if self.at(Punct::Dot) {
                let start = expr.span;
                let dot_span = self.cur_span();
                self.bump();
                match self.kind().clone() {
                    TokenKind::Ident(name) => {
                        let ident_span = self.cur_span();
                        self.bump();
                        let method = Ident::new(name, ident_span);
                        if self.at(Punct::LParen) {
                            let opener = self.cur_span();
                            self.bump();
                            let args = self.parse_call_args(opener);
                            let span = self.span_from(start);
                            let id = self.id();
                            expr = Expr {
                                id,
                                kind: ExprKind::MethodCall {
                                    receiver: Box::new(expr),
                                    method,
                                    args,
                                },
                                span,
                            };
                        } else {
                            let span = self.span_from(start);
                            let id = self.id();
                            expr = Expr {
                                id,
                                kind: ExprKind::Field {
                                    base: Box::new(expr),
                                    name: method,
                                },
                                span,
                            };
                        }
                    }
                    TokenKind::Int { value, suffix } => {
                        let idx_span = self.cur_span();
                        self.bump();
                        // Chained tuple access requires parentheses (G-6.31):
                        // `t.0.1` is E-PARSE-UNEXPECTED, `(t.0).0` is accepted.
                        if matches!(&expr.kind, ExprKind::TupleIndex { .. })
                            && !self.paren_exprs.contains(&expr.id)
                        {
                            self.diags.error(
                                "E-PARSE-UNEXPECTED",
                                dot_span,
                                "chained tuple indices require parentheses: write `(x.0).0`",
                            );
                        }
                        if suffix.is_some() || value > u32::MAX as u128 {
                            self.diags.error(
                                "E-PARSE-UNEXPECTED",
                                idx_span,
                                "tuple index must be a plain integer that fits in 32 bits",
                            );
                        }
                        let index = if value > u32::MAX as u128 {
                            0
                        } else {
                            value as u32
                        };
                        let span = self.span_from(start);
                        let id = self.id();
                        expr = Expr {
                            id,
                            kind: ExprKind::TupleIndex {
                                base: Box::new(expr),
                                index,
                            },
                            span,
                        };
                    }
                    TokenKind::Float { .. } => {
                        let bad = self.cur_span();
                        self.diags.error(
                            "E-PARSE-UNEXPECTED",
                            bad,
                            "unsupported tuple-index form (e.g. `x.0.1`): a float literal cannot follow `.`",
                        );
                        self.bump();
                    }
                    _ => {
                        self.expected("a field name or tuple index after `.`");
                    }
                }
            } else if self.at(Punct::LBracket) {
                let start = expr.span;
                let opener = self.cur_span();
                self.bump();
                let index = self.parse_expr(true);
                self.expect_close(Punct::RBracket, opener, "`]`");
                let span = self.span_from(start);
                let id = self.id();
                expr = Expr {
                    id,
                    kind: ExprKind::Index {
                        base: Box::new(expr),
                        index: Box::new(index),
                    },
                    span,
                };
            } else if self.at(Punct::Question) {
                let start = expr.span;
                self.bump();
                let span = self.span_from(start);
                let id = self.id();
                expr = Expr {
                    id,
                    kind: ExprKind::Try(Box::new(expr)),
                    span,
                };
            } else if self.at(Punct::Bang) {
                let start = expr.span;
                self.bump();
                let group =
                    if self.at(Punct::LParen) || self.at(Punct::LBracket) || self.at(Punct::LBrace)
                    {
                        Some(self.skip_balanced())
                    } else {
                        None
                    };
                let span = match group {
                    Some(group) => start.join(group),
                    None => self.span_from(start),
                };
                self.diags.error(
                    "E-UNSUPPORTED-MACRO",
                    span,
                    "macro invocations (`name!(...)`) are not supported in Ravum Core 1.0",
                );
                expr = self.unit_expr(span);
            } else if allow_struct
                && self.at(Punct::LBrace)
                && matches!(&expr.kind, ExprKind::Path(_))
            {
                let start = expr.span;
                let old = std::mem::replace(&mut expr.kind, ExprKind::Unit);
                if let ExprKind::Path(path) = old {
                    let opener = self.cur_span();
                    let fields = self.parse_struct_lit_fields(opener);
                    expr.kind = ExprKind::StructLit { path, fields };
                    expr.span = self.span_from(start);
                }
            } else {
                break;
            }
            if self.pos == before {
                break;
            }
        }
        expr
    }

    fn parse_call_args(&mut self, opener: Span) -> Vec<Expr> {
        let mut args = Vec::new();
        if self.at(Punct::RParen) {
            self.bump();
            return args;
        }
        loop {
            if self.fatal || self.at_eof() || self.at(Punct::RParen) {
                break;
            }
            let before = self.pos;
            args.push(self.parse_expr(true));
            if self.eat(Punct::Comma) {
                if self.at(Punct::RParen) {
                    break;
                }
                continue;
            }
            if self.pos == before {
                self.bump();
                continue;
            }
            break;
        }
        self.expect_close(Punct::RParen, opener, "`)`");
        args
    }

    fn parse_struct_lit_fields(&mut self, opener: Span) -> Vec<FieldInit> {
        self.bump(); // `{`
        let mut fields = Vec::new();
        loop {
            if self.fatal || self.at_eof() || self.at(Punct::RBrace) {
                break;
            }
            let before = self.pos;
            if self.at(Punct::DotDot) || self.at(Punct::DotDotEq) {
                let span = self.cur_span();
                self.bump();
                self.diags.error(
                    "E-UNSUPPORTED-STRUCT-UPDATE",
                    span,
                    "struct update syntax (`..base`) is not supported in Ravum Core 1.0",
                );
                if self.starts_expr() {
                    let _ = self.parse_expr(true);
                }
                break;
            }
            let name = self.parse_ident("field name");
            let value = if self.eat(Punct::Colon) {
                self.parse_expr(true)
            } else {
                // Shorthand `Foo { field }`.
                let path = Path {
                    id: self.id(),
                    root: PathRoot::Plain,
                    segments: vec![PathSegment {
                        name: name.clone(),
                        args: Vec::new(),
                    }],
                    span: name.span,
                };
                let id = self.id();
                Expr {
                    id,
                    kind: ExprKind::Path(path),
                    span: name.span,
                }
            };
            fields.push(FieldInit { name, value });
            if self.eat(Punct::Comma) {
                continue;
            }
            if self.at(Punct::RBrace) {
                break;
            }
            self.expected("`,` or `}`");
            if self.pos == before {
                self.bump();
            }
        }
        self.expect_close(Punct::RBrace, opener, "`}`");
        fields
    }

    fn parse_primary(&mut self, allow_struct: bool) -> Expr {
        if self.fatal {
            return self.unit_expr(self.cur_span());
        }
        let start = self.cur_span();
        if let Some(hash) = self.attribute_hash_pos() {
            self.report_attribute(hash);
            return self.unit_expr(self.span_from(start));
        }
        if self.at(Punct::Pipe) || self.at(Punct::PipePipe) {
            return self.parse_closure_unsupported();
        }
        if self.at(Punct::LParen) {
            return self.parse_paren_expr();
        }
        if self.at(Punct::LBracket) {
            return self.parse_array_expr();
        }
        match self.kind().clone() {
            TokenKind::Int { value, suffix } => {
                let span = self.cur_span();
                self.bump();
                let suffix = self.int_suffix(suffix, span);
                let id = self.id();
                Expr {
                    id,
                    kind: ExprKind::Int { value, suffix },
                    span: self.span_from(start),
                }
            }
            TokenKind::Float { text, suffix } => {
                let span = self.cur_span();
                self.bump();
                let suffix = self.float_suffix(suffix, span);
                let id = self.id();
                Expr {
                    id,
                    kind: ExprKind::Float { text, suffix },
                    span: self.span_from(start),
                }
            }
            TokenKind::Str(value) => {
                self.bump();
                let id = self.id();
                Expr {
                    id,
                    kind: ExprKind::Str(value),
                    span: self.span_from(start),
                }
            }
            TokenKind::Char(value) => {
                self.bump();
                let id = self.id();
                Expr {
                    id,
                    kind: ExprKind::Char(value),
                    span: self.span_from(start),
                }
            }
            // `effects` is contextual (LEX-5.5): outside a signature clause it is
            // an ordinary identifier, so it is a plain one-segment path here.
            TokenKind::Kw(Kw::Effects) => {
                let segment_span = self.cur_span();
                self.bump();
                let path_id = self.id();
                let path = Path {
                    id: path_id,
                    root: PathRoot::Plain,
                    segments: vec![PathSegment {
                        name: Ident::new("effects".to_string(), segment_span),
                        args: Vec::new(),
                    }],
                    span: self.span_from(start),
                };
                let id = self.id();
                Expr {
                    id,
                    kind: ExprKind::Path(path),
                    span: self.span_from(start),
                }
            }
            TokenKind::Kw(Kw::True) => {
                self.bump();
                let id = self.id();
                Expr {
                    id,
                    kind: ExprKind::Bool(true),
                    span: self.span_from(start),
                }
            }
            TokenKind::Kw(Kw::False) => {
                self.bump();
                let id = self.id();
                Expr {
                    id,
                    kind: ExprKind::Bool(false),
                    span: self.span_from(start),
                }
            }
            TokenKind::Kw(Kw::SelfValue) => {
                if self.peek_punct(1, Punct::ColonColon) {
                    let path = self.parse_path(true, false);
                    let id = self.id();
                    Expr {
                        id,
                        kind: ExprKind::Path(path),
                        span: self.span_from(start),
                    }
                } else {
                    self.bump();
                    let id = self.id();
                    Expr {
                        id,
                        kind: ExprKind::SelfValue,
                        span: self.span_from(start),
                    }
                }
            }
            TokenKind::Kw(Kw::Crate) | TokenKind::Kw(Kw::Super) => {
                let path = self.parse_path(true, false);
                let id = self.id();
                Expr {
                    id,
                    kind: ExprKind::Path(path),
                    span: self.span_from(start),
                }
            }
            TokenKind::Ident(_) => {
                let path = self.parse_path(true, false);
                // A struct literal is recognised in `parse_postfix` so that it
                // composes with `.field`, `[i]`, `?` and call postfixes.
                let id = self.id();
                Expr {
                    id,
                    kind: ExprKind::Path(path),
                    span: self.span_from(start),
                }
            }
            TokenKind::Kw(Kw::If) => self.parse_if_expr(),
            TokenKind::Kw(Kw::Match) => self.parse_match_expr(),
            TokenKind::Kw(Kw::While) => self.parse_while_expr(),
            TokenKind::Kw(Kw::Loop) => self.parse_loop_expr(),
            TokenKind::Kw(Kw::For) => self.parse_for_expr(),
            TokenKind::Kw(Kw::Return) => {
                self.bump();
                let value = if self.starts_expr() {
                    Some(Box::new(self.parse_expr(true)))
                } else {
                    None
                };
                let id = self.id();
                Expr {
                    id,
                    kind: ExprKind::Return(value),
                    span: self.span_from(start),
                }
            }
            TokenKind::Kw(Kw::Break) => {
                self.bump();
                let id = self.id();
                Expr {
                    id,
                    kind: ExprKind::Break,
                    span: self.span_from(start),
                }
            }
            TokenKind::Kw(Kw::Continue) => {
                self.bump();
                let id = self.id();
                Expr {
                    id,
                    kind: ExprKind::Continue,
                    span: self.span_from(start),
                }
            }
            TokenKind::Kw(Kw::Unsafe) => {
                self.bump();
                if !self.at(Punct::LBrace) {
                    self.expected("`{` after `unsafe`");
                    return self.unit_expr(self.span_from(start));
                }
                let block = self.parse_block();
                let id = self.id();
                Expr {
                    id,
                    kind: ExprKind::Unsafe(block),
                    span: self.span_from(start),
                }
            }
            TokenKind::Kw(Kw::Async) => {
                self.bump();
                self.diags.error(
                    "E-UNSUPPORTED-ASYNC",
                    start,
                    "`async` blocks are not supported in Ravum Core 1.0",
                );
                let inner = if self.starts_expr() {
                    self.parse_expr(true)
                } else {
                    self.unit_expr(self.zero_span())
                };
                let span = start.join(inner.span);
                self.unit_expr(span)
            }
            TokenKind::Kw(Kw::Move) => {
                if self.peek_punct(1, Punct::Pipe) || self.peek_punct(1, Punct::PipePipe) {
                    self.bump(); // `move`
                    return self.parse_closure_unsupported();
                }
                self.bump();
                self.diags.error(
                    "E-UNSUPPORTED-MOVE",
                    start,
                    "`move` closures are not supported in Ravum Core 1.0",
                );
                if self.starts_expr() {
                    let _ = self.parse_expr(true);
                }
                self.unit_expr(self.span_from(start))
            }
            TokenKind::Kw(Kw::SelfType) => {
                self.unexpected("`Self` is a type, not an expression");
                self.bump();
                self.unit_expr(self.span_from(start))
            }
            TokenKind::Kw(Kw::Let) => {
                self.unexpected("`let` is not allowed in expression position");
                self.bump();
                self.unit_expr(self.span_from(start))
            }
            TokenKind::Kw(Kw::Else) => {
                self.unexpected("unexpected `else` without a preceding `if`");
                self.bump();
                self.unit_expr(self.span_from(start))
            }
            TokenKind::Punct(Punct::LBrace) => {
                let block = self.parse_block();
                let id = self.id();
                Expr {
                    id,
                    kind: ExprKind::Block(block),
                    span: self.span_from(start),
                }
            }
            TokenKind::Punct(Punct::DotDot) | TokenKind::Punct(Punct::DotDotEq) => {
                // Defensive: `parse_range` normally handles prefix ranges.
                let inclusive = self.at(Punct::DotDotEq);
                self.bump();
                let end = if self.starts_expr() {
                    Some(Box::new(self.parse_binary(1, allow_struct)))
                } else {
                    None
                };
                let id = self.id();
                Expr {
                    id,
                    kind: ExprKind::Range {
                        start: None,
                        end,
                        inclusive,
                    },
                    span: self.span_from(start),
                }
            }
            _ => {
                let found = self.describe_token();
                self.unexpected(format!("expected an expression, found {found}"));
                if !self.is_expr_stopper() && !self.at_eof() {
                    self.bump();
                }
                self.unit_expr(self.span_from(start))
            }
        }
    }

    /// Closures do not exist in Core 1.0 (G-6.33). Report the canonical code
    /// once, then skip the parameter list and the body expression so the
    /// surrounding statement still parses without a cascade.
    fn parse_closure_unsupported(&mut self) -> Expr {
        let start = self.cur_span();
        self.diags.error(
            "E-UNSUPPORTED-CLOSURE",
            start,
            "closures (`|x| e`) are not supported in Ravum Core 1.0",
        );
        if self.at(Punct::PipePipe) {
            self.bump();
        } else {
            self.bump(); // opening `|`
            let mut depth = 0i32;
            while !self.at_eof() {
                match self.kind() {
                    TokenKind::Punct(Punct::LParen | Punct::LBracket | Punct::LBrace) => {
                        depth += 1;
                        self.bump();
                    }
                    TokenKind::Punct(Punct::RParen | Punct::RBracket | Punct::RBrace) => {
                        if depth == 0 {
                            break;
                        }
                        depth -= 1;
                        self.bump();
                    }
                    TokenKind::Punct(Punct::Pipe) if depth == 0 => {
                        self.bump();
                        break;
                    }
                    _ => self.bump(),
                }
            }
        }
        // Skip the closure body: one expression up to the surrounding separator.
        let mut depth = 0i32;
        while !self.at_eof() {
            match self.kind() {
                TokenKind::Punct(Punct::Semi | Punct::Comma | Punct::FatArrow) if depth == 0 => {
                    break;
                }
                TokenKind::Punct(Punct::RParen | Punct::RBracket | Punct::RBrace) if depth == 0 => {
                    break;
                }
                TokenKind::Punct(Punct::LParen | Punct::LBracket | Punct::LBrace) => {
                    depth += 1;
                    self.bump();
                }
                TokenKind::Punct(Punct::RParen | Punct::RBracket | Punct::RBrace) => {
                    depth -= 1;
                    self.bump();
                }
                _ => self.bump(),
            }
        }
        self.unit_expr(self.span_from(start))
    }

    fn parse_paren_expr(&mut self) -> Expr {
        let start = self.cur_span();
        self.bump(); // `(`
        if self.eat(Punct::RParen) {
            let id = self.id();
            return Expr {
                id,
                kind: ExprKind::Unit,
                span: self.span_from(start),
            };
        }
        let first = self.parse_expr(true);
        if self.eat(Punct::Comma) {
            let mut elems = vec![first];
            loop {
                if self.fatal || self.at_eof() || self.at(Punct::RParen) {
                    break;
                }
                let before = self.pos;
                elems.push(self.parse_expr(true));
                if self.eat(Punct::Comma) {
                    continue;
                }
                if self.pos == before {
                    self.bump();
                    continue;
                }
                break;
            }
            self.expect_close(Punct::RParen, start, "`)`");
            let id = self.id();
            Expr {
                id,
                kind: ExprKind::Tuple(elems),
                span: self.span_from(start),
            }
        } else {
            self.expect_close(Punct::RParen, start, "`)`");
            self.paren_exprs.insert(first.id);
            first
        }
    }

    fn parse_array_expr(&mut self) -> Expr {
        let start = self.cur_span();
        self.bump(); // `[`
        if self.eat(Punct::RBracket) {
            let id = self.id();
            return Expr {
                id,
                kind: ExprKind::Array(Vec::new()),
                span: self.span_from(start),
            };
        }
        let first = self.parse_expr(true);
        if self.at(Punct::Semi) {
            self.bump();
            let len = self.parse_expr(true);
            self.expect_close(Punct::RBracket, start, "`]`");
            let id = self.id();
            Expr {
                id,
                kind: ExprKind::ArrayRepeat {
                    value: Box::new(first),
                    len: Box::new(len),
                },
                span: self.span_from(start),
            }
        } else {
            let mut elems = vec![first];
            while self.eat(Punct::Comma) {
                if self.at(Punct::RBracket) || self.at_eof() {
                    break;
                }
                let before = self.pos;
                elems.push(self.parse_expr(true));
                if self.pos == before {
                    self.bump();
                }
            }
            self.expect_close(Punct::RBracket, start, "`]`");
            let id = self.id();
            Expr {
                id,
                kind: ExprKind::Array(elems),
                span: self.span_from(start),
            }
        }
    }

    fn parse_condition(&mut self) -> Expr {
        // Assignment is a statement, never an expression (G-6.8).
        self.forbid_assign += 1;
        let expr = self.parse_expr(false);
        self.forbid_assign -= 1;
        expr
    }

    fn parse_if_expr(&mut self) -> Expr {
        let start = self.cur_span();
        self.bump(); // `if`
        let cond = self.parse_condition();
        let then_block = self.parse_block();
        let mut else_branch = None;
        if self.eat_kw(Kw::Else) {
            if self.at_kw(Kw::If) {
                let nested = self.parse_if_expr();
                else_branch = Some(Box::new(nested));
            } else if self.at(Punct::LBrace) {
                let block = self.parse_block();
                let span = block.span;
                let id = self.id();
                else_branch = Some(Box::new(Expr {
                    id,
                    kind: ExprKind::Block(block),
                    span,
                }));
            } else {
                self.expected("`if` or `{` after `else`");
            }
        }
        let span = self.span_from(start);
        let id = self.id();
        Expr {
            id,
            kind: ExprKind::If {
                cond: Box::new(cond),
                then_block,
                else_branch,
            },
            span,
        }
    }

    fn parse_while_expr(&mut self) -> Expr {
        let start = self.cur_span();
        self.bump(); // `while`
        let cond = self.parse_condition();
        let body = self.parse_block();
        let span = self.span_from(start);
        let id = self.id();
        Expr {
            id,
            kind: ExprKind::While {
                cond: Box::new(cond),
                body,
            },
            span,
        }
    }

    fn parse_loop_expr(&mut self) -> Expr {
        let start = self.cur_span();
        self.bump(); // `loop`
        let body = self.parse_block();
        let span = self.span_from(start);
        let id = self.id();
        Expr {
            id,
            kind: ExprKind::Loop { body },
            span,
        }
    }

    fn parse_for_expr(&mut self) -> Expr {
        let start = self.cur_span();
        self.bump(); // `for`
        let pat = self.parse_pattern();
        if self.eat_kw(Kw::In) {
            // ok
        } else {
            self.expected("`in`");
        }
        let iter = self.parse_condition();
        let body = self.parse_block();
        let span = self.span_from(start);
        let id = self.id();
        Expr {
            id,
            kind: ExprKind::For {
                pat,
                iter: Box::new(iter),
                body,
            },
            span,
        }
    }

    fn parse_match_expr(&mut self) -> Expr {
        let start = self.cur_span();
        self.bump(); // `match`
        let scrutinee = self.parse_condition();
        let opener = self.cur_span();
        if !self.eat(Punct::LBrace) {
            self.expected("`{`");
            let span = self.span_from(start);
            let id = self.id();
            return Expr {
                id,
                kind: ExprKind::Match {
                    scrutinee: Box::new(scrutinee),
                    arms: Vec::new(),
                },
                span,
            };
        }
        let mut arms = Vec::new();
        loop {
            if self.fatal || self.at_eof() || self.at(Punct::RBrace) {
                break;
            }
            if self.is_item_start() {
                break;
            }
            let before = self.pos;
            let arm_start = self.cur_span();
            let pat = self.parse_pattern();
            if !self.eat(Punct::FatArrow) {
                self.expected("`=>`");
                self.recover_block_or_arm(true);
                if self.pos == before {
                    self.bump();
                }
                continue;
            }
            let body = self.parse_expr(true);
            let body_block_like = is_block_like(&body);
            arms.push(MatchArm {
                id: self.id(),
                pat,
                body,
                span: self.span_from(arm_start),
            });
            if self.eat(Punct::Comma) {
                continue;
            }
            if self.at(Punct::RBrace) {
                break;
            }
            if body_block_like {
                // `Pat => { ... }` arms may omit the following comma.
                continue;
            }
            self.expected("`,` or `}`");
            if self.pos == before {
                self.bump();
            }
        }
        self.expect_close(Punct::RBrace, opener, "`}` of the match");
        let span = self.span_from(start);
        let id = self.id();
        Expr {
            id,
            kind: ExprKind::Match {
                scrutinee: Box::new(scrutinee),
                arms,
            },
            span,
        }
    }

    // ---------------------------------------------------------------------
    // Literal suffix validation
    // ---------------------------------------------------------------------

    fn int_suffix(&mut self, suffix: Option<String>, span: Span) -> Option<IntTy> {
        match suffix {
            None => None,
            Some(name) => match IntTy::from_name(&name) {
                Some(ty) => Some(ty),
                None => {
                    self.diags.error(
                        "E-PARSE-BAD-SUFFIX",
                        span,
                        format!("unknown integer literal suffix `{name}`"),
                    );
                    None
                }
            },
        }
    }

    fn float_suffix(&mut self, suffix: Option<String>, span: Span) -> Option<FloatTy> {
        match suffix {
            None => None,
            Some(name) => match FloatTy::from_name(&name) {
                Some(ty) => Some(ty),
                None => {
                    self.diags.error(
                        "E-PARSE-BAD-SUFFIX",
                        span,
                        format!("unknown float literal suffix `{name}`"),
                    );
                    None
                }
            },
        }
    }
}

fn vis_span(vis: &Vis) -> Span {
    match vis {
        Vis::Public(span) => *span,
        Vis::Private => Span::dummy(),
    }
}

/// Expressions that are valid as statements without a trailing `;`.
fn is_block_like(expr: &Expr) -> bool {
    matches!(
        &expr.kind,
        ExprKind::If { .. }
            | ExprKind::Match { .. }
            | ExprKind::While { .. }
            | ExprKind::Loop { .. }
            | ExprKind::For { .. }
            | ExprKind::Block(_)
    )
}

#[cfg(test)]
mod tests {
    use super::{parse_file, ParseResult};
    use crate::ast::*;
    use std::collections::BTreeSet;
    use std::time::Instant;

    fn parse(src: &str) -> ParseResult {
        parse_file(0, src)
    }

    fn dump_diags(result: &ParseResult) -> String {
        result
            .diags
            .iter()
            .map(|d| {
                format!(
                    "{}[{}..{}]: {}",
                    d.code, d.span.start, d.span.end, d.message
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn parse_ok(src: &str) -> Program {
        let result = parse(src);
        assert!(
            !result.diags.has_errors(),
            "unexpected diagnostics for {src:?}:\n{}",
            dump_diags(&result)
        );
        result.program
    }

    fn top(program: &Program) -> &[Item] {
        &program.root.items
    }

    fn as_fn(item: &Item) -> &FnItem {
        match &item.kind {
            ItemKind::Fn(f) => f,
            other => panic!("expected a fn item, got {other:?}"),
        }
    }

    fn let_init<'a>(f: &'a FnItem, index: usize) -> &'a Expr {
        match &f.body.stmts[index].kind {
            StmtKind::Let(l) => l
                .init
                .as_ref()
                .unwrap_or_else(|| panic!("let #{index} has no initializer")),
            other => panic!("statement #{index} is not a let: {other:?}"),
        }
    }

    fn let_pat<'a>(f: &'a FnItem, index: usize) -> &'a Pat {
        match &f.body.stmts[index].kind {
            StmtKind::Let(l) => &l.pat,
            other => panic!("statement #{index} is not a let: {other:?}"),
        }
    }

    fn stmt_expr(stmt: &Stmt) -> (&Expr, bool) {
        match &stmt.kind {
            StmtKind::Expr { expr, semi } => (expr, *semi),
            other => panic!("statement is not an expression: {other:?}"),
        }
    }

    fn diags_of(src: &str) -> Vec<(String, u32, u32)> {
        parse(src)
            .diags
            .iter()
            .map(|d| (d.code.clone(), d.span.start, d.span.end))
            .collect()
    }

    fn assert_error_at(src: &str, code: &str, start: u32) {
        let diags = diags_of(src);
        assert!(
            diags.iter().any(|(c, s, _)| c == code && *s == start),
            "expected {code} at byte {start} for {src:?}, got {diags:?}"
        );
    }

    // -----------------------------------------------------------------
    // Items
    // -----------------------------------------------------------------

    #[test]
    fn unsupported_closure_reports_single_canonical_code() {
        let result = parse("fn main() { let f = |x: i32| x + 1; }");
        let codes: BTreeSet<String> = result.diags.iter().map(|d| d.code.clone()).collect();
        assert_eq!(
            codes,
            BTreeSet::from(["E-UNSUPPORTED-CLOSURE".to_string()]),
            "{}",
            dump_diags(&result)
        );
        assert_eq!(result.diags.error_count(), 1, "{}", dump_diags(&result));
    }

    #[test]
    fn raw_pointer_type_is_e_unsupported_unsafe() {
        let result = parse("fn f(p: *const i32) {}\nfn g(p: *mut u8) {}\n");
        let codes: BTreeSet<String> = result.diags.iter().map(|d| d.code.clone()).collect();
        assert_eq!(
            codes,
            BTreeSet::from(["E-UNSUPPORTED-UNSAFE".to_string()]),
            "{}",
            dump_diags(&result)
        );
        assert_eq!(result.diags.error_count(), 2, "{}", dump_diags(&result));
    }

    #[test]
    fn attribute_reports_only_e_unsupported_attr() {
        let result = parse("#[test]\nfn main() {}\n");
        let codes: BTreeSet<String> = result.diags.iter().map(|d| d.code.clone()).collect();
        assert_eq!(
            codes,
            BTreeSet::from(["E-UNSUPPORTED-ATTR".to_string()]),
            "{}",
            dump_diags(&result)
        );
    }

    #[test]
    fn assignment_in_condition_is_e_parse_unexpected() {
        let result = parse("fn main() { let mut x = 1; if (x = 1) { } }");
        let codes: BTreeSet<String> = result.diags.iter().map(|d| d.code.clone()).collect();
        assert_eq!(
            codes,
            BTreeSet::from(["E-PARSE-UNEXPECTED".to_string()]),
            "{}",
            dump_diags(&result)
        );
    }

    #[test]
    fn chained_comparison_is_rejected_but_parenthesized_is_accepted() {
        let result = parse("fn chained(a: i32, b: i32, c: i32) -> bool { let x = a < b < c; x }");
        let codes: BTreeSet<String> = result.diags.iter().map(|d| d.code.clone()).collect();
        assert_eq!(
            codes,
            BTreeSet::from(["E-PARSE-UNEXPECTED".to_string()]),
            "{}",
            dump_diags(&result)
        );

        let ok = parse_ok("fn f(a: bool, b: bool, c: bool) -> bool { (a == b) == c }");
        assert_ne!(ok.root.id, 0);
    }

    #[test]
    fn misplaced_effects_is_a_single_parse_expected() {
        let result = parse("fn f() effects(io) -> i32 { return 0; }\nfn main() {}\n");
        let codes: BTreeSet<String> = result.diags.iter().map(|d| d.code.clone()).collect();
        assert_eq!(
            codes,
            BTreeSet::from(["E-PARSE-EXPECTED".to_string()]),
            "{}",
            dump_diags(&result)
        );
    }

    #[test]
    fn multi_error_recovery_reports_both_errors() {
        let src = "const C = 1;\n\nfn chained(a: i32, b: i32, c: i32) -> bool {\n    a < b < c\n}\n\nfn main() {\n}\n";
        let result = parse(src);
        let codes: BTreeSet<String> = result.diags.iter().map(|d| d.code.clone()).collect();
        assert_eq!(
            codes,
            BTreeSet::from([
                "E-PARSE-EXPECTED".to_string(),
                "E-PARSE-UNEXPECTED".to_string(),
            ]),
            "{}",
            dump_diags(&result)
        );
        assert_eq!(top(&result.program).len(), 3);
    }

    #[test]
    fn chained_tuple_index_requires_parentheses() {
        let result = parse("fn f(t: (i32, i32)) { let x = t.0.1; }");
        let codes: BTreeSet<String> = result.diags.iter().map(|d| d.code.clone()).collect();
        assert_eq!(
            codes,
            BTreeSet::from(["E-PARSE-UNEXPECTED".to_string()]),
            "{}",
            dump_diags(&result)
        );

        let ok = parse_ok("fn f(t: (i32, i32)) { let x = (t.0).0; }");
        assert_ne!(ok.root.id, 0);
    }

    #[test]
    fn range_after_integer_is_not_a_float() {
        let result = parse("fn main() { for i in 0..10 { let x = i; } }");
        assert!(!result.diags.has_errors(), "{}", dump_diags(&result));
    }

    #[test]
    fn empty_and_comment_only_files() {
        for src in ["", "// nothing\n", "/* nothing */\n"] {
            let program = parse_ok(src);
            assert!(program.root.items.is_empty(), "{src:?}");
            assert_ne!(program.root.id, 0);
            assert_eq!(program.root.id >> 24, 0);
        }
    }

    #[test]
    fn modules_and_use_items() {
        let src = "\
mod foo;
mod bar { fn inner() {} }
use crate::foo::Bar;
use super::x;
use self::y;
";
        let program = parse_ok(src);
        let items = top(&program);
        assert_eq!(items.len(), 5);
        match &items[0].kind {
            ItemKind::ModDecl(decl) => assert_eq!(decl.name.name, "foo"),
            other => panic!("expected mod decl, got {other:?}"),
        }
        match &items[1].kind {
            ItemKind::Mod(module) => {
                assert_eq!(module.name.as_ref().unwrap().name, "bar");
                assert_eq!(module.items.len(), 1);
                assert_eq!(as_fn(&module.items[0]).name.name, "inner");
            }
            other => panic!("expected inline mod, got {other:?}"),
        }
        match &items[2].kind {
            ItemKind::Use(u) => {
                assert_eq!(u.path.root, PathRoot::Crate);
                assert_eq!(u.path.display(), "crate::foo::Bar");
            }
            other => panic!("expected use, got {other:?}"),
        }
        match &items[3].kind {
            ItemKind::Use(u) => assert_eq!(u.path.root, PathRoot::Super),
            other => panic!("expected use, got {other:?}"),
        }
        match &items[4].kind {
            ItemKind::Use(u) => assert_eq!(u.path.root, PathRoot::SelfMod),
            other => panic!("expected use, got {other:?}"),
        }
    }

    #[test]
    fn function_signature_and_body() {
        let src = "\
pub fn compute<T, U>(a: T, mut b: u32) -> Vec<T> effects(io, alloc) {
    let x: i32 = 1;
    let y: i32;
    return y;
}
";
        let program = parse_ok(src);
        let item = &top(&program)[0];
        assert!(item.vis.is_public());
        let f = as_fn(item);
        assert_eq!(f.name.name, "compute");
        assert!(!f.is_test);
        assert_eq!(f.generics.len(), 2);
        assert_eq!(f.generics[0].name.name, "T");
        assert_eq!(f.generics[1].name.name, "U");
        assert_eq!(f.params.len(), 2);
        match &f.params[0].kind {
            ParamKind::Binding { name, ty } => {
                assert_eq!(name.name, "a");
                assert!(matches!(&ty.kind, TypeKind::Path(p) if p.display() == "T"));
            }
            other => panic!("expected binding, got {other:?}"),
        }
        match &f.params[1].kind {
            // `mut` on a parameter is accepted and ignored by the frozen AST.
            ParamKind::Binding { name, ty } => {
                assert_eq!(name.name, "b");
                assert!(matches!(&ty.kind, TypeKind::Path(p) if p.display() == "u32"));
            }
            other => panic!("expected binding, got {other:?}"),
        }
        match &f.ret.kind {
            TypeKind::Path(path) => {
                assert_eq!(path.display(), "Vec");
                assert_eq!(path.segments[0].args.len(), 1);
            }
            other => panic!("expected Vec<T> return, got {other:?}"),
        }
        let effects = f.effects.as_ref().expect("effects clause");
        assert_eq!(
            effects.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            vec!["io", "alloc"]
        );
        assert_eq!(f.body.stmts.len(), 3);
        assert!(f.body.tail.is_none());
        match &f.body.stmts[1].kind {
            StmtKind::Let(l) => {
                assert!(l.init.is_none());
                assert!(matches!(&l.ty.as_ref().unwrap().kind, TypeKind::Path(_)));
            }
            other => panic!("expected typed let without init, got {other:?}"),
        }
        let (expr, semi) = stmt_expr(&f.body.stmts[2]);
        assert!(semi);
        assert!(matches!(&expr.kind, ExprKind::Return(Some(_))));
    }

    #[test]
    fn test_functions_and_unit_return() {
        let program = parse_ok("test fn works() { assert_eq(1, 1); }");
        let f = as_fn(&top(&program)[0]);
        assert!(f.is_test);
        assert_eq!(f.name.name, "works");
        assert!(f.params.is_empty());
        assert!(matches!(f.ret.kind, TypeKind::Unit));
        assert_eq!(f.body.stmts.len(), 1);
    }

    #[test]
    fn struct_items_unit_named_and_visibility() {
        let src = "struct Unit;\npub struct Point<T> { pub x: T, y: T }\n";
        let program = parse_ok(src);
        let items = top(&program);
        match &items[0].kind {
            ItemKind::Struct(s) => {
                assert_eq!(s.name.name, "Unit");
                assert!(s.fields.is_empty());
                assert!(s.generics.is_empty());
            }
            other => panic!("expected unit struct, got {other:?}"),
        }
        assert!(items[1].vis.is_public());
        match &items[1].kind {
            ItemKind::Struct(s) => {
                assert_eq!(s.name.name, "Point");
                assert_eq!(s.generics.len(), 1);
                assert_eq!(s.fields.len(), 2);
                assert!(s.fields[0].vis.is_public());
                assert_eq!(s.fields[0].name.name, "x");
                assert!(!s.fields[1].vis.is_public());
                assert_eq!(s.fields[1].name.name, "y");
            }
            other => panic!("expected struct, got {other:?}"),
        }
    }

    #[test]
    fn enum_variants_all_forms() {
        let src = "pub enum Shape<T> { Empty, Circle(T), Rect { w: T, h: T }, pub Unit }";
        let program = parse_ok(src);
        let item = &top(&program)[0];
        assert!(item.vis.is_public());
        let e = match &item.kind {
            ItemKind::Enum(e) => e,
            other => panic!("expected enum, got {other:?}"),
        };
        assert_eq!(e.name.name, "Shape");
        assert_eq!(e.generics.len(), 1);
        assert_eq!(e.variants.len(), 4);
        assert_eq!(e.variants[0].name.name, "Empty");
        assert!(matches!(e.variants[0].fields, VariantDefFields::Unit));
        match &e.variants[1].fields {
            VariantDefFields::Tuple(tys) => {
                assert_eq!(tys.len(), 1);
                assert!(matches!(&tys[0].kind, TypeKind::Path(p) if p.display() == "T"));
            }
            other => panic!("expected tuple variant, got {other:?}"),
        }
        match &e.variants[2].fields {
            VariantDefFields::Struct(fields) => {
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].name.name, "w");
                assert_eq!(fields[1].name.name, "h");
            }
            other => panic!("expected struct variant, got {other:?}"),
        }
        assert!(matches!(e.variants[3].fields, VariantDefFields::Unit));
    }

    #[test]
    fn impl_generics_receivers_and_associated_fns() {
        let src = "\
impl<T> Vec<T> {
    fn new() -> T { x }
    fn get(&self) -> T { x }
    fn set(&mut self, v: T) { }
}
";
        let program = parse_ok(src);
        let imp = match &top(&program)[0].kind {
            ItemKind::Impl(i) => i,
            other => panic!("expected impl, got {other:?}"),
        };
        assert_eq!(imp.generics.len(), 1);
        match &imp.self_ty.kind {
            TypeKind::Path(p) => {
                assert_eq!(p.display(), "Vec");
                assert_eq!(p.segments[0].args.len(), 1);
            }
            other => panic!("expected path self type, got {other:?}"),
        }
        assert_eq!(imp.items.len(), 3);
        let new = as_fn(&imp.items[0]);
        assert_eq!(new.name.name, "new");
        assert!(new.params.is_empty());
        let get = as_fn(&imp.items[1]);
        assert!(matches!(
            get.params[0].kind,
            ParamKind::Receiver(Receiver::Shared)
        ));
        let set = as_fn(&imp.items[2]);
        assert!(matches!(
            set.params[0].kind,
            ParamKind::Receiver(Receiver::Exclusive)
        ));
        match &set.params[1].kind {
            ParamKind::Binding { name, .. } => assert_eq!(name.name, "v"),
            other => panic!("expected binding, got {other:?}"),
        }
    }

    #[test]
    fn const_item() {
        let program = parse_ok("const N: i32 = 10;");
        let item = &top(&program)[0];
        let c = match &item.kind {
            ItemKind::Const(c) => c,
            other => panic!("expected const, got {other:?}"),
        };
        assert_eq!(c.name.name, "N");
        assert!(matches!(&c.ty.kind, TypeKind::Path(p) if p.display() == "i32"));
        match &c.value.kind {
            ExprKind::Int { value, suffix } => {
                assert_eq!(*value, 10);
                assert!(suffix.is_none());
            }
            other => panic!("expected int literal, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------
    // Types
    // -----------------------------------------------------------------

    #[test]
    fn all_type_forms() {
        let src = "fn types(a: &i32, b: &mut i32, c: [i32; 3], d: [i32], e: (i32, u8), f: (), g: _, h: Self, i: Vec<i32>) {}";
        let program = parse_ok(src);
        let f = as_fn(&top(&program)[0]);
        assert_eq!(f.params.len(), 9);
        let ty = |n: usize| match &f.params[n].kind {
            ParamKind::Binding { ty, .. } => ty,
            other => panic!("expected binding, got {other:?}"),
        };
        assert!(matches!(&ty(0).kind, TypeKind::Ref { mutable: false, .. }));
        assert!(matches!(&ty(1).kind, TypeKind::Ref { mutable: true, .. }));
        match &ty(2).kind {
            TypeKind::Array { elem, len } => {
                assert!(matches!(&elem.kind, TypeKind::Path(_)));
                assert!(matches!(&len.kind, ExprKind::Int { value: 3, .. }));
            }
            other => panic!("expected array, got {other:?}"),
        }
        assert!(matches!(&ty(3).kind, TypeKind::Slice(_)));
        match &ty(4).kind {
            TypeKind::Tuple(parts) => assert_eq!(parts.len(), 2),
            other => panic!("expected tuple, got {other:?}"),
        }
        assert!(matches!(&ty(5).kind, TypeKind::Unit));
        assert!(matches!(&ty(6).kind, TypeKind::Infer));
        assert!(matches!(&ty(7).kind, TypeKind::SelfType));
        match &ty(8).kind {
            TypeKind::Path(p) => {
                assert_eq!(p.display(), "Vec");
                assert_eq!(p.segments[0].args.len(), 1);
            }
            other => panic!("expected generic path, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------
    // Expressions
    // -----------------------------------------------------------------

    #[test]
    fn literals_paths_calls_methods_fields_and_indices() {
        let src = "\
impl S {
    fn m(&self) {
        let a = self;
        let b = self.x;
        let c = 1u8;
        let d = 2.5f32;
        let e = true;
        let f = 'z';
        let g = \"hi\";
        let h = foo::bar(1);
        let i = a.method(1, 2);
        let j = pair.0;
        let k = arr[idx];
    }
}
";
        let program = parse_ok(src);
        let imp = match &top(&program)[0].kind {
            ItemKind::Impl(i) => i,
            other => panic!("expected impl, got {other:?}"),
        };
        let f = as_fn(&imp.items[0]);
        assert!(matches!(let_init(f, 0).kind, ExprKind::SelfValue));
        assert!(matches!(&let_init(f, 1).kind, ExprKind::Field { .. }));
        match &let_init(f, 2).kind {
            ExprKind::Int { value, suffix } => {
                assert_eq!(*value, 1);
                assert_eq!(*suffix, Some(IntTy::U8));
            }
            other => panic!("expected int, got {other:?}"),
        }
        match &let_init(f, 3).kind {
            ExprKind::Float { suffix, .. } => assert_eq!(*suffix, Some(FloatTy::F32)),
            other => panic!("expected float, got {other:?}"),
        }
        assert!(matches!(let_init(f, 4).kind, ExprKind::Bool(true)));
        assert!(matches!(let_init(f, 5).kind, ExprKind::Char('z')));
        assert!(matches!(&let_init(f, 6).kind, ExprKind::Str(s) if s == "hi"));
        match &let_init(f, 7).kind {
            ExprKind::Call { callee, args } => {
                assert!(matches!(&callee.kind, ExprKind::Path(p) if p.display() == "foo::bar"));
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected call, got {other:?}"),
        }
        match &let_init(f, 8).kind {
            ExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method.name, "method");
                assert_eq!(args.len(), 2);
            }
            other => panic!("expected method call, got {other:?}"),
        }
        match &let_init(f, 9).kind {
            ExprKind::TupleIndex { index, .. } => assert_eq!(*index, 0),
            other => panic!("expected tuple index, got {other:?}"),
        }
        assert!(matches!(&let_init(f, 10).kind, ExprKind::Index { .. }));
    }

    #[test]
    fn binary_precedence_follows_design() {
        let program = parse_ok("fn f() { let x = 1 + 2 * 3; }");
        let f = as_fn(&top(&program)[0]);
        match &let_init(f, 0).kind {
            ExprKind::Binary { op, lhs, rhs } => {
                assert_eq!(*op, BinOp::Add);
                assert!(matches!(lhs.kind, ExprKind::Int { value: 1, .. }));
                match &rhs.kind {
                    ExprKind::Binary { op, lhs, rhs } => {
                        assert_eq!(*op, BinOp::Mul);
                        assert!(matches!(lhs.kind, ExprKind::Int { value: 2, .. }));
                        assert!(matches!(rhs.kind, ExprKind::Int { value: 3, .. }));
                    }
                    other => panic!("expected 2 * 3, got {other:?}"),
                }
            }
            other => panic!("expected binary expression, got {other:?}"),
        }

        let program = parse_ok("fn f() { let x = a || b && c == d; }");
        let f = as_fn(&top(&program)[0]);
        match &let_init(f, 0).kind {
            ExprKind::Binary { op, rhs, .. } => {
                assert_eq!(*op, BinOp::Or);
                assert!(matches!(&rhs.kind, ExprKind::Binary { op: BinOp::And, .. }));
            }
            other => panic!("expected || at the root, got {other:?}"),
        }
    }

    #[test]
    fn cast_and_assignment_operators() {
        let src = "fn f() { x = 1 as u8; x += 2; x >>= 1; }";
        let program = parse_ok(src);
        let f = as_fn(&top(&program)[0]);
        match &f.body.stmts[0].kind {
            StmtKind::Expr { expr, semi } => {
                assert!(semi);
                match &expr.kind {
                    ExprKind::Assign { target, value } => {
                        assert!(matches!(&target.kind, ExprKind::Path(_)));
                        assert!(matches!(&value.kind, ExprKind::Cast { .. }));
                    }
                    other => panic!("expected assignment, got {other:?}"),
                }
            }
            other => panic!("expected expression statement, got {other:?}"),
        }
        match &stmt_expr(&f.body.stmts[1]).0.kind {
            ExprKind::AssignOp { op, .. } => assert_eq!(*op, BinOp::Add),
            other => panic!("expected +=, got {other:?}"),
        }
        // `>>=` is lexed as one token and split internally; it must stay Shr.
        match &stmt_expr(&f.body.stmts[2]).0.kind {
            ExprKind::AssignOp { op, .. } => assert_eq!(*op, BinOp::Shr),
            other => panic!("expected >>=, got {other:?}"),
        }
    }

    #[test]
    fn if_else_chain_is_nested_expressions() {
        let program = parse_ok("fn f() { let r = if a { 1 } else if b { 2 } else { 3 }; }");
        let f = as_fn(&top(&program)[0]);
        match &let_init(f, 0).kind {
            ExprKind::If { else_branch, .. } => {
                let Some(else_branch) = else_branch else {
                    panic!("missing else branch");
                };
                match &else_branch.kind {
                    ExprKind::If { else_branch, .. } => {
                        let Some(inner) = else_branch else {
                            panic!("missing final else");
                        };
                        assert!(matches!(&inner.kind, ExprKind::Block(_)));
                    }
                    other => panic!("expected else-if, got {other:?}"),
                }
            }
            other => panic!("expected if expression, got {other:?}"),
        }
    }

    #[test]
    fn match_arms_and_pattern_forms() {
        let src =
            "fn f() { let r = match x { Foo::A => 1, Foo::B(y) => y, Foo { f } => f, _ => 0, }; }";
        let program = parse_ok(src);
        let f = as_fn(&top(&program)[0]);
        let arms = match &let_init(f, 0).kind {
            ExprKind::Match { arms, .. } => arms,
            other => panic!("expected match, got {other:?}"),
        };
        assert_eq!(arms.len(), 4);
        assert!(matches!(&arms[0].pat.kind, PatKind::Path(_)));
        match &arms[1].pat.kind {
            PatKind::Variant { fields, .. } => {
                assert!(matches!(fields, VariantPatFields::Tuple(p) if p.len() == 1));
            }
            other => panic!("expected tuple variant pattern, got {other:?}"),
        }
        match &arms[2].pat.kind {
            PatKind::Variant { fields, .. } => match fields {
                VariantPatFields::Struct(fields) => {
                    assert_eq!(fields.len(), 1);
                    assert_eq!(fields[0].name.name, "f");
                }
                other => panic!("expected struct fields, got {other:?}"),
            },
            other => panic!("expected struct variant pattern, got {other:?}"),
        }
        assert!(matches!(arms[3].pat.kind, PatKind::Wild));
    }

    #[test]
    fn block_tuple_array_struct_literal_expressions() {
        let src = "fn f() { let b = { 1 }; let t = (1, 2); let u = (); let arr = [1, 2]; let rep = [0; 4]; let p = Point { x: 1, y }; }";
        let program = parse_ok(src);
        let f = as_fn(&top(&program)[0]);
        match &let_init(f, 0).kind {
            ExprKind::Block(block) => assert!(block.tail.is_some()),
            other => panic!("expected block, got {other:?}"),
        }
        match &let_init(f, 1).kind {
            ExprKind::Tuple(items) => assert_eq!(items.len(), 2),
            other => panic!("expected tuple, got {other:?}"),
        }
        assert!(matches!(let_init(f, 2).kind, ExprKind::Unit));
        match &let_init(f, 3).kind {
            ExprKind::Array(items) => assert_eq!(items.len(), 2),
            other => panic!("expected array, got {other:?}"),
        }
        match &let_init(f, 4).kind {
            ExprKind::ArrayRepeat { value, len } => {
                assert!(matches!(value.kind, ExprKind::Int { value: 0, .. }));
                assert!(matches!(len.kind, ExprKind::Int { value: 4, .. }));
            }
            other => panic!("expected array repeat, got {other:?}"),
        }
        match &let_init(f, 5).kind {
            ExprKind::StructLit { fields, path } => {
                assert_eq!(path.display(), "Point");
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].name.name, "x");
                // Shorthand `y` becomes a path expression.
                assert!(matches!(&fields[1].value.kind, ExprKind::Path(p) if p.display() == "y"));
            }
            other => panic!("expected struct literal, got {other:?}"),
        }
    }

    #[test]
    fn unsafe_block_and_extern_block_parse() {
        let program = parse_ok(
            "extern \"C\" {\n    fn abs(x: i32) -> i32;\n}\nfn main() { let v = unsafe { abs(-1) }; }",
        );
        let items = top(&program);
        match &items[0].kind {
            ItemKind::Extern(block) => {
                assert_eq!(block.abi.name, "C");
                assert_eq!(block.fns.len(), 1);
                assert_eq!(block.fns[0].name.name, "abs");
                assert_eq!(block.fns[0].params.len(), 1);
            }
            other => panic!("expected an extern block, got {other:?}"),
        }
        let main = as_fn(&items[1]);
        assert!(matches!(&let_init(main, 0).kind, ExprKind::Unsafe(_)));
    }

    #[test]
    fn ranges_try_and_return() {
        let program =
            parse_ok("fn f() -> u8 { let r = 0..n; let s = 0..=n; let t = g()?; return t; }");
        let f = as_fn(&top(&program)[0]);
        match &let_init(f, 0).kind {
            ExprKind::Range {
                start,
                end,
                inclusive,
            } => {
                assert!(start.is_some() && end.is_some());
                assert!(!inclusive);
            }
            other => panic!("expected exclusive range, got {other:?}"),
        }
        match &let_init(f, 1).kind {
            ExprKind::Range { inclusive, .. } => assert!(inclusive),
            other => panic!("expected inclusive range, got {other:?}"),
        }
        assert!(matches!(&let_init(f, 2).kind, ExprKind::Try(_)));
        let (expr, semi) = stmt_expr(&f.body.stmts[3]);
        assert!(semi);
        match &expr.kind {
            ExprKind::Return(Some(value)) => {
                assert!(matches!(&value.kind, ExprKind::Path(_)));
            }
            other => panic!("expected return with value, got {other:?}"),
        }
    }

    #[test]
    fn loop_expressions() {
        let src = "fn f() { while i < 10 { i += 1; } loop { break; } for x in 0..3 { continue; } }";
        let program = parse_ok(src);
        let f = as_fn(&top(&program)[0]);
        // `for` is the final expression of the block, so it is the block tail
        // (a trailing block-like expression is the block's value).
        assert_eq!(f.body.stmts.len(), 2);
        let for_expr = f.body.tail.as_ref().expect("for must be the block tail");
        let (while_expr, semi) = stmt_expr(&f.body.stmts[0]);
        assert!(!semi, "block-like statements do not need `;`");
        match &while_expr.kind {
            ExprKind::While { cond, body } => {
                assert!(matches!(&cond.kind, ExprKind::Binary { op: BinOp::Lt, .. }));
                assert_eq!(body.stmts.len(), 1);
            }
            other => panic!("expected while, got {other:?}"),
        }
        match &stmt_expr(&f.body.stmts[1]).0.kind {
            ExprKind::Loop { body } => {
                let (break_expr, _) = stmt_expr(&body.stmts[0]);
                assert!(matches!(break_expr.kind, ExprKind::Break));
            }
            other => panic!("expected loop, got {other:?}"),
        }
        match &for_expr.kind {
            ExprKind::For { pat, iter, body } => {
                assert!(matches!(&pat.kind, PatKind::Binding { mutable: false, .. }));
                match &iter.kind {
                    ExprKind::Range {
                        start,
                        end,
                        inclusive,
                    } => {
                        assert!(start.is_some() && end.is_some());
                        assert!(!inclusive);
                    }
                    other => panic!("expected range iterator, got {other:?}"),
                }
                let (continue_expr, _) = stmt_expr(&body.stmts[0]);
                assert!(matches!(continue_expr.kind, ExprKind::Continue));
            }
            other => panic!("expected for, got {other:?}"),
        }
    }

    #[test]
    fn pattern_forms() {
        let src = "fn f() { let _ = 1; let mut x = 2; let (a, b) = p; let E::V(q) = e; let E::S { f: g, h } = e; }";
        let program = parse_ok(src);
        let f = as_fn(&top(&program)[0]);
        assert!(matches!(let_pat(f, 0).kind, PatKind::Wild));
        assert!(matches!(
            let_pat(f, 1).kind,
            PatKind::Binding { mutable: true, .. }
        ));
        match &let_pat(f, 2).kind {
            PatKind::Tuple(pats) => assert_eq!(pats.len(), 2),
            other => panic!("expected tuple pattern, got {other:?}"),
        }
        match &let_pat(f, 3).kind {
            PatKind::Variant { fields, .. } => match fields {
                VariantPatFields::Tuple(pats) => assert_eq!(pats.len(), 1),
                other => panic!("expected tuple variant pattern, got {other:?}"),
            },
            other => panic!("expected variant pattern, got {other:?}"),
        }
        match &let_pat(f, 4).kind {
            PatKind::Variant { fields, .. } => match fields {
                VariantPatFields::Struct(fields) => {
                    assert_eq!(fields.len(), 2);
                    assert_eq!(fields[0].name.name, "f");
                    assert!(
                        matches!(&fields[0].pat.kind, PatKind::Binding { name, .. } if name.name == "g")
                    );
                    // Shorthand `h` binds `h`.
                    assert!(
                        matches!(&fields[1].pat.kind, PatKind::Binding { name, .. } if name.name == "h")
                    );
                }
                other => panic!("expected struct pattern fields, got {other:?}"),
            },
            other => panic!("expected struct variant pattern, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------
    // Node ids
    // -----------------------------------------------------------------

    const RICH: &str = r#"
const N: i32 = 10;

pub struct Point<T> {
    pub x: T,
    y: T,
}

pub enum Shape<T> {
    Empty,
    Circle(T),
    Rect { w: T, h: T },
}

impl<T> Point<T> {
    pub fn new(x: T, mut y: T) -> Point<T> {
        Point { x, y }
    }

    fn norm(&self) -> f64 {
        let mut total: f64 = 0.0;
        while total < 10.0 {
            total += 1.0;
        }
        for i in 0..3 {
            let _ = i;
        }
        if self.x == self.x {
            total
        } else {
            0.0
        }
    }
}

fn apply<T, U>(a: &T, b: &mut U, c: [i32; 3], d: [i32], e: (i32, u8), g: _, h: Self) -> Result<T, U> effects(io, alloc) {
    let r = [1, 2, 3];
    let s = [0; 3];
    let t = match a {
        _ => 1,
    };
    let u = foo::bar(a)?;
    return u;
}

test fn works() {
    assert_eq(1, 1);
}

mod inner {
    use crate::N;
}
"#;

    fn collect_module(module: &Module, out: &mut Vec<NodeId>) {
        out.push(module.id);
        for item in &module.items {
            collect_item(item, out);
        }
    }

    fn collect_item(item: &Item, out: &mut Vec<NodeId>) {
        out.push(item.id);
        match &item.kind {
            ItemKind::Fn(f) => {
                for generic in &f.generics {
                    out.push(generic.id);
                }
                for param in &f.params {
                    out.push(param.id);
                    if let ParamKind::Binding { ty, .. } = &param.kind {
                        collect_type(ty, out);
                    }
                }
                collect_type(&f.ret, out);
                collect_block(&f.body, out);
            }
            ItemKind::Struct(s) => {
                for generic in &s.generics {
                    out.push(generic.id);
                }
                for field in &s.fields {
                    out.push(field.id);
                    collect_type(&field.ty, out);
                }
            }
            ItemKind::Enum(e) => {
                for generic in &e.generics {
                    out.push(generic.id);
                }
                for variant in &e.variants {
                    out.push(variant.id);
                    match &variant.fields {
                        VariantDefFields::Unit => {}
                        VariantDefFields::Tuple(tys) => {
                            for ty in tys {
                                collect_type(ty, out);
                            }
                        }
                        VariantDefFields::Struct(fields) => {
                            for field in fields {
                                out.push(field.id);
                                collect_type(&field.ty, out);
                            }
                        }
                    }
                }
            }
            ItemKind::Impl(imp) => {
                for generic in &imp.generics {
                    out.push(generic.id);
                }
                collect_type(&imp.self_ty, out);
                for sub in &imp.items {
                    collect_item(sub, out);
                }
            }
            ItemKind::Const(c) => {
                collect_type(&c.ty, out);
                collect_expr(&c.value, out);
            }
            ItemKind::Use(u) => collect_path(&u.path, out),
            ItemKind::Mod(module) => collect_module(module, out),
            ItemKind::ModDecl(_) => {}
            ItemKind::Extern(block) => {
                for f in &block.fns {
                    out.push(f.id);
                    for param in &f.params {
                        out.push(param.id);
                        if let ParamKind::Binding { ty, .. } = &param.kind {
                            collect_type(ty, out);
                        }
                    }
                    collect_type(&f.ret, out);
                }
            }
        }
    }

    fn collect_type(ty: &Type, out: &mut Vec<NodeId>) {
        out.push(ty.id);
        match &ty.kind {
            TypeKind::Path(path) => collect_path(path, out),
            TypeKind::Ref { inner, .. } => collect_type(inner, out),
            TypeKind::Tuple(parts) => {
                for part in parts {
                    collect_type(part, out);
                }
            }
            TypeKind::Array { elem, len } => {
                collect_type(elem, out);
                collect_expr(len, out);
            }
            TypeKind::Slice(elem) => collect_type(elem, out),
            TypeKind::Unit | TypeKind::Infer | TypeKind::SelfType => {}
        }
    }

    fn collect_path(path: &Path, out: &mut Vec<NodeId>) {
        out.push(path.id);
        for segment in &path.segments {
            for arg in &segment.args {
                collect_type(arg, out);
            }
        }
    }

    fn collect_block(block: &Block, out: &mut Vec<NodeId>) {
        out.push(block.id);
        for stmt in &block.stmts {
            out.push(stmt.id);
            match &stmt.kind {
                StmtKind::Let(l) => {
                    collect_pat(&l.pat, out);
                    if let Some(ty) = &l.ty {
                        collect_type(ty, out);
                    }
                    if let Some(init) = &l.init {
                        collect_expr(init, out);
                    }
                }
                StmtKind::Expr { expr, .. } => collect_expr(expr, out),
            }
        }
        if let Some(tail) = &block.tail {
            collect_expr(tail, out);
        }
    }

    fn collect_pat(pat: &Pat, out: &mut Vec<NodeId>) {
        out.push(pat.id);
        match &pat.kind {
            PatKind::Wild | PatKind::Binding { .. } | PatKind::Lit(_) => {}
            PatKind::Tuple(pats) => {
                for sub in pats {
                    collect_pat(sub, out);
                }
            }
            PatKind::Path(path) => collect_path(path, out),
            PatKind::Variant { path, fields } => {
                collect_path(path, out);
                match fields {
                    VariantPatFields::Tuple(pats) => {
                        for sub in pats {
                            collect_pat(sub, out);
                        }
                    }
                    VariantPatFields::Struct(fields) => {
                        for field in fields {
                            collect_pat(&field.pat, out);
                        }
                    }
                }
            }
        }
    }

    fn collect_expr(expr: &Expr, out: &mut Vec<NodeId>) {
        out.push(expr.id);
        match &expr.kind {
            ExprKind::Unit
            | ExprKind::Int { .. }
            | ExprKind::Float { .. }
            | ExprKind::Bool(_)
            | ExprKind::Char(_)
            | ExprKind::Str(_)
            | ExprKind::SelfValue
            | ExprKind::Break
            | ExprKind::Continue => {}
            ExprKind::Path(path) => collect_path(path, out),
            ExprKind::Call { callee, args } => {
                collect_expr(callee, out);
                for arg in args {
                    collect_expr(arg, out);
                }
            }
            ExprKind::MethodCall { receiver, args, .. } => {
                collect_expr(receiver, out);
                for arg in args {
                    collect_expr(arg, out);
                }
            }
            ExprKind::Field { base, .. } | ExprKind::TupleIndex { base, .. } => {
                collect_expr(base, out);
            }
            ExprKind::Index { base, index } => {
                collect_expr(base, out);
                collect_expr(index, out);
            }
            ExprKind::Unary { expr, .. } => collect_expr(expr, out),
            ExprKind::Binary { lhs, rhs, .. } => {
                collect_expr(lhs, out);
                collect_expr(rhs, out);
            }
            ExprKind::Assign { target, value } | ExprKind::AssignOp { target, value, .. } => {
                collect_expr(target, out);
                collect_expr(value, out);
            }
            ExprKind::Cast { expr, ty } => {
                collect_expr(expr, out);
                collect_type(ty, out);
            }
            ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => {
                collect_expr(cond, out);
                collect_block(then_block, out);
                if let Some(branch) = else_branch {
                    collect_expr(branch, out);
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                collect_expr(scrutinee, out);
                for arm in arms {
                    out.push(arm.id);
                    collect_pat(&arm.pat, out);
                    collect_expr(&arm.body, out);
                }
            }
            ExprKind::While { cond, body } => {
                collect_expr(cond, out);
                collect_block(body, out);
            }
            ExprKind::Loop { body } => collect_block(body, out),
            ExprKind::For { pat, iter, body } => {
                collect_pat(pat, out);
                collect_expr(iter, out);
                collect_block(body, out);
            }
            ExprKind::Block(block) => collect_block(block, out),
            ExprKind::Unsafe(block) => collect_block(block, out),
            ExprKind::Tuple(items) | ExprKind::Array(items) => {
                for item in items {
                    collect_expr(item, out);
                }
            }
            ExprKind::ArrayRepeat { value, len } => {
                collect_expr(value, out);
                collect_expr(len, out);
            }
            ExprKind::StructLit { path, fields } => {
                collect_path(path, out);
                for field in fields {
                    collect_expr(&field.value, out);
                }
            }
            ExprKind::Range { start, end, .. } => {
                if let Some(start) = start {
                    collect_expr(start, out);
                }
                if let Some(end) = end {
                    collect_expr(end, out);
                }
            }
            ExprKind::Try(inner) => collect_expr(inner, out),
            ExprKind::Return(value) => {
                if let Some(value) = value {
                    collect_expr(value, out);
                }
            }
        }
    }

    #[test]
    fn node_ids_are_unique_and_carry_the_file_tag() {
        let program = parse_ok(RICH);
        let mut ids = Vec::new();
        collect_module(&program.root, &mut ids);
        assert!(ids.len() > 80, "expected many nodes, got {}", ids.len());
        let mut seen = BTreeSet::new();
        for id in &ids {
            assert_ne!(*id, 0, "node id 0 is not allowed");
            assert!(seen.insert(*id), "duplicate node id {id}");
            assert_eq!(*id >> 24, 0, "id {id} does not carry file 0");
        }

        let result = parse_file(7, RICH);
        assert!(!result.diags.has_errors(), "{}", dump_diags(&result));
        let mut ids7 = Vec::new();
        collect_module(&result.program.root, &mut ids7);
        let set7: BTreeSet<NodeId> = ids7.iter().copied().collect();
        assert_eq!(set7.len(), ids7.len(), "duplicate node id for file 7");
        for id in &ids7 {
            assert_eq!(*id >> 24, 7, "id {id} does not carry file 7");
            assert!(!seen.contains(id), "file 7 id {id} collides with file 0");
        }
    }

    // -----------------------------------------------------------------
    // Diagnostics and recovery
    // -----------------------------------------------------------------

    #[test]
    fn recovers_after_several_errors_and_keeps_parsing() {
        let src = "trait A {}\nfn bad() { let x = ; }\nstruct S { y: }\nfn last() { }\n";
        let result = parse(src);
        assert!(
            result.diags.error_count() >= 3,
            "expected at least 3 errors, got:\n{}",
            dump_diags(&result)
        );
        let names: Vec<String> = top(&result.program)
            .iter()
            .filter_map(|item| match &item.kind {
                ItemKind::Fn(f) => Some(f.name.name.clone()),
                ItemKind::Struct(s) => Some(s.name.name.clone()),
                _ => None,
            })
            .collect();
        assert!(names.iter().any(|n| n == "bad"), "{names:?}");
        assert!(names.iter().any(|n| n == "S"), "{names:?}");
        assert!(names.iter().any(|n| n == "last"), "{names:?}");
    }

    #[test]
    fn errors_have_stable_codes_and_exact_spans() {
        // (source, diagnostic code, byte offset of the expected span start)
        let cases: &[(&str, &str, u32)] = &[
            ("fn f() {\n", "E-PARSE-UNCLOSED", 7),
            ("fn f( {}\n", "E-PARSE-UNCLOSED", 4),
            ("fn f() -> {}\n", "E-PARSE-EXPECTED", 10),
            ("fn f() { let x = ; }", "E-PARSE-UNEXPECTED", 17),
            ("fn f() { let x: ; }", "E-PARSE-EXPECTED", 16),
            ("fn f() { let x = 1 }", "E-PARSE-EXPECTED", 19),
            ("fn f() { if x }", "E-PARSE-EXPECTED", 14),
            ("fn f() { x. }", "E-PARSE-EXPECTED", 12),
            ("fn f() { match x { _ -> 1 } }", "E-PARSE-EXPECTED", 21),
            ("struct S { x: }", "E-PARSE-EXPECTED", 14),
            ("trait T {}", "E-UNSUPPORTED-TRAIT", 0),
            ("unsafe fn f() {}", "E-UNSUPPORTED-UNSAFE", 0),
            ("async fn f() {}", "E-UNSUPPORTED-ASYNC", 0),
            ("extern \"C\" fn f() {}", "E-UNSUPPORTED-EXTERN", 0),
            ("#[derive(Clone)]\nstruct S;", "E-UNSUPPORTED-ATTR", 0),
            ("fn f<T>() where T: X {}", "E-UNSUPPORTED-WHERE", 10),
            ("fn f<T: Clone>() {}", "E-UNSUPPORTED-BOUNDS", 6),
            ("use foo::*;", "E-UNSUPPORTED-GLOB", 9),
            ("use foo as bar;", "E-UNSUPPORTED-ALIAS", 8),
            ("pub use foo::bar;", "E-UNSUPPORTED-REEXPORT", 0),
            ("pub(crate) fn f() {}", "E-UNSUPPORTED-VIS", 0),
            ("struct S(T);", "E-UNSUPPORTED-TUPLE-STRUCT", 8),
            ("fn f() { foo!(); }", "E-UNSUPPORTED-MACRO", 9),
            ("fn f() { [1, 2, 3 }", "E-PARSE-UNCLOSED", 9),
        ];
        for (src, code, start) in cases {
            assert_error_at(src, code, *start);
        }
    }

    #[test]
    fn too_large_token_stream_is_reported_without_parsing() {
        let src = "1 ".repeat(super::MAX_TOKENS + 10);
        let result = parse(&src);
        assert!(result
            .diags
            .iter()
            .any(|d| d.code == "E-PARSE-TOO-LARGE" && d.span.start == 0));
        assert!(result.program.root.items.is_empty());
        assert_ne!(result.program.root.id, 0);
    }

    // -----------------------------------------------------------------
    // Hostile inputs (no panic, bounded time)
    // -----------------------------------------------------------------

    #[test]
    fn hostile_100k_tokens_parses_under_two_seconds() {
        let mut src = String::from("fn big() {\n");
        for _ in 0..20_000 {
            src.push_str("let x = 1;\n");
        }
        src.push('}');
        let started = Instant::now();
        let result = parse(&src);
        let elapsed = started.elapsed();
        assert!(!result.diags.has_errors(), "{}", dump_diags(&result));
        let f = as_fn(&top(&result.program)[0]);
        assert_eq!(f.body.stmts.len(), 20_000);
        assert!(
            elapsed.as_secs_f64() < 2.0,
            "100k-token parse took {elapsed:?}"
        );
    }

    #[test]
    fn hostile_deep_nesting_reports_depth_without_panic() {
        let mut src = String::from("fn f() { ");
        src.push_str(&"(".repeat(1_000));
        src.push('1');
        src.push_str(&")".repeat(1_000));
        src.push_str(" }");
        let started = Instant::now();
        let result = parse(&src);
        assert!(
            result.diags.iter().any(|d| d.code == "E-PARSE-DEPTH"),
            "expected E-PARSE-DEPTH, got:\n{}",
            dump_diags(&result)
        );
        assert!(
            started.elapsed().as_secs_f64() < 2.0,
            "deep-nesting parse took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn hostile_bizarre_inputs_never_panic() {
        let cases: &[&str] = &[
            "\u{0}\u{1}\u{2}",
            "fn f() { \"unterminated",
            "/* unterminated",
            "'",
            "fn é() {}",
            "fn f() { x <- y; }",
            "fn f() { 0x; }",
            "\u{feff}fn f() {}",
            "fn f() { let x = '\\u{d800}'; }",
            "fn\u{0}f",
            "\u{10ffff}",
            "fn f() { 😀 }",
            "fn f() { -",
            "}}}",
            "fn f() { let x = [;]; }",
        ];
        for src in cases {
            let result = parse(src);
            assert_ne!(result.program.root.id, 0);
            // No assertion on diagnostics: some hostile inputs are merely
            // lexically odd. The contract is "no panic, no hang".
        }
    }
}
