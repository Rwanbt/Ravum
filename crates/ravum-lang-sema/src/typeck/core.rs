use super::*;

impl<'p, 'w, 'f> TypeChecker<'p, 'w, 'f> {
    pub(super) fn error(&mut self, code: &'static str, span: Span, message: impl Into<String>) {
        self.diags.error(code, span, message);
    }

    pub(super) fn error_note(
        &mut self,
        code: &'static str,
        span: Span,
        message: impl Into<String>,
        note: impl Into<String>,
        note_span: Option<Span>,
    ) {
        self.diags
            .push(Diagnostic::error(code, span, message).with_note(note, note_span));
    }

    pub(super) fn warning(&mut self, code: &'static str, span: Span, message: impl Into<String>) {
        self.diags.warning(code, span, message);
    }

    // ------------------------------------------------------------------
    // Portée et locales
    // ------------------------------------------------------------------

    pub(super) fn push_scope(&mut self) {
        self.scopes.push(BTreeMap::new());
    }

    pub(super) fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    pub(super) fn bind_local(
        &mut self,
        name: &str,
        ty: Type,
        mutable: bool,
        deferred: bool,
        decl: Span,
    ) {
        // Every binding site (parameter, `let`, pattern) runs inside the
        // scope its function, block or arm pushed.
        let scope = self
            .scopes
            .last_mut()
            .expect("invariant: a local is bound inside an open scope");
        scope.insert(
            name.to_string(),
            Local {
                ty,
                mutable,
                deferred,
                decl,
            },
        );
    }

    pub(super) fn lookup_local(&self, name: &str) -> Option<Local> {
        for scope in self.scopes.iter().rev() {
            if let Some(local) = scope.get(name) {
                return Some(local.clone());
            }
        }
        None
    }

    // ------------------------------------------------------------------
    // Types
    // ------------------------------------------------------------------

    pub(super) fn resolve_type(&mut self, ty: &AstType) -> Type {
        resolve::resolve_type_syntax(
            self.world,
            &mut self.facts.type_nodes,
            &mut self.facts.expr_types,
            self.diags,
            self.module,
            &self.generics,
            self.self_ty.as_ref(),
            ty,
        )
    }

    pub(super) fn compatible(a: &Type, b: &Type) -> bool {
        a == b
            || matches!(a, Type::Never)
            || matches!(b, Type::Never)
            || matches!(a, Type::Error)
            || matches!(b, Type::Error)
    }

    pub(super) fn unify(
        &mut self,
        found: &Type,
        expected: &Type,
        span: Span,
        context: &str,
    ) -> bool {
        let found = self.resolve_vars(found);
        let expected = self.resolve_vars(expected);
        if Self::compatible(&found, &expected) {
            return true;
        }
        if (self.has_infer_var(&found) || self.has_infer_var(&expected))
            && self.unify_pattern(&expected, &found, &mut BTreeMap::new())
        {
            return true;
        }
        self.error(
            "E-TYP-MISMATCH",
            span,
            format!("{context}: expected `{expected}`, found `{found}`"),
        );
        false
    }

    /// Two branch types (`if`/`else`, `match` arms) agree when they are
    /// compatible once inference variables are unified, in either order: a
    /// first branch `Vec::new()` (`Vec<$0>`) takes the type of a second one.
    pub(super) fn branch_types_agree(&mut self, first: &Type, second: &Type) -> bool {
        let first = self.resolve_vars(first);
        let second = self.resolve_vars(second);
        if Self::compatible(&first, &second) {
            return true;
        }
        if !(self.has_infer_var(&first) || self.has_infer_var(&second))
            || !self.unify_pattern(&first, &second, &mut BTreeMap::new())
        {
            return false;
        }
        // `unify_pattern` reads a generic parameter of the pattern as a
        // wildcard; a branch pair agrees only if the types, once the variables
        // are bound, are really the same (`Vec<T>` never agrees with `Vec<i32>`).
        let first = self.resolve_vars(&first);
        let second = self.resolve_vars(&second);
        Self::compatible(&first, &second)
    }

    pub(super) fn has_infer_var(&self, ty: &Type) -> bool {
        has_infer_var_rec(ty)
    }

    pub(super) fn check_value_expr(&mut self, expr: &Expr, expected: Option<&Type>) -> Type {
        self.value_position += 1;
        let ty = self.check_expr(expr, expected);
        self.value_position -= 1;
        ty
    }

    pub(super) fn record_expr_type(&mut self, id: NodeId, ty: &Type) {
        self.facts.expr_types.insert(id, ty.clone());
    }

    // ------------------------------------------------------------------
    // Inférence (variables de type)
    // ------------------------------------------------------------------

    pub(super) fn fresh_var(&mut self, span: Span) -> Type {
        let index = self.vars.len();
        self.vars.push(None);
        self.var_spans.push(span);
        Type::Param {
            name: format!("${index}"),
            index: VAR_BASE + index,
        }
    }

    pub(super) fn resolve_vars(&self, ty: &Type) -> Type {
        resolve_vars_keep(&self.vars, ty)
    }

    pub(super) fn bind_var(&mut self, index: usize, ty: &Type) -> bool {
        let ty = self.resolve_vars(ty);
        if infer_var_index(&ty) == Some(index) {
            return true;
        }
        if self.var_occurs(index, &ty) {
            return false;
        }
        // TYP-1.6: `!` is not an inhabitable type argument. Binding an
        // inference variable to `!` (or to e.g. `Vec<!>`) would generate
        // uninhabitable Rust (`Vec<!>` needs a nightly feature). `!` coerces to
        // the expected type; it must never *determine* it. Leaving the variable
        // unbound lets a real constraint bind it, or yields E-TYP-AMBIGUOUS
        // instead of a false accept.
        if contains_never(&ty) {
            return true;
        }
        // An inference variable index comes only from `fresh_var`.
        let slot = self
            .vars
            .get_mut(index)
            .expect("invariant: an inference variable is created before it is bound");
        *slot = Some(ty);
        true
    }

    pub(super) fn var_occurs(&self, target: usize, ty: &Type) -> bool {
        if let Some(index) = infer_var_index(ty) {
            if index == target {
                return true;
            }
            return match self.vars.get(index).and_then(|v| v.clone()) {
                Some(bound) => self.var_occurs(target, &bound),
                None => false,
            };
        }
        match ty {
            Type::Ref { inner, .. } => self.var_occurs(target, inner),
            Type::Array { elem, .. } => self.var_occurs(target, elem),
            Type::Slice(inner) | Type::Vec(inner) | Type::Option(inner) => {
                self.var_occurs(target, inner)
            }
            Type::Tuple(items) => items.iter().any(|t| self.var_occurs(target, t)),
            Type::Result(ok, err) => self.var_occurs(target, ok) || self.var_occurs(target, err),
            Type::Nominal { args, .. } => args.iter().any(|t| self.var_occurs(target, t)),
            _ => false,
        }
    }

    /// Remplace les paramètres génériques de déclaration (`index < VAR_BASE`)
    /// par des variables d'inférence fraîches, de façon consistante.
    pub(super) fn instantiate(&mut self, ty: &Type, span: Span) -> Type {
        let mut mapping: BTreeMap<usize, Type> = BTreeMap::new();
        self.instantiate_with(ty, span, &mut mapping)
    }

    pub(super) fn instantiate_with(
        &mut self,
        ty: &Type,
        span: Span,
        mapping: &mut BTreeMap<usize, Type>,
    ) -> Type {
        match ty {
            Type::Param { index, .. } if infer_var_index(ty).is_none() => mapping
                .entry(*index)
                .or_insert_with(|| self.fresh_var(span))
                .clone(),
            Type::Ref { mutable, inner } => Type::Ref {
                mutable: *mutable,
                inner: Box::new(self.instantiate_with(inner, span, mapping)),
            },
            Type::Array { elem, len } => Type::Array {
                elem: Box::new(self.instantiate_with(elem, span, mapping)),
                len: *len,
            },
            Type::Slice(inner) => {
                Type::Slice(Box::new(self.instantiate_with(inner, span, mapping)))
            }
            Type::Tuple(items) => Type::Tuple(
                items
                    .iter()
                    .map(|t| self.instantiate_with(t, span, mapping))
                    .collect(),
            ),
            Type::Vec(inner) => Type::Vec(Box::new(self.instantiate_with(inner, span, mapping))),
            Type::Option(inner) => {
                Type::Option(Box::new(self.instantiate_with(inner, span, mapping)))
            }
            Type::Result(ok, err) => Type::Result(
                Box::new(self.instantiate_with(ok, span, mapping)),
                Box::new(self.instantiate_with(err, span, mapping)),
            ),
            Type::Nominal { def, name, args } => Type::Nominal {
                def: *def,
                name: name.clone(),
                args: args
                    .iter()
                    .map(|t| self.instantiate_with(t, span, mapping))
                    .collect(),
            },
            other => other.clone(),
        }
    }

    /// Unification avec variables d'inférence et paramètres de déclaration.
    pub(super) fn unify_pattern(
        &mut self,
        pattern: &Type,
        actual: &Type,
        subst: &mut BTreeMap<usize, Type>,
    ) -> bool {
        let p = self.resolve_vars(pattern);
        let a = self.resolve_vars(actual);
        if matches!(a, Type::Never | Type::Error) || matches!(p, Type::Error) {
            return true;
        }
        if let Some(index) = infer_var_index(&p) {
            return self.bind_var(index, &a);
        }
        if let Some(index) = infer_var_index(&a) {
            let concrete = substitute(&p, subst);
            return self.bind_var(index, &concrete);
        }
        match (&p, &a) {
            (Type::Param { index, .. }, actual) => match subst.get(index) {
                Some(existing) => existing == actual,
                None => {
                    subst.insert(*index, actual.clone());
                    true
                }
            },
            (
                Type::Ref {
                    mutable: pm,
                    inner: pi,
                },
                Type::Ref {
                    mutable: am,
                    inner: ai,
                },
            ) => pm == am && self.unify_pattern(pi, ai, subst),
            (Type::Array { elem: pe, len: pl }, Type::Array { elem: ae, len: al }) => {
                pl == al && self.unify_pattern(pe, ae, subst)
            }
            (Type::Slice(pi), Type::Slice(ai)) => self.unify_pattern(pi, ai, subst),
            (Type::Tuple(ps), Type::Tuple(as_)) => {
                ps.len() == as_.len()
                    && ps
                        .iter()
                        .zip(as_.iter())
                        .all(|(p, a)| self.unify_pattern(p, a, subst))
            }
            (Type::Vec(pi), Type::Vec(ai)) => self.unify_pattern(pi, ai, subst),
            (Type::Option(pi), Type::Option(ai)) => self.unify_pattern(pi, ai, subst),
            (Type::Result(po, pe), Type::Result(ao, ae)) => {
                self.unify_pattern(po, ao, subst) && self.unify_pattern(pe, ae, subst)
            }
            (
                Type::Nominal {
                    def: pd, args: pa, ..
                },
                Type::Nominal {
                    def: ad, args: aa, ..
                },
            ) => {
                pd == ad
                    && pa.len() == aa.len()
                    && pa
                        .iter()
                        .zip(aa.iter())
                        .all(|(p, a)| self.unify_pattern(p, a, subst))
            }
            (p, a) => p == a,
        }
    }

    /// Résout toutes les variables d'inférence dans les faits et signale les
    /// variables encore libres (`E-TYP-AMBIGUOUS`).
    pub(super) fn finalize_inference(&mut self) {
        let mut unresolved: BTreeSet<usize> = BTreeSet::new();
        {
            let vars = &self.vars;
            for ty in self.facts.expr_types.values_mut() {
                *ty = resolve_vars_final(vars, ty, &mut unresolved);
            }
            for ty in self.facts.pat_types.values_mut() {
                *ty = resolve_vars_final(vars, ty, &mut unresolved);
            }
            for ty in self.facts.type_nodes.values_mut() {
                *ty = resolve_vars_final(vars, ty, &mut unresolved);
            }
            for target in self.facts.methods.values_mut() {
                if let MethodTarget::Builtin { ret, .. } = target {
                    *ret = resolve_vars_final(vars, ret, &mut unresolved);
                }
            }
        }
        for index in unresolved {
            let span = self.var_spans.get(index).copied().unwrap_or(Span::dummy());
            self.error(
                "E-TYP-AMBIGUOUS",
                span,
                "cannot infer this type; add an explicit type annotation".to_string(),
            );
        }
    }

    // ------------------------------------------------------------------
    // Entrée
    // ------------------------------------------------------------------

    pub(super) fn run(&mut self, sources: Option<&SourceMap>) {
        self.validate_entry_points(sources);
        // Constantes (avant les corps : les corps peuvent les lire).
        let consts: Vec<(ModuleId, NodeId, &'p ConstItem)> = self
            .world
            .consts
            .iter()
            .map(|(id, c)| {
                (
                    self.world.item_module.get(id).copied().unwrap_or(0),
                    *id,
                    *c,
                )
            })
            .collect();
        for (module, id, c) in consts {
            self.check_const(module, id, c);
        }
        // Corps des fonctions de modules.
        let asts: Vec<&'p Module> = self.world.module_asts.clone();
        let mut fns: Vec<(ModuleId, NodeId, Span, &'p FnItem)> = Vec::new();
        for (module, ast) in asts.iter().enumerate() {
            for item in &ast.items {
                if let ItemKind::Fn(f) = &item.kind {
                    fns.push((module, item.id, item.span, f));
                }
            }
        }
        for (module, id, decl_span, f) in fns {
            self.check_fn(module, id, decl_span, f, None);
        }
        // Méthodes d'impl.
        let impl_items: Vec<(ModuleId, &'p Item, &'p ast::ImplItem)> =
            self.world.impl_items.clone();
        let mut methods: Vec<PendingMethod<'p>> = Vec::new();
        for (module, _, imp) in impl_items {
            let self_ty = self
                .facts
                .type_nodes
                .get(&imp.self_ty.id)
                .cloned()
                .unwrap_or(Type::Error);
            let generics: Vec<String> = imp.generics.iter().map(|g| g.name.name.clone()).collect();
            for (member, f) in imp.methods() {
                methods.push((
                    module,
                    member.id,
                    member.span,
                    f,
                    self_ty.clone(),
                    generics.clone(),
                ));
            }
        }
        for (module, id, decl_span, f, self_ty, generics) in methods {
            self.check_fn(module, id, decl_span, f, Some((self_ty, generics)));
        }
        self.finalize_inference();
    }

    pub(super) fn validate_entry_points(&mut self, sources: Option<&SourceMap>) {
        // `main` : uniquement dans le module racine (toutes les déclarations
        // sont comptées, y compris les doublons ignorés par la résolution).
        let mains: Vec<NodeId> = {
            let root_ast = self.world.module_asts[self.world.root];
            root_ast
                .items
                .iter()
                .filter_map(|item| match &item.kind {
                    ItemKind::Fn(f) if f.name.name == "main" => Some(item.id),
                    _ => None,
                })
                .collect()
        };
        let binary = match sources {
            Some(sources) => sources
                .file(self.program.file)
                .map(|f| f.path.ends_with("main.rv"))
                .unwrap_or(true),
            None => true,
        };
        if mains.is_empty() {
            if binary {
                self.error(
                    "E-TYP-MAIN-MISSING",
                    self.program.root.span,
                    "binary crate requires exactly one `fn main()`".to_string(),
                );
            }
        } else {
            self.facts.main_fn = Some(mains[0]);
            if mains.len() > 1 {
                for id in &mains[1..] {
                    let span = self
                        .world
                        .fns
                        .get(id)
                        .map(|f| f.span)
                        .unwrap_or(Span::dummy());
                    self.error(
                        "E-TYP-MAIN-DUPLICATE",
                        span,
                        "a binary crate must define exactly one `main`".to_string(),
                    );
                }
            }
            let span = self
                .world
                .fns
                .get(&mains[0])
                .map(|f| f.span)
                .unwrap_or(Span::dummy());
            self.validate_main_signature(mains[0], span);
        }

        // `test fn` : pas de paramètre, retour `()` ou `Result<(), Error>`.
        // Une signature invalide est une erreur de **typage** nommée
        // `E-TYP-MAIN` par `diagnostics.md` (« `test fn` invalide »), et non une
        // erreur de syntaxe : G-3.6 et MOD-4.10 décrivent la contrainte de
        // signature, pas une forme grammaticale refusée par le parseur.
        let tests = self.facts.test_fns.clone();
        for id in tests {
            let Some(sig) = self.facts.fn_sigs.get(&id).cloned() else {
                continue;
            };
            let span = self
                .world
                .fns
                .get(&id)
                .map(|f| f.span)
                .unwrap_or(Span::dummy());
            if !sig.params.is_empty() {
                self.error(
                    "E-TYP-MAIN",
                    span,
                    "a test function takes no parameter".to_string(),
                );
            }
            if !(sig.ret == Type::Unit || self.is_result_unit_error(&sig.ret)) {
                self.error(
                    "E-TYP-MAIN",
                    span,
                    "a test function returns `()` or `Result<(), Error>`".to_string(),
                );
            }
        }
    }

    pub(super) fn validate_main_signature(&mut self, id: NodeId, span: Span) {
        let Some(sig) = self.facts.fn_sigs.get(&id).cloned() else {
            return;
        };
        let mut ok = true;
        if !sig.params.is_empty() {
            ok = false;
            self.error(
                "E-TYP-MAIN",
                span,
                "`main` must not take parameters".to_string(),
            );
        }
        if !(sig.ret == Type::Unit || self.is_result_unit_error(&sig.ret)) {
            ok = false;
            self.error_note(
                "E-TYP-MAIN",
                span,
                format!("invalid return type `{}` for `main`", sig.ret),
                "expected `()` or `Result<(), Error>`",
                None,
            );
        }
        let _ = ok;
    }

    pub(super) fn is_result_unit_error(&self, ty: &Type) -> bool {
        matches!(ty, Type::Result(ok, err) if **ok == Type::Unit && builtins::is_error_type(err))
    }

    // ------------------------------------------------------------------
    // Fonctions
    // ------------------------------------------------------------------

    pub(super) fn check_fn(
        &mut self,
        module: ModuleId,
        item: NodeId,
        decl_span: Span,
        f: &FnItem,
        impl_ctx: Option<(Type, Vec<String>)>,
    ) {
        let Some(sig) = self.facts.fn_sigs.get(&item).cloned() else {
            return;
        };
        self.module = module;
        self.generics = match &impl_ctx {
            Some((_, impl_generics)) => impl_generics
                .iter()
                .enumerate()
                .map(|(i, n)| (n.clone(), i))
                .collect(),
            None => f
                .generics
                .iter()
                .enumerate()
                .map(|(i, g)| (g.name.name.clone(), i))
                .collect(),
        };
        self.self_ty = impl_ctx.as_ref().map(|(t, _)| t.clone());
        self.fn_ret = sig.ret.clone();
        self.current_fn = Some(item);
        self.in_const = false;
        self.loop_depth = 0;
        self.scopes.clear();
        self.push_scope();
        self.unused_candidates.clear();
        self.used_locals.clear();
        for param in &sig.params {
            let mutable = param.mutable
                || (param.is_receiver && matches!(sig.receiver, Some(Receiver::Exclusive)));
            self.bind_local(&param.name, param.ty.clone(), mutable, false, decl_span);
        }
        let body_ty = self.check_block(&f.body, Some(&sig.ret));
        // DIAG-1.6: `W-OWN-UNUSED-VARIABLE` is one of the two warnings Core 1.0
        // documents. A binding is unused when no path resolved it inside the
        // function; shadowing is handled conservatively (a name used once counts
        // for every binding of that name, so the warning can be missed but never
        // invented).
        for (name, span) in std::mem::take(&mut self.unused_candidates) {
            if !self.used_locals.contains(&name) {
                self.warning(
                    "W-OWN-UNUSED-VARIABLE",
                    span,
                    format!("variable `{name}` is never used"),
                );
            }
        }
        let body_ty = self.resolve_vars(&body_ty);
        let ret = self.resolve_vars(&sig.ret);
        // G-5.11 / TYP-1.6: a function declared `-> !` must have a divergent
        // body. `compatible` treats `!` as bottom in both directions, so the
        // normal body check below cannot catch a body that has type `()`.
        // EXPR-5.11/6.7/G-6.19 type `loop`/`while`/`for` as `()`, even when the
        // loop diverges, so only `panic`, `return` or a call to a divergent
        // function can make the body divergent.
        if matches!(ret, Type::Never) && !matches!(body_ty, Type::Never | Type::Error) {
            self.error(
                "E-TYP-RETURN",
                f.body.span,
                "a function returning `!` requires a divergent body; `loop`/`while`/`for` \
                 have type `()` (EXPR-5.11/6.7), so use `panic`, `return` or a call to a \
                 divergent function"
                    .to_string(),
            );
        }
        if !Self::compatible(&body_ty, &ret)
            && !self.unify(&body_ty, &ret, f.body.span, "function body")
        {
            self.error(
                "E-TYP-MISMATCH",
                f.body.span,
                format!(
                    "function body returns `{body_ty}` but `{}` returns `{ret}`",
                    sig.name
                ),
            );
        }
        self.pop_scope();
        self.current_fn = None;
        self.self_ty = None;
        self.generics.clear();
    }

    // ------------------------------------------------------------------
    // Blocs et instructions
    // ------------------------------------------------------------------

    pub(super) fn check_block(&mut self, block: &Block, expected: Option<&Type>) -> Type {
        self.push_scope();
        let mut last_type: Option<Type> = None;
        for stmt in &block.stmts {
            last_type = Some(self.check_stmt(stmt));
        }
        let ty = match &block.tail {
            Some(tail) => self.check_expr(tail, expected),
            None => match &last_type {
                Some(Type::Never) => Type::Never,
                _ => Type::Unit,
            },
        };
        self.pop_scope();
        self.facts.expr_types.insert(block.id, ty.clone());
        ty
    }

    pub(super) fn check_stmt(&mut self, stmt: &Stmt) -> Type {
        match &stmt.kind {
            StmtKind::Let(lets) => {
                let declared = lets.ty.as_ref().map(|t| self.resolve_type(t));
                // G-5.11 / TYP-1.6: `!` is not a storable local type. A diverging
                // initializer still coerces to its expected type; the annotation
                // itself would make the generated Rust bind a never-typed place,
                // which the reference backend cannot represent.
                if let (Some(declared_ty), Some(annotation)) = (&declared, lets.ty.as_ref()) {
                    if crate::types::contains_never(declared_ty) {
                        let message = format!(
                            "the never type (`!`) is not a valid local type annotation in Core 1.0 (found `{declared_ty}`)"
                        );
                        let span = annotation.span;
                        self.error("E-TYP-MISMATCH", span, message);
                    }
                }
                // A loop is a statement, never a value (G-6.19, F-SYN-019/033).
                // Without this check `let x = while c { 1 };` type-checks as `()`
                // and silently binds a unit, which the specification forbids.
                if let Some(init) = lets.init.as_ref() {
                    if matches!(
                        init.kind,
                        ExprKind::While { .. } | ExprKind::Loop { .. } | ExprKind::For { .. }
                    ) {
                        self.error(
                            "E-TYP-STATEMENT-VALUE",
                            init.span,
                            "a `while`/`loop`/`for` expression is a statement and cannot be \
                             bound by `let`"
                                .to_string(),
                        );
                    }
                }
                let init_ty = lets
                    .init
                    .as_ref()
                    .map(|e| self.check_expr(e, declared.as_ref()));
                let final_ty = match (&declared, &init_ty) {
                    (Some(d), Some(i)) => {
                        self.unify(
                            i,
                            d,
                            lets.init.as_ref().map(|e| e.span).unwrap_or(stmt.span),
                            "let",
                        );
                        d.clone()
                    }
                    (Some(d), None) => d.clone(),
                    (None, Some(i)) => i.clone(),
                    (None, None) => {
                        self.error(
                            "E-TYP-AMBIGUOUS",
                            stmt.span,
                            "`let` requires a type or an initializer".to_string(),
                        );
                        Type::Error
                    }
                };
                self.check_let_pattern(&lets.pat, &final_ty, lets.init.is_none());
                if let PatKind::Binding { name, .. } = &lets.pat.kind {
                    self.unused_candidates
                        .push((name.name.clone(), lets.pat.span));
                }
                Type::Unit
            }
            StmtKind::Expr { expr, semi } => {
                let ty = self.check_expr(expr, None);
                if *semi && !matches!(ty, Type::Unit | Type::Never | Type::Error) {
                    self.error(
                        "E-TYP-STATEMENT-VALUE",
                        expr.span,
                        format!("expression of type `{ty}` used as a statement; expected `()`"),
                    );
                }
                ty
            }
        }
    }

    pub(super) fn check_let_pattern(&mut self, pat: &Pat, ty: &Type, deferred: bool) {
        match &pat.kind {
            PatKind::Binding { mutable, name } => {
                if let Some(existing) = self.scopes.last().and_then(|s| s.get(&name.name)) {
                    // L'ombrement est autorisé : on écrase la liaison.
                    let _ = existing;
                }
                self.bind_local(&name.name, ty.clone(), *mutable, deferred, pat.span);
                self.facts.pat_types.insert(pat.id, ty.clone());
            }
            _ => {
                self.error(
                    "E-PARSE-EXPECTED",
                    pat.span,
                    "`let` binds exactly one identifier in Core 1.0".to_string(),
                );
                self.facts.pat_types.insert(pat.id, ty.clone());
            }
        }
    }

    // ------------------------------------------------------------------
    // Expressions
    // ------------------------------------------------------------------

    pub(super) fn check_expr(&mut self, expr: &Expr, expected: Option<&Type>) -> Type {
        // EXPR-5.11 / EXPR-6.7 / G-6.19: control expressions have type `()` and
        // are not value expressions. They are allowed as direct statements and
        // as the tail of a `()` block, not as arguments, fields, assignment
        // values, match arms/scrutinees or repeated array values.
        if self.value_position > 0
            && matches!(
                expr.kind,
                ExprKind::While { .. } | ExprKind::Loop { .. } | ExprKind::For { .. }
            )
        {
            self.error(
                "E-TYP-STATEMENT-VALUE",
                expr.span,
                "a `while`/`loop`/`for` expression is a statement and cannot be used as a value"
                    .to_string(),
            );
        }
        let ty = self.check_expr_inner(expr, expected);
        // TYP-1.6: `!` is not inhabitable, so it must never become a type
        // argument (`Option<!>`, `Result<!, E>`, `W<!>`). The bottom type only
        // coerces towards an expected type; when an aggregate constructor or
        // literal *infers* its argument from a diverging expression, the type is
        // genuinely ambiguous. Emitting E-TYP-AMBIGUOUS here avoids generating
        // Rust that needs the unstable `never_type` feature.
        if matches!(
            ty,
            Type::Option(_) | Type::Result(_, _) | Type::Nominal { .. }
        ) && contains_never(&ty)
        {
            self.error(
                "E-TYP-AMBIGUOUS",
                expr.span,
                "cannot infer an aggregate type from a diverging expression; \
                 annotate it or use a non-diverging value"
                    .to_string(),
            );
        }
        // TYP-13.2 / TYP-13.4 : les variables d'inférence sont une
        // représentation interne. Une fois qu'une contrainte les a liées
        // (`v.push(1)` lie l'élément de `Vec::new()`), tout type remis à un
        // appelant doit refléter la liaison, sinon une comparaison brute
        // (`s += v[i]`, `y = v[i]`, ...) voit encore `$0` et émet un faux
        // `E-TYP-MISMATCH`. `resolve_vars` ne lie rien : il substitue seulement
        // les variables déjà liées, donc aucune inférence n'est ajoutée.
        let ty = self.resolve_vars(&ty);
        self.record_expr_type(expr.id, &ty);
        ty
    }
}
