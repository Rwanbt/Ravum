use super::*;

impl<'a> Parser<'a> {
    pub(super) fn new(
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
            reach: 0,
            fatal: false,
            depth_reported: false,
            nodes_reported: false,
            forbid_assign: 0,
            stmt_head: false,
            stmt_end: false,
            paren_exprs: BTreeSet::new(),
            in_for_iter: false,
        }
    }

    pub(super) fn finish(mut self) -> DiagnosticBag {
        self.diags.sort();
        self.diags
    }

    // ---------------------------------------------------------------------
    // Token access
    // ---------------------------------------------------------------------

    pub(super) fn eof_index(&self) -> usize {
        self.tokens.len().saturating_sub(1)
    }

    pub(super) fn cur(&self) -> &Token {
        let idx = self.pos.min(self.tokens.len().saturating_sub(1));
        &self.tokens[idx]
    }

    pub(super) fn cur_span(&self) -> Span {
        self.cur().span
    }

    pub(super) fn prev_end(&self) -> u32 {
        if self.pos == 0 {
            self.cur().span.start
        } else {
            self.tokens
                .get(self.pos - 1)
                .map(|t| t.span.end)
                .unwrap_or(0)
        }
    }

    pub(super) fn span_from(&self, start: Span) -> Span {
        let end = self.prev_end().max(start.start);
        Span::new(start.file, start.start, end)
    }

    pub(super) fn zero_span(&self) -> Span {
        let at = self.prev_end();
        Span::new(self.file, at, at)
    }

    pub(super) fn at_eof(&self) -> bool {
        self.cur().is_eof()
    }

    pub(super) fn bump(&mut self) {
        if !self.at_eof() {
            self.pos = (self.pos + 1).min(self.eof_index());
        }
    }

    pub(super) fn bump_n(&mut self, n: usize) {
        for _ in 0..n {
            self.bump();
        }
    }

    pub(super) fn kind(&self) -> &TokenKind {
        &self.cur().kind
    }

    pub(super) fn at(&self, p: Punct) -> bool {
        matches!(self.kind(), TokenKind::Punct(q) if *q == p)
    }

    pub(super) fn at_kw(&self, kw: Kw) -> bool {
        matches!(self.kind(), TokenKind::Kw(k) if *k == kw)
    }

    pub(super) fn eat(&mut self, p: Punct) -> bool {
        if self.at(p) {
            self.bump();
            true
        } else {
            false
        }
    }

    pub(super) fn eat_kw(&mut self, kw: Kw) -> bool {
        if self.at_kw(kw) {
            self.bump();
            true
        } else {
            false
        }
    }

    pub(super) fn peek_at(&self, n: usize) -> &Token {
        let idx = (self.pos + n).min(self.eof_index());
        &self.tokens[idx]
    }

    pub(super) fn peek_punct(&self, n: usize, p: Punct) -> bool {
        matches!(&self.peek_at(n).kind, TokenKind::Punct(q) if *q == p)
    }

    pub(super) fn peek_ident(&self) -> bool {
        matches!(&self.peek_at(1).kind, TokenKind::Ident(_))
    }

    // ---------------------------------------------------------------------
    // Ids and guards
    // ---------------------------------------------------------------------

    pub(super) fn id(&mut self) -> NodeId {
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

    /// Enter one level of syntactic nesting.
    pub(super) fn descend(&mut self) {
        self.depth += 1;
        self.reach = self.reach.max(self.depth);
    }

    /// Start measuring an operand: returns the reach to restore afterwards.
    pub(super) fn open_operand(&mut self) -> u32 {
        std::mem::replace(&mut self.reach, self.depth)
    }

    /// Add one link to a left-associative chain whose spine currently reaches
    /// `spine`. Returns the new reach of the spine, or `None` after reporting
    /// `E-PARSE-DEPTH` when the tree would exceed the nesting limit.
    pub(super) fn chain_link(&mut self, spine: u32) -> Option<u32> {
        let reach = spine.max(self.reach).saturating_add(1);
        if reach >= MAX_DEPTH {
            self.fail_depth();
            return None;
        }
        Some(reach)
    }

    pub(super) fn fail_depth(&mut self) {
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

    pub(super) fn describe_token(&self) -> String {
        match self.kind() {
            TokenKind::Ident(name) => format!("identifier `{name}`"),
            TokenKind::Kw(kw) => format!("keyword `{}`", kw.as_str()),
            TokenKind::Int { .. } => "integer literal".to_string(),
            TokenKind::Float { .. } => "float literal".to_string(),
            TokenKind::Str(_) => "string literal".to_string(),
            TokenKind::Char(_) => "character literal".to_string(),
            TokenKind::Punct(p) => format!("`{}`", punct_str(*p)),
            TokenKind::Lifetime(name) => format!("lifetime `'{name}`"),
            TokenKind::Comment(_) => "comment".to_string(),
            TokenKind::Eof => "end of file".to_string(),
        }
    }

    pub(super) fn expected(&mut self, what: &str) {
        let span = self.cur_span();
        let found = self.describe_token();
        self.diags.error(
            "E-PARSE-EXPECTED",
            span,
            format!("expected {what}, found {found}"),
        );
    }

    pub(super) fn unexpected(&mut self, message: impl Into<String>) {
        let span = self.cur_span();
        self.diags.error("E-PARSE-UNEXPECTED", span, message.into());
    }

    pub(super) fn unclosed(&mut self, opener: Span, what: &str) {
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

    pub(super) fn expect_close(&mut self, close: Punct, opener: Span, what: &str) -> bool {
        if self.eat(close) {
            true
        } else {
            self.unclosed(opener, what);
            false
        }
    }

    pub(super) fn recover_and_bump(&mut self, code: &str, message: impl Into<String>) {
        let span = self.cur_span();
        self.diags.error(code, span, message);
        self.bump();
    }

    // ---------------------------------------------------------------------
    // Structural helpers
    // ---------------------------------------------------------------------

    pub(super) fn unit_expr(&mut self, span: Span) -> Expr {
        let id = self.id();
        Expr {
            id,
            kind: ExprKind::Unit,
            span,
        }
    }

    pub(super) fn infer_type(&mut self, span: Span) -> Type {
        let id = self.id();
        Type {
            id,
            kind: TypeKind::Infer,
            span,
        }
    }

    pub(super) fn unit_type(&mut self, span: Span) -> Type {
        let id = self.id();
        Type {
            id,
            kind: TypeKind::Unit,
            span,
        }
    }

    pub(super) fn never_type(&mut self, span: Span) -> Type {
        let id = self.id();
        Type {
            id,
            kind: TypeKind::Never,
            span,
        }
    }

    pub(super) fn wild_pat(&mut self, span: Span) -> Pat {
        let id = self.id();
        Pat {
            id,
            kind: PatKind::Wild,
            span,
        }
    }

    pub(super) fn empty_block(&mut self, span: Span) -> Block {
        let id = self.id();
        Block {
            id,
            stmts: Vec::new(),
            tail: None,
            span,
        }
    }

    pub(super) fn is_expr_stopper(&self) -> bool {
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

    pub(super) fn is_type_stopper(&self) -> bool {
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
                        | Punct::LBrace
                        | Punct::FatArrow
                        | Punct::Arrow
                        | Punct::Colon
                )
        )
    }

    pub(super) fn is_pattern_stopper(&self) -> bool {
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

    pub(super) fn starts_expr(&self) -> bool {
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

    pub(super) fn starts_pattern(&self) -> bool {
        Self::starts_pattern_kind(self.kind())
    }

    pub(super) fn starts_pattern_at(&self, n: usize) -> bool {
        Self::starts_pattern_kind(&self.peek_at(n).kind)
    }

    pub(super) fn starts_pattern_kind(kind: &TokenKind) -> bool {
        match kind {
            // G-6.36: a float literal is a pattern too.
            TokenKind::Ident(_)
            | TokenKind::Int { .. }
            | TokenKind::Float { .. }
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

    /// True when a `}` that is not matched by an opener appears after the
    /// current token: the enclosing block is closed later in the file, so an
    /// item here is nested in it (G-2.6) rather than a sign that its `}` is
    /// missing.
    pub(super) fn has_unmatched_close_ahead(&self) -> bool {
        let mut depth: i64 = 0;
        for token in &self.tokens[self.pos..] {
            match token.kind {
                TokenKind::Punct(Punct::LBrace) => depth += 1,
                TokenKind::Punct(Punct::RBrace) => {
                    depth -= 1;
                    if depth < 0 {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    pub(super) fn is_item_start(&self) -> bool {
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
    pub(super) fn skip_balanced(&mut self) -> Span {
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
    pub(super) fn recover_item(&mut self) {
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

    pub(super) fn recover_block_or_arm(&mut self, stop_at_rbrace: bool) {
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

    pub(super) fn parse_module_items(&mut self, close_with_rbrace: bool) -> Vec<Item> {
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
}
