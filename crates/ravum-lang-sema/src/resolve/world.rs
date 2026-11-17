use super::*;

impl<'a> World<'a> {
    pub fn build(program: &'a ast::Program) -> World<'a> {
        let mut world = World {
            modules: Vec::new(),
            module_asts: Vec::new(),
            fns: BTreeMap::new(),
            extern_fns: BTreeMap::new(),
            structs: BTreeMap::new(),
            enums: BTreeMap::new(),
            consts: BTreeMap::new(),
            impl_items: Vec::new(),
            use_items: Vec::new(),
            module_of_node: BTreeMap::new(),
            item_module: BTreeMap::new(),
            impls: BTreeMap::new(),
            mod_decls: Vec::new(),
            duplicates: Vec::new(),
            prelude_conflicts: Vec::new(),
            test_fns: Vec::new(),
            root: 0,
        };
        world.modules.push(ModuleInfo {
            node: program.root.id,
            parent: None,
            path: "crate".to_string(),
            types: BTreeMap::new(),
            values: BTreeMap::new(),
            modules: BTreeMap::new(),
            imports: Vec::new(),
        });
        world.module_asts.push(&program.root);
        world.module_of_node.insert(program.root.id, 0);
        world.item_module.insert(program.root.id, 0);
        world.build_module(&program.root, 0);
        world
    }

    pub(crate) fn build_module(&mut self, module: &'a Module, id: ModuleId) {
        for item in &module.items {
            self.item_module.insert(item.id, id);
            match &item.kind {
                ItemKind::Fn(f) => {
                    self.fns.insert(item.id, f);
                    self.register(
                        id,
                        Ns::Value,
                        &f.name.name,
                        Sym::Fn(item.id),
                        item.vis,
                        item.span,
                    );
                    if f.is_test {
                        self.test_fns.push(item.id);
                    }
                }
                ItemKind::Struct(s) => {
                    self.structs.insert(item.id, s);
                    self.register(
                        id,
                        Ns::Type,
                        &s.name.name,
                        Sym::Struct(item.id),
                        item.vis,
                        item.span,
                    );
                }
                ItemKind::Enum(e) => {
                    self.enums.insert(item.id, e);
                    self.register(
                        id,
                        Ns::Type,
                        &e.name.name,
                        Sym::Enum(item.id),
                        item.vis,
                        item.span,
                    );
                }
                ItemKind::Const(c) => {
                    self.consts.insert(item.id, c);
                    self.register(
                        id,
                        Ns::Value,
                        &c.name.name,
                        Sym::Const(item.id),
                        item.vis,
                        item.span,
                    );
                }
                ItemKind::Mod(inner) => {
                    let name = inner
                        .name
                        .as_ref()
                        .map(|n| n.name.clone())
                        .unwrap_or_default();
                    let child = self.modules.len();
                    let parent_path = self.modules[id].path.clone();
                    let path = join_path(&parent_path, &name);
                    self.modules.push(ModuleInfo {
                        node: inner.id,
                        parent: Some(id),
                        path,
                        types: BTreeMap::new(),
                        values: BTreeMap::new(),
                        modules: BTreeMap::new(),
                        imports: Vec::new(),
                    });
                    self.module_asts.push(inner);
                    self.module_of_node.insert(inner.id, child);
                    self.register(
                        id,
                        Ns::Module,
                        &name,
                        Sym::Module(child),
                        item.vis,
                        item.span,
                    );
                    self.build_module(inner, child);
                }
                ItemKind::ModDecl(d) => {
                    self.mod_decls
                        .push((id, item.id, d.name.span, d.name.name.clone()));
                }
                ItemKind::Impl(imp) => {
                    self.impl_items.push((id, item, imp));
                }
                ItemKind::Extern(block) => {
                    for f in &block.fns {
                        self.item_module.insert(f.id, id);
                        self.extern_fns.insert(f.id, f);
                        self.register(id, Ns::Value, &f.name.name, Sym::Fn(f.id), item.vis, f.span);
                    }
                }
                ItemKind::Use(u) => {
                    self.use_items.push((id, item, u));
                }
            }
        }
    }

    pub(crate) fn register(
        &mut self,
        module: ModuleId,
        ns: Ns,
        name: &str,
        sym: Sym,
        vis: Vis,
        span: Span,
    ) {
        // Namespace invariant: a table only holds symbols of its own
        // namespace, so a lookup in `Ns::Module` returns a module, in
        // `Ns::Value` a function or a constant, in `Ns::Type` a struct or an
        // enum. The `_` arms after a lookup rely on it.
        debug_assert_eq!(
            sym.ns(),
            ns,
            "symbol `{name}` registered in the wrong namespace"
        );
        if let Some(previous) = self.table(module, ns).get(name) {
            let first = previous.span;
            self.duplicates
                .push((module, ns, name.to_string(), span, first));
            return;
        }
        self.table_mut(module, ns)
            .insert(name.to_string(), ItemSym { sym, vis, span });
        let reserved = match ns {
            Ns::Type => builtins::is_prelude_type_name(name),
            Ns::Value => builtins::is_prelude_value_name(name),
            Ns::Module => builtins::PRELUDE_MODULES.contains(&name),
        };
        if reserved {
            self.prelude_conflicts
                .push((module, ns, name.to_string(), span));
        }
    }

    pub(crate) fn table(&self, module: ModuleId, ns: Ns) -> &BTreeMap<String, ItemSym> {
        match ns {
            Ns::Type => &self.modules[module].types,
            Ns::Value => &self.modules[module].values,
            Ns::Module => &self.modules[module].modules,
        }
    }

    pub(crate) fn table_mut(&mut self, module: ModuleId, ns: Ns) -> &mut BTreeMap<String, ItemSym> {
        match ns {
            Ns::Type => &mut self.modules[module].types,
            Ns::Value => &mut self.modules[module].values,
            Ns::Module => &mut self.modules[module].modules,
        }
    }

    /// Recherche un nom dans un espace de noms (item local ou import).
    pub fn lookup(&self, module: ModuleId, ns: Ns, name: &str) -> Option<SymRef> {
        if let Some(item) = self.table(module, ns).get(name) {
            return Some(SymRef {
                sym: item.sym,
                vis: item.vis,
                def_module: module,
                import: None,
            });
        }
        for (index, imp) in self.modules[module].imports.iter().enumerate() {
            if imp.ns == ns && imp.name == name {
                // MOD-4.1/MOD-4.3: a `use` is an item of the importing module,
                // private like every item (`pub use` does not exist in Core
                // 1.0). Reached by name, it is visible in that module's subtree
                // only, whatever the visibility of what it imports.
                return Some(SymRef {
                    sym: imp.sym,
                    vis: Vis::Private,
                    def_module: module,
                    import: Some(index),
                });
            }
        }
        None
    }

    /// Marque l'import correspondant comme utilisé (si applicable).
    pub fn mark_used(&self, module: ModuleId, sym: SymRef) {
        if let Some(index) = sym.import {
            if let Some(imp) = self.modules[module].imports.get(index) {
                imp.mark_used();
            }
        }
    }

    /// Visibilité d'un item depuis `from` : public, ou module définissant et
    /// ses descendants.
    pub fn visible_from(&self, sym: &SymRef, from: ModuleId) -> bool {
        if sym.vis.is_public() || sym.def_module == ModuleId::MAX {
            return true;
        }
        let mut current = Some(from);
        while let Some(m) = current {
            if m == sym.def_module {
                return true;
            }
            current = self.modules[m].parent;
        }
        false
    }

    /// Module ancêtre à `levels` niveaux.
    pub fn ancestor(&self, module: ModuleId, levels: usize) -> Option<ModuleId> {
        let mut current = module;
        for _ in 0..levels {
            current = self.modules[current].parent?;
        }
        Some(current)
    }

    pub fn nominal_kind(&self, def: NodeId) -> Option<NominalKind> {
        if self.structs.contains_key(&def) {
            Some(NominalKind::Struct)
        } else if self.enums.contains_key(&def) {
            Some(NominalKind::Enum)
        } else {
            None
        }
    }

    pub fn method(&self, def: NodeId, name: &str, associated: bool) -> Option<&MethodEntry> {
        self.impls
            .get(&def)?
            .iter()
            .find(|m| m.name == name && m.is_associated() == associated)
    }

    pub fn type_generics(&self, def: NodeId) -> Vec<String> {
        if let Some(s) = self.structs.get(&def) {
            s.generics.iter().map(|g| g.name.name.clone()).collect()
        } else if let Some(e) = self.enums.get(&def) {
            e.generics.iter().map(|g| g.name.name.clone()).collect()
        } else {
            Vec::new()
        }
    }
}

// ---------------------------------------------------------------------------
// Identifiants et chemins Rust
// ---------------------------------------------------------------------------

pub(crate) fn join_path(parent: &str, name: &str) -> String {
    format!("{parent}::{name}")
}

/// Mangle un identifiant Ravum en identifiant Rust valide (`r#type`, ...).
pub(crate) fn rust_ident(name: &str) -> String {
    const RUST_KEYWORDS: &[&str] = &[
        "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate",
        "do", "dyn", "else", "enum", "extern", "false", "final", "fn", "for", "if", "impl", "in",
        "let", "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref",
        "return", "self", "Self", "static", "struct", "super", "trait", "true", "try", "type",
        "typeof", "union", "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
    ];
    if RUST_KEYWORDS.contains(&name) {
        match name {
            "self" | "Self" | "crate" | "super" => format!("{name}_"),
            _ => format!("r#{name}"),
        }
    } else {
        name.to_string()
    }
}

// ---------------------------------------------------------------------------
// Résolution de chemins de valeurs (partagée avec `typeck`)
// ---------------------------------------------------------------------------

/// Path of the only builtin constant ([`ValuePath::BuiltinNone`]).
pub(crate) const OPTION_NONE: &str = "Option::None";

/// A prelude constructor that takes a payload. `None`, the unit one, is
/// [`ValuePath::BuiltinNone`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PayloadCtor {
    Some,
    Ok,
    Err,
}

impl PayloadCtor {
    pub(crate) fn path(self) -> &'static str {
        match self {
            PayloadCtor::Some => "Option::Some",
            PayloadCtor::Ok => "Result::Ok",
            PayloadCtor::Err => "Result::Err",
        }
    }
}

/// Résultat d'une résolution de chemin de valeur.
#[derive(Debug, Clone)]
pub(crate) enum ValuePath {
    Module,
    BuiltinModule,
    /// Fonction ou constante utilisateur.
    Item(NodeId),
    /// Type utilisé comme valeur (erreur de typage).
    Type(Type),
    Variant {
        enum_item: NodeId,
        variant: usize,
    },
    /// Fonction builtin libre (`std::io::println`, `drop`, ...).
    BuiltinFn(&'static str),
    /// The only builtin constant, `Option::None` (path [`OPTION_NONE`]).
    BuiltinNone,
    /// Constructeur de variante builtin (`Option::Some`, `Result::Ok`, ...).
    /// Its only unit variant, `None`, is [`ValuePath::BuiltinNone`].
    BuiltinCtor {
        ctor: PayloadCtor,
    },
    /// Fonction associée utilisateur (`Type::new`).
    AssocFn(NodeId),
}

/// Erreur de résolution d'un chemin (code stable + message).
#[derive(Debug, Clone)]
pub(crate) struct PathError {
    pub code: &'static str,
    pub message: String,
}
