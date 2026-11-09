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
//!   `E-OWN-MOVE-BORROWED` ; le move d'un élément de `Vec` par indexation est
//!   `E-OWN-MOVE-INDEXED` (`spec/language/ownership.md` §5.5). Le code retenu
//!   est celui du contrat (`E-OWN-MOVE-BORROWED`) ; `E-OWN-MOVE-OUT-OF-BORROW`,
//!   cité par la mission, n'est pas émis pour ne pas dupliquer un code.
//! * **Répétition de tableau** : `[e; n]` exige une valeur `Copy`, sinon
//!   `E-OWN-NOT-COPY` (OWN-2.5).
//! * **Non couvert volontairement** : mutabilité des places (`E-OWN-IMMUTABLE`,
//!   émise par le type-checker), l'ordre de destruction (non observable en
//!   Core 1.0), et la vérification des signatures de retour de référence
//!   (déléguée au type-checker ; le corps est vérifié).
//!
//! Robustesse : profondeur bornée, budget de diagnostics par fonction
//! (`E-OWN-LIMIT`), aucune panique sur un AST incomplet (les faits manquants
//! désactivent seulement la règle correspondante).

use crate::facts::{CallTarget, CheckedProgram, Facts, MethodTarget, Resolution};
use crate::types::Type;
use ravum_lang_syntax::ast::{
    BinOp, Block, Expr, ExprKind, FnItem, Item, ItemKind, MatchArm, Module, ParamKind, Pat,
    PatKind, Receiver, Stmt, StmtKind, UnOp,
};
use ravum_lang_syntax::diag::{Diagnostic, DiagnosticBag};
use ravum_lang_syntax::span::Span;
use std::collections::BTreeMap;

/// Bornes de robustesse / performance (P0-H).
const MAX_DEPTH: u32 = 256;
const MAX_FIXPOINT: u32 = 16;
const MAX_DIAGS_PER_FN: u32 = 200;

const E_UNINIT: &str = "E-OWN-UNINIT";
const E_USE_AFTER_MOVE: &str = "E-OWN-USE-AFTER-MOVE";
const E_DOUBLE_MOVE: &str = "E-OWN-DOUBLE-MOVE";
const E_PARTIAL_MOVE: &str = "E-OWN-PARTIAL-MOVE";
const E_MOVE_INDEXED: &str = "E-OWN-MOVE-INDEXED";
const E_MOVE_BORROWED: &str = "E-OWN-MOVE-BORROWED";
const E_BORROW_CONFLICT: &str = "E-OWN-BORROW-CONFLICT";
const E_REF_ESCAPE: &str = "E-OWN-REF-ESCAPE";
const E_NOT_COPY: &str = "E-OWN-NOT-COPY";
const E_LIMIT: &str = "E-OWN-LIMIT";

/// Verify ownership rules on a typed program. Diagnostics are appended in source
/// order by the caller; this function only pushes them.
pub fn check_ownership(checked: &CheckedProgram, diags: &mut DiagnosticBag) {
    let mut fns: Vec<(&Item, &FnItem)> = Vec::new();
    collect_fns(&checked.program.root, &mut fns);
    for (item, f) in fns {
        let mut checker = OwnChecker::new(&checked.facts, diags);
        checker.check_fn(item, f);
    }
}

fn collect_fns<'a>(module: &'a Module, out: &mut Vec<(&'a Item, &'a FnItem)>) {
    for item in &module.items {
        match &item.kind {
            ItemKind::Fn(f) => out.push((item, f)),
            ItemKind::Mod(inner) => collect_fns(inner, out),
            ItemKind::Impl(imp) => {
                for sub in &imp.items {
                    match &sub.kind {
                        ItemKind::Fn(f) => out.push((sub, f)),
                        ItemKind::Mod(inner) => collect_fns(inner, out),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

type SlotId = u32;

/// One projection on a place. `Deref` separates reference levels.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Step {
    Deref,
    Field(String),
    Tuple(u32),
    Index(IndexKey),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum IndexKey {
    Lit(u128),
    /// Index computed from a non-literal expression: treated as possibly equal
    /// to every other index (conservative, Q-OWN-5).
    Opaque,
}

/// A place rooted at a local binding (`x`, `x.champ`, `x.0`, `v[i]`, `*r`, ...).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Place {
    slot: SlotId,
    steps: Vec<Step>,
}

#[derive(Debug, Clone)]
struct SlotInfo {
    name: String,
    ty: Type,
    /// True when the binding certainly holds a `&'static str` (a string literal
    /// or a copy of one), the only storable reference in Core 1.0.
    static_ref: bool,
    decl_span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PartialMove {
    span: Span,
    used_since: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct SlotState {
    /// Definitely initialised on every path reaching this point.
    init: bool,
    /// Definitely moved (on every path).
    moved: bool,
    /// Moved on at least one path.
    maybe_moved: bool,
    /// Span of the (last) move, for diagnostics.
    move_span: Option<Span>,
    /// A use happened after the recorded move (distinguishes double move from
    /// use-after-move in diagnostics).
    used_since: bool,
    /// Projections that may have been partially moved, with their move record.
    partial: BTreeMap<Vec<Step>, PartialMove>,
}

impl SlotState {
    fn initialised() -> Self {
        SlotState {
            init: true,
            ..SlotState::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct State {
    slots: BTreeMap<SlotId, SlotState>,
}

#[derive(Debug, Clone)]
struct Loan {
    place: Place,
    mutable: bool,
    span: Span,
}

#[derive(Debug, Clone, Default)]
struct LoopCtx {
    breaks: Vec<State>,
    continues: Vec<State>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Access {
    Read,
    Write,
    Move,
    BorrowShared,
    BorrowExclusive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PartialRel {
    Exact,
    Extension,
    Ancestor,
    Disjoint,
}

struct OwnChecker<'a> {
    facts: &'a Facts,
    diags: &'a mut DiagnosticBag,
    slots: BTreeMap<SlotId, SlotInfo>,
    scopes: Vec<Vec<(String, SlotId)>>,
    state: State,
    loans: Vec<Loan>,
    next_slot: SlotId,
    /// Diagnostics are emitted only when this is true (false during the silent
    /// fixpoint passes of loops).
    emit: bool,
    depth: u32,
    /// Current path is unreachable (after `return`/`break`/`continue`).
    diverged: bool,
    loop_stack: Vec<LoopCtx>,
    emitted: u32,
    budget_hit: bool,
    too_deep: bool,
    /// Depth of call-argument evaluation: a move that happens there is a
    /// *usage* of the value, not the "second effective move" that
    /// `E-OWN-DOUBLE-MOVE` describes.
    call_depth: u32,
}

impl<'a> OwnChecker<'a> {
    fn new(facts: &'a Facts, diags: &'a mut DiagnosticBag) -> Self {
        OwnChecker {
            facts,
            diags,
            slots: BTreeMap::new(),
            scopes: Vec::new(),
            state: State::default(),
            loans: Vec::new(),
            next_slot: 0,
            emit: true,
            depth: 0,
            diverged: false,
            loop_stack: Vec::new(),
            emitted: 0,
            budget_hit: false,
            too_deep: false,
            call_depth: 0,
        }
    }

    // ------------------------------------------------------------------
    // Function entry
    // ------------------------------------------------------------------

    fn check_fn(&mut self, item: &Item, f: &FnItem) {
        self.state = State::default();
        self.slots.clear();
        self.scopes.clear();
        self.loans.clear();
        self.next_slot = 0;
        self.emit = true;
        self.depth = 0;
        self.diverged = false;
        self.loop_stack.clear();
        self.emitted = 0;
        self.budget_hit = false;
        self.too_deep = false;
        self.call_depth = 0;

        self.push_scope();
        let mut params: Vec<(String, Type)> = Vec::new();
        if let Some(sig) = self.facts.fn_sigs.get(&item.id) {
            for p in &sig.params {
                let name = if p.is_receiver {
                    "self".to_string()
                } else {
                    p.name.clone()
                };
                params.push((name, p.ty.clone()));
            }
        } else {
            // Fact table missing (checker not finished): fall back on the AST.
            for p in &f.params {
                match &p.kind {
                    ParamKind::Binding { name, ty } => {
                        let ty = self
                            .facts
                            .type_nodes
                            .get(&ty.id)
                            .cloned()
                            .unwrap_or(Type::Error);
                        params.push((name.name.clone(), ty));
                    }
                    ParamKind::Receiver(r) => {
                        let ty = match r {
                            Receiver::ByValue => Type::Error,
                            Receiver::Shared => Type::Ref {
                                mutable: false,
                                inner: Box::new(Type::Error),
                            },
                            Receiver::Exclusive => Type::Ref {
                                mutable: true,
                                inner: Box::new(Type::Error),
                            },
                        };
                        params.push(("self".to_string(), ty));
                    }
                }
            }
        }
        for (name, ty) in params {
            let slot = self.declare_slot(&name, ty, false, item.span);
            self.state.slots.insert(slot, SlotState::initialised());
        }

        self.analyze_block(&f.body, true);

        self.pop_scope();
        self.loans.clear();
    }

    // ------------------------------------------------------------------
    // Scopes and slots
    // ------------------------------------------------------------------

    fn push_scope(&mut self) {
        self.scopes.push(Vec::new());
    }

    fn pop_scope(&mut self) {
        if let Some(vars) = self.scopes.pop() {
            for (_, slot) in vars {
                self.state.slots.remove(&slot);
                self.slots.remove(&slot);
            }
        }
    }

    fn declare_slot(&mut self, name: &str, ty: Type, static_ref: bool, span: Span) -> SlotId {
        let id = self.next_slot;
        self.next_slot = self.next_slot.saturating_add(1);
        self.slots.insert(
            id,
            SlotInfo {
                name: name.to_string(),
                ty,
                static_ref,
                decl_span: span,
            },
        );
        if let Some(scope) = self.scopes.last_mut() {
            scope.push((name.to_string(), id));
        }
        id
    }

    fn lookup(&self, name: &str) -> Option<SlotId> {
        for scope in self.scopes.iter().rev() {
            for (n, slot) in scope.iter().rev() {
                if n == name {
                    return Some(*slot);
                }
            }
        }
        None
    }

    fn slot_info(&self, slot: SlotId) -> Option<&SlotInfo> {
        self.slots.get(&slot)
    }

    fn expr_type(&self, expr: &Expr) -> Type {
        self.facts
            .expr_types
            .get(&expr.id)
            .cloned()
            .unwrap_or(Type::Error)
    }

    fn path_local_slot(&self, expr: &Expr) -> Option<SlotId> {
        if let Some(Resolution::Local { name, .. }) = self.facts.resolutions.get(&expr.id) {
            return self.lookup(name);
        }
        if let ExprKind::Path(path) = &expr.kind {
            if path.segments.len() == 1 {
                if let Some(seg) = path.segments.first() {
                    return self.lookup(&seg.name.name);
                }
            }
        }
        None
    }

    // ------------------------------------------------------------------
    // Block / statement / CFG
    // ------------------------------------------------------------------

    fn enter(&mut self) -> bool {
        if self.depth >= MAX_DEPTH {
            self.too_deep = true;
            return false;
        }
        self.depth += 1;
        true
    }

    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    fn analyze_block(&mut self, block: &Block, fn_body: bool) {
        if !self.enter() {
            return;
        }
        self.push_scope();
        self.loans.clear();
        for stmt in &block.stmts {
            if self.diverged {
                break;
            }
            self.analyze_stmt(stmt);
        }
        if !self.diverged {
            if let Some(tail) = &block.tail {
                self.loans.clear();
                self.eval_value(tail);
                if fn_body && self.is_nonstatic_ref(tail) {
                    self.report_ref_escape(tail.span, "retourner");
                }
                self.loans.clear();
            }
        }
        self.pop_scope();
        self.leave();
    }

    fn analyze_stmt(&mut self, stmt: &Stmt) {
        if !self.enter() {
            return;
        }
        self.loans.clear();
        match &stmt.kind {
            StmtKind::Let(l) => self.check_let(l, stmt.span),
            StmtKind::Expr { expr, .. } => self.eval_value(expr),
        }
        self.loans.clear();
        self.leave();
    }

    fn check_let(&mut self, l: &ravum_lang_syntax::ast::LetStmt, stmt_span: Span) {
        let mut init_static = false;
        let has_init = l.init.is_some();
        if let Some(init) = &l.init {
            self.eval_value(init);
            init_static = self.is_static_ref(init);
        }

        if let PatKind::Binding { name, .. } = &l.pat.kind {
            let ty = self
                .facts
                .pat_types
                .get(&l.pat.id)
                .cloned()
                .or_else(|| l.init.as_ref().map(|e| self.expr_type(e)))
                .unwrap_or(Type::Error);
            let static_ref = ty.is_reference() && init_static;
            if ty.is_reference() && !static_ref {
                self.report_ref_escape(stmt_span, "stocker");
            }
            let slot = self.declare_slot(&name.name, ty, static_ref, l.pat.span);
            let st = if has_init {
                SlotState::initialised()
            } else {
                SlotState::default()
            };
            self.state.slots.insert(slot, st);
            return;
        }

        // Destructuring pattern: the initialiser was consumed as a whole.
        let mut bindings: Vec<(String, Type, Span)> = Vec::new();
        self.collect_pat_bindings(&l.pat, &mut bindings);
        for (name, ty, span) in bindings {
            if ty.is_reference() {
                // Provenance of a reference extracted by a pattern cannot be
                // proven static: refuse conservatively.
                self.report_ref_escape(span, "stocker");
            }
            let slot = self.declare_slot(&name, ty, false, span);
            let st = if has_init {
                SlotState::initialised()
            } else {
                SlotState::default()
            };
            self.state.slots.insert(slot, st);
        }
    }

    fn collect_pat_bindings(&self, pat: &Pat, out: &mut Vec<(String, Type, Span)>) {
        match &pat.kind {
            PatKind::Binding { name, .. } => {
                let ty = self
                    .facts
                    .pat_types
                    .get(&pat.id)
                    .cloned()
                    .unwrap_or(Type::Error);
                out.push((name.name.clone(), ty, pat.span));
            }
            PatKind::Tuple(items) => {
                for item in items {
                    self.collect_pat_bindings(item, out);
                }
            }
            PatKind::Variant { fields, .. } => match fields {
                ravum_lang_syntax::ast::VariantPatFields::Tuple(items) => {
                    for item in items {
                        self.collect_pat_bindings(item, out);
                    }
                }
                ravum_lang_syntax::ast::VariantPatFields::Struct(fields) => {
                    for f in fields {
                        self.collect_pat_bindings(&f.pat, out);
                    }
                }
            },
            PatKind::Wild | PatKind::Lit(_) | PatKind::Path(_) => {}
        }
    }

    // ------------------------------------------------------------------
    // Expressions
    // ------------------------------------------------------------------

    fn eval_value(&mut self, expr: &Expr) {
        if !self.enter() {
            return;
        }
        match &expr.kind {
            ExprKind::Unit
            | ExprKind::Int { .. }
            | ExprKind::Float { .. }
            | ExprKind::Bool(_)
            | ExprKind::Char(_)
            | ExprKind::Str(_) => {}
            ExprKind::SelfValue | ExprKind::Path(_) => {
                if let Some(place) = self.eval_place_expr(expr) {
                    self.consume_place(&place, expr.span);
                }
            }
            ExprKind::Field { base, .. } | ExprKind::TupleIndex { base, .. } => {
                if let Some(place) = self.eval_place_expr(expr) {
                    self.consume_place(&place, expr.span);
                } else {
                    let base_ty = self.expr_type(base);
                    self.eval_value(base);
                    if base_ty.is_reference() && !self.expr_type(expr).is_error_or_copy(self.facts)
                    {
                        // Moving a field/index out of a borrowed temporary.
                        self.report_move_borrowed(expr.span);
                    }
                }
            }
            ExprKind::Index { base, index } => {
                if let Some(place) = self.eval_place_expr(expr) {
                    self.consume_place(&place, expr.span);
                } else {
                    let base_ty = self.expr_type(base);
                    self.eval_value(base);
                    self.eval_value(index);
                    if base_ty.is_reference() {
                        if !self.expr_type(expr).is_error_or_copy(self.facts) {
                            self.report_move_borrowed(expr.span);
                        }
                    } else if matches!(base_ty, Type::Vec(_))
                        && !self.expr_type(expr).is_error_or_copy(self.facts)
                    {
                        self.report_move_indexed(expr.span);
                    }
                }
            }
            ExprKind::Unary { op, expr: inner } => match op {
                UnOp::Ref => self.eval_borrow(inner, false, expr.span),
                UnOp::RefMut => self.eval_borrow(inner, true, expr.span),
                UnOp::Deref => {
                    if let Some(place) = self.eval_place_expr(expr) {
                        self.consume_place(&place, expr.span);
                    } else {
                        let inner_ty = self.expr_type(inner);
                        self.eval_value(inner);
                        if inner_ty.is_reference()
                            && !self.expr_type(expr).is_error_or_copy(self.facts)
                        {
                            self.report_move_borrowed(expr.span);
                        }
                    }
                }
                UnOp::Neg | UnOp::Not => self.eval_value(inner),
            },
            ExprKind::Binary { op, lhs, rhs } => {
                self.eval_value(lhs);
                if matches!(op, BinOp::And | BinOp::Or) {
                    let after_lhs = self.state.clone();
                    let lhs_diverged = self.diverged;
                    self.eval_value(rhs);
                    if self.diverged {
                        self.state = after_lhs;
                        self.diverged = lhs_diverged;
                    } else if !lhs_diverged {
                        self.state = join(&after_lhs, &self.state);
                    }
                } else {
                    self.eval_value(rhs);
                }
            }
            ExprKind::Assign { target, value } => {
                self.eval_value(value);
                let target_ty = self.expr_type(target);
                if target_ty.is_reference() && self.is_nonstatic_ref(value) {
                    self.report_ref_escape(value.span, "stocker");
                }
                if let Some(place) = self.eval_place_expr(target) {
                    self.write_place(&place, target.span);
                }
            }
            ExprKind::AssignOp { target, value, .. } => {
                let place = self.eval_place_expr(target);
                if let Some(p) = &place {
                    self.read_place(p, target.span);
                }
                self.eval_value(value);
                if let Some(p) = place {
                    self.write_place(&p, target.span);
                }
            }
            ExprKind::Cast { expr: inner, .. } => self.eval_value(inner),
            ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => self.eval_if(cond, then_block, else_branch.as_deref()),
            ExprKind::Match { scrutinee, arms } => self.eval_match(scrutinee, arms),
            ExprKind::While { cond, body } => self.eval_while(cond, body),
            ExprKind::Loop { body } => self.eval_loop(body),
            ExprKind::For { pat, iter, body } => self.eval_for(pat, iter, body),
            ExprKind::Block(block) => self.analyze_block(block, false),
            ExprKind::Unsafe(block) => self.analyze_block(block, false),
            ExprKind::Tuple(items) | ExprKind::Array(items) => {
                for item in items {
                    self.eval_value(item);
                    if self.is_nonstatic_ref(item) {
                        self.report_ref_escape(item.span, "stocker");
                    }
                }
            }
            ExprKind::ArrayRepeat { value, len } => {
                let ty = self.expr_type(value);
                if !ty.is_error_or_copy(self.facts) {
                    self.emit_diag(
                        E_NOT_COPY,
                        value.span,
                        "répétition d'un tableau avec une valeur mobile".to_string(),
                        vec![(
                            "la valeur serait dupliquée ; elle doit être `Copy`".to_string(),
                            None,
                        )],
                    );
                    if let Some(place) = self.eval_place_expr(value) {
                        self.read_place(&place, value.span);
                    } else {
                        self.eval_value(value);
                    }
                } else {
                    self.eval_value(value);
                }
                self.eval_value(len);
            }
            ExprKind::StructLit { fields, .. } => {
                for field in fields {
                    self.eval_value(&field.value);
                    if self.is_nonstatic_ref(&field.value) {
                        self.report_ref_escape(field.value.span, "stocker");
                    }
                }
            }
            ExprKind::Range { start, end, .. } => {
                if let Some(start) = start {
                    self.eval_value(start);
                }
                if let Some(end) = end {
                    self.eval_value(end);
                }
            }
            ExprKind::Try(inner) => {
                // `?` may leave the function on the error path; the normal path
                // simply continues with the payload.
                self.eval_value(inner);
            }
            ExprKind::Call { callee, args } => {
                if !matches!(callee.kind, ExprKind::Path(_)) {
                    self.eval_value(callee);
                }
                self.eval_call(expr, args);
            }
            ExprKind::MethodCall {
                receiver,
                method,
                args,
            } => {
                let _ = method;
                self.eval_method_call(expr, receiver, args);
            }
            ExprKind::Return(value) => {
                if let Some(value) = value {
                    self.eval_value(value);
                    if self.is_nonstatic_ref(value) {
                        self.report_ref_escape(value.span, "retourner");
                    }
                }
                self.diverged = true;
            }
            ExprKind::Break => {
                let st = self.state.clone();
                if let Some(ctx) = self.loop_stack.last_mut() {
                    ctx.breaks.push(st);
                }
                self.diverged = true;
            }
            ExprKind::Continue => {
                let st = self.state.clone();
                if let Some(ctx) = self.loop_stack.last_mut() {
                    ctx.continues.push(st);
                }
                self.diverged = true;
            }
        }
        self.leave();
    }

    fn eval_borrow(&mut self, operand: &Expr, mutable: bool, span: Span) {
        if let Some(place) = self.eval_place_expr(operand) {
            self.borrow_place(&place, span, mutable);
        } else {
            // Borrow of a temporary rvalue: allowed, dies with the instruction.
            self.eval_value(operand);
        }
    }

    /// Evaluate an expression as a place without consuming it.
    fn eval_place_expr(&mut self, expr: &Expr) -> Option<Place> {
        if !self.enter() {
            return None;
        }
        let result = match &expr.kind {
            ExprKind::SelfValue => Some(Place {
                slot: self.lookup("self")?,
                steps: Vec::new(),
            }),
            ExprKind::Path(_) => self.path_local_slot(expr).map(|slot| Place {
                slot,
                steps: Vec::new(),
            }),
            ExprKind::Field { base, name } => {
                if let Some(mut place) = self.eval_place_expr(base) {
                    self.auto_deref(&mut place);
                    place.steps.push(Step::Field(name.name.clone()));
                    Some(place)
                } else {
                    None
                }
            }
            ExprKind::TupleIndex { base, index } => {
                if let Some(mut place) = self.eval_place_expr(base) {
                    self.auto_deref(&mut place);
                    place.steps.push(Step::Tuple(*index));
                    Some(place)
                } else {
                    None
                }
            }
            ExprKind::Index { base, index } => {
                if let Some(mut place) = self.eval_place_expr(base) {
                    self.eval_value(index);
                    self.auto_deref(&mut place);
                    let key = match &index.kind {
                        ExprKind::Int { value, .. } => IndexKey::Lit(*value),
                        _ => IndexKey::Opaque,
                    };
                    place.steps.push(Step::Index(key));
                    Some(place)
                } else {
                    None
                }
            }
            ExprKind::Unary {
                op: UnOp::Deref,
                expr: inner,
            } => {
                if let Some(mut place) = self.eval_place_expr(inner) {
                    place.steps.push(Step::Deref);
                    Some(place)
                } else {
                    None
                }
            }
            _ => None,
        };
        self.leave();
        result
    }

    fn auto_deref(&self, place: &mut Place) {
        let mut guard = 0;
        while self.place_type(place).is_reference() && guard < 16 {
            place.steps.push(Step::Deref);
            guard += 1;
        }
    }

    fn eval_if(&mut self, cond: &Expr, then_block: &Block, else_branch: Option<&Expr>) {
        self.eval_value(cond);
        // Condition borrows/temporaries end before the branches execute; branch
        // statements are distinct instructions.
        self.loans.clear();
        let after_cond = self.state.clone();
        let cond_diverged = self.diverged;

        let mut states: Vec<State> = Vec::new();
        self.state = after_cond.clone();
        self.diverged = cond_diverged;
        if !self.diverged {
            self.analyze_block(then_block, false);
        }
        if !self.diverged {
            states.push(self.state.clone());
        }

        self.state = after_cond.clone();
        self.diverged = cond_diverged;
        if let Some(branch) = else_branch {
            if !self.diverged {
                self.eval_value(branch);
            }
        }
        if !self.diverged {
            states.push(self.state.clone());
        }

        match join_all(&states) {
            Some(st) => {
                self.state = st;
                self.diverged = false;
            }
            None => {
                self.state = after_cond;
                self.diverged = true;
            }
        }
    }

    fn eval_match(&mut self, scrutinee: &Expr, arms: &[MatchArm]) {
        self.eval_value(scrutinee);
        // The scrutinee is consumed; loans it created expire before the arms so
        // that a subsequent `match v.pop() { ... v.push(..) }` stays accepted.
        // Any reference that could escape from the scrutinee would already be
        // rejected as a ref escape.
        self.loans.clear();
        let base = self.state.clone();
        let base_diverged = self.diverged;

        let mut states: Vec<State> = Vec::new();
        for arm in arms {
            self.state = base.clone();
            self.diverged = base_diverged;
            if self.diverged {
                continue;
            }
            self.push_scope();
            let mut bindings: Vec<(String, Type, Span)> = Vec::new();
            self.collect_pat_bindings(&arm.pat, &mut bindings);
            let mut arm_slots: Vec<SlotId> = Vec::new();
            for (name, ty, span) in bindings {
                if ty.is_reference() {
                    self.report_ref_escape(span, "stocker");
                }
                let slot = self.declare_slot(&name, ty, false, span);
                arm_slots.push(slot);
                self.state.slots.insert(slot, SlotState::initialised());
            }
            self.eval_value(&arm.body);
            if !self.diverged {
                let mut st = self.state.clone();
                for slot in &arm_slots {
                    drop_slot(&mut st, *slot);
                }
                states.push(st);
            }
            self.pop_scope();
        }

        match join_all(&states) {
            Some(st) => {
                self.state = st;
                self.diverged = false;
            }
            None => {
                self.state = base;
                self.diverged = true;
            }
        }
    }

    fn eval_while(&mut self, cond: &Expr, body: &Block) {
        let saved_emit = self.emit;
        let pre = self.state.clone();
        let mut entry = pre.clone();
        let mut iterations = 0;
        loop {
            self.emit = false;
            self.state = entry.clone();
            self.diverged = false;
            self.loans.clear();
            self.eval_value(cond);
            self.loans.clear();
            let cond_diverged = self.diverged;
            self.loop_stack.push(LoopCtx::default());
            if !cond_diverged {
                self.analyze_block(body, false);
            }
            let ctx = self.loop_stack.pop().unwrap_or_default();
            let mut next = entry.clone();
            if !self.diverged {
                next = join(&next, &self.state);
            }
            for c in &ctx.continues {
                next = join(&next, c);
            }
            iterations += 1;
            let stable = next == entry;
            entry = next;
            if stable || iterations >= MAX_FIXPOINT {
                break;
            }
        }

        // Final pass: emit diagnostics from the stabilised header state.
        self.emit = saved_emit;
        self.state = entry;
        self.diverged = false;
        self.loans.clear();
        self.eval_value(cond);
        self.loans.clear();
        let cond_diverged = self.diverged;
        let after_cond = self.state.clone();
        self.loop_stack.push(LoopCtx::default());
        if !cond_diverged {
            self.analyze_block(body, false);
        }
        let ctx = self.loop_stack.pop().unwrap_or_default();

        // Exit: either the (at least one) condition evaluation fails, or a
        // `break` was taken. The failing-condition state comes from the
        // stabilised header, not from the body exit.
        let mut exit: Option<State> = if cond_diverged {
            None
        } else {
            Some(after_cond)
        };
        for b in &ctx.breaks {
            exit = Some(match exit {
                Some(e) => join(&e, b),
                None => b.clone(),
            });
        }
        match exit {
            Some(st) => {
                self.state = st;
                self.diverged = false;
            }
            None => {
                self.diverged = true;
            }
        }
    }

    fn eval_loop(&mut self, body: &Block) {
        let saved_emit = self.emit;
        let pre = self.state.clone();
        let mut entry = pre.clone();
        let mut iterations = 0;
        loop {
            self.emit = false;
            self.state = entry.clone();
            self.diverged = false;
            self.loans.clear();
            self.loop_stack.push(LoopCtx::default());
            self.analyze_block(body, false);
            let ctx = self.loop_stack.pop().unwrap_or_default();
            let mut next = entry.clone();
            if !self.diverged {
                next = join(&next, &self.state);
            }
            for c in &ctx.continues {
                next = join(&next, c);
            }
            iterations += 1;
            let stable = next == entry;
            entry = next;
            if stable || iterations >= MAX_FIXPOINT {
                break;
            }
        }

        self.emit = saved_emit;
        self.state = entry;
        self.diverged = false;
        self.loans.clear();
        self.loop_stack.push(LoopCtx::default());
        self.analyze_block(body, false);
        let ctx = self.loop_stack.pop().unwrap_or_default();

        // `loop` always executes its body at least once; the only normal exit is
        // `break`.
        let mut exit: Option<State> = None;
        for b in &ctx.breaks {
            exit = Some(match exit {
                Some(e) => join(&e, b),
                None => b.clone(),
            });
        }
        match exit {
            Some(st) => {
                self.state = st;
                self.diverged = false;
            }
            None => {
                self.diverged = true;
            }
        }
    }

    fn eval_for(&mut self, pat: &Pat, iter_expr: &Expr, body: &Block) {
        self.eval_value(iter_expr);
        self.loans.clear();
        let after_iter = self.state.clone();
        let iter_diverged = self.diverged;

        self.push_scope();
        let mut bindings: Vec<(String, Type, Span)> = Vec::new();
        self.collect_pat_bindings(pat, &mut bindings);
        let mut loop_slots: Vec<SlotId> = Vec::new();
        for (name, ty, span) in &bindings {
            if ty.is_reference() {
                self.report_ref_escape(*span, "stocker");
            }
            let slot = self.declare_slot(name, ty.clone(), false, *span);
            loop_slots.push(slot);
        }

        let saved_emit = self.emit;
        let mut entry = after_iter.clone();
        let mut iterations = 0;
        loop {
            self.emit = false;
            self.state = entry.clone();
            for slot in &loop_slots {
                self.state.slots.insert(*slot, SlotState::initialised());
            }
            self.diverged = iter_diverged;
            self.loans.clear();
            self.loop_stack.push(LoopCtx::default());
            if !self.diverged {
                self.analyze_block(body, false);
            }
            let ctx = self.loop_stack.pop().unwrap_or_default();
            let mut next = entry.clone();
            if !self.diverged {
                next = join(&next, &self.state);
            }
            for c in &ctx.continues {
                next = join(&next, c);
            }
            for slot in &loop_slots {
                drop_slot(&mut next, *slot);
            }
            iterations += 1;
            let stable = next == entry;
            entry = next;
            if stable || iterations >= MAX_FIXPOINT {
                break;
            }
        }

        self.emit = saved_emit;
        self.state = entry.clone();
        for slot in &loop_slots {
            self.state.slots.insert(*slot, SlotState::initialised());
        }
        self.diverged = iter_diverged;
        self.loans.clear();
        self.loop_stack.push(LoopCtx::default());
        if !self.diverged {
            self.analyze_block(body, false);
        }
        let ctx = self.loop_stack.pop().unwrap_or_default();

        // Exit: zero iterations (`after_iter`) or any number of iterations then
        // normal termination (the fixpoint header), plus every `break`.
        let mut exit: Option<State> = if iter_diverged {
            None
        } else {
            Some(join(&after_iter, &entry))
        };
        for b in &ctx.breaks {
            exit = Some(match exit {
                Some(e) => join(&e, b),
                None => b.clone(),
            });
        }
        self.pop_scope();
        match exit {
            Some(mut st) => {
                for slot in &loop_slots {
                    drop_slot(&mut st, *slot);
                }
                self.state = st;
                self.diverged = false;
            }
            None => {
                self.diverged = true;
            }
        }
    }

    // ------------------------------------------------------------------
    // Calls
    // ------------------------------------------------------------------

    fn eval_call(&mut self, expr: &Expr, args: &[Expr]) {
        let snapshot = self.loans.len();
        self.call_depth = self.call_depth.saturating_add(1);
        let target = self.facts.calls.get(&expr.id).cloned();
        match target {
            Some(CallTarget::UserFn { item }) => {
                let params: Vec<Type> = self
                    .facts
                    .fn_sigs
                    .get(&item)
                    .map(|sig| {
                        sig.params
                            .iter()
                            .filter(|p| !p.is_receiver)
                            .map(|p| p.ty.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                if params.is_empty() {
                    // Signature unknown: do not invent a storing rule.
                    for arg in args {
                        self.eval_call_arg(arg, None, true);
                    }
                } else {
                    for (i, arg) in args.iter().enumerate() {
                        self.eval_call_arg(arg, params.get(i), false);
                    }
                }
            }
            Some(CallTarget::BuiltinFn { path }) => {
                if last_path_segment(&path) == "drop" {
                    // `drop(x)` is an explicit move (or a no-op copy).
                    for arg in args {
                        self.eval_value(arg);
                    }
                } else {
                    for arg in args {
                        self.eval_call_arg(arg, None, true);
                    }
                }
            }
            Some(CallTarget::VariantCtor { .. }) => {
                for arg in args {
                    self.eval_value(arg);
                    if self.is_nonstatic_ref(arg) {
                        self.report_ref_escape(arg.span, "stocker");
                    }
                }
            }
            Some(CallTarget::Error) | None => {
                for arg in args {
                    self.eval_call_arg(arg, None, true);
                }
            }
        }
        self.loans.truncate(snapshot);
        self.call_depth = self.call_depth.saturating_sub(1);
    }

    fn eval_method_call(&mut self, expr: &Expr, receiver: &Expr, args: &[Expr]) {
        let snapshot = self.loans.len();
        self.call_depth = self.call_depth.saturating_add(1);
        match self.facts.methods.get(&expr.id).cloned() {
            Some(MethodTarget::User { item }) => {
                let sig = self.facts.fn_sigs.get(&item);
                let receiver_kind = sig.and_then(|s| s.receiver);
                let params: Vec<Type> = sig
                    .map(|s| {
                        s.params
                            .iter()
                            .filter(|p| !p.is_receiver)
                            .map(|p| p.ty.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                match receiver_kind {
                    Some(Receiver::Shared) => self.borrow_receiver(receiver, false),
                    Some(Receiver::Exclusive) => self.borrow_receiver(receiver, true),
                    Some(Receiver::ByValue) => self.eval_value(receiver),
                    None => self.borrow_receiver(receiver, false),
                }
                if params.is_empty() {
                    for arg in args {
                        self.eval_call_arg(arg, None, true);
                    }
                } else {
                    for (i, arg) in args.iter().enumerate() {
                        self.eval_call_arg(arg, params.get(i), false);
                    }
                }
            }
            Some(MethodTarget::Builtin { path, .. }) => {
                let mutating = builtin_method_is_mutating(&path);
                self.borrow_receiver(receiver, mutating);
                let stores_refs = builtin_method_stores_refs(&path);
                for arg in args {
                    self.eval_call_arg(arg, None, !stores_refs);
                }
            }
            Some(MethodTarget::Error) | None => {
                self.borrow_receiver(receiver, false);
                for arg in args {
                    self.eval_call_arg(arg, None, true);
                }
            }
        }
        self.loans.truncate(snapshot);
        self.call_depth = self.call_depth.saturating_sub(1);
    }

    fn borrow_receiver(&mut self, receiver: &Expr, mutable: bool) {
        if let Some(mut place) = self.eval_place_expr(receiver) {
            self.auto_deref(&mut place);
            self.borrow_place(&place, receiver.span, mutable);
        } else {
            self.eval_value(receiver);
        }
    }

    fn eval_call_arg(&mut self, arg: &Expr, param: Option<&Type>, allow_ref_temporaries: bool) {
        let arg_ty = self.expr_type(arg);
        if arg_ty.is_reference() {
            if self.is_static_ref(arg) {
                self.eval_value(arg);
                return;
            }
            if matches!(
                arg.kind,
                ExprKind::Unary {
                    op: UnOp::Ref | UnOp::RefMut,
                    ..
                }
            ) {
                self.eval_value(arg);
                return;
            }
            match param {
                Some(Type::Ref { mutable, .. }) => {
                    // Reborrow of a reference-typed place for the duration of
                    // the call; a reference rvalue (e.g. `s.as_str()`) is a
                    // temporary consumed by the call.
                    if let Some(mut place) = self.eval_place_expr(arg) {
                        self.auto_deref(&mut place);
                        self.borrow_place(&place, arg.span, *mutable);
                    } else {
                        self.eval_value(arg);
                    }
                }
                Some(_) => {
                    if allow_ref_temporaries {
                        self.eval_value(arg);
                    } else {
                        self.report_ref_escape(arg.span, "stocker");
                        self.eval_value(arg);
                    }
                }
                None => {
                    if !allow_ref_temporaries {
                        self.report_ref_escape(arg.span, "stocker");
                    }
                    self.eval_value(arg);
                }
            }
        } else {
            self.eval_value(arg);
        }
    }

    // ------------------------------------------------------------------
    // Place consumption, reads, writes, borrows
    // ------------------------------------------------------------------

    fn consume_place(&mut self, place: &Place, span: Span) {
        if !self.check_availability(place, span, Access::Move) {
            return;
        }
        if self.place_type(place).is_error_or_copy(self.facts) {
            self.mark_used(place.slot);
            return;
        }
        self.perform_move(place, span);
    }

    fn read_place(&mut self, place: &Place, span: Span) {
        if self.check_availability(place, span, Access::Read) {
            self.mark_used(place.slot);
        }
    }

    fn write_place(&mut self, place: &Place, span: Span) {
        if !self.check_availability(place, span, Access::Write) {
            return;
        }
        if place.steps.is_empty() {
            if let Some(st) = self.state.slots.get_mut(&place.slot) {
                st.init = true;
                st.moved = false;
                st.maybe_moved = false;
                st.move_span = None;
                st.used_since = true;
                st.partial.clear();
            }
            return;
        }
        if let Some(st) = self.state.slots.get_mut(&place.slot) {
            // Writing a projection: an exact re-assignment of a partially moved
            // projection restores it; writing an ancestor restores all its
            // sub-projections.
            let keys: Vec<Vec<Step>> = st.partial.keys().cloned().collect();
            for key in keys {
                if steps_start_with(&key, &place.steps) {
                    st.partial.remove(&key);
                }
            }
            st.used_since = true;
        }
    }

    fn borrow_place(&mut self, place: &Place, span: Span, mutable: bool) -> bool {
        let access = if mutable {
            Access::BorrowExclusive
        } else {
            Access::BorrowShared
        };
        if !self.check_availability(place, span, access) {
            return false;
        }
        self.mark_used(place.slot);
        self.loans.push(Loan {
            place: place.clone(),
            mutable,
            span,
        });
        true
    }

    fn mark_used(&mut self, slot: SlotId) {
        if let Some(st) = self.state.slots.get_mut(&slot) {
            st.used_since = true;
            for rec in st.partial.values_mut() {
                rec.used_since = true;
            }
        }
    }

    fn check_availability(&mut self, place: &Place, span: Span, access: Access) -> bool {
        let Some(st) = self.state.slots.get(&place.slot).cloned() else {
            // Unknown binding: a broken program; do not invent an error.
            return false;
        };

        let whole_reinit = access == Access::Write && place.steps.is_empty();

        if (st.moved || st.maybe_moved) && !whole_reinit {
            let is_effective_move = access == Access::Move && self.call_depth == 0;
            self.report_use_after_move(place, span, &st, is_effective_move);
            return false;
        }

        if !st.partial.is_empty() && !whole_reinit {
            let mut relation = PartialRel::Disjoint;
            let mut record: Option<PartialMove> = None;
            for (moved, rec) in &st.partial {
                match classify_partial(&place.steps, moved) {
                    PartialRel::Exact => {
                        relation = PartialRel::Exact;
                        record = Some(rec.clone());
                        break;
                    }
                    PartialRel::Extension => {
                        if relation != PartialRel::Exact {
                            relation = PartialRel::Extension;
                            record = Some(rec.clone());
                        }
                    }
                    PartialRel::Ancestor => {
                        if relation == PartialRel::Disjoint {
                            relation = PartialRel::Ancestor;
                            record = Some(rec.clone());
                        }
                    }
                    PartialRel::Disjoint => {}
                }
            }
            // Design corrigé : un champ peut être déplacé ; l'agrégat entier
            // reste utilisable uniquement s'il n'a subi aucun déplacement
            // partiel. Un autre champ reste utilisable.
            let allowed = match access {
                Access::Write => matches!(
                    relation,
                    PartialRel::Exact | PartialRel::Ancestor | PartialRel::Disjoint
                ),
                _ => relation == PartialRel::Disjoint,
            };
            if !allowed {
                match (relation, record) {
                    (PartialRel::Exact | PartialRel::Extension, Some(rec)) => {
                        let is_effective_move = access == Access::Move && self.call_depth == 0;
                        self.report_partial_use_after_move(place, span, &rec, is_effective_move);
                    }
                    (_, rec) => {
                        self.report_partial_move(place, span, rec.map(|r| r.span));
                    }
                }
                return false;
            }
        }

        let uninitialised = !st.init && !st.moved && !st.maybe_moved;
        if uninitialised && !whole_reinit {
            self.report_uninit(place, span);
            return false;
        }

        // Loans: exclusive loan forbids every use; shared loans forbid writes,
        // moves and exclusive borrows.
        let mut conflict: Option<(bool, Span)> = None;
        for loan in &self.loans {
            if places_overlap(&loan.place, place) {
                let bad = match access {
                    Access::Read | Access::BorrowShared => loan.mutable,
                    Access::Write | Access::Move | Access::BorrowExclusive => true,
                };
                if bad {
                    conflict = Some((loan.mutable, loan.span));
                    break;
                }
            }
        }
        if let Some((loan_mutable, loan_span)) = conflict {
            self.report_borrow_conflict(place, span, access, loan_mutable, loan_span);
            return false;
        }

        true
    }

    fn perform_move(&mut self, place: &Place, span: Span) {
        if place.steps.is_empty() {
            if let Some(st) = self.state.slots.get_mut(&place.slot) {
                st.init = false;
                st.moved = true;
                st.maybe_moved = true;
                st.move_span = Some(span);
                st.used_since = false;
                st.partial.clear();
            }
            return;
        }

        let last_index = place.steps.len() - 1;
        let last = place.steps[last_index].clone();
        // Moving anything through a reference borrows memory this function does
        // not own, even when only a field/index is moved.
        if place.steps[..last_index]
            .iter()
            .any(|step| matches!(step, Step::Deref))
        {
            self.report_move_borrowed(span);
            return;
        }
        let before = self.type_at(place, last_index);
        match last {
            Step::Deref => {
                self.report_move_borrowed(span);
            }
            Step::Field(_) => {
                // Un champ de struct peut être déplacé et suivi individuellement
                // (design corrigé) ; l'usage de l'agrégat entier sera refusé.
                self.record_partial_move(place, span);
            }
            Step::Tuple(_) => {
                self.record_partial_move(place, span);
            }
            Step::Index(_) => match before {
                Type::Vec(_) => {
                    self.report_move_indexed(span);
                }
                Type::Slice(_) => {
                    self.report_move_borrowed(span);
                }
                _ => {
                    // Fixed-size array (or unknown): partial move.
                    self.record_partial_move(place, span);
                }
            },
        }
    }

    fn record_partial_move(&mut self, place: &Place, span: Span) {
        if let Some(st) = self.state.slots.get_mut(&place.slot) {
            st.partial.insert(
                place.steps.clone(),
                PartialMove {
                    span,
                    used_since: false,
                },
            );
            st.used_since = false;
        }
    }

    // ------------------------------------------------------------------
    // Types and Copy classification
    // ------------------------------------------------------------------

    fn place_type(&self, place: &Place) -> Type {
        self.type_at(place, place.steps.len())
    }

    /// Type obtained after applying the first `count` projection steps.
    fn type_at(&self, place: &Place, count: usize) -> Type {
        let Some(info) = self.slot_info(place.slot) else {
            return Type::Error;
        };
        let mut ty = info.ty.clone();
        for step in place.steps.iter().take(count) {
            ty = match step {
                Step::Deref => match ty {
                    Type::Ref { inner, .. } => *inner,
                    _ => Type::Error,
                },
                Step::Field(name) => field_type(&ty, name, self.facts),
                Step::Tuple(index) => match ty {
                    Type::Tuple(items) => {
                        items.get(*index as usize).cloned().unwrap_or(Type::Error)
                    }
                    _ => Type::Error,
                },
                Step::Index(_) => match ty {
                    Type::Array { elem, .. } => *elem,
                    Type::Vec(elem) => *elem,
                    Type::Slice(elem) => *elem,
                    _ => Type::Error,
                },
            };
        }
        ty
    }

    // ------------------------------------------------------------------
    // Static `&str` provenance
    // ------------------------------------------------------------------

    fn is_static_ref(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Str(_) => true,
            // `&e` always borrows a *place* (a variable, a temporary, or the
            // temporary holding a `&'static str` value). It never produces a
            // `'static` reference itself: `let r = &"a";` has type `&&str` and
            // stores a reference to a temporary, which must be rejected like any
            // other escaping borrow (F2 audit finding).
            ExprKind::Unary { op: UnOp::Ref, .. } => false,
            ExprKind::Path(_) | ExprKind::SelfValue => {
                if let Some(slot) = self.path_local_slot(expr) {
                    self.slot_info(slot).map(|s| s.static_ref).unwrap_or(false)
                } else {
                    false
                }
            }
            ExprKind::Block(block) | ExprKind::Unsafe(block) => block
                .tail
                .as_ref()
                .map(|tail| self.is_static_ref(tail))
                .unwrap_or(false),
            // A `&str` produced by `if`/`match` is static only when every
            // branch is itself static; this removes the F1 false positives
            // without weakening the non-escape rule.
            ExprKind::If {
                then_block,
                else_branch,
                ..
            } => {
                then_block
                    .tail
                    .as_ref()
                    .map(|tail| self.is_static_ref(tail))
                    .unwrap_or(false)
                    && else_branch
                        .as_ref()
                        .map(|branch| self.is_static_ref(branch))
                        .unwrap_or(false)
            }
            ExprKind::Match { arms, .. } => {
                !arms.is_empty() && arms.iter().all(|arm| self.is_static_ref(&arm.body))
            }
            // Interprocedural provenance (audit finding F1): a call to a user
            // function classified as always returning `&'static str` is static.
            // A builtin (notably `String::as_str`) is never accepted.
            ExprKind::Call { .. } => matches!(
                self.facts.calls.get(&expr.id),
                Some(CallTarget::UserFn { item }) if self.facts.static_str_fns.contains(item)
            ),
            _ => false,
        }
    }

    fn is_nonstatic_ref(&self, expr: &Expr) -> bool {
        self.expr_type(expr).is_reference() && !self.is_static_ref(expr)
    }

    // ------------------------------------------------------------------
    // Diagnostics
    // ------------------------------------------------------------------

    fn emit_diag(
        &mut self,
        code: &str,
        span: Span,
        message: String,
        notes: Vec<(String, Option<Span>)>,
    ) {
        if !self.emit || self.budget_hit {
            return;
        }
        self.emitted = self.emitted.saturating_add(1);
        if self.emitted > MAX_DIAGS_PER_FN {
            self.budget_hit = true;
            self.diags.push(Diagnostic::error(
                E_LIMIT,
                span,
                "trop d'erreurs d'emprunt dans cette fonction ; analyse interrompue",
            ));
            return;
        }
        let mut diag = Diagnostic::error(code, span, message);
        for (note, note_span) in notes {
            diag = diag.with_note(note, note_span);
        }
        self.diags.push(diag);
    }

    fn report_uninit(&mut self, place: &Place, span: Span) {
        let name = self.display_place(place);
        let decl = self.slot_info(place.slot).map(|s| s.decl_span);
        self.emit_diag(
            E_UNINIT,
            span,
            format!("utilisation de `{name}` avant son initialisation"),
            vec![("déclarée ici, sans valeur initiale".to_string(), decl)],
        );
    }

    fn report_use_after_move(&mut self, place: &Place, span: Span, st: &SlotState, is_move: bool) {
        let name = self.display_place(place);
        if is_move && st.moved && !st.used_since {
            let first = st.move_span;
            self.emit_diag(
                E_DOUBLE_MOVE,
                span,
                format!("déplacement multiple de `{name}`"),
                vec![("premier déplacement effectué ici".to_string(), first)],
            );
        } else {
            let first = st.move_span;
            let note = if st.maybe_moved && !st.moved {
                "déplacement possible sur un autre chemin (analyse conservatrice)"
            } else {
                "déplacement effectué ici"
            };
            self.emit_diag(
                E_USE_AFTER_MOVE,
                span,
                format!("utilisation de `{name}` après son déplacement"),
                vec![(note.to_string(), first)],
            );
        }
    }

    fn report_partial_use_after_move(
        &mut self,
        place: &Place,
        span: Span,
        rec: &PartialMove,
        is_move: bool,
    ) {
        let name = self.display_place(place);
        if is_move && !rec.used_since {
            self.emit_diag(
                E_DOUBLE_MOVE,
                span,
                format!("déplacement multiple de `{name}`"),
                vec![(
                    "déplacement partiel effectué ici".to_string(),
                    Some(rec.span),
                )],
            );
        } else {
            self.emit_diag(
                E_USE_AFTER_MOVE,
                span,
                format!("utilisation de `{name}` après son déplacement"),
                vec![(
                    "déplacement partiel effectué ici".to_string(),
                    Some(rec.span),
                )],
            );
        }
    }

    fn report_partial_move(&mut self, place: &Place, span: Span, moved_span: Option<Span>) {
        let name = self.display_place(place);
        self.emit_diag(
            E_PARTIAL_MOVE,
            span,
            format!("utilisation de l'agrégat `{name}` après un déplacement partiel"),
            vec![(
                "un champ ou un élément a déjà été déplacé".to_string(),
                moved_span,
            )],
        );
    }

    fn report_move_indexed(&mut self, span: Span) {
        self.emit_diag(
            E_MOVE_INDEXED,
            span,
            "déplacement d'un élément de `Vec` par indexation interdit".to_string(),
            vec![(
                "utiliser `.pop()`, un motif ou une méthode sur `&mut self`".to_string(),
                None,
            )],
        );
    }

    fn report_move_borrowed(&mut self, span: Span) {
        self.emit_diag(
            E_MOVE_BORROWED,
            span,
            "déplacement d'une valeur à travers une référence interdit".to_string(),
            vec![(
                "la valeur empruntée n'est pas possédée par cette fonction".to_string(),
                None,
            )],
        );
    }

    fn report_borrow_conflict(
        &mut self,
        place: &Place,
        span: Span,
        access: Access,
        loan_mutable: bool,
        loan_span: Span,
    ) {
        let name = self.display_place(place);
        let message = match access {
            Access::Move => format!("déplacement de `{name}` pendant qu'il est emprunté"),
            Access::Read => format!("lecture de `{name}` pendant un emprunt exclusif"),
            Access::Write => format!("écriture de `{name}` pendant qu'il est emprunté"),
            Access::BorrowExclusive => {
                format!("emprunt exclusif de `{name}` en conflit avec un prêt actif")
            }
            Access::BorrowShared => {
                format!("emprunt partagé de `{name}` en conflit avec un emprunt exclusif")
            }
        };
        let kind = if loan_mutable {
            "un prêt exclusif est déjà actif ici"
        } else {
            "un prêt partagé est déjà actif ici"
        };
        self.emit_diag(
            E_BORROW_CONFLICT,
            span,
            message,
            vec![(kind.to_string(), Some(loan_span))],
        );
    }

    fn report_ref_escape(&mut self, span: Span, action: &str) {
        self.emit_diag(
            E_REF_ESCAPE,
            span,
            format!("une référence ne peut pas être {action} en Core 1.0"),
            vec![(
                "une référence ne vit que le temps de l'instruction ; seule `&'static str` est stockable"
                    .to_string(),
                None,
            )],
        );
    }

    fn display_place(&self, place: &Place) -> String {
        let Some(info) = self.slot_info(place.slot) else {
            return "<inconnu>".to_string();
        };
        let mut out = String::new();
        let mut derefs = 0;
        for step in &place.steps {
            if matches!(step, Step::Deref) {
                derefs += 1;
            }
        }
        for _ in 0..derefs {
            out.push('*');
        }
        out.push_str(&info.name);
        for step in &place.steps {
            match step {
                Step::Deref => {}
                Step::Field(name) => {
                    out.push('.');
                    out.push_str(name);
                }
                Step::Tuple(index) => {
                    out.push('.');
                    out.push_str(&index.to_string());
                }
                Step::Index(IndexKey::Lit(value)) => {
                    out.push('[');
                    out.push_str(&value.to_string());
                    out.push(']');
                }
                Step::Index(IndexKey::Opaque) => out.push_str("[_]"),
            }
        }
        out
    }
}

// ----------------------------------------------------------------------
// Pure helpers
// ----------------------------------------------------------------------

fn join(a: &State, b: &State) -> State {
    let mut slots = BTreeMap::new();
    for (slot, sa) in &a.slots {
        match b.slots.get(slot) {
            Some(sb) => {
                slots.insert(*slot, join_slot(sa, sb));
            }
            None => {
                slots.insert(*slot, sa.clone());
            }
        }
    }
    for (slot, sb) in &b.slots {
        if !a.slots.contains_key(slot) {
            slots.insert(*slot, sb.clone());
        }
    }
    State { slots }
}

fn join_all(states: &[State]) -> Option<State> {
    let mut iter = states.iter();
    let first = iter.next()?;
    let mut acc = first.clone();
    for st in iter {
        acc = join(&acc, st);
    }
    Some(acc)
}

fn join_slot(a: &SlotState, b: &SlotState) -> SlotState {
    let mut partial = a.partial.clone();
    for (path, rec_b) in &b.partial {
        partial
            .entry(path.clone())
            .and_modify(|rec| {
                rec.used_since = rec.used_since || rec_b.used_since;
            })
            .or_insert_with(|| rec_b.clone());
    }
    SlotState {
        init: a.init && b.init,
        moved: a.moved && b.moved,
        maybe_moved: a.maybe_moved || b.maybe_moved || (a.moved != b.moved),
        move_span: a.move_span.or(b.move_span),
        used_since: a.used_since || b.used_since,
        partial,
    }
}

fn drop_slot(state: &mut State, slot: SlotId) {
    state.slots.remove(&slot);
}

/// `inner` starts with `outer` (or equals it).
fn steps_start_with(inner: &[Step], outer: &[Step]) -> bool {
    inner.len() >= outer.len() && inner.iter().zip(outer.iter()).all(|(a, b)| a == b)
}

/// Two projection steps denote the same position. An index computed from a
/// non-literal expression is compatible with every index (conservative,
/// Q-OWN-5); mismatched projection kinds are compatible too.
fn step_compatible(a: &Step, b: &Step) -> bool {
    match (a, b) {
        (Step::Deref, Step::Deref) => true,
        (Step::Field(x), Step::Field(y)) => x == y,
        (Step::Tuple(x), Step::Tuple(y)) => x == y,
        (Step::Index(x), Step::Index(y)) => match (x, y) {
            (IndexKey::Lit(lx), IndexKey::Lit(ly)) => lx == ly,
            _ => true,
        },
        (Step::Deref, _) | (_, Step::Deref) => false,
        // Unknown/mismatched kinds: stay conservative.
        _ => true,
    }
}

fn prefix_compatible(short: &[Step], long: &[Step]) -> bool {
    short.len() <= long.len()
        && short
            .iter()
            .zip(long.iter())
            .all(|(a, b)| step_compatible(a, b))
}

/// True when the first incompatible projection of two paths proves that the
/// places cannot overlap.
fn diverges_disjoint(a: &[Step], b: &[Step]) -> bool {
    for (x, y) in a.iter().zip(b.iter()) {
        if !step_compatible(x, y) {
            return match (x, y) {
                (Step::Field(p), Step::Field(q)) => p != q,
                (Step::Tuple(p), Step::Tuple(q)) => p != q,
                (Step::Index(IndexKey::Lit(p)), Step::Index(IndexKey::Lit(q))) => p != q,
                (Step::Deref, _) | (_, Step::Deref) => true,
                // Opaque/different kinds: cannot prove disjointness.
                _ => false,
            };
        }
    }
    false
}

fn classify_partial(place: &[Step], moved: &[Step]) -> PartialRel {
    if place.len() == moved.len()
        && place
            .iter()
            .zip(moved.iter())
            .all(|(a, b)| step_compatible(a, b))
    {
        PartialRel::Exact
    } else if place.len() > moved.len() && prefix_compatible(moved, place) {
        PartialRel::Extension
    } else if place.len() < moved.len() && prefix_compatible(place, moved) {
        PartialRel::Ancestor
    } else if diverges_disjoint(place, moved) {
        PartialRel::Disjoint
    } else {
        // Overlapping but not comparable (e.g. opaque index): refuse.
        PartialRel::Extension
    }
}

fn places_overlap(a: &Place, b: &Place) -> bool {
    if a.slot != b.slot {
        return false;
    }
    let n = a.steps.len().min(b.steps.len());
    for i in 0..n {
        match (&a.steps[i], &b.steps[i]) {
            (Step::Deref, Step::Deref) => {}
            (Step::Field(x), Step::Field(y)) => {
                if x != y {
                    return false;
                }
            }
            (Step::Tuple(x), Step::Tuple(y)) => {
                if x != y {
                    return false;
                }
            }
            (Step::Index(x), Step::Index(y)) => {
                if x != y {
                    match (x, y) {
                        (IndexKey::Lit(lx), IndexKey::Lit(ly)) if lx != ly => return false,
                        _ => return true,
                    }
                }
            }
            (Step::Deref, _) | (_, Step::Deref) => return false,
            // Struct field vs tuple index (or other mismatched kinds): the
            // types cannot tell us, stay conservative.
            _ => return true,
        }
    }
    true
}

fn last_path_segment(path: &str) -> &str {
    let last = path.rsplit("::").next().unwrap_or(path);
    last.rsplit('.').next().unwrap_or(last)
}

fn builtin_method_is_mutating(path: &str) -> bool {
    matches!(
        last_path_segment(path),
        "push"
            | "push_str"
            | "pop"
            | "insert"
            | "remove"
            | "clear"
            | "retain"
            | "sort"
            | "sort_by"
            | "sort_by_key"
            | "reverse"
            | "truncate"
            | "extend"
            | "extend_from_slice"
            | "append"
            | "resize"
            | "reserve"
            | "reserve_exact"
            | "shrink_to_fit"
            | "swap"
            | "fill"
            | "drain"
            | "split_off"
            | "dedup"
            | "rotate_left"
            | "rotate_right"
            | "copy_from_slice"
    )
}

/// Builtin methods that store their argument *value* in the receiver, and can
/// therefore make a reference escape. Methods like `String::push_str`,
/// `Vec::extend` or `slice::fill` copy the pointee and must NOT be listed here:
/// `s.push_str(t.as_str())` is valid (`spec/language/ownership.md` OWN-6.3).
fn builtin_method_stores_refs(path: &str) -> bool {
    matches!(last_path_segment(path), "push" | "insert")
}

fn field_type(ty: &Type, name: &str, facts: &Facts) -> Type {
    match ty {
        Type::Nominal { def, .. } => {
            if let Some(info) = facts.structs.get(def) {
                info.fields
                    .iter()
                    .find(|f| f.name == name)
                    .map(|f| f.ty.clone())
                    .unwrap_or(Type::Error)
            } else {
                Type::Error
            }
        }
        _ => Type::Error,
    }
}

/// Copy classification (OWN-2.1/2.2). Nominal types use the `is_copy` flag
/// computed by the checker; missing facts are treated as mobile (conservative).
fn is_copy_type(ty: &Type, facts: &Facts) -> bool {
    match ty {
        Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128 => true,
        Type::U8 | Type::U16 | Type::U32 | Type::U64 | Type::U128 => true,
        Type::Isize | Type::Usize | Type::F32 | Type::F64 => true,
        Type::Bool | Type::Char | Type::Unit | Type::Str => true,
        Type::Never => true,
        Type::Ref { .. } => true,
        Type::Array { elem, .. } => is_copy_type(elem, facts),
        Type::Tuple(items) => items.iter().all(|t| is_copy_type(t, facts)),
        Type::Slice(_) => true,
        Type::String | Type::Vec(_) => false,
        Type::Option(inner) => is_copy_type(inner, facts),
        Type::Result(ok, err) => is_copy_type(ok, facts) && is_copy_type(err, facts),
        Type::Nominal { def, .. } => {
            if let Some(info) = facts.structs.get(def) {
                info.is_copy
            } else if let Some(info) = facts.enums.get(def) {
                info.is_copy
            } else {
                false
            }
        }
        // A generic parameter could be `Copy` or not: assume the worst.
        Type::Param { .. } => false,
        // `Type::Error` must not cascade ownership errors.
        Type::Error => true,
    }
}

impl Type {
    /// Copy or `Error` (never penalise a program whose typing already failed).
    fn is_error_or_copy(&self, facts: &Facts) -> bool {
        is_copy_type(self, facts)
    }
}
#[cfg(test)]
mod tests {
    use crate::facts::{CheckOptions, CheckResult, Profile};
    use ravum_lang_syntax::parser;
    use ravum_lang_syntax::source::SourceMap;

    /// Parse and check a complete program. The parser/checker are real: parse
    /// errors are a test failure, and ownership diagnostics only exist when the
    /// earlier phases succeeded (OWN-11.1).
    fn run(src: &str) -> CheckResult {
        run_at("src/lib.rv", src)
    }

    /// Same, as a binary crate (the conformance corpus uses `src/main.rv`).
    fn run_bin(src: &str) -> CheckResult {
        run_at("src/main.rv", src)
    }

    fn run_at(path: &str, src: &str) -> CheckResult {
        let mut sources = SourceMap::new();
        let file = sources.add(path, src);
        let parsed = parser::parse_file(file, src);
        let parse_codes: Vec<String> = parsed.diags.iter().map(|d| d.code.clone()).collect();
        assert!(
            parsed.diags.is_empty(),
            "unexpected parse diagnostics {parse_codes:?}:\n{}",
            parsed.diags.render(&sources)
        );
        let options = CheckOptions {
            profile: Profile::Core,
            edition: ravum_lang_syntax::LANGUAGE_EDITION.to_string(),
        };
        crate::check::check(&parsed.program, &sources, &options)
    }

    fn own_codes(result: &CheckResult) -> Vec<String> {
        result
            .diags
            .iter()
            .filter(|d| d.code.starts_with("E-OWN-"))
            .map(|d| d.code.clone())
            .collect()
    }

    fn failures(result: &CheckResult) -> Vec<(String, String)> {
        result
            .diags
            .iter()
            .map(|d| (d.code.clone(), d.message.clone()))
            .collect()
    }

    /// The program is accepted and produces no ownership diagnostic.
    fn accepted(src: &str) {
        let result = run(src);
        let codes = own_codes(&result);
        assert!(
            codes.is_empty(),
            "unexpected ownership diagnostics {codes:?}:\n{:?}",
            failures(&result)
        );
        assert!(
            result.checked.is_some(),
            "expected a checked program, got {:?}",
            failures(&result)
        );
    }

    /// The program is rejected and the exact ownership code is present.
    fn rejected(src: &str, code: &str) {
        let result = run(src);
        assert!(
            result.checked.is_none(),
            "expected rejection for {code}, but the program was accepted"
        );
        assert!(
            result.diags.iter().any(|d| d.code == code),
            "expected {code}, got {:?}",
            failures(&result)
        );
    }

    // -- Use after move / double move / move through calls ------------------

    #[test]
    fn use_after_move_is_rejected() {
        rejected(
            r#"
fn f(s: String) -> usize {
    let t = s;
    return s.len();
}
"#,
            "E-OWN-USE-AFTER-MOVE",
        );
    }

    #[test]
    fn double_move_is_rejected() {
        rejected(
            r#"
fn f(s: String) {
    let a = s;
    let b = s;
}
"#,
            "E-OWN-DOUBLE-MOVE",
        );
    }

    #[test]
    fn move_into_call_then_use_is_rejected() {
        rejected(
            r#"
fn g(t: String) {}
fn f(s: String) {
    g(s);
    g(s);
}
"#,
            "E-OWN-USE-AFTER-MOVE",
        );
    }

    #[test]
    fn copy_values_can_be_reused() {
        accepted(
            r#"
fn f(x: i32) -> i32 {
    let a = x;
    let b = x;
    return a + b;
}
"#,
        );
    }

    #[test]
    fn copy_struct_can_be_reused() {
        accepted(
            r#"
struct Point { x: i32, y: i32 }
fn f(p: Point) -> i32 {
    let q = p;
    return p.x + q.y;
}
"#,
        );
    }

    #[test]
    fn reassignment_after_move_is_allowed() {
        accepted(
            r#"
fn f() -> String {
    let mut s = String::from("a");
    let t = s;
    s = String::from("b");
    return t;
}
"#,
        );
    }

    // -- Borrow conflicts ----------------------------------------------------

    #[test]
    fn exclusive_borrow_conflicts_with_read() {
        rejected(
            r#"
fn h(a: &mut i32, b: i32) {}
fn f() {
    let mut x = 1;
    h(&mut x, x);
}
"#,
            "E-OWN-BORROW-CONFLICT",
        );
    }

    #[test]
    fn two_exclusive_arguments_conflict() {
        rejected(
            r#"
fn h(a: &mut i32, b: &mut i32) {}
fn f() {
    let mut x = 1;
    h(&mut x, &mut x);
}
"#,
            "E-OWN-BORROW-CONFLICT",
        );
    }

    #[test]
    fn shared_and_exclusive_arguments_conflict() {
        rejected(
            r#"
fn h(a: &i32, b: &mut i32) {}
fn f() {
    let mut x = 1;
    h(&x, &mut x);
}
"#,
            "E-OWN-BORROW-CONFLICT",
        );
    }

    #[test]
    fn move_during_shared_borrow_conflicts() {
        rejected(
            r#"
fn h(a: &String, b: String) {}
fn f() {
    let s = String::from("a");
    h(&s, s);
}
"#,
            "E-OWN-BORROW-CONFLICT",
        );
    }

    #[test]
    fn two_shared_arguments_are_allowed() {
        accepted(
            r#"
fn h(a: &i32, b: &i32) -> i32 { return *a + *b; }
fn f() -> i32 {
    let x = 1;
    return h(&x, &x);
}
"#,
        );
    }

    #[test]
    fn disjoint_fields_can_be_borrowed_exclusively() {
        accepted(
            r#"
struct P { x: i32, y: i32 }
fn h(a: &mut i32, b: &mut i32) {}
fn f() {
    let mut p = P { x: 1, y: 2 };
    h(&mut p.x, &mut p.y);
}
"#,
        );
    }

    #[test]
    fn sequential_method_calls_do_not_overlap() {
        accepted(
            r#"
fn f() {
    let mut v: Vec<i32> = Vec::new();
    v.push(1);
    v.push(2);
}
"#,
        );
    }

    // -- Reference escape ----------------------------------------------------

    #[test]
    fn reference_cannot_be_stored_in_a_let() {
        rejected(
            r#"
fn f() {
    let x = 1;
    let r = &x;
}
"#,
            "E-OWN-REF-ESCAPE",
        );
    }

    #[test]
    fn reference_parameter_cannot_be_returned() {
        rejected(
            r#"
fn f(x: &i32) -> &i32 { return x; }
"#,
            "E-OWN-REF-ESCAPE",
        );
    }

    #[test]
    fn reference_parameter_cannot_be_stored() {
        rejected(
            r#"
fn f(x: &i32) {
    let y = x;
}
"#,
            "E-OWN-REF-ESCAPE",
        );
    }

    #[test]
    fn as_str_cannot_be_stored() {
        rejected(
            r#"
fn f(s: String) {
    let t = s.as_str();
}
"#,
            "E-OWN-REF-ESCAPE",
        );
    }

    #[test]
    fn string_literal_is_static_and_storable() {
        accepted(
            r#"
fn f() -> &str {
    let l = "litteral";
    return l;
}
"#,
        );
    }

    #[test]
    fn temporary_reference_in_call_is_allowed() {
        accepted(
            r#"
fn f(s: String) {
    io::println(s.as_str());
}
"#,
        );
    }

    #[test]
    fn push_str_copies_the_pointee_and_accepts_a_temporary_reference() {
        accepted(
            r#"
fn f(mut s: String, t: String) {
    s.push_str(t.as_str());
}
"#,
        );
    }

    // -- Provenance interprocédurale du texte statique (finding F1) -----------

    #[test]
    fn static_text_from_function_is_storable() {
        accepted(
            r#"
fn lit() -> &str { "a" }
fn f() -> &str {
    let text = lit();
    return text;
}
"#,
        );
    }

    #[test]
    fn static_text_from_return_only_function_is_storable() {
        // The `lit` body has no tail expression: every explicit return is
        // static, which is enough for the fixpoint.
        accepted(
            r#"
fn lit() -> &str { return "a"; }
fn f() -> &str {
    let text = lit();
    return text;
}
"#,
        );
    }

    #[test]
    fn static_text_from_function_with_if_is_storable() {
        accepted(
            r#"
fn via_if(c: bool) -> &str {
    if c { "a" } else { "b" }
}
fn f(c: bool) -> &str {
    let text = via_if(c);
    return text;
}
"#,
        );
    }

    #[test]
    fn static_text_from_function_with_match_is_storable() {
        accepted(
            r#"
fn via_match(c: bool) -> &str {
    match c {
        true => "a",
        false => "b",
    }
}
fn f(c: bool) -> &str {
    let text = via_match(c);
    return text;
}
"#,
        );
    }

    #[test]
    fn non_static_text_function_stays_refused() {
        rejected(
            r#"
fn bad(s: &str) -> &str { s }
fn f(s: &str) -> &str {
    let text = bad(s);
    return text;
}
"#,
            "E-OWN-REF-ESCAPE",
        );
    }

    #[test]
    fn recursive_text_function_is_not_classified_static() {
        // No static base: the fixpoint must never add `rec`, so both its own
        // return and the caller's `let` are conservatively refused.
        rejected(
            r#"
fn rec() -> &str { rec() }
fn f() -> &str {
    let text = rec();
    return text;
}
"#,
            "E-OWN-REF-ESCAPE",
        );
    }

    // -- Definitive initialisation ------------------------------------------

    #[test]
    fn init_on_both_paths_is_accepted() {
        accepted(
            r#"
fn f(c: bool) -> i32 {
    let mut x: i32;
    if c { x = 1; } else { x = 2; }
    return x;
}
"#,
        );
    }

    #[test]
    fn init_on_one_path_is_rejected() {
        rejected(
            r#"
fn f(c: bool) -> i32 {
    let mut x: i32;
    if c { x = 1; }
    return x;
}
"#,
            "E-OWN-UNINIT",
        );
    }

    #[test]
    fn read_before_init_is_rejected() {
        rejected(
            r#"
fn f() -> i32 {
    let x: i32;
    return x;
}
"#,
            "E-OWN-UNINIT",
        );
    }

    #[test]
    fn borrow_before_init_is_rejected() {
        rejected(
            r#"
fn f() {
    let x: i32;
    let r = &x;
}
"#,
            "E-OWN-UNINIT",
        );
    }

    // -- Drop ----------------------------------------------------------------

    #[test]
    fn use_after_drop_is_rejected() {
        rejected(
            r#"
fn f() {
    let s = String::from("a");
    drop(s);
    io::println(s.as_str());
}
"#,
            "E-OWN-USE-AFTER-MOVE",
        );
    }

    #[test]
    fn drop_of_copy_is_a_noop() {
        accepted(
            r#"
fn f() {
    let n = 1;
    drop(n);
    let m = n + 1;
}
"#,
        );
    }

    // -- Loops ---------------------------------------------------------------

    #[test]
    fn move_in_loop_is_rejected() {
        rejected(
            r#"
fn f(v: Vec<i32>) {
    let s = String::from("x");
    for i in 0usize .. v.len() {
        drop(s);
    }
}
"#,
            "E-OWN-USE-AFTER-MOVE",
        );
    }

    #[test]
    fn borrowing_a_loop_invariant_is_allowed() {
        accepted(
            r#"
fn f(v: Vec<i32>) -> usize {
    let s = String::from("x");
    let mut n: usize = 0;
    for i in 0usize .. v.len() {
        n += s.len();
    }
    return n;
}
"#,
        );
    }

    #[test]
    fn init_in_loop_body_is_rejected_after_the_loop() {
        rejected(
            r#"
fn f(c: bool) -> i32 {
    let mut x: i32;
    while c { x = 1; }
    return x;
}
"#,
            "E-OWN-UNINIT",
        );
    }

    // -- Partial moves (design corrige : autorises et suivis) ----------------

    #[test]
    fn struct_field_move_is_accepted() {
        accepted(
            r#"
struct D { titre: String, n: i32 }
fn into_titre(d: D) -> String {
    return d.titre;
}
"#,
        );
    }

    #[test]
    fn other_fields_remain_usable_after_partial_move() {
        accepted(
            r#"
struct D { titre: String, n: i32 }
fn f(d: D) -> i32 {
    let t = d.titre;
    return d.n;
}
"#,
        );
    }

    #[test]
    fn whole_struct_use_after_partial_move_is_rejected() {
        rejected(
            r#"
struct D { titre: String, n: i32 }
fn f(d: D) -> D {
    let t = d.titre;
    return d;
}
"#,
            "E-OWN-PARTIAL-MOVE",
        );
    }

    #[test]
    fn moved_field_cannot_be_read_again() {
        rejected(
            r#"
struct D { titre: String }
fn f(d: D) -> usize {
    let t = d.titre;
    return d.titre.len();
}
"#,
            "E-OWN-USE-AFTER-MOVE",
        );
    }

    #[test]
    fn tuple_element_move_is_accepted() {
        accepted(
            r#"
fn f(t: (String, i32)) -> String {
    return t.0;
}
"#,
        );
    }

    #[test]
    fn tuple_whole_use_after_partial_move_is_rejected() {
        rejected(
            r#"
fn f(t: (String, i32)) -> (String, i32) {
    let s = t.0;
    return t;
}
"#,
            "E-OWN-PARTIAL-MOVE",
        );
    }

    #[test]
    fn moving_a_field_out_of_a_borrow_is_rejected() {
        rejected(
            r#"
struct D { titre: String }
fn f(d: &mut D) -> String {
    return d.titre;
}
"#,
            "E-OWN-MOVE-BORROWED",
        );
    }

    #[test]
    fn moving_a_vec_element_by_index_is_rejected() {
        rejected(
            r#"
fn f(v: Vec<String>) -> String {
    return v[0];
}
"#,
            "E-OWN-MOVE-INDEXED",
        );
    }

    #[test]
    fn opaque_index_after_array_partial_move_is_rejected() {
        rejected(
            r#"
fn f(a: [String; 2], i: usize) -> usize {
    let s = a[0];
    return a[i].len();
}
"#,
            "E-OWN-USE-AFTER-MOVE",
        );
    }

    #[test]
    fn distinct_literal_array_element_stays_usable_after_partial_move() {
        accepted(
            r#"
fn f(a: [String; 2]) -> usize {
    let s = a[0];
    return a[1].len();
}
"#,
        );
    }

    // -- Reference parameters / reborrows ------------------------------------

    #[test]
    fn shared_reference_parameter_usable_throughout() {
        accepted(
            r#"
fn compte(v: &Vec<i32>) -> usize {
    return v.len();
}
"#,
        );
    }

    #[test]
    fn exclusive_reference_parameter_can_be_reborrowed() {
        accepted(
            r#"
fn ajoute(v: &mut Vec<i32>) {
    v.push(1);
    v.push(2);
}
"#,
        );
    }

    // -- Corpus de réfutation `conformance/cases/ownership` ------------------
    // Ces cas sont la porte T4 : leurs codes attendus doivent rester exacts.

    fn conformance_fails(src: &str, code: &str) {
        let result = run_bin(src);
        assert!(
            result.checked.is_none(),
            "expected rejection for {code}, but the program was accepted"
        );
        // The conformance runner compares the *set* of diagnostic codes
        // exactly; online ownership must not add extra codes to the case.
        let actual: std::collections::BTreeSet<String> =
            result.diags.iter().map(|d| d.code.clone()).collect();
        let expected: std::collections::BTreeSet<String> =
            std::iter::once(code.to_string()).collect();
        assert_eq!(
            actual,
            expected,
            "conformance case expects exactly {{{code}}}, got {:?}",
            failures(&result)
        );
    }

    #[test]
    fn conformance_use_after_move() {
        conformance_fails(
            r#"
fn h(s: String) -> usize {
    let t = s;
    return s.len();
}

fn main() {
}
"#,
            "E-OWN-USE-AFTER-MOVE",
        );
    }

    #[test]
    fn conformance_double_move() {
        conformance_fails(
            r#"
fn f(s: String) {
    let a = s;
    let b = s;
}

fn main() {
}
"#,
            "E-OWN-DOUBLE-MOVE",
        );
    }

    #[test]
    fn conformance_move_in_call() {
        conformance_fails(
            r#"
use std::io;

fn take(s: String) {
}

fn main() {
    let s = String::from("a");
    take(s);
    io::println(s.as_str());
}
"#,
            "E-OWN-USE-AFTER-MOVE",
        );
    }

    #[test]
    fn conformance_drop_then_use() {
        conformance_fails(
            r#"
use std::io;

fn main() {
    let s = String::from("a");
    drop(s);
    io::println(s.as_str());
}
"#,
            "E-OWN-USE-AFTER-MOVE",
        );
    }

    #[test]
    fn conformance_borrow_conflict_shared_mut() {
        conformance_fails(
            r#"
fn f2(a: &i32, b: &mut i32) {
}

fn main() {
    let mut x = 1;
    f2(&x, &mut x);
}
"#,
            "E-OWN-BORROW-CONFLICT",
        );
    }

    #[test]
    fn conformance_borrow_conflict_two_mut() {
        conformance_fails(
            r#"
fn f(a: &mut i32, b: &mut i32) {
}

fn main() {
    let mut x = 1;
    f(&mut x, &mut x);
}
"#,
            "E-OWN-BORROW-CONFLICT",
        );
    }

    #[test]
    fn conformance_ref_escape() {
        conformance_fails(
            r#"
fn bad(v: &Vec<i32>) -> &Vec<i32> {
    return v;
}

fn main() {
}
"#,
            "E-OWN-REF-ESCAPE",
        );
    }

    #[test]
    fn conformance_copy_ok() {
        let result = run_bin(
            r#"
struct Point { x: i32, y: i32 }

fn f(p: Point) -> Point {
    let q = p;
    return p;
}

fn main() {
    let a = 1;
    let b = a;
    let p = Point { x: 1, y: 2 };
    let q = p;
    let s = f(q);
}
"#,
        );
        assert!(
            result.checked.is_some(),
            "expected acceptance, got {:?}",
            failures(&result)
        );
    }

    #[test]
    fn conformance_conditional_init() {
        conformance_fails(
            r#"
fn f(c: bool) -> i32 {
    let x: i32;
    if c {
        x = 1;
    }
    return x;
}

fn main() {
}
"#,
            "E-OWN-UNINIT",
        );
    }
}
