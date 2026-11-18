//! Typage : inférence déterministe, vérification des expressions, patterns,
//! exhaustivité, appels, méthodes builtin/utilisateur et casts.
//!
//! Règles de conception appliquées (décisions du Lead, canoniques) :
//! * littéraux : typage contextuel immédiat, défaut `i32`/`f64` ; pas
//!   d'unification rétroactive du type d'une variable déjà liée ;
//! * aucune coercition implicite sauf `&[T; N] → &[T]` et `&String → &str`, en
//!   argument et en récepteur de méthode ;
//! * `!` (`Type::Never`) se convertit vers n'importe quel type attendu ;
//! * `for` itère un range entier, `Vec<T>` par valeur (`x: T`), `&Vec<T>` et
//!   `&[T]` par référence (`x: &T`) ;
//! * `[v; n]` exige une longueur constante entière (`E-TYP-ARRAY-REPEAT`).
//!
//! Le module remplit : `expr_types`, `pat_types`, `type_nodes`, `resolutions`,
//! `calls`, `methods`, `consts`, `main_fn` (les signatures d'items le sont par
//! `resolve`). Un programme accepté n'a aucun nœud d'expression non typé.

use crate::builtins::{self, MethodSelfKind};
use crate::facts::{
    CallTarget, CheckedProgram, ConstInfo, ConstValue, Facts, MethodTarget, Resolution,
};
use crate::resolve::{
    self, ModuleId, NominalKind, PathError, PayloadCtor, ValuePath, World, OPTION_NONE,
};
use crate::types::{contains_never, Type, MAX_TYPE_DEPTH};
use ravum_lang_syntax::ast::{
    self, Block, ConstItem, Expr, ExprKind, FieldInit, FloatTy, FnItem, IntTy, Item, ItemKind,
    LitPat, MatchArm, Module, NodeId, Pat, PatKind, Path, PathRoot, Receiver, Stmt, StmtKind,
    Type as AstType, UnOp, VariantPatFields,
};
use ravum_lang_syntax::diag::{Diagnostic, DiagnosticBag};
use ravum_lang_syntax::source::SourceMap;
use ravum_lang_syntax::span::Span;
use std::collections::{BTreeMap, BTreeSet};

/// Point d'entrée public : reconstruit le monde puis type le programme.
pub fn typecheck(checked: &mut CheckedProgram, diags: &mut DiagnosticBag) -> bool {
    let world = World::build(&checked.program);
    typecheck_with_world(&checked.program, &mut checked.facts, &world, None, diags)
}

/// Variante réutilisant le monde de la phase de résolution.
pub(crate) fn typecheck_with_world(
    program: &ast::Program,
    facts: &mut Facts,
    world: &World,
    sources: Option<&SourceMap>,
    diags: &mut DiagnosticBag,
) -> bool {
    let mut checker = TypeChecker::new(program, facts, world, diags);
    checker.run(sources);
    compute_static_str_fns(program, &mut *checker.facts);
    resolve::emit_unused_import_warnings(world, checker.diags);
    !checker.diags.has_errors()
}

impl<'p, 'w, 'f> TypeChecker<'p, 'w, 'f> {
    fn new(
        program: &'p ast::Program,
        facts: &'f mut Facts,
        world: &'w World<'p>,
        diags: &'f mut DiagnosticBag,
    ) -> Self {
        TypeChecker {
            program,
            facts,
            world,
            diags,
            module: world.root,
            generics: Vec::new(),
            self_ty: None,
            fn_ret: Type::Unit,
            current_fn: None,
            scopes: Vec::new(),
            loop_depth: 0,
            value_position: 0,
            unsafe_depth: 0,
            in_const: false,
            variant_pats: BTreeMap::new(),
            unused_candidates: Vec::new(),
            used_locals: BTreeSet::new(),
            vars: Vec::new(),
            var_spans: Vec::new(),
            expr_spans: BTreeMap::new(),
        }
    }
}

// ----------------------------------------------------------------------
// Provenance interprocédurale du texte statique (`&'static str`)
// ----------------------------------------------------------------------
//
// Ferme le finding d'audit F1 : une fonction `fn lit() -> &str { "a" }` ne
// peut retourner qu'un texte statique en Core 1.0 (retourner un paramètre
// `&str` est `E-OWN-REF-ESCAPE`). On calcule par point fixe l'ensemble des
// fonctions dont le corps retourne *certainement* un texte statique, pour que
// `own::is_static_ref` puisse accepter `let t = lit();`. Le calcul est
// volontairement conservateur : toute construction non reconnue fait échouer
// la classification (mieux vaut un faux positif qu'une référence pendante).
//
// Règles de classification :
// * ne considérer que les fonctions dont le type de retour est exactement
//   `&str` ;
// * tous les `return` explicites du corps doivent être statiques ;
// * si le corps a une expression de queue, elle doit être statique ; sinon il
//   faut au moins un `return` explicite (une fonction `&str` valide ne peut pas
//   retomber sur `()`);
// * une expression statique est : un littéral `Str`, un `if`/`match`/bloc dont
//   toutes les branches et la queue sont statiques, ou un appel à une fonction
//   déjà présente dans l'ensemble (fixpoint ; aucun builtin).
// Limite assumée : les variables locales (`let t = "a"; return t;`) ne sont pas
// suivies ici ; leur usage *interne* reste accepté par `own.rs`, mais une telle
// fonction n'est pas classée statique pour ses appelants.

mod aggregates;
mod calls;
mod consts;
mod control;
mod core;
mod coverage;
mod expr;
mod helpers;
mod places;
mod static_str;

use helpers::*;
use static_str::*;

#[derive(Clone, Debug)]
struct Local {
    ty: Type,
    mutable: bool,
    /// `let x: T;` sans initialiseur : l'affectation initiale est permise même
    /// sans `mut` (l'initialisation définie est vérifiée par `own`).
    deferred: bool,
    /// Declaration span, mirroring `own.rs`'s slot identity (the `Pat` span for
    /// a pattern binding, the enclosing item span for a parameter or `self`).
    decl: Span,
}

/// Les variables d'inférence utilisent `Type::Param` avec un index réservé :
/// les vrais paramètres génériques ont un index < `VAR_BASE`.
const VAR_BASE: usize = 1_000_000;

fn infer_var_index(ty: &Type) -> Option<usize> {
    match ty {
        Type::Param { index, .. } if *index >= VAR_BASE => Some(*index - VAR_BASE),
        _ => None,
    }
}

fn has_infer_var_rec(ty: &Type) -> bool {
    if infer_var_index(ty).is_some() {
        return true;
    }
    match ty {
        Type::Ref { inner, .. } | Type::Array { elem: inner, .. } => has_infer_var_rec(inner),
        Type::Slice(inner) | Type::Vec(inner) | Type::Option(inner) => has_infer_var_rec(inner),
        Type::Tuple(items) => items.iter().any(has_infer_var_rec),
        Type::Result(ok, err) => has_infer_var_rec(ok) || has_infer_var_rec(err),
        Type::Nominal { args, .. } => args.iter().any(has_infer_var_rec),
        _ => false,
    }
}

/// An inherent method queued for checking after its module's free functions.
type PendingMethod<'p> = (ModuleId, NodeId, Span, &'p FnItem, Type, Vec<String>);

struct TypeChecker<'p, 'w, 'f> {
    program: &'p ast::Program,
    facts: &'f mut Facts,
    world: &'w World<'p>,
    diags: &'f mut DiagnosticBag,
    module: ModuleId,
    generics: Vec<(String, usize)>,
    self_ty: Option<Type>,
    fn_ret: Type,
    current_fn: Option<NodeId>,
    scopes: Vec<BTreeMap<String, Local>>,
    loop_depth: usize,
    /// > 0 while checking a nested value position (argument, field, assignment
    /// > right-hand side, match arm/scrutinee, array repeat). EXPR-5.11/6.7/G-6.19
    /// > forbid `while`/`loop`/`for` as value expressions; only a direct statement
    /// > (or a `()`-block tail) may contain one.
    value_position: usize,
    /// Profondeur d'imbrication des blocs `unsafe` (>= 1 = contexte unsafe).
    unsafe_depth: usize,
    in_const: bool,
    /// Variante résolue pour chaque pattern (`pat.id -> (enum, index)`), utilisé
    /// par l'analyse d'exhaustivité.
    variant_pats: BTreeMap<NodeId, (NodeId, usize)>,
    /// `let` bindings of the current function not resolved by any path yet, and
    /// the local names actually resolved. Compared at the end of the function to
    /// emit `W-OWN-UNUSED-VARIABLE` (DIAG-1.6 lists it among the two warnings).
    unused_candidates: Vec<(String, Span)>,
    used_locals: BTreeSet<String>,
    /// Variables d'inférence (index >= [`VAR_BASE`]), liées par unification.
    vars: Vec<Option<Type>>,
    var_spans: Vec<Span>,
    /// Span of every checked expression, to place `E-TYP-DEPTH` once the
    /// final types are known (`finalize_inference`).
    expr_spans: BTreeMap<NodeId, Span>,
}

impl<'p, 'w, 'f> TypeChecker<'p, 'w, 'f> {
    // ------------------------------------------------------------------
    // Diagnostics
    // ------------------------------------------------------------------
}

/// G-6.36: a float literal cannot be a Rust pattern, so codegen lowers it to a
/// guard. A guarded arm never contributes to exhaustiveness, so such a pattern
/// must not make the checker believe a variant is covered (`Some(1.5)` does not
/// cover `Some`); this predicate keeps the two phases consistent.
fn pattern_contains_float(pat: &Pat) -> bool {
    match &pat.kind {
        PatKind::Lit(LitPat::Float { .. }) => true,
        PatKind::Tuple(items) => items.iter().any(pattern_contains_float),
        PatKind::Variant { fields, .. } => match fields {
            VariantPatFields::Tuple(items) => items.iter().any(pattern_contains_float),
            VariantPatFields::Struct(fields) => fields
                .iter()
                .any(|field| pattern_contains_float(&field.pat)),
        },
        _ => false,
    }
}

/// Payload pattern of a one-field prelude constructor (`Some(p)`, `Ok(p)`,
/// `Err(p)`). A malformed arity is already reported by `check_pattern`; a
/// missing payload is treated as a wildcard.
fn payload_1(pat: &Pat) -> Option<&Pat> {
    match &pat.kind {
        PatKind::Variant {
            fields: VariantPatFields::Tuple(items),
            ..
        } => items.first(),
        _ => None,
    }
}

#[derive(PartialEq, Eq)]
enum PatternKind {
    Some,
    None,
    Ok,
    Err,
    Wild,
    Other,
}

fn pattern_kind(pat: &Pat) -> PatternKind {
    match &pat.kind {
        PatKind::Wild | PatKind::Binding { .. } => PatternKind::Wild,
        PatKind::Path(path) => path_pattern_kind(path),
        PatKind::Variant { path, .. } => path_pattern_kind(path),
        _ => PatternKind::Other,
    }
}

fn path_pattern_kind(path: &Path) -> PatternKind {
    match path.last().map(|s| s.name.name.as_str()) {
        Some("Some") => PatternKind::Some,
        Some("None") => PatternKind::None,
        Some("Ok") => PatternKind::Ok,
        Some("Err") => PatternKind::Err,
        _ => PatternKind::Other,
    }
}

#[cfg(test)]
mod forged_tests;
#[cfg(test)]
mod helper_tests;
