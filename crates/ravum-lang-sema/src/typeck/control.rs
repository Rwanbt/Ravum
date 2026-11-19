use super::*;

impl<'p, 'w, 'f> TypeChecker<'p, 'w, 'f> {
    pub(super) fn check_if(
        &mut self,
        cond: &Expr,
        then_block: &Block,
        else_branch: Option<&Expr>,
        expected: Option<&Type>,
    ) -> Type {
        let cond_ty = self.check_expr(cond, Some(&Type::Bool));
        if !matches!(cond_ty, Type::Bool | Type::Never | Type::Error) {
            self.error(
                "E-TYP-MISMATCH",
                cond.span,
                format!("`if` condition must be `bool`, found `{cond_ty}`"),
            );
        }
        let then_ty = self.check_block(then_block, expected);
        match else_branch {
            None => {
                if !matches!(then_ty, Type::Unit | Type::Never | Type::Error) {
                    self.error(
                        "E-TYP-IF-NO-ELSE",
                        then_block.span,
                        format!(
                            "`if` without `else` has type `()` but its block returns `{then_ty}`"
                        ),
                    );
                }
                Type::Unit
            }
            Some(else_expr) => {
                let else_ty = match expected {
                    Some(exp) => self.check_expr(else_expr, Some(exp)),
                    None => {
                        if matches!(then_ty, Type::Never) {
                            self.check_expr(else_expr, None)
                        } else {
                            self.check_expr(else_expr, Some(&then_ty))
                        }
                    }
                };
                if !self.branch_types_agree(&then_ty, &else_ty) {
                    self.error(
                        "E-TYP-MISMATCH",
                        else_expr.span,
                        format!("`if` branches have different types: `{then_ty}` and `{else_ty}`"),
                    );
                }
                if matches!(then_ty, Type::Never) {
                    self.resolve_vars(&else_ty)
                } else {
                    self.resolve_vars(&then_ty)
                }
            }
        }
    }

    pub(super) fn check_while(&mut self, cond: &Expr, body: &Block) -> Type {
        let cond_ty = self.check_expr(cond, Some(&Type::Bool));
        if !matches!(cond_ty, Type::Bool | Type::Never | Type::Error) {
            self.error(
                "E-TYP-MISMATCH",
                cond.span,
                format!("`while` condition must be `bool`, found `{cond_ty}`"),
            );
        }
        self.loop_depth += 1;
        self.check_block(body, None);
        self.loop_depth -= 1;
        Type::Unit
    }

    pub(super) fn check_loop(&mut self, body: &Block) -> Type {
        // EXPR-5.11 / EXPR-6.7 / G-6.19 (MUST): `loop` has type `()`, even when
        // it has no reachable `break` and diverges semantically (EXPR-6.2).
        // Divergence is not used to widen the expression type.
        self.loop_depth += 1;
        self.check_block(body, None);
        self.loop_depth -= 1;
        Type::Unit
    }

    pub(super) fn check_for(&mut self, pat: &Pat, iter: &Expr, body: &Block) -> Type {
        let item_ty = if let ExprKind::Range {
            start,
            end,
            inclusive,
        } = &iter.kind
        {
            let start = start.as_deref();
            let end = end.as_deref();
            self.check_for_range(iter, start, end, *inclusive)
        } else {
            let iter_ty = self.check_expr(iter, None);
            match &iter_ty {
                Type::Vec(elem) => (**elem).clone(),
                // EXPR-6.4 / TYP-5.8 (MUST): the collection form consumes an
                // **owned** `Vec<T>`; `&Vec<T>` and `&[T]` are E-TYP-MISMATCH and
                // are iterated by index (`for i in 0..s.len()`). The checker used
                // to accept both and yield `&T` items, which the ownership pass
                // then rejected with E-OWN-REF-ESCAPE: the wrong phase and the
                // wrong code for a rule the type checker owns.
                // TYP-1.6: `for` cannot consume a `!` value; before this
                // check the program was accepted and rustc rejected `() is not
                // an iterator`.
                Type::Never => {
                    self.error(
                        "E-TYP-MISMATCH",
                        iter.span,
                        "`for` cannot iterate over a diverging expression of type `!`".to_string(),
                    );
                    Type::Error
                }
                Type::Error => Type::Error,
                other => {
                    self.error(
                        "E-TYP-MISMATCH",
                        iter.span,
                        format!(
                            "`for` iterates a range or an owned `Vec<T>`, found `{other}`; a `&Vec<T>` or `&[T]` is iterated by index (EXPR-6.4)"
                        ),
                    );
                    Type::Error
                }
            }
        };
        self.push_scope();
        self.check_pattern(pat, &item_ty);
        self.loop_depth += 1;
        self.check_block(body, None);
        self.loop_depth -= 1;
        self.pop_scope();
        Type::Unit
    }

    pub(super) fn check_for_range(
        &mut self,
        iter: &Expr,
        start: Option<&Expr>,
        end: Option<&Expr>,
        inclusive: bool,
    ) -> Type {
        self.record_expr_type(iter.id, &Type::Unit);
        if inclusive {
            self.error(
                "E-TYP-RANGE",
                iter.span,
                "inclusive ranges (`..=`) are not supported in Core 1.0".to_string(),
            );
        }
        let (Some(start), Some(end)) = (start, end) else {
            let message = if end.is_none() {
                "an open range is not allowed in a `for`; give an upper bound: `start..end`"
            } else {
                "`for` requires an explicit `start..end` range"
            };
            self.error("E-TYP-RANGE", iter.span, message.to_string());
            return Type::Error;
        };
        let start_lit = is_untyped_num_literal(start);
        let (start_ty, end_ty) = if start_lit {
            let e = self.check_expr(end, None);
            let s = self.check_expr(start, Some(&e));
            (s, e)
        } else {
            let s = self.check_expr(start, None);
            let e = self.check_expr(end, Some(&s));
            (s, e)
        };
        if !(start_ty.is_int() && end_ty.is_int() && start_ty == end_ty) {
            // `diagnostics.md` §3 assigns "bornes de plage de types différents ou
            // non entières" to E-TYP-RANGE, not to the generic type mismatch.
            self.error(
                "E-TYP-RANGE",
                iter.span,
                format!(
                    "range bounds must be integers of the same type: `{start_ty}` vs `{end_ty}`"
                ),
            );
            return Type::Error;
        }
        start_ty
    }

    pub(super) fn check_range(
        &mut self,
        expr: &Expr,
        start: Option<&Expr>,
        end: Option<&Expr>,
        inclusive: bool,
    ) -> Type {
        // Type whichever bounds are present.
        for bound in start.iter().chain(end.iter()) {
            self.check_expr(bound, None);
        }
        let _ = inclusive;
        let _ = expr;
        // The parser already rejects a range outside a `for` clause with
        // E-UNSUPPORTED-RANGE (parse phase); the checker must not add a second
        // code for the same construct.
        Type::Error
    }

    pub(super) fn check_match(
        &mut self,
        scrutinee: &Expr,
        arms: &[MatchArm],
        expected: Option<&Type>,
    ) -> Type {
        let scrutinee_ty = self.check_value_expr(scrutinee, None);
        let mut result: Option<Type> = None;
        let mut patterns: Vec<Option<&Pat>> = Vec::with_capacity(arms.len());
        for arm in arms {
            self.push_scope();
            self.check_pattern(&arm.pat, &scrutinee_ty);
            let body_expected: Option<&Type> = result.as_ref().or(expected);
            let body_ty = self.check_value_expr(&arm.body, body_expected);
            self.pop_scope();
            // EXPR-5.9 / G-6.44 (MUST): an unreachable arm is neither an error
            // nor removed, and produces **no diagnostic**. DIAG-1.6 keeps the
            // warning list closed to `W-RES-UNUSED-IMPORT` and
            // `W-OWN-UNUSED-VARIABLE`. An earlier `W-TYP-UNREACHABLE` emission
            // violated both rules and was removed; `registry_audit.py` now keeps
            // it from coming back.
            patterns.push(Some(&arm.pat));
            result = Some(match result {
                None => body_ty,
                Some(acc) => {
                    if !self.branch_types_agree(&acc, &body_ty) {
                        self.error(
                            "E-TYP-MISMATCH",
                            arm.body.span,
                            format!("match arms have different types: `{acc}` and `{body_ty}`"),
                        );
                        acc
                    } else if matches!(acc, Type::Never) {
                        self.resolve_vars(&body_ty)
                    } else {
                        self.resolve_vars(&acc)
                    }
                }
            });
        }
        // G-6.22 / TYP-10.4 (MUST): the coverage check descends into variant
        // payloads and tuple components, so `Some(Some(n))` no longer counts as
        // covering `Some` and a complete `(bool, bool)` enumeration is accepted.
        if !self.patterns_exhaustive(&scrutinee_ty, &patterns, 0) {
            self.error_note(
                "E-TYP-NONEXHAUSTIVE",
                expr_span_match(scrutinee, arms),
                format!(
                    "non-exhaustive match: the arms do not cover every value of `{scrutinee_ty}`"
                ),
                "cover every constructor and tuple element, or add a `_` arm",
                None,
            );
        }
        let ty = result.unwrap_or(Type::Never);
        if let Some(exp) = expected {
            if !Self::compatible(&ty, exp) {
                self.error(
                    "E-TYP-MISMATCH",
                    scrutinee.span,
                    format!("`match` has type `{ty}`, expected `{exp}`"),
                );
            }
        }
        ty
    }

    /// `future.await` (CONC-6.1): only inside an `async fn`; `Future<T>` → `T`.
    pub(super) fn check_await(&mut self, expr: &Expr, inner: &Expr) -> Type {
        let inner_ty = self.check_expr(inner, None);
        if !self.in_async_fn() {
            self.error(
                "E-CONC-AWAIT-OUTSIDE",
                expr.span,
                "`.await` is only valid inside an `async fn`; use `task::block_on` elsewhere"
                    .to_string(),
            );
            return Type::Error;
        }
        match self.resolve_vars(&inner_ty) {
            Type::Nominal { def, mut args, .. }
                if def == crate::builtins::PRELUDE_FUTURE_DEF && args.len() == 1 =>
            {
                args.remove(0)
            }
            Type::Error => Type::Error,
            other => {
                self.error(
                    "E-TYP-MISMATCH",
                    inner.span,
                    format!("`.await` expects a `Future<T>`, found `{other}`"),
                );
                Type::Error
            }
        }
    }

    pub(super) fn check_try(&mut self, expr: &Expr, inner: &Expr) -> Type {
        let inner_ty = self.check_expr(inner, None);
        if self.in_const || self.current_fn.is_none() {
            self.error(
                "E-TYP-TRY",
                expr.span,
                "`?` is only valid inside a function returning `Result` or `Option`".to_string(),
            );
            return Type::Error;
        }
        match inner_ty {
            Type::Result(ok, err) => match &self.fn_ret {
                Type::Result(_, fn_err) if **fn_err == *err => (*ok).clone(),
                other => {
                    self.error(
                        "E-TYP-TRY",
                        expr.span,
                        format!(
                            "`?` on `Result<{ok}, {err}>` requires the function to return `Result<_, {err}>`, found `{other}`"
                        ),
                    );
                    Type::Error
                }
            },
            Type::Option(inner_ok) => match &self.fn_ret {
                Type::Option(_) => (*inner_ok).clone(),
                other => {
                    self.error(
                        "E-TYP-TRY",
                        expr.span,
                        format!(
                            "`?` on `Option<{inner_ok}>` requires the function to return `Option<_>`, found `{other}`"
                        ),
                    );
                    Type::Error
                }
            },
            Type::Never | Type::Error => inner_ty,
            other => {
                self.error(
                    "E-TYP-TRY",
                    expr.span,
                    format!("`?` is not defined on `{other}`"),
                );
                Type::Error
            }
        }
    }

    pub(super) fn check_return(&mut self, value: Option<&Expr>) -> Type {
        if self.in_const || self.current_fn.is_none() {
            self.error(
                "E-TYP-RETURN",
                value.map(|v| v.span).unwrap_or(Span::dummy()),
                "`return` is not allowed here".to_string(),
            );
            if let Some(value) = value {
                self.check_expr(value, None);
            }
            return Type::Never;
        }
        let ret = self.fn_ret.clone();
        match value {
            None => {
                if ret != Type::Unit {
                    self.error(
                        "E-TYP-RETURN",
                        Span::dummy(),
                        format!("`return;` is invalid in a function returning `{ret}`"),
                    );
                }
            }
            Some(value) => {
                if ret == Type::Unit {
                    self.check_expr(value, None);
                    self.error(
                        "E-TYP-RETURN",
                        value.span,
                        "`return` with a value is invalid in a function returning `()`".to_string(),
                    );
                } else if matches!(ret, Type::Never) {
                    // Nothing inhabits `!`: `return 1;` cannot produce a value
                    // of the divergent return type. The symmetric `compatible`
                    // predicate would otherwise accept it (it treats `!` as
                    // bottom on either side).
                    let found = self.check_expr(value, Some(&ret));
                    let found = self.resolve_vars(&found);
                    if !matches!(found, Type::Never | Type::Error) {
                        self.error(
                            "E-TYP-RETURN",
                            value.span,
                            format!(
                                "`return` cannot produce a value in a function returning `!` \
                                 (found `{found}`)"
                            ),
                        );
                    }
                } else {
                    let found = self.check_expr(value, Some(&ret));
                    let found = self.resolve_vars(&found);
                    let ret = self.resolve_vars(&ret);
                    if !Self::compatible(&found, &ret)
                        && !self.unify(&found, &ret, value.span, "return")
                    {
                        self.error(
                            "E-TYP-RETURN",
                            value.span,
                            format!("`return` type `{found}` does not match `{ret}`"),
                        );
                    }
                }
            }
        }
        Type::Never
    }

    // ------------------------------------------------------------------
    // Agrégats
    // ------------------------------------------------------------------

    pub(super) fn check_array(&mut self, items: &[Expr], expected: Option<&Type>) -> Type {
        if items.is_empty() {
            self.error(
                "E-TYP-AMBIGUOUS",
                Span::dummy(),
                "cannot infer the element type of an empty array".to_string(),
            );
            return Type::Error;
        }
        // TYP-1.6: an annotation supplies the expected element type, so a
        // diverging element coerces instead of forcing `[!; N]`.
        let expected_elem = match expected {
            Some(Type::Array { elem, len }) if *len == items.len() => Some((**elem).clone()),
            _ => None,
        };
        let mut types = Vec::with_capacity(items.len());
        for item in items {
            let found = self.check_expr(item, expected_elem.as_ref());
            match &expected_elem {
                Some(elem) => {
                    // TYP-5.1/TYP-5.7: under an annotation every element is
                    // verified against the annotated element type. `coerce`
                    // only returns the expected type — it never reports a
                    // mismatch — so `let a: [i32; 2] = [1, true];` was accepted
                    // and the generated Rust failed with E0308. `unify` reports
                    // `E-TYP-MISMATCH` on the offending element.
                    self.unify(&found, elem, item.span, "array element");
                    types.push(elem.clone());
                }
                None => types.push(found),
            }
        }
        // EXPR-5.11/6.7: control expressions are not values, so they cannot be
        // array elements either.
        for item in items {
            if matches!(
                item.kind,
                ExprKind::While { .. } | ExprKind::Loop { .. } | ExprKind::For { .. }
            ) {
                self.error(
                    "E-TYP-STATEMENT-VALUE",
                    item.span,
                    "a `while`/`loop`/`for` expression is a statement and cannot be an array element"
                        .to_string(),
                );
            }
        }
        let annotated_elem = expected_elem.is_some();
        let elem = expected_elem.unwrap_or_else(|| types[0].clone());
        // TYP-4: an array is homogeneous (`[T; N]`). Without an annotation the
        // first element fixes `T`, so every later element must have the same
        // type; otherwise `[1, true]` would be accepted and the generated Rust
        // (`[1, true]`) would not compile. With an annotation each element was
        // already checked against the expected element type above.
        if !annotated_elem {
            for (item, ty) in items.iter().zip(types.iter()).skip(1) {
                if !Self::compatible(ty, &elem) {
                    self.error(
                        "E-TYP-MISMATCH",
                        item.span,
                        format!(
                            "array elements must all have the same type: \
                             the first element has type `{elem}`, this one has type `{ty}`"
                        ),
                    );
                }
            }
        }
        // TYP-1.6: without an annotation, a leading `!` leaves the element type
        // uninferable; `[panic(), …]` would otherwise generate `[!; N]`.
        if contains_never(&elem) {
            self.error(
                "E-TYP-AMBIGUOUS",
                items[0].span,
                "cannot infer an array element type from a diverging expression; \
                 annotate the array or use a non-diverging value"
                    .to_string(),
            );
        }
        Type::Array {
            elem: Box::new(elem),
            len: items.len(),
        }
    }

    pub(super) fn check_array_repeat(
        &mut self,
        expr: &Expr,
        value: &Expr,
        len: &Expr,
        expected: Option<&Type>,
    ) -> Type {
        let expected_elem = match expected {
            Some(Type::Array { elem, .. }) => Some((**elem).clone()),
            _ => None,
        };
        let found = self.check_value_expr(value, expected_elem.as_ref());
        let value_ty = match &expected_elem {
            Some(elem) => self.coerce(found, elem),
            None => found,
        };
        // TYP-1.6: an annotation supplies the element type; without one, a
        // diverging repeat value leaves `[!; N]`, which is uninferable.
        if expected_elem.is_none() && contains_never(&value_ty) {
            self.error(
                "E-TYP-AMBIGUOUS",
                value.span,
                "cannot infer an array element type from a diverging expression; \
                 annotate the array or use a non-diverging value"
                    .to_string(),
            );
        }
        let length = self.const_int_value(len);
        match length {
            Some(0) => {
                self.error(
                    "E-TYP-ARRAY-REPEAT",
                    len.span,
                    "array length must be greater than zero".to_string(),
                );
                Type::Array {
                    elem: Box::new(value_ty),
                    len: 1,
                }
            }
            Some(n) => Type::Array {
                elem: Box::new(value_ty),
                len: n as usize,
            },
            None => {
                self.error(
                    "E-TYP-ARRAY-REPEAT",
                    expr.span,
                    "array repetition length must be a constant integer".to_string(),
                );
                Type::Array {
                    elem: Box::new(value_ty),
                    len: 1,
                }
            }
        }
    }

    pub(super) fn const_int_value(&mut self, expr: &Expr) -> Option<u128> {
        match &expr.kind {
            ExprKind::Int { value, .. } => {
                self.record_expr_type(expr.id, &Type::Usize);
                Some(*value)
            }
            ExprKind::Path(path) => {
                let target =
                    resolve::resolve_value_path(self.world, self.module, &self.generics, path);
                if let Ok(ValuePath::Item(id)) = target {
                    let info = self.facts.consts.get(&id).cloned();
                    if let Some(info) = info {
                        if let ConstValue::Int(value) = &info.value {
                            if *value >= 0 {
                                self.facts
                                    .resolutions
                                    .insert(path.id, Resolution::Item { item: id });
                                self.record_expr_type(expr.id, &info.ty);
                                return Some(*value as u128);
                            }
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }
}
