use super::*;

impl<'p, 'w, 'f> TypeChecker<'p, 'w, 'f> {
    pub(super) fn check_call(
        &mut self,
        expr: &Expr,
        callee: &Expr,
        args: &[Expr],
        expected: Option<&Type>,
    ) -> Type {
        let path = match &callee.kind {
            ExprKind::Path(path) => path,
            _ => {
                // Even a diverging callee (`(return)()`) is refused: the
                // backend has no call target to lower, and rustc refuses to
                // call a `!` value too (check => build).
                let callee_ty = self.check_expr(callee, None);
                self.error(
                    "E-TYP-CALL",
                    callee.span,
                    format!("value of type `{callee_ty}` is not callable"),
                );
                for arg in args {
                    self.check_expr(arg, None);
                }
                self.facts.calls.insert(expr.id, CallTarget::Error);
                return Type::Error;
            }
        };

        // Liaison locale utilisée comme fonction.
        if path.root == PathRoot::Plain && path.segments.len() == 1 {
            let name = &path.segments[0].name.name;
            if self.lookup_local(name).is_some() {
                let local = self.lookup_local(name).unwrap();
                self.error(
                    "E-TYP-CALL",
                    callee.span,
                    format!("value `{name}` of type `{}` is not callable", local.ty),
                );
                for arg in args {
                    self.check_expr(arg, None);
                }
                self.facts.resolutions.insert(path.id, Resolution::Error);
                self.facts.expr_types.insert(callee.id, local.ty);
                self.facts.calls.insert(expr.id, CallTarget::Error);
                return Type::Error;
            }
        }

        let target =
            match resolve::resolve_value_path(self.world, self.module, &self.generics, path) {
                Ok(target) => target,
                Err(error) => {
                    self.emit_path_error(callee.span, &error);
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    self.facts.resolutions.insert(path.id, Resolution::Error);
                    self.facts.expr_types.insert(callee.id, Type::Error);
                    self.facts.calls.insert(expr.id, CallTarget::Error);
                    return Type::Error;
                }
            };
        self.record_value_resolution(path, &target);

        let result = match &target {
            ValuePath::Item(id) => {
                let is_extern = self
                    .facts
                    .fn_sigs
                    .get(id)
                    .map(|signature| signature.is_extern)
                    .unwrap_or(false);
                if self.world.fns.contains_key(id) || is_extern {
                    self.check_user_fn_call(*id, args, expected, expr.span, None)
                } else {
                    self.error(
                        "E-TYP-CALL",
                        callee.span,
                        "a constant is not callable".to_string(),
                    );
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    Type::Error
                }
            }
            ValuePath::AssocFn(id) => {
                self.check_user_fn_call(*id, args, expected, expr.span, Some(*id))
            }
            ValuePath::BuiltinFn(path) => {
                if let Some(assoc) = builtins::builtin_assoc_fn(path) {
                    self.check_builtin_assoc_call(assoc, args, expected, expr.span)
                } else {
                    let sig = builtins::resolved_builtin_fn(path);
                    self.check_builtin_fn_call(sig, args, expected, expr.span)
                }
            }
            ValuePath::Variant { enum_item, variant } => {
                self.check_variant_call(*enum_item, *variant, args, expected, expr.span)
            }
            ValuePath::BuiltinCtor { ctor } => {
                self.check_builtin_ctor_call(*ctor, args, expected, expr.span)
            }
            ValuePath::BuiltinNone => {
                self.error(
                    "E-TYP-ARITY",
                    expr.span,
                    format!("`{OPTION_NONE}` is a value, not a function"),
                );
                for arg in args {
                    self.check_expr(arg, None);
                }
                Type::Error
            }
            ValuePath::Module | ValuePath::BuiltinModule => {
                self.error(
                    "E-TYP-CALL",
                    callee.span,
                    "a module is not callable".to_string(),
                );
                for arg in args {
                    self.check_expr(arg, None);
                }
                Type::Error
            }
            ValuePath::Type(t) => {
                self.error(
                    "E-TYP-CALL",
                    callee.span,
                    format!("type `{t}` is not callable"),
                );
                for arg in args {
                    self.check_expr(arg, None);
                }
                Type::Error
            }
        };

        // Le nœud `callee` est un nœud d'expression : il reçoit le type du
        // résultat de l'appel (le codegen ne s'en sert pas, mais aucun nœud
        // d'un programme accepté ne doit manquer).
        self.facts.expr_types.insert(callee.id, result.clone());
        self.facts.calls.insert(
            expr.id,
            match &target {
                ValuePath::Item(id) => CallTarget::UserFn { item: *id },
                ValuePath::AssocFn(id) => CallTarget::UserFn { item: *id },
                ValuePath::Variant { enum_item, variant } => CallTarget::VariantCtor {
                    enum_item: *enum_item,
                    variant: *variant,
                },
                ValuePath::BuiltinFn(path) => CallTarget::BuiltinFn {
                    path: (*path).to_string(),
                },
                ValuePath::BuiltinCtor { ctor } => CallTarget::BuiltinFn {
                    path: ctor.path().to_string(),
                },
                _ => CallTarget::Error,
            },
        );
        result
    }

    pub(super) fn check_user_fn_call(
        &mut self,
        item: NodeId,
        args: &[Expr],
        expected: Option<&Type>,
        span: Span,
        _assoc: Option<NodeId>,
    ) -> Type {
        let Some(sig) = self.facts.fn_sigs.get(&item).cloned() else {
            for arg in args {
                self.check_expr(arg, None);
            }
            return Type::Error;
        };
        if sig.is_extern && self.unsafe_depth == 0 {
            self.error(
                "E-UNSAFE-CONTEXT",
                span,
                format!(
                    "extern function `{}` can only be called inside an `unsafe {{ ... }}` block",
                    sig.name
                ),
            );
        }
        // Correspondance impl génériques <-> type de retour attendu.
        let mut subst: BTreeMap<usize, Type> = BTreeMap::new();
        if let Some(entry) = self.method_entry_by_item(item) {
            if !self.method_visible(&entry) {
                self.error(
                    "E-RES-PRIVATE",
                    span,
                    format!("function `{}` is private", entry.name),
                );
            }
            if let Some(Type::Nominal { def, args, .. }) = &expected {
                if *def == nominal_def(&entry.self_ty).unwrap_or(*def) {
                    self.bind_impl_generics(&entry, args, &mut subst);
                }
            }
        }
        if args.len() != sig.params.len() {
            self.error(
                "E-TYP-ARITY",
                span,
                format!(
                    "function `{}` expects {} argument(s), found {}",
                    sig.name,
                    sig.params.len(),
                    args.len()
                ),
            );
        }
        // A callee parameter not bound yet is expected as an inference
        // variable, shared by every argument of the call: an argument checked
        // against a bare `T` (`g(Some(1), 1)` for `g<T>(Option<T>, T)`) would
        // otherwise be refused for producing `Option<i32>`, not `Option<T>`.
        let mut expected_params: BTreeMap<usize, Type> = BTreeMap::new();
        for (index, arg) in args.iter().enumerate() {
            if let Some(param) = sig.params.get(index) {
                for (k, v) in &subst {
                    expected_params.entry(*k).or_insert_with(|| v.clone());
                }
                let param_ty = self.instantiate_with(&param.ty, span, &mut expected_params);
                let arg_ty = self.check_arg(arg, &param_ty);
                if !self.unify_pattern(&param.ty, &arg_ty, &mut subst) {
                    self.error(
                        "E-TYP-MISMATCH",
                        arg.span,
                        format!(
                            "argument {} of `{}`: expected `{}`, found `{}`",
                            index + 1,
                            sig.name,
                            substitute(&param.ty, &subst),
                            arg_ty
                        ),
                    );
                }
            } else {
                self.check_expr(arg, None);
            }
        }
        let ret = substitute(&sig.ret, &subst);
        let ret = self.instantiate(&ret, span);
        if let Some(expected) = expected {
            self.unify_pattern(&ret, expected, &mut subst);
        }
        self.resolve_vars(&ret)
    }

    pub(super) fn method_visible(&self, entry: &resolve::MethodEntry) -> bool {
        if entry.vis.is_public() {
            return true;
        }
        let mut current = Some(self.module);
        while let Some(m) = current {
            if m == entry.def_module {
                return true;
            }
            current = self.world.modules[m].parent;
        }
        false
    }

    pub(super) fn method_entry_by_item(&self, item: NodeId) -> Option<resolve::MethodEntry> {
        for methods in self.world.impls.values() {
            if let Some(entry) = methods.iter().find(|m| m.item == item) {
                return Some(entry.clone());
            }
        }
        None
    }

    pub(super) fn bind_impl_generics(
        &self,
        entry: &resolve::MethodEntry,
        actual_args: &[Type],
        subst: &mut BTreeMap<usize, Type>,
    ) {
        let Type::Nominal { args, .. } = &entry.self_ty else {
            return;
        };
        for (index, pattern) in args.iter().enumerate() {
            if let (Type::Param { index: param, .. }, Some(actual)) =
                (pattern, actual_args.get(index))
            {
                subst.insert(*param, actual.clone());
            }
        }
    }

    pub(super) fn check_arg(&mut self, arg: &Expr, expected: &Type) -> Type {
        let expected = self.resolve_vars(expected);
        let needs_late_coercion = matches!(
            &expected,
            Type::Ref { inner, .. } if matches!(**inner, Type::Str | Type::Slice(_))
        );
        let found = if needs_late_coercion {
            self.check_value_expr(arg, None)
        } else {
            self.check_value_expr(arg, Some(&expected))
        };
        self.coerce(found, &expected)
    }

    /// Les deux seules coercitions implicites admises, en argument et en
    /// récepteur : `&String → &str` et `&[T; N] → &[T]` (TYP-13.1). Un
    /// `String` **possédé** n'est pas un `&String` : `String → &str` est
    /// explicite (`.as_str()`, TYP-8.5) et n'est pas admis ici.
    pub(super) fn coerce(&self, found: Type, expected: &Type) -> Type {
        if matches!(found, Type::Never) || found == *expected {
            return expected.clone();
        }
        match (expected, &found) {
            (
                Type::Ref {
                    mutable: false,
                    inner: exp_inner,
                },
                found_ty,
            ) => match (&**exp_inner, found_ty) {
                // TYP-13.1: the only text adjustment is `&String -> &str`.
                // TYP-8.5: `String -> &str` is explicit (`.as_str()`); an
                // owned `String` argument used to be coerced here and the
                // generated Rust failed with E0308.
                (
                    Type::Str,
                    Type::Ref {
                        mutable: false,
                        inner,
                    },
                ) if **inner == Type::String => expected.clone(),
                (
                    Type::Slice(elem),
                    Type::Ref {
                        mutable: false,
                        inner,
                    },
                ) => match &**inner {
                    Type::Array { elem: felem, .. } if elem == felem => expected.clone(),
                    _ => found,
                },
                _ => found,
            },
            _ => found,
        }
    }

    pub(super) fn check_builtin_fn_call(
        &mut self,
        sig: builtins::BuiltinFnSig,
        args: &[Expr],
        expected: Option<&Type>,
        span: Span,
    ) -> Type {
        if sig.path == "std::panic::assert_eq" {
            return self.check_assert_eq(args, sig.path);
        }
        let params: Vec<Type> = builtins::fn_param_types(&sig)
            .iter()
            .map(|p| self.instantiate(p, span))
            .collect();
        if args.len() != params.len() {
            self.error(
                "E-TYP-ARITY",
                span,
                format!(
                    "`{}` expects {} argument(s), found {}",
                    sig.path,
                    params.len(),
                    args.len()
                ),
            );
        }
        let mut subst: BTreeMap<usize, Type> = BTreeMap::new();
        for (index, arg) in args.iter().enumerate() {
            if let Some(param) = params.get(index) {
                let expected_param = self.resolve_vars(&substitute(param, &subst));
                let arg_ty = self.check_arg(arg, &expected_param);
                if !self.unify_pattern(param, &arg_ty, &mut subst) {
                    self.error(
                        "E-TYP-MISMATCH",
                        arg.span,
                        format!(
                            "argument {} of `{}`: expected `{}`, found `{}`",
                            index + 1,
                            sig.path,
                            expected_param,
                            arg_ty
                        ),
                    );
                }
            } else {
                self.check_expr(arg, None);
            }
        }
        let ret = self.resolve_vars(&substitute(&builtins::fn_ret_type(&sig), &subst));
        if let Some(expected) = expected {
            self.unify_pattern(&ret, expected, &mut subst);
        }
        self.resolve_vars(&ret)
    }

    /// True when the expression can be borrowed mutably: a `let mut` binding, a
    /// field or index of a mutable place, or a dereference of `&mut T`.
    pub(super) fn is_mutable_place(&self, expr: &Expr) -> bool {
        // A value of type `&mut T` can always be reborrowed mutably, whatever
        // the mutability of the binding that holds it.
        if matches!(
            self.facts.expr_types.get(&expr.id),
            Some(Type::Ref { mutable: true, .. })
        ) {
            return true;
        }
        match &expr.kind {
            ExprKind::Path(path) => matches!(
                self.facts.resolutions.get(&path.id),
                Some(crate::facts::Resolution::Local { mutable: true, .. })
            ),
            ExprKind::Field { base, .. }
            | ExprKind::TupleIndex { base, .. }
            | ExprKind::Index { base, .. } => self.is_mutable_place(base),
            ExprKind::Unary { op, expr } => {
                matches!(op, UnOp::Deref)
                    && matches!(
                        self.facts.expr_types.get(&expr.id),
                        Some(Type::Ref { mutable: true, .. })
                    )
            }
            _ => false,
        }
    }

    /// A `&mut self` receiver must be a mutable place (TYP-3.2, OWN-7.4): a
    /// `let mut` binding, `self` in a `&mut self` method, or a reborrowable
    /// `&mut T` value. A temporary (struct literal, call result) is not a place
    /// and is rejected by `place_mutability` returning `None`.
    pub(super) fn receiver_is_mutable(&self, receiver: &Expr) -> bool {
        self.is_mutable_place(receiver) || matches!(self.place_mutability(receiver), Some(true))
    }

    pub(super) fn check_assert_eq(&mut self, args: &[Expr], _path: &'static str) -> Type {
        if args.len() != 2 {
            self.error(
                "E-TYP-ARITY",
                args.first().map(|a| a.span).unwrap_or(Span::dummy()),
                "`assert_eq` expects 2 arguments".to_string(),
            );
        }
        let (a_ty, b_ty) = if let (Some(first), Some(second)) = (args.first(), args.get(1)) {
            if is_untyped_num_literal(first) {
                let b = self.check_expr(second, None);
                let a = self.check_expr(first, Some(&b));
                (a, b)
            } else {
                let a = self.check_expr(first, None);
                let b = self.check_expr(second, Some(&a));
                (a, b)
            }
        } else {
            for arg in args {
                self.check_expr(arg, None);
            }
            (Type::Error, Type::Error)
        };
        if !(Self::compatible(&a_ty, &b_ty) || (is_text_like(&a_ty) && is_text_like(&b_ty))) {
            self.error(
                "E-TYP-MISMATCH",
                args.get(1).map(|a| a.span).unwrap_or(Span::dummy()),
                format!("`assert_eq` arguments have different types `{a_ty}` and `{b_ty}`"),
            );
        } else {
            // `assert_eq` compares with `==` (STD-8.2), so a type without `==`
            // cannot be asserted: `Vec`, `Option`, `Result`, structs and enums
            // have no equality in Core 1.0.
            let comparable = (is_text_like(&a_ty) && is_text_like(&b_ty))
                || (a_ty == b_ty
                    && (a_ty.is_int()
                        || a_ty.is_float()
                        || a_ty == Type::Bool
                        || a_ty == Type::Char));
            if !comparable && !matches!(a_ty, Type::Error) {
                self.error(
                    "E-TYP-EQ",
                    args.first().map(|a| a.span).unwrap_or(Span::dummy()),
                    format!("`assert_eq` requires a type with `==`; `{a_ty}` has none in Core 1.0"),
                );
            }
        }
        Type::Unit
    }

    pub(super) fn check_builtin_assoc_call(
        &mut self,
        sig: builtins::BuiltinAssocFn,
        args: &[Expr],
        expected: Option<&Type>,
        span: Span,
    ) -> Type {
        let params: Vec<Type> = builtins::assoc_param_types(&sig)
            .iter()
            .map(|p| self.instantiate(p, span))
            .collect();
        if args.len() != params.len() {
            self.error(
                "E-TYP-ARITY",
                span,
                format!(
                    "`{}` expects {} argument(s), found {}",
                    sig.path,
                    params.len(),
                    args.len()
                ),
            );
        }
        let mut subst: BTreeMap<usize, Type> = BTreeMap::new();
        // Le type de retour attendu fixe les paramètres génériques (`Vec::new`).
        let ret_pattern = self.instantiate(&builtins::assoc_ret_type(&sig), span);
        if let Some(exp) = expected {
            self.unify_pattern(&ret_pattern, exp, &mut subst);
        }
        for (index, arg) in args.iter().enumerate() {
            if let Some(param) = params.get(index) {
                let expected_param = self.resolve_vars(&substitute(param, &subst));
                let arg_ty = self.check_arg(arg, &expected_param);
                if !self.unify_pattern(param, &arg_ty, &mut subst) {
                    self.error(
                        "E-TYP-MISMATCH",
                        arg.span,
                        format!(
                            "argument {} of `{}`: expected `{}`, found `{}`",
                            index + 1,
                            sig.path,
                            expected_param,
                            arg_ty
                        ),
                    );
                }
            } else {
                self.check_expr(arg, None);
            }
        }
        self.resolve_vars(&ret_pattern)
    }

    pub(super) fn check_variant_call(
        &mut self,
        enum_item: NodeId,
        variant: usize,
        args: &[Expr],
        expected: Option<&Type>,
        span: Span,
    ) -> Type {
        let info = match self.facts.enums.get(&enum_item) {
            Some(info) => info.clone(),
            None => {
                for arg in args {
                    self.check_expr(arg, None);
                }
                return Type::Error;
            }
        };
        let variant_info = match info.variants.get(variant) {
            Some(v) => v.clone(),
            None => return Type::Error,
        };
        let mut subst: BTreeMap<usize, Type> = BTreeMap::new();
        if let Some(Type::Nominal {
            def,
            args: exp_args,
            ..
        }) = expected
        {
            if *def == enum_item && exp_args.len() == info.generics.len() {
                for (i, arg) in exp_args.iter().enumerate() {
                    subst.insert(i, arg.clone());
                }
            }
        }
        match variant_info.kind {
            crate::facts::VariantKind::Tuple => {
                if args.len() != variant_info.tuple_types.len() {
                    self.error(
                        "E-TYP-ARITY",
                        span,
                        format!(
                            "variant `{}::{}` expects {} argument(s), found {}",
                            info.name,
                            variant_info.name,
                            variant_info.tuple_types.len(),
                            args.len()
                        ),
                    );
                }
                for (index, arg) in args.iter().enumerate() {
                    if let Some(pattern) = variant_info.tuple_types.get(index) {
                        let expected_ty = substitute(pattern, &subst);
                        let arg_ty = self.check_arg(arg, &expected_ty);
                        // TYP-1.6: `!` must not determine a generic variant
                        // argument. `check_arg`/`coerce` returns the expected
                        // type for a `!` argument, so the raw expression type is
                        // read from the facts; an unsubstituted type parameter
                        // means there is nothing to infer from.
                        let raw_arg_ty = self
                            .facts
                            .expr_types
                            .get(&arg.id)
                            .cloned()
                            .unwrap_or_else(|| arg_ty.clone());
                        if contains_never(&raw_arg_ty) && has_unbound_param(&expected_ty) {
                            self.error(
                                "E-TYP-AMBIGUOUS",
                                arg.span,
                                "cannot infer a generic variant argument from a diverging \
                                 expression; annotate the enum type or use a non-diverging value"
                                    .to_string(),
                            );
                        }
                        if !self.unify_pattern(pattern, &arg_ty, &mut subst) {
                            self.error(
                                "E-TYP-MISMATCH",
                                arg.span,
                                format!(
                                    "argument {} of `{}::{}`: expected `{}`, found `{}`",
                                    index + 1,
                                    info.name,
                                    variant_info.name,
                                    expected_ty,
                                    arg_ty
                                ),
                            );
                        }
                    } else {
                        self.check_expr(arg, None);
                    }
                }
            }
            crate::facts::VariantKind::Unit => {
                if !args.is_empty() {
                    self.error(
                        "E-TYP-ARITY",
                        span,
                        format!(
                            "variant `{}::{}` takes no arguments",
                            info.name, variant_info.name
                        ),
                    );
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                }
            }
            crate::facts::VariantKind::Struct => {
                self.error(
                    "E-TYP-CALL",
                    span,
                    format!(
                        "variant `{}::{}` is a struct variant; use `{{ field: value }}`",
                        info.name, variant_info.name
                    ),
                );
                for arg in args {
                    self.check_expr(arg, None);
                }
            }
        }
        let mut final_args = Vec::with_capacity(info.generics.len());
        for index in 0..info.generics.len() {
            match subst.get(&index) {
                Some(t) => final_args.push(self.resolve_vars(t)),
                None => final_args.push(self.fresh_var(span)),
            }
        }
        let result = Type::Nominal {
            def: enum_item,
            name: info.name.clone(),
            args: final_args,
        };
        if let Some(expected) = expected {
            self.unify_pattern(&result, expected, &mut subst);
        }
        self.resolve_vars(&result)
    }

    pub(super) fn check_builtin_ctor_call(
        &mut self,
        ctor: PayloadCtor,
        args: &[Expr],
        expected: Option<&Type>,
        span: Span,
    ) -> Type {
        let path = ctor.path();
        match ctor {
            PayloadCtor::Some => {
                if args.len() != 1 {
                    self.error(
                        "E-TYP-ARITY",
                        span,
                        "`Option::Some` expects exactly 1 argument".to_string(),
                    );
                }
                let inner_expected = match expected {
                    Some(Type::Option(inner)) => Some((**inner).clone()),
                    _ => None,
                };
                let inner = match args.first() {
                    Some(arg) => match &inner_expected {
                        Some(expected) => self.check_arg(arg, expected),
                        None => self.check_value_expr(arg, None),
                    },
                    None => inner_expected.unwrap_or(Type::Error),
                };
                let result = Type::option(inner);
                if let Some(expected) = expected {
                    if !self.unify_pattern(&result, expected, &mut BTreeMap::new()) {
                        self.error(
                            "E-TYP-MISMATCH",
                            span,
                            format!("`Option::Some` produces `{result}`, expected `{expected}`"),
                        );
                    }
                }
                self.resolve_vars(&result)
            }
            PayloadCtor::Ok | PayloadCtor::Err => {
                if args.len() != 1 {
                    self.error(
                        "E-TYP-ARITY",
                        span,
                        format!("`{path}` expects exactly 1 argument"),
                    );
                }
                let (ok_expected, err_expected) = match expected {
                    Some(Type::Result(ok, err)) => (Some((**ok).clone()), Some((**err).clone())),
                    _ => (None, None),
                };
                let value = match args.first() {
                    Some(arg) => {
                        let expectation = if ctor == PayloadCtor::Ok {
                            ok_expected.clone()
                        } else {
                            err_expected.clone()
                        };
                        match expectation {
                            Some(expected) => self.check_arg(arg, &expected),
                            None => self.check_value_expr(arg, None),
                        }
                    }
                    None => Type::Error,
                };
                let (ok, err) = if ctor == PayloadCtor::Ok {
                    (value, err_expected.unwrap_or_else(|| self.fresh_var(span)))
                } else {
                    (ok_expected.unwrap_or_else(|| self.fresh_var(span)), value)
                };
                let result = Type::Result(Box::new(ok), Box::new(err));
                if let Some(expected) = expected {
                    if !self.unify_pattern(&result, expected, &mut BTreeMap::new()) {
                        self.error(
                            "E-TYP-MISMATCH",
                            span,
                            format!("`{path}` produces `{result}`, expected `{expected}`"),
                        );
                    }
                }
                self.resolve_vars(&result)
            }
        }
    }

    // ------------------------------------------------------------------
    // Méthodes
    // ------------------------------------------------------------------
}
