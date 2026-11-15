use super::*;

impl<'a> Parser<'a> {
    pub(super) fn parse_block(&mut self) -> Block {
        let start = self.cur_span();
        if !self.eat(Punct::LBrace) {
            self.expected("`{`");
            return self.empty_block(Span::new(self.file, start.start, start.start));
        }
        self.parse_block_after_open(start)
    }

    pub(super) fn parse_block_after_open(&mut self, start: Span) -> Block {
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

    pub(super) fn parse_let_stmt(&mut self) -> Stmt {
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

    pub(super) fn parse_pattern(&mut self) -> Pat {
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
                "E-UNSUPPORTED-TOKEN",
                op,
                "or-patterns (`a | b`) are not supported in Ravum Core 1.0",
            );
            if self.starts_pattern() {
                let _ = self.parse_pattern();
            }
        }
        pat
    }
}
