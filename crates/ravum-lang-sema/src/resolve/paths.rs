use super::*;

impl PathError {
    pub(crate) fn unknown(name: &str) -> Self {
        Self {
            code: "E-RES-UNKNOWN",
            message: format!("unknown name `{name}`"),
        }
    }

    pub(crate) fn private(name: &str) -> Self {
        Self {
            code: "E-RES-PRIVATE",
            message: format!("`{name}` is private"),
        }
    }

    pub(crate) fn variant_unqualified(name: &str, enum_name: &str) -> Self {
        Self {
            code: "E-RES-UNKNOWN",
            message: format!(
                "variant `{name}` must be qualified as `{enum_name}::{name}` in Core 1.0"
            ),
        }
    }
}

/// Résout un chemin de valeur (sans les liaisons locales, gérées par typeck).
///
/// Les noms non qualifiés `Some`/`None`/`Ok`/`Err` sont acceptés et résolus vers
/// les constructeurs du prélude.
pub(crate) fn resolve_value_path(
    world: &World,
    module: ModuleId,
    generics: &[(String, usize)],
    path: &Path,
) -> Result<ValuePath, PathError> {
    let segments = &path.segments;
    if segments.is_empty() {
        return Err(PathError::unknown("<empty>"));
    }

    // Un seul segment non qualifié : valeur locale, prélude, constructeur.
    if path.root == PathRoot::Plain && segments.len() == 1 {
        let name = &segments[0].name.name;
        if !segments[0].args.is_empty() {
            return Err(PathError {
                code: "E-PARSE-UNEXPECTED",
                message: "explicit generic arguments are not supported in expressions".to_string(),
            });
        }
        if generics.iter().any(|(n, _)| n == name) {
            return Err(PathError::unknown(name));
        }
        if let Some(sym) = world.lookup(module, Ns::Value, name) {
            world.mark_used(module, sym);
            // A name looked up in the current module is always visible from it:
            // `lookup` answers `def_module == module`, which `visible_from` accepts.
            return match sym.sym {
                Sym::Fn(id) | Sym::Const(id) => Ok(ValuePath::Item(id)),
                Sym::BuiltinFn(p) => Ok(ValuePath::BuiltinFn(p)),
                other => Err(PathError {
                    code: "E-RES-UNKNOWN",
                    message: format!("`{name}` is a {}, not a value", other.label()),
                }),
            };
        }
        if let Some(p) = builtins::prelude_value_path(name) {
            return Ok(ValuePath::BuiltinFn(p));
        }
        match name.as_str() {
            "Some" => {
                return Ok(ValuePath::BuiltinCtor {
                    ctor: PayloadCtor::Some,
                })
            }
            "None" => return Ok(ValuePath::BuiltinNone),
            "Ok" => {
                return Ok(ValuePath::BuiltinCtor {
                    ctor: PayloadCtor::Ok,
                })
            }
            "Err" => {
                return Ok(ValuePath::BuiltinCtor {
                    ctor: PayloadCtor::Err,
                })
            }
            _ => {}
        }
        if let Some(sym) = world.lookup(module, Ns::Type, name) {
            world.mark_used(module, sym);
            return Ok(ValuePath::Type(type_of_symbol(world, sym.sym)));
        }
        if let Some(t) = builtins::prelude_type(name) {
            return Ok(ValuePath::Type(t));
        }
        if let Some(t) = prelude_container(name) {
            return Ok(ValuePath::Type(t));
        }
        // Variante non qualifiée ?
        if let Some((enum_item, enum_name)) = find_unqualified_variant(world, name) {
            return Err(PathError::variant_unqualified(name, &enum_name).with_enum(enum_item));
        }
        return Err(PathError::unknown(name));
    }

    // Point de départ : module ou type.
    enum Cursor {
        Module(ModuleId),
        Builtin(&'static str),
        Type(Type),
    }
    let mut index = 0usize;
    let mut cursor = match path.root {
        PathRoot::Crate => Cursor::Module(world.root),
        PathRoot::SelfMod => Cursor::Module(module),
        PathRoot::Super => match world.ancestor(module, 1) {
            Some(parent) => Cursor::Module(parent),
            None => return Err(PathError::unknown("super")),
        },
        PathRoot::Plain => {
            let first = &segments[0].name.name;
            // `Self` is the keyword `Kw::SelfType`, never an identifier segment, and
            // the parser refuses it in expression and pattern position.
            if let Some(sym) = world.lookup(module, Ns::Module, first) {
                world.mark_used(module, sym);
                // A name looked up in the current module is always visible from it:
                // `lookup` answers `def_module == module`, which `visible_from` accepts.
                index = 1;
                match sym.sym {
                    Sym::Module(m) => Cursor::Module(m),
                    Sym::BuiltinModule(p) => Cursor::Builtin(p),
                    _ => return Err(PathError::unknown(first)),
                }
            } else if first == "std" {
                index = 1;
                Cursor::Builtin("std")
            } else if builtins::builtin_root_module(first).is_some() {
                // STD-1.1: no module is in the prelude; `io::f` needs `use std::io;`.
                return Err(PathError {
                    code: "E-RES-UNKNOWN",
                    message: format!("module `{first}` is not in scope (add `use std::{first};`)"),
                });
            } else if let Some(sym) = world.lookup(module, Ns::Type, first) {
                world.mark_used(module, sym);
                index = 1;
                Cursor::Type(type_of_symbol(world, sym.sym))
            } else if let Some(t) = builtins::prelude_type(first) {
                index = 1;
                Cursor::Type(t)
            } else if let Some(t) = prelude_container(first) {
                index = 1;
                Cursor::Type(t)
            } else {
                return Err(PathError::unknown(first));
            }
        }
    };

    loop {
        // Entered with `index < len` (an empty path is refused above, and a
        // plain path consumes only its first segment before the loop when it
        // has at least two); each turn advances or returns on the last segment.
        let last = index + 1 == segments.len();
        let name = &segments[index].name.name;
        match cursor {
            Cursor::Module(m) => {
                if !last {
                    if let Some(sym) = world.lookup(m, Ns::Module, name) {
                        world.mark_used(m, sym);
                        if !world.visible_from(&sym, module) {
                            return Err(PathError::private(name));
                        }
                        cursor = match sym.sym {
                            Sym::Module(child) => Cursor::Module(child),
                            Sym::BuiltinModule(p) => Cursor::Builtin(p),
                            _ => return Err(PathError::unknown(name)),
                        };
                        index += 1;
                    } else if let Some(sym) = world.lookup(m, Ns::Type, name) {
                        // `Module::Type::item` : le segment suivant est un item
                        // associé ou une variante.
                        world.mark_used(m, sym);
                        if !world.visible_from(&sym, module) {
                            return Err(PathError::private(name));
                        }
                        cursor = Cursor::Type(type_of_symbol(world, sym.sym));
                        index += 1;
                    } else {
                        return Err(PathError::unknown(name));
                    }
                } else if let Some(sym) = world.lookup(m, Ns::Value, name) {
                    world.mark_used(m, sym);
                    if !world.visible_from(&sym, module) {
                        return Err(PathError::private(name));
                    }
                    return match sym.sym {
                        Sym::Fn(id) | Sym::Const(id) => Ok(ValuePath::Item(id)),
                        Sym::BuiltinFn(p) => Ok(ValuePath::BuiltinFn(p)),
                        other => Err(PathError {
                            code: "E-RES-UNKNOWN",
                            message: format!("`{name}` is a {}, not a value", other.label()),
                        }),
                    };
                } else if let Some(sym) = world.lookup(m, Ns::Module, name) {
                    world.mark_used(m, sym);
                    if !world.visible_from(&sym, module) {
                        return Err(PathError::private(name));
                    }
                    return match sym.sym {
                        Sym::Module(_) => Ok(ValuePath::Module),
                        Sym::BuiltinModule(_) => Ok(ValuePath::BuiltinModule),
                        _ => Err(PathError::unknown(name)),
                    };
                } else if let Some(sym) = world.lookup(m, Ns::Type, name) {
                    world.mark_used(m, sym);
                    if !world.visible_from(&sym, module) {
                        return Err(PathError::private(name));
                    }
                    return Ok(ValuePath::Type(type_of_symbol(world, sym.sym)));
                } else {
                    return Err(PathError::unknown(name));
                }
            }
            Cursor::Builtin(p) => {
                if !last {
                    match builtins::builtin_child_module(p, name) {
                        Some(child) => {
                            cursor = Cursor::Builtin(child);
                            index += 1;
                        }
                        None => return Err(PathError::unknown(name)),
                    }
                } else {
                    let candidate = format!("{p}::{name}");
                    if let Some(sig) = builtins::builtin_fn(&candidate) {
                        return Ok(ValuePath::BuiltinFn(sig.path));
                    }
                    if builtins::is_builtin_module(&candidate) {
                        return Ok(ValuePath::BuiltinModule);
                    }
                    return Err(PathError::unknown(name));
                }
            }
            Cursor::Type(t) => {
                if !last {
                    return Err(PathError::unknown(name));
                }
                return resolve_associated(world, t, name);
            }
        }
    }
}

impl PathError {
    pub(crate) fn with_enum(self, _enum_item: NodeId) -> PathError {
        self
    }
}

/// Type conteneur du prélude utilisé comme préfixe d'un chemin associé.
pub(crate) fn prelude_container(name: &str) -> Option<Type> {
    match name {
        "Vec" => Some(Type::Vec(Box::new(Type::Error))),
        "Option" => Some(Type::Option(Box::new(Type::Error))),
        "Result" => Some(Type::Result(Box::new(Type::Error), Box::new(Type::Error))),
        "Map" => Some(builtins::map_type(Type::Error, Type::Error)),
        _ => None,
    }
}

pub(crate) fn type_of_symbol(world: &World, sym: Sym) -> Type {
    match sym {
        Sym::Struct(id) => Type::Nominal {
            def: id,
            name: world
                .structs
                .get(&id)
                .map(|s| s.name.name.clone())
                .unwrap_or_default(),
            args: Vec::new(),
        },
        Sym::Enum(id) => Type::Nominal {
            def: id,
            name: world
                .enums
                .get(&id)
                .map(|e| e.name.name.clone())
                .unwrap_or_default(),
            args: Vec::new(),
        },
        _ => Type::Error,
    }
}

pub(crate) fn find_unqualified_variant(world: &World, name: &str) -> Option<(NodeId, String)> {
    for (id, item) in world.enums.iter() {
        if item.variants.iter().any(|v| v.name.name == name) {
            return Some((*id, item.name.name.clone()));
        }
    }
    if matches!(name, "Some" | "None") {
        return Some((NodeId::MAX, "Option".to_string()));
    }
    if matches!(name, "Ok" | "Err") {
        return Some((NodeId::MAX, "Result".to_string()));
    }
    None
}

pub(crate) fn resolve_associated(
    world: &World,
    ty: Type,
    name: &str,
) -> Result<ValuePath, PathError> {
    match &ty {
        Type::Nominal { def, .. } if *def == builtins::PRELUDE_ERROR_DEF => {
            builtin_assoc("Error", name)
        }
        Type::Nominal { def, .. } if *def == builtins::PRELUDE_MAP_DEF => {
            builtin_assoc("Map", name)
        }
        Type::Nominal { def, .. } => {
            // `nominal_kind` answers `Enum` exactly for the ids of `world.enums`.
            if let Some(enum_ast) = world.enums.get(def) {
                if let Some(index) = enum_ast.variants.iter().position(|v| v.name.name == name) {
                    return Ok(ValuePath::Variant {
                        enum_item: *def,
                        variant: index,
                    });
                }
            }
            if let Some(entry) = world.method(*def, name, true) {
                return Ok(ValuePath::AssocFn(entry.item));
            }
            Err(PathError::unknown(name))
        }
        Type::Option(_) => match name {
            "Some" => Ok(ValuePath::BuiltinCtor {
                ctor: PayloadCtor::Some,
            }),
            "None" => Ok(ValuePath::BuiltinNone),
            _ => Err(PathError::unknown(name)),
        },
        Type::Result(_, _) => match name {
            "Ok" => Ok(ValuePath::BuiltinCtor {
                ctor: PayloadCtor::Ok,
            }),
            "Err" => Ok(ValuePath::BuiltinCtor {
                ctor: PayloadCtor::Err,
            }),
            _ => Err(PathError::unknown(name)),
        },
        Type::String => builtin_assoc("String", name),
        Type::Vec(_) => builtin_assoc("Vec", name),
        _ => Err(PathError::unknown(name)),
    }
}

pub(crate) fn builtin_assoc(type_name: &str, name: &str) -> Result<ValuePath, PathError> {
    let path = format!("{type_name}::{name}");
    if let Some(sig) = builtins::builtin_assoc_fn(&path) {
        Ok(ValuePath::BuiltinFn(sig.path))
    } else {
        Err(PathError::unknown(name))
    }
}

// ---------------------------------------------------------------------------
// API de la phase de résolution
// ---------------------------------------------------------------------------

/// Résout le programme : construit le monde, vérifie les noms, les imports et
/// la visibilité, puis enregistre les signatures d'items dans `facts`.
pub fn resolve(
    program: &ast::Program,
    _sources: &SourceMap,
    facts: &mut Facts,
    diags: &mut DiagnosticBag,
) -> bool {
    let mut world = World::build(program);
    resolve_with_world(&mut world, facts, diags)
}

/// Variante réutilisant un monde déjà construit (utilisée par `check`).
pub(crate) fn resolve_with_world(
    world: &mut World,
    facts: &mut Facts,
    diags: &mut DiagnosticBag,
) -> bool {
    report_structural_errors(world, diags);
    fill_module_paths(world, facts);
    fill_item_paths(world, facts);
    resolve_imports(world, diags);
    resolve_headers(world, facts, diags);
    compute_copy_and_recursive_types(world, facts, diags);
    facts.test_fns = world.test_fns.clone();
    !diags.has_errors()
}

pub(crate) fn report_structural_errors(world: &World, diags: &mut DiagnosticBag) {
    for (_, _, span, name) in &world.mod_decls {
        diags.push(
            Diagnostic::error(
                "E-RES-MODDECL",
                *span,
                format!("module declaration `mod {name};` was not expanded by the loader"),
            )
            .with_note(
                "the loader (worker cli) must expand file modules before checking",
                None,
            ),
        );
    }
    for (module, ns, name, dup_span, first_span) in &world.duplicates {
        // Un doublon de `main` est diagnostiqué par le typage (E-TYP-MAIN-DUPLICATE).
        if *module == world.root && *ns == Ns::Value && name == "main" {
            continue;
        }
        let ns_name = match ns {
            Ns::Type => "type",
            Ns::Value => "value",
            Ns::Module => "module",
        };
        diags.push(
            Diagnostic::error(
                "E-RES-DUPLICATE",
                *dup_span,
                format!("duplicate {ns_name} name `{name}` in this module"),
            )
            .with_note("first declared here", Some(*first_span)),
        );
    }
    for (_, ns, name, span) in &world.prelude_conflicts {
        let what = match ns {
            Ns::Type => "type",
            Ns::Value => "value",
            Ns::Module => "module",
        };
        diags.push(
            Diagnostic::error(
                "E-RES-CONFLICT",
                *span,
                format!("{what} name `{name}` collides with a prelude name"),
            )
            .with_note("prelude names are reserved in Core 1.0", None),
        );
    }
}

pub(crate) fn fill_module_paths(world: &World, facts: &mut Facts) {
    for module in &world.modules {
        facts.module_paths.insert(module.node, module.path.clone());
        facts.item_paths.insert(module.node, module.path.clone());
    }
}

pub(crate) fn fill_item_paths(world: &World, facts: &mut Facts) {
    for module in &world.modules {
        for (name, item) in &module.types {
            if let Some(id) = item.sym.item() {
                facts
                    .item_paths
                    .insert(id, join_path(&module.path, &rust_ident(name)));
            }
        }
        for (name, item) in &module.values {
            if let Some(id) = item.sym.item() {
                facts
                    .item_paths
                    .insert(id, join_path(&module.path, &rust_ident(name)));
            }
        }
        for (name, item) in &module.modules {
            if let Sym::Module(child) = item.sym {
                facts.item_paths.insert(
                    item.sym.item().unwrap_or(world.modules[child].node),
                    join_path(&module.path, &rust_ident(name)),
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Imports
// ---------------------------------------------------------------------------

pub(crate) fn resolve_imports(world: &mut World, diags: &mut DiagnosticBag) {
    let uses: Vec<(ModuleId, &Item, &UseItem)> = world
        .use_items
        .iter()
        .map(|(m, item, u)| (*m, *item, *u))
        .collect();
    let mut reported_reexport: BTreeSet<NodeId> = BTreeSet::new();
    for (module, item, use_item) in uses {
        let display = use_item.path.display();
        if item.vis.is_public() && reported_reexport.insert(item.id) {
            diags.error(
                "E-UNSUPPORTED-REEXPORT",
                item.span,
                "`pub use` (re-export) is not supported in Core 1.0",
            );
        }
        match resolve_use_target(world, module, &use_item.path) {
            UseTarget::Module(sym, vis, def_module) => {
                let name = last_segment(&use_item.path);
                if !entry_visible(world, vis, def_module, module) {
                    diags.error(
                        "E-RES-PRIVATE",
                        use_item.span,
                        format!("`{display}` is private and cannot be imported here"),
                    );
                    continue;
                }
                insert_import(
                    world,
                    module,
                    ImportEntry {
                        name,
                        ns: Ns::Module,
                        sym,
                        span: use_item.span,
                        display,
                        used: Cell::new(false),
                        uses: Cell::new(0),
                    },
                    diags,
                );
            }
            UseTarget::BuiltinModule(path) => {
                insert_import(
                    world,
                    module,
                    ImportEntry {
                        name: last_segment(&use_item.path),
                        ns: Ns::Module,
                        sym: Sym::BuiltinModule(path),
                        span: use_item.span,
                        display,
                        used: Cell::new(false),
                        uses: Cell::new(0),
                    },
                    diags,
                );
            }
            UseTarget::BuiltinFn(path) => {
                insert_import(
                    world,
                    module,
                    ImportEntry {
                        name: last_segment(&use_item.path),
                        ns: Ns::Value,
                        sym: Sym::BuiltinFn(path),
                        span: use_item.span,
                        display,
                        used: Cell::new(false),
                        uses: Cell::new(0),
                    },
                    diags,
                );
            }
            UseTarget::Symbol(sym, vis, def_module) => {
                if !entry_visible(world, vis, def_module, module) {
                    diags.error(
                        "E-RES-PRIVATE",
                        use_item.span,
                        format!("`{display}` is private and cannot be imported here"),
                    );
                    continue;
                }
                insert_import(
                    world,
                    module,
                    ImportEntry {
                        name: last_segment(&use_item.path),
                        ns: sym.ns(),
                        sym,
                        span: use_item.span,
                        display,
                        used: Cell::new(false),
                        uses: Cell::new(0),
                    },
                    diags,
                );
            }
            UseTarget::PrivateSegment(segment) => {
                // MOD-4.2/MOD-6.2: only `pub` modules may appear on the path
                // of a `use`; a private intermediate module denies access to
                // everything below it, exactly like a value/type path does.
                diags.error(
                    "E-RES-PRIVATE",
                    use_item.span,
                    format!(
                        "`{display}` is inaccessible from this module: \
                         intermediate module `{segment}` is private"
                    ),
                );
            }
            UseTarget::Ambiguous => {
                // MOD-3.2: a bare relative path is first resolved against the
                // current module, then against the crate root; when both
                // interpretations resolve to different entities the path is
                // ambiguous and the import is refused.
                diags.error(
                    "E-RES-CONFLICT",
                    use_item.span,
                    format!(
                        "`{display}` is ambiguous: it resolves both in the current module \
                         and at the crate root; use a `crate::`/`self::`/`super::` prefix"
                    ),
                );
            }
            UseTarget::Unknown => {
                diags.error(
                    "E-RES-UNKNOWN",
                    use_item.span,
                    format!("unknown import path `{display}`"),
                );
            }
        }
    }
}

pub(crate) fn last_segment(path: &Path) -> String {
    path.last()
        .map(|s| s.name.name.clone())
        .unwrap_or_else(|| path.display())
}

pub(crate) fn entry_visible(world: &World, vis: Vis, def_module: ModuleId, from: ModuleId) -> bool {
    if vis.is_public() || def_module == ModuleId::MAX {
        return true;
    }
    let mut current = Some(from);
    while let Some(m) = current {
        if m == def_module {
            return true;
        }
        current = world.modules[m].parent;
    }
    false
}

pub(crate) fn insert_import(
    world: &mut World,
    module: ModuleId,
    entry: ImportEntry,
    diags: &mut DiagnosticBag,
) {
    // Same namespace invariant as `World::register`, for imports.
    debug_assert_eq!(
        entry.sym.ns(),
        entry.ns,
        "import `{}` in the wrong namespace",
        entry.name
    );
    if let Some(existing) = world.table(module, entry.ns).get(&entry.name) {
        let span = existing.span;
        diags.push(
            Diagnostic::error(
                "E-RES-DUPLICATE",
                entry.span,
                format!("import `{}` conflicts with an existing name", entry.name),
            )
            .with_note("first declared here", Some(span)),
        );
        return;
    }
    if let Some(existing) = world.modules[module]
        .imports
        .iter()
        .find(|i| i.ns == entry.ns && i.name == entry.name)
    {
        let span = existing.span;
        diags.push(
            Diagnostic::error(
                "E-RES-DUPLICATE",
                entry.span,
                format!("import `{}` conflicts with an existing import", entry.name),
            )
            .with_note("first imported here", Some(span)),
        );
        return;
    }
    world.modules[module].imports.push(entry);
}
