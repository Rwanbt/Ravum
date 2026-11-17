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
use crate::types::{contains_never, Type};
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

mod copy;
mod header;
mod paths;
mod uses;
mod world;

pub(crate) use copy::*;
pub(crate) use header::*;
pub use paths::resolve;
pub(crate) use paths::*;
pub(crate) use uses::*;
pub(crate) use world::*;
pub(crate) use world::{PathError, ValuePath};

#[cfg(test)]
mod sym_tests {
    use super::*;

    /// The noun used in "`x` is a …, not a value" diagnostics, and the
    /// namespace of each symbol kind.
    #[test]
    fn every_symbol_kind_has_a_label_and_a_namespace() {
        let cases = [
            (Sym::Fn(1), "function", Ns::Value),
            (Sym::Const(1), "constant", Ns::Value),
            (Sym::BuiltinFn("drop"), "function", Ns::Value),
            (Sym::Struct(1), "struct", Ns::Type),
            (Sym::Enum(1), "enum", Ns::Type),
            (Sym::Module(0), "module", Ns::Module),
            (Sym::BuiltinModule("std::io"), "module", Ns::Module),
        ];
        for (sym, label, ns) in cases {
            assert_eq!(sym.label(), label);
            assert_eq!(sym.ns(), ns);
        }
    }
}
