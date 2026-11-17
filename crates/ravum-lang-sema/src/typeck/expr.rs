use super::*;

impl<'p, 'w, 'f> TypeChecker<'p, 'w, 'f> {
    pub(super) fn check_expr_inner(&mut self, expr: &Expr, expected: Option<&Type>) -> Type {
        match &expr.kind {
            ExprKind::Unit => Type::Unit,
            ExprKind::Int { value, suffix } => {
                self.check_int_literal(expr, *value, *suffix, expected)
            }
            ExprKind::Float { text, suffix } => {
                self.check_float_literal(expr, text, *suffix, expected)
            }
            ExprKind::Bool(_) => Type::Bool,
            ExprKind::Char(_) => Type::Char,
            ExprKind::Str(_) => Type::str_ref(),
            ExprKind::SelfValue => self.check_self_value(expr),
            ExprKind::Path(path) => self.check_path_expr(expr, path, expected),
            ExprKind::Call { callee, args } => self.check_call(expr, callee, args, expected),
            ExprKind::MethodCall {
                receiver,
                method,
                args,
            } => self.check_method_call(expr, receiver, &method.name, &method.span, args),
            ExprKind::Field { base, name } => self.check_field(base, &name.name, name.span),
            ExprKind::TupleIndex { base, index } => self.check_tuple_index(base, *index, expr.span),
            ExprKind::Index { base, index } => self.check_index(expr, base, index),
            ExprKind::Unary { op, expr: operand } => self.check_unary(expr, *op, operand, expected),
            ExprKind::Binary { op, lhs, rhs } => self.check_binary(expr, *op, lhs, rhs, expected),
            ExprKind::Assign { target, value } => self.check_assign(target, value),
            ExprKind::AssignOp { op, target, value } => {
                self.check_assign_op(expr, *op, target, value)
            }
            ExprKind::Cast { expr: value, ty } => self.check_cast(value, ty),
            ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => self.check_if(cond, then_block, else_branch.as_deref(), expected),
            ExprKind::Match { scrutinee, arms } => self.check_match(scrutinee, arms, expected),
            ExprKind::While { cond, body } => self.check_while(cond, body),
            ExprKind::Loop { body } => self.check_loop(body),
            ExprKind::For { pat, iter, body } => self.check_for(pat, iter, body),
            ExprKind::Block(block) => self.check_block(block, expected),
            ExprKind::Unsafe(block) => {
                self.unsafe_depth += 1;
                let ty = self.check_block(block, expected);
                self.unsafe_depth -= 1;
                ty
            }
            ExprKind::Tuple(items) => {
                // Tuples are heterogeneous by design (TYP-1.2): each element is
                // checked against the corresponding expected element, never
                // unified with its siblings. Reusing the array checker here
                // wrongly rejected `(i32, bool)`.
                let types = self.check_tuple_items(items, expected);
                Type::Tuple(types)
            }
            ExprKind::Array(items) => self.check_array(items, expected),
            ExprKind::ArrayRepeat { value, len } => {
                self.check_array_repeat(expr, value, len, expected)
            }
            ExprKind::StructLit { path, fields } => {
                self.check_struct_lit(expr, path, fields, expected)
            }
            ExprKind::Range {
                start,
                end,
                inclusive,
            } => self.check_range(expr, start.as_deref(), end.as_deref(), *inclusive),
            ExprKind::Try(inner) => self.check_try(expr, inner),
            ExprKind::Return(value) => self.check_return(value.as_deref()),
            ExprKind::Break => {
                if self.loop_depth == 0 {
                    self.error(
                        "E-TYP-BREAK-OUTSIDE",
                        expr.span,
                        "`break` outside of a loop".to_string(),
                    );
                }
                Type::Never
            }
            ExprKind::Continue => {
                if self.loop_depth == 0 {
                    self.error(
                        "E-TYP-BREAK-OUTSIDE",
                        expr.span,
                        "`continue` outside of a loop".to_string(),
                    );
                }
                Type::Never
            }
        }
    }

    pub(super) fn check_tuple_items(
        &mut self,
        items: &[Expr],
        expected: Option<&Type>,
    ) -> Vec<Type> {
        // Only a same-arity tuple expectation can guide individual elements; a
        // different arity is reported by the enclosing unification, not here.
        let expected_items = match expected {
            Some(Type::Tuple(expected_items)) if expected_items.len() == items.len() => {
                Some(expected_items.clone())
            }
            _ => None,
        };
        let mut types = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            // EXPR-5.11/6.7: `while`/`loop`/`for` are not value expressions.
            if matches!(
                item.kind,
                ExprKind::While { .. } | ExprKind::Loop { .. } | ExprKind::For { .. }
            ) {
                self.error(
                    "E-TYP-STATEMENT-VALUE",
                    item.span,
                    "a `while`/`loop`/`for` expression is a statement and cannot be a tuple element"
                        .to_string(),
                );
            }
            let expected_item = expected_items.as_ref().and_then(|list| list.get(index));
            let found = self.check_expr(item, expected_item);
            let found = match expected_item {
                // TYP-1.6: an annotated tuple supplies the element type, so a
                // `!` element coerces instead of producing `(!, T)`.
                Some(expected) => self.coerce(found, expected),
                None => found,
            };
            // TYP-1.6: without an expected element type, a diverging element
            // would make the tuple `(!, …)`, which is uninhabitable and not
            // representable by the reference backend. An explicit annotation
            // (or a non-diverging value) is required.
            if expected_item.is_none() && contains_never(&found) {
                self.error(
                    "E-TYP-AMBIGUOUS",
                    item.span,
                    "cannot infer a tuple element type from a diverging expression; \
                     annotate the tuple or use a non-diverging value"
                        .to_string(),
                );
            }
            types.push(found);
        }
        types
    }

    // ------------------------------------------------------------------
    // Littéraux
    // ------------------------------------------------------------------

    pub(super) fn check_int_literal(
        &mut self,
        expr: &Expr,
        value: u128,
        suffix: Option<IntTy>,
        expected: Option<&Type>,
    ) -> Type {
        let ty = match suffix {
            Some(suffix) => int_type(suffix),
            None => match expected {
                Some(t) if t.is_int() => t.clone(),
                _ => Type::I32,
            },
        };
        if !int_value_fits(value, &ty) {
            self.error(
                "E-TYP-LITERAL-RANGE",
                expr.span,
                format!("integer literal `{value}` does not fit in `{ty}`"),
            );
        }
        ty
    }

    pub(super) fn check_float_literal(
        &mut self,
        expr: &Expr,
        text: &str,
        suffix: Option<ast::FloatTy>,
        expected: Option<&Type>,
    ) -> Type {
        let ty = match suffix {
            Some(ast::FloatTy::F32) => Type::F32,
            Some(ast::FloatTy::F64) => Type::F64,
            None => match expected {
                Some(Type::F32) => Type::F32,
                _ => Type::F64,
            },
        };
        let finite = match ty {
            Type::F32 => text.parse::<f32>().map(|v| v.is_finite()).unwrap_or(false),
            _ => text.parse::<f64>().map(|v| v.is_finite()).unwrap_or(false),
        };
        if !finite {
            self.error(
                "E-TYP-FLOAT-LITERAL",
                expr.span,
                format!("floating-point literal `{text}` is not representable in `{ty}`"),
            );
        }
        ty
    }

    // ------------------------------------------------------------------
    // `self`
    // ------------------------------------------------------------------

    pub(super) fn check_self_value(&mut self, expr: &Expr) -> Type {
        match self.lookup_local("self") {
            Some(local) => {
                self.facts.resolutions.insert(
                    expr.id,
                    Resolution::Local {
                        name: "self".to_string(),
                        mutable: local.mutable,
                        decl: local.decl,
                    },
                );
                local.ty
            }
            None => {
                self.error(
                    "E-RES-SELF",
                    expr.span,
                    "`self` is only available in a method".to_string(),
                );
                Type::Error
            }
        }
    }

    // ------------------------------------------------------------------
    // Chemins
    // ------------------------------------------------------------------

    pub(super) fn check_path_expr(
        &mut self,
        expr: &Expr,
        path: &Path,
        expected: Option<&Type>,
    ) -> Type {
        // Une liaison locale masque tout le reste. Les faits de résolution sont
        // indexés par l'identifiant du **nœud Path** (pas celui de l'expression).
        if path.root == PathRoot::Plain && path.segments.len() == 1 {
            let name = &path.segments[0].name.name;
            if let Some(local) = self.lookup_local(name) {
                self.used_locals.insert(name.clone());
                self.facts.resolutions.insert(
                    path.id,
                    Resolution::Local {
                        name: name.clone(),
                        mutable: local.mutable,
                        decl: local.decl,
                    },
                );
                return local.ty;
            }
        }
        match resolve::resolve_value_path(self.world, self.module, &self.generics, path) {
            Ok(target) => {
                self.record_value_resolution(path.id, &target);
                self.type_of_value_path(expr, &target, expected)
            }
            Err(error) => {
                self.emit_path_error(expr.span, &error);
                self.facts.resolutions.insert(path.id, Resolution::Error);
                Type::Error
            }
        }
    }

    pub(super) fn emit_path_error(&mut self, span: Span, error: &PathError) {
        self.error(error.code, span, error.message.clone());
    }

    pub(super) fn record_value_resolution(&mut self, id: NodeId, target: &ValuePath) {
        let resolution = match target {
            ValuePath::Item(item) => Resolution::Item { item: *item },
            ValuePath::Variant { enum_item, variant } => Resolution::Variant {
                enum_item: *enum_item,
                variant: *variant,
            },
            ValuePath::BuiltinFn(path) => Resolution::BuiltinFn {
                path: (*path).to_string(),
            },
            ValuePath::BuiltinNone => Resolution::BuiltinConst {
                path: OPTION_NONE.to_string(),
            },
            ValuePath::BuiltinCtor { ctor } => Resolution::BuiltinFn {
                path: ctor.path().to_string(),
            },
            ValuePath::AssocFn(item) => Resolution::Item { item: *item },
            ValuePath::Module => Resolution::Module,
            ValuePath::BuiltinModule => Resolution::BuiltinModule,
            ValuePath::Type(_) => Resolution::Error,
        };
        self.facts.resolutions.insert(id, resolution);
    }

    pub(super) fn type_of_value_path(
        &mut self,
        expr: &Expr,
        target: &ValuePath,
        expected: Option<&Type>,
    ) -> Type {
        match target {
            ValuePath::Item(item) => {
                if let Some(info) = self.facts.consts.get(item) {
                    return info.ty.clone();
                }
                if self.world.consts.contains_key(item) {
                    return Type::Error;
                }
                self.error(
                    "E-TYP-CALL",
                    expr.span,
                    "a function item cannot be used as a value in Core 1.0".to_string(),
                );
                Type::Error
            }
            ValuePath::BuiltinFn(path) => {
                self.error(
                    "E-TYP-CALL",
                    expr.span,
                    format!("builtin function `{path}` must be called"),
                );
                Type::Error
            }
            ValuePath::BuiltinCtor { ctor } => {
                let path = ctor.path();
                self.error(
                    "E-TYP-CALL",
                    expr.span,
                    format!("variant constructor `{path}` requires arguments"),
                );
                Type::Error
            }
            ValuePath::BuiltinNone => self.builtin_none_type(expr, expected),
            ValuePath::Variant { enum_item, variant } => {
                let info = match self.facts.enums.get(enum_item) {
                    Some(info) => info.clone(),
                    None => return Type::Error,
                };
                let variant_info = match info.variants.get(*variant) {
                    Some(v) => v.clone(),
                    None => return Type::Error,
                };
                if variant_info.kind != crate::facts::VariantKind::Unit {
                    self.error(
                        "E-TYP-CALL",
                        expr.span,
                        format!(
                            "variant `{}::{}` requires arguments",
                            info.name, variant_info.name
                        ),
                    );
                    return Type::Error;
                }
                let args =
                    self.builtin_enum_args(expected, *enum_item, info.generics.len(), expr.span);
                Type::Nominal {
                    def: *enum_item,
                    name: info.name.clone(),
                    args,
                }
            }
            ValuePath::AssocFn(item) => {
                let _ = item;
                self.error(
                    "E-TYP-CALL",
                    expr.span,
                    "an associated function must be called".to_string(),
                );
                Type::Error
            }
            ValuePath::Module | ValuePath::BuiltinModule => {
                self.error(
                    "E-TYP-CALL",
                    expr.span,
                    "a module cannot be used as a value".to_string(),
                );
                Type::Error
            }
            ValuePath::Type(t) => {
                self.error(
                    "E-TYP-CALL",
                    expr.span,
                    format!("type `{t}` cannot be used as a value"),
                );
                Type::Error
            }
        }
    }

    pub(super) fn builtin_enum_args(
        &mut self,
        expected: Option<&Type>,
        def: NodeId,
        arity: usize,
        span: Span,
    ) -> Vec<Type> {
        if let Some(Type::Nominal { def: d, args, .. }) = expected {
            if *d == def && args.len() == arity {
                return args.clone();
            }
        }
        if arity == 0 {
            return Vec::new();
        }
        let mut args = Vec::with_capacity(arity);
        for _ in 0..arity {
            args.push(self.fresh_var(span));
        }
        args
    }

    pub(super) fn builtin_none_type(&mut self, expr: &Expr, expected: Option<&Type>) -> Type {
        match expected {
            Some(Type::Option(inner)) => Type::option((**inner).clone()),
            _ => Type::option(self.fresh_var(expr.span)),
        }
    }

    // ------------------------------------------------------------------
    // Appels
    // ------------------------------------------------------------------
}
