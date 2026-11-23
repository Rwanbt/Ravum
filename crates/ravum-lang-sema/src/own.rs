//! Ownership, move and borrow checking.
//!
//! OWNER: worker `own`. Implements the conservative model of
//! `spec/language/ownership.md`: initialisation, use-after-move, double move,
//! non-escaping borrows and borrow conflicts. The entry point is called by the
//! checker after typing; it must never mutate the AST or types.
//!
//! # Modèle volontairement conservateur
//!
//! Le but est la **sûreté** : mieux vaut refuser un programme valide que
//! d'accepter un programme non sûr. Les choix suivants sont assumés et
//! documentés ici pour que les refus ne soient pas des surprises :
//!
//! * **Fusions** : à un point de jonction (`if`, `match`, sortie de boucle),
//!   l'état d'une place est l'intersection des états des chemins. Si un seul
//!   chemin a déplacé (ou n'a pas initialisé), la place est considérée comme
//!   « peut-être déplacée » (resp. « peut-être non initialisée ») et tout usage
//!   ultérieur est refusé.
//! * **Boucles** : l'état d'entrée est un point fixe calculé sur le corps
//!   (itération silencieuse puis passe finale qui émet les diagnostics). Le
//!   corps d'une boucle est donc analysé comme s'il pouvait être exécuté après
//!   lui-même : un mobile déplacé dans une boucle est refusé à l'itération
//!   suivante (`E-OWN-USE-AFTER-MOVE`, note « déplacement possible à
//!   l'itération précédente »).
//! * **Prêts** : un prêt `&place` / `&mut place` reste actif jusqu'à la fin de
//!   l'instruction englobante ; les prêts créés dans les arguments ou le
//!   récepteur d'un appel expirent à la fin de cet appel (`spec/language/
//!   ownership.md` §3.1). Deux emprunts du même chemin (ou de préfixes
//!   imbriqués) sont en conflit dès que l'un est exclusif ; un déplacement
//!   pendant un prêt est un conflit.
//! * **Indexations** : une indexation par une expression non littérale est
//!   traitée comme non disjointe de toute autre indexation (question ouverte
//!   Q-OWN-5 du contrat) ; un indice littéral différent est considéré comme
//!   disjoint.
//! * **Non-échappement** : une référence non `&'static str` ne peut être ni
//!   liée par un `let`, ni liée par un pattern, ni placée dans un agrégat
//!   (tuple, array, struct, variante), ni affectée à une place de type
//!   référence, ni retournée. Les paramètres `&T` / `&mut T` restent utilisables
//!   dans tout le corps et peuvent être réempruntés (`&*p`, `&mut *p`) ; les
//!   littéraux `&str` et les variables initialisées par un littéral `&str`
//!   sont les seules références stockables.
//! * **Moves partiels** : le déplacement d'un champ de struct, d'un élément de
//!   tuple ou d'un élément de tableau est autorisé et suivi par projection
//!   (design corrigé). Le champ déplacé ne peut plus être lu
//!   (`E-OWN-USE-AFTER-MOVE`, `E-OWN-DOUBLE-MOVE` s'il est déplacé deux fois) ;
//!   les autres champs restent lisibles, empruntables et déplaçables ; utiliser
//!   l'agrégat entier après un déplacement partiel est `E-OWN-PARTIAL-MOVE`.
//!   Réaffecter exactement la projection déplacée la réinitialise. Un move de
//!   champ hors de `self` possédé (`fn into_titre(self) -> String`) est accepté.
//! * **Moves interdits** : le move à travers une référence (`*r` mobile) est
//!   `E-OWN-MOVE-OUT-OF-BORROW` ; le move d'un élément de `Vec` par indexation est
//!   `E-OWN-PARTIAL-MOVE` (`spec/language/ownership.md` §5.5). Le code retenu
//!   est celui du contrat (`E-OWN-MOVE-OUT-OF-BORROW`) ; `E-OWN-MOVE-OUT-OF-BORROW`,
//!   cité par la mission, n'est pas émis pour ne pas dupliquer un code.
//! * **Répétition de tableau** : `[e; n]` exige une valeur `Copy`, sinon
//!   `E-TYP-ARRAY-REPEAT` (OWN-2.5).
//! * **Non couvert volontairement** : mutabilité des places (`E-OWN-IMMUTABLE`,
//!   émise par le type-checker), l'ordre de destruction (non observable en
//!   Core 1.0), et la vérification des signatures de retour de référence
//!   (déléguée au type-checker ; le corps est vérifié).
//!
//! Robustesse : profondeur bornée, budget de diagnostics par fonction
//! (`E-OWN-LIMIT`), aucune panique sur un AST incomplet (les faits manquants
//! désactivent seulement la règle correspondante).

use crate::facts::{
    BindingId, CallTarget, CheckedProgram, Facts, MethodTarget, OwnFacts, OwnPlace, Projection,
    ProjectionIndex, Resolution, UseKind,
};
use crate::types::Type;
use ravum_lang_syntax::ast::{
    BinOp, Block, Expr, ExprKind, FnItem, Item, ItemKind, MatchArm, Module, NodeId, ParamKind, Pat,
    PatKind, Receiver, Stmt, StmtKind, UnOp,
};
use ravum_lang_syntax::diag::{Diagnostic, DiagnosticBag};
use ravum_lang_syntax::span::Span;
use std::collections::BTreeMap;

/// Bornes de robustesse / performance (P0-H).
///
/// The analysis enters up to three levels per syntactic nesting level (block,
/// statement, expression; an operand read as a place then as a value), and the
/// parser bounds the tree height at `ravum_lang_syntax::parser::MAX_DEPTH`
/// (256). Four times that bound cannot be reached by a parsed program; a tree
/// that reaches it anyway is refused with `E-OWN-LIMIT`, never skipped.
pub(super) const MAX_DEPTH: u32 = 1024;
pub(super) const MAX_FIXPOINT: u32 = 16;
/// Per-function borrow-error bound. It **must** stay strictly below the
/// per-file diagnostic budget (`ravum_lang_syntax::diag::DIAGNOSTIC_BUDGET`,
/// 100, DIAG-1.7/DIAG-4.3): otherwise the file budget would emit
/// `E-PARSE-TOO-LARGE` first and `E-OWN-LIMIT` could never be observed.
/// 50 keeps the limit reachable even when earlier phases already spent part of
/// the file budget.
pub(super) const MAX_DIAGS_PER_FN: u32 = 50;

pub(super) const E_UNINIT: &str = "E-OWN-UNINIT";
pub(super) const E_USE_AFTER_MOVE: &str = "E-OWN-USE-AFTER-MOVE";
pub(super) const E_DOUBLE_MOVE: &str = "E-OWN-DOUBLE-MOVE";
pub(super) const E_PARTIAL_MOVE: &str = "E-OWN-PARTIAL-MOVE";
pub(super) const E_MOVE_INDEXED: &str = "E-OWN-PARTIAL-MOVE";
pub(super) const E_MOVE_BORROWED: &str = "E-OWN-MOVE-OUT-OF-BORROW";
pub(super) const E_BORROW_CONFLICT: &str = "E-OWN-BORROW-CONFLICT";
pub(super) const E_REF_ESCAPE: &str = "E-OWN-REF-ESCAPE";
pub(super) const E_NOT_COPY: &str = "E-TYP-ARRAY-REPEAT";
pub(super) const E_LIMIT: &str = "E-OWN-LIMIT";
pub(super) const E_IMMUTABLE: &str = "E-OWN-IMMUTABLE";

/// Verify ownership rules on a typed program. Diagnostics are appended in source
/// order by the caller; this function only pushes them.
pub fn check_ownership(checked: &CheckedProgram, diags: &mut DiagnosticBag) -> OwnFacts {
    // OWN-6.1/6.2: a reference cannot be *stored* — a struct field or an enum
    // payload is such a storage. This is checked on the declaration because no
    // value could ever inhabit it: constructing one from a non-static reference
    // is already refused at the construction site.
    check_stored_reference_fields(&checked.program.root, &checked.facts, diags);
    let mut fns: Vec<(&Item, &FnItem)> = Vec::new();
    collect_fns(&checked.program.root, &mut fns);
    // The accumulator is shared by every function: each checker inserts its
    // decisions directly, so the cost is one ordered-map insert per recorded
    // expression. The previous shape gave every function its own table and
    // merged it with `BTreeMap::append` after each `check_fn`; `append` moves
    // the whole accumulated map on every call, which made the phase quadratic
    // in the number of functions (25 758 ms of the 26 424 ms `check` at
    // 50 110 lines, `docs/reports/LANG-TIMINGS.md` §8).
    let mut own = OwnFacts::default();
    // RFC-OWN-FACTS-2 step C1: the checker publishes its copy-ness decision once,
    // for every resolved type node, so the backend never re-derives it.
    publish_copy_type(&checked.facts, &mut own);
    for (item, f) in fns {
        if checked.facts.reuse_bodies.contains_key(&item.id) {
            continue; // T13 step 2: reused body
        }
        let mut checker = OwnChecker::new(&checked.facts, diags, &mut own);
        checker.check_fn(item, f);
    }
    own
}

/// RFC-OWN-FACTS-2 step C1: publish, for every resolved type node, whether the
/// ownership layer considers that type `Copy`.
///
/// This is the single place where copy-ness becomes a fact. It is a free
/// function rather than a method on `check_ownership` so that the codegen
/// fixtures, which assemble `Facts` by hand, publish exactly the same table as
/// production code — the step-B guard then compares like with like instead of
/// passing vacuously.
pub fn copy_types(facts: &Facts) -> std::collections::BTreeSet<NodeId> {
    let mut set = std::collections::BTreeSet::new();
    for (id, ty) in &facts.type_nodes {
        if is_copy_type(ty, facts) {
            set.insert(*id);
        }
    }
    set
}

/// Fill `OwnFacts::copy_type` from [`copy_types`].
pub fn publish_copy_type(facts: &Facts, own: &mut OwnFacts) {
    own.copy_type = copy_types(facts);
}

/// Walk every stored field type: struct fields and enum variant payloads.
pub(super) fn check_stored_reference_fields(
    module: &Module,
    facts: &Facts,
    diags: &mut DiagnosticBag,
) {
    for item in &module.items {
        match &item.kind {
            ItemKind::Struct(s) => {
                for field in &s.fields {
                    report_stored_reference(&field.ty, facts, diags);
                }
            }
            ItemKind::Enum(e) => {
                for variant in &e.variants {
                    match &variant.fields {
                        ravum_lang_syntax::ast::VariantDefFields::Unit => {}
                        ravum_lang_syntax::ast::VariantDefFields::Tuple(types) => {
                            for ty in types {
                                report_stored_reference(ty, facts, diags);
                            }
                        }
                        ravum_lang_syntax::ast::VariantDefFields::Struct(fields) => {
                            for field in fields {
                                report_stored_reference(&field.ty, facts, diags);
                            }
                        }
                    }
                }
            }
            ItemKind::Mod(inner) => check_stored_reference_fields(inner, facts, diags),
            _ => {}
        }
    }
}

pub(super) fn report_stored_reference(
    ty: &ravum_lang_syntax::ast::Type,
    facts: &Facts,
    diags: &mut DiagnosticBag,
) {
    let Some(sema_ty) = facts.type_nodes.get(&ty.id) else {
        return;
    };
    if !type_stores_nonstatic_reference(sema_ty) {
        return;
    }
    // The diagnostic points at the field type (`&i32` in `struct W { r: &i32 }`),
    // the escape site required by OWN-6.6.
    diags.error(
        E_REF_ESCAPE,
        ty.span,
        "une référence ne peut pas être stockée dans un champ ou une charge utile d'énumération"
            .to_string(),
    );
}

/// `true` for the borrowed-text type `&str` (`Type::Ref { mutable: false,
/// inner: Str }`). D13's staticity analysis only speaks about this type: it is
/// the single reference type Core 1.0 lets cross an instruction boundary.
pub(super) fn is_borrowed_text(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Ref {
            mutable: false,
            inner,
        } if matches!(**inner, Type::Str)
    )
}

/// `true` when the type stores a reference that is not the `&str` of a string
/// literal. OWN-6.1 forbids storing a reference in a field, a `Vec`, a tuple, an
/// `Option`/`Result`; OWN-6.2 makes `&'static str` the single exception, which
/// is why `&str` (the only nameable text reference, TYP-6.3) is allowed.
pub(super) fn type_stores_nonstatic_reference(ty: &Type) -> bool {
    match ty {
        Type::Ref { inner, .. } => !matches!(**inner, Type::Str),
        Type::Array { elem, .. } => type_stores_nonstatic_reference(elem),
        Type::Slice(inner) | Type::Vec(inner) | Type::Option(inner) => {
            type_stores_nonstatic_reference(inner)
        }
        Type::Tuple(items) => items.iter().any(type_stores_nonstatic_reference),
        Type::Result(ok, err) => {
            type_stores_nonstatic_reference(ok) || type_stores_nonstatic_reference(err)
        }
        Type::Nominal { args, .. } => args.iter().any(type_stores_nonstatic_reference),
        _ => false,
    }
}

pub(super) fn collect_fns<'a>(module: &'a Module, out: &mut Vec<(&'a Item, &'a FnItem)>) {
    for item in &module.items {
        match &item.kind {
            ItemKind::Fn(f) => out.push((item, f)),
            ItemKind::Mod(inner) => collect_fns(inner, out),
            ItemKind::Impl(imp) => {
                for (sub, f) in imp.methods() {
                    out.push((sub, f));
                }
            }
            _ => {}
        }
    }
}

pub(super) type SlotId = u32;

/// One projection on a place. `Deref` separates reference levels.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum Step {
    Deref,
    Field(String),
    Tuple(u32),
    Index(IndexKey),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum IndexKey {
    Lit(u128),
    /// Index computed from a non-literal expression: treated as possibly equal
    /// to every other index (conservative, Q-OWN-5).
    Opaque,
}

/// A place rooted at a local binding (`x`, `x.champ`, `x.0`, `v[i]`, `*r`, ...).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Place {
    pub(super) slot: SlotId,
    pub(super) steps: Vec<Step>,
}

/// Convert an internal projection to the public fact representation.
pub(super) fn projection(step: &Step) -> Projection {
    match step {
        Step::Deref => Projection::Deref,
        Step::Field(name) => Projection::Field(name.clone()),
        Step::Tuple(index) => Projection::Tuple(*index),
        Step::Index(IndexKey::Lit(value)) => Projection::Index(ProjectionIndex::Lit(*value)),
        Step::Index(IndexKey::Opaque) => Projection::Index(ProjectionIndex::Opaque),
    }
}

#[derive(Debug, Clone)]
pub(super) struct SlotInfo {
    pub(super) name: String,
    pub(super) ty: Type,
    /// True when the binding certainly holds a `&'static str` (a string literal
    /// or a copy of one), the only storable reference in Core 1.0.
    pub(super) static_ref: bool,
    pub(super) decl_span: Span,
    /// Program-wide identity of the binding (RFC-OWN-FACTS-2 step C4).
    pub(super) binding: BindingId,
    /// Declared without `mut` and without an initialiser: the binding takes
    /// its value from one assignment, and may not be assigned again.
    pub(super) assign_once: bool,
}

/// One binding introduced by a pattern, with the information needed to decide
/// whether a `&str` it receives is static (D13 rule 1).
#[derive(Debug, Clone)]
pub(super) struct PatBinding {
    pub(super) name: String,
    pub(super) ty: Type,
    pub(super) span: Span,
    /// `true` when the binding destructures a *component* of the matched value
    /// (tuple element, enum payload, struct field) instead of copying the value
    /// itself. A `&str` read out of an aggregate is static by construction:
    /// OWN-6.1/3.3 forbid storing a borrowed `&str` in any aggregate, and every
    /// storage site is checked (literals, struct literals, `Some`/`Ok`/`Err`,
    /// `push`, assignment, `let`, `return`).
    pub(super) nested: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PartialMove {
    pub(super) span: Span,
    pub(super) used_since: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct SlotState {
    /// Definitely initialised on every path reaching this point.
    pub(super) init: bool,
    /// Definitely moved (on every path).
    pub(super) moved: bool,
    /// Moved on at least one path.
    pub(super) maybe_moved: bool,
    /// Span of the (last) move, for diagnostics.
    pub(super) move_span: Option<Span>,
    /// A use happened after the recorded move (distinguishes double move from
    /// use-after-move in diagnostics).
    pub(super) used_since: bool,
    /// Projections that may have been partially moved, with their move record.
    pub(super) partial: BTreeMap<Vec<Step>, PartialMove>,
    /// Assigned on at least one path since its declaration (only tracked for
    /// the bindings that may be assigned once).
    pub(super) maybe_assigned: bool,
}

impl SlotState {
    pub(super) fn initialised() -> Self {
        SlotState {
            init: true,
            ..SlotState::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct State {
    pub(super) slots: BTreeMap<SlotId, SlotState>,
}

#[derive(Debug, Clone)]
pub(super) struct Loan {
    pub(super) place: Place,
    pub(super) mutable: bool,
    pub(super) span: Span,
}

#[derive(Debug, Clone, Default)]
pub(super) struct LoopCtx {
    pub(super) breaks: Vec<State>,
    pub(super) continues: Vec<State>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Access {
    Read,
    Write,
    Move,
    BorrowShared,
    BorrowExclusive,
}

/// Outcome of a write to a projection under an array index (OWN-5.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ElementWrite {
    /// The write restores nothing: an ordinary element assignment.
    Plain,
    /// The write restores a moved place and the array can be rebuilt around it.
    Reinit,
    /// The write would restore a moved place, but the array cannot be rebuilt;
    /// the span is the move that prevents it.
    Blocked(Span),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PartialRel {
    Exact,
    Extension,
    Ancestor,
    Disjoint,
}

pub(super) struct OwnChecker<'a> {
    pub(super) facts: &'a Facts,
    pub(super) diags: &'a mut DiagnosticBag,
    pub(super) slots: BTreeMap<SlotId, SlotInfo>,
    pub(super) scopes: Vec<Vec<(String, SlotId)>>,
    pub(super) state: State,
    pub(super) loans: Vec<Loan>,
    /// Loans taken by the expressions that enclose the construct under
    /// analysis (a call whose earlier argument borrows, for instance). A nested
    /// block, branch or loop ends its own temporaries, never these.
    pub(super) loan_floor: usize,
    pub(super) next_slot: SlotId,
    /// Diagnostics are emitted only when this is true (false during the silent
    /// fixpoint passes of loops).
    pub(super) emit: bool,
    pub(super) depth: u32,
    /// Current path is unreachable (after `return`/`break`/`continue`).
    pub(super) diverged: bool,
    pub(super) loop_stack: Vec<LoopCtx>,
    pub(super) emitted: u32,
    pub(super) budget_hit: bool,
    pub(super) too_deep: bool,
    /// Depth of call-argument evaluation: a move that happens there is a
    /// *usage* of the value, not the "second effective move" that
    /// `E-OWN-DOUBLE-MOVE` describes.
    pub(super) call_depth: u32,
    /// Ownership decisions recorded for the backend (RFC-OWN-FACTS-1, step A).
    /// Shared across the functions of one program (see `check_ownership`).
    pub(super) own: &'a mut OwnFacts,
}

mod checker_a;
mod checker_b;
mod state;
pub use state::is_copy_type;
use state::*;

#[cfg(test)]
mod tests;
