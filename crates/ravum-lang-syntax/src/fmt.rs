//! Token-based formatter for Ravum Core 1.0.
//!
//! OWNER: worker `syntax`.
//!
//! The formatter works on the token stream (with comments, `keep_trivia = true`)
//! rather than on the AST: it preserves every token and every comment exactly
//! (source slices are copied verbatim), and only inserts whitespace/newlines.
//! Because the token sequence is never changed, `parse(format(x)) == parse(x)`
//! holds structurally (spans and node ids differ by construction, so tests
//! compare span/id-free fingerprints).
//!
//! Canonical style:
//!
//! * indentation: 4 spaces per brace level;
//! * one trailing `\n`, line endings normalised to `\n`;
//! * one space around binary operators, after `,`, after `:` and around `=>`,
//!   `->` and `=`;
//! * no space before `,`, `;`, `)`, `]`, `:` or `.`;
//! * opening braces stay on the current line, bodies are indented;
//! * `else` stays on the same line as the closing brace;
//! * a comma directly inside a brace opens a new line (struct fields, enum
//!   variants, match arms), except before the closing `}`;
//! * line comments are moved to their own line; block comments are kept inline
//!   (a multi-line block comment is placed on its own lines).
//!
//! Any lexing error makes `format_source` return `Err(diagnostics)`: the
//! formatter never rewrites corrupted text.
//!
//! # Known limitations (documented, not hidden)
//!
//! * there is no line-width limit and no alignment: long lines are never
//!   wrapped and adjacent assignments/fields are not aligned;
//! * a trailing `// comment` at the end of a code line is moved to the
//!   following line (the token stream cannot distinguish "trailing" reliably);
//! * blank lines between items are not preserved (the output is canonical);
//! * the lines *inside* a multi-line `/* ... */` comment are copied verbatim
//!   and are not re-indented;
//! * distinguishing generic angles from comparison operators is a local
//!   heuristic (`Vec<i32>` vs `a < b`); whitespace never changes parsing, so a
//!   mis-classified angle bracket only affects spacing, never meaning;
//! * expression generic arguments must use `::<...>` (Core 1.0 restriction);
//!   the formatter keeps them glued (`Vec::<i32>::new()`).

use crate::diag::DiagnosticBag;
use crate::lexer;
use crate::span::FileId;
use crate::token::{Punct, Token, TokenKind};

/// Reformat one `.rv` file. Returns the canonical text, or the diagnostics that
/// prevented formatting (lexing errors only).
pub fn format_source(file: FileId, src: &str) -> Result<String, DiagnosticBag> {
    let (tokens, diags) = lexer::lex(file, src, true);
    if diags.has_errors() {
        return Err(diags);
    }
    if tokens.is_empty() {
        // Cannot happen with the current lexer (EOF is always pushed), but be
        // defensive: an empty token stream formats to a single newline.
        return Ok("\n".to_string());
    }
    Ok(Formatter::new(src, tokens).run())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Angle {
    None,
    Open,
    /// `>`
    Close,
    /// `>>`
    CloseTwo,
    /// `>=` used to close a generic list: `Vec<T>=x` (the lexer emits `>=`).
    CloseEq,
    /// `>>=` used to close two generic lists: `Vec<Vec<T>>=x`.
    CloseTwoEq,
}

impl Angle {
    fn is_close(self) -> bool {
        matches!(
            self,
            Angle::Close | Angle::CloseTwo | Angle::CloseEq | Angle::CloseTwoEq
        )
    }
}

fn is_comment(token: &Token) -> bool {
    matches!(token.kind, TokenKind::Comment(_))
}

/// Decide which `<` are generic-list openers and which `>` / `>>` close them.
/// The decision is a local heuristic (see module docs); a wrong guess only
/// changes spacing.
fn compute_angles(tokens: &[Token], sig: &[usize]) -> Vec<Angle> {
    let mut roles = vec![Angle::None; tokens.len()];
    let mut depth: i32 = 0;
    for (si, &ti) in sig.iter().enumerate() {
        match &tokens[ti].kind {
            TokenKind::Punct(Punct::Lt) => {
                if depth > 0 {
                    depth += 1;
                    roles[ti] = Angle::Open;
                } else if looks_like_generic_open(tokens, sig, si) {
                    depth = 1;
                    roles[ti] = Angle::Open;
                }
            }
            TokenKind::Punct(Punct::Gt) => {
                if depth > 0 {
                    depth -= 1;
                    roles[ti] = Angle::Close;
                }
            }
            TokenKind::Punct(Punct::Shr) => {
                if depth > 0 {
                    depth = (depth - 2).max(0);
                    roles[ti] = Angle::CloseTwo;
                }
            }
            TokenKind::Punct(Punct::Ge) => {
                if depth > 0 {
                    depth = (depth - 1).max(0);
                    roles[ti] = Angle::CloseEq;
                }
            }
            TokenKind::Punct(Punct::ShrEq) => {
                if depth > 0 {
                    depth = (depth - 2).max(0);
                    roles[ti] = Angle::CloseTwoEq;
                }
            }
            _ => {}
        }
    }
    roles
}

fn looks_like_generic_open(tokens: &[Token], sig: &[usize], si: usize) -> bool {
    if si == 0 {
        return false;
    }
    let prev = &tokens[sig[si - 1]].kind;
    let (candidate, prev_impl) = match prev {
        TokenKind::Ident(_) => (true, false),
        TokenKind::Punct(Punct::ColonColon) => (true, false),
        TokenKind::Kw(crate::token::Kw::Impl) => (true, true),
        _ => (false, false),
    };
    if !candidate {
        return false;
    }
    let mut depth: i32 = 1;
    let mut nest: i32 = 0;
    let mut j = si + 1;
    while j < sig.len() {
        let kind = &tokens[sig[j]].kind;
        match kind {
            TokenKind::Punct(Punct::Lt) => depth += 1,
            TokenKind::Punct(Punct::Gt) => {
                depth -= 1;
                if depth == 0 {
                    return follow_set_ok(tokens, sig, j + 1, prev_impl);
                }
            }
            TokenKind::Punct(Punct::Shr) => {
                depth -= 2;
                if depth <= 0 {
                    return follow_set_ok(tokens, sig, j + 1, prev_impl);
                }
            }
            // `>=` closes a generic list and is simultaneously the start of an
            // assignment (`Vec<T>=x`): the follow token is `=`, always valid.
            TokenKind::Punct(Punct::Ge) => {
                depth -= 1;
                if depth == 0 {
                    return true;
                }
            }
            // `>>=` closes two generic lists and carries the `=` of `Vec<Vec<T>>=x`.
            TokenKind::Punct(Punct::ShrEq) => {
                depth -= 2;
                if depth <= 0 {
                    return true;
                }
            }
            TokenKind::Punct(Punct::LParen | Punct::LBracket) => nest += 1,
            TokenKind::Punct(Punct::RParen | Punct::RBracket) => {
                if nest == 0 {
                    return false;
                }
                nest -= 1;
            }
            TokenKind::Punct(
                Punct::Semi
                | Punct::LBrace
                | Punct::RBrace
                | Punct::FatArrow
                | Punct::Arrow
                | Punct::EqEq
                | Punct::Ne
                | Punct::DotDot
                | Punct::DotDotEq
                | Punct::Plus
                | Punct::Minus
                | Punct::Star
                | Punct::Slash
                | Punct::Percent
                | Punct::Bang
                | Punct::Question
                | Punct::AmpAmp
                | Punct::PipePipe
                | Punct::Colon,
            ) if nest == 0 => return false,
            TokenKind::Kw(
                crate::token::Kw::Fn
                | crate::token::Kw::If
                | crate::token::Kw::Match
                | crate::token::Kw::While
                | crate::token::Kw::Loop
                | crate::token::Kw::For
                | crate::token::Kw::Return
                | crate::token::Kw::Break
                | crate::token::Kw::Continue
                | crate::token::Kw::Let
                | crate::token::Kw::Struct
                | crate::token::Kw::Enum
                | crate::token::Kw::Const
                | crate::token::Kw::Use
                | crate::token::Kw::Mod
                | crate::token::Kw::Test
                | crate::token::Kw::Pub
                | crate::token::Kw::Trait
                | crate::token::Kw::Extern
                | crate::token::Kw::Async
                | crate::token::Kw::Unsafe
                | crate::token::Kw::Move
                | crate::token::Kw::Type
                | crate::token::Kw::Static
                | crate::token::Kw::Effects,
            ) if nest == 0 => return false,
            _ => {}
        }
        j += 1;
    }
    false
}

fn follow_set_ok(tokens: &[Token], sig: &[usize], j: usize, prev_impl: bool) -> bool {
    let Some(&ti) = sig.get(j) else {
        return true; // generic list at end of input (fragment)
    };
    match &tokens[ti].kind {
        TokenKind::Punct(
            Punct::LParen
            | Punct::LBrace
            | Punct::ColonColon
            | Punct::Comma
            | Punct::Semi
            | Punct::RParen
            | Punct::RBracket
            | Punct::RBrace
            | Punct::Arrow
            | Punct::Eq
            | Punct::Question
            | Punct::Dot
            | Punct::Gt
            | Punct::Shr,
        ) => true,
        TokenKind::Kw(crate::token::Kw::Where) => true,
        TokenKind::Ident(_) if prev_impl => true,
        TokenKind::Eof => true,
        _ => false,
    }
}

/// Classify prefix operators (`-`, `*`, `&`, `!`) versus binary ones. Prefix
/// operators glue to their operand; binary ones get spaces on both sides.
fn compute_unary(tokens: &[Token], sig: &[usize], roles: &[Angle]) -> Vec<bool> {
    let mut unary = vec![false; tokens.len()];
    for (si, &ti) in sig.iter().enumerate() {
        let prefix = match &tokens[ti].kind {
            TokenKind::Punct(Punct::Bang) => true,
            TokenKind::Punct(Punct::Minus | Punct::Star | Punct::Amp) => {
                if si == 0 {
                    true
                } else {
                    !is_operand_end(tokens, sig[si - 1], roles)
                }
            }
            _ => continue,
        };
        unary[ti] = prefix;
    }
    unary
}

fn is_operand_end(tokens: &[Token], idx: usize, roles: &[Angle]) -> bool {
    match &tokens[idx].kind {
        TokenKind::Ident(_)
        | TokenKind::Int { .. }
        | TokenKind::Float { .. }
        | TokenKind::Str(_)
        | TokenKind::Char(_) => true,
        TokenKind::Kw(
            crate::token::Kw::True
            | crate::token::Kw::False
            | crate::token::Kw::SelfValue
            | crate::token::Kw::SelfType,
        ) => true,
        TokenKind::Punct(Punct::RParen | Punct::RBracket | Punct::RBrace | Punct::Question) => true,
        TokenKind::Punct(Punct::Gt | Punct::Shr | Punct::Ge | Punct::ShrEq) => {
            roles[idx].is_close()
        }
        _ => false,
    }
}

fn is_binary_punct(p: Punct) -> bool {
    matches!(
        p,
        Punct::Plus
            | Punct::Minus
            | Punct::Star
            | Punct::Slash
            | Punct::Percent
            | Punct::Amp
            | Punct::AmpAmp
            | Punct::Pipe
            | Punct::PipePipe
            | Punct::Caret
            | Punct::Shl
            | Punct::Shr
            | Punct::EqEq
            | Punct::Ne
            | Punct::Lt
            | Punct::Le
            | Punct::Gt
            | Punct::Ge
    )
}

fn is_closing_token(tokens: &[Token], roles: &[Angle], idx: usize) -> bool {
    match &tokens[idx].kind {
        TokenKind::Punct(Punct::RParen | Punct::RBracket | Punct::RBrace) => true,
        TokenKind::Punct(Punct::Gt | Punct::Shr | Punct::Ge | Punct::ShrEq) => {
            roles[idx].is_close()
        }
        _ => false,
    }
}

struct Formatter<'a> {
    src: &'a str,
    tokens: Vec<Token>,
    out: String,
    line_empty: bool,
    indent: usize,
    /// Expected closing tokens for `(`, `[` and `{`.
    stack: Vec<Punct>,
    generic_depth: i32,
    roles: Vec<Angle>,
    unary: Vec<bool>,
    /// For each token index, the next non-comment, non-EOF token (or EOF).
    next_sig: Vec<usize>,
    /// Index of the last emitted significant token.
    last_sig: Option<usize>,
}

impl<'a> Formatter<'a> {
    fn new(src: &'a str, tokens: Vec<Token>) -> Self {
        let eof_idx = tokens.len().saturating_sub(1);
        let sig: Vec<usize> = tokens
            .iter()
            .enumerate()
            .filter(|(_, token)| !is_comment(token) && !token.is_eof())
            .map(|(i, _)| i)
            .collect();
        let roles = compute_angles(&tokens, &sig);
        let unary = compute_unary(&tokens, &sig, &roles);
        let mut next_sig = vec![eof_idx; tokens.len()];
        let mut next = eof_idx;
        for i in (0..tokens.len()).rev() {
            next_sig[i] = next;
            if i != eof_idx && !is_comment(&tokens[i]) {
                next = i;
            }
        }
        Self {
            src,
            tokens,
            out: String::new(),
            line_empty: true,
            indent: 0,
            stack: Vec::new(),
            generic_depth: 0,
            roles,
            unary,
            next_sig,
            last_sig: None,
        }
    }

    fn run(mut self) -> String {
        let mut i = 0;
        while i < self.tokens.len() {
            if self.tokens[i].is_eof() {
                break;
            }
            if i > 0 && self.should_preserve_blank_line(i) {
                self.blank_line();
            }
            if let TokenKind::Comment(text) = self.tokens[i].kind.clone() {
                self.emit_comment(&text);
                i += 1;
                continue;
            }
            let next = self.emit_token(i);
            if next <= i {
                // Defensive: never loop on the same token.
                i += 1;
            } else {
                self.last_sig = Some(next - 1);
                i = next;
            }
        }
        self.finish_output()
    }

    fn finish_output(mut self) -> String {
        while self.out.ends_with(' ') || self.out.ends_with('\n') {
            self.out.pop();
        }
        self.out.push('\n');
        self.out
    }

    // ---------------------------------------------------------------------
    // Writer primitives
    // ---------------------------------------------------------------------

    fn text(&self, idx: usize) -> &str {
        let span = self.tokens[idx].span;
        self.src
            .get(span.start as usize..span.end as usize)
            .unwrap_or("")
    }

    fn write(&mut self, text: &str) {
        if self.line_empty {
            for _ in 0..self.indent {
                self.out.push_str("    ");
            }
            self.line_empty = false;
        }
        self.out.push_str(text);
    }

    fn space(&mut self) {
        if !self.line_empty && !self.out.ends_with(' ') {
            self.out.push(' ');
        }
    }

    fn newline(&mut self) {
        while self.out.ends_with(' ') {
            self.out.pop();
        }
        if !self.out.is_empty() && !self.out.ends_with('\n') {
            self.out.push('\n');
        }
        self.line_empty = true;
    }

    /// Preserve at most one blank line from the source between two tokens.
    /// Called only at a fresh output line; multiple source blank lines collapse
    /// into one and no blank line is ever inserted where the source had none.
    fn blank_line(&mut self) {
        if !self.line_empty || self.out.is_empty() {
            return;
        }
        if !self.out.ends_with('\n') {
            self.out.push('\n');
        }
        while self.out.ends_with("\n\n\n") {
            self.out.pop();
        }
        if !self.out.ends_with("\n\n") {
            self.out.push('\n');
        }
    }

    fn should_preserve_blank_line(&self, i: usize) -> bool {
        if i == 0 || !self.line_empty {
            return false;
        }
        // Never add an empty line before a closing delimiter, a separator, an
        // `else` or the end of file: those are not "between items/statements".
        match &self.tokens[i].kind {
            TokenKind::Eof => return false,
            TokenKind::Punct(
                Punct::RBrace | Punct::RParen | Punct::RBracket | Punct::Comma | Punct::Semi,
            ) => return false,
            TokenKind::Kw(crate::token::Kw::Else) => return false,
            _ => {}
        }
        let prev_end = self.tokens[i - 1].span.end as usize;
        let start = self.tokens[i].span.start as usize;
        if start <= prev_end {
            return false;
        }
        let gap = match self.src.get(prev_end..start) {
            Some(gap) => gap,
            None => return false,
        };
        gap.bytes().filter(|b| *b == b'\n').count() >= 2
    }

    // ---------------------------------------------------------------------
    // Spacing decisions
    // ---------------------------------------------------------------------

    fn space_before(&mut self, i: usize) {
        if self.line_empty {
            return;
        }
        let Some(prev) = self.last_sig else {
            return;
        };
        if self.space_between(prev, i) {
            self.space();
        }
    }

    fn space_between(&self, p: usize, c: usize) -> bool {
        // Rule 1: a previous token that glues to what follows.
        match &self.tokens[p].kind {
            TokenKind::Punct(
                Punct::LParen
                | Punct::LBracket
                | Punct::Dot
                | Punct::ColonColon
                | Punct::Bang
                | Punct::DotDot
                | Punct::DotDotEq,
            ) => return false,
            TokenKind::Punct(Punct::Lt) if self.roles[p] == Angle::Open => return false,
            _ => {}
        }
        if self.unary[p] {
            return false;
        }

        // Rule 2: a current token that glues to what precedes it.
        match &self.tokens[c].kind {
            TokenKind::Punct(
                Punct::Comma
                | Punct::Semi
                | Punct::RParen
                | Punct::RBracket
                | Punct::Colon
                | Punct::ColonColon
                | Punct::Dot
                | Punct::Question,
            ) => false,
            TokenKind::Punct(Punct::Lt) if self.roles[c] == Angle::Open => false,
            TokenKind::Punct(Punct::Gt | Punct::Shr | Punct::Ge | Punct::ShrEq)
                if self.roles[c].is_close() =>
            {
                false
            }
            TokenKind::Punct(Punct::DotDot | Punct::DotDotEq) => false,
            TokenKind::Punct(Punct::LParen) => {
                // Calls and declarations glue; control-flow keywords do not.
                let glue = matches!(
                    &self.tokens[p].kind,
                    TokenKind::Ident(_)
                        | TokenKind::Punct(Punct::RParen | Punct::RBracket | Punct::Question)
                        | TokenKind::Kw(
                            crate::token::Kw::Pub
                                | crate::token::Kw::Effects
                                | crate::token::Kw::SelfValue
                                | crate::token::Kw::SelfType
                        )
                );
                let generic_close = matches!(
                    &self.tokens[p].kind,
                    TokenKind::Punct(Punct::Gt | Punct::Shr | Punct::Ge | Punct::ShrEq)
                ) && self.roles[p].is_close();
                !(glue || generic_close)
            }
            TokenKind::Punct(Punct::LBracket) => {
                let glue = matches!(
                    &self.tokens[p].kind,
                    TokenKind::Ident(_)
                        | TokenKind::Punct(Punct::RParen | Punct::RBracket | Punct::Question)
                        | TokenKind::Kw(crate::token::Kw::SelfValue | crate::token::Kw::SelfType)
                );
                let generic_close = matches!(
                    &self.tokens[p].kind,
                    TokenKind::Punct(Punct::Gt | Punct::Shr | Punct::Ge | Punct::ShrEq)
                ) && self.roles[p].is_close();
                !(glue || generic_close)
            }
            // `!` is either a prefix operator (space as usual) or the marker of
            // a macro invocation, which glues after a path/expression.
            TokenKind::Punct(Punct::Bang) => !self.operand_end(p),
            _ => true,
        }
    }

    fn operand_end(&self, idx: usize) -> bool {
        is_operand_end(&self.tokens, idx, &self.roles)
    }

    fn update_generic(&mut self, role: Angle) {
        match role {
            Angle::Open => self.generic_depth += 1,
            Angle::Close => self.generic_depth = (self.generic_depth - 1).max(0),
            Angle::CloseTwo => self.generic_depth = (self.generic_depth - 2).max(0),
            Angle::CloseEq => self.generic_depth = (self.generic_depth - 1).max(0),
            Angle::CloseTwoEq => self.generic_depth = (self.generic_depth - 2).max(0),
            Angle::None => {}
        }
    }

    fn brace_top(&self) -> bool {
        self.stack.last().copied() == Some(Punct::RBrace)
    }

    // ---------------------------------------------------------------------
    // Comments
    // ---------------------------------------------------------------------

    fn emit_comment(&mut self, text: &str) {
        if text.starts_with("//") {
            let text = text.trim_end_matches('\r');
            if !self.line_empty {
                self.newline();
            }
            self.write(text);
            self.newline();
        } else if text.contains('\n') {
            if !self.line_empty {
                self.newline();
            }
            self.write(text);
            self.newline();
        } else {
            if !self.line_empty {
                self.space();
            }
            self.write(text);
            self.space();
        }
    }

    // ---------------------------------------------------------------------
    // Token emission
    // ---------------------------------------------------------------------

    fn emit_token(&mut self, i: usize) -> usize {
        let kind = self.tokens[i].kind.clone();
        let role = self.roles[i];
        match kind {
            TokenKind::Punct(p) => self.emit_punct(i, p, role),
            TokenKind::Eof => i + 1,
            _ => {
                self.space_before(i);
                let text = self.text(i).to_string();
                self.write(&text);
                self.update_generic(role);
                i + 1
            }
        }
    }

    fn emit_punct(&mut self, i: usize, p: Punct, role: Angle) -> usize {
        match p {
            Punct::LBrace => self.emit_open_brace(i),
            Punct::RBrace => self.emit_close_brace(i),
            Punct::Comma => {
                self.space_before(i);
                self.write(",");
                self.after_comma(i);
                i + 1
            }
            Punct::Semi => {
                self.space_before(i);
                self.write(";");
                self.after_semi();
                i + 1
            }
            Punct::LParen => {
                self.space_before(i);
                self.write("(");
                self.stack.push(Punct::RParen);
                i + 1
            }
            Punct::LBracket => {
                self.space_before(i);
                self.write("[");
                self.stack.push(Punct::RBracket);
                i + 1
            }
            Punct::RParen | Punct::RBracket => {
                self.space_before(i);
                self.write(punct_text(p));
                if self.stack.last().copied() == Some(p) {
                    self.stack.pop();
                }
                i + 1
            }
            _ => {
                self.space_before(i);
                // `>=`/`>>=` may close a generic list and start an assignment at
                // the same time (`Vec<T>=x`); render the angle brackets glued to
                // the type and the `=` as a separate assignment operator.
                match role {
                    Angle::CloseEq => {
                        self.write(">");
                        self.space();
                        self.write("=");
                    }
                    Angle::CloseTwoEq => {
                        self.write(">>");
                        self.space();
                        self.write("=");
                    }
                    _ => self.write(punct_text(p)),
                }
                self.update_generic(role);
                i + 1
            }
        }
    }

    fn emit_open_brace(&mut self, i: usize) -> usize {
        let next = self.next_sig[i];
        if next == i + 1 && matches!(self.tokens[next].kind, TokenKind::Punct(Punct::RBrace)) {
            // Empty block/struct literal: keep it on one line.
            if !self.line_empty {
                self.space();
            }
            self.write("{}");
            self.post_close(next);
            return next + 1;
        }
        if !self.line_empty {
            self.space();
        }
        self.write("{");
        self.stack.push(Punct::RBrace);
        self.indent += 1;
        self.newline();
        i + 1
    }

    fn emit_close_brace(&mut self, i: usize) -> usize {
        if !self.line_empty {
            self.newline();
        }
        self.indent = self.indent.saturating_sub(1);
        self.write("}");
        if self.stack.last().copied() == Some(Punct::RBrace) {
            self.stack.pop();
        }
        self.post_close(i);
        i + 1
    }

    fn after_comma(&mut self, i: usize) {
        let next = self.next_sig[i];
        if is_closing_token(&self.tokens, &self.roles, next) {
            return;
        }
        if self.generic_depth > 0 {
            self.space();
            return;
        }
        if self.brace_top() {
            self.newline();
        } else {
            self.space();
        }
    }

    fn after_semi(&mut self) {
        if self.brace_top() || self.stack.is_empty() {
            self.newline();
        } else {
            self.space();
        }
    }

    fn post_close(&mut self, close_idx: usize) {
        let next = self.next_sig[close_idx];
        if self.tokens[next].is_eof() {
            self.newline();
            return;
        }
        match &self.tokens[next].kind {
            TokenKind::Punct(
                Punct::Semi
                | Punct::Comma
                | Punct::RParen
                | Punct::RBracket
                | Punct::Dot
                | Punct::Question
                | Punct::Colon
                | Punct::ColonColon
                | Punct::LBracket,
            ) => {}
            TokenKind::Kw(crate::token::Kw::Else) => self.space(),
            TokenKind::Punct(p) if is_binary_punct(*p) => self.space(),
            _ => self.newline(),
        }
    }
}

fn punct_text(p: Punct) -> &'static str {
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

#[cfg(test)]
mod tests {
    use super::format_source;
    use crate::ast::Program;
    use crate::parser::parse_file;
    use std::path::{Path, PathBuf};

    fn fmt(src: &str) -> String {
        format_source(0, src).unwrap_or_else(|diags| {
            panic!(
                "formatting failed for {src:?}: {:?}",
                diags.iter().map(|d| &d.code).collect::<Vec<_>>()
            )
        })
    }

    fn examples_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("examples")
    }

    fn all_rv_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("read examples dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                all_rv_files(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rv") {
                out.push(path);
            }
        }
    }

    /// Span- and id-free view of the AST, used to compare the meaning of a
    /// source and of its formatted version. Node ids and spans necessarily
    /// differ after formatting (byte offsets move), so they are erased from the
    /// `Debug` rendering; everything else (structure, names, literal values)
    /// is compared verbatim.
    fn normalized_debug(program: &Program) -> String {
        let debug = format!("{program:?}");
        let bytes = debug.as_bytes();
        let mut out = String::new();
        let mut i = 0;
        while i < debug.len() {
            if debug[i..].starts_with("id: ") {
                out.push_str("id: 0");
                i += 4;
                while i < debug.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                continue;
            }
            if debug[i..].starts_with("Span {") {
                while i < debug.len() && bytes[i] != b'}' {
                    i += 1;
                }
                if i < debug.len() {
                    i += 1;
                }
                out.push_str("Span");
                continue;
            }
            let ch = debug[i..].chars().next().expect("non-empty suffix");
            out.push(ch);
            i += ch.len_utf8();
        }
        out
    }

    fn assert_meaning_preserved(src: &str, formatted: &str) {
        let original = parse_file(0, src);
        let reformatted = parse_file(0, formatted);
        assert_eq!(
            normalized_debug(&original.program),
            normalized_debug(&reformatted.program),
            "formatting changed the parsed meaning of {src:?}\n--- formatted ---\n{formatted}"
        );
    }

    fn assert_idempotent(src: &str) -> String {
        let once = fmt(src);
        let twice = fmt(&once);
        assert_eq!(once, twice, "formatting is not idempotent for {src:?}");
        assert_meaning_preserved(src, &once);
        once
    }

    #[test]
    fn blank_line_between_items_is_preserved() {
        let out = fmt("use std::io;\n\nfn main() {}\n");
        assert_eq!(out, "use std::io;\n\nfn main() {}\n");
        // Idempotence and meaning are checked too.
        assert_eq!(fmt(&out), out);
        assert_meaning_preserved("use std::io;\n\nfn main() {}\n", &out);
    }

    #[test]
    fn multiple_blank_lines_collapse_to_one() {
        let out = fmt("fn a() {}\n\n\n\nfn b() {}\n");
        assert_eq!(out, "fn a() {}\n\nfn b() {}\n");
        assert_eq!(out.matches("\n\n").count(), 1, "{out:?}");
        assert_eq!(fmt(&out), out);
    }

    #[test]
    fn blank_lines_between_statements_are_preserved_once() {
        let out = fmt("fn f() {\n    let a = 1;\n\n\n    let b = 2;\n}\n");
        assert_eq!(out, "fn f() {\n    let a = 1;\n\n    let b = 2;\n}\n");
        assert_eq!(fmt(&out), out);
    }

    #[test]
    fn no_blank_line_is_inserted_when_the_source_has_none() {
        let out = fmt("use std::io;\nfn main() {}\n");
        assert_eq!(out, "use std::io;\nfn main() {}\n");
    }

    #[test]
    fn empty_source_formats_to_one_newline() {
        assert_eq!(fmt(""), "\n");
        assert_eq!(fmt("\n\n\n"), "\n");
        assert_eq!(fmt("// only a comment"), "// only a comment\n");
        assert_eq!(fmt("// only a comment\n"), "// only a comment\n");
        assert_eq!(fmt("/* block */"), "/* block */\n");
        assert_eq!(fmt("\r\n"), "\n");
    }

    #[test]
    fn simple_function_is_formatted_canonically() {
        let out = fmt("fn main(){let x=1+2*3;x+=1;}");
        let expected = "\
fn main() {
    let x = 1 + 2 * 3;
    x += 1;
}
";
        assert_eq!(out, expected);
    }

    #[test]
    fn struct_enum_and_impl_layout() {
        let out = fmt(
            "pub struct Point{pub x:f64,y:f64}\nenum E{A,B(i32),C{x:i32}}\nimpl Point{fn new()->Point{Point{x:0.0,y:0.0}}}",
        );
        assert!(out.contains("pub struct Point {\n"), "{out}");
        assert!(out.contains("\n    pub x: f64,\n"), "{out}");
        assert!(out.contains("\n    y: f64\n"), "{out}");
        assert!(
            out.contains("enum E {\n    A,\n    B(i32),\n    C {\n"),
            "{out}"
        );
        assert!(out.contains("\n        x: i32\n    }\n}"), "{out}");
        assert!(out.contains("impl Point {\n"), "{out}");
        assert!(out.contains("    fn new() -> Point {\n"), "{out}");
        assert!(
            out.contains("        Point {\n            x: 0.0,\n            y: 0.0\n        }\n"),
            "{out}"
        );
    }

    #[test]
    fn comments_are_kept_on_their_own_lines() {
        let out = fmt("// top\nfn f(){ // trailing\nlet x=1; // tail\n/* b */}");
        let expected = "\
// top
fn f() {
    // trailing
    let x = 1;
    // tail
    /* b */
}
";
        assert_eq!(out, expected);
    }

    #[test]
    fn operator_and_punctuation_spacing() {
        let out = fmt("fn f(a:i32,b:&str)->i32{let c:Vec<i32>=a;c[0]+1}");
        let expected = "\
fn f(a: i32, b: &str) -> i32 {
    let c: Vec<i32> = a;
    c[0] + 1
}
";
        assert_eq!(out, expected);
    }

    #[test]
    fn generics_ranges_for_and_match() {
        let out = fmt(
            "fn f(){let x:Result<i32,Error>=g::<i32>();for i in 0..3{let y=[0;2];}match x{Ok(v)=>v,Err(_)=>0}}",
        );
        let expected = "\
fn f() {
    let x: Result<i32, Error> = g::<i32>();
    for i in 0..3 {
        let y = [0; 2];
    }
    match x {
        Ok(v) => v,
        Err(_) => 0
    }
}
";
        assert_eq!(out, expected);
    }

    #[test]
    fn literals_are_preserved_verbatim() {
        let out = fmt("fn f(){let s=\"a  b\";let c='\\n';let n=0xFFu8;let f2=1.50e3;}");
        assert!(out.contains("\"a  b\""), "{out}");
        assert!(out.contains("'\\n'"), "{out}");
        assert!(out.contains("0xFFu8"), "{out}");
        assert!(out.contains("1.50e3"), "{out}");
    }

    #[test]
    fn crlf_is_normalised() {
        let out = fmt("fn f(){\r\n    let x = 1;\r\n}\r\n");
        assert!(!out.contains('\r'), "{out:?}");
        assert_eq!(out, "fn f() {\n    let x = 1;\n}\n");
    }

    #[test]
    fn lexing_errors_are_rejected() {
        for (src, code) in [
            ("\"unterminated", "E-LEX-STRING"),
            ("/* nope", "E-LEX-COMMENT"),
            ("fn f() { \"x }", "E-LEX-STRING"),
            ("'", "E-LEX-CHAR"),
        ] {
            let err = format_source(0, src).expect_err("lexing error must not be formatted");
            assert!(
                err.iter().any(|d| d.code == code),
                "expected {code} for {src:?}, got {:?}",
                err.iter().map(|d| &d.code).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn inline_corpus_is_idempotent_and_meaning_preserving() {
        let corpus: &[&str] = &[
            "fn main() {}",
            "pub fn add(a: i32, b: i32) -> i32 { a + b }",
            "test fn works() { assert_eq(1, 1); }",
            "const N: i32 = 10;",
            "struct Unit;\nstruct S { pub a: i32, b: bool }\n",
            "enum E { A, B(i32, u8), C { x: i32 } }\n",
            "impl<T> Vec<T> { fn len(&self) -> usize { 0 } }\n",
            "mod inner { use crate::foo::Bar; }\nmod decl;\n",
            "fn f() { let x: [u8; 3] = [1, 2, 3]; let y = [0; 4]; }\n",
            "fn f() { let t = (1, (2, 3)); let u = (); let s = Point { x: 1, y }; }\n",
            "fn f() { while i < n { i += 1; } loop { break; } for x in 0..=n { continue; } }\n",
            "fn f() { if a { 1 } else if b { 2 } else { 3 } }\n",
            "fn f() { match v { E::A => 0, E::B(x) => x, E::C { y } => y, _ => 1 } }\n",
            "fn f() -> Result<(), Error> { let x = g()?; return Ok(()); }\n",
            "fn f() { x = 1 as u8; x += 2; x >>= 1; a & b; a << 2; a | b; a ^ b; }\n",
            "fn f() { let s = self.x; let m = receiver.method(1, 2); let p = pair.0; }\n",
            "fn f() { -x; !b; &v; &mut v; *p; }\n",
            "// line comment\nfn f() {\n    /* inner */\n    let x = 1;\n}\n",
            "/* multi\n   line */\nfn f() {}\n",
            "fn f<T, U>(a: &T, b: &mut U) -> Vec<T> effects(io, alloc) { x }\n",
            "fn f() { let r = 0..n; let s = 0..=n; }\n",
        ];
        for src in corpus {
            let formatted = assert_idempotent(src);
            assert!(!formatted.is_empty());
        }
    }

    #[test]
    fn hello_example_is_already_canonically_formatted() {
        // `ravum-lang fmt --check examples/hello` must report no change.
        let path = examples_root().join("hello").join("src").join("main.rv");
        let src = std::fs::read_to_string(&path).expect("read hello example");
        let formatted = fmt(&src);
        assert_eq!(
            src, formatted,
            "examples/hello is not canonically formatted (fmt --check would fail)"
        );
    }

    #[test]
    fn examples_are_idempotent_and_meaning_preserving() {
        let root = examples_root();
        let mut files = Vec::new();
        all_rv_files(&root, &mut files);
        assert!(!files.is_empty(), "no .rv example under {}", root.display());
        for file in files {
            let src = std::fs::read_to_string(&file).expect("read example");
            let formatted = assert_idempotent(&src);
            // The formatted example must still be a valid example (no parse
            // diagnostics introduced by formatting).
            let reparsed = parse_file(0, &formatted);
            let errors: Vec<String> = reparsed
                .diags
                .iter()
                .map(|d| format!("{}: {}", d.code, d.message))
                .collect();
            assert!(
                errors.is_empty(),
                "formatted {} does not parse cleanly:\n{}\n--- formatted ---\n{formatted}",
                file.display(),
                errors.join("\n")
            );
        }
    }
}
