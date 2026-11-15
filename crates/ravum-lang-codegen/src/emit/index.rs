//! Compile index: raw items, ownership hints, return kinds.
//!
//! Extracted from `emit` by E4 (P-6), no behaviour change.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RawKind {
    Fn,
    Struct,
    Enum,
    Const,
}

#[derive(Debug, Clone)]
pub(super) struct OwnerHint {
    /// Definition item id when the impl self type was resolved to a nominal type.
    pub(super) def: Option<NodeId>,
    /// Lexical type name used when no definition id is available.
    pub(super) name_hint: String,
}

#[derive(Debug, Clone)]
pub(super) struct RawItem {
    pub(super) id: NodeId,
    pub(super) name: String,
    pub(super) kind: RawKind,
    pub(super) is_test: bool,
    /// How the function returns (`()` versus `Result<(), Error>`).
    pub(super) ret: RetKind,
    pub(super) module_ravum: String,
    pub(super) module_rust: String,
    pub(super) owner: Option<OwnerHint>,
}

#[derive(Debug, Clone)]
pub(super) struct ItemEntry {
    pub(super) rust_name: String,
    pub(super) rust_path: String,
}

/// How `main` / a test function returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RetKind {
    Unit,
    Result,
    Other,
}

#[derive(Debug, Clone)]
pub(super) struct FnEntry {
    pub(super) rust_name: String,
    pub(super) ret: RetKind,
}

#[derive(Debug, Clone)]
pub(super) struct ModEntry {
    pub(super) rust_name: String,
    #[allow(dead_code)]
    pub(super) ravum_path: String,
    #[allow(dead_code)]
    pub(super) rust_path: String,
}

#[derive(Debug, Default)]
pub(super) struct Index {
    pub(super) modules: BTreeMap<NodeId, ModEntry>,
    pub(super) items: BTreeMap<NodeId, ItemEntry>,
    pub(super) fns: BTreeMap<NodeId, FnEntry>,
    /// AST variant names (Rust-mangled) per enum item id, in declaration order.
    pub(super) enum_variants: BTreeMap<NodeId, Vec<String>>,
    /// `(display name, item id)` of the tests, in declaration order.
    pub(super) tests: Vec<(String, NodeId)>,
    pub(super) user_main: Option<NodeId>,
    /// Ravum module paths (`""` for the root) used to rewrite `item_paths`.
    pub(super) module_paths: BTreeSet<String>,
}

impl Index {
    pub(super) fn build(checked: &CheckedProgram) -> Index {
        let mut index = Index::default();
        let mut raws: Vec<RawItem> = Vec::new();
        index.walk_module(
            checked,
            &checked.program.root,
            "",
            "crate::rvm_root",
            &mut raws,
        );

        // The user `main`: an explicit fact wins, else the first `main` declared
        // in the root module, else the first `main` in declaration order.
        let candidate = checked
            .facts
            .main_fn
            .or_else(|| first_user_main(&raws))
            .filter(|id| {
                raws.iter()
                    .any(|raw| raw.id == *id && raw.kind == RawKind::Fn)
            });
        index.user_main = candidate;

        // Rust names (the user `main` is renamed so the emitted `fn main` is ours).
        let mut rust_names: BTreeMap<NodeId, String> = BTreeMap::new();
        for raw in &raws {
            let name = if Some(raw.id) == index.user_main {
                GENERATED_MAIN.to_string()
            } else {
                mangle_ident(&raw.name)
            };
            rust_names.insert(raw.id, name);
        }

        // Pass A: non-method items (types must exist before methods reference them).
        for raw in raws.iter().filter(|raw| raw.owner.is_none()) {
            let rust_name = rust_names[&raw.id].clone();
            let rust_path = format!("{}::{}", raw.module_rust, rust_name);
            index.items.insert(
                raw.id,
                ItemEntry {
                    rust_name,
                    rust_path,
                },
            );
        }

        // Pass B: inherent methods -> `Type::method`, owned by the type path.
        for raw in raws.iter().filter(|raw| raw.owner.is_some()) {
            let rust_name = rust_names[&raw.id].clone();
            let owner_path = match &raw.owner {
                Some(owner) => match owner.def.and_then(|def| index.items.get(&def)) {
                    Some(entry) => entry.rust_path.clone(),
                    None if owner.name_hint.is_empty() => raw.module_rust.clone(),
                    None => format!("{}::{}", raw.module_rust, mangle_ident(&owner.name_hint)),
                },
                None => raw.module_rust.clone(),
            };
            index.items.insert(
                raw.id,
                ItemEntry {
                    rust_path: format!("{}::{}", owner_path, rust_name),
                    rust_name,
                },
            );
        }

        // Function table (`ret` already classifies `main`/test entry points).
        for raw in raws.iter().filter(|raw| raw.kind == RawKind::Fn) {
            let entry = match index.items.get(&raw.id) {
                Some(entry) => entry,
                None => continue,
            };
            index.fns.insert(
                raw.id,
                FnEntry {
                    rust_name: entry.rust_name.clone(),
                    ret: raw.ret,
                },
            );
        }

        // Tests: `Facts::test_fns` order is authoritative when present.
        if checked.facts.test_fns.is_empty() {
            for raw in raws.iter().filter(|raw| raw.is_test) {
                index.tests.push((test_display_name(raw), raw.id));
            }
        } else {
            for id in &checked.facts.test_fns {
                if let Some(raw) = raws.iter().find(|raw| raw.id == *id) {
                    index.tests.push((test_display_name(raw), raw.id));
                }
            }
        }

        index
    }

    pub(super) fn walk_module(
        &mut self,
        checked: &CheckedProgram,
        module: &Module,
        ravum_path: &str,
        rust_path: &str,
        raws: &mut Vec<RawItem>,
    ) {
        let rust_name = if ravum_path.is_empty() {
            "rvm_root".to_string()
        } else {
            let own = module
                .name
                .as_ref()
                .map(|ident| ident.name.clone())
                .unwrap_or_default();
            format!("rvm_{}", sanitize_segment(&own))
        };
        self.modules.insert(
            module.id,
            ModEntry {
                rust_name,
                ravum_path: ravum_path.to_string(),
                rust_path: rust_path.to_string(),
            },
        );
        self.module_paths.insert(ravum_path.to_string());

        for item in &module.items {
            match &item.kind {
                ItemKind::Fn(fun) => raws.push(RawItem {
                    id: item.id,
                    name: fun.name.name.clone(),
                    kind: RawKind::Fn,
                    is_test: fun.is_test,
                    ret: ret_kind_of(checked, item.id, fun),
                    module_ravum: ravum_path.to_string(),
                    module_rust: rust_path.to_string(),
                    owner: None,
                }),
                ItemKind::Struct(structure) => raws.push(RawItem {
                    id: item.id,
                    name: structure.name.name.clone(),
                    kind: RawKind::Struct,
                    is_test: false,
                    ret: RetKind::Unit,
                    module_ravum: ravum_path.to_string(),
                    module_rust: rust_path.to_string(),
                    owner: None,
                }),
                ItemKind::Enum(enumeration) => {
                    let variants = enumeration
                        .variants
                        .iter()
                        .map(|variant| mangle_ident(&variant.name.name))
                        .collect::<Vec<_>>();
                    self.enum_variants.insert(item.id, variants);
                    raws.push(RawItem {
                        id: item.id,
                        name: enumeration.name.name.clone(),
                        kind: RawKind::Enum,
                        is_test: false,
                        ret: RetKind::Unit,
                        module_ravum: ravum_path.to_string(),
                        module_rust: rust_path.to_string(),
                        owner: None,
                    });
                }
                ItemKind::Const(constant) => raws.push(RawItem {
                    id: item.id,
                    name: constant.name.name.clone(),
                    kind: RawKind::Const,
                    is_test: false,
                    ret: RetKind::Unit,
                    module_ravum: ravum_path.to_string(),
                    module_rust: rust_path.to_string(),
                    owner: None,
                }),
                ItemKind::Impl(implementation) => {
                    let owner = impl_owner_hint(checked, implementation);
                    for sub in &implementation.items {
                        if let ItemKind::Fn(fun) = &sub.kind {
                            raws.push(RawItem {
                                id: sub.id,
                                name: fun.name.name.clone(),
                                kind: RawKind::Fn,
                                is_test: fun.is_test,
                                ret: ret_kind_of(checked, sub.id, fun),
                                module_ravum: ravum_path.to_string(),
                                module_rust: rust_path.to_string(),
                                owner: Some(owner.clone()),
                            });
                        }
                    }
                }
                ItemKind::Mod(inner) => {
                    let name = inner
                        .name
                        .as_ref()
                        .map(|ident| ident.name.clone())
                        .unwrap_or_default();
                    let child_ravum = if ravum_path.is_empty() {
                        name.clone()
                    } else {
                        format!("{ravum_path}::{name}")
                    };
                    let child_rust = format!("{rust_path}::rvm_{}", sanitize_segment(&name));
                    self.walk_module(checked, inner, &child_ravum, &child_rust, raws);
                }
                ItemKind::Use(_) | ItemKind::ModDecl(_) | ItemKind::Extern(_) => {}
            }
        }
    }
}

pub(super) fn first_user_main(raws: &[RawItem]) -> Option<NodeId> {
    let candidates: Vec<&RawItem> = raws
        .iter()
        .filter(|raw| {
            raw.kind == RawKind::Fn && !raw.is_test && raw.owner.is_none() && raw.name == "main"
        })
        .collect();
    candidates
        .iter()
        .find(|raw| raw.module_ravum.is_empty())
        .or_else(|| candidates.first())
        .map(|raw| raw.id)
}

pub(super) fn test_display_name(raw: &RawItem) -> String {
    if raw.module_ravum.is_empty() {
        raw.name.clone()
    } else {
        format!("{}::{}", raw.module_ravum, raw.name)
    }
}

pub(super) fn impl_owner_hint(checked: &CheckedProgram, implementation: &ImplItem) -> OwnerHint {
    if let Some(Type::Nominal { def, name, .. }) =
        checked.facts.type_nodes.get(&implementation.self_ty.id)
    {
        return OwnerHint {
            def: Some(*def),
            name_hint: name.clone(),
        };
    }
    match &implementation.self_ty.kind {
        TypeKind::Path(path) => OwnerHint {
            def: None,
            name_hint: path
                .last()
                .map(|segment| segment.name.name.clone())
                .unwrap_or_default(),
        },
        _ => OwnerHint {
            def: None,
            name_hint: String::new(),
        },
    }
}

pub(super) fn classify_ret(ty: &Type) -> RetKind {
    match ty {
        Type::Unit => RetKind::Unit,
        Type::Never => RetKind::Unit,
        Type::Result(ok, _) if **ok == Type::Unit => RetKind::Result,
        Type::Result(_, _) => RetKind::Other,
        _ => RetKind::Other,
    }
}

/// Return classification of a function: resolved type node first, then `FnSig`,
/// then the syntactic shape (fallback for partially filled facts).
pub(super) fn ret_kind_of(checked: &CheckedProgram, id: NodeId, fun: &FnItem) -> RetKind {
    if let Some(ty) = checked.facts.type_nodes.get(&fun.ret.id) {
        return classify_ret(ty);
    }
    if let Some(sig) = checked.facts.fn_sigs.get(&id) {
        return classify_ret(&sig.ret);
    }
    match &fun.ret.kind {
        TypeKind::Unit => RetKind::Unit,
        TypeKind::Path(path) if path.last().map(|s| s.name.name.as_str()) == Some("Result") => {
            RetKind::Result
        }
        _ => RetKind::Other,
    }
}

// ---------------------------------------------------------------------------
// Identifiers.
// ---------------------------------------------------------------------------
