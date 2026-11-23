use super::*;

pub(crate) struct HeaderCtx<'w, 'a, 'f> {
    pub(crate) world: &'w World<'a>,
    pub(crate) facts: &'f mut Facts,
    pub(crate) diags: &'f mut DiagnosticBag,
    pub(crate) module: ModuleId,
    pub(crate) generics: Vec<(String, usize)>,
    pub(crate) self_ty: Option<Type>,
}

impl<'w, 'a, 'f> HeaderCtx<'w, 'a, 'f> {
    pub(crate) fn new(
        world: &'w World<'a>,
        facts: &'f mut Facts,
        diags: &'f mut DiagnosticBag,
        module: ModuleId,
        generics: Vec<(String, usize)>,
        self_ty: Option<Type>,
    ) -> Self {
        Self {
            world,
            facts,
            diags,
            module,
            generics,
            self_ty,
        }
    }

    pub(crate) fn error(&mut self, code: &'static str, span: Span, message: String) {
        self.diags.error(code, span, message);
    }

    pub(crate) fn resolve_type(&mut self, ty: &AstType) -> Type {
        let resolved = resolve_type_syntax(
            self.world,
            &mut self.facts.type_nodes,
            &mut self.facts.expr_types,
            self.diags,
            self.module,
            &self.generics,
            self.self_ty.as_ref(),
            ty,
        );
        check_written_type_depth(self.diags, ty, resolved)
    }
}

pub(crate) fn generic_scope(
    diags: &mut DiagnosticBag,
    params: &[ast::GenericParam],
) -> Vec<(String, usize)> {
    let mut out = Vec::with_capacity(params.len());
    for (index, param) in params.iter().enumerate() {
        if out.iter().any(|(n, _)| n == &param.name.name) {
            diags.error(
                "E-RES-DUPLICATE",
                param.name.span,
                format!("duplicate generic parameter `{}`", param.name.name),
            );
            continue;
        }
        out.push((param.name.name.clone(), index));
    }
    out
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_fn_header(
    world: &World,
    facts: &mut Facts,
    diags: &mut DiagnosticBag,
    module: ModuleId,
    item: &Item,
    f: &FnItem,
    impl_ctx: Option<(Type, Vec<String>)>,
) -> FnSig {
    let (generics, self_ty) = match &impl_ctx {
        Some((self_ty, impl_generics)) => (
            impl_generics
                .iter()
                .enumerate()
                .map(|(i, n)| (n.clone(), i))
                .collect(),
            Some(self_ty.clone()),
        ),
        None => (generic_scope(diags, &f.generics), None),
    };
    let mut ctx = HeaderCtx::new(world, facts, diags, module, generics, self_ty);

    if impl_ctx.is_some() && !f.generics.is_empty() {
        ctx.error(
            "E-TYP-ARITY",
            f.span,
            "methods cannot declare their own generic parameters in Core 1.0".to_string(),
        );
    }

    let mut params: Vec<ParamSig> = Vec::with_capacity(f.params.len());
    let mut receiver: Option<Receiver> = None;
    for (index, param) in f.params.iter().enumerate() {
        match &param.kind {
            ast::ParamKind::Receiver(rec) => {
                if index != 0 {
                    ctx.error(
                        "E-RES-SELF",
                        param.span,
                        "`self` parameter must be the first parameter".to_string(),
                    );
                }
                if impl_ctx.is_none() {
                    ctx.error(
                        "E-RES-SELF",
                        param.span,
                        "`self` parameter is only allowed in an `impl` method".to_string(),
                    );
                }
                if receiver.is_some() {
                    ctx.error(
                        "E-RES-DUPLICATE",
                        param.span,
                        "duplicate `self` parameter".to_string(),
                    );
                }
                receiver = Some(*rec);
                let self_ty = ctx.self_ty.clone().unwrap_or(Type::Error);
                let ty = match rec {
                    Receiver::Shared => Type::Ref {
                        mutable: false,
                        inner: Box::new(self_ty),
                    },
                    Receiver::Exclusive => Type::Ref {
                        mutable: true,
                        inner: Box::new(self_ty),
                    },
                    Receiver::ByValue => self_ty,
                };
                params.push(ParamSig {
                    name: "self".to_string(),
                    ty,
                    is_receiver: true,
                    mutable: false,
                });
            }
            ast::ParamKind::Binding { mutable, name, ty } => {
                if params.iter().any(|p| p.name == name.name) {
                    ctx.error(
                        "E-RES-DUPLICATE",
                        name.span,
                        format!("duplicate parameter `{}`", name.name),
                    );
                }
                let resolved = ctx.resolve_type(ty);
                // G-5.11 / TYP-1.6: `!` is not a parameter type.
                if contains_never(&resolved) {
                    ctx.error(
                        "E-TYP-MISMATCH",
                        ty.span,
                        format!(
                            "the never type (`!`) is not a valid parameter type in Core 1.0 \
                             (parameter `{}`, type `{resolved}`)",
                            name.name
                        ),
                    );
                }
                params.push(ParamSig {
                    name: name.name.clone(),
                    ty: resolved,
                    is_receiver: false,
                    mutable: *mutable,
                });
            }
        }
    }

    let mut ret = ctx.resolve_type(&f.ret);
    // `!` is legal only as the *whole* return type of a divergent function: a
    // compound such as `Vec<!>` or `Result<!, E>` is not inhabitable.
    if contains_never(&ret) && !matches!(ret, Type::Never) {
        ctx.error(
            "E-TYP-MISMATCH",
            f.ret.span,
            format!(
                "the never type (`!`) is only allowed as a whole return type; \
                 `{ret}` is not a valid return type"
            ),
        );
    }

    let declared_effects = f.effects.as_ref().map(|idents| {
        let mut set = EffectSet::default();
        for ident in idents {
            match Effect::from_name(&ident.name) {
                Some(effect) => {
                    if effect.is_unsupported() {
                        ctx.error(
                            "E-UNSUPPORTED-EFFECT",
                            ident.span,
                            format!("effect `{}` is not supported in Core 1.0", ident.name),
                        );
                    }
                    set.insert(effect);
                }
                None => {
                    ctx.error(
                        "E-EFF-UNKNOWN",
                        ident.span,
                        format!("unknown effect `{}`", ident.name),
                    );
                }
            }
        }
        set
    });

    let generics = match &impl_ctx {
        Some((_, impl_generics)) => impl_generics.clone(),
        None => ctx.generics.iter().map(|(n, _)| n.clone()).collect(),
    };
    let rust_path = facts.item_paths.get(&item.id).cloned().unwrap_or_else(|| {
        let module_path = world.modules[module].path.clone();
        join_path(&module_path, &rust_ident(&f.name.name))
    });
    if f.is_async {
        // An `async fn` call yields a future of its declared result (CONC-6.1).
        ret = crate::builtins::generic_opaque("Future", ret).unwrap_or(Type::Error);
        facts.async_fns.insert(item.id);
    }
    let sig = FnSig {
        item: item.id,
        rust_path,
        name: f.name.name.clone(),
        generics,
        params,
        ret,
        receiver,
        effects: EffectSet::default(),
        declared_effects,
        is_test: f.is_test,
        is_unsafe: false,
        is_extern: false,
    };
    facts.fn_sigs.insert(item.id, sig.clone());
    sig
}

/// Résout un bloc `extern "C" { ... }` : signatures, effets et contrôle des
/// types FFI. Les fonctions externes n'ont pas de corps Ravum ; le codegen
/// émet une déclaration `extern "C"` plus un wrapper sûr, et le checker exige
/// un bloc `unsafe` à chaque appel.
pub(crate) fn resolve_extern_block(
    world: &World,
    facts: &mut Facts,
    diags: &mut DiagnosticBag,
    module: ModuleId,
    block: &ast::ExternBlock,
) {
    for f in &block.fns {
        let (params, ret) = {
            let mut ctx = HeaderCtx::new(world, facts, diags, module, Vec::new(), None);
            let mut params: Vec<ParamSig> = Vec::with_capacity(f.params.len());
            for param in &f.params {
                match &param.kind {
                    ast::ParamKind::Receiver(_) => {
                        ctx.error(
                            "E-RES-SELF",
                            param.span,
                            "`self` is not allowed in an `extern` function declaration".to_string(),
                        );
                    }
                    // `mut` on an `extern` parameter is ignored on purpose: the
                    // declaration only fixes the ABI (see `ffi_safe` below) and
                    // an extern callee cannot mutate a caller binding, so the
                    // signature is always recorded immutable.
                    ast::ParamKind::Binding { name, ty, .. } => {
                        if params.iter().any(|p| p.name == name.name) {
                            ctx.error(
                                "E-RES-DUPLICATE",
                                name.span,
                                format!("duplicate parameter `{}`", name.name),
                            );
                        }
                        let resolved = ctx.resolve_type(ty);
                        if !ffi_safe_param(&resolved) {
                            ctx.error(
                                "E-FFI-TYPE",
                                ty.span,
                                format!(
                                    "type `{}` is not FFI-safe in Core 1.0; extern parameters and returns must be `i8`/`i16`/`i32`/`i64`, `u8`/`u16`/`u32`/`u64`, `isize`/`usize`, `f32`/`f64`, `bool`, `()` or `CHandle`; a parameter may also be `&str` and a return `Option<String>` (i128/u128 are not admitted)",
                                    resolved.display()
                                ),
                            );
                        }
                        params.push(ParamSig {
                            name: name.name.clone(),
                            ty: resolved,
                            is_receiver: false,
                            mutable: false,
                        });
                    }
                }
            }
            let ret = ctx.resolve_type(&f.ret);
            if !ffi_safe_return(&ret) {
                ctx.error(
                    "E-FFI-TYPE",
                    f.ret.span,
                    format!(
                        "type `{}` is not FFI-safe in Core 1.0; extern parameters and returns must be `i8`/`i16`/`i32`/`i64`, `u8`/`u16`/`u32`/`u64`, `isize`/`usize`, `f32`/`f64`, `bool`, `()` or `CHandle`; a parameter may also be `&str` and a return `Option<String>` (i128/u128 are not admitted)",
                        ret.display()
                    ),
                );
            }
            (params, ret)
        };
        // The wrapper is emitted in the module of the `extern` block (a
        // dependency's block lives in that dependency's module).
        let module_path = world.modules[module].path.clone();
        let rust_path = join_path(&module_path, &format!("rvm_ffi_wrap_{}", f.id));
        facts.item_paths.insert(f.id, rust_path.clone());
        facts.extern_symbols.insert(f.id, f.name.name.clone());
        let sig = FnSig {
            item: f.id,
            rust_path,
            name: f.name.name.clone(),
            generics: Vec::new(),
            params,
            ret,
            receiver: None,
            effects: EffectSet::new([Effect::Unsafe, Effect::Ffi]),
            declared_effects: None,
            is_test: false,
            is_unsafe: true,
            is_extern: true,
        };
        facts.fn_sigs.insert(f.id, sig);
    }
}

/// Types acceptés à la frontière FFI en Core 1.0. Volontairement restrictif :
/// pas de références, pas de `String`/`Vec`, pas de types nominaux, pas de
/// `char` (largeur C non portable).
///
/// `i128`/`u128` sont retirés de la liste (décision du responsable S4,
/// 2026-11-14 ; C-FFI-2) : aucune largeur C portable ne les couvre. Une
/// déclaration `extern` qui les utilise est refusée `E-FFI-TYPE` dès la
/// résolution, donc avant que le backend ne voie la signature. La liste jumelle
/// du backend (`emit.rs::ast_ffi_safe`) doit encore être alignée (agent E) ;
/// d'ici là, `ffi_safe` est la seule barrière et le test C-FFI-1 le vérifie.
/// A parameter of an `extern` function: a scalar, a `CHandle`, or `&str`,
/// passed to C as a pointer and a byte length (NAT-3.3).
pub(crate) fn ffi_safe_param(ty: &Type) -> bool {
    ffi_safe(ty)
        || is_chandle(ty)
        || matches!(ty, Type::Ref { mutable: false, inner } if **inner == Type::Str)
}

/// A return of an `extern` function: a scalar, a `CHandle`, or
/// `Option<String>` read from a NUL-terminated C string (NAT-3.3).
pub(crate) fn ffi_safe_return(ty: &Type) -> bool {
    ffi_safe(ty) || is_chandle(ty) || matches!(ty, Type::Option(inner) if **inner == Type::String)
}

fn is_chandle(ty: &Type) -> bool {
    matches!(ty, Type::Nominal { def, .. } if *def == builtins::PRELUDE_CHANDLE_DEF)
}

pub(crate) fn ffi_safe(ty: &Type) -> bool {
    matches!(
        ty,
        Type::I8
            | Type::I16
            | Type::I32
            | Type::I64
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64
            | Type::Isize
            | Type::Usize
            | Type::F32
            | Type::F64
            | Type::Bool
            | Type::Unit
    )
}

pub(crate) fn resolve_struct(
    world: &World,
    facts: &mut Facts,
    diags: &mut DiagnosticBag,
    module: ModuleId,
    item: &Item,
    s: &StructItem,
) {
    let mut ctx = HeaderCtx::new(world, facts, diags, module, Vec::new(), None);
    let generics = generic_scope(ctx.diags, &s.generics);
    ctx.generics = generics.clone();
    let mut fields: Vec<FieldInfo> = Vec::with_capacity(s.fields.len());
    for field in &s.fields {
        if fields.iter().any(|f| f.name == field.name.name) {
            ctx.error(
                "E-RES-DUPLICATE",
                field.name.span,
                format!("duplicate field `{}`", field.name.name),
            );
            continue;
        }
        let ty = ctx.resolve_type(&field.ty);
        // G-5.11 / TYP-1.6: `!` is not a field type.
        if contains_never(&ty) {
            ctx.error(
                "E-TYP-MISMATCH",
                field.ty.span,
                format!(
                    "the never type (`!`) is not a valid field type in Core 1.0 \
                     (field `{}` of `{}`, type `{ty}`)",
                    field.name.name, s.name.name
                ),
            );
        }
        fields.push(FieldInfo {
            name: field.name.name.clone(),
            rust_name: rust_ident(&field.name.name),
            ty,
            public: field.vis.is_public(),
        });
    }
    let facts = ctx.facts;
    let path = facts.item_paths.get(&item.id).cloned().unwrap_or_default();
    facts.structs.insert(
        item.id,
        StructInfo {
            item: item.id,
            rust_path: path,
            name: s.name.name.clone(),
            generics: generics.iter().map(|(n, _)| n.clone()).collect(),
            fields,
            is_copy: false,
            invariants: s
                .invariants
                .iter()
                .map(|i| (i.method.clone(), i.text.clone()))
                .collect(),
        },
    );
}

pub(crate) fn resolve_enum(
    world: &World,
    facts: &mut Facts,
    diags: &mut DiagnosticBag,
    module: ModuleId,
    item: &Item,
    e: &EnumItem,
) {
    let mut ctx = HeaderCtx::new(world, facts, diags, module, Vec::new(), None);
    let generics = generic_scope(ctx.diags, &e.generics);
    ctx.generics = generics.clone();
    let mut variants: Vec<VariantInfo> = Vec::with_capacity(e.variants.len());
    let mut seen: Vec<String> = Vec::new();
    for (index, variant) in e.variants.iter().enumerate() {
        if seen.contains(&variant.name.name) {
            ctx.error(
                "E-RES-DUPLICATE",
                variant.name.span,
                format!("duplicate variant `{}`", variant.name.name),
            );
            continue;
        }
        seen.push(variant.name.name.clone());
        let (kind, tuple_types, fields) = match &variant.fields {
            VariantDefFields::Unit => (VariantKind::Unit, Vec::new(), Vec::new()),
            VariantDefFields::Tuple(types) => {
                let mut out = Vec::with_capacity(types.len());
                for ty in types {
                    let resolved = ctx.resolve_type(ty);
                    if contains_never(&resolved) {
                        ctx.error(
                            "E-TYP-MISMATCH",
                            ty.span,
                            format!(
                                "the never type (`!`) is not a valid variant field type in Core 1.0 \
                                 (variant `{}`, type `{resolved}`)",
                                variant.name.name
                            ),
                        );
                    }
                    out.push(resolved);
                }
                (VariantKind::Tuple, out, Vec::new())
            }
            VariantDefFields::Struct(defs) => {
                let mut out: Vec<FieldInfo> = Vec::with_capacity(defs.len());
                for field in defs {
                    if out.iter().any(|f| f.name == field.name.name) {
                        ctx.error(
                            "E-RES-DUPLICATE",
                            field.name.span,
                            format!("duplicate field `{}`", field.name.name),
                        );
                        continue;
                    }
                    let ty = ctx.resolve_type(&field.ty);
                    if contains_never(&ty) {
                        ctx.error(
                            "E-TYP-MISMATCH",
                            field.ty.span,
                            format!(
                                "the never type (`!`) is not a valid variant field type in Core 1.0 \
                                 (variant `{}`, field `{}`, type `{ty}`)",
                                variant.name.name, field.name.name
                            ),
                        );
                    }
                    out.push(FieldInfo {
                        name: field.name.name.clone(),
                        rust_name: rust_ident(&field.name.name),
                        ty,
                        public: true,
                    });
                }
                (VariantKind::Struct, Vec::new(), out)
            }
        };
        variants.push(VariantInfo {
            index: variants.len().min(index),
            name: variant.name.name.clone(),
            rust_name: rust_ident(&variant.name.name),
            kind,
            tuple_types,
            fields,
        });
    }
    // Indices définitifs = ordre de déclaration effectif.
    for (i, variant) in variants.iter_mut().enumerate() {
        variant.index = i;
    }
    let facts = ctx.facts;
    let path = facts.item_paths.get(&item.id).cloned().unwrap_or_default();
    facts.enums.insert(
        item.id,
        EnumInfo {
            item: item.id,
            rust_path: path,
            name: e.name.name.clone(),
            generics: generics.iter().map(|(n, _)| n.clone()).collect(),
            variants,
            is_copy: false,
        },
    );
}

pub(crate) fn resolve_impl(
    world: &mut World,
    facts: &mut Facts,
    diags: &mut DiagnosticBag,
    module: ModuleId,
    item: &Item,
    imp: &ast::ImplItem,
) {
    let (target, generics, def) = {
        let mut ctx = HeaderCtx::new(world, facts, diags, module, Vec::new(), None);
        let generics = generic_scope(ctx.diags, &imp.generics);
        ctx.generics = generics.clone();
        let target = ctx.resolve_type(&imp.self_ty);
        let (def, args) = match &target {
            Type::Nominal { def, args, .. } if ctx.world.nominal_kind(*def).is_some() => {
                (*def, args.clone())
            }
            _ => {
                ctx.error(
                    "E-RES-IMPL-SELF-TYPE",
                    imp.self_ty.span,
                    "`impl` target must be a nominal type defined in this package".to_string(),
                );
                return;
            }
        };
        let names: Vec<&str> = generics.iter().map(|(n, _)| n.as_str()).collect();
        let ok = args.len() == names.len()
            && args.iter().enumerate().all(|(i, arg)| {
                matches!(arg, Type::Param { name, index } if *index == i && name == names[i])
            });
        if !ok {
            ctx.error(
                "E-RES-IMPL-TARGET",
                imp.self_ty.span,
                "generic parameters of `impl` must match the target type arguments in order"
                    .to_string(),
            );
        }
        (target, generics, def)
    };
    let impl_generics: Vec<String> = generics.iter().map(|(n, _)| n.clone()).collect();
    world.impls.entry(def).or_default();
    for member in imp.non_methods() {
        diags.error(
            "E-RES-IMPL-UNKNOWN",
            member.span,
            "only fn items are allowed inside an `impl` in Core 1.0".to_string(),
        );
    }
    for (member, f) in imp.methods() {
        let sig = resolve_fn_header(
            world,
            facts,
            diags,
            module,
            member,
            f,
            Some((target.clone(), impl_generics.clone())),
        );
        let type_path = facts.item_paths.get(&def).cloned().unwrap_or_default();
        let method_path = format!("{}::{}", type_path, rust_ident(&f.name.name));
        facts.item_paths.insert(member.id, method_path.clone());
        if let Some(sig_mut) = facts.fn_sigs.get_mut(&member.id) {
            sig_mut.rust_path = method_path;
        }
        let duplicate = world
            .impls
            .get(&def)
            .map(|methods| methods.iter().any(|m| m.name == sig.name))
            .unwrap_or(false);
        if duplicate {
            diags.push(
                Diagnostic::error(
                    "E-RES-IMPL-DUPLICATE",
                    member.span,
                    format!("duplicate method `{}` for this type", sig.name),
                )
                .with_note("a type can have only one method with a given name", None),
            );
            continue;
        }
        world.impls.get_mut(&def).unwrap().push(MethodEntry {
            item: member.id,
            name: sig.name.clone(),
            def_module: module,
            vis: member.vis,
            receiver: sig.receiver,
            self_ty: target.clone(),
        });
        let _ = item;
    }
}

// ---------------------------------------------------------------------------
// Résolution de types (partagée avec `typeck`)
// ---------------------------------------------------------------------------

/// TYP-1.7: a written type deeper than [`MAX_TYPE_DEPTH`] is refused with
/// `E-TYP-DEPTH`, on the whole written type. Called on every type written in
/// a declaration or a body, once per written type (not per nested node). The
/// refused type becomes [`Type::Error`], so nothing checked against it reports
/// a second, derived diagnostic.
pub(crate) fn check_written_type_depth(
    diags: &mut DiagnosticBag,
    ty: &AstType,
    resolved: Type,
) -> Type {
    if resolved.depth() > MAX_TYPE_DEPTH {
        diags.error(
            "E-TYP-DEPTH",
            ty.span,
            format!(
                "this type nests more than {MAX_TYPE_DEPTH} levels; \
                 split it with a named struct"
            ),
        );
        return Type::Error;
    }
    resolved
}

/// Résout une syntaxe de type en [`Type`], en enregistrant **chaque** nœud de
/// type dans `type_nodes` et les nœuds d'expression de longueur de tableau dans
/// `expr_types`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_type_syntax(
    world: &World,
    type_nodes: &mut BTreeMap<NodeId, Type>,
    expr_types: &mut BTreeMap<NodeId, Type>,
    diags: &mut DiagnosticBag,
    module: ModuleId,
    generics: &[(String, usize)],
    self_ty: Option<&Type>,
    ty: &AstType,
) -> Type {
    let resolved = match &ty.kind {
        TypeKind::Unit => Type::Unit,
        TypeKind::Never => Type::Never,
        TypeKind::Infer => {
            diags.error(
                "E-TYP-AMBIGUOUS",
                ty.span,
                "`_` is not allowed in a type position in Core 1.0".to_string(),
            );
            Type::Error
        }
        TypeKind::SelfType => match self_ty {
            Some(t) => t.clone(),
            None => {
                diags.error(
                    "E-RES-SELF-OUTSIDE-IMPL",
                    ty.span,
                    "`Self` is only valid inside an `impl`".to_string(),
                );
                Type::Error
            }
        },
        TypeKind::Ref { mutable, inner } => {
            if is_str_syntax(inner) {
                if *mutable {
                    diags.error(
                        "E-RES-UNKNOWN",
                        inner.span,
                        "`&mut str` does not exist in Core 1.0".to_string(),
                    );
                    Type::Error
                } else {
                    type_nodes.insert(inner.id, Type::Str);
                    Type::Ref {
                        mutable: false,
                        inner: Box::new(Type::Str),
                    }
                }
            } else {
                let inner_ty = resolve_type_syntax(
                    world, type_nodes, expr_types, diags, module, generics, self_ty, inner,
                );
                Type::Ref {
                    mutable: *mutable,
                    inner: Box::new(inner_ty),
                }
            }
        }
        TypeKind::Tuple(items) => Type::Tuple(
            items
                .iter()
                .map(|item| {
                    resolve_type_syntax(
                        world, type_nodes, expr_types, diags, module, generics, self_ty, item,
                    )
                })
                .collect(),
        ),
        TypeKind::Array { elem, len } => {
            let elem_ty = resolve_type_syntax(
                world, type_nodes, expr_types, diags, module, generics, self_ty, elem,
            );
            let length = array_length(len, expr_types, diags);
            Type::Array {
                elem: Box::new(elem_ty),
                len: length.unwrap_or(1),
            }
        }
        TypeKind::Slice(inner) => Type::Slice(Box::new(resolve_type_syntax(
            world, type_nodes, expr_types, diags, module, generics, self_ty, inner,
        ))),
        TypeKind::Path(path) => resolve_type_path(
            world, type_nodes, expr_types, diags, module, generics, self_ty, path,
        ),
    };
    type_nodes.insert(ty.id, resolved.clone());
    resolved
}

pub(crate) fn is_str_syntax(ty: &AstType) -> bool {
    match &ty.kind {
        TypeKind::Path(path) => {
            path.root == PathRoot::Plain
                && path.segments.len() == 1
                && path.segments[0].name.name == "str"
        }
        _ => false,
    }
}

pub(crate) fn array_length(
    len: &Expr,
    expr_types: &mut BTreeMap<NodeId, Type>,
    diags: &mut DiagnosticBag,
) -> Option<usize> {
    expr_types.insert(len.id, Type::Usize);
    match &len.kind {
        ast::ExprKind::Int {
            value,
            suffix: None,
        } => {
            if *value == 0 {
                diags.error(
                    "E-TYP-ARRAY-REPEAT",
                    len.span,
                    "array length must be greater than zero".to_string(),
                );
                None
            } else if *value > usize::MAX as u128 {
                diags.error(
                    "E-TYP-ARRAY-REPEAT",
                    len.span,
                    "array length does not fit in `usize`".to_string(),
                );
                None
            } else {
                Some(*value as usize)
            }
        }
        _ => {
            diags.error(
                "E-TYP-ARRAY-REPEAT",
                len.span,
                "array length must be a positive decimal integer literal".to_string(),
            );
            None
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_type_args(
    world: &World,
    type_nodes: &mut BTreeMap<NodeId, Type>,
    expr_types: &mut BTreeMap<NodeId, Type>,
    diags: &mut DiagnosticBag,
    module: ModuleId,
    generics: &[(String, usize)],
    self_ty: Option<&Type>,
    args: &[AstType],
) -> Vec<Type> {
    args.iter()
        .map(|arg| {
            resolve_type_syntax(
                world, type_nodes, expr_types, diags, module, generics, self_ty, arg,
            )
        })
        .collect()
}

/// `str` et `str` mut n'existent pas comme types autonomes.
pub(crate) fn report_str(segment: &ast::PathSegment, diags: &mut DiagnosticBag) -> Type {
    diags.error(
        "E-RES-UNKNOWN",
        segment.name.span,
        "`str` is not a type in Core 1.0; use `&str`".to_string(),
    );
    Type::Error
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_type_path(
    world: &World,
    type_nodes: &mut BTreeMap<NodeId, Type>,
    expr_types: &mut BTreeMap<NodeId, Type>,
    diags: &mut DiagnosticBag,
    module: ModuleId,
    generics: &[(String, usize)],
    self_ty: Option<&Type>,
    path: &Path,
) -> Type {
    let segments = &path.segments;
    if segments.is_empty() {
        diags.error("E-RES-UNKNOWN", path.span, "empty type path".to_string());
        return Type::Error;
    }

    // `Self` is the keyword `Kw::SelfType`: the parser builds `TypeKind::SelfType`
    // for it, never a path segment, so a type path cannot start with it.

    // Segment unique non qualifié : paramètre générique, `str`, type local,
    // type prélude.
    if path.root == PathRoot::Plain && segments.len() == 1 {
        let segment = &segments[0];
        let name = &segment.name.name;
        if let Some((n, index)) = generics.iter().find(|(n, _)| n == name) {
            if !segment.args.is_empty() {
                diags.error(
                    "E-TYP-ARITY",
                    path.span,
                    format!("generic parameter `{name}` takes no arguments"),
                );
            }
            return Type::Param {
                name: n.clone(),
                index: *index,
            };
        }
        if name == "str" {
            return report_str(segment, diags);
        }
        if let Some(sym) = world.lookup(module, Ns::Type, name) {
            world.mark_used(module, sym);
            // A name looked up in the current module is always visible from it:
            // `lookup` answers `def_module == module`, which `visible_from` accepts.
            let args = resolve_type_args(
                world,
                type_nodes,
                expr_types,
                diags,
                module,
                generics,
                self_ty,
                &segment.args,
            );
            return nominal_type(world, sym.sym, &args, segment.name.span, diags)
                .unwrap_or(Type::Error);
        }
        if is_builtin_type_name(name) {
            let args = resolve_type_args(
                world,
                type_nodes,
                expr_types,
                diags,
                module,
                generics,
                self_ty,
                &segment.args,
            );
            return prelude_nominal(name, &args, segment.name.span, diags).unwrap_or(Type::Error);
        }
        if world.lookup(module, Ns::Module, name).is_some()
            || builtins::builtin_root_module(name).is_some()
        {
            diags.error(
                "E-RES-UNKNOWN",
                segment.name.span,
                format!("`{name}` is a module, not a type"),
            );
            return Type::Error;
        }
        diags.error(
            "E-RES-UNKNOWN",
            segment.name.span,
            format!("unknown type `{name}`"),
        );
        return Type::Error;
    }

    // Point de départ : module (les chemins de type ne traversent pas de type).
    enum Cursor {
        Module(ModuleId),
        Builtin(&'static str),
    }
    let mut index = 0usize;
    let mut cursor = match path.root {
        PathRoot::Crate => Cursor::Module(world.root),
        PathRoot::SelfMod => Cursor::Module(module),
        PathRoot::Super => match world.ancestor(module, 1) {
            Some(parent) => Cursor::Module(parent),
            None => {
                diags.error(
                    "E-RES-UNKNOWN",
                    path.span,
                    "`super` used at the crate root".to_string(),
                );
                return Type::Error;
            }
        },
        PathRoot::Plain => {
            let first = &segments[0].name.name;
            if let Some(sym) = world.lookup(module, Ns::Module, first) {
                world.mark_used(module, sym);
                index = 1;
                match sym.sym {
                    Sym::Module(m) => Cursor::Module(m),
                    Sym::BuiltinModule(p) => Cursor::Builtin(p),
                    _ => return Type::Error,
                }
            } else if first == "std" {
                index = 1;
                Cursor::Builtin("std")
            } else if builtins::builtin_root_module(first).is_some() {
                // STD-1.1: no module is in the prelude.
                diags.error(
                    "E-RES-UNKNOWN",
                    segments[0].name.span,
                    format!("module `{first}` is not in scope (add `use std::{first};`)"),
                );
                return Type::Error;
            } else if world.lookup(module, Ns::Type, first).is_some() || is_builtin_type_name(first)
            {
                diags.error(
                    "E-RES-UNKNOWN",
                    segments[0].name.span,
                    format!("`{first}` is a type, not a module"),
                );
                return Type::Error;
            } else {
                diags.error(
                    "E-RES-UNKNOWN",
                    segments[0].name.span,
                    format!("unknown type `{first}`"),
                );
                return Type::Error;
            }
        }
    };

    loop {
        // Entered with `index < len` (an empty path is refused above, and a
        // plain path consumes only its first segment before the loop when it
        // has at least two); each turn advances or returns on the last segment.
        let last = index + 1 == segments.len();
        let segment = &segments[index];
        let name = &segment.name.name;
        match cursor {
            Cursor::Module(m) => {
                if !last {
                    match world.lookup(m, Ns::Module, name) {
                        Some(sym) => {
                            world.mark_used(m, sym);
                            if !world.visible_from(&sym, module) {
                                diags.error(
                                    "E-RES-PRIVATE",
                                    segment.name.span,
                                    format!("module `{name}` is private"),
                                );
                                return Type::Error;
                            }
                            cursor = match sym.sym {
                                Sym::Module(child) => Cursor::Module(child),
                                Sym::BuiltinModule(p) => Cursor::Builtin(p),
                                _ => {
                                    let message = format!("`{name}` is not a module");
                                    diags.error("E-RES-UNKNOWN", segment.name.span, message);
                                    return Type::Error;
                                }
                            };
                            index += 1;
                        }
                        None => {
                            diags.error(
                                "E-RES-UNKNOWN",
                                segment.name.span,
                                format!("unknown module `{name}`"),
                            );
                            return Type::Error;
                        }
                    }
                } else if let Some(sym) = world.lookup(m, Ns::Type, name) {
                    world.mark_used(m, sym);
                    if !world.visible_from(&sym, module) {
                        diags.error(
                            "E-RES-PRIVATE",
                            segment.name.span,
                            format!("type `{name}` is private"),
                        );
                        return Type::Error;
                    }
                    let args = resolve_type_args(
                        world,
                        type_nodes,
                        expr_types,
                        diags,
                        module,
                        generics,
                        self_ty,
                        &segment.args,
                    );
                    return nominal_type(world, sym.sym, &args, segment.name.span, diags)
                        .unwrap_or(Type::Error);
                } else if let Some(sym) = world.lookup(m, Ns::Module, name) {
                    world.mark_used(m, sym);
                    diags.error(
                        "E-RES-UNKNOWN",
                        segment.name.span,
                        format!("`{name}` is a module, not a type"),
                    );
                    return Type::Error;
                } else {
                    diags.error(
                        "E-RES-UNKNOWN",
                        segment.name.span,
                        format!("unknown type `{name}`"),
                    );
                    return Type::Error;
                }
            }
            Cursor::Builtin(p) => {
                diags.error(
                    "E-RES-UNKNOWN",
                    segment.name.span,
                    format!("`{p}::{name}` is not a type"),
                );
                return Type::Error;
            }
        }
    }
}

pub(crate) fn is_builtin_type_name(name: &str) -> bool {
    matches!(name, "Vec" | "Option" | "Result")
        || builtins::prelude_type(name).is_some()
        || builtins::PRELUDE_GENERIC_OPAQUES
            .iter()
            .any(|(opaque, _)| *opaque == name)
}

pub(crate) fn prelude_nominal(
    name: &str,
    args: &[Type],
    span: Span,
    diags: &mut DiagnosticBag,
) -> Option<Type> {
    match name {
        "Vec" => {
            expect_arity(name, 1, args.len(), span, diags)?;
            Some(Type::Vec(Box::new(args[0].clone())))
        }
        "Option" => {
            expect_arity(name, 1, args.len(), span, diags)?;
            Some(Type::Option(Box::new(args[0].clone())))
        }
        "Result" => {
            expect_arity(name, 2, args.len(), span, diags)?;
            Some(Type::Result(
                Box::new(args[0].clone()),
                Box::new(args[1].clone()),
            ))
        }
        _ if builtins::PRELUDE_GENERIC_OPAQUES
            .iter()
            .any(|(opaque, _)| *opaque == name) =>
        {
            expect_arity(name, 1, args.len(), span, diags)?;
            builtins::generic_opaque(name, args[0].clone())
        }
        _ => {
            if !args.is_empty() {
                diags.error(
                    "E-TYP-ARITY",
                    span,
                    format!("type `{name}` takes no generic arguments"),
                );
                return None;
            }
            builtins::prelude_type(name)
        }
    }
}

pub(crate) fn nominal_type(
    world: &World,
    sym: Sym,
    args: &[Type],
    span: Span,
    diags: &mut DiagnosticBag,
) -> Option<Type> {
    let def = match sym {
        Sym::Struct(id) | Sym::Enum(id) => id,
        _ => {
            diags.error("E-RES-UNKNOWN", span, "expected a nominal type".to_string());
            return None;
        }
    };
    let generics = world.type_generics(def);
    let expected = generics.len();
    if args.len() != expected {
        diags.error(
            "E-TYP-ARITY",
            span,
            format!(
                "type expects {expected} generic argument(s), found {}",
                args.len()
            ),
        );
        return None;
    }
    let name = world
        .structs
        .get(&def)
        .map(|s| s.name.name.clone())
        .or_else(|| world.enums.get(&def).map(|e| e.name.name.clone()))
        .unwrap_or_default();
    Some(Type::Nominal {
        def,
        name,
        args: args.to_vec(),
    })
}

pub(crate) fn expect_arity(
    name: &str,
    expected: usize,
    found: usize,
    span: Span,
    diags: &mut DiagnosticBag,
) -> Option<()> {
    if expected == found {
        Some(())
    } else {
        diags.error(
            "E-TYP-ARITY",
            span,
            format!("type `{name}` expects {expected} generic argument(s), found {found}"),
        );
        None
    }
}

// ---------------------------------------------------------------------------
// Copy structurel et récursion de taille
// ---------------------------------------------------------------------------

pub(crate) fn compute_copy_and_recursive_types(
    world: &World,
    facts: &mut Facts,
    diags: &mut DiagnosticBag,
) {
    let defs: Vec<NodeId> = facts
        .structs
        .keys()
        .chain(facts.enums.keys())
        .copied()
        .collect();
    let mut state: BTreeMap<NodeId, CopyState> = BTreeMap::new();
    let mut reported: BTreeSet<NodeId> = BTreeSet::new();
    let mut flags: BTreeMap<NodeId, bool> = BTreeMap::new();
    for def in defs {
        let mut stack = Vec::new();
        let is_copy = nominal_is_copy(
            def,
            world,
            facts,
            &mut state,
            &mut stack,
            &mut reported,
            diags,
        );
        flags.insert(def, is_copy);
    }
    for (def, is_copy) in flags {
        if let Some(info) = facts.structs.get_mut(&def) {
            info.is_copy = is_copy;
        }
        if let Some(info) = facts.enums.get_mut(&def) {
            info.is_copy = is_copy;
        }
    }
}
