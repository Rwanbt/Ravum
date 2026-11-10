//! Résolution des noms : arbre de modules, tables d'items, `use`, visibilité,
//! espaces de noms et enregistrement des signatures dans [`Facts`].
//!
//! ## Modèle
//!
//! * Un arbre de modules est construit à partir de `Program::root` et des
//!   modules inline. Les `mod foo;` non expansés par le loader sont une erreur
//!   (`E-RES-MODDECL`) : le checker ne lit jamais le disque.
//! * Trois espaces de noms indépendants : types (structs, enums), valeurs
//!   (fonctions, constantes) et modules. Un doublon **dans le même** espace est
//!   `E-RES-DUPLICATE`.
//! * Les noms du prélude (`String`, `Vec`, `Option`, `Result`, `Error`,
//!   primitifs, `panic`, `assert`, `assert_eq`, `drop`, `Some`/`None`/`Ok`/
//!   `Err`, modules `io`/`fs`/...) sont réservés : les redéfinir est
//!   `E-RES-CONFLICT`.
//! * Visibilité : privé par défaut ; un item privé n'est visible que dans son
//!   module et ses descendants. L'accès depuis un autre module est
//!   `E-RES-PRIVATE`.
//! * `use` : import simple uniquement (`use crate::foo::Bar;`), pas de glob, ni
//!   d'alias, ni de ré-export (`pub use` → `E-UNSUPPORTED-REEXPORT`). Les
//!   imports inutilisés produisent `W-RES-UNUSED-IMPORT` en fin de typage.
//!
//! Ce module remplit `Facts::item_paths`, `Facts::module_paths`,
//! `Facts::fn_sigs`, `Facts::structs`, `Facts::enums`, `Facts::type_nodes` et
//! `Facts::test_fns`. Les constantes (`Facts::consts`) sont évaluées et
//! enregistrées par `typeck` (leur valeur fait partie du fait).

use crate::builtins;
use crate::facts::{
    Effect, EffectSet, EnumInfo, Facts, FieldInfo, FnSig, ParamSig, StructInfo, VariantInfo,
    VariantKind,
};
use crate::types::Type;
use ravum_lang_syntax::ast::{
    self, ConstItem, EnumItem, Expr, FnItem, Item, ItemKind, Module, NodeId, Path, PathRoot,
    Receiver, StructItem, Type as AstType, TypeKind, UseItem, VariantDefFields, Vis,
};
use ravum_lang_syntax::diag::{Diagnostic, DiagnosticBag};
use ravum_lang_syntax::source::SourceMap;
use ravum_lang_syntax::span::Span;
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) type ModuleId = usize;

// ---------------------------------------------------------------------------
// Monde résolu (tables d'items partagées par `resolve`, `typeck` et `effects`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ns {
    Type,
    Value,
    Module,
}

/// Symbole d'une table d'items ou d'un import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sym {
    Fn(NodeId),
    Struct(NodeId),
    Enum(NodeId),
    Const(NodeId),
    Module(ModuleId),
    /// Module std (`std::io`, ...).
    BuiltinModule(&'static str),
    /// Fonction libre std/prélude (`std::io::println`, `drop`, ...).
    BuiltinFn(&'static str),
}

impl Sym {
    pub fn ns(&self) -> Ns {
        match self {
            Sym::Fn(_) | Sym::Const(_) | Sym::BuiltinFn(_) => Ns::Value,
            Sym::Struct(_) | Sym::Enum(_) => Ns::Type,
            Sym::Module(_) | Sym::BuiltinModule(_) => Ns::Module,
        }
    }

    pub fn item(&self) -> Option<NodeId> {
        match self {
            Sym::Fn(id) | Sym::Struct(id) | Sym::Enum(id) | Sym::Const(id) => Some(*id),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Sym::Fn(_) => "function",
            Sym::Struct(_) => "struct",
            Sym::Enum(_) => "enum",
            Sym::Const(_) => "constant",
            Sym::Module(_) | Sym::BuiltinModule(_) => "module",
            Sym::BuiltinFn(_) => "function",
        }
    }
}

/// Item d'un module, dans son espace de noms.
#[derive(Debug, Clone)]
pub(crate) struct ItemSym {
    pub sym: Sym,
    pub vis: Vis,
    pub span: Span,
}

/// Import `use` d'un module.
#[derive(Debug)]
pub(crate) struct ImportEntry {
    pub name: String,
    pub ns: Ns,
    pub sym: Sym,
    pub vis: Vis,
    pub def_module: ModuleId,
    pub span: Span,
    pub display: String,
    pub used: Cell<bool>,
}

impl ImportEntry {
    pub fn mark_used(&self) {
        self.used.set(true);
    }
}

/// Résultat d'une recherche de nom (item local ou import).
#[derive(Debug, Clone, Copy)]
pub(crate) struct SymRef {
    pub sym: Sym,
    pub vis: Vis,
    pub def_module: ModuleId,
    /// Index dans `ModuleInfo::imports` quand le symbole vient d'un `use`.
    pub import: Option<usize>,
}

/// Module de l'arbre.
#[derive(Debug)]
pub(crate) struct ModuleInfo {
    pub node: NodeId,
    pub parent: Option<ModuleId>,
    pub name: String,
    /// Chemin Rust (`crate`, `crate::foo`).
    pub path: String,
    pub types: BTreeMap<String, ItemSym>,
    pub values: BTreeMap<String, ItemSym>,
    pub modules: BTreeMap<String, ItemSym>,
    pub imports: Vec<ImportEntry>,
}

/// Méthode ou fonction associée d'un `impl`.
#[derive(Debug, Clone)]
pub(crate) struct MethodEntry {
    pub item: NodeId,
    pub name: String,
    pub def_module: ModuleId,
    pub vis: Vis,
    pub receiver: Option<Receiver>,
    /// Paramètres génériques de l'`impl`, dans l'ordre.
    pub impl_generics: Vec<String>,
    /// Type cible de l'`impl`, avec `Type::Param { index }` pour ses génériques.
    pub self_ty: Type,
}

impl MethodEntry {
    pub fn is_associated(&self) -> bool {
        self.receiver.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NominalKind {
    Struct,
    Enum,
}

/// Monde résolu : modules, tables d'items, impls, diagnostics structurels.
///
/// Le monde emprunte l'AST (`&'a Module`, `&'a FnItem`, ...) : il ne possède
/// que les noms et les types résolus.
pub(crate) struct World<'a> {
    pub modules: Vec<ModuleInfo>,
    pub module_asts: Vec<&'a Module>,
    pub fns: BTreeMap<NodeId, &'a FnItem>,
    /// Fonctions déclarées dans un bloc `extern "C" { ... }` (sans corps).
    pub extern_fns: BTreeMap<NodeId, &'a ast::ExternFn>,
    pub structs: BTreeMap<NodeId, &'a StructItem>,
    pub enums: BTreeMap<NodeId, &'a EnumItem>,
    pub consts: BTreeMap<NodeId, &'a ConstItem>,
    pub impl_items: Vec<(ModuleId, &'a Item, &'a ast::ImplItem)>,
    pub use_items: Vec<(ModuleId, &'a Item, &'a UseItem)>,
    pub module_of_node: BTreeMap<NodeId, ModuleId>,
    pub item_module: BTreeMap<NodeId, ModuleId>,
    /// Méthodes par type nominal (clé = id de l'item struct/enum).
    pub impls: BTreeMap<NodeId, Vec<MethodEntry>>,
    pub mod_decls: Vec<(ModuleId, NodeId, Span, String)>,
    pub duplicates: Vec<(ModuleId, Ns, String, Span, Span)>,
    pub prelude_conflicts: Vec<(ModuleId, Ns, String, Span)>,
    pub test_fns: Vec<NodeId>,
    pub root: ModuleId,
}

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
            name: String::new(),
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

    fn build_module(&mut self, module: &'a Module, id: ModuleId) {
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
                        name: name.clone(),
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

    fn register(&mut self, module: ModuleId, ns: Ns, name: &str, sym: Sym, vis: Vis, span: Span) {
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

    fn table(&self, module: ModuleId, ns: Ns) -> &BTreeMap<String, ItemSym> {
        match ns {
            Ns::Type => &self.modules[module].types,
            Ns::Value => &self.modules[module].values,
            Ns::Module => &self.modules[module].modules,
        }
    }

    fn table_mut(&mut self, module: ModuleId, ns: Ns) -> &mut BTreeMap<String, ItemSym> {
        match ns {
            Ns::Type => &mut self.modules[module].types,
            Ns::Value => &mut self.modules[module].values,
            Ns::Module => &mut self.modules[module].modules,
        }
    }

    pub fn module(&self, id: ModuleId) -> &ModuleInfo {
        &self.modules[id]
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
                return Some(SymRef {
                    sym: imp.sym,
                    vis: imp.vis,
                    def_module: imp.def_module,
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

/// Résultat d'une résolution de chemin de valeur.
#[derive(Debug, Clone)]
pub(crate) enum ValuePath {
    Module(ModuleId),
    BuiltinModule(&'static str),
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
    /// Constante builtin (`Option::None`).
    BuiltinConst(&'static str),
    /// Constructeur de variante builtin (`Option::Some`, `Result::Ok`, ...).
    BuiltinCtor {
        path: &'static str,
        /// `true` pour une variante unitaire.
        unit: bool,
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

impl PathError {
    fn unknown(name: &str) -> Self {
        Self {
            code: "E-RES-UNKNOWN",
            message: format!("unknown name `{name}`"),
        }
    }

    fn private(name: &str) -> Self {
        Self {
            code: "E-RES-PRIVATE",
            message: format!("`{name}` is private"),
        }
    }

    fn variant_unqualified(name: &str, enum_name: &str) -> Self {
        Self {
            code: "E-RES-UNKNOWN",
            message: format!(
                "variant `{name}` must be qualified as `{enum_name}::{name}` in Core 1.0"
            ),
        }
    }

    fn self_outside_impl() -> Self {
        Self {
            code: "E-RES-SELF",
            message: "`self` is only available in a method".to_string(),
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
    self_ty: Option<&Type>,
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
        }
        if let Some(p) = builtins::prelude_value_path(name) {
            return Ok(ValuePath::BuiltinFn(p));
        }
        match name.as_str() {
            "Some" => {
                return Ok(ValuePath::BuiltinCtor {
                    path: "Option::Some",
                    unit: false,
                })
            }
            "None" => return Ok(ValuePath::BuiltinConst("Option::None")),
            "Ok" => {
                return Ok(ValuePath::BuiltinCtor {
                    path: "Result::Ok",
                    unit: false,
                })
            }
            "Err" => {
                return Ok(ValuePath::BuiltinCtor {
                    path: "Result::Err",
                    unit: false,
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
            match first.as_str() {
                "crate" => {
                    index = 1;
                    Cursor::Module(world.root)
                }
                "self" => {
                    index = 1;
                    Cursor::Module(module)
                }
                "super" => {
                    let mut levels = 1usize;
                    while segments.get(levels).map(|s| s.name.name.as_str()) == Some("super") {
                        levels += 1;
                    }
                    index = levels;
                    match world.ancestor(module, levels) {
                        Some(parent) => Cursor::Module(parent),
                        None => return Err(PathError::unknown("super")),
                    }
                }
                "Self" => {
                    index = 1;
                    match self_ty {
                        Some(t) => Cursor::Type(t.clone()),
                        None => return Err(PathError::self_outside_impl()),
                    }
                }
                _ => {
                    if let Some(sym) = world.lookup(module, Ns::Module, first) {
                        world.mark_used(module, sym);
                        if !world.visible_from(&sym, module) {
                            return Err(PathError::private(first));
                        }
                        index = 1;
                        match sym.sym {
                            Sym::Module(m) => Cursor::Module(m),
                            Sym::BuiltinModule(p) => Cursor::Builtin(p),
                            _ => return Err(PathError::unknown(first)),
                        }
                    } else if let Some(p) = builtins::builtin_root_module(first) {
                        index = 1;
                        Cursor::Builtin(p)
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
            }
        }
    };

    loop {
        if index >= segments.len() {
            return match cursor {
                Cursor::Module(m) => Ok(ValuePath::Module(m)),
                Cursor::Builtin(p) => Ok(ValuePath::BuiltinModule(p)),
                Cursor::Type(t) => Ok(ValuePath::Type(t)),
            };
        }
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
                        Sym::Module(child) => Ok(ValuePath::Module(child)),
                        Sym::BuiltinModule(p) => Ok(ValuePath::BuiltinModule(p)),
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
                        let module = builtins::BUILTIN_MODULES
                            .iter()
                            .copied()
                            .find(|m| *m == candidate)
                            .unwrap_or(p);
                        return Ok(ValuePath::BuiltinModule(module));
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
    fn with_enum(self, _enum_item: NodeId) -> PathError {
        self
    }
}

/// Type conteneur du prélude utilisé comme préfixe d'un chemin associé.
fn prelude_container(name: &str) -> Option<Type> {
    match name {
        "Vec" => Some(Type::Vec(Box::new(Type::Error))),
        "Option" => Some(Type::Option(Box::new(Type::Error))),
        "Result" => Some(Type::Result(Box::new(Type::Error), Box::new(Type::Error))),
        _ => None,
    }
}

fn type_of_symbol(world: &World, sym: Sym) -> Type {
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

fn find_unqualified_variant(world: &World, name: &str) -> Option<(NodeId, String)> {
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

fn resolve_associated(world: &World, ty: Type, name: &str) -> Result<ValuePath, PathError> {
    match &ty {
        Type::Nominal { def, .. } if *def == builtins::PRELUDE_ERROR_DEF => {
            builtin_assoc("Error", name)
        }
        Type::Nominal { def, .. } => {
            if world.nominal_kind(*def) == Some(NominalKind::Enum) {
                if let Some(enum_ast) = world.enums.get(def) {
                    if let Some(index) = enum_ast.variants.iter().position(|v| v.name.name == name)
                    {
                        return Ok(ValuePath::Variant {
                            enum_item: *def,
                            variant: index,
                        });
                    }
                }
            }
            if let Some(entry) = world.method(*def, name, true) {
                return Ok(ValuePath::AssocFn(entry.item));
            }
            Err(PathError::unknown(name))
        }
        Type::Option(_) => match name {
            "Some" => Ok(ValuePath::BuiltinCtor {
                path: "Option::Some",
                unit: false,
            }),
            "None" => Ok(ValuePath::BuiltinConst("Option::None")),
            _ => Err(PathError::unknown(name)),
        },
        Type::Result(_, _) => match name {
            "Ok" => Ok(ValuePath::BuiltinCtor {
                path: "Result::Ok",
                unit: false,
            }),
            "Err" => Ok(ValuePath::BuiltinCtor {
                path: "Result::Err",
                unit: false,
            }),
            _ => Err(PathError::unknown(name)),
        },
        Type::String => builtin_assoc("String", name),
        Type::Vec(_) => builtin_assoc("Vec", name),
        _ => Err(PathError::unknown(name)),
    }
}

fn builtin_assoc(type_name: &str, name: &str) -> Result<ValuePath, PathError> {
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

fn report_structural_errors(world: &World, diags: &mut DiagnosticBag) {
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

fn fill_module_paths(world: &World, facts: &mut Facts) {
    for module in &world.modules {
        facts.module_paths.insert(module.node, module.path.clone());
        facts.item_paths.insert(module.node, module.path.clone());
    }
}

fn fill_item_paths(world: &World, facts: &mut Facts) {
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

fn resolve_imports(world: &mut World, diags: &mut DiagnosticBag) {
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
                        vis,
                        def_module,
                        span: use_item.span,
                        display,
                        used: Cell::new(false),
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
                        vis: Vis::Public(Span::dummy()),
                        def_module: ModuleId::MAX,
                        span: use_item.span,
                        display,
                        used: Cell::new(false),
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
                        vis: Vis::Public(Span::dummy()),
                        def_module: ModuleId::MAX,
                        span: use_item.span,
                        display,
                        used: Cell::new(false),
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
                        vis,
                        def_module,
                        span: use_item.span,
                        display,
                        used: Cell::new(false),
                    },
                    diags,
                );
            }
            UseTarget::Variant => {
                diags.error(
                    "E-RES-UNKNOWN",
                    use_item.span,
                    format!("variant import `{display}` is not supported in Core 1.0"),
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

fn last_segment(path: &Path) -> String {
    path.last()
        .map(|s| s.name.name.clone())
        .unwrap_or_else(|| path.display())
}

fn entry_visible(world: &World, vis: Vis, def_module: ModuleId, from: ModuleId) -> bool {
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

fn insert_import(
    world: &mut World,
    module: ModuleId,
    entry: ImportEntry,
    diags: &mut DiagnosticBag,
) {
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

enum UseTarget {
    Module(Sym, Vis, ModuleId),
    BuiltinModule(&'static str),
    BuiltinFn(&'static str),
    Symbol(Sym, Vis, ModuleId),
    Variant,
    Unknown,
}

enum UseCursor {
    Module(ModuleId),
    Builtin(&'static str),
}

fn resolve_use_target(world: &World, module: ModuleId, path: &Path) -> UseTarget {
    let segments = &path.segments;
    if segments.is_empty() {
        return UseTarget::Unknown;
    }

    // `use x;` : import depuis le module courant.
    if path.root == PathRoot::Plain && segments.len() == 1 {
        let name = &segments[0].name.name;
        if let Some(r) = world.lookup(module, Ns::Value, name) {
            world.mark_used(module, r);
            return finish_use_symbol(r.sym, r.vis, r.def_module);
        }
        if let Some(r) = world.lookup(module, Ns::Type, name) {
            world.mark_used(module, r);
            return finish_use_symbol(r.sym, r.vis, r.def_module);
        }
        if let Some(r) = world.lookup(module, Ns::Module, name) {
            world.mark_used(module, r);
            return match r.sym {
                Sym::Module(m) => UseTarget::Module(r.sym, r.vis, m),
                Sym::BuiltinModule(p) => UseTarget::BuiltinModule(p),
                _ => UseTarget::Unknown,
            };
        }
        return UseTarget::Unknown;
    }

    let mut index = 0usize;
    let mut cursor = match path.root {
        PathRoot::Crate => UseCursor::Module(world.root),
        PathRoot::SelfMod => UseCursor::Module(module),
        PathRoot::Super => match world.ancestor(module, 1) {
            Some(parent) => UseCursor::Module(parent),
            None => return UseTarget::Unknown,
        },
        PathRoot::Plain => {
            let first = &segments[0].name.name;
            match first.as_str() {
                "crate" => {
                    index = 1;
                    UseCursor::Module(world.root)
                }
                "self" => {
                    index = 1;
                    UseCursor::Module(module)
                }
                "super" => {
                    let mut levels = 1usize;
                    while segments.get(levels).map(|s| s.name.name.as_str()) == Some("super") {
                        levels += 1;
                    }
                    index = levels;
                    match world.ancestor(module, levels) {
                        Some(parent) => UseCursor::Module(parent),
                        None => return UseTarget::Unknown,
                    }
                }
                _ => {
                    if let Some(r) = world.lookup(module, Ns::Module, first) {
                        world.mark_used(module, r);
                        index = 1;
                        match r.sym {
                            Sym::Module(m) => UseCursor::Module(m),
                            Sym::BuiltinModule(p) => UseCursor::Builtin(p),
                            _ => return UseTarget::Unknown,
                        }
                    } else if let Some(p) = builtins::builtin_root_module(first) {
                        index = 1;
                        UseCursor::Builtin(p)
                    } else {
                        return UseTarget::Unknown;
                    }
                }
            }
        }
    };

    loop {
        if index >= segments.len() {
            return match cursor {
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
                        Sym::Module(_) | Sym::BuiltinModule(_) => match r.sym {
                            Sym::Module(m2) => UseTarget::Module(r.sym, r.vis, m2),
                            Sym::BuiltinModule(p) => UseTarget::BuiltinModule(p),
                            _ => UseTarget::Unknown,
                        },
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

fn finish_use_symbol(sym: Sym, vis: Vis, def_module: ModuleId) -> UseTarget {
    match sym {
        Sym::BuiltinFn(p) => UseTarget::BuiltinFn(p),
        Sym::BuiltinModule(p) => UseTarget::BuiltinModule(p),
        Sym::Module(m) => UseTarget::Module(sym, vis, m),
        Sym::Fn(_) | Sym::Const(_) | Sym::Struct(_) | Sym::Enum(_) => {
            UseTarget::Symbol(sym, vis, def_module)
        }
    }
}

// ---------------------------------------------------------------------------
// Signatures d'items
// ---------------------------------------------------------------------------

fn resolve_headers(world: &mut World, facts: &mut Facts, diags: &mut DiagnosticBag) {
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

struct HeaderCtx<'w, 'a, 'f> {
    world: &'w World<'a>,
    facts: &'f mut Facts,
    diags: &'f mut DiagnosticBag,
    module: ModuleId,
    generics: Vec<(String, usize)>,
    self_ty: Option<Type>,
}

impl<'w, 'a, 'f> HeaderCtx<'w, 'a, 'f> {
    fn new(
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

    fn error(&mut self, code: &'static str, span: Span, message: String) {
        self.diags.error(code, span, message);
    }

    fn resolve_type(&mut self, ty: &AstType) -> Type {
        resolve_type_syntax(
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
}

fn generic_scope(diags: &mut DiagnosticBag, params: &[ast::GenericParam]) -> Vec<(String, usize)> {
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
fn resolve_fn_header(
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
                });
            }
            ast::ParamKind::Binding { name, ty } => {
                if params.iter().any(|p| p.name == name.name) {
                    ctx.error(
                        "E-RES-DUPLICATE",
                        name.span,
                        format!("duplicate parameter `{}`", name.name),
                    );
                }
                let resolved = ctx.resolve_type(ty);
                params.push(ParamSig {
                    name: name.name.clone(),
                    ty: resolved,
                    is_receiver: false,
                });
            }
        }
    }

    let ret = ctx.resolve_type(&f.ret);

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
fn resolve_extern_block(
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
                    ast::ParamKind::Binding { name, ty } => {
                        if params.iter().any(|p| p.name == name.name) {
                            ctx.error(
                                "E-RES-DUPLICATE",
                                name.span,
                                format!("duplicate parameter `{}`", name.name),
                            );
                        }
                        let resolved = ctx.resolve_type(ty);
                        if !ffi_safe(&resolved) {
                            ctx.error(
                                "E-FFI-TYPE",
                                ty.span,
                                format!(
                                    "type `{}` is not FFI-safe in Core 1.0; extern parameters and returns must be integers, floats, `bool` or `()`",
                                    resolved.display()
                                ),
                            );
                        }
                        params.push(ParamSig {
                            name: name.name.clone(),
                            ty: resolved,
                            is_receiver: false,
                        });
                    }
                }
            }
            let ret = ctx.resolve_type(&f.ret);
            if !ffi_safe(&ret) {
                ctx.error(
                    "E-FFI-TYPE",
                    f.ret.span,
                    format!(
                        "type `{}` is not FFI-safe in Core 1.0; extern parameters and returns must be integers, floats, `bool` or `()`",
                        ret.display()
                    ),
                );
            }
            (params, ret)
        };
        let rust_path = format!("crate::rvm_ffi_wrap_{}", f.id);
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
fn ffi_safe(ty: &Type) -> bool {
    matches!(
        ty,
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
            | Type::Unit
    )
}

fn resolve_struct(
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
        },
    );
}

fn resolve_enum(
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
                    out.push(ctx.resolve_type(ty));
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

fn resolve_impl(
    world: &mut World,
    facts: &mut Facts,
    diags: &mut DiagnosticBag,
    module: ModuleId,
    item: &Item,
    imp: &ast::ImplItem,
) {
    let (target, generics) = {
        let mut ctx = HeaderCtx::new(world, facts, diags, module, Vec::new(), None);
        let generics = generic_scope(ctx.diags, &imp.generics);
        ctx.generics = generics.clone();
        let target = ctx.resolve_type(&imp.self_ty);
        let def = match &target {
            Type::Nominal { def, .. } if ctx.world.nominal_kind(*def).is_some() => *def,
            _ => {
                ctx.error(
                    "E-RES-IMPL-SELF-TYPE",
                    imp.self_ty.span,
                    "`impl` target must be a nominal type defined in this package".to_string(),
                );
                return;
            }
        };
        if let Type::Nominal { args, .. } = &target {
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
        }
        let _ = def;
        (target, generics)
    };
    let def = match &target {
        Type::Nominal { def, .. } => *def,
        _ => return,
    };
    let impl_generics: Vec<String> = generics.iter().map(|(n, _)| n.clone()).collect();
    world.impls.entry(def).or_default();
    for member in &imp.items {
        let f = match &member.kind {
            ItemKind::Fn(f) => f,
            _ => {
                diags.error(
                    "E-RES-IMPL-UNKNOWN",
                    member.span,
                    "only fn items are allowed inside an `impl` in Core 1.0".to_string(),
                );
                continue;
            }
        };
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
            impl_generics: impl_generics.clone(),
            self_ty: target.clone(),
        });
        let _ = item;
    }
}

// ---------------------------------------------------------------------------
// Résolution de types (partagée avec `typeck`)
// ---------------------------------------------------------------------------

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
                        "E-TYP-UNKNOWN-TYPE",
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

fn is_str_syntax(ty: &AstType) -> bool {
    match &ty.kind {
        TypeKind::Path(path) => {
            path.root == PathRoot::Plain
                && path.segments.len() == 1
                && path.segments[0].name.name == "str"
        }
        _ => false,
    }
}

fn array_length(
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
fn resolve_type_args(
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
fn report_str(segment: &ast::PathSegment, diags: &mut DiagnosticBag) -> Type {
    diags.error(
        "E-TYP-UNKNOWN-TYPE",
        segment.name.span,
        "`str` is not a type in Core 1.0; use `&str`".to_string(),
    );
    Type::Error
}

#[allow(clippy::too_many_arguments)]
fn resolve_type_path(
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

    // Cas `Self`.
    if path.root == PathRoot::Plain && segments[0].name.name == "Self" {
        if segments.len() != 1 {
            diags.error(
                "E-RES-UNKNOWN",
                path.span,
                "`Self` cannot be used as a module prefix".to_string(),
            );
            return Type::Error;
        }
        return match self_ty {
            Some(t) => t.clone(),
            None => {
                diags.error(
                    "E-RES-SELF-OUTSIDE-IMPL",
                    path.span,
                    "`Self` is only valid inside an `impl`".to_string(),
                );
                Type::Error
            }
        };
    }

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
            match first.as_str() {
                "crate" => {
                    index = 1;
                    Cursor::Module(world.root)
                }
                "self" => {
                    index = 1;
                    Cursor::Module(module)
                }
                "super" => {
                    let mut levels = 1usize;
                    while segments.get(levels).map(|s| s.name.name.as_str()) == Some("super") {
                        levels += 1;
                    }
                    index = levels;
                    match world.ancestor(module, levels) {
                        Some(parent) => Cursor::Module(parent),
                        None => {
                            diags.error(
                                "E-RES-UNKNOWN",
                                path.span,
                                "`super` escapes the crate root".to_string(),
                            );
                            return Type::Error;
                        }
                    }
                }
                _ => {
                    if let Some(sym) = world.lookup(module, Ns::Module, first) {
                        world.mark_used(module, sym);
                        index = 1;
                        match sym.sym {
                            Sym::Module(m) => Cursor::Module(m),
                            Sym::BuiltinModule(p) => Cursor::Builtin(p),
                            _ => return Type::Error,
                        }
                    } else if let Some(p) = builtins::builtin_root_module(first) {
                        index = 1;
                        Cursor::Builtin(p)
                    } else if world.lookup(module, Ns::Type, first).is_some()
                        || is_builtin_type_name(first)
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
            }
        }
    };

    loop {
        if index >= segments.len() {
            diags.error(
                "E-RES-UNKNOWN",
                path.span,
                "a module is not a type".to_string(),
            );
            return Type::Error;
        }
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
                                    diags.error(
                                        "E-RES-UNKNOWN",
                                        segment.name.span,
                                        format!("`{name}` is not a module"),
                                    );
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

fn is_builtin_type_name(name: &str) -> bool {
    matches!(name, "Vec" | "Option" | "Result") || builtins::prelude_type(name).is_some()
}

fn prelude_nominal(
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

fn nominal_type(
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

fn expect_arity(
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

fn compute_copy_and_recursive_types(world: &World, facts: &mut Facts, diags: &mut DiagnosticBag) {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CopyState {
    InProgress,
    Done(bool),
}

#[allow(clippy::too_many_arguments)]
fn nominal_is_copy(
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
fn type_is_copy(
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
