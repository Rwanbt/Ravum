use super::*;

impl<'p, 'w, 'f> TypeChecker<'p, 'w, 'f> {
    pub(super) fn check_method_call(
        &mut self,
        expr: &Expr,
        receiver: &Expr,
        name: &str,
        name_span: &Span,
        args: &[Expr],
    ) -> Type {
        let recv_ty = self.check_expr(receiver, None);
        let recv_ty = self.resolve_vars(&recv_ty);
        let base = deref_type(&recv_ty);
        // `Vec<T>::clone` exists only for `Copy` elements (STD-10.4, F-STD-024).
        // For a mobile element the method does not exist at all, so the
        // diagnostic is a resolution failure, not a type mismatch.
        if name == "clone" {
            if let Type::Vec(inner) = &base {
                if !crate::own::is_copy_type(inner, self.facts) {
                    self.error(
                        "E-RES-UNKNOWN",
                        *name_span,
                        format!(
                            "`clone` on `Vec<{inner}>` requires a `Copy` element type; \
                             `{inner}` is mobile"
                        ),
                    );
                }
            }
        }
        match &base {
            Type::Nominal { def, .. } => {
                let entry = self.world.method(*def, name, false).cloned();
                match entry {
                    Some(entry) => {
                        let target =
                            self.check_user_method_call(&entry, &base, receiver, args, name_span);
                        let ty = target.1;
                        self.facts
                            .methods
                            .insert(expr.id, MethodTarget::User { item: entry.item });
                        ty
                    }
                    None => {
                        self.error(
                            "E-RES-UNKNOWN",
                            *name_span,
                            format!("no method `{name}` on type `{base}`"),
                        );
                        for arg in args {
                            self.check_expr(arg, None);
                        }
                        self.facts.methods.insert(expr.id, MethodTarget::Error);
                        Type::Error
                    }
                }
            }
            Type::String
            | Type::Vec(_)
            | Type::Option(_)
            | Type::Result(_, _)
            | Type::Slice(_)
            | Type::Str
            | Type::F64
            | Type::F32
            | Type::Bool
            | Type::Char
            | Type::I8
            | Type::I16
            | Type::I32
            | Type::I64
            | Type::I128
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64
            | Type::U128
            | Type::Isize
            | Type::Usize => {
                let kind = method_self_kind(&base);
                match kind.and_then(|k| builtins::builtin_method(k, name)) {
                    Some(sig) => {
                        if builtins::builtin_method_takes_exclusive_receiver(kind.unwrap(), name)
                            && !self.is_mutable_place(receiver)
                        {
                            self.error(
                                "E-OWN-IMMUTABLE",
                                *name_span,
                                format!(
                                    "`{name}` takes `&mut self`; the receiver must be a mutable \
                                     place (`let mut …` or a `&mut` borrow)"
                                ),
                            );
                        }
                        let env = method_env_for(&base);
                        let params = builtins::method_param_types(sig, &env);
                        if args.len() != params.len() {
                            self.error(
                                "E-TYP-ARITY",
                                *name_span,
                                format!(
                                    "method `{name}` expects {} argument(s), found {}",
                                    params.len(),
                                    args.len()
                                ),
                            );
                        }
                        for (index, arg) in args.iter().enumerate() {
                            match params.get(index) {
                                Some(param) => {
                                    let arg_ty = self.check_arg(arg, param);
                                    if !self.unify_pattern(param, &arg_ty, &mut BTreeMap::new()) {
                                        self.error(
                                            "E-TYP-MISMATCH",
                                            arg.span,
                                            format!(
                                                "argument {} of `{name}`: expected `{param}`, found `{arg_ty}`",
                                                index + 1
                                            ),
                                        );
                                    }
                                }
                                None => {
                                    self.check_expr(arg, None);
                                }
                            }
                        }
                        let ret = self.resolve_vars(&builtins::method_ret_type(sig, &env));
                        self.facts.methods.insert(
                            expr.id,
                            MethodTarget::Builtin {
                                path: sig.path.to_string(),
                                ret: ret.clone(),
                            },
                        );
                        ret
                    }
                    None => {
                        self.error(
                            "E-RES-UNKNOWN",
                            *name_span,
                            format!("no builtin method `{name}` on type `{base}`"),
                        );
                        for arg in args {
                            self.check_expr(arg, None);
                        }
                        self.facts.methods.insert(expr.id, MethodTarget::Error);
                        Type::Error
                    }
                }
            }
            Type::Param { name: param, .. } => {
                self.error(
                    "E-RES-UNKNOWN",
                    *name_span,
                    format!("no method `{name}` can be resolved on generic parameter `{param}`"),
                );
                for arg in args {
                    self.check_expr(arg, None);
                }
                self.facts.methods.insert(expr.id, MethodTarget::Error);
                Type::Error
            }
            _ => {
                self.error(
                    "E-RES-UNKNOWN",
                    *name_span,
                    format!("no method `{name}` on type `{base}`"),
                );
                for arg in args {
                    self.check_expr(arg, None);
                }
                self.facts.methods.insert(expr.id, MethodTarget::Error);
                Type::Error
            }
        }
    }

    pub(super) fn check_user_method_call(
        &mut self,
        entry: &resolve::MethodEntry,
        recv: &Type,
        receiver: &Expr,
        args: &[Expr],
        name_span: &Span,
    ) -> (NodeId, Type) {
        if !self.method_visible(entry) {
            self.error(
                "E-RES-PRIVATE",
                *name_span,
                format!("method `{}` is private", entry.name),
            );
        }
        let Some(sig) = self.facts.fn_sigs.get(&entry.item).cloned() else {
            for arg in args {
                self.check_expr(arg, None);
            }
            return (entry.item, Type::Error);
        };
        // TYP-3.2/OWN-7.4 (MUST): a `&mut self` method requires a mutable
        // receiver place; a temporary receiver is `E-OWN-IMMUTABLE`. The
        // builtin path already enforces this, the user-method path did not:
        // `P { x: 0 }.set()` and `p.set()` on an immutable binding were
        // accepted and rustc rejected the generated code (E0596/E0594).
        if matches!(sig.receiver, Some(Receiver::Exclusive)) && !self.receiver_is_mutable(receiver)
        {
            self.error(
                "E-OWN-IMMUTABLE",
                *name_span,
                format!(
                    "`{}` takes `&mut self`; the receiver must be a mutable place \
                     (`let mut …` or a `&mut` borrow)",
                    entry.name
                ),
            );
        }
        let mut subst: BTreeMap<usize, Type> = BTreeMap::new();
        if let Type::Nominal { args: actual, .. } = recv {
            self.bind_impl_generics(entry, actual, &mut subst);
        }
        let params: Vec<&crate::facts::ParamSig> =
            sig.params.iter().filter(|p| !p.is_receiver).collect();
        if args.len() != params.len() {
            self.error(
                "E-TYP-ARITY",
                *name_span,
                format!(
                    "method `{}` expects {} argument(s), found {}",
                    entry.name,
                    params.len(),
                    args.len()
                ),
            );
        }
        for (index, arg) in args.iter().enumerate() {
            match params.get(index) {
                Some(param) => {
                    let param_ty = substitute(&param.ty, &subst);
                    let arg_ty = self.check_arg(arg, &param_ty);
                    if !self.unify_pattern(&param.ty, &arg_ty, &mut subst) {
                        self.error(
                            "E-TYP-MISMATCH",
                            arg.span,
                            format!(
                                "argument {} of `{}`: expected `{}`, found `{}`",
                                index + 1,
                                entry.name,
                                param_ty,
                                arg_ty
                            ),
                        );
                    }
                }
                None => {
                    self.check_expr(arg, None);
                }
            }
        }
        let ret = self.resolve_vars(&substitute(&sig.ret, &subst));
        (entry.item, ret)
    }

    // ------------------------------------------------------------------
    // Accès champs / index
    // ------------------------------------------------------------------

    pub(super) fn check_field(&mut self, base: &Expr, name: &str, span: Span) -> Type {
        let base_ty = self.check_expr(base, None);
        let ty = deref_type(&base_ty);
        match ty {
            Type::Nominal { def, ref args, .. } => {
                let Some(field) = self
                    .facts
                    .structs
                    .get(&def)
                    .and_then(|info| info.fields.iter().find(|f| f.name == name))
                    .cloned()
                else {
                    self.error(
                        "E-TYP-FIELD",
                        span,
                        format!("no field `{name}` on type `{ty}`"),
                    );
                    return Type::Error;
                };
                self.check_field_visibility(&def, &field, span);
                let subst = nominal_subst(self.world, &def, args);
                substitute(&field.ty, &subst)
            }
            Type::Tuple(_) => {
                self.error(
                    "E-TYP-FIELD",
                    span,
                    format!("no field `{name}` on tuple; use `.0`, `.1`, ..."),
                );
                Type::Error
            }
            _ => {
                // A method name used without a call is not a field and not a
                // value: `s.parse_i64` is a resolution failure (STD-9.10,
                // F-STD-021), which is a different diagnostic from "unknown
                // field" and points the user at the fix.
                // A user type never reaches this arm (the `Nominal` arm above
                // answers "no field"), so only builtin methods are looked up.
                let is_method = method_self_kind(&ty)
                    .and_then(|kind| builtins::builtin_method(kind, name))
                    .is_some();
                if is_method {
                    self.error(
                        "E-RES-UNKNOWN",
                        span,
                        format!("`{name}` is a method of `{ty}`, not a value; call it with `(…)`"),
                    );
                } else {
                    self.error(
                        "E-TYP-FIELD",
                        span,
                        format!("no field `{name}` on type `{ty}`"),
                    );
                }
                Type::Error
            }
        }
    }

    pub(super) fn check_field_visibility(
        &mut self,
        def: &NodeId,
        field: &crate::facts::FieldInfo,
        span: Span,
    ) {
        if field.public {
            return;
        }
        let def_module = self.world.item_module.get(def).copied().unwrap_or(0);
        let mut visible = false;
        let mut current = Some(self.module);
        while let Some(m) = current {
            if m == def_module {
                visible = true;
                break;
            }
            current = self.world.modules[m].parent;
        }
        if !visible {
            self.error(
                "E-RES-PRIVATE",
                span,
                format!("field `{}` is private", field.name),
            );
        }
    }

    pub(super) fn check_tuple_index(&mut self, base: &Expr, index: u32, span: Span) -> Type {
        let base_ty = self.check_expr(base, None);
        match deref_type(&base_ty) {
            Type::Tuple(items) => match items.get(index as usize) {
                Some(ty) => ty.clone(),
                None => {
                    self.error(
                        "E-TYP-FIELD",
                        span,
                        format!(
                            "tuple index `{index}` is out of bounds (arity {})",
                            items.len()
                        ),
                    );
                    Type::Error
                }
            },
            other => {
                self.error(
                    "E-TYP-FIELD",
                    span,
                    format!("`.{index}` requires a tuple, found `{other}`"),
                );
                Type::Error
            }
        }
    }

    pub(super) fn check_index(&mut self, expr: &Expr, base: &Expr, index: &Expr) -> Type {
        let base_ty = self.check_expr(base, None);
        let index_ty = self.check_expr(index, Some(&Type::Usize));
        if !matches!(index_ty, Type::Usize | Type::Error) {
            self.error_note(
                "E-TYP-INDEX",
                index.span,
                format!("index must be `usize`, found `{index_ty}`"),
                "convert explicitly with `as usize`",
                None,
            );
        }
        match deref_type(&base_ty) {
            Type::Array { elem, .. } => (*elem).clone(),
            Type::Vec(elem) => (*elem).clone(),
            Type::Slice(elem) => (*elem).clone(),
            Type::String => {
                self.error(
                    "E-TYP-INDEX",
                    expr.span,
                    "`String` is not indexable in Core 1.0".to_string(),
                );
                Type::Error
            }
            other => {
                self.error(
                    "E-TYP-INDEX",
                    expr.span,
                    format!("type `{other}` is not indexable"),
                );
                Type::Error
            }
        }
    }

    // ------------------------------------------------------------------
    // Opérateurs
    // ------------------------------------------------------------------

    pub(super) fn check_unary(
        &mut self,
        expr: &Expr,
        op: UnOp,
        operand: &Expr,
        expected: Option<&Type>,
    ) -> Type {
        match op {
            UnOp::Neg => {
                if let ExprKind::Int { value, suffix } = &operand.kind {
                    let ty = match suffix {
                        Some(suffix) => int_type(*suffix),
                        None => match expected {
                            Some(t) if t.is_int() => t.clone(),
                            _ => Type::I32,
                        },
                    };
                    if !ty.is_signed_int() {
                        self.error(
                            "E-TYP-MISMATCH",
                            expr.span,
                            format!("cannot negate a value of type `{ty}`"),
                        );
                        self.facts.expr_types.insert(operand.id, ty.clone());
                        return ty;
                    }
                    let (min_magnitude, _, _) = int_bounds(&ty);
                    if *value > min_magnitude {
                        self.error(
                            "E-TYP-LITERAL-RANGE",
                            operand.span,
                            format!("integer literal `-{value}` does not fit in `{ty}`"),
                        );
                    }
                    self.facts.expr_types.insert(operand.id, ty.clone());
                    return ty;
                }
                let inner_ty = self.check_expr(operand, expected.filter(|t| t.is_numeric()));
                match &inner_ty {
                    t if t.is_signed_int() || t.is_float() => inner_ty.clone(),
                    Type::Never | Type::Error => inner_ty.clone(),
                    t => {
                        self.error(
                            "E-TYP-MISMATCH",
                            expr.span,
                            format!("cannot negate a value of type `{t}`"),
                        );
                        t.clone()
                    }
                }
            }
            UnOp::Not => {
                let inner_ty = self.check_expr(
                    operand,
                    expected.filter(|t| **t == Type::Bool || t.is_int()),
                );
                match &inner_ty {
                    Type::Bool => Type::Bool,
                    t if t.is_int() => t.clone(),
                    Type::Never | Type::Error => inner_ty.clone(),
                    t => {
                        self.error(
                            "E-TYP-MISMATCH",
                            expr.span,
                            format!("`!` requires `bool` or an integer, found `{t}`"),
                        );
                        Type::Error
                    }
                }
            }
            UnOp::Ref | UnOp::RefMut => {
                let inner_ty = self.check_expr(operand, None);
                // `&mut` of a place needs a mutable place (rustc E0596). A
                // value that is not a place is a temporary and may be borrowed.
                if op == UnOp::RefMut
                    && !matches!(inner_ty, Type::Error)
                    && self.names_a_place(operand)
                    && self.place_mutability(operand) == Some(false)
                {
                    self.error(
                        "E-OWN-IMMUTABLE",
                        operand.span,
                        "cannot borrow an immutable place as `&mut`".to_string(),
                    );
                }
                Type::Ref {
                    mutable: op == UnOp::RefMut,
                    inner: Box::new(inner_ty),
                }
            }
            UnOp::Deref => {
                let inner_ty = self.check_expr(operand, None);
                match inner_ty {
                    Type::Ref { inner, .. } => (*inner).clone(),
                    Type::Never | Type::Error => inner_ty,
                    other => {
                        self.error(
                            "E-TYP-MISMATCH",
                            expr.span,
                            format!("cannot dereference a value of type `{other}`"),
                        );
                        Type::Error
                    }
                }
            }
        }
    }

    pub(super) fn check_binary(
        &mut self,
        expr: &Expr,
        op: ast::BinOp,
        lhs: &Expr,
        rhs: &Expr,
        expected: Option<&Type>,
    ) -> Type {
        use ast::BinOp as B;
        let lhs_lit = is_untyped_num_literal(lhs);
        let rhs_lit = is_untyped_num_literal(rhs);
        let is_shift = matches!(op, B::Shl | B::Shr);

        let (l_ty, r_ty) = if is_shift {
            let r = self.check_expr(rhs, None);
            let l = if lhs_lit {
                self.check_expr(lhs, Some(&r))
            } else {
                self.check_expr(lhs, expected)
            };
            (l, r)
        } else if lhs_lit && !rhs_lit {
            let r = self.check_expr(rhs, expected.filter(|t| t.is_numeric()));
            let l = self.check_expr(lhs, Some(&r));
            (l, r)
        } else {
            let l = self.check_expr(lhs, expected.filter(|t| t.is_numeric()));
            let r = self.check_expr(rhs, Some(&l));
            (l, r)
        };

        match op {
            B::Add | B::Sub | B::Mul | B::Div | B::Rem => {
                if (l_ty.is_int() && r_ty.is_int()) || (l_ty.is_float() && r_ty.is_float()) {
                    if l_ty == r_ty {
                        l_ty
                    } else {
                        self.error(
                            "E-TYP-MISMATCH",
                            expr.span,
                            format!(
                                "operands of `{}` must have the same type: `{l_ty}` vs `{r_ty}`",
                                op.as_str()
                            ),
                        );
                        Type::Error
                    }
                } else {
                    self.error(
                        "E-TYP-MISMATCH",
                        expr.span,
                        format!(
                            "operator `{}` is not defined for `{l_ty}` and `{r_ty}`",
                            op.as_str()
                        ),
                    );
                    Type::Error
                }
            }
            B::BitAnd | B::BitOr | B::BitXor => {
                let ok = (l_ty.is_int() && r_ty.is_int() && l_ty == r_ty)
                    || (l_ty == Type::Bool && r_ty == Type::Bool);
                if ok {
                    l_ty
                } else {
                    self.error(
                        "E-TYP-MISMATCH",
                        expr.span,
                        format!(
                            "operator `{}` requires two integers of the same type or two `bool`",
                            op.as_str()
                        ),
                    );
                    Type::Error
                }
            }
            B::Shl | B::Shr => {
                if l_ty.is_int() && r_ty.is_int() {
                    if is_negative_int_literal(rhs) {
                        self.error(
                            "E-TYP-SHIFT",
                            rhs.span,
                            "shift amount must not be negative".to_string(),
                        );
                    }
                    l_ty
                } else {
                    self.error(
                        "E-TYP-MISMATCH",
                        expr.span,
                        format!(
                            "operator `{}` requires integer operands, found `{l_ty}` and `{r_ty}`",
                            op.as_str()
                        ),
                    );
                    Type::Error
                }
            }
            B::Eq | B::Ne => {
                // TYP-7.3 : `==`/`!=` sont définis sur tous les entiers,
                // `f32`/`f64`, `bool`, `char`, et sur le texte (`String`, `&str`
                // et tout mélange des deux). Les agrégats restent non
                // comparables (`E-TYP-EQ`).
                let same_scalar = l_ty == r_ty
                    && (l_ty.is_int()
                        || l_ty.is_float()
                        || l_ty == Type::Bool
                        || l_ty == Type::Char);
                if (is_text_like(&l_ty) && is_text_like(&r_ty)) || same_scalar {
                    Type::Bool
                } else if l_ty.is_numeric() && r_ty.is_numeric() {
                    // EXPR-4.8 / TYP-13.2 : deux types numériques différents ne
                    // se mélangent jamais implicitement (`f32` vs `f64`,
                    // `i32` vs `i64`, ...) : c'est un désaccord d'opérandes
                    // (`E-TYP-MISMATCH`), pas un type non comparable.
                    self.error(
                        "E-TYP-MISMATCH",
                        expr.span,
                        format!(
                            "operands of `{}` must have the same type: `{l_ty}` vs `{r_ty}`",
                            op.as_str()
                        ),
                    );
                    Type::Bool
                } else {
                    self.error(
                        "E-TYP-EQ",
                        expr.span,
                        format!(
                            "`{}` is not defined for `{l_ty}` and `{r_ty}` in Core 1.0",
                            op.as_str()
                        ),
                    );
                    Type::Bool
                }
            }
            B::Lt | B::Le | B::Gt | B::Ge => {
                // TYP-7.4 : `< <= > >=` ne sont définis que sur les entiers,
                // `f32`/`f64` et `char` (jamais sur le texte).
                let same_scalar =
                    l_ty == r_ty && (l_ty.is_int() || l_ty.is_float() || l_ty == Type::Char);
                if same_scalar {
                    Type::Bool
                } else if l_ty.is_numeric() && r_ty.is_numeric() {
                    // EXPR-4.8 / TYP-13.2 : même règle que pour `==`/`!=`.
                    self.error(
                        "E-TYP-MISMATCH",
                        expr.span,
                        format!(
                            "operands of `{}` must have the same type: `{l_ty}` vs `{r_ty}`",
                            op.as_str()
                        ),
                    );
                    Type::Bool
                } else {
                    self.error(
                        "E-TYP-CMP",
                        expr.span,
                        format!(
                            "`{}` is not defined for `{l_ty}` and `{r_ty}` in Core 1.0",
                            op.as_str()
                        ),
                    );
                    Type::Bool
                }
            }
            B::And | B::Or => {
                if l_ty == Type::Bool && r_ty == Type::Bool {
                    Type::Bool
                } else {
                    self.error(
                        "E-TYP-MISMATCH",
                        expr.span,
                        format!(
                            "`{}` requires `bool` operands, found `{l_ty}` and `{r_ty}`",
                            op.as_str()
                        ),
                    );
                    Type::Bool
                }
            }
        }
    }

    pub(super) fn check_assign(&mut self, target: &Expr, value: &Expr) -> Type {
        let target_ty = self.check_expr(target, None);
        let value_ty = self.check_value_expr(value, Some(&target_ty));
        if !Self::compatible(&value_ty, &target_ty) {
            self.error(
                "E-TYP-MISMATCH",
                value.span,
                format!("cannot assign `{value_ty}` to a place of type `{target_ty}`"),
            );
        }
        // A binding declared without an initialiser (`let x: T;`) receives its
        // value by one plain assignment of the whole binding; the ownership
        // phase checks that it happens at most once on every path. Nothing
        // else about it is mutable.
        if !matches!(target_ty, Type::Error)
            && !self.place_is_mutable(target)
            && !self.is_deferred_binding(target)
        {
            self.error(
                "E-OWN-IMMUTABLE",
                target.span,
                "cannot assign to an immutable place".to_string(),
            );
        }
        Type::Unit
    }

    /// The expression is a place rooted at a local binding or at `self` (not a
    /// constant, a function or a temporary).
    fn names_a_place(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Path(path) => {
                path.root == PathRoot::Plain
                    && path.segments.len() == 1
                    && self.lookup_local(&path.segments[0].name.name).is_some()
            }
            ExprKind::SelfValue => true,
            ExprKind::Field { base, .. }
            | ExprKind::TupleIndex { base, .. }
            | ExprKind::Index { base, .. } => self.names_a_place(base),
            ExprKind::Unary {
                op: UnOp::Deref, ..
            } => true,
            _ => false,
        }
    }

    fn is_deferred_binding(&self, expr: &Expr) -> bool {
        let ExprKind::Path(path) = &expr.kind else {
            return false;
        };
        path.root == PathRoot::Plain
            && path.segments.len() == 1
            && self
                .lookup_local(&path.segments[0].name.name)
                .is_some_and(|local| local.deferred)
    }

    /// `Some(false)` = place immuable ; `Some(true)` = place mutable ; `None` =
    /// l'expression n'est pas une place (déjà rejetée par le parser).
    pub(super) fn place_is_mutable(&self, expr: &Expr) -> bool {
        self.place_mutability(expr).unwrap_or(true)
    }

    pub(super) fn place_mutability(&self, expr: &Expr) -> Option<bool> {
        match &expr.kind {
            ExprKind::Path(path) => {
                if path.root == PathRoot::Plain && path.segments.len() == 1 {
                    let name = &path.segments[0].name.name;
                    if let Some(local) = self.lookup_local(name) {
                        return Some(local.mutable);
                    }
                }
                Some(false)
            }
            ExprKind::SelfValue => self.lookup_local("self").map(|l| l.mutable),
            ExprKind::Field { base, .. }
            | ExprKind::TupleIndex { base, .. }
            | ExprKind::Index { base, .. } => {
                if self.place_mutability(base) == Some(true) {
                    return Some(true);
                }
                Some(self.base_is_mut_ref(base))
            }
            ExprKind::Unary {
                op: UnOp::Deref,
                expr: inner,
            } => {
                let ty = self
                    .facts
                    .expr_types
                    .get(&inner.id)
                    .cloned()
                    .or_else(|| self.facts.type_nodes.get(&inner.id).cloned());
                match ty {
                    Some(Type::Ref { mutable, .. }) => Some(mutable),
                    Some(Type::Never) | Some(Type::Error) => None,
                    _ => Some(false),
                }
            }
            _ => None,
        }
    }

    pub(super) fn base_is_mut_ref(&self, base: &Expr) -> bool {
        matches!(
            self.facts.expr_types.get(&base.id),
            Some(Type::Ref { mutable: true, .. })
        )
    }

    pub(super) fn check_assign_op(
        &mut self,
        expr: &Expr,
        op: ast::BinOp,
        target: &Expr,
        value: &Expr,
    ) -> Type {
        // G-6.8 / EXPR-9.4 (MUST) : toute la famille des affectations
        // composées est **acceptée** (`+= -= *= /= %= &= |= ^= <<= >>=`), avec
        // la sémantique `place = place op e`. `place` n'est évaluée qu'une
        // fois : c'est l'affectation composée du Rust émis (`place op= e`) qui
        // le garantit ; le vérificateur de types ne contrôle donc que les
        // types des opérandes et la mutabilité de la place.
        use ast::BinOp as B;
        let sym = format!("{}=", op.as_str());
        let target_ty = self.check_expr(target, None);
        let value_ty = self.check_value_expr(value, Some(&target_ty));
        match op {
            B::Add | B::Sub | B::Mul | B::Div | B::Rem => {
                // TYP-13.2 : aucune conversion implicite, pas même entre deux
                // entiers (`i32` vs `i64`) ni entre `f32` et `f64` (EXPR-4.8).
                let same_numeric = (target_ty.is_int() && value_ty.is_int())
                    || (target_ty.is_float() && value_ty.is_float());
                if !same_numeric {
                    self.error(
                        "E-TYP-MISMATCH",
                        expr.span,
                        format!(
                            "operator `{sym}` is not defined for `{target_ty}` and `{value_ty}`"
                        ),
                    );
                } else if target_ty != value_ty {
                    self.error(
                        "E-TYP-MISMATCH",
                        expr.span,
                        format!(
                            "operands of `{sym}` must have the same type: `{target_ty}` vs `{value_ty}`"
                        ),
                    );
                }
            }
            B::BitAnd | B::BitOr | B::BitXor => {
                let ok = (target_ty.is_int() && value_ty.is_int() && target_ty == value_ty)
                    || (target_ty == Type::Bool && value_ty == Type::Bool);
                if !ok {
                    self.error(
                        "E-TYP-MISMATCH",
                        expr.span,
                        format!(
                            "operator `{sym}` requires two integers of the same type or two `bool`"
                        ),
                    );
                }
            }
            B::Shl | B::Shr => {
                // EXPR-3.4 : comme pour `<<`/`>>`, les deux opérandes doivent
                // être entiers mais leurs types peuvent différer (le résultat a
                // le type de la gauche) ; un décalage littéral négatif est
                // refusé statiquement.
                if target_ty.is_int() && value_ty.is_int() {
                    if is_negative_int_literal(value) {
                        self.error(
                            "E-TYP-SHIFT",
                            value.span,
                            "shift amount must not be negative".to_string(),
                        );
                    }
                } else {
                    self.error(
                        "E-TYP-MISMATCH",
                        expr.span,
                        format!(
                            "operator `{sym}` requires integer operands, found `{target_ty}` and `{value_ty}`"
                        ),
                    );
                }
            }
            // Le parseur ne construit un `AssignOp` qu'avec l'un des dix
            // opérateurs ci-dessus (G-6.8) ; ce bras n'est donc pas atteignable,
            // mais un refus typé vaut mieux qu'un silence.
            B::And | B::Or | B::Eq | B::Ne | B::Lt | B::Le | B::Gt | B::Ge => {
                self.error(
                    "E-TYP-MISMATCH",
                    expr.span,
                    format!("`{sym}` is not a compound assignment operator"),
                );
            }
        }
        if !matches!(target_ty, Type::Error) && !self.place_is_mutable(target) {
            self.error(
                "E-OWN-IMMUTABLE",
                target.span,
                "cannot assign to an immutable place".to_string(),
            );
        }
        Type::Unit
    }

    pub(super) fn check_cast(&mut self, value: &Expr, ty: &AstType) -> Type {
        let target = self.resolve_type(ty);
        // TYP-1.6 / G-5.11: `!` is not constructible, so it cannot be a cast
        // target (`x as !` would generate invalid Rust). Checked before the
        // `Never` source shortcut, which otherwise returns the target as-is.
        if crate::types::contains_never(&target) {
            self.check_expr(value, None);
            self.error(
                "E-TYP-CAST",
                ty.span,
                "the never type (`!`) is not a valid cast target in Core 1.0".to_string(),
            );
            return Type::Error;
        }
        let source = self.check_expr(value, None);
        if matches!(source, Type::Never) || matches!(target, Type::Error) {
            return target;
        }
        if !cast_allowed(&source, &target) {
            self.error(
                "E-TYP-CAST",
                value.span,
                format!("cast from `{source}` to `{target}` is not allowed in Core 1.0"),
            );
            return Type::Error;
        }
        target
    }

    // ------------------------------------------------------------------
    // Contrôle
    // ------------------------------------------------------------------
}
