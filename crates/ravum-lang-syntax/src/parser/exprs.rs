use super::*;

impl<'a> Parser<'a> {
    pub(super) fn parse_expr(&mut self, allow_struct: bool) -> Expr {
        if self.fatal {
            return self.unit_expr(self.cur_span());
        }
        if self.depth >= MAX_DEPTH {
            self.fail_depth();
            return self.unit_expr(self.cur_span());
        }
        self.descend();
        let expr = self.parse_assign(allow_struct);
        self.depth -= 1;
        expr
    }

    pub(super) fn parse_assign(&mut self, allow_struct: bool) -> Expr {
        if self.fatal {
            return self.unit_expr(self.cur_span());
        }
        if self.depth >= MAX_DEPTH {
            self.fail_depth();
            return self.unit_expr(self.cur_span());
        }
        self.descend();
        let expr = self.parse_assign_inner(allow_struct);
        self.depth -= 1;
        expr
    }

    pub(super) fn parse_assign_inner(&mut self, allow_struct: bool) -> Expr {
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
            if !Self::is_place(&lhs) {
                // The left side of an assignment must denote a place: a name, a
                // field, an index, or a dereference (G-6.11, F-TYP-005). This is
                // given as a parse error by the specification, so it is reported
                // here and not reclassified later by the checker.
                self.diags.error(
                    "E-PARSE-EXPECTED",
                    lhs.span,
                    "left side of an assignment must be a place (a name, a field, an \
                     index or a dereference)",
                );
            }
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

    /// True when the expression can be assigned to: a path, or a field/index
    /// access whose receiver is itself a place, or a dereference. Parentheses
    /// leave no node of their own, so `(t).0` and `t.0` behave identically.
    pub(super) fn is_place(expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Path(_) | ExprKind::SelfValue => true,
            ExprKind::Field { base, .. }
            | ExprKind::TupleIndex { base, .. }
            | ExprKind::Index { base, .. } => Self::is_place(base),
            ExprKind::Unary { op, expr } => matches!(op, UnOp::Deref) && Self::is_place(expr),
            _ => false,
        }
    }

    /// Assignment operators are the lowest precedence level, right-associative.
    pub(super) fn peek_assign(&self) -> Option<(Option<BinOp>, usize)> {
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

    /// A range is a `for`-clause construct in Core 1.0. Report it in the parse
    /// phase (E-UNSUPPORTED-RANGE, per `diagnostics.md` §3) instead of letting
    /// the checker reclassify it as a type error.
    pub(super) fn reject_range_outside_for(&mut self, span: Span) {
        if self.in_for_iter {
            return;
        }
        self.diags.error(
            "E-UNSUPPORTED-RANGE",
            span,
            "a range is only allowed in a `for` clause in Ravum Core 1.0",
        );
    }

    pub(super) fn parse_range(&mut self, allow_struct: bool) -> Expr {
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
            self.reject_range_outside_for(start);
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
            self.reject_range_outside_for(start);
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
    pub(super) fn peek_binop(&self) -> Option<(BinOp, u8, usize)> {
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

    pub(super) fn parse_binary(&mut self, min_prec: u8, allow_struct: bool) -> Expr {
        let outer = self.open_operand();
        let mut lhs = self.parse_cast(allow_struct);
        let mut spine = self.reach;
        while let Some((op, prec, count)) = self.peek_binop() {
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
            self.reach = self.depth;
            let rhs = self.parse_binary(prec + 1, allow_struct);
            let Some(reach) = self.chain_link(spine) else {
                break;
            };
            spine = reach;
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
        self.reach = outer.max(spine);
        lhs
    }

    pub(super) fn parse_cast(&mut self, allow_struct: bool) -> Expr {
        let outer = self.open_operand();
        let mut expr = self.parse_unary(allow_struct);
        let mut spine = self.reach;
        while self.at_kw(Kw::As) {
            let start = expr.span;
            self.bump();
            self.reach = self.depth;
            let ty = self.parse_type();
            let Some(reach) = self.chain_link(spine) else {
                break;
            };
            spine = reach;
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
        self.reach = outer.max(spine);
        expr
    }

    pub(super) fn parse_unary(&mut self, allow_struct: bool) -> Expr {
        if self.fatal {
            return self.unit_expr(self.cur_span());
        }
        if self.depth >= MAX_DEPTH {
            self.fail_depth();
            return self.unit_expr(self.cur_span());
        }
        self.descend();
        let expr = self.parse_unary_inner(allow_struct);
        self.depth -= 1;
        expr
    }

    pub(super) fn parse_unary_inner(&mut self, allow_struct: bool) -> Expr {
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
}
