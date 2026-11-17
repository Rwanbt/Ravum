use super::*;

pub(crate) enum UseTarget {
    /// The last segment names a module. `ModuleId` is the module **where the
    /// entry is declared** (not the target module): MOD-4.3 makes a private
    /// module visible from its declaring module and that module's descendants,
    /// so the visibility check needs the declaring module. The synthetic
    /// `use crate;` / `use self;` / `use super;` form has no declared entry and
    /// passes the anchoring module with `Vis::Public`.
    Module(Sym, Vis, ModuleId),
    BuiltinModule(&'static str),
    BuiltinFn(&'static str),
    Symbol(Sym, Vis, ModuleId),
    /// A non-final segment is a module that is not visible from the importing
    /// module (MOD-4.2/MOD-6.2); carries the offending segment name.
    PrivateSegment(String),
    /// MOD-3.2: a bare relative path resolves both in the current module and
    /// at the crate root, to two different entities.
    Ambiguous,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UseCursor {
    Module(ModuleId),
    Builtin(&'static str),
}

pub(crate) fn resolve_use_target(world: &World, module: ModuleId, path: &Path) -> UseTarget {
    let segments = &path.segments;
    if segments.is_empty() {
        return UseTarget::Unknown;
    }

    // `use x;` : le nom est résolu dans le module courant, puis à la racine
    // (MOD-3.2 : un chemin relatif nu est interprété relativement au module
    // courant d'abord, puis à la racine du crate).
    if path.root == PathRoot::Plain && segments.len() == 1 {
        let name = &segments[0].name.name;
        if let Some(target) = resolve_use_single_name(world, module, name) {
            return target;
        }
        if module != world.root {
            return resolve_use_single_name(world, world.root, name).unwrap_or(UseTarget::Unknown);
        }
        return UseTarget::Unknown;
    }

    let (index, cursor) = match path.root {
        PathRoot::Crate => (0, UseCursor::Module(world.root)),
        PathRoot::SelfMod => (0, UseCursor::Module(module)),
        PathRoot::Super => match world.ancestor(module, 1) {
            Some(parent) => (0, UseCursor::Module(parent)),
            None => return UseTarget::Unknown,
        },
        // MOD-3.2: bare relative path, resolvable from the current module
        // first and then from the crate root. `crate`, `self` and `super` are
        // keywords: the parser turns them into a `PathRoot`, never a segment.
        PathRoot::Plain => return resolve_use_bare_path(world, module, segments),
    };

    resolve_use_tail(world, module, cursor, index, segments)
}

/// `use x;` — one plain segment. Tries each namespace in `at` (value, type,
/// module); `None` means the name is unknown there. The caller applies the
/// MOD-3.2 fallback to the crate root.
pub(crate) fn resolve_use_single_name(
    world: &World,
    at: ModuleId,
    name: &str,
) -> Option<UseTarget> {
    if let Some(r) = world.lookup(at, Ns::Value, name) {
        world.mark_used(at, r);
        return Some(finish_use_symbol(r.sym, r.vis, r.def_module));
    }
    if let Some(r) = world.lookup(at, Ns::Type, name) {
        world.mark_used(at, r);
        return Some(finish_use_symbol(r.sym, r.vis, r.def_module));
    }
    if let Some(r) = world.lookup(at, Ns::Module, name) {
        world.mark_used(at, r);
        return match r.sym {
            // MOD-4.3: the declaring module, not the target module, is what
            // decides visibility of the `mod` entry.
            Sym::Module(_) => Some(UseTarget::Module(r.sym, r.vis, r.def_module)),
            Sym::BuiltinModule(p) => Some(UseTarget::BuiltinModule(p)),
            _ => None,
        };
    }
    None
}

/// First-segment module anchor for a bare path in `at` (MOD-3.2). `std` is
/// available everywhere, so it is the fallback when no user module matches.
pub(crate) fn use_module_anchor(world: &World, at: ModuleId, first: &str) -> Option<UseCursor> {
    if let Some(r) = world.lookup(at, Ns::Module, first) {
        world.mark_used(at, r);
        return match r.sym {
            Sym::Module(m) => Some(UseCursor::Module(m)),
            Sym::BuiltinModule(p) => Some(UseCursor::Builtin(p)),
            _ => None,
        };
    }
    builtins::builtin_root_module(first).map(UseCursor::Builtin)
}

/// MOD-3.2: a bare relative path (`use foo::Bar;`, no `crate`/`self`/`super`
/// prefix) is interpreted relative to the current module first, then to the
/// crate root. When both interpretations resolve to different entities the
/// path is ambiguous (`E-RES-CONFLICT`); when one side does not resolve the
/// path, the other is used, which is what "d'abord ... puis ..." requires.
pub(crate) fn resolve_use_bare_path(
    world: &World,
    module: ModuleId,
    segments: &[ast::PathSegment],
) -> UseTarget {
    let first = &segments[0].name.name;
    let local = use_module_anchor(world, module, first);
    let root = if module == world.root {
        local
    } else {
        use_module_anchor(world, world.root, first)
    };
    let (local, root) = match (local, root) {
        (None, None) => return UseTarget::Unknown,
        (Some(cursor), None) | (None, Some(cursor)) => {
            return resolve_use_tail(world, module, cursor, 1, segments)
        }
        (Some(local), Some(root)) if local == root => {
            // Same module reached through two routes: no ambiguity.
            return resolve_use_tail(world, module, local, 1, segments);
        }
        (Some(local), Some(root)) => (local, root),
    };

    let local_target = resolve_use_tail(world, module, local, 1, segments);
    if !is_use_resolution(&local_target) {
        // The local interpretation does not resolve the path: MOD-3.2 falls
        // back to the crate root.
        let root_target = resolve_use_tail(world, module, root, 1, segments);
        return if is_use_resolution(&root_target) {
            root_target
        } else {
            prefer_use_failure(local_target, root_target)
        };
    }
    let root_target = resolve_use_tail(world, module, root, 1, segments);
    if is_use_resolution(&root_target) && !same_use_target(&local_target, &root_target) {
        UseTarget::Ambiguous
    } else {
        local_target
    }
}

/// Resolves the path from `index` in `segments`, starting at `cursor`.
/// Visibility is always judged from `from` (the importing module), never from
/// the module currently traversed (MOD-4.2/MOD-6.2).
pub(crate) fn resolve_use_tail(
    world: &World,
    from: ModuleId,
    mut cursor: UseCursor,
    mut index: usize,
    segments: &[ast::PathSegment],
) -> UseTarget {
    loop {
        if index >= segments.len() {
            return match cursor {
                // MOD-4.3/MOD-3.2: the synthetic anchoring form has no `mod`
                // entry, so it is public by construction.
                UseCursor::Module(m) => {
                    UseTarget::Module(Sym::Module(m), Vis::Public(Span::dummy()), m)
                }
                UseCursor::Builtin(p) => UseTarget::BuiltinModule(p),
            };
        }
        let last = index + 1 == segments.len();
        let name = &segments[index].name.name;
        match cursor {
            UseCursor::Module(m) => {
                if !last {
                    if let Some(r) = world.lookup(m, Ns::Module, name) {
                        world.mark_used(m, r);
                        // MOD-4.2/MOD-6.2: every module on the path of a
                        // `use` must be visible **from the importing module**,
                        // not from the module currently traversed. Checking
                        // only the final segment let
                        // `use crate::a::secret::X;` reach a private module.
                        if !entry_visible(world, r.vis, r.def_module, from) {
                            return UseTarget::PrivateSegment(name.clone());
                        }
                        cursor = match r.sym {
                            Sym::Module(child) => UseCursor::Module(child),
                            Sym::BuiltinModule(p) => UseCursor::Builtin(p),
                            _ => return UseTarget::Unknown,
                        };
                        index += 1;
                        continue;
                    }
                    return UseTarget::Unknown;
                }
                if let Some(r) = world.lookup(m, Ns::Module, name) {
                    world.mark_used(m, r);
                    return match r.sym {
                        Sym::Module(_) => UseTarget::Module(r.sym, r.vis, r.def_module),
                        Sym::BuiltinModule(p) => UseTarget::BuiltinModule(p),
                        _ => UseTarget::Unknown,
                    };
                }
                if let Some(r) = world.lookup(m, Ns::Type, name) {
                    world.mark_used(m, r);
                    return finish_use_symbol(r.sym, r.vis, r.def_module);
                }
                if let Some(r) = world.lookup(m, Ns::Value, name) {
                    world.mark_used(m, r);
                    return finish_use_symbol(r.sym, r.vis, r.def_module);
                }
                return UseTarget::Unknown;
            }
            UseCursor::Builtin(p) => {
                // `std` is the implicit prelude package: every module it
                // exposes is public by construction (MOD-7.3), so the
                // visibility rule of MOD-6.2 has nothing to reject here.
                let candidate = format!("{p}::{name}");
                if last {
                    if let Some(sig) = builtins::builtin_fn(&candidate) {
                        return UseTarget::BuiltinFn(sig.path);
                    }
                    if let Some(child) = builtins::builtin_child_module(p, name) {
                        return UseTarget::BuiltinModule(child);
                    }
                    return UseTarget::Unknown;
                }
                match builtins::builtin_child_module(p, name) {
                    Some(child) => {
                        cursor = UseCursor::Builtin(child);
                        index += 1;
                    }
                    None => return UseTarget::Unknown,
                }
            }
        }
    }
}

pub(crate) fn is_use_resolution(target: &UseTarget) -> bool {
    matches!(
        target,
        UseTarget::Module(..)
            | UseTarget::BuiltinModule(_)
            | UseTarget::BuiltinFn(_)
            | UseTarget::Symbol(..)
    )
}

/// Two successful interpretations are the same import when they designate the
/// same symbol declared in the same module (or the same builtin path).
pub(crate) fn same_use_target(a: &UseTarget, b: &UseTarget) -> bool {
    match (a, b) {
        (UseTarget::Module(sa, _, da), UseTarget::Module(sb, _, db))
        | (UseTarget::Symbol(sa, _, da), UseTarget::Symbol(sb, _, db)) => sa == sb && da == db,
        (UseTarget::BuiltinModule(pa), UseTarget::BuiltinModule(pb))
        | (UseTarget::BuiltinFn(pa), UseTarget::BuiltinFn(pb)) => pa == pb,
        _ => false,
    }
}

/// When neither interpretation resolves, report the most informative failure:
/// the local one unless it is a bare "unknown" and the root one is not.
pub(crate) fn prefer_use_failure(local: UseTarget, root: UseTarget) -> UseTarget {
    match &local {
        UseTarget::Unknown => root,
        _ => local,
    }
}

pub(crate) fn finish_use_symbol(sym: Sym, vis: Vis, def_module: ModuleId) -> UseTarget {
    match sym {
        Sym::BuiltinFn(p) => UseTarget::BuiltinFn(p),
        Sym::BuiltinModule(p) => UseTarget::BuiltinModule(p),
        // MOD-4.3: keep the declaring module so `entry_visible` can decide
        // whether the `use` sits in the module's subtree.
        Sym::Module(_) => UseTarget::Module(sym, vis, def_module),
        Sym::Fn(_) | Sym::Const(_) | Sym::Struct(_) | Sym::Enum(_) => {
            UseTarget::Symbol(sym, vis, def_module)
        }
    }
}

// ---------------------------------------------------------------------------
// Signatures d'items
// ---------------------------------------------------------------------------

pub(crate) fn resolve_headers(world: &mut World, facts: &mut Facts, diags: &mut DiagnosticBag) {
    {
        let module_asts: Vec<(ModuleId, &Module)> = world
            .module_asts
            .iter()
            .enumerate()
            .map(|(id, m)| (id, *m))
            .collect();
        for (module, ast) in &module_asts {
            for item in &ast.items {
                match &item.kind {
                    ItemKind::Fn(f) => {
                        resolve_fn_header(world, facts, diags, *module, item, f, None);
                    }
                    ItemKind::Struct(s) => {
                        resolve_struct(world, facts, diags, *module, item, s);
                    }
                    ItemKind::Enum(e) => {
                        resolve_enum(world, facts, diags, *module, item, e);
                    }
                    ItemKind::Const(c) => {
                        let mut ctx =
                            HeaderCtx::new(world, facts, diags, *module, Vec::new(), None);
                        ctx.resolve_type(&c.ty);
                    }
                    ItemKind::Extern(block) => {
                        resolve_extern_block(world, facts, diags, *module, block);
                    }
                    ItemKind::Impl(_)
                    | ItemKind::Use(_)
                    | ItemKind::Mod(_)
                    | ItemKind::ModDecl(_) => {}
                }
            }
        }
    }
    let impl_items: Vec<(ModuleId, &Item, &ast::ImplItem)> = world
        .impl_items
        .iter()
        .map(|(m, item, imp)| (*m, *item, *imp))
        .collect();
    for (module, item, imp) in impl_items {
        resolve_impl(world, facts, diags, module, item, imp);
    }
}
