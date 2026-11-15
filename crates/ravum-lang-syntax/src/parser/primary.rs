use super::*;

impl<'a> Parser<'a> {
    pub(super) fn parse_postfix(&mut self, allow_struct: bool) -> Expr {
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

    pub(super) fn parse_call_args(&mut self, opener: Span) -> Vec<Expr> {
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

    pub(super) fn parse_struct_lit_fields(&mut self, opener: Span) -> Vec<FieldInit> {
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
                    "E-UNSUPPORTED-RANGE",
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

    pub(super) fn parse_primary(&mut self, allow_struct: bool) -> Expr {
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
    pub(super) fn parse_closure_unsupported(&mut self) -> Expr {
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

    pub(super) fn parse_paren_expr(&mut self) -> Expr {
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
}
