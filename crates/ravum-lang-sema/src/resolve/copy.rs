use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CopyState {
    InProgress,
    Done(bool),
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn nominal_is_copy(
    def: NodeId,
    world: &World,
    facts: &Facts,
    state: &mut BTreeMap<NodeId, CopyState>,
    stack: &mut Vec<NodeId>,
    reported: &mut BTreeSet<NodeId>,
    diags: &mut DiagnosticBag,
) -> bool {
    match state.get(&def) {
        Some(CopyState::Done(v)) => return *v,
        Some(CopyState::InProgress) => {
            if reported.insert(def) {
                let span = world
                    .structs
                    .get(&def)
                    .map(|s| s.span)
                    .or_else(|| world.enums.get(&def).map(|e| e.span))
                    .unwrap_or(Span::dummy());
                let name = world
                    .structs
                    .get(&def)
                    .map(|s| s.name.name.clone())
                    .or_else(|| world.enums.get(&def).map(|e| e.name.name.clone()))
                    .unwrap_or_default();
                diags.error(
                    "E-TYP-RECURSIVE",
                    span,
                    format!("type `{name}` recurses without indirection (no `Box` in Core 1.0)"),
                );
            }
            return false;
        }
        None => {}
    }
    state.insert(def, CopyState::InProgress);
    stack.push(def);
    let mut copy = true;
    let fields: Vec<Type> = if let Some(info) = facts.structs.get(&def) {
        info.fields.iter().map(|f| f.ty.clone()).collect()
    } else if let Some(info) = facts.enums.get(&def) {
        let mut out = Vec::new();
        for variant in &info.variants {
            out.extend(variant.tuple_types.iter().cloned());
            out.extend(variant.fields.iter().map(|f| f.ty.clone()));
        }
        out
    } else {
        Vec::new()
    };
    if fields.is_empty() && world.nominal_kind(def).is_none() {
        copy = false;
    }
    for field in &fields {
        if !type_is_copy(field, world, facts, state, stack, reported, diags) {
            copy = false;
        }
    }
    stack.pop();
    state.insert(def, CopyState::Done(copy));
    copy
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn type_is_copy(
    ty: &Type,
    world: &World,
    facts: &Facts,
    state: &mut BTreeMap<NodeId, CopyState>,
    stack: &mut Vec<NodeId>,
    reported: &mut BTreeSet<NodeId>,
    diags: &mut DiagnosticBag,
) -> bool {
    match ty {
        Type::I8
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
        | Type::Usize
        | Type::F32
        | Type::F64
        | Type::Bool
        | Type::Char
        | Type::Unit
        | Type::Never
        | Type::Ref { .. } => true,
        Type::Str | Type::String | Type::Vec(_) | Type::Slice(_) => false,
        Type::Array { elem, .. } => type_is_copy(elem, world, facts, state, stack, reported, diags),
        Type::Tuple(items) => items
            .iter()
            .all(|item| type_is_copy(item, world, facts, state, stack, reported, diags)),
        Type::Option(inner) => type_is_copy(inner, world, facts, state, stack, reported, diags),
        Type::Result(ok, err) => {
            type_is_copy(ok, world, facts, state, stack, reported, diags)
                && type_is_copy(err, world, facts, state, stack, reported, diags)
        }
        Type::Nominal { def, .. } => {
            if *def == builtins::PRELUDE_ERROR_DEF || world.nominal_kind(*def).is_none() {
                return false;
            }
            nominal_is_copy(*def, world, facts, state, stack, reported, diags)
        }
        Type::Param { .. } | Type::Error => false,
    }
}

// ---------------------------------------------------------------------------
// Imports inutilisés
// ---------------------------------------------------------------------------

/// Émet `W-RES-UNUSED-IMPORT` pour chaque import jamais utilisé.
pub(crate) fn emit_unused_import_warnings(world: &World, diags: &mut DiagnosticBag) {
    for module in &world.modules {
        for import in &module.imports {
            if !import.used.get() {
                diags.warning(
                    "W-RES-UNUSED-IMPORT",
                    import.span,
                    format!("unused import `{}`", import.display),
                );
            }
        }
    }
}
