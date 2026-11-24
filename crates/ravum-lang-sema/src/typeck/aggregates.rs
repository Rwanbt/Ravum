use super::*;

impl<'p, 'w, 'f> TypeChecker<'p, 'w, 'f> {
    pub(super) fn check_struct_lit(
        &mut self,
        expr: &Expr,
        path: &Path,
        fields: &[FieldInit],
        expected: Option<&Type>,
    ) -> Type {
        let target =
            match resolve::resolve_value_path(self.world, self.module, &self.generics, path) {
                Ok(target) => target,
                Err(error) => {
                    self.emit_path_error(path.span, &error);
                    self.facts.resolutions.insert(path.id, Resolution::Error);
                    for field in fields {
                        self.check_expr(&field.value, None);
                    }
                    return Type::Error;
                }
            };
        // Le codegen exige un fait de résolution pour le chemin du littéral.
        match &target {
            ValuePath::Type(Type::Nominal { def, .. }) => {
                self.facts
                    .resolutions
                    .insert(path.id, Resolution::Item { item: *def });
            }
            ValuePath::Variant { enum_item, variant } => {
                self.facts.resolutions.insert(
                    path.id,
                    Resolution::Variant {
                        enum_item: *enum_item,
                        variant: *variant,
                    },
                );
            }
            ValuePath::BuiltinNone => {
                self.facts.resolutions.insert(
                    path.id,
                    Resolution::BuiltinConst {
                        path: OPTION_NONE.to_string(),
                    },
                );
            }
            _ => {
                self.facts.resolutions.insert(path.id, Resolution::Error);
            }
        }
        match target {
            // A struct literal path never carries type arguments: `type_of_symbol`
            // builds a nominal with empty `args`, and G-5.9 refuses explicit
            // generic arguments in expressions (E-PARSE-UNEXPECTED).
            ValuePath::Type(Type::Nominal { def, name, .. }) => {
                if self.world.nominal_kind(def) != Some(NominalKind::Struct) {
                    self.error(
                        "E-RES-UNKNOWN",
                        path.span,
                        format!("`{name}` is not a struct"),
                    );
                    for field in fields {
                        self.check_expr(&field.value, None);
                    }
                    return Type::Error;
                }
                let info = match self.facts.structs.get(&def) {
                    Some(info) => info.clone(),
                    None => return Type::Error,
                };
                let mut subst: BTreeMap<usize, Type> = BTreeMap::new();
                if let Some(Type::Nominal {
                    def: exp_def,
                    args: exp_args,
                    ..
                }) = expected
                {
                    if *exp_def == def && exp_args.len() == info.generics.len() {
                        for (i, arg) in exp_args.iter().enumerate() {
                            subst.insert(i, arg.clone());
                        }
                    }
                }
                let fields_ok = self.check_struct_fields(
                    expr,
                    fields,
                    &info.fields,
                    &info.generics,
                    &def,
                    &mut subst,
                );
                let _ = fields_ok;
                let mut final_args = Vec::with_capacity(info.generics.len());
                for index in 0..info.generics.len() {
                    match subst.get(&index) {
                        Some(t) => final_args.push(self.resolve_vars(t)),
                        None => final_args.push(self.fresh_var(expr.span)),
                    }
                }
                Type::Nominal {
                    def,
                    name: info.name.clone(),
                    args: final_args,
                }
            }
            ValuePath::Variant { enum_item, variant } => {
                let info = match self.facts.enums.get(&enum_item) {
                    Some(info) => info.clone(),
                    None => return Type::Error,
                };
                let variant_info = match info.variants.get(variant) {
                    Some(v) => v.clone(),
                    None => return Type::Error,
                };
                if variant_info.kind != crate::facts::VariantKind::Struct {
                    self.error(
                        "E-TYP-FIELD",
                        path.span,
                        format!(
                            "variant `{}::{}` has no named fields",
                            info.name, variant_info.name
                        ),
                    );
                    for field in fields {
                        self.check_expr(&field.value, None);
                    }
                    return Type::Error;
                }
                let mut subst: BTreeMap<usize, Type> = BTreeMap::new();
                if let Some(Type::Nominal {
                    def: exp_def,
                    args: exp_args,
                    ..
                }) = expected
                {
                    if *exp_def == enum_item && exp_args.len() == info.generics.len() {
                        for (i, arg) in exp_args.iter().enumerate() {
                            subst.insert(i, arg.clone());
                        }
                    }
                }
                self.check_struct_fields(
                    expr,
                    fields,
                    &variant_info.fields,
                    &info.generics,
                    &enum_item,
                    &mut subst,
                );
                let mut final_args = Vec::with_capacity(info.generics.len());
                for index in 0..info.generics.len() {
                    match subst.get(&index) {
                        Some(t) => final_args.push(t.clone()),
                        None => {
                            final_args.push(Type::Error);
                            self.error(
                                "E-TYP-AMBIGUOUS",
                                expr.span,
                                format!("cannot infer the generic arguments of `{}`", info.name),
                            );
                        }
                    }
                }
                Type::Nominal {
                    def: enum_item,
                    name: info.name.clone(),
                    args: final_args,
                }
            }
            ValuePath::BuiltinNone => self.builtin_none_type(expr, expected),
            _ => {
                self.error(
                    "E-RES-UNKNOWN",
                    path.span,
                    "struct literal must target a struct or a struct variant".to_string(),
                );
                for field in fields {
                    self.check_expr(&field.value, None);
                }
                Type::Error
            }
        }
    }

    pub(super) fn check_struct_fields(
        &mut self,
        expr: &Expr,
        provided: &[FieldInit],
        declared: &[crate::facts::FieldInfo],
        generics: &[String],
        def: &NodeId,
        subst: &mut BTreeMap<usize, Type>,
    ) -> bool {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for field in provided {
            if !seen.insert(field.name.name.as_str()) {
                self.error(
                    "E-TYP-FIELD",
                    field.name.span,
                    format!("duplicate field `{}`", field.name.name),
                );
            }
            let Some(decl) = declared.iter().find(|d| d.name == field.name.name) else {
                self.error(
                    "E-TYP-FIELD",
                    field.name.span,
                    format!("no field `{}` in this aggregate", field.name.name),
                );
                self.check_expr(&field.value, None);
                continue;
            };
            let decl = decl.clone();
            if !decl.public && !self.aggregate_field_visible(def) {
                self.error(
                    "E-RES-PRIVATE",
                    field.name.span,
                    format!("field `{}` is private", decl.name),
                );
            }
            let expected_ty = substitute(&decl.ty, subst);
            let value_ty = self.check_arg(&field.value, &expected_ty);
            // TYP-1.6: `!` must not determine a generic field type. Without
            // this check, `W { v: panic("x") }` leaks the unresolved `T` into
            // the generated Rust (E0425). `check_arg`/`coerce` returns the
            // expected type for a `!` argument, so the raw expression type is
            // read from the facts recorded by `check_expr`.
            let raw_value_ty = self
                .facts
                .expr_types
                .get(&field.value.id)
                .cloned()
                .unwrap_or_else(|| value_ty.clone());
            if contains_never(&raw_value_ty)
                && (matches!(expected_ty, Type::Param { .. } | Type::Error)
                    || has_unbound_param(&expected_ty))
            {
                self.error(
                    "E-TYP-AMBIGUOUS",
                    field.value.span,
                    "cannot infer a generic field type from a diverging expression; \
                     annotate the field type or use a non-diverging value"
                        .to_string(),
                );
            }
            if !self.unify_pattern(&decl.ty, &value_ty, subst) {
                self.error(
                    "E-TYP-MISMATCH",
                    field.value.span,
                    format!(
                        "field `{}`: expected `{}`, found `{}`",
                        field.name.name, expected_ty, value_ty
                    ),
                );
            }
        }
        let missing: Vec<String> = declared
            .iter()
            .filter(|d| !seen.contains(d.name.as_str()))
            .map(|d| d.name.clone())
            .collect();
        if !missing.is_empty() {
            self.error(
                "E-TYP-MISSING-FIELD",
                expr.span,
                format!("missing field(s): {}", missing.join(", ")),
            );
        }
        let _ = generics;
        true
    }

    pub(super) fn aggregate_field_visible(&self, def: &NodeId) -> bool {
        let def_module = self.world.item_module.get(def).copied().unwrap_or(0);
        let mut current = Some(self.module);
        while let Some(m) = current {
            if m == def_module {
                return true;
            }
            current = self.world.modules[m].parent;
        }
        false
    }

    // ------------------------------------------------------------------
    // Patterns et exhaustivité
    // ------------------------------------------------------------------

    pub(super) fn check_pattern(&mut self, pat: &Pat, expected: &Type) -> Type {
        let ty = match &pat.kind {
            PatKind::Wild => expected.clone(),
            PatKind::Binding { mutable, name } => {
                // OWN-6.8 / P1: under a match on a reference, a binding is a
                // bounded reference to the matched part, not a move of it.
                let bound = match self.pat_by_ref {
                    Some(m) if !matches!(expected, Type::Error | Type::Never) => Type::Ref {
                        mutable: m,
                        inner: Box::new(expected.clone()),
                    },
                    _ => expected.clone(),
                };
                self.bind_local(&name.name, bound, *mutable, false, pat.span);
                expected.clone()
            }
            PatKind::Lit(lit) => self.check_lit_pattern(pat, lit, expected),
            PatKind::Tuple(items) => {
                if let Type::Tuple(types) = expected {
                    if types.len() != items.len() {
                        // Arity, not a type mismatch: every other arity check in
                        // this file (calls, variants, other patterns) reports
                        // `E-TYP-ARITY`, and `spec/language/features.md`
                        // F-SYN-030 mandates that code here too.
                        self.error(
                            "E-TYP-ARITY",
                            pat.span,
                            format!(
                                "tuple pattern has {} element(s), type has {}",
                                items.len(),
                                types.len()
                            ),
                        );
                    }
                    for (item, ty) in items.iter().zip(types.iter()) {
                        self.check_pattern(item, ty);
                    }
                    expected.clone()
                } else {
                    // The recovered error type matches any pattern (one
                    // diagnostic already explains it).
                    if !matches!(expected, Type::Error | Type::Never) {
                        self.error(
                            "E-TYP-MISMATCH",
                            pat.span,
                            format!("tuple pattern cannot match type `{expected}`"),
                        );
                    }
                    for item in items {
                        self.check_pattern(item, &Type::Error);
                    }
                    Type::Error
                }
            }
            PatKind::Path(path) => self.check_path_pattern(pat, path, expected),
            PatKind::Variant { path, fields } => {
                self.check_variant_pattern(pat, path, fields, expected)
            }
        };
        // A binding under a match on a reference records its own type, the
        // bounded reference (OWN-6.8); the pattern still matches `ty`.
        let recorded = match (&pat.kind, self.pat_by_ref) {
            (PatKind::Binding { .. }, Some(m)) if !matches!(ty, Type::Error | Type::Never) => {
                Type::Ref {
                    mutable: m,
                    inner: Box::new(ty.clone()),
                }
            }
            _ => ty.clone(),
        };
        self.facts.pat_types.insert(pat.id, recorded);
        ty
    }

    pub(super) fn check_lit_pattern(&mut self, pat: &Pat, lit: &LitPat, expected: &Type) -> Type {
        match lit {
            LitPat::Int { value, suffix, neg } => {
                let ty = match suffix {
                    Some(suffix) => int_type(*suffix),
                    None => match expected {
                        Type::I8
                        | Type::I16
                        | Type::I32
                        | Type::I64
                        | Type::I128
                        | Type::Isize
                        | Type::U8
                        | Type::U16
                        | Type::U32
                        | Type::U64
                        | Type::U128
                        | Type::Usize => expected.clone(),
                        _ => Type::I32,
                    },
                };
                // TYP-1.6: `!` coerces to every type, so a literal pattern on a
                // diverging scrutinee is not a type mismatch (the bool, char and
                // text patterns below already exempt `!`/`<error>`). Without
                // this exemption `match panic("x") { 1 => …, _ => … }` was
                // rejected while the same match with `true`/`'a'`/`"a"` was not.
                if !expected.is_int() && !matches!(expected, Type::Error | Type::Never) {
                    self.error(
                        "E-TYP-MISMATCH",
                        pat.span,
                        format!("integer pattern cannot match type `{expected}`"),
                    );
                }
                // G-6.37: the sign decides which bound applies. A negative
                // pattern never fits an unsigned type, and `-128` fits `i8`
                // while `128` does not (`int_bounds` gives the magnitude of the
                // minimum). Before G-6.37 was implemented the checker guessed a
                // negative from a two's-complement `value > i128::MAX`, which
                // also accepted an out-of-range positive literal.
                let fits = if *neg {
                    negative_value_fits(*value, &ty)
                } else {
                    int_value_fits(*value, &ty)
                };
                if !fits {
                    let shown = if *neg {
                        format!("-{value}")
                    } else {
                        format!("{value}")
                    };
                    self.error(
                        "E-TYP-LITERAL-RANGE",
                        pat.span,
                        format!("integer pattern `{shown}` does not fit in `{ty}`"),
                    );
                }
                ty
            }
            // G-6.36: a float pattern compares with `==`; the literal type is
            // the suffix, else the scrutinee type, else `f64` (the default of
            // float expressions). `NaN` never matches and `-0.0` matches `0.0`
            // because no bit comparison is made.
            LitPat::Float { suffix, .. } => {
                let ty = match suffix {
                    Some(FloatTy::F32) => Type::F32,
                    Some(FloatTy::F64) => Type::F64,
                    None => match expected {
                        Type::F32 => Type::F32,
                        _ => Type::F64,
                    },
                };
                if !matches!(expected, Type::F32 | Type::F64 | Type::Error | Type::Never) {
                    self.error(
                        "E-TYP-MISMATCH",
                        pat.span,
                        format!("float pattern cannot match type `{expected}`"),
                    );
                }
                ty
            }
            LitPat::Bool(_) => {
                if *expected != Type::Bool && !matches!(expected, Type::Error | Type::Never) {
                    self.error(
                        "E-TYP-MISMATCH",
                        pat.span,
                        format!("`bool` pattern cannot match type `{expected}`"),
                    );
                }
                Type::Bool
            }
            LitPat::Char(_) => {
                if *expected != Type::Char && !matches!(expected, Type::Error | Type::Never) {
                    self.error(
                        "E-TYP-MISMATCH",
                        pat.span,
                        format!("`char` pattern cannot match type `{expected}`"),
                    );
                }
                Type::Char
            }
            LitPat::Str(_) => {
                if !is_text_like(expected) && !matches!(expected, Type::Error | Type::Never) {
                    self.error(
                        "E-TYP-MISMATCH",
                        pat.span,
                        format!("text pattern cannot match type `{expected}`"),
                    );
                }
                Type::str_ref()
            }
        }
    }

    pub(super) fn check_path_pattern(&mut self, pat: &Pat, path: &Path, expected: &Type) -> Type {
        match resolve::resolve_value_path(self.world, self.module, &self.generics, path) {
            Ok(ValuePath::Variant { enum_item, variant }) => {
                self.variant_pats.insert(pat.id, (enum_item, variant));
                self.facts
                    .resolutions
                    .insert(path.id, Resolution::Variant { enum_item, variant });
                // A bare path only matches a unit variant: `E::B` for a variant
                // that carries a payload is not a pattern (rustc E0532/E0533).
                let payload = self.facts.enums.get(&enum_item).and_then(|info| {
                    let found = info.variants.get(variant)?;
                    let fields = match found.kind {
                        crate::facts::VariantKind::Unit => return None,
                        crate::facts::VariantKind::Tuple => found.tuple_types.len(),
                        crate::facts::VariantKind::Struct => found.fields.len(),
                    };
                    Some((info.name.clone(), found.name.clone(), fields))
                });
                if let Some((enum_name, variant_name, fields)) = payload {
                    self.error(
                        "E-TYP-ARITY",
                        pat.span,
                        format!(
                            "variant `{enum_name}::{variant_name}` expects {fields} field(s), found 0"
                        ),
                    );
                }
                self.pattern_enum_ty(pat.span, enum_item, expected)
            }
            Ok(ValuePath::BuiltinNone) => {
                self.facts.resolutions.insert(
                    path.id,
                    Resolution::BuiltinConst {
                        path: OPTION_NONE.to_string(),
                    },
                );
                match expected {
                    Type::Option(_) | Type::Error | Type::Never => expected.clone(),
                    _ => {
                        self.error(
                            "E-TYP-MISMATCH",
                            pat.span,
                            format!("`None` pattern cannot match type `{expected}`"),
                        );
                        Type::Error
                    }
                }
            }
            Ok(ValuePath::Item(id)) => {
                if let Some(info) = self.facts.consts.get(&id) {
                    let ty = info.ty.clone();
                    self.facts
                        .resolutions
                        .insert(path.id, Resolution::Item { item: id });
                    if !Self::compatible(&ty, expected) {
                        self.error(
                            "E-TYP-MISMATCH",
                            pat.span,
                            format!("constant pattern has type `{ty}`, expected `{expected}`"),
                        );
                    }
                    ty
                } else {
                    self.error(
                        "E-RES-UNKNOWN",
                        pat.span,
                        "constant pattern must reference a `const`".to_string(),
                    );
                    Type::Error
                }
            }
            Ok(other) => {
                let _ = other;
                self.error(
                    "E-RES-UNKNOWN",
                    pat.span,
                    "pattern does not name a variant or a constant".to_string(),
                );
                Type::Error
            }
            Err(error) => {
                self.emit_path_error(pat.span, &error);
                Type::Error
            }
        }
    }

    pub(super) fn pattern_enum_ty(
        &mut self,
        span: Span,
        enum_item: NodeId,
        expected: &Type,
    ) -> Type {
        match expected {
            Type::Nominal { def, .. } if *def == enum_item => expected.clone(),
            Type::Error | Type::Never => expected.clone(),
            _ => {
                self.error(
                    "E-TYP-MISMATCH",
                    span,
                    format!("variant pattern does not match type `{expected}`"),
                );
                Type::Error
            }
        }
    }

    pub(super) fn check_variant_pattern(
        &mut self,
        pat: &Pat,
        path: &Path,
        fields: &VariantPatFields,
        expected: &Type,
    ) -> Type {
        let target =
            match resolve::resolve_value_path(self.world, self.module, &self.generics, path) {
                Ok(target) => target,
                Err(error) => {
                    self.emit_path_error(pat.span, &error);
                    for field in variant_pat_patterns(fields) {
                        self.check_pattern(field, &Type::Error);
                    }
                    return Type::Error;
                }
            };
        match target {
            ValuePath::Variant { enum_item, variant } => {
                self.variant_pats.insert(pat.id, (enum_item, variant));
                self.facts
                    .resolutions
                    .insert(path.id, Resolution::Variant { enum_item, variant });
                let info = match self.facts.enums.get(&enum_item) {
                    Some(info) => info.clone(),
                    None => return Type::Error,
                };
                let variant_info = match info.variants.get(variant) {
                    Some(v) => v.clone(),
                    None => return Type::Error,
                };
                let subst = match expected {
                    Type::Nominal { def, args, .. } if *def == enum_item => {
                        let mut map = BTreeMap::new();
                        for (i, arg) in args.iter().enumerate() {
                            map.insert(i, arg.clone());
                        }
                        map
                    }
                    _ => BTreeMap::new(),
                };
                self.check_variant_payload(
                    pat,
                    fields,
                    &variant_info,
                    &subst,
                    &info.name,
                    &variant_info.name,
                );
                match expected {
                    Type::Nominal { def, .. } if *def == enum_item => expected.clone(),
                    Type::Error | Type::Never => expected.clone(),
                    _ => {
                        self.error(
                            "E-TYP-MISMATCH",
                            pat.span,
                            format!("variant pattern does not match type `{expected}`"),
                        );
                        Type::Error
                    }
                }
            }
            ValuePath::BuiltinCtor { ctor } => {
                self.facts.resolutions.insert(
                    path.id,
                    Resolution::BuiltinFn {
                        path: ctor.path().to_string(),
                    },
                );
                self.check_builtin_variant_payload(pat, Some(ctor), fields, expected)
            }
            ValuePath::BuiltinNone => {
                self.facts.resolutions.insert(
                    path.id,
                    Resolution::BuiltinConst {
                        path: OPTION_NONE.to_string(),
                    },
                );
                self.check_builtin_variant_payload(pat, None, fields, expected)
            }
            _ => {
                self.error(
                    "E-RES-UNKNOWN",
                    pat.span,
                    "pattern does not name a variant".to_string(),
                );
                for field in variant_pat_patterns(fields) {
                    self.check_pattern(field, &Type::Error);
                }
                Type::Error
            }
        }
    }

    pub(super) fn check_variant_payload(
        &mut self,
        pat: &Pat,
        fields: &VariantPatFields,
        variant_info: &crate::facts::VariantInfo,
        subst: &BTreeMap<usize, Type>,
        enum_name: &str,
        variant_name: &str,
    ) {
        match fields {
            VariantPatFields::Tuple(pats) => {
                if variant_info.kind != crate::facts::VariantKind::Tuple
                    || pats.len() != variant_info.tuple_types.len()
                {
                    self.error(
                        "E-TYP-ARITY",
                        pat.span,
                        format!(
                            "variant `{enum_name}::{variant_name}` expects {} field(s), found {}",
                            variant_info.tuple_types.len(),
                            pats.len()
                        ),
                    );
                }
                for (sub, ty) in pats.iter().zip(variant_info.tuple_types.iter()) {
                    let ty = substitute(ty, subst);
                    self.check_pattern(sub, &ty);
                }
            }
            VariantPatFields::Struct(field_pats) => {
                if variant_info.kind != crate::facts::VariantKind::Struct {
                    self.error(
                        "E-TYP-ARITY",
                        pat.span,
                        format!("variant `{enum_name}::{variant_name}` has no named fields"),
                    );
                }
                let mut seen: BTreeSet<&str> = BTreeSet::new();
                for field in field_pats {
                    seen.insert(field.name.name.as_str());
                    match variant_info
                        .fields
                        .iter()
                        .find(|f| f.name == field.name.name)
                    {
                        Some(decl) => {
                            let ty = substitute(&decl.ty, subst);
                            self.check_pattern(&field.pat, &ty);
                        }
                        None => {
                            self.error(
                                "E-TYP-FIELD",
                                field.name.span,
                                format!("no field `{}` in this variant", field.name.name),
                            );
                            self.check_pattern(&field.pat, &Type::Error);
                        }
                    }
                }
                let missing: Vec<String> = variant_info
                    .fields
                    .iter()
                    .filter(|f| !seen.contains(f.name.as_str()))
                    .map(|f| f.name.clone())
                    .collect();
                if !missing.is_empty() {
                    self.error(
                        "E-TYP-MISSING-FIELD",
                        pat.span,
                        format!("missing field(s): {}", missing.join(", ")),
                    );
                }
            }
        }
    }

    pub(super) fn check_builtin_variant_payload(
        &mut self,
        pat: &Pat,
        ctor: Option<PayloadCtor>,
        fields: &VariantPatFields,
        expected: &Type,
    ) -> Type {
        // `None` stands for `Option::None`, the only unit prelude constructor.
        match ctor {
            Some(PayloadCtor::Some) => {
                let inner = match expected {
                    Type::Option(inner) => (**inner).clone(),
                    Type::Error | Type::Never => Type::Error,
                    _ => {
                        self.error(
                            "E-TYP-MISMATCH",
                            pat.span,
                            format!("`Some` pattern cannot match type `{expected}`"),
                        );
                        Type::Error
                    }
                };
                match fields {
                    VariantPatFields::Tuple(pats) => {
                        if pats.len() != 1 {
                            self.error(
                                "E-TYP-ARITY",
                                pat.span,
                                "`Some` pattern expects exactly 1 field".to_string(),
                            );
                        }
                        for sub in pats {
                            self.check_pattern(sub, &inner);
                        }
                    }
                    VariantPatFields::Struct(_) => {
                        self.error(
                            "E-TYP-ARITY",
                            pat.span,
                            "`Some` pattern takes a tuple field".to_string(),
                        );
                    }
                }
                expected.clone()
            }
            Some(ctor @ (PayloadCtor::Ok | PayloadCtor::Err)) => {
                let path = ctor.path();
                let payload = match (ctor, expected) {
                    (PayloadCtor::Ok, Type::Result(ok, _)) => (**ok).clone(),
                    (PayloadCtor::Err, Type::Result(_, err)) => (**err).clone(),
                    (_, Type::Error | Type::Never) => Type::Error,
                    _ => {
                        self.error(
                            "E-TYP-MISMATCH",
                            pat.span,
                            format!("`{path}` pattern cannot match type `{expected}`"),
                        );
                        Type::Error
                    }
                };
                match fields {
                    VariantPatFields::Tuple(pats) => {
                        if pats.len() != 1 {
                            self.error(
                                "E-TYP-ARITY",
                                pat.span,
                                format!("`{path}` pattern expects exactly 1 field"),
                            );
                        }
                        for sub in pats {
                            self.check_pattern(sub, &payload);
                        }
                    }
                    VariantPatFields::Struct(_) => {
                        self.error(
                            "E-TYP-ARITY",
                            pat.span,
                            format!("`{path}` pattern takes a tuple field"),
                        );
                    }
                }
                expected.clone()
            }
            None => {
                if !matches!(expected, Type::Option(_) | Type::Error | Type::Never) {
                    self.error(
                        "E-TYP-MISMATCH",
                        pat.span,
                        format!("`None` pattern cannot match type `{expected}`"),
                    );
                }
                // TYP-10.2 / G-6.29: `None` is a unit constructor. Its payload
                // was never validated, so `match o { None(v) => … }` on an
                // `Option<i32>` was accepted while every user unit variant
                // correctly reports E-TYP-ARITY. A payload on `None` is never
                // reachable at run time.
                match fields {
                    VariantPatFields::Tuple(pats) if pats.is_empty() => {}
                    VariantPatFields::Tuple(pats) => {
                        self.error(
                            "E-TYP-ARITY",
                            pat.span,
                            format!("`None` pattern takes no field, {} given", pats.len()),
                        );
                        for sub in pats {
                            self.check_pattern(sub, &Type::Error);
                        }
                    }
                    VariantPatFields::Struct(struct_fields) => {
                        self.error(
                            "E-TYP-ARITY",
                            pat.span,
                            "`None` pattern takes no field".to_string(),
                        );
                        for field in struct_fields {
                            self.check_pattern(&field.pat, &Type::Error);
                        }
                    }
                }
                expected.clone()
            }
        }
    }

    // ------------------------------------------------------------------
    // Constantes
    // ------------------------------------------------------------------
}
