use super::*;

impl<'a> Parser<'a> {
    pub(super) fn parse_array_expr(&mut self) -> Expr {
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

    pub(super) fn parse_condition(&mut self) -> Expr {
        // Assignment is a statement, never an expression (G-6.8).
        self.forbid_assign += 1;
        let expr = self.parse_expr(false);
        self.forbid_assign -= 1;
        expr
    }

    pub(super) fn parse_if_expr(&mut self) -> Expr {
        let start = self.cur_span();
        self.bump(); // `if`
        let cond = self.parse_condition();
        let then_block = self.parse_block();
        let mut else_branch = None;
        if self.eat_kw(Kw::Else) {
            if self.at_kw(Kw::If) {
                // An `else if` chain nests one `if` per link: it counts
                // against the nesting limit like any other nesting.
                if self.depth >= MAX_DEPTH {
                    self.fail_depth();
                } else {
                    self.descend();
                    let nested = self.parse_if_expr();
                    self.depth -= 1;
                    else_branch = Some(Box::new(nested));
                }
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

    pub(super) fn parse_while_expr(&mut self) -> Expr {
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

    pub(super) fn parse_loop_expr(&mut self) -> Expr {
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

    pub(super) fn parse_for_expr(&mut self) -> Expr {
        let start = self.cur_span();
        self.bump(); // `for`
        let pat = self.parse_pattern();
        if self.eat_kw(Kw::In) {
            // ok
        } else {
            self.expected("`in`");
        }
        let outer = self.in_for_iter;
        self.in_for_iter = true;
        let iter = self.parse_condition();
        self.in_for_iter = outer;
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

    pub(super) fn parse_match_expr(&mut self) -> Expr {
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
            // G-6.21/G-6.42 (MUST): `match_arm = pattern "=>" ( expr "," | block )`.
            // A body that starts as a block-like expression ends with it, so
            // the next arm may start with `(` or `-` without being read as a
            // call or a subtraction of the block.
            let starts_block_like = self.at(Punct::LBrace)
                || self.at_kw(Kw::If)
                || self.at_kw(Kw::Match)
                || self.at_kw(Kw::While)
                || self.at_kw(Kw::Loop)
                || self.at_kw(Kw::For);
            let body = if starts_block_like {
                self.parse_primary(true)
            } else {
                self.parse_expr(true)
            };
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

    // The lexer validates literal suffixes (LEX-8.3) and reports
    // `E-LEX-INT-SUFFIX` / `E-LEX-NUMBER`, so a token never carries an unknown
    // suffix in practice. These parsers are the defensive, fail-closed path:
    // should an unknown suffix ever reach the parser, it is reported with the
    // same canonical lexer code instead of being dropped. `E-PARSE-BAD-SUFFIX`
    // used to be registered for this case; it duplicated `E-LEX-INT-SUFFIX`
    // with no rule assigning it to the parser and was unreachable
    // (`docs/qualification/T2-FINDINGS.md` §20).

    pub(super) fn int_suffix(&mut self, suffix: Option<String>, span: Span) -> Option<IntTy> {
        match suffix {
            None => None,
            Some(name) => match IntTy::from_name(&name) {
                Some(ty) => Some(ty),
                None => {
                    self.diags.error(
                        "E-LEX-INT-SUFFIX",
                        span,
                        format!("unknown integer literal suffix `{name}`"),
                    );
                    None
                }
            },
        }
    }

    pub(super) fn float_suffix(&mut self, suffix: Option<String>, span: Span) -> Option<FloatTy> {
        match suffix {
            None => None,
            Some(name) => match FloatTy::from_name(&name) {
                Some(ty) => Some(ty),
                None => {
                    self.diags.error(
                        "E-LEX-NUMBER",
                        span,
                        format!("unknown float literal suffix `{name}`"),
                    );
                    None
                }
            },
        }
    }
}
