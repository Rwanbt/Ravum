//! Reloadable-program contract (RFC-E11-HOT-RELOAD, `spec/language/reload.md`).
//!
//! A program built with `ravum-lang watch --reload` declares, in its root
//! module, a type `State`, `fn init() -> State` and
//! `fn step(state: State, line: String) -> State`, and optionally
//! `fn migrate(old: OldType) -> State`. This module checks that contract on an
//! already checked program and computes the **state schema** (HR-1.1): a
//! canonical text of the developed `State` type and its SHA-256 digest.
//!
//! The schema is *structural*: user types are numbered in order of first
//! appearance (`#0` is the root), so `migrate` can take a type that repeats
//! the previous `State` under another name. Field names, variant names, their
//! order and every component type are part of it.

use crate::builtins::prelude_opaque_name;
use crate::facts::{CheckedProgram, VariantKind};
use crate::loader::sha256_hex;
use crate::types::Type;
use ravum_lang_syntax::ast::{ItemKind, NodeId};
use ravum_lang_syntax::diag::{Diagnostic, DiagnosticBag};
use ravum_lang_syntax::span::Span;
use std::collections::BTreeMap;

/// Codes of this contract (registered in `spec/language/diagnostics.md`).
pub const E_HR_ENTRY: &str = "E-HR-ENTRY";
pub const E_HR_STATE_TYPE: &str = "E-HR-STATE-TYPE";
pub const E_HR_STATE_RESOURCE: &str = "E-HR-STATE-RESOURCE";

/// Schema of one state type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateSchema {
    /// Item id of the root type.
    pub def: NodeId,
    /// Canonical text (HR-1.1).
    pub text: String,
    /// Lowercase hexadecimal SHA-256 of `text`.
    pub digest: String,
    /// User types reachable from the root, root first, in schema order.
    pub types: Vec<NodeId>,
}

/// The checked contract of a reloadable program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadContract {
    pub state: StateSchema,
    pub init: NodeId,
    pub step: NodeId,
    /// `migrate` and the schema of its parameter type.
    pub migrate: Option<(NodeId, StateSchema)>,
}

impl ReloadContract {
    /// Every user type that needs a canonical (de)serializer, without repeats.
    pub fn serialized_types(&self) -> Vec<NodeId> {
        let mut out = self.state.types.clone();
        if let Some((_, old)) = &self.migrate {
            for def in &old.types {
                if !out.contains(def) {
                    out.push(*def);
                }
            }
        }
        out
    }
}

/// Check the reload contract of `checked`; diagnostics on failure.
pub fn reload_contract(checked: &CheckedProgram) -> Result<ReloadContract, Vec<Diagnostic>> {
    let root = &checked.program.root;
    let mut fns: BTreeMap<&str, (NodeId, Span)> = BTreeMap::new();
    let mut types: BTreeMap<&str, (NodeId, Span)> = BTreeMap::new();
    for item in &root.items {
        match &item.kind {
            ItemKind::Fn(f) => {
                fns.insert(f.name.name.as_str(), (item.id, item.span));
            }
            ItemKind::Struct(s) => {
                types.insert(s.name.name.as_str(), (item.id, item.span));
            }
            ItemKind::Enum(e) => {
                types.insert(e.name.name.as_str(), (item.id, item.span));
            }
            _ => {}
        }
    }
    let whole = root.span;
    let mut diags = Vec::new();

    let Some(&(state_def, state_span)) = types.get("State") else {
        return Err(vec![Diagnostic::error(
            E_HR_ENTRY,
            whole,
            "a reloadable program declares a struct or enum `State` in its root module",
        )]);
    };
    let is_state = |ty: &Type| matches!(ty, Type::Nominal { def, args, .. } if *def == state_def && args.is_empty());

    let state = match schema(checked, state_def, state_span) {
        Ok(s) => Some(s),
        Err(d) => {
            diags.extend(d);
            None
        }
    };

    let sig_of = |name: &str| {
        fns.get(name)
            .map(|(id, span)| (*id, *span, checked.facts.fn_sigs.get(id)))
    };

    let init = match sig_of("init") {
        Some((id, span, Some(sig))) => {
            if sig.params.is_empty() && sig.generics.is_empty() && is_state(&sig.ret) {
                Some(id)
            } else {
                diags.push(Diagnostic::error(
                    E_HR_ENTRY,
                    span,
                    "`init` must be `fn init() -> State`",
                ));
                None
            }
        }
        _ => {
            diags.push(Diagnostic::error(
                E_HR_ENTRY,
                whole,
                "a reloadable program declares `fn init() -> State`",
            ));
            None
        }
    };

    let step = match sig_of("step") {
        Some((id, span, Some(sig))) => {
            let ok = sig.generics.is_empty()
                && sig.params.len() == 2
                && is_state(&sig.params[0].ty)
                && sig.params[1].ty == Type::String
                && is_state(&sig.ret);
            if ok {
                Some(id)
            } else {
                diags.push(Diagnostic::error(
                    E_HR_ENTRY,
                    span,
                    "`step` must be `fn step(state: State, line: String) -> State`",
                ));
                None
            }
        }
        _ => {
            diags.push(Diagnostic::error(
                E_HR_ENTRY,
                whole,
                "a reloadable program declares `fn step(state: State, line: String) -> State`",
            ));
            None
        }
    };

    let migrate = match sig_of("migrate") {
        None => None,
        Some((id, span, sig)) => {
            let old = sig.and_then(|sig| {
                if !sig.generics.is_empty() || sig.params.len() != 1 || !is_state(&sig.ret) {
                    return None;
                }
                match &sig.params[0].ty {
                    Type::Nominal { def, args, .. } if args.is_empty() && *def != state_def => {
                        Some(*def)
                    }
                    _ => None,
                }
            });
            match old {
                None => {
                    diags.push(Diagnostic::error(
                        E_HR_ENTRY,
                        span,
                        "`migrate` must be `fn migrate(old: OldState) -> State`, where `OldState` is another user type",
                    ));
                    None
                }
                Some(old_def) => match schema(checked, old_def, span) {
                    Ok(s) => Some((id, s)),
                    Err(d) => {
                        diags.extend(d);
                        None
                    }
                },
            }
        }
    };

    match (state, init, step) {
        (Some(state), Some(init), Some(step)) if diags.is_empty() => Ok(ReloadContract {
            state,
            init,
            step,
            migrate,
        }),
        _ => Err(diags),
    }
}

/// Canonical schema of the user type `root` (HR-1.1, HR-1.6).
pub fn schema(
    checked: &CheckedProgram,
    root: NodeId,
    span: Span,
) -> Result<StateSchema, Vec<Diagnostic>> {
    let mut order: Vec<NodeId> = vec![root];
    let mut diags = Vec::new();
    let mut lines = Vec::new();
    let mut next = 0;
    while next < order.len() {
        let def = order[next];
        next += 1;
        let mut line = format!("#{}=", next - 1);
        if let Some(info) = checked.facts.structs.get(&def) {
            if !info.generics.is_empty() {
                diags.push(generic_error(span, &info.name));
                continue;
            }
            line.push_str("struct{");
            for field in &info.fields {
                line.push_str(&field.name);
                line.push(':');
                type_text(checked, &field.ty, &mut order, &mut line, span, &mut diags);
                line.push(';');
            }
            line.push('}');
        } else if let Some(info) = checked.facts.enums.get(&def) {
            if !info.generics.is_empty() {
                diags.push(generic_error(span, &info.name));
                continue;
            }
            line.push_str("enum{");
            for variant in &info.variants {
                line.push_str(&variant.name);
                match variant.kind {
                    VariantKind::Unit => {}
                    VariantKind::Tuple => {
                        line.push('(');
                        for ty in &variant.tuple_types {
                            type_text(checked, ty, &mut order, &mut line, span, &mut diags);
                            line.push(';');
                        }
                        line.push(')');
                    }
                    VariantKind::Struct => {
                        line.push('{');
                        for field in &variant.fields {
                            line.push_str(&field.name);
                            line.push(':');
                            type_text(checked, &field.ty, &mut order, &mut line, span, &mut diags);
                            line.push(';');
                        }
                        line.push('}');
                    }
                }
                line.push(';');
            }
            line.push('}');
        } else {
            diags.push(Diagnostic::error(
                E_HR_STATE_TYPE,
                span,
                "the state type must be a user struct or enum",
            ));
            continue;
        }
        lines.push(line);
    }
    if !diags.is_empty() {
        return Err(diags);
    }
    let mut text = lines.join("\n");
    text.push('\n');
    Ok(StateSchema {
        def: root,
        digest: sha256_hex(text.as_bytes()),
        text,
        types: order,
    })
}

fn generic_error(span: Span, name: &str) -> Diagnostic {
    Diagnostic::error(
        E_HR_STATE_TYPE,
        span,
        format!("the state may not contain the generic type `{name}`"),
    )
}

fn type_text(
    checked: &CheckedProgram,
    ty: &Type,
    order: &mut Vec<NodeId>,
    out: &mut String,
    span: Span,
    diags: &mut Vec<Diagnostic>,
) {
    let prim = match ty {
        Type::I8 => "i8",
        Type::I16 => "i16",
        Type::I32 => "i32",
        Type::I64 => "i64",
        Type::I128 => "i128",
        Type::U8 => "u8",
        Type::U16 => "u16",
        Type::U32 => "u32",
        Type::U64 => "u64",
        Type::U128 => "u128",
        Type::Isize => "isize",
        Type::Usize => "usize",
        Type::F32 => "f32",
        Type::F64 => "f64",
        Type::Bool => "bool",
        Type::Char => "char",
        Type::Unit => "()",
        Type::String => "String",
        _ => "",
    };
    if !prim.is_empty() {
        out.push_str(prim);
        return;
    }
    match ty {
        Type::Vec(inner) => {
            out.push_str("Vec<");
            type_text(checked, inner, order, out, span, diags);
            out.push('>');
        }
        Type::Option(inner) => {
            out.push_str("Option<");
            type_text(checked, inner, order, out, span, diags);
            out.push('>');
        }
        Type::Result(ok, err) => {
            out.push_str("Result<");
            type_text(checked, ok, order, out, span, diags);
            out.push(',');
            type_text(checked, err, order, out, span, diags);
            out.push('>');
        }
        Type::Array { elem, len } => {
            out.push('[');
            type_text(checked, elem, order, out, span, diags);
            out.push_str(&format!(";{len}]"));
        }
        Type::Tuple(items) => {
            out.push('(');
            for item in items {
                type_text(checked, item, order, out, span, diags);
                out.push(';');
            }
            out.push(')');
        }
        Type::Nominal { def, name, args } => {
            if let Some(opaque) = prelude_opaque_name(*def) {
                diags.push(Diagnostic::error(
                    E_HR_STATE_RESOURCE,
                    span,
                    format!("the state may not hold the resource `{opaque}`: it does not survive a reload"),
                ));
                return;
            }
            let known =
                checked.facts.structs.contains_key(def) || checked.facts.enums.contains_key(def);
            if !known || !args.is_empty() {
                diags.push(Diagnostic::error(
                    E_HR_STATE_TYPE,
                    span,
                    format!("the state may not contain the type `{name}`"),
                ));
                return;
            }
            let index = match order.iter().position(|d| d == def) {
                Some(i) => i,
                None => {
                    order.push(*def);
                    order.len() - 1
                }
            };
            out.push_str(&format!("#{index}"));
        }
        Type::Ref { .. } | Type::Str | Type::Slice(_) => {
            diags.push(Diagnostic::error(
                E_HR_STATE_RESOURCE,
                span,
                "the state may not hold a reference: it must be transferable (CONC-2.1)",
            ));
        }
        _ => diags.push(Diagnostic::error(
            E_HR_STATE_TYPE,
            span,
            "the state contains a type that has no canonical form",
        )),
    }
}

/// Codes of the DSP block contract (RFC-E11-DSP-BLOCK, `reload.md` §5).
pub const E_DSP_ENTRY: &str = "E-DSP-ENTRY";
pub const E_DSP_STATE_TYPE: &str = "E-DSP-STATE-TYPE";

/// Name of the per-sample entry of a DSP block.
pub const DSP_ENTRY: &str = "process_sample";

/// Schema text of the host-owned DSP state (`ravum_protocol::DSP_STATE_SCHEMA_V1_TEXT`,
/// repeated here because the checker depends on no host crate; a CLI test
/// asserts that both texts are equal).
pub const DSP_STATE_SCHEMA_V1_TEXT: &str = "#0=struct{z1:f32;z2:f32;}\n";

/// The checked contract of a DSP block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DspContract {
    pub state: StateSchema,
    pub process: NodeId,
    /// An initializer selects ABI 3. Legacy ABI 2 takes host-zeroed state.
    pub init: Option<NodeId>,
    pub migrate: Option<(NodeId, StateSchema)>,
}

fn opaque_dsp_field(ty: &Type) -> bool {
    match ty {
        Type::F32 | Type::F64 | Type::I32 | Type::U32 | Type::Bool => true,
        Type::Array { elem, .. } => opaque_dsp_field(elem),
        Type::Nominal { args, .. } => args.is_empty(),
        _ => false,
    }
}

fn opaque_dsp_schema(checked: &CheckedProgram, state: &StateSchema) -> bool {
    state.types.iter().all(|def| {
        checked.facts.structs.get(def).is_some_and(|item| {
            item.generics.is_empty() && item.fields.iter().all(|field| opaque_dsp_field(&field.ty))
        })
    })
}

fn opaque_dsp_migration(
    checked: &CheckedProgram,
    item: NodeId,
    state_def: NodeId,
    span: Span,
) -> Option<StateSchema> {
    let sig = checked.facts.fn_sigs.get(&item)?;
    if !sig.generics.is_empty()
        || sig.params.len() != 1
        || !matches!(&sig.ret, Type::Nominal { def, args, .. } if *def == state_def && args.is_empty())
    {
        return None;
    }
    let Type::Nominal { def, args, .. } = &sig.params[0].ty else {
        return None;
    };
    if !args.is_empty() || *def == state_def {
        return None;
    }
    schema(checked, *def, span)
        .ok()
        .filter(|old| opaque_dsp_schema(checked, old))
}

/// Check the DSP block contract: `struct State` with the host state schema and
/// `fn process_sample(state: &mut State, input: f32) -> f32` in the root module.
pub fn dsp_contract(checked: &CheckedProgram) -> Result<DspContract, Vec<Diagnostic>> {
    let root = &checked.program.root;
    let whole = root.span;
    let mut state_item = None;
    let mut process_item = None;
    let mut init_item = None;
    let mut migrate_item = None;
    for item in &root.items {
        match &item.kind {
            ItemKind::Struct(s) if s.name.name == "State" => {
                state_item = Some((item.id, item.span))
            }
            ItemKind::Enum(e) if e.name.name == "State" => state_item = Some((item.id, item.span)),
            ItemKind::Fn(f) if f.name.name == DSP_ENTRY => {
                process_item = Some((item.id, item.span))
            }
            ItemKind::Fn(f) if f.name.name == "init" => init_item = Some(item.id),
            ItemKind::Fn(f) if f.name.name == "migrate" => {
                migrate_item = Some((item.id, item.span))
            }
            _ => {}
        }
    }
    let Some((state_def, state_span)) = state_item else {
        return Err(vec![Diagnostic::error(
            E_DSP_ENTRY,
            whole,
            "a DSP block declares a struct `State` in its root module",
        )]);
    };
    let mut diags = Vec::new();
    let state = match schema(checked, state_def, state_span) {
        Ok(s) if s.text == DSP_STATE_SCHEMA_V1_TEXT && init_item.is_none() => Some(s),
        Ok(s) if init_item.is_some() && opaque_dsp_schema(checked, &s) => Some(s),
        Ok(_) => {
            diags.push(Diagnostic::error(
                E_DSP_STATE_TYPE,
                state_span,
                "DSP requires the fixed ABI 2 state or an initialized ABI 3 struct with supported scalar fields, arrays and nested structs",
            ));
            None
        }
        Err(_) => {
            diags.push(Diagnostic::error(
                E_DSP_STATE_TYPE,
                state_span,
                "DSP state must have a finite, non-generic struct layout",
            ));
            None
        }
    };
    let mut init = None;
    let mut migrate = None;
    if state.is_some() && init_item.is_some() {
        init = init_item.filter(|id| {
            checked.facts.fn_sigs.get(id).is_some_and(|sig| {
                sig.generics.is_empty()
                    && sig.params.is_empty()
                    && sig.effects.effects.is_empty()
                    && matches!(&sig.ret, Type::Nominal { def, args, .. } if *def == state_def && args.is_empty())
            })
        });
        if init.is_none() {
            diags.push(Diagnostic::error(
                E_DSP_ENTRY,
                whole,
                "opaque DSP state requires `fn init() -> State effects()`",
            ));
        } else if let Some(entry) = init {
            // This ABI entry cannot convey a trap. Apply the intrinsic trap
            // checks to its own callees even though it runs off the audio thread.
            let reachable = crate::effects::reachable_from(&checked.facts.call_graph, entry);
            let mut traps = DiagnosticBag::new();
            crate::traps::check_rt_traps(checked, &mut traps, Some(&reachable));
            diags.extend(traps.into_vec());
        }
        if let Some((item, span)) = migrate_item {
            match opaque_dsp_migration(checked, item, state_def, span) {
                Some(old) => migrate = Some((item, old)),
                None => diags.push(Diagnostic::error(E_DSP_ENTRY, span,
                    "opaque DSP migration requires `fn migrate(old: OldState) -> State` with a supported old layout")),
            }
        }
    }
    let process = match process_item {
        None => {
            diags.push(Diagnostic::error(
                E_DSP_ENTRY,
                whole,
                "a DSP block declares `fn process_sample(state: &mut State, input: f32) -> f32`",
            ));
            None
        }
        Some((id, span)) => {
            let ok = checked.facts.fn_sigs.get(&id).is_some_and(|sig| {
                let state_ref = matches!(&sig.params.first().map(|p| &p.ty),
                    Some(Type::Ref { mutable: true, inner })
                        if matches!(inner.as_ref(), Type::Nominal { def, args, .. } if *def == state_def && args.is_empty()));
                sig.generics.is_empty()
                    && sig.params.len() == 2
                    && state_ref
                    && sig.params[1].ty == Type::F32
                    && sig.ret == Type::F32
            });
            if ok {
                Some(id)
            } else {
                diags.push(Diagnostic::error(
                    E_DSP_ENTRY,
                    span,
                    "`process_sample` must be `fn process_sample(state: &mut State, input: f32) -> f32`",
                ));
                None
            }
        }
    };
    match (state, process) {
        (Some(state), Some(process)) if diags.is_empty() => Ok(DspContract {
            state,
            process,
            init,
            migrate,
        }),
        _ => Err(diags),
    }
}
