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
use crate::resolve::{self, ModuleId, NominalKind, PathError, ValuePath, World};
use crate::types::{contains_never, Type};
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
    let mut checker = TypeChecker {
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
    };
    checker.run(sources);
    compute_static_str_fns(program, &mut *checker.facts);
    resolve::emit_unused_import_warnings(world, checker.diags);
    !checker.diags.has_errors()
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

/// Calcule `Facts::static_str_fns` par point fixe (monotone, borné par le
/// nombre de candidats).
fn compute_static_str_fns(program: &ast::Program, facts: &mut Facts) {
    let mut candidates: Vec<(&FnItem, NodeId)> = Vec::new();
    collect_static_str_candidates(&program.root, facts, &mut candidates);
    facts.static_str_fns.clear();
    let mut changed = true;
    let mut rounds = 0usize;
    while changed && rounds <= candidates.len() {
        changed = false;
        for (f, id) in &candidates {
            if facts.static_str_fns.contains(id) {
                continue;
            }
            if fn_returns_static_text(f, facts) {
                facts.static_str_fns.insert(*id);
                changed = true;
            }
        }
        rounds += 1;
    }
}

fn collect_static_str_candidates<'a>(
    module: &'a Module,
    facts: &Facts,
    out: &mut Vec<(&'a FnItem, NodeId)>,
) {
    for item in &module.items {
        match &item.kind {
            ItemKind::Fn(f) => {
                if fn_sig_returns_static_str(item.id, f, facts) {
                    out.push((f, item.id));
                }
            }
            ItemKind::Mod(inner) => collect_static_str_candidates(inner, facts, out),
            ItemKind::Impl(imp) => {
                for sub in &imp.items {
                    match &sub.kind {
                        ItemKind::Fn(f) => {
                            if fn_sig_returns_static_str(sub.id, f, facts) {
                                out.push((f, sub.id));
                            }
                        }
                        ItemKind::Mod(inner) => collect_static_str_candidates(inner, facts, out),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

fn fn_sig_returns_static_str(item: NodeId, f: &FnItem, facts: &Facts) -> bool {
    let ret = facts
        .fn_sigs
        .get(&item)
        .map(|sig| sig.ret.clone())
        .or_else(|| facts.type_nodes.get(&f.ret.id).cloned());
    matches!(ret, Some(ty) if is_static_str_type(&ty))
}

fn is_static_str_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Ref {
            mutable: false,
            inner,
        } if matches!(**inner, Type::Str)
    )
}

/// `true` when every path returning from the function yields a static text.
fn fn_returns_static_text(f: &FnItem, facts: &Facts) -> bool {
    let mut seen_return = false;
    if !block_returns_are_static(&f.body, facts, &mut seen_return) {
        return false;
    }
    match &f.body.tail {
        Some(tail) => is_static_text_expr(tail, facts),
        // No tail: a well-typed `&str` function must return explicitly on every
        // path; all those returns were checked above.
        None => seen_return,
    }
}

/// Checks every explicit `return` reachable in the block (including nested
/// blocks and expressions) and records that at least one exists.
fn block_returns_are_static(block: &Block, facts: &Facts, seen: &mut bool) -> bool {
    for stmt in &block.stmts {
        let ok = match &stmt.kind {
            StmtKind::Let(l) => l
                .init
                .as_ref()
                .map(|init| expr_returns_are_static(init, facts, seen))
                .unwrap_or(true),
            StmtKind::Expr { expr, .. } => expr_returns_are_static(expr, facts, seen),
        };
        if !ok {
            return false;
        }
    }
    match &block.tail {
        Some(tail) => expr_returns_are_static(tail, facts, seen),
        None => true,
    }
}

fn expr_returns_are_static(expr: &Expr, facts: &Facts, seen: &mut bool) -> bool {
    match &expr.kind {
        ExprKind::Return(Some(value)) => {
            *seen = true;
            is_static_text_expr(value, facts) && expr_returns_are_static(value, facts, seen)
        }
        ExprKind::Return(None) => false,
        ExprKind::Call { callee, args } => {
            expr_returns_are_static(callee, facts, seen)
                && args
                    .iter()
                    .all(|arg| expr_returns_are_static(arg, facts, seen))
        }
        ExprKind::MethodCall { receiver, args, .. } => {
            expr_returns_are_static(receiver, facts, seen)
                && args
                    .iter()
                    .all(|arg| expr_returns_are_static(arg, facts, seen))
        }
        ExprKind::Field { base, .. } | ExprKind::TupleIndex { base, .. } => {
            expr_returns_are_static(base, facts, seen)
        }
        ExprKind::Index { base, index } => {
            expr_returns_are_static(base, facts, seen)
                && expr_returns_are_static(index, facts, seen)
        }
        ExprKind::Unary { expr, .. } | ExprKind::Cast { expr, .. } | ExprKind::Try(expr) => {
            expr_returns_are_static(expr, facts, seen)
        }
        ExprKind::Binary { lhs, rhs, .. } => {
            expr_returns_are_static(lhs, facts, seen) && expr_returns_are_static(rhs, facts, seen)
        }
        ExprKind::Assign { target, value } | ExprKind::AssignOp { target, value, .. } => {
            expr_returns_are_static(target, facts, seen)
                && expr_returns_are_static(value, facts, seen)
        }
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => {
            expr_returns_are_static(cond, facts, seen)
                && block_returns_are_static(then_block, facts, seen)
                && else_branch
                    .as_ref()
                    .map(|branch| expr_returns_are_static(branch, facts, seen))
                    .unwrap_or(true)
        }
        ExprKind::Match { scrutinee, arms } => {
            expr_returns_are_static(scrutinee, facts, seen)
                && arms
                    .iter()
                    .all(|arm| expr_returns_are_static(&arm.body, facts, seen))
        }
        ExprKind::While { cond, body } => {
            expr_returns_are_static(cond, facts, seen)
                && block_returns_are_static(body, facts, seen)
        }
        ExprKind::Loop { body } => block_returns_are_static(body, facts, seen),
        ExprKind::For { iter, body, .. } => {
            expr_returns_are_static(iter, facts, seen)
                && block_returns_are_static(body, facts, seen)
        }
        ExprKind::Block(block) | ExprKind::Unsafe(block) => {
            block_returns_are_static(block, facts, seen)
        }
        ExprKind::Tuple(items) | ExprKind::Array(items) => items
            .iter()
            .all(|item| expr_returns_are_static(item, facts, seen)),
        ExprKind::ArrayRepeat { value, len } => {
            expr_returns_are_static(value, facts, seen) && expr_returns_are_static(len, facts, seen)
        }
        ExprKind::StructLit { fields, .. } => fields
            .iter()
            .all(|field| expr_returns_are_static(&field.value, facts, seen)),
        ExprKind::Range { start, end, .. } => {
            start
                .as_ref()
                .map(|start| expr_returns_are_static(start, facts, seen))
                .unwrap_or(true)
                && end
                    .as_ref()
                    .map(|end| expr_returns_are_static(end, facts, seen))
                    .unwrap_or(true)
        }
        // No `return` can hide in these, and they are never a static text.
        ExprKind::Unit
        | ExprKind::Int { .. }
        | ExprKind::Float { .. }
        | ExprKind::Bool(_)
        | ExprKind::Char(_)
        | ExprKind::Str(_)
        | ExprKind::SelfValue
        | ExprKind::Path(_)
        | ExprKind::Break
        | ExprKind::Continue => true,
    }
}

/// `true` when the expression certainly evaluates to a `'static` text value.
fn is_static_text_expr(expr: &Expr, facts: &Facts) -> bool {
    match &expr.kind {
        ExprKind::Str(_) => true,
        // Interprocedural propagation: only user functions classified by the
        // fixpoint; never a builtin (`as_str` and friends must stay non-static).
        ExprKind::Call { .. } => matches!(
            facts.calls.get(&expr.id),
            Some(CallTarget::UserFn { item }) if facts.static_str_fns.contains(item)
        ),
        ExprKind::If {
            then_block,
            else_branch: Some(else_branch),
            ..
        } => block_value_is_static(then_block, facts) && is_static_text_expr(else_branch, facts),
        ExprKind::Match { arms, .. } => {
            !arms.is_empty() && arms.iter().all(|arm| is_static_text_expr(&arm.body, facts))
        }
        ExprKind::Block(block) | ExprKind::Unsafe(block) => block_value_is_static(block, facts),
        // A diverging branch that returns a static text contributes a static
        // value on every path that actually produces one.
        ExprKind::Return(Some(value)) => is_static_text_expr(value, facts),
        _ => false,
    }
}

/// The value of a block is its tail expression; a block with no tail produces
/// `()` (never a static text). Conservative by design.
fn block_value_is_static(block: &Block, facts: &Facts) -> bool {
    block
        .tail
        .as_ref()
        .map(|tail| is_static_text_expr(tail, facts))
        .unwrap_or(false)
}

#[derive(Clone, Debug)]
struct Local {
    ty: Type,
    mutable: bool,
    /// `let x: T;` sans initialiseur : l'affectation initiale est permise même
    /// sans `mut` (l'initialisation définie est vérifiée par `own`).
    deferred: bool,
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
    /// right-hand side, match arm/scrutinee, array repeat). EXPR-5.11/6.7/G-6.19
    /// forbid `while`/`loop`/`for` as value expressions; only a direct statement
    /// (or a `()`-block tail) may contain one.
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
}

impl<'p, 'w, 'f> TypeChecker<'p, 'w, 'f> {
    // ------------------------------------------------------------------
    // Diagnostics
    // ------------------------------------------------------------------

    fn error(&mut self, code: &'static str, span: Span, message: impl Into<String>) {
        self.diags.error(code, span, message);
    }

    fn error_note(
        &mut self,
        code: &'static str,
        span: Span,
        message: impl Into<String>,
        note: impl Into<String>,
        note_span: Option<Span>,
    ) {
        self.diags
            .push(Diagnostic::error(code, span, message).with_note(note, note_span));
    }

    fn warning(&mut self, code: &'static str, span: Span, message: impl Into<String>) {
        self.diags.warning(code, span, message);
    }

    // ------------------------------------------------------------------
    // Portée et locales
    // ------------------------------------------------------------------

    fn push_scope(&mut self) {
        self.scopes.push(BTreeMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn bind_local(&mut self, name: &str, ty: Type, mutable: bool, deferred: bool) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(
                name.to_string(),
                Local {
                    ty,
                    mutable,
                    deferred,
                },
            );
        }
    }

    fn lookup_local(&self, name: &str) -> Option<Local> {
        for scope in self.scopes.iter().rev() {
            if let Some(local) = scope.get(name) {
                return Some(local.clone());
            }
        }
        None
    }

    // ------------------------------------------------------------------
    // Types
    // ------------------------------------------------------------------

    fn resolve_type(&mut self, ty: &AstType) -> Type {
        resolve::resolve_type_syntax(
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

    fn compatible(a: &Type, b: &Type) -> bool {
        a == b
            || matches!(a, Type::Never)
            || matches!(b, Type::Never)
            || matches!(a, Type::Error)
            || matches!(b, Type::Error)
    }

    fn unify(&mut self, found: &Type, expected: &Type, span: Span, context: &str) -> bool {
        let found = self.resolve_vars(found);
        let expected = self.resolve_vars(expected);
        if Self::compatible(&found, &expected) {
            return true;
        }
        if self.has_infer_var(&found) || self.has_infer_var(&expected) {
            if self.unify_pattern(&expected, &found, &mut BTreeMap::new()) {
                return true;
            }
        }
        self.error(
            "E-TYP-MISMATCH",
            span,
            format!("{context}: expected `{expected}`, found `{found}`"),
        );
        false
    }

    fn has_infer_var(&self, ty: &Type) -> bool {
        has_infer_var_rec(ty)
    }

    fn check_value_expr(&mut self, expr: &Expr, expected: Option<&Type>) -> Type {
        self.value_position += 1;
        let ty = self.check_expr(expr, expected);
        self.value_position -= 1;
        ty
    }

    fn record_expr_type(&mut self, id: NodeId, ty: &Type) {
        self.facts.expr_types.insert(id, ty.clone());
    }

    // ------------------------------------------------------------------
    // Inférence (variables de type)
    // ------------------------------------------------------------------

    fn fresh_var(&mut self, span: Span) -> Type {
        let index = self.vars.len();
        self.vars.push(None);
        self.var_spans.push(span);
        Type::Param {
            name: format!("${index}"),
            index: VAR_BASE + index,
        }
    }

    fn resolve_vars(&self, ty: &Type) -> Type {
        resolve_vars_keep(&self.vars, ty)
    }

    fn bind_var(&mut self, index: usize, ty: &Type) -> bool {
        let ty = self.resolve_vars(ty);
        if infer_var_index(&ty) == Some(index) {
            return true;
        }
        if self.var_occurs(index, &ty) {
            return false;
        }
        // TYP-1.6: `!` is not an inhabitable type argument. Binding an
        // inference variable to `!` (or to e.g. `Vec<!>`) would generate
        // uninhabitable Rust (`Vec<!>` needs a nightly feature). `!` coerces to
        // the expected type; it must never *determine* it. Leaving the variable
        // unbound lets a real constraint bind it, or yields E-TYP-AMBIGUOUS
        // instead of a false accept.
        if contains_never(&ty) {
            return true;
        }
        if let Some(slot) = self.vars.get_mut(index) {
            *slot = Some(ty);
            true
        } else {
            false
        }
    }

    fn var_occurs(&self, target: usize, ty: &Type) -> bool {
        if let Some(index) = infer_var_index(ty) {
            if index == target {
                return true;
            }
            return match self.vars.get(index).and_then(|v| v.clone()) {
                Some(bound) => self.var_occurs(target, &bound),
                None => false,
            };
        }
        match ty {
            Type::Ref { inner, .. } => self.var_occurs(target, inner),
            Type::Array { elem, .. } => self.var_occurs(target, elem),
            Type::Slice(inner) | Type::Vec(inner) | Type::Option(inner) => {
                self.var_occurs(target, inner)
            }
            Type::Tuple(items) => items.iter().any(|t| self.var_occurs(target, t)),
            Type::Result(ok, err) => self.var_occurs(target, ok) || self.var_occurs(target, err),
            Type::Nominal { args, .. } => args.iter().any(|t| self.var_occurs(target, t)),
            _ => false,
        }
    }

    /// Remplace les paramètres génériques de déclaration (`index < VAR_BASE`)
    /// par des variables d'inférence fraîches, de façon consistante.
    fn instantiate(&mut self, ty: &Type, span: Span) -> Type {
        let mut mapping: BTreeMap<usize, Type> = BTreeMap::new();
        self.instantiate_with(ty, span, &mut mapping)
    }

    fn instantiate_with(
        &mut self,
        ty: &Type,
        span: Span,
        mapping: &mut BTreeMap<usize, Type>,
    ) -> Type {
        match ty {
            Type::Param { .. } if infer_var_index(ty).is_none() => {
                let Type::Param { index, .. } = ty else {
                    unreachable!()
                };
                mapping
                    .entry(*index)
                    .or_insert_with(|| self.fresh_var(span))
                    .clone()
            }
            Type::Ref { mutable, inner } => Type::Ref {
                mutable: *mutable,
                inner: Box::new(self.instantiate_with(inner, span, mapping)),
            },
            Type::Array { elem, len } => Type::Array {
                elem: Box::new(self.instantiate_with(elem, span, mapping)),
                len: *len,
            },
            Type::Slice(inner) => {
                Type::Slice(Box::new(self.instantiate_with(inner, span, mapping)))
            }
            Type::Tuple(items) => Type::Tuple(
                items
                    .iter()
                    .map(|t| self.instantiate_with(t, span, mapping))
                    .collect(),
            ),
            Type::Vec(inner) => Type::Vec(Box::new(self.instantiate_with(inner, span, mapping))),
            Type::Option(inner) => {
                Type::Option(Box::new(self.instantiate_with(inner, span, mapping)))
            }
            Type::Result(ok, err) => Type::Result(
                Box::new(self.instantiate_with(ok, span, mapping)),
                Box::new(self.instantiate_with(err, span, mapping)),
            ),
            Type::Nominal { def, name, args } => Type::Nominal {
                def: *def,
                name: name.clone(),
                args: args
                    .iter()
                    .map(|t| self.instantiate_with(t, span, mapping))
                    .collect(),
            },
            other => other.clone(),
        }
    }

    /// Unification avec variables d'inférence et paramètres de déclaration.
    fn unify_pattern(
        &mut self,
        pattern: &Type,
        actual: &Type,
        subst: &mut BTreeMap<usize, Type>,
    ) -> bool {
        let p = self.resolve_vars(pattern);
        let a = self.resolve_vars(actual);
        if matches!(a, Type::Never | Type::Error) || matches!(p, Type::Error) {
            return true;
        }
        if let Some(index) = infer_var_index(&p) {
            return self.bind_var(index, &a);
        }
        if let Some(index) = infer_var_index(&a) {
            let concrete = substitute(&p, subst);
            return self.bind_var(index, &concrete);
        }
        match (&p, &a) {
            (Type::Param { index, .. }, actual) => match subst.get(index) {
                Some(existing) => existing == actual,
                None => {
                    subst.insert(*index, actual.clone());
                    true
                }
            },
            (
                Type::Ref {
                    mutable: pm,
                    inner: pi,
                },
                Type::Ref {
                    mutable: am,
                    inner: ai,
                },
            ) => pm == am && self.unify_pattern(pi, ai, subst),
            (Type::Array { elem: pe, len: pl }, Type::Array { elem: ae, len: al }) => {
                pl == al && self.unify_pattern(pe, ae, subst)
            }
            (Type::Slice(pi), Type::Slice(ai)) => self.unify_pattern(pi, ai, subst),
            (Type::Tuple(ps), Type::Tuple(as_)) => {
                ps.len() == as_.len()
                    && ps
                        .iter()
                        .zip(as_.iter())
                        .all(|(p, a)| self.unify_pattern(p, a, subst))
            }
            (Type::Vec(pi), Type::Vec(ai)) => self.unify_pattern(pi, ai, subst),
            (Type::Option(pi), Type::Option(ai)) => self.unify_pattern(pi, ai, subst),
            (Type::Result(po, pe), Type::Result(ao, ae)) => {
                self.unify_pattern(po, ao, subst) && self.unify_pattern(pe, ae, subst)
            }
            (
                Type::Nominal {
                    def: pd, args: pa, ..
                },
                Type::Nominal {
                    def: ad, args: aa, ..
                },
            ) => {
                pd == ad
                    && pa.len() == aa.len()
                    && pa
                        .iter()
                        .zip(aa.iter())
                        .all(|(p, a)| self.unify_pattern(p, a, subst))
            }
            (p, a) => p == a,
        }
    }

    /// Résout toutes les variables d'inférence dans les faits et signale les
    /// variables encore libres (`E-TYP-AMBIGUOUS`).
    fn finalize_inference(&mut self) {
        let mut unresolved: BTreeSet<usize> = BTreeSet::new();
        {
            let vars = &self.vars;
            for ty in self.facts.expr_types.values_mut() {
                *ty = resolve_vars_final(vars, ty, &mut unresolved);
            }
            for ty in self.facts.pat_types.values_mut() {
                *ty = resolve_vars_final(vars, ty, &mut unresolved);
            }
            for ty in self.facts.type_nodes.values_mut() {
                *ty = resolve_vars_final(vars, ty, &mut unresolved);
            }
            for target in self.facts.methods.values_mut() {
                if let MethodTarget::Builtin { ret, .. } = target {
                    *ret = resolve_vars_final(vars, ret, &mut unresolved);
                }
            }
        }
        for index in unresolved {
            let span = self.var_spans.get(index).copied().unwrap_or(Span::dummy());
            self.error(
                "E-TYP-AMBIGUOUS",
                span,
                "cannot infer this type; add an explicit type annotation".to_string(),
            );
        }
    }

    // ------------------------------------------------------------------
    // Entrée
    // ------------------------------------------------------------------

    fn run(&mut self, sources: Option<&SourceMap>) {
        self.validate_entry_points(sources);
        // Constantes (avant les corps : les corps peuvent les lire).
        let consts: Vec<(ModuleId, NodeId, &'p ConstItem)> = self
            .world
            .consts
            .iter()
            .map(|(id, c)| {
                (
                    self.world.item_module.get(id).copied().unwrap_or(0),
                    *id,
                    *c,
                )
            })
            .collect();
        for (module, id, c) in consts {
            self.check_const(module, id, c);
        }
        // Corps des fonctions de modules.
        let asts: Vec<&'p Module> = self.world.module_asts.clone();
        let mut fns: Vec<(ModuleId, NodeId, &'p FnItem)> = Vec::new();
        for (module, ast) in asts.iter().enumerate() {
            for item in &ast.items {
                if let ItemKind::Fn(f) = &item.kind {
                    fns.push((module, item.id, f));
                }
            }
        }
        for (module, id, f) in fns {
            self.check_fn(module, id, f, None);
        }
        // Méthodes d'impl.
        let impl_items: Vec<(ModuleId, &'p Item, &'p ast::ImplItem)> =
            self.world.impl_items.clone();
        let mut methods: Vec<(ModuleId, NodeId, &'p FnItem, Type, Vec<String>)> = Vec::new();
        for (module, _, imp) in impl_items {
            let self_ty = self
                .facts
                .type_nodes
                .get(&imp.self_ty.id)
                .cloned()
                .unwrap_or(Type::Error);
            let generics: Vec<String> = imp.generics.iter().map(|g| g.name.name.clone()).collect();
            for member in &imp.items {
                if let ItemKind::Fn(f) = &member.kind {
                    methods.push((module, member.id, f, self_ty.clone(), generics.clone()));
                }
            }
        }
        for (module, id, f, self_ty, generics) in methods {
            self.check_fn(module, id, f, Some((self_ty, generics)));
        }
        self.finalize_inference();
    }

    fn validate_entry_points(&mut self, sources: Option<&SourceMap>) {
        // `main` : uniquement dans le module racine (toutes les déclarations
        // sont comptées, y compris les doublons ignorés par la résolution).
        let mains: Vec<NodeId> = {
            let root_ast = self.world.module_asts[self.world.root];
            root_ast
                .items
                .iter()
                .filter_map(|item| match &item.kind {
                    ItemKind::Fn(f) if f.name.name == "main" => Some(item.id),
                    _ => None,
                })
                .collect()
        };
        let binary = match sources {
            Some(sources) => sources
                .file(self.program.file)
                .map(|f| f.path.ends_with("main.rv"))
                .unwrap_or(true),
            None => true,
        };
        if mains.is_empty() {
            if binary {
                self.error(
                    "E-TYP-MAIN-MISSING",
                    self.program.root.span,
                    "binary crate requires exactly one `fn main()`".to_string(),
                );
            }
        } else {
            self.facts.main_fn = Some(mains[0]);
            if mains.len() > 1 {
                for id in &mains[1..] {
                    let span = self
                        .world
                        .fns
                        .get(id)
                        .map(|f| f.span)
                        .unwrap_or(Span::dummy());
                    self.error(
                        "E-TYP-MAIN-DUPLICATE",
                        span,
                        "a binary crate must define exactly one `main`".to_string(),
                    );
                }
            }
            let span = self
                .world
                .fns
                .get(&mains[0])
                .map(|f| f.span)
                .unwrap_or(Span::dummy());
            self.validate_main_signature(mains[0], span);
        }

        // `test fn` : pas de paramètre, retour `()` ou `Result<(), Error>`.
        // Une signature invalide est une erreur de **typage** nommée
        // `E-TYP-MAIN` par `diagnostics.md` (« `test fn` invalide »), et non une
        // erreur de syntaxe : G-3.6 et MOD-4.10 décrivent la contrainte de
        // signature, pas une forme grammaticale refusée par le parseur.
        let tests = self.facts.test_fns.clone();
        for id in tests {
            let Some(sig) = self.facts.fn_sigs.get(&id).cloned() else {
                continue;
            };
            let span = self
                .world
                .fns
                .get(&id)
                .map(|f| f.span)
                .unwrap_or(Span::dummy());
            if !sig.params.is_empty() {
                self.error(
                    "E-TYP-MAIN",
                    span,
                    "a test function takes no parameter".to_string(),
                );
            }
            if !(sig.ret == Type::Unit || self.is_result_unit_error(&sig.ret)) {
                self.error(
                    "E-TYP-MAIN",
                    span,
                    "a test function returns `()` or `Result<(), Error>`".to_string(),
                );
            }
        }
    }

    fn validate_main_signature(&mut self, id: NodeId, span: Span) {
        let Some(sig) = self.facts.fn_sigs.get(&id).cloned() else {
            return;
        };
        let mut ok = true;
        if !sig.params.is_empty() {
            ok = false;
            self.error(
                "E-TYP-MAIN",
                span,
                "`main` must not take parameters".to_string(),
            );
        }
        if !(sig.ret == Type::Unit || self.is_result_unit_error(&sig.ret)) {
            ok = false;
            self.error_note(
                "E-TYP-MAIN",
                span,
                format!("invalid return type `{}` for `main`", sig.ret),
                "expected `()` or `Result<(), Error>`",
                None,
            );
        }
        let _ = ok;
    }

    fn is_result_unit_error(&self, ty: &Type) -> bool {
        matches!(ty, Type::Result(ok, err) if **ok == Type::Unit && builtins::is_error_type(err))
    }

    // ------------------------------------------------------------------
    // Fonctions
    // ------------------------------------------------------------------

    fn check_fn(
        &mut self,
        module: ModuleId,
        item: NodeId,
        f: &FnItem,
        impl_ctx: Option<(Type, Vec<String>)>,
    ) {
        let Some(sig) = self.facts.fn_sigs.get(&item).cloned() else {
            return;
        };
        self.module = module;
        self.generics = match &impl_ctx {
            Some((_, impl_generics)) => impl_generics
                .iter()
                .enumerate()
                .map(|(i, n)| (n.clone(), i))
                .collect(),
            None => f
                .generics
                .iter()
                .enumerate()
                .map(|(i, g)| (g.name.name.clone(), i))
                .collect(),
        };
        self.self_ty = impl_ctx.as_ref().map(|(t, _)| t.clone());
        self.fn_ret = sig.ret.clone();
        self.current_fn = Some(item);
        self.in_const = false;
        self.loop_depth = 0;
        self.scopes.clear();
        self.push_scope();
        self.unused_candidates.clear();
        self.used_locals.clear();
        for param in &sig.params {
            let mutable = param.mutable
                || (param.is_receiver && matches!(sig.receiver, Some(Receiver::Exclusive)));
            self.bind_local(&param.name, param.ty.clone(), mutable, false);
        }
        let body_ty = self.check_block(&f.body, Some(&sig.ret));
        // DIAG-1.6: `W-OWN-UNUSED-VARIABLE` is one of the two warnings Core 1.0
        // documents. A binding is unused when no path resolved it inside the
        // function; shadowing is handled conservatively (a name used once counts
        // for every binding of that name, so the warning can be missed but never
        // invented).
        for (name, span) in std::mem::take(&mut self.unused_candidates) {
            if !self.used_locals.contains(&name) {
                self.warning(
                    "W-OWN-UNUSED-VARIABLE",
                    span,
                    format!("variable `{name}` is never used"),
                );
            }
        }
        let body_ty = self.resolve_vars(&body_ty);
        let ret = self.resolve_vars(&sig.ret);
        // G-5.11 / TYP-1.6: a function declared `-> !` must have a divergent
        // body. `compatible` treats `!` as bottom in both directions, so the
        // normal body check below cannot catch a body that has type `()`.
        // EXPR-5.11/6.7/G-6.19 type `loop`/`while`/`for` as `()`, even when the
        // loop diverges, so only `panic`, `return` or a call to a divergent
        // function can make the body divergent.
        if matches!(ret, Type::Never) && !matches!(body_ty, Type::Never | Type::Error) {
            self.error(
                "E-TYP-RETURN",
                f.body.span,
                "a function returning `!` requires a divergent body; `loop`/`while`/`for` \
                 have type `()` (EXPR-5.11/6.7), so use `panic`, `return` or a call to a \
                 divergent function"
                    .to_string(),
            );
        }
        if !Self::compatible(&body_ty, &ret)
            && !self.unify(&body_ty, &ret, f.body.span, "function body")
        {
            self.error(
                "E-TYP-MISMATCH",
                f.body.span,
                format!(
                    "function body returns `{body_ty}` but `{}` returns `{ret}`",
                    sig.name
                ),
            );
        }
        self.pop_scope();
        self.current_fn = None;
        self.self_ty = None;
        self.generics.clear();
    }

    // ------------------------------------------------------------------
    // Blocs et instructions
    // ------------------------------------------------------------------

    fn check_block(&mut self, block: &Block, expected: Option<&Type>) -> Type {
        self.push_scope();
        let mut last_type: Option<Type> = None;
        for stmt in &block.stmts {
            last_type = Some(self.check_stmt(stmt));
        }
        let ty = match &block.tail {
            Some(tail) => self.check_expr(tail, expected),
            None => match &last_type {
                Some(t) if matches!(t, Type::Never) => Type::Never,
                _ => Type::Unit,
            },
        };
        self.pop_scope();
        self.facts.expr_types.insert(block.id, ty.clone());
        ty
    }

    fn check_stmt(&mut self, stmt: &Stmt) -> Type {
        match &stmt.kind {
            StmtKind::Let(lets) => {
                let declared = lets.ty.as_ref().map(|t| self.resolve_type(t));
                // G-5.11 / TYP-1.6: `!` is not a storable local type. A diverging
                // initializer still coerces to its expected type; the annotation
                // itself would make the generated Rust bind a never-typed place,
                // which the reference backend cannot represent.
                if let (Some(declared_ty), Some(annotation)) = (&declared, lets.ty.as_ref()) {
                    if crate::types::contains_never(declared_ty) {
                        let message = format!(
                            "the never type (`!`) is not a valid local type annotation in Core 1.0 (found `{declared_ty}`)"
                        );
                        let span = annotation.span;
                        self.error("E-TYP-MISMATCH", span, message);
                    }
                }
                // A loop is a statement, never a value (G-6.19, F-SYN-019/033).
                // Without this check `let x = while c { 1 };` type-checks as `()`
                // and silently binds a unit, which the specification forbids.
                if let Some(init) = lets.init.as_ref() {
                    if matches!(
                        init.kind,
                        ExprKind::While { .. } | ExprKind::Loop { .. } | ExprKind::For { .. }
                    ) {
                        self.error(
                            "E-TYP-STATEMENT-VALUE",
                            init.span,
                            "a `while`/`loop`/`for` expression is a statement and cannot be \
                             bound by `let`"
                                .to_string(),
                        );
                    }
                }
                let init_ty = lets
                    .init
                    .as_ref()
                    .map(|e| self.check_expr(e, declared.as_ref()));
                let final_ty = match (&declared, &init_ty) {
                    (Some(d), Some(i)) => {
                        self.unify(
                            i,
                            d,
                            lets.init.as_ref().map(|e| e.span).unwrap_or(stmt.span),
                            "let",
                        );
                        d.clone()
                    }
                    (Some(d), None) => d.clone(),
                    (None, Some(i)) => i.clone(),
                    (None, None) => {
                        self.error(
                            "E-TYP-AMBIGUOUS",
                            stmt.span,
                            "`let` requires a type or an initializer".to_string(),
                        );
                        Type::Error
                    }
                };
                self.check_let_pattern(&lets.pat, &final_ty, lets.init.is_none());
                if let PatKind::Binding { name, .. } = &lets.pat.kind {
                    self.unused_candidates
                        .push((name.name.clone(), lets.pat.span));
                }
                Type::Unit
            }
            StmtKind::Expr { expr, semi } => {
                let ty = self.check_expr(expr, None);
                if *semi && !matches!(ty, Type::Unit | Type::Never | Type::Error) {
                    self.error(
                        "E-TYP-STATEMENT-VALUE",
                        expr.span,
                        format!("expression of type `{ty}` used as a statement; expected `()`"),
                    );
                }
                ty
            }
        }
    }

    fn check_let_pattern(&mut self, pat: &Pat, ty: &Type, deferred: bool) {
        match &pat.kind {
            PatKind::Binding { mutable, name } => {
                if let Some(existing) = self.scopes.last().and_then(|s| s.get(&name.name)) {
                    // L'ombrement est autorisé : on écrase la liaison.
                    let _ = existing;
                }
                self.bind_local(&name.name, ty.clone(), *mutable, deferred);
                self.facts.pat_types.insert(pat.id, ty.clone());
            }
            _ => {
                self.error(
                    "E-PARSE-EXPECTED",
                    pat.span,
                    "`let` binds exactly one identifier in Core 1.0".to_string(),
                );
                self.facts.pat_types.insert(pat.id, ty.clone());
            }
        }
    }

    // ------------------------------------------------------------------
    // Expressions
    // ------------------------------------------------------------------

    fn check_expr(&mut self, expr: &Expr, expected: Option<&Type>) -> Type {
        // EXPR-5.11 / EXPR-6.7 / G-6.19: control expressions have type `()` and
        // are not value expressions. They are allowed as direct statements and
        // as the tail of a `()` block, not as arguments, fields, assignment
        // values, match arms/scrutinees or repeated array values.
        if self.value_position > 0
            && matches!(
                expr.kind,
                ExprKind::While { .. } | ExprKind::Loop { .. } | ExprKind::For { .. }
            )
        {
            self.error(
                "E-TYP-STATEMENT-VALUE",
                expr.span,
                "a `while`/`loop`/`for` expression is a statement and cannot be used as a value"
                    .to_string(),
            );
        }
        let ty = self.check_expr_inner(expr, expected);
        // TYP-1.6: `!` is not inhabitable, so it must never become a type
        // argument (`Option<!>`, `Result<!, E>`, `W<!>`). The bottom type only
        // coerces towards an expected type; when an aggregate constructor or
        // literal *infers* its argument from a diverging expression, the type is
        // genuinely ambiguous. Emitting E-TYP-AMBIGUOUS here avoids generating
        // Rust that needs the unstable `never_type` feature.
        if matches!(
            ty,
            Type::Option(_) | Type::Result(_, _) | Type::Nominal { .. }
        ) && contains_never(&ty)
        {
            self.error(
                "E-TYP-AMBIGUOUS",
                expr.span,
                "cannot infer an aggregate type from a diverging expression; \
                 annotate it or use a non-diverging value"
                    .to_string(),
            );
        }
        // TYP-13.2 / TYP-13.4 : les variables d'inférence sont une
        // représentation interne. Une fois qu'une contrainte les a liées
        // (`v.push(1)` lie l'élément de `Vec::new()`), tout type remis à un
        // appelant doit refléter la liaison, sinon une comparaison brute
        // (`s += v[i]`, `y = v[i]`, ...) voit encore `$0` et émet un faux
        // `E-TYP-MISMATCH`. `resolve_vars` ne lie rien : il substitue seulement
        // les variables déjà liées, donc aucune inférence n'est ajoutée.
        let ty = self.resolve_vars(&ty);
        self.record_expr_type(expr.id, &ty);
        ty
    }

    fn check_expr_inner(&mut self, expr: &Expr, expected: Option<&Type>) -> Type {
        match &expr.kind {
            ExprKind::Unit => Type::Unit,
            ExprKind::Int { value, suffix } => {
                self.check_int_literal(expr, *value, *suffix, expected)
            }
            ExprKind::Float { text, suffix } => {
                self.check_float_literal(expr, text, *suffix, expected)
            }
            ExprKind::Bool(_) => Type::Bool,
            ExprKind::Char(_) => Type::Char,
            ExprKind::Str(_) => Type::str_ref(),
            ExprKind::SelfValue => self.check_self_value(expr),
            ExprKind::Path(path) => self.check_path_expr(expr, path, expected),
            ExprKind::Call { callee, args } => self.check_call(expr, callee, args, expected),
            ExprKind::MethodCall {
                receiver,
                method,
                args,
            } => self.check_method_call(expr, receiver, &method.name, &method.span, args),
            ExprKind::Field { base, name } => self.check_field(base, &name.name, name.span),
            ExprKind::TupleIndex { base, index } => self.check_tuple_index(base, *index, expr.span),
            ExprKind::Index { base, index } => self.check_index(expr, base, index),
            ExprKind::Unary { op, expr: operand } => self.check_unary(expr, *op, operand, expected),
            ExprKind::Binary { op, lhs, rhs } => self.check_binary(expr, *op, lhs, rhs, expected),
            ExprKind::Assign { target, value } => self.check_assign(target, value),
            ExprKind::AssignOp { op, target, value } => {
                self.check_assign_op(expr, *op, target, value)
            }
            ExprKind::Cast { expr: value, ty } => self.check_cast(value, ty),
            ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => self.check_if(cond, then_block, else_branch.as_deref(), expected),
            ExprKind::Match { scrutinee, arms } => self.check_match(scrutinee, arms, expected),
            ExprKind::While { cond, body } => self.check_while(cond, body),
            ExprKind::Loop { body } => self.check_loop(body),
            ExprKind::For { pat, iter, body } => self.check_for(pat, iter, body),
            ExprKind::Block(block) => self.check_block(block, expected),
            ExprKind::Unsafe(block) => {
                self.unsafe_depth += 1;
                let ty = self.check_block(block, expected);
                self.unsafe_depth -= 1;
                ty
            }
            ExprKind::Tuple(items) => {
                // Tuples are heterogeneous by design (TYP-1.2): each element is
                // checked against the corresponding expected element, never
                // unified with its siblings. Reusing the array checker here
                // wrongly rejected `(i32, bool)`.
                let types = self.check_tuple_items(items, expected);
                Type::Tuple(types)
            }
            ExprKind::Array(items) => self.check_array(items, expected),
            ExprKind::ArrayRepeat { value, len } => {
                self.check_array_repeat(expr, value, len, expected)
            }
            ExprKind::StructLit { path, fields } => {
                self.check_struct_lit(expr, path, fields, expected)
            }
            ExprKind::Range {
                start,
                end,
                inclusive,
            } => self.check_range(expr, start.as_deref(), end.as_deref(), *inclusive),
            ExprKind::Try(inner) => self.check_try(expr, inner),
            ExprKind::Return(value) => self.check_return(value.as_deref()),
            ExprKind::Break => {
                if self.loop_depth == 0 {
                    self.error(
                        "E-TYP-BREAK-OUTSIDE",
                        expr.span,
                        "`break` outside of a loop".to_string(),
                    );
                }
                Type::Never
            }
            ExprKind::Continue => {
                if self.loop_depth == 0 {
                    self.error(
                        "E-TYP-BREAK-OUTSIDE",
                        expr.span,
                        "`continue` outside of a loop".to_string(),
                    );
                }
                Type::Never
            }
        }
    }

    fn check_tuple_items(&mut self, items: &[Expr], expected: Option<&Type>) -> Vec<Type> {
        // Only a same-arity tuple expectation can guide individual elements; a
        // different arity is reported by the enclosing unification, not here.
        let expected_items = match expected {
            Some(Type::Tuple(expected_items)) if expected_items.len() == items.len() => {
                Some(expected_items.clone())
            }
            _ => None,
        };
        let mut types = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            // EXPR-5.11/6.7: `while`/`loop`/`for` are not value expressions.
            if matches!(
                item.kind,
                ExprKind::While { .. } | ExprKind::Loop { .. } | ExprKind::For { .. }
            ) {
                self.error(
                    "E-TYP-STATEMENT-VALUE",
                    item.span,
                    "a `while`/`loop`/`for` expression is a statement and cannot be a tuple element"
                        .to_string(),
                );
            }
            let expected_item = expected_items.as_ref().and_then(|list| list.get(index));
            let found = self.check_expr(item, expected_item);
            let found = match expected_item {
                // TYP-1.6: an annotated tuple supplies the element type, so a
                // `!` element coerces instead of producing `(!, T)`.
                Some(expected) => self.coerce(found, expected),
                None => found,
            };
            // TYP-1.6: without an expected element type, a diverging element
            // would make the tuple `(!, …)`, which is uninhabitable and not
            // representable by the reference backend. An explicit annotation
            // (or a non-diverging value) is required.
            if expected_item.is_none() && contains_never(&found) {
                self.error(
                    "E-TYP-AMBIGUOUS",
                    item.span,
                    "cannot infer a tuple element type from a diverging expression; \
                     annotate the tuple or use a non-diverging value"
                        .to_string(),
                );
            }
            types.push(found);
        }
        types
    }

    fn check_expr_list(&mut self, items: &[Expr], _expected: Option<&Type>) -> Vec<Type> {
        let mut types = Vec::with_capacity(items.len());
        let mut first: Option<Type> = None;
        for item in items {
            let ty = self.check_expr(item, first.as_ref());
            if first.is_none() {
                first = Some(ty.clone());
            } else if let Some(f) = &first {
                if !Self::compatible(&ty, f) {
                    self.error(
                        "E-TYP-MISMATCH",
                        item.span,
                        format!("array/tuple element type `{ty}` does not match `{f}`"),
                    );
                }
            }
            types.push(ty);
        }
        types
    }

    // ------------------------------------------------------------------
    // Littéraux
    // ------------------------------------------------------------------

    fn check_int_literal(
        &mut self,
        expr: &Expr,
        value: u128,
        suffix: Option<IntTy>,
        expected: Option<&Type>,
    ) -> Type {
        let ty = match suffix {
            Some(suffix) => int_type(suffix),
            None => match expected {
                Some(t) if t.is_int() => t.clone(),
                _ => Type::I32,
            },
        };
        if !int_value_fits(value, &ty) {
            self.error(
                "E-TYP-LITERAL-RANGE",
                expr.span,
                format!("integer literal `{value}` does not fit in `{ty}`"),
            );
        }
        ty
    }

    fn check_float_literal(
        &mut self,
        expr: &Expr,
        text: &str,
        suffix: Option<ast::FloatTy>,
        expected: Option<&Type>,
    ) -> Type {
        let ty = match suffix {
            Some(ast::FloatTy::F32) => Type::F32,
            Some(ast::FloatTy::F64) => Type::F64,
            None => match expected {
                Some(Type::F32) => Type::F32,
                _ => Type::F64,
            },
        };
        let finite = match ty {
            Type::F32 => text.parse::<f32>().map(|v| v.is_finite()).unwrap_or(false),
            _ => text.parse::<f64>().map(|v| v.is_finite()).unwrap_or(false),
        };
        if !finite {
            self.error(
                "E-TYP-FLOAT-LITERAL",
                expr.span,
                format!("floating-point literal `{text}` is not representable in `{ty}`"),
            );
        }
        ty
    }

    // ------------------------------------------------------------------
    // `self`
    // ------------------------------------------------------------------

    fn check_self_value(&mut self, expr: &Expr) -> Type {
        match self.lookup_local("self") {
            Some(local) => {
                self.facts.resolutions.insert(
                    expr.id,
                    Resolution::Local {
                        name: "self".to_string(),
                        mutable: local.mutable,
                    },
                );
                local.ty
            }
            None => {
                self.error(
                    "E-RES-SELF",
                    expr.span,
                    "`self` is only available in a method".to_string(),
                );
                Type::Error
            }
        }
    }

    // ------------------------------------------------------------------
    // Chemins
    // ------------------------------------------------------------------

    fn check_path_expr(&mut self, expr: &Expr, path: &Path, expected: Option<&Type>) -> Type {
        // Une liaison locale masque tout le reste. Les faits de résolution sont
        // indexés par l'identifiant du **nœud Path** (pas celui de l'expression).
        if path.root == PathRoot::Plain && path.segments.len() == 1 {
            let name = &path.segments[0].name.name;
            if let Some(local) = self.lookup_local(name) {
                self.used_locals.insert(name.clone());
                self.facts.resolutions.insert(
                    path.id,
                    Resolution::Local {
                        name: name.clone(),
                        mutable: local.mutable,
                    },
                );
                return local.ty;
            }
        }
        match resolve::resolve_value_path(
            self.world,
            self.module,
            &self.generics,
            self.self_ty.as_ref(),
            path,
        ) {
            Ok(target) => {
                self.record_value_resolution(path.id, &target);
                self.type_of_value_path(expr, &target, expected)
            }
            Err(error) => {
                self.emit_path_error(expr.span, &error);
                self.facts.resolutions.insert(path.id, Resolution::Error);
                Type::Error
            }
        }
    }

    fn emit_path_error(&mut self, span: Span, error: &PathError) {
        self.error(error.code, span, error.message.clone());
    }

    fn record_value_resolution(&mut self, id: NodeId, target: &ValuePath) {
        let resolution = match target {
            ValuePath::Item(item) => Resolution::Item { item: *item },
            ValuePath::Variant { enum_item, variant } => Resolution::Variant {
                enum_item: *enum_item,
                variant: *variant,
            },
            ValuePath::BuiltinFn(path) => Resolution::BuiltinFn {
                path: (*path).to_string(),
            },
            ValuePath::BuiltinConst(path) => Resolution::BuiltinConst {
                path: (*path).to_string(),
            },
            ValuePath::BuiltinCtor { path, unit } => {
                if *unit {
                    Resolution::BuiltinConst {
                        path: (*path).to_string(),
                    }
                } else {
                    Resolution::BuiltinFn {
                        path: (*path).to_string(),
                    }
                }
            }
            ValuePath::AssocFn(item) => Resolution::Item { item: *item },
            ValuePath::Module(_) => Resolution::Module,
            ValuePath::BuiltinModule(_) => Resolution::BuiltinModule,
            ValuePath::Type(_) => Resolution::Error,
        };
        self.facts.resolutions.insert(id, resolution);
    }

    fn type_of_value_path(
        &mut self,
        expr: &Expr,
        target: &ValuePath,
        expected: Option<&Type>,
    ) -> Type {
        match target {
            ValuePath::Item(item) => {
                if let Some(info) = self.facts.consts.get(item) {
                    return info.ty.clone();
                }
                if self.world.consts.contains_key(item) {
                    return Type::Error;
                }
                self.error(
                    "E-TYP-CALL",
                    expr.span,
                    "a function item cannot be used as a value in Core 1.0".to_string(),
                );
                Type::Error
            }
            ValuePath::BuiltinFn(path) => {
                self.error(
                    "E-TYP-CALL",
                    expr.span,
                    format!("builtin function `{path}` must be called"),
                );
                Type::Error
            }
            ValuePath::BuiltinCtor { path, unit } => {
                if *unit {
                    return self.builtin_unit_variant_type(expr, path, expected);
                }
                self.error(
                    "E-TYP-CALL",
                    expr.span,
                    format!("variant constructor `{path}` requires arguments"),
                );
                Type::Error
            }
            ValuePath::BuiltinConst(path) => self.builtin_unit_variant_type(expr, path, expected),
            ValuePath::Variant { enum_item, variant } => {
                let info = match self.facts.enums.get(enum_item) {
                    Some(info) => info.clone(),
                    None => return Type::Error,
                };
                let variant_info = match info.variants.get(*variant) {
                    Some(v) => v.clone(),
                    None => return Type::Error,
                };
                if variant_info.kind != crate::facts::VariantKind::Unit {
                    self.error(
                        "E-TYP-CALL",
                        expr.span,
                        format!(
                            "variant `{}::{}` requires arguments",
                            info.name, variant_info.name
                        ),
                    );
                    return Type::Error;
                }
                let args =
                    self.builtin_enum_args(expected, *enum_item, info.generics.len(), expr.span);
                Type::Nominal {
                    def: *enum_item,
                    name: info.name.clone(),
                    args,
                }
            }
            ValuePath::AssocFn(item) => {
                let _ = item;
                self.error(
                    "E-TYP-CALL",
                    expr.span,
                    "an associated function must be called".to_string(),
                );
                Type::Error
            }
            ValuePath::Module(_) | ValuePath::BuiltinModule(_) => {
                self.error(
                    "E-TYP-CALL",
                    expr.span,
                    "a module cannot be used as a value".to_string(),
                );
                Type::Error
            }
            ValuePath::Type(t) => {
                self.error(
                    "E-TYP-CALL",
                    expr.span,
                    format!("type `{t}` cannot be used as a value"),
                );
                Type::Error
            }
        }
    }

    fn builtin_enum_args(
        &mut self,
        expected: Option<&Type>,
        def: NodeId,
        arity: usize,
        span: Span,
    ) -> Vec<Type> {
        if let Some(Type::Nominal { def: d, args, .. }) = expected {
            if *d == def && args.len() == arity {
                return args.clone();
            }
        }
        if arity == 0 {
            return Vec::new();
        }
        let mut args = Vec::with_capacity(arity);
        for _ in 0..arity {
            args.push(self.fresh_var(span));
        }
        args
    }

    fn builtin_unit_variant_type(
        &mut self,
        expr: &Expr,
        path: &str,
        expected: Option<&Type>,
    ) -> Type {
        match path {
            "Option::None" => match expected {
                Some(Type::Option(inner)) => Type::option((**inner).clone()),
                _ => Type::option(self.fresh_var(expr.span)),
            },
            _ => {
                self.error(
                    "E-RES-UNKNOWN",
                    expr.span,
                    format!("unknown builtin constant `{path}`"),
                );
                Type::Error
            }
        }
    }

    // ------------------------------------------------------------------
    // Appels
    // ------------------------------------------------------------------

    fn check_call(
        &mut self,
        expr: &Expr,
        callee: &Expr,
        args: &[Expr],
        expected: Option<&Type>,
    ) -> Type {
        let path = match &callee.kind {
            ExprKind::Path(path) => path,
            _ => {
                let callee_ty = self.check_expr(callee, None);
                if !matches!(callee_ty, Type::Never) {
                    self.error(
                        "E-TYP-CALL",
                        callee.span,
                        format!("value of type `{callee_ty}` is not callable"),
                    );
                }
                for arg in args {
                    self.check_expr(arg, None);
                }
                self.facts.calls.insert(expr.id, CallTarget::Error);
                return Type::Error;
            }
        };

        // Liaison locale utilisée comme fonction.
        if path.root == PathRoot::Plain && path.segments.len() == 1 {
            let name = &path.segments[0].name.name;
            if self.lookup_local(name).is_some() {
                let local = self.lookup_local(name).unwrap();
                self.error(
                    "E-TYP-CALL",
                    callee.span,
                    format!("value `{name}` of type `{}` is not callable", local.ty),
                );
                for arg in args {
                    self.check_expr(arg, None);
                }
                self.facts.resolutions.insert(path.id, Resolution::Error);
                self.facts.expr_types.insert(callee.id, local.ty);
                self.facts.calls.insert(expr.id, CallTarget::Error);
                return Type::Error;
            }
        }

        let target = match resolve::resolve_value_path(
            self.world,
            self.module,
            &self.generics,
            self.self_ty.as_ref(),
            path,
        ) {
            Ok(target) => target,
            Err(error) => {
                self.emit_path_error(callee.span, &error);
                for arg in args {
                    self.check_expr(arg, None);
                }
                self.facts.resolutions.insert(path.id, Resolution::Error);
                self.facts.expr_types.insert(callee.id, Type::Error);
                self.facts.calls.insert(expr.id, CallTarget::Error);
                return Type::Error;
            }
        };
        self.record_value_resolution(path.id, &target);

        let result = match &target {
            ValuePath::Item(id) => {
                let is_extern = self
                    .facts
                    .fn_sigs
                    .get(id)
                    .map(|signature| signature.is_extern)
                    .unwrap_or(false);
                if self.world.fns.contains_key(id) || is_extern {
                    self.check_user_fn_call(*id, args, expected, expr.span, None)
                } else {
                    self.error(
                        "E-TYP-CALL",
                        callee.span,
                        "a constant is not callable".to_string(),
                    );
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    Type::Error
                }
            }
            ValuePath::AssocFn(id) => {
                self.check_user_fn_call(*id, args, expected, expr.span, Some(*id))
            }
            ValuePath::BuiltinFn(path) => {
                if let Some(assoc) = builtins::builtin_assoc_fn(path) {
                    self.check_builtin_assoc_call(assoc, args, expected, expr.span)
                } else if let Some(sig) = builtins::builtin_fn(path) {
                    self.check_builtin_fn_call(sig, args, expected, expr.span)
                } else {
                    self.error(
                        "E-RES-UNKNOWN",
                        callee.span,
                        format!("unknown builtin function `{path}`"),
                    );
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    Type::Error
                }
            }
            ValuePath::Variant { enum_item, variant } => {
                self.check_variant_call(*enum_item, *variant, args, expected, expr.span)
            }
            ValuePath::BuiltinCtor { path, .. } => {
                self.check_builtin_ctor_call(path, args, expected, expr.span)
            }
            ValuePath::BuiltinConst(path) => {
                self.error(
                    "E-TYP-ARITY",
                    expr.span,
                    format!("`{path}` is a value, not a function"),
                );
                for arg in args {
                    self.check_expr(arg, None);
                }
                Type::Error
            }
            ValuePath::Module(_) | ValuePath::BuiltinModule(_) => {
                self.error(
                    "E-TYP-CALL",
                    callee.span,
                    "a module is not callable".to_string(),
                );
                for arg in args {
                    self.check_expr(arg, None);
                }
                Type::Error
            }
            ValuePath::Type(t) => {
                self.error(
                    "E-TYP-CALL",
                    callee.span,
                    format!("type `{t}` is not callable"),
                );
                for arg in args {
                    self.check_expr(arg, None);
                }
                Type::Error
            }
        };

        // Le nœud `callee` est un nœud d'expression : il reçoit le type du
        // résultat de l'appel (le codegen ne s'en sert pas, mais aucun nœud
        // d'un programme accepté ne doit manquer).
        self.facts.expr_types.insert(callee.id, result.clone());
        self.facts.calls.insert(
            expr.id,
            match &target {
                ValuePath::Item(id) => CallTarget::UserFn { item: *id },
                ValuePath::AssocFn(id) => CallTarget::UserFn { item: *id },
                ValuePath::Variant { enum_item, variant } => CallTarget::VariantCtor {
                    enum_item: *enum_item,
                    variant: *variant,
                },
                ValuePath::BuiltinFn(path) => CallTarget::BuiltinFn {
                    path: (*path).to_string(),
                },
                ValuePath::BuiltinCtor { path, .. } => CallTarget::BuiltinFn {
                    path: (*path).to_string(),
                },
                _ => CallTarget::Error,
            },
        );
        result
    }

    fn check_user_fn_call(
        &mut self,
        item: NodeId,
        args: &[Expr],
        expected: Option<&Type>,
        span: Span,
        _assoc: Option<NodeId>,
    ) -> Type {
        let Some(sig) = self.facts.fn_sigs.get(&item).cloned() else {
            for arg in args {
                self.check_expr(arg, None);
            }
            return Type::Error;
        };
        if sig.is_extern && self.unsafe_depth == 0 {
            self.error(
                "E-UNSAFE-CONTEXT",
                span,
                format!(
                    "extern function `{}` can only be called inside an `unsafe {{ ... }}` block",
                    sig.name
                ),
            );
        }
        // Correspondance impl génériques <-> type de retour attendu.
        let mut subst: BTreeMap<usize, Type> = BTreeMap::new();
        if let Some(entry) = self.method_entry_by_item(item) {
            if !self.method_visible(&entry) {
                self.error(
                    "E-RES-PRIVATE",
                    span,
                    format!("function `{}` is private", entry.name),
                );
            }
            if let Some(Type::Nominal { def, args, .. }) = &expected {
                if *def == nominal_def(&entry.self_ty).unwrap_or(*def) {
                    self.bind_impl_generics(&entry, args, &mut subst);
                }
            }
        }
        if args.len() != sig.params.len() {
            self.error(
                "E-TYP-ARITY",
                span,
                format!(
                    "function `{}` expects {} argument(s), found {}",
                    sig.name,
                    sig.params.len(),
                    args.len()
                ),
            );
        }
        for (index, arg) in args.iter().enumerate() {
            if let Some(param) = sig.params.get(index) {
                let param_ty = substitute(&param.ty, &subst);
                let arg_ty = self.check_arg(arg, &param_ty);
                if !self.unify_pattern(&param.ty, &arg_ty, &mut subst) {
                    self.error(
                        "E-TYP-MISMATCH",
                        arg.span,
                        format!(
                            "argument {} of `{}`: expected `{}`, found `{}`",
                            index + 1,
                            sig.name,
                            substitute(&param.ty, &subst),
                            arg_ty
                        ),
                    );
                }
            } else {
                self.check_expr(arg, None);
            }
        }
        let ret = substitute(&sig.ret, &subst);
        let ret = self.instantiate(&ret, span);
        if let Some(expected) = expected {
            self.unify_pattern(&ret, expected, &mut subst);
        }
        self.resolve_vars(&ret)
    }

    fn method_visible(&self, entry: &resolve::MethodEntry) -> bool {
        if entry.vis.is_public() {
            return true;
        }
        let mut current = Some(self.module);
        while let Some(m) = current {
            if m == entry.def_module {
                return true;
            }
            current = self.world.modules[m].parent;
        }
        false
    }

    fn method_entry_by_item(&self, item: NodeId) -> Option<resolve::MethodEntry> {
        for methods in self.world.impls.values() {
            if let Some(entry) = methods.iter().find(|m| m.item == item) {
                return Some(entry.clone());
            }
        }
        None
    }

    fn bind_impl_generics(
        &self,
        entry: &resolve::MethodEntry,
        actual_args: &[Type],
        subst: &mut BTreeMap<usize, Type>,
    ) {
        let Type::Nominal { args, .. } = &entry.self_ty else {
            return;
        };
        for (index, pattern) in args.iter().enumerate() {
            if let (Type::Param { index: param, .. }, Some(actual)) =
                (pattern, actual_args.get(index))
            {
                subst.insert(*param, actual.clone());
            }
        }
    }

    fn check_arg(&mut self, arg: &Expr, expected: &Type) -> Type {
        let expected = self.resolve_vars(expected);
        let needs_late_coercion = matches!(
            &expected,
            Type::Ref { inner, .. } if matches!(**inner, Type::Str | Type::Slice(_))
        );
        let found = if needs_late_coercion {
            self.check_value_expr(arg, None)
        } else {
            self.check_value_expr(arg, Some(&expected))
        };
        self.coerce(found, &expected)
    }

    /// Les deux seules coercitions implicites admises, en argument et en
    /// récepteur : `&String → &str` (y compris `String → &str`) et
    /// `&[T; N] → &[T]`.
    fn coerce(&self, found: Type, expected: &Type) -> Type {
        if matches!(found, Type::Never) || found == *expected {
            return expected.clone();
        }
        match (expected, &found) {
            (
                Type::Ref {
                    mutable: false,
                    inner: exp_inner,
                },
                found_ty,
            ) => match (&**exp_inner, found_ty) {
                // KNOWN DIVERGENCE (D-COERCE-1, `T2-FINDINGS.md` §19.2): TYP-13.1
                // admits only `&String -> &str`, but this arm also accepts an
                // owned `String` by value, which keeps `line.trim()` directly
                // usable as a text argument (`examples/stats`,
                // `conformance/cases/run/stats`). Removing it is a language
                // decision, not a bug fix: it breaks a documented reference app
                // and the corpus case that `features.md` F-STD-017 cites.
                (Type::Str, Type::String) => expected.clone(),
                (
                    Type::Str,
                    Type::Ref {
                        mutable: false,
                        inner,
                    },
                ) if **inner == Type::String => expected.clone(),
                (Type::Str, Type::Str) => expected.clone(),
                (
                    Type::Slice(elem),
                    Type::Ref {
                        mutable: false,
                        inner,
                    },
                ) => match &**inner {
                    Type::Array { elem: felem, .. } if elem == felem => expected.clone(),
                    Type::Slice(felem) if elem == felem => expected.clone(),
                    _ => found,
                },
                _ => found,
            },
            _ => found,
        }
    }

    fn check_builtin_fn_call(
        &mut self,
        sig: builtins::BuiltinFnSig,
        args: &[Expr],
        expected: Option<&Type>,
        span: Span,
    ) -> Type {
        if sig.path == "std::panic::assert_eq" {
            return self.check_assert_eq(args, sig.path);
        }
        let params: Vec<Type> = builtins::fn_param_types(&sig)
            .iter()
            .map(|p| self.instantiate(p, span))
            .collect();
        if args.len() != params.len() {
            self.error(
                "E-TYP-ARITY",
                span,
                format!(
                    "`{}` expects {} argument(s), found {}",
                    sig.path,
                    params.len(),
                    args.len()
                ),
            );
        }
        let mut subst: BTreeMap<usize, Type> = BTreeMap::new();
        for (index, arg) in args.iter().enumerate() {
            if let Some(param) = params.get(index) {
                let expected_param = self.resolve_vars(&substitute(param, &subst));
                let arg_ty = self.check_arg(arg, &expected_param);
                if !self.unify_pattern(param, &arg_ty, &mut subst) {
                    self.error(
                        "E-TYP-MISMATCH",
                        arg.span,
                        format!(
                            "argument {} of `{}`: expected `{}`, found `{}`",
                            index + 1,
                            sig.path,
                            expected_param,
                            arg_ty
                        ),
                    );
                }
            } else {
                self.check_expr(arg, None);
            }
        }
        let ret = self.resolve_vars(&substitute(&builtins::fn_ret_type(&sig), &subst));
        if let Some(expected) = expected {
            self.unify_pattern(&ret, expected, &mut subst);
        }
        self.resolve_vars(&ret)
    }

    /// True when the expression can be borrowed mutably: a `let mut` binding, a
    /// field or index of a mutable place, or a dereference of `&mut T`.
    fn is_mutable_place(&self, expr: &Expr) -> bool {
        // A value of type `&mut T` can always be reborrowed mutably, whatever
        // the mutability of the binding that holds it.
        if matches!(
            self.facts.expr_types.get(&expr.id),
            Some(Type::Ref { mutable: true, .. })
        ) {
            return true;
        }
        match &expr.kind {
            ExprKind::Path(path) => matches!(
                self.facts.resolutions.get(&path.id),
                Some(crate::facts::Resolution::Local { mutable: true, .. })
            ),
            ExprKind::Field { base, .. }
            | ExprKind::TupleIndex { base, .. }
            | ExprKind::Index { base, .. } => self.is_mutable_place(base),
            ExprKind::Unary { op, expr } => {
                matches!(op, UnOp::Deref)
                    && matches!(
                        self.facts.expr_types.get(&expr.id),
                        Some(Type::Ref { mutable: true, .. })
                    )
            }
            _ => false,
        }
    }

    fn check_assert_eq(&mut self, args: &[Expr], _path: &'static str) -> Type {
        if args.len() != 2 {
            self.error(
                "E-TYP-ARITY",
                args.first().map(|a| a.span).unwrap_or(Span::dummy()),
                "`assert_eq` expects 2 arguments".to_string(),
            );
        }
        let (a_ty, b_ty) = if let (Some(first), Some(second)) = (args.first(), args.get(1)) {
            if is_untyped_num_literal(first) {
                let b = self.check_expr(second, None);
                let a = self.check_expr(first, Some(&b));
                (a, b)
            } else {
                let a = self.check_expr(first, None);
                let b = self.check_expr(second, Some(&a));
                (a, b)
            }
        } else {
            for arg in args {
                self.check_expr(arg, None);
            }
            (Type::Error, Type::Error)
        };
        if !(Self::compatible(&a_ty, &b_ty) || (is_text_like(&a_ty) && is_text_like(&b_ty))) {
            self.error(
                "E-TYP-MISMATCH",
                args.get(1).map(|a| a.span).unwrap_or(Span::dummy()),
                format!("`assert_eq` arguments have different types `{a_ty}` and `{b_ty}`"),
            );
        } else {
            // `assert_eq` compares with `==` (STD-8.2), so a type without `==`
            // cannot be asserted: `Vec`, `Option`, `Result`, structs and enums
            // have no equality in Core 1.0.
            let comparable = (is_text_like(&a_ty) && is_text_like(&b_ty))
                || (a_ty == b_ty
                    && (a_ty.is_int()
                        || a_ty.is_float()
                        || a_ty == Type::Bool
                        || a_ty == Type::Char));
            if !comparable && !matches!(a_ty, Type::Error) {
                self.error(
                    "E-TYP-EQ",
                    args.first().map(|a| a.span).unwrap_or(Span::dummy()),
                    format!("`assert_eq` requires a type with `==`; `{a_ty}` has none in Core 1.0"),
                );
            }
        }
        Type::Unit
    }

    fn check_builtin_assoc_call(
        &mut self,
        sig: builtins::BuiltinAssocFn,
        args: &[Expr],
        expected: Option<&Type>,
        span: Span,
    ) -> Type {
        let params: Vec<Type> = builtins::assoc_param_types(&sig)
            .iter()
            .map(|p| self.instantiate(p, span))
            .collect();
        if args.len() != params.len() {
            self.error(
                "E-TYP-ARITY",
                span,
                format!(
                    "`{}` expects {} argument(s), found {}",
                    sig.path,
                    params.len(),
                    args.len()
                ),
            );
        }
        let mut subst: BTreeMap<usize, Type> = BTreeMap::new();
        // Le type de retour attendu fixe les paramètres génériques (`Vec::new`).
        let ret_pattern = self.instantiate(&builtins::assoc_ret_type(&sig), span);
        if let Some(exp) = expected {
            self.unify_pattern(&ret_pattern, exp, &mut subst);
        }
        for (index, arg) in args.iter().enumerate() {
            if let Some(param) = params.get(index) {
                let expected_param = self.resolve_vars(&substitute(param, &subst));
                let arg_ty = self.check_arg(arg, &expected_param);
                if !self.unify_pattern(param, &arg_ty, &mut subst) {
                    self.error(
                        "E-TYP-MISMATCH",
                        arg.span,
                        format!(
                            "argument {} of `{}`: expected `{}`, found `{}`",
                            index + 1,
                            sig.path,
                            expected_param,
                            arg_ty
                        ),
                    );
                }
            } else {
                self.check_expr(arg, None);
            }
        }
        self.resolve_vars(&ret_pattern)
    }

    fn check_variant_call(
        &mut self,
        enum_item: NodeId,
        variant: usize,
        args: &[Expr],
        expected: Option<&Type>,
        span: Span,
    ) -> Type {
        let info = match self.facts.enums.get(&enum_item) {
            Some(info) => info.clone(),
            None => {
                for arg in args {
                    self.check_expr(arg, None);
                }
                return Type::Error;
            }
        };
        let variant_info = match info.variants.get(variant) {
            Some(v) => v.clone(),
            None => return Type::Error,
        };
        let mut subst: BTreeMap<usize, Type> = BTreeMap::new();
        if let Some(Type::Nominal {
            def,
            args: exp_args,
            ..
        }) = expected
        {
            if *def == enum_item && exp_args.len() == info.generics.len() {
                for (i, arg) in exp_args.iter().enumerate() {
                    subst.insert(i, arg.clone());
                }
            }
        }
        match variant_info.kind {
            crate::facts::VariantKind::Tuple => {
                if args.len() != variant_info.tuple_types.len() {
                    self.error(
                        "E-TYP-ARITY",
                        span,
                        format!(
                            "variant `{}::{}` expects {} argument(s), found {}",
                            info.name,
                            variant_info.name,
                            variant_info.tuple_types.len(),
                            args.len()
                        ),
                    );
                }
                for (index, arg) in args.iter().enumerate() {
                    if let Some(pattern) = variant_info.tuple_types.get(index) {
                        let expected_ty = substitute(pattern, &subst);
                        let arg_ty = self.check_arg(arg, &expected_ty);
                        // TYP-1.6: `!` must not determine a generic variant
                        // argument. `check_arg`/`coerce` returns the expected
                        // type for a `!` argument, so the raw expression type is
                        // read from the facts; an unsubstituted type parameter
                        // means there is nothing to infer from.
                        let raw_arg_ty = self
                            .facts
                            .expr_types
                            .get(&arg.id)
                            .cloned()
                            .unwrap_or_else(|| arg_ty.clone());
                        if contains_never(&raw_arg_ty) && has_unbound_param(&expected_ty) {
                            self.error(
                                "E-TYP-AMBIGUOUS",
                                arg.span,
                                "cannot infer a generic variant argument from a diverging \
                                 expression; annotate the enum type or use a non-diverging value"
                                    .to_string(),
                            );
                        }
                        if !self.unify_pattern(pattern, &arg_ty, &mut subst) {
                            self.error(
                                "E-TYP-MISMATCH",
                                arg.span,
                                format!(
                                    "argument {} of `{}::{}`: expected `{}`, found `{}`",
                                    index + 1,
                                    info.name,
                                    variant_info.name,
                                    expected_ty,
                                    arg_ty
                                ),
                            );
                        }
                    } else {
                        self.check_expr(arg, None);
                    }
                }
            }
            crate::facts::VariantKind::Unit => {
                if !args.is_empty() {
                    self.error(
                        "E-TYP-ARITY",
                        span,
                        format!(
                            "variant `{}::{}` takes no arguments",
                            info.name, variant_info.name
                        ),
                    );
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                }
            }
            crate::facts::VariantKind::Struct => {
                self.error(
                    "E-TYP-CALL",
                    span,
                    format!(
                        "variant `{}::{}` is a struct variant; use `{{ field: value }}`",
                        info.name, variant_info.name
                    ),
                );
                for arg in args {
                    self.check_expr(arg, None);
                }
            }
        }
        let mut final_args = Vec::with_capacity(info.generics.len());
        for index in 0..info.generics.len() {
            match subst.get(&index) {
                Some(t) => final_args.push(self.resolve_vars(t)),
                None => final_args.push(self.fresh_var(span)),
            }
        }
        let result = Type::Nominal {
            def: enum_item,
            name: info.name.clone(),
            args: final_args,
        };
        if let Some(expected) = expected {
            self.unify_pattern(&result, expected, &mut subst);
        }
        self.resolve_vars(&result)
    }

    fn check_builtin_ctor_call(
        &mut self,
        path: &'static str,
        args: &[Expr],
        expected: Option<&Type>,
        span: Span,
    ) -> Type {
        match path {
            "Option::Some" => {
                if args.len() != 1 {
                    self.error(
                        "E-TYP-ARITY",
                        span,
                        "`Option::Some` expects exactly 1 argument".to_string(),
                    );
                }
                let inner_expected = match expected {
                    Some(Type::Option(inner)) => Some((**inner).clone()),
                    _ => None,
                };
                let inner = match args.first() {
                    Some(arg) => match &inner_expected {
                        Some(expected) => self.check_arg(arg, expected),
                        None => self.check_value_expr(arg, None),
                    },
                    None => inner_expected.unwrap_or(Type::Error),
                };
                let result = Type::option(inner);
                if let Some(expected) = expected {
                    if !self.unify_pattern(&result, expected, &mut BTreeMap::new()) {
                        self.error(
                            "E-TYP-MISMATCH",
                            span,
                            format!("`Option::Some` produces `{result}`, expected `{expected}`"),
                        );
                    }
                }
                self.resolve_vars(&result)
            }
            "Result::Ok" | "Result::Err" => {
                if args.len() != 1 {
                    self.error(
                        "E-TYP-ARITY",
                        span,
                        format!("`{path}` expects exactly 1 argument"),
                    );
                }
                let (ok_expected, err_expected) = match expected {
                    Some(Type::Result(ok, err)) => (Some((**ok).clone()), Some((**err).clone())),
                    _ => (None, None),
                };
                let value = match args.first() {
                    Some(arg) => {
                        let expectation = if path == "Result::Ok" {
                            ok_expected.clone()
                        } else {
                            err_expected.clone()
                        };
                        match expectation {
                            Some(expected) => self.check_arg(arg, &expected),
                            None => self.check_value_expr(arg, None),
                        }
                    }
                    None => Type::Error,
                };
                let (ok, err) = if path == "Result::Ok" {
                    (value, err_expected.unwrap_or_else(|| self.fresh_var(span)))
                } else {
                    (ok_expected.unwrap_or_else(|| self.fresh_var(span)), value)
                };
                let result = Type::Result(Box::new(ok), Box::new(err));
                if let Some(expected) = expected {
                    if !self.unify_pattern(&result, expected, &mut BTreeMap::new()) {
                        self.error(
                            "E-TYP-MISMATCH",
                            span,
                            format!("`{path}` produces `{result}`, expected `{expected}`"),
                        );
                    }
                }
                self.resolve_vars(&result)
            }
            _ => {
                self.error(
                    "E-RES-UNKNOWN",
                    span,
                    format!("unknown builtin constructor `{path}`"),
                );
                for arg in args {
                    self.check_expr(arg, None);
                }
                Type::Error
            }
        }
    }

    // ------------------------------------------------------------------
    // Méthodes
    // ------------------------------------------------------------------

    fn check_method_call(
        &mut self,
        expr: &Expr,
        receiver: &Expr,
        name: &str,
        name_span: &Span,
        args: &[Expr],
    ) -> Type {
        let recv_ty = self.check_expr(receiver, None);
        let recv_ty = self.resolve_vars(&recv_ty);
        let base = deref_type(&recv_ty);
        // `Vec<T>::clone` exists only for `Copy` elements (STD-10.4, F-STD-024).
        // For a mobile element the method does not exist at all, so the
        // diagnostic is a resolution failure, not a type mismatch.
        if name == "clone" {
            if let Type::Vec(inner) = &base {
                if !crate::own::is_copy_type(inner, self.facts) {
                    self.error(
                        "E-RES-UNKNOWN",
                        *name_span,
                        format!(
                            "`clone` on `Vec<{inner}>` requires a `Copy` element type; \
                             `{inner}` is mobile"
                        ),
                    );
                }
            }
        }
        match &base {
            Type::Nominal { def, .. } => {
                let entry = self.world.method(*def, name, false).cloned();
                match entry {
                    Some(entry) => {
                        let target = self.check_user_method_call(&entry, &base, args, name_span);
                        let ty = target.1;
                        self.facts
                            .methods
                            .insert(expr.id, MethodTarget::User { item: entry.item });
                        ty
                    }
                    None => {
                        self.error(
                            "E-RES-UNKNOWN",
                            *name_span,
                            format!("no method `{name}` on type `{base}`"),
                        );
                        for arg in args {
                            self.check_expr(arg, None);
                        }
                        self.facts.methods.insert(expr.id, MethodTarget::Error);
                        Type::Error
                    }
                }
            }
            Type::String
            | Type::Vec(_)
            | Type::Option(_)
            | Type::Result(_, _)
            | Type::Slice(_)
            | Type::Str
            | Type::F64
            | Type::F32
            | Type::Bool
            | Type::Char
            | Type::I8
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
            | Type::Usize => {
                let kind = method_self_kind(&base);
                match kind.and_then(|k| builtins::builtin_method(k, name)) {
                    Some(sig) => {
                        if builtins::builtin_method_takes_exclusive_receiver(kind.unwrap(), name)
                            && !self.is_mutable_place(receiver)
                        {
                            self.error(
                                "E-OWN-IMMUTABLE",
                                *name_span,
                                format!(
                                    "`{name}` takes `&mut self`; the receiver must be a mutable \
                                     place (`let mut …` or a `&mut` borrow)"
                                ),
                            );
                        }
                        let env = method_env_for(&base);
                        let params = builtins::method_param_types(sig, &env);
                        if args.len() != params.len() {
                            self.error(
                                "E-TYP-ARITY",
                                *name_span,
                                format!(
                                    "method `{name}` expects {} argument(s), found {}",
                                    params.len(),
                                    args.len()
                                ),
                            );
                        }
                        for (index, arg) in args.iter().enumerate() {
                            match params.get(index) {
                                Some(param) => {
                                    let arg_ty = self.check_arg(arg, param);
                                    if !self.unify_pattern(param, &arg_ty, &mut BTreeMap::new()) {
                                        self.error(
                                            "E-TYP-MISMATCH",
                                            arg.span,
                                            format!(
                                                "argument {} of `{name}`: expected `{param}`, found `{arg_ty}`",
                                                index + 1
                                            ),
                                        );
                                    }
                                }
                                None => {
                                    self.check_expr(arg, None);
                                }
                            }
                        }
                        let ret = self.resolve_vars(&builtins::method_ret_type(sig, &env));
                        self.facts.methods.insert(
                            expr.id,
                            MethodTarget::Builtin {
                                path: sig.path.to_string(),
                                ret: ret.clone(),
                            },
                        );
                        ret
                    }
                    None => {
                        self.error(
                            "E-RES-UNKNOWN",
                            *name_span,
                            format!("no builtin method `{name}` on type `{base}`"),
                        );
                        for arg in args {
                            self.check_expr(arg, None);
                        }
                        self.facts.methods.insert(expr.id, MethodTarget::Error);
                        Type::Error
                    }
                }
            }
            Type::Param { name: param, .. } => {
                self.error(
                    "E-RES-UNKNOWN",
                    *name_span,
                    format!("no method `{name}` can be resolved on generic parameter `{param}`"),
                );
                for arg in args {
                    self.check_expr(arg, None);
                }
                self.facts.methods.insert(expr.id, MethodTarget::Error);
                Type::Error
            }
            _ => {
                self.error(
                    "E-RES-UNKNOWN",
                    *name_span,
                    format!("no method `{name}` on type `{base}`"),
                );
                for arg in args {
                    self.check_expr(arg, None);
                }
                self.facts.methods.insert(expr.id, MethodTarget::Error);
                Type::Error
            }
        }
    }

    fn check_user_method_call(
        &mut self,
        entry: &resolve::MethodEntry,
        recv: &Type,
        args: &[Expr],
        name_span: &Span,
    ) -> (NodeId, Type) {
        if !self.method_visible(entry) {
            self.error(
                "E-RES-PRIVATE",
                *name_span,
                format!("method `{}` is private", entry.name),
            );
        }
        let Some(sig) = self.facts.fn_sigs.get(&entry.item).cloned() else {
            for arg in args {
                self.check_expr(arg, None);
            }
            return (entry.item, Type::Error);
        };
        let mut subst: BTreeMap<usize, Type> = BTreeMap::new();
        if let Type::Nominal { args: actual, .. } = recv {
            self.bind_impl_generics(entry, actual, &mut subst);
        }
        let params: Vec<&crate::facts::ParamSig> =
            sig.params.iter().filter(|p| !p.is_receiver).collect();
        if args.len() != params.len() {
            self.error(
                "E-TYP-ARITY",
                *name_span,
                format!(
                    "method `{}` expects {} argument(s), found {}",
                    entry.name,
                    params.len(),
                    args.len()
                ),
            );
        }
        for (index, arg) in args.iter().enumerate() {
            match params.get(index) {
                Some(param) => {
                    let param_ty = substitute(&param.ty, &subst);
                    let arg_ty = self.check_arg(arg, &param_ty);
                    if !self.unify_pattern(&param.ty, &arg_ty, &mut subst) {
                        self.error(
                            "E-TYP-MISMATCH",
                            arg.span,
                            format!(
                                "argument {} of `{}`: expected `{}`, found `{}`",
                                index + 1,
                                entry.name,
                                param_ty,
                                arg_ty
                            ),
                        );
                    }
                }
                None => {
                    self.check_expr(arg, None);
                }
            }
        }
        let ret = self.resolve_vars(&substitute(&sig.ret, &subst));
        (entry.item, ret)
    }

    // ------------------------------------------------------------------
    // Accès champs / index
    // ------------------------------------------------------------------

    fn check_field(&mut self, base: &Expr, name: &str, span: Span) -> Type {
        let base_ty = self.check_expr(base, None);
        let ty = deref_type(&base_ty);
        match ty {
            Type::Nominal { def, ref args, .. } => {
                let Some(field) = self
                    .facts
                    .structs
                    .get(&def)
                    .and_then(|info| info.fields.iter().find(|f| f.name == name))
                    .cloned()
                else {
                    self.error(
                        "E-TYP-FIELD",
                        span,
                        format!("no field `{name}` on type `{ty}`"),
                    );
                    return Type::Error;
                };
                self.check_field_visibility(&def, &field, span);
                let subst = nominal_subst(self.world, &def, &args);
                substitute(&field.ty, &subst)
            }
            Type::Tuple(_) => {
                self.error(
                    "E-TYP-FIELD",
                    span,
                    format!("no field `{name}` on tuple; use `.0`, `.1`, ..."),
                );
                Type::Error
            }
            _ => {
                // A method name used without a call is not a field and not a
                // value: `s.parse_i64` is a resolution failure (STD-9.10,
                // F-STD-021), which is a different diagnostic from "unknown
                // field" and points the user at the fix.
                let is_method = method_self_kind(&ty)
                    .and_then(|kind| builtins::builtin_method(kind, name))
                    .is_some()
                    || matches!(
                        &ty,
                        Type::Nominal { def, .. } if self.world.method(*def, name, false).is_some()
                    );
                if is_method {
                    self.error(
                        "E-RES-UNKNOWN",
                        span,
                        format!("`{name}` is a method of `{ty}`, not a value; call it with `(…)`"),
                    );
                } else {
                    self.error(
                        "E-TYP-FIELD",
                        span,
                        format!("no field `{name}` on type `{ty}`"),
                    );
                }
                Type::Error
            }
        }
    }

    fn check_field_visibility(
        &mut self,
        def: &NodeId,
        field: &crate::facts::FieldInfo,
        span: Span,
    ) {
        if field.public {
            return;
        }
        let def_module = self.world.item_module.get(def).copied().unwrap_or(0);
        let mut visible = false;
        let mut current = Some(self.module);
        while let Some(m) = current {
            if m == def_module {
                visible = true;
                break;
            }
            current = self.world.modules[m].parent;
        }
        if !visible {
            self.error(
                "E-RES-PRIVATE",
                span,
                format!("field `{}` is private", field.name),
            );
        }
    }

    fn check_tuple_index(&mut self, base: &Expr, index: u32, span: Span) -> Type {
        let base_ty = self.check_expr(base, None);
        match deref_type(&base_ty) {
            Type::Tuple(items) => match items.get(index as usize) {
                Some(ty) => ty.clone(),
                None => {
                    self.error(
                        "E-TYP-FIELD",
                        span,
                        format!(
                            "tuple index `{index}` is out of bounds (arity {})",
                            items.len()
                        ),
                    );
                    Type::Error
                }
            },
            other => {
                self.error(
                    "E-TYP-FIELD",
                    span,
                    format!("`.{index}` requires a tuple, found `{other}`"),
                );
                Type::Error
            }
        }
    }

    fn check_index(&mut self, expr: &Expr, base: &Expr, index: &Expr) -> Type {
        let base_ty = self.check_expr(base, None);
        let index_ty = self.check_expr(index, Some(&Type::Usize));
        if !matches!(index_ty, Type::Usize | Type::Error) {
            self.error_note(
                "E-TYP-INDEX",
                index.span,
                format!("index must be `usize`, found `{index_ty}`"),
                "convert explicitly with `as usize`",
                None,
            );
        }
        match deref_type(&base_ty) {
            Type::Array { elem, .. } => (*elem).clone(),
            Type::Vec(elem) => (*elem).clone(),
            Type::Slice(elem) => (*elem).clone(),
            Type::String => {
                self.error(
                    "E-TYP-INDEX",
                    expr.span,
                    "`String` is not indexable in Core 1.0".to_string(),
                );
                Type::Error
            }
            other => {
                self.error(
                    "E-TYP-INDEX",
                    expr.span,
                    format!("type `{other}` is not indexable"),
                );
                Type::Error
            }
        }
    }

    // ------------------------------------------------------------------
    // Opérateurs
    // ------------------------------------------------------------------

    fn check_unary(
        &mut self,
        expr: &Expr,
        op: UnOp,
        operand: &Expr,
        expected: Option<&Type>,
    ) -> Type {
        match op {
            UnOp::Neg => {
                if let ExprKind::Int { value, suffix } = &operand.kind {
                    let ty = match suffix {
                        Some(suffix) => int_type(*suffix),
                        None => match expected {
                            Some(t) if t.is_int() => t.clone(),
                            _ => Type::I32,
                        },
                    };
                    if !ty.is_signed_int() {
                        self.error(
                            "E-TYP-MISMATCH",
                            expr.span,
                            format!("cannot negate a value of type `{ty}`"),
                        );
                        self.facts.expr_types.insert(operand.id, ty.clone());
                        return ty;
                    }
                    let (min_magnitude, _, _) = int_bounds(&ty);
                    if *value > min_magnitude {
                        self.error(
                            "E-TYP-LITERAL-RANGE",
                            operand.span,
                            format!("integer literal `-{value}` does not fit in `{ty}`"),
                        );
                    }
                    self.facts.expr_types.insert(operand.id, ty.clone());
                    return ty;
                }
                let inner_ty = self.check_expr(operand, expected.filter(|t| t.is_numeric()));
                match &inner_ty {
                    t if t.is_signed_int() || t.is_float() => inner_ty.clone(),
                    Type::Never | Type::Error => inner_ty.clone(),
                    t => {
                        self.error(
                            "E-TYP-MISMATCH",
                            expr.span,
                            format!("cannot negate a value of type `{t}`"),
                        );
                        t.clone()
                    }
                }
            }
            UnOp::Not => {
                let inner_ty = self.check_expr(
                    operand,
                    expected.filter(|t| **t == Type::Bool || t.is_int()),
                );
                match &inner_ty {
                    Type::Bool => Type::Bool,
                    t if t.is_int() => t.clone(),
                    Type::Never | Type::Error => inner_ty.clone(),
                    t => {
                        self.error(
                            "E-TYP-MISMATCH",
                            expr.span,
                            format!("`!` requires `bool` or an integer, found `{t}`"),
                        );
                        Type::Error
                    }
                }
            }
            UnOp::Ref | UnOp::RefMut => {
                let inner_ty = self.check_expr(operand, None);
                Type::Ref {
                    mutable: op == UnOp::RefMut,
                    inner: Box::new(inner_ty),
                }
            }
            UnOp::Deref => {
                let inner_ty = self.check_expr(operand, None);
                match inner_ty {
                    Type::Ref { inner, .. } => (*inner).clone(),
                    Type::Never | Type::Error => inner_ty,
                    other => {
                        self.error(
                            "E-TYP-MISMATCH",
                            expr.span,
                            format!("cannot dereference a value of type `{other}`"),
                        );
                        Type::Error
                    }
                }
            }
        }
    }

    fn check_binary(
        &mut self,
        expr: &Expr,
        op: ast::BinOp,
        lhs: &Expr,
        rhs: &Expr,
        expected: Option<&Type>,
    ) -> Type {
        use ast::BinOp as B;
        let lhs_lit = is_untyped_num_literal(lhs);
        let rhs_lit = is_untyped_num_literal(rhs);
        let is_shift = matches!(op, B::Shl | B::Shr);

        let (l_ty, r_ty) = if is_shift {
            let r = self.check_expr(rhs, None);
            let l = if lhs_lit {
                self.check_expr(lhs, Some(&r))
            } else {
                self.check_expr(lhs, expected)
            };
            (l, r)
        } else if lhs_lit && !rhs_lit {
            let r = self.check_expr(rhs, expected.filter(|t| t.is_numeric()));
            let l = self.check_expr(lhs, Some(&r));
            (l, r)
        } else {
            let l = self.check_expr(lhs, expected.filter(|t| t.is_numeric()));
            let r = self.check_expr(rhs, Some(&l));
            (l, r)
        };

        match op {
            B::Add | B::Sub | B::Mul | B::Div | B::Rem => {
                if (l_ty.is_int() && r_ty.is_int()) || (l_ty.is_float() && r_ty.is_float()) {
                    if l_ty == r_ty {
                        l_ty
                    } else {
                        self.error(
                            "E-TYP-MISMATCH",
                            expr.span,
                            format!(
                                "operands of `{}` must have the same type: `{l_ty}` vs `{r_ty}`",
                                op.as_str()
                            ),
                        );
                        Type::Error
                    }
                } else {
                    self.error(
                        "E-TYP-MISMATCH",
                        expr.span,
                        format!(
                            "operator `{}` is not defined for `{l_ty}` and `{r_ty}`",
                            op.as_str()
                        ),
                    );
                    Type::Error
                }
            }
            B::BitAnd | B::BitOr | B::BitXor => {
                let ok = (l_ty.is_int() && r_ty.is_int() && l_ty == r_ty)
                    || (l_ty == Type::Bool && r_ty == Type::Bool);
                if ok {
                    l_ty
                } else {
                    self.error(
                        "E-TYP-MISMATCH",
                        expr.span,
                        format!(
                            "operator `{}` requires two integers of the same type or two `bool`",
                            op.as_str()
                        ),
                    );
                    Type::Error
                }
            }
            B::Shl | B::Shr => {
                if l_ty.is_int() && r_ty.is_int() {
                    if is_negative_int_literal(rhs) {
                        self.error(
                            "E-TYP-SHIFT",
                            rhs.span,
                            "shift amount must not be negative".to_string(),
                        );
                    }
                    l_ty
                } else {
                    self.error(
                        "E-TYP-MISMATCH",
                        expr.span,
                        format!(
                            "operator `{}` requires integer operands, found `{l_ty}` and `{r_ty}`",
                            op.as_str()
                        ),
                    );
                    Type::Error
                }
            }
            B::Eq | B::Ne => {
                // TYP-7.3 : `==`/`!=` sont définis sur tous les entiers,
                // `f32`/`f64`, `bool`, `char`, et sur le texte (`String`, `&str`
                // et tout mélange des deux). Les agrégats restent non
                // comparables (`E-TYP-EQ`).
                let same_scalar = l_ty == r_ty
                    && (l_ty.is_int()
                        || l_ty.is_float()
                        || l_ty == Type::Bool
                        || l_ty == Type::Char);
                if (is_text_like(&l_ty) && is_text_like(&r_ty)) || same_scalar {
                    Type::Bool
                } else if l_ty.is_numeric() && r_ty.is_numeric() {
                    // EXPR-4.8 / TYP-13.2 : deux types numériques différents ne
                    // se mélangent jamais implicitement (`f32` vs `f64`,
                    // `i32` vs `i64`, ...) : c'est un désaccord d'opérandes
                    // (`E-TYP-MISMATCH`), pas un type non comparable.
                    self.error(
                        "E-TYP-MISMATCH",
                        expr.span,
                        format!(
                            "operands of `{}` must have the same type: `{l_ty}` vs `{r_ty}`",
                            op.as_str()
                        ),
                    );
                    Type::Bool
                } else {
                    self.error(
                        "E-TYP-EQ",
                        expr.span,
                        format!(
                            "`{}` is not defined for `{l_ty}` and `{r_ty}` in Core 1.0",
                            op.as_str()
                        ),
                    );
                    Type::Bool
                }
            }
            B::Lt | B::Le | B::Gt | B::Ge => {
                // TYP-7.4 : `< <= > >=` ne sont définis que sur les entiers,
                // `f32`/`f64` et `char` (jamais sur le texte).
                let same_scalar =
                    l_ty == r_ty && (l_ty.is_int() || l_ty.is_float() || l_ty == Type::Char);
                if same_scalar {
                    Type::Bool
                } else if l_ty.is_numeric() && r_ty.is_numeric() {
                    // EXPR-4.8 / TYP-13.2 : même règle que pour `==`/`!=`.
                    self.error(
                        "E-TYP-MISMATCH",
                        expr.span,
                        format!(
                            "operands of `{}` must have the same type: `{l_ty}` vs `{r_ty}`",
                            op.as_str()
                        ),
                    );
                    Type::Bool
                } else {
                    self.error(
                        "E-TYP-CMP",
                        expr.span,
                        format!(
                            "`{}` is not defined for `{l_ty}` and `{r_ty}` in Core 1.0",
                            op.as_str()
                        ),
                    );
                    Type::Bool
                }
            }
            B::And | B::Or => {
                if l_ty == Type::Bool && r_ty == Type::Bool {
                    Type::Bool
                } else {
                    self.error(
                        "E-TYP-MISMATCH",
                        expr.span,
                        format!(
                            "`{}` requires `bool` operands, found `{l_ty}` and `{r_ty}`",
                            op.as_str()
                        ),
                    );
                    Type::Bool
                }
            }
        }
    }

    fn check_assign(&mut self, target: &Expr, value: &Expr) -> Type {
        let target_ty = self.check_expr(target, None);
        let value_ty = self.check_value_expr(value, Some(&target_ty));
        if !Self::compatible(&value_ty, &target_ty) {
            self.error(
                "E-TYP-MISMATCH",
                value.span,
                format!("cannot assign `{value_ty}` to a place of type `{target_ty}`"),
            );
        }
        if !matches!(target_ty, Type::Error) && !self.place_is_mutable(target) {
            self.error(
                "E-OWN-IMMUTABLE",
                target.span,
                "cannot assign to an immutable place".to_string(),
            );
        }
        Type::Unit
    }

    /// `Some(false)` = place immuable ; `Some(true)` = place mutable ; `None` =
    /// l'expression n'est pas une place (déjà rejetée par le parser).
    fn place_is_mutable(&self, expr: &Expr) -> bool {
        match self.place_mutability(expr) {
            Some(mutable) => mutable,
            None => true,
        }
    }

    fn place_mutability(&self, expr: &Expr) -> Option<bool> {
        match &expr.kind {
            ExprKind::Path(path) => {
                if path.root == PathRoot::Plain && path.segments.len() == 1 {
                    let name = &path.segments[0].name.name;
                    if let Some(local) = self.lookup_local(name) {
                        return Some(local.mutable || local.deferred);
                    }
                }
                Some(false)
            }
            ExprKind::SelfValue => self.lookup_local("self").map(|l| l.mutable),
            ExprKind::Field { base, .. }
            | ExprKind::TupleIndex { base, .. }
            | ExprKind::Index { base, .. } => {
                if self.place_mutability(base) == Some(true) {
                    return Some(true);
                }
                Some(self.base_is_mut_ref(base))
            }
            ExprKind::Unary {
                op: UnOp::Deref,
                expr: inner,
            } => {
                let ty = self
                    .facts
                    .expr_types
                    .get(&inner.id)
                    .cloned()
                    .or_else(|| self.facts.type_nodes.get(&inner.id).cloned());
                match ty {
                    Some(Type::Ref { mutable, .. }) => Some(mutable),
                    Some(Type::Never) | Some(Type::Error) => None,
                    _ => Some(false),
                }
            }
            _ => None,
        }
    }

    fn base_is_mut_ref(&self, base: &Expr) -> bool {
        matches!(
            self.facts.expr_types.get(&base.id),
            Some(Type::Ref { mutable: true, .. })
        )
    }

    fn check_assign_op(
        &mut self,
        expr: &Expr,
        op: ast::BinOp,
        target: &Expr,
        value: &Expr,
    ) -> Type {
        // G-6.8 / EXPR-9.4 (MUST) : toute la famille des affectations
        // composées est **acceptée** (`+= -= *= /= %= &= |= ^= <<= >>=`), avec
        // la sémantique `place = place op e`. `place` n'est évaluée qu'une
        // fois : c'est l'affectation composée du Rust émis (`place op= e`) qui
        // le garantit ; le vérificateur de types ne contrôle donc que les
        // types des opérandes et la mutabilité de la place.
        use ast::BinOp as B;
        let sym = format!("{}=", op.as_str());
        let target_ty = self.check_expr(target, None);
        let value_ty = self.check_value_expr(value, Some(&target_ty));
        match op {
            B::Add | B::Sub | B::Mul | B::Div | B::Rem => {
                // TYP-13.2 : aucune conversion implicite, pas même entre deux
                // entiers (`i32` vs `i64`) ni entre `f32` et `f64` (EXPR-4.8).
                let same_numeric = (target_ty.is_int() && value_ty.is_int())
                    || (target_ty.is_float() && value_ty.is_float());
                if !same_numeric {
                    self.error(
                        "E-TYP-MISMATCH",
                        expr.span,
                        format!(
                            "operator `{sym}` is not defined for `{target_ty}` and `{value_ty}`"
                        ),
                    );
                } else if target_ty != value_ty {
                    self.error(
                        "E-TYP-MISMATCH",
                        expr.span,
                        format!(
                            "operands of `{sym}` must have the same type: `{target_ty}` vs `{value_ty}`"
                        ),
                    );
                }
            }
            B::BitAnd | B::BitOr | B::BitXor => {
                let ok = (target_ty.is_int() && value_ty.is_int() && target_ty == value_ty)
                    || (target_ty == Type::Bool && value_ty == Type::Bool);
                if !ok {
                    self.error(
                        "E-TYP-MISMATCH",
                        expr.span,
                        format!(
                            "operator `{sym}` requires two integers of the same type or two `bool`"
                        ),
                    );
                }
            }
            B::Shl | B::Shr => {
                // EXPR-3.4 : comme pour `<<`/`>>`, les deux opérandes doivent
                // être entiers mais leurs types peuvent différer (le résultat a
                // le type de la gauche) ; un décalage littéral négatif est
                // refusé statiquement.
                if target_ty.is_int() && value_ty.is_int() {
                    if is_negative_int_literal(value) {
                        self.error(
                            "E-TYP-SHIFT",
                            value.span,
                            "shift amount must not be negative".to_string(),
                        );
                    }
                } else {
                    self.error(
                        "E-TYP-MISMATCH",
                        expr.span,
                        format!(
                            "operator `{sym}` requires integer operands, found `{target_ty}` and `{value_ty}`"
                        ),
                    );
                }
            }
            // Le parseur ne construit un `AssignOp` qu'avec l'un des dix
            // opérateurs ci-dessus (G-6.8) ; ce bras n'est donc pas atteignable,
            // mais un refus typé vaut mieux qu'un silence.
            B::And | B::Or | B::Eq | B::Ne | B::Lt | B::Le | B::Gt | B::Ge => {
                self.error(
                    "E-TYP-MISMATCH",
                    expr.span,
                    format!("`{sym}` is not a compound assignment operator"),
                );
            }
        }
        if !matches!(target_ty, Type::Error) && !self.place_is_mutable(target) {
            self.error(
                "E-OWN-IMMUTABLE",
                target.span,
                "cannot assign to an immutable place".to_string(),
            );
        }
        Type::Unit
    }

    fn check_cast(&mut self, value: &Expr, ty: &AstType) -> Type {
        let target = self.resolve_type(ty);
        // TYP-1.6 / G-5.11: `!` is not constructible, so it cannot be a cast
        // target (`x as !` would generate invalid Rust). Checked before the
        // `Never` source shortcut, which otherwise returns the target as-is.
        if crate::types::contains_never(&target) {
            self.check_expr(value, None);
            self.error(
                "E-TYP-CAST",
                ty.span,
                "the never type (`!`) is not a valid cast target in Core 1.0".to_string(),
            );
            return Type::Error;
        }
        let source = self.check_expr(value, None);
        if matches!(source, Type::Never) || matches!(target, Type::Error) {
            return target;
        }
        if !cast_allowed(&source, &target) {
            self.error(
                "E-TYP-CAST",
                value.span,
                format!("cast from `{source}` to `{target}` is not allowed in Core 1.0"),
            );
            return Type::Error;
        }
        target
    }

    // ------------------------------------------------------------------
    // Contrôle
    // ------------------------------------------------------------------

    fn check_if(
        &mut self,
        cond: &Expr,
        then_block: &Block,
        else_branch: Option<&Expr>,
        expected: Option<&Type>,
    ) -> Type {
        let cond_ty = self.check_expr(cond, Some(&Type::Bool));
        if !matches!(cond_ty, Type::Bool | Type::Never | Type::Error) {
            self.error(
                "E-TYP-MISMATCH",
                cond.span,
                format!("`if` condition must be `bool`, found `{cond_ty}`"),
            );
        }
        let then_ty = self.check_block(then_block, expected);
        match else_branch {
            None => {
                if !matches!(then_ty, Type::Unit | Type::Never | Type::Error) {
                    self.error(
                        "E-TYP-IF-NO-ELSE",
                        then_block.span,
                        format!(
                            "`if` without `else` has type `()` but its block returns `{then_ty}`"
                        ),
                    );
                }
                Type::Unit
            }
            Some(else_expr) => {
                let else_ty = match expected {
                    Some(exp) => self.check_expr(else_expr, Some(exp)),
                    None => {
                        if matches!(then_ty, Type::Never) {
                            self.check_expr(else_expr, None)
                        } else {
                            self.check_expr(else_expr, Some(&then_ty))
                        }
                    }
                };
                if !Self::compatible(&then_ty, &else_ty) {
                    self.error(
                        "E-TYP-MISMATCH",
                        else_expr.span,
                        format!("`if` branches have different types: `{then_ty}` and `{else_ty}`"),
                    );
                }
                if matches!(then_ty, Type::Never) {
                    else_ty
                } else {
                    then_ty
                }
            }
        }
    }

    fn check_while(&mut self, cond: &Expr, body: &Block) -> Type {
        let cond_ty = self.check_expr(cond, Some(&Type::Bool));
        if !matches!(cond_ty, Type::Bool | Type::Never | Type::Error) {
            self.error(
                "E-TYP-MISMATCH",
                cond.span,
                format!("`while` condition must be `bool`, found `{cond_ty}`"),
            );
        }
        self.loop_depth += 1;
        self.check_block(body, None);
        self.loop_depth -= 1;
        Type::Unit
    }

    fn check_loop(&mut self, body: &Block) -> Type {
        // EXPR-5.11 / EXPR-6.7 / G-6.19 (MUST): `loop` has type `()`, even when
        // it has no reachable `break` and diverges semantically (EXPR-6.2).
        // Divergence is not used to widen the expression type.
        self.loop_depth += 1;
        self.check_block(body, None);
        self.loop_depth -= 1;
        Type::Unit
    }

    fn check_for(&mut self, pat: &Pat, iter: &Expr, body: &Block) -> Type {
        let item_ty = if let ExprKind::Range {
            start,
            end,
            inclusive,
        } = &iter.kind
        {
            let start = start.as_deref();
            let end = end.as_deref();
            self.check_for_range(iter, start, end, *inclusive)
        } else {
            let iter_ty = self.check_expr(iter, None);
            match &iter_ty {
                Type::Vec(elem) => (**elem).clone(),
                // EXPR-6.4 / TYP-5.8 (MUST): the collection form consumes an
                // **owned** `Vec<T>`; `&Vec<T>` and `&[T]` are E-TYP-MISMATCH and
                // are iterated by index (`for i in 0..s.len()`). The checker used
                // to accept both and yield `&T` items, which the ownership pass
                // then rejected with E-OWN-REF-ESCAPE: the wrong phase and the
                // wrong code for a rule the type checker owns.
                // TYP-1.6: `for` cannot consume a `!` value; before this
                // check the program was accepted and rustc rejected `() is not
                // an iterator`.
                Type::Never => {
                    self.error(
                        "E-TYP-MISMATCH",
                        iter.span,
                        "`for` cannot iterate over a diverging expression of type `!`".to_string(),
                    );
                    Type::Error
                }
                Type::Error => Type::Error,
                other => {
                    self.error(
                        "E-TYP-MISMATCH",
                        iter.span,
                        format!(
                            "`for` iterates a range or an owned `Vec<T>`, found `{other}`; a `&Vec<T>` or `&[T]` is iterated by index (EXPR-6.4)"
                        ),
                    );
                    Type::Error
                }
            }
        };
        self.push_scope();
        self.check_pattern(pat, &item_ty);
        self.loop_depth += 1;
        self.check_block(body, None);
        self.loop_depth -= 1;
        self.pop_scope();
        Type::Unit
    }

    fn check_for_range(
        &mut self,
        iter: &Expr,
        start: Option<&Expr>,
        end: Option<&Expr>,
        inclusive: bool,
    ) -> Type {
        self.record_expr_type(iter.id, &Type::Unit);
        if inclusive {
            self.error(
                "E-TYP-RANGE",
                iter.span,
                "inclusive ranges (`..=`) are not supported in Core 1.0".to_string(),
            );
        }
        let (Some(start), Some(end)) = (start, end) else {
            self.error(
                "E-TYP-RANGE",
                iter.span,
                "`for` requires an explicit `start..end` range".to_string(),
            );
            return Type::Error;
        };
        let start_lit = is_untyped_num_literal(start);
        let (start_ty, end_ty) = if start_lit {
            let e = self.check_expr(end, None);
            let s = self.check_expr(start, Some(&e));
            (s, e)
        } else {
            let s = self.check_expr(start, None);
            let e = self.check_expr(end, Some(&s));
            (s, e)
        };
        if !(start_ty.is_int() && end_ty.is_int() && start_ty == end_ty) {
            // `diagnostics.md` §3 assigns "bornes de plage de types différents ou
            // non entières" to E-TYP-RANGE, not to the generic type mismatch.
            self.error(
                "E-TYP-RANGE",
                iter.span,
                format!(
                    "range bounds must be integers of the same type: `{start_ty}` vs `{end_ty}`"
                ),
            );
            return Type::Error;
        }
        start_ty
    }

    fn check_range(
        &mut self,
        expr: &Expr,
        start: Option<&Expr>,
        end: Option<&Expr>,
        inclusive: bool,
    ) -> Type {
        if let Some(start) = start {
            self.check_expr(start, None);
        }
        if let Some(end) = end {
            self.check_expr(end, None);
        }
        let _ = inclusive;
        let _ = expr;
        // The parser already rejects a range outside a `for` clause with
        // E-UNSUPPORTED-RANGE (parse phase); the checker must not add a second
        // code for the same construct.
        Type::Error
    }

    fn check_match(
        &mut self,
        scrutinee: &Expr,
        arms: &[MatchArm],
        expected: Option<&Type>,
    ) -> Type {
        let scrutinee_ty = self.check_value_expr(scrutinee, None);
        let mut result: Option<Type> = None;
        let mut coverage = Coverage::new(&scrutinee_ty, self.facts);
        for arm in arms {
            self.push_scope();
            self.check_pattern(&arm.pat, &scrutinee_ty);
            let body_expected: Option<&Type> = result.as_ref().or(expected);
            let body_ty = self.check_value_expr(&arm.body, body_expected);
            self.pop_scope();
            // EXPR-5.9 / G-6.44 (MUST): an unreachable arm is neither an error
            // nor removed, and produces **no diagnostic**. DIAG-1.6 keeps the
            // warning list closed to `W-RES-UNUSED-IMPORT` and
            // `W-OWN-UNUSED-VARIABLE`. An earlier `W-TYP-UNREACHABLE` emission
            // violated both rules and was removed; `registry_audit.py` now keeps
            // it from coming back.
            self.cover_pattern(&mut coverage, &arm.pat);
            result = Some(match result {
                None => body_ty,
                Some(acc) => {
                    if !Self::compatible(&acc, &body_ty) {
                        self.error(
                            "E-TYP-MISMATCH",
                            arm.body.span,
                            format!("match arms have different types: `{acc}` and `{body_ty}`"),
                        );
                        acc
                    } else if matches!(acc, Type::Never) {
                        body_ty
                    } else {
                        acc
                    }
                }
            });
        }
        if !coverage.is_exhaustive() {
            let missing = coverage.describe_missing();
            self.error_note(
                "E-TYP-NONEXHAUSTIVE",
                expr_span_match(scrutinee, arms),
                missing,
                "cover every variant or add a `_` arm",
                None,
            );
        }
        let ty = result.unwrap_or(Type::Never);
        if let Some(exp) = expected {
            if !Self::compatible(&ty, exp) {
                self.error(
                    "E-TYP-MISMATCH",
                    scrutinee.span,
                    format!("`match` has type `{ty}`, expected `{exp}`"),
                );
            }
        }
        ty
    }

    fn check_try(&mut self, expr: &Expr, inner: &Expr) -> Type {
        let inner_ty = self.check_expr(inner, None);
        if self.in_const || self.current_fn.is_none() {
            self.error(
                "E-TYP-TRY",
                expr.span,
                "`?` is only valid inside a function returning `Result` or `Option`".to_string(),
            );
            return Type::Error;
        }
        match inner_ty {
            Type::Result(ok, err) => match &self.fn_ret {
                Type::Result(_, fn_err) if **fn_err == *err => (*ok).clone(),
                other => {
                    self.error(
                        "E-TYP-TRY",
                        expr.span,
                        format!(
                            "`?` on `Result<{ok}, {err}>` requires the function to return `Result<_, {err}>`, found `{other}`"
                        ),
                    );
                    Type::Error
                }
            },
            Type::Option(inner_ok) => match &self.fn_ret {
                Type::Option(_) => (*inner_ok).clone(),
                other => {
                    self.error(
                        "E-TYP-TRY",
                        expr.span,
                        format!(
                            "`?` on `Option<{inner_ok}>` requires the function to return `Option<_>`, found `{other}`"
                        ),
                    );
                    Type::Error
                }
            },
            Type::Never | Type::Error => inner_ty,
            other => {
                self.error(
                    "E-TYP-TRY",
                    expr.span,
                    format!("`?` is not defined on `{other}`"),
                );
                Type::Error
            }
        }
    }

    fn check_return(&mut self, value: Option<&Expr>) -> Type {
        if self.in_const || self.current_fn.is_none() {
            self.error(
                "E-TYP-RETURN",
                value.map(|v| v.span).unwrap_or(Span::dummy()),
                "`return` is not allowed here".to_string(),
            );
            if let Some(value) = value {
                self.check_expr(value, None);
            }
            return Type::Never;
        }
        let ret = self.fn_ret.clone();
        match value {
            None => {
                if ret != Type::Unit {
                    self.error(
                        "E-TYP-RETURN",
                        Span::dummy(),
                        format!("`return;` is invalid in a function returning `{ret}`"),
                    );
                }
            }
            Some(value) => {
                if ret == Type::Unit {
                    self.check_expr(value, None);
                    self.error(
                        "E-TYP-RETURN",
                        value.span,
                        "`return` with a value is invalid in a function returning `()`".to_string(),
                    );
                } else if matches!(ret, Type::Never) {
                    // Nothing inhabits `!`: `return 1;` cannot produce a value
                    // of the divergent return type. The symmetric `compatible`
                    // predicate would otherwise accept it (it treats `!` as
                    // bottom on either side).
                    let found = self.check_expr(value, Some(&ret));
                    let found = self.resolve_vars(&found);
                    if !matches!(found, Type::Never | Type::Error) {
                        self.error(
                            "E-TYP-RETURN",
                            value.span,
                            format!(
                                "`return` cannot produce a value in a function returning `!` \
                                 (found `{found}`)"
                            ),
                        );
                    }
                } else {
                    let found = self.check_expr(value, Some(&ret));
                    let found = self.resolve_vars(&found);
                    let ret = self.resolve_vars(&ret);
                    if !Self::compatible(&found, &ret)
                        && !self.unify(&found, &ret, value.span, "return")
                    {
                        self.error(
                            "E-TYP-RETURN",
                            value.span,
                            format!("`return` type `{found}` does not match `{ret}`"),
                        );
                    }
                }
            }
        }
        Type::Never
    }

    // ------------------------------------------------------------------
    // Agrégats
    // ------------------------------------------------------------------

    fn check_array(&mut self, items: &[Expr], expected: Option<&Type>) -> Type {
        if items.is_empty() {
            self.error(
                "E-TYP-AMBIGUOUS",
                Span::dummy(),
                "cannot infer the element type of an empty array".to_string(),
            );
            return Type::Error;
        }
        // TYP-1.6: an annotation supplies the expected element type, so a
        // diverging element coerces instead of forcing `[!; N]`.
        let expected_elem = match expected {
            Some(Type::Array { elem, len }) if *len == items.len() => Some((**elem).clone()),
            _ => None,
        };
        let mut types = Vec::with_capacity(items.len());
        for item in items {
            let found = self.check_expr(item, expected_elem.as_ref());
            types.push(match &expected_elem {
                Some(elem) => self.coerce(found, elem),
                None => found,
            });
        }
        // EXPR-5.11/6.7: control expressions are not values, so they cannot be
        // array elements either.
        for item in items {
            if matches!(
                item.kind,
                ExprKind::While { .. } | ExprKind::Loop { .. } | ExprKind::For { .. }
            ) {
                self.error(
                    "E-TYP-STATEMENT-VALUE",
                    item.span,
                    "a `while`/`loop`/`for` expression is a statement and cannot be an array element"
                        .to_string(),
                );
            }
        }
        let annotated_elem = expected_elem.is_some();
        let elem = expected_elem.unwrap_or_else(|| types[0].clone());
        // TYP-4: an array is homogeneous (`[T; N]`). Without an annotation the
        // first element fixes `T`, so every later element must have the same
        // type; otherwise `[1, true]` would be accepted and the generated Rust
        // (`[1, true]`) would not compile. With an annotation each element was
        // already checked against the expected element type above.
        if !annotated_elem {
            for (item, ty) in items.iter().zip(types.iter()).skip(1) {
                if !Self::compatible(ty, &elem) {
                    self.error(
                        "E-TYP-MISMATCH",
                        item.span,
                        format!(
                            "array elements must all have the same type: \
                             the first element has type `{elem}`, this one has type `{ty}`"
                        ),
                    );
                }
            }
        }
        // TYP-1.6: without an annotation, a leading `!` leaves the element type
        // uninferable; `[panic(), …]` would otherwise generate `[!; N]`.
        if contains_never(&elem) {
            self.error(
                "E-TYP-AMBIGUOUS",
                items[0].span,
                "cannot infer an array element type from a diverging expression; \
                 annotate the array or use a non-diverging value"
                    .to_string(),
            );
        }
        Type::Array {
            elem: Box::new(elem),
            len: items.len(),
        }
    }

    fn check_array_repeat(
        &mut self,
        expr: &Expr,
        value: &Expr,
        len: &Expr,
        expected: Option<&Type>,
    ) -> Type {
        let expected_elem = match expected {
            Some(Type::Array { elem, .. }) => Some((**elem).clone()),
            _ => None,
        };
        let found = self.check_value_expr(value, expected_elem.as_ref());
        let value_ty = match &expected_elem {
            Some(elem) => self.coerce(found, elem),
            None => found,
        };
        // TYP-1.6: an annotation supplies the element type; without one, a
        // diverging repeat value leaves `[!; N]`, which is uninferable.
        if expected_elem.is_none() && contains_never(&value_ty) {
            self.error(
                "E-TYP-AMBIGUOUS",
                value.span,
                "cannot infer an array element type from a diverging expression; \
                 annotate the array or use a non-diverging value"
                    .to_string(),
            );
        }
        let length = self.const_int_value(len);
        match length {
            Some(n) if n == 0 => {
                self.error(
                    "E-TYP-ARRAY-REPEAT",
                    len.span,
                    "array length must be greater than zero".to_string(),
                );
                Type::Array {
                    elem: Box::new(value_ty),
                    len: 1,
                }
            }
            Some(n) => Type::Array {
                elem: Box::new(value_ty),
                len: n as usize,
            },
            None => {
                self.error(
                    "E-TYP-ARRAY-REPEAT",
                    expr.span,
                    "array repetition length must be a constant integer".to_string(),
                );
                Type::Array {
                    elem: Box::new(value_ty),
                    len: 1,
                }
            }
        }
    }

    fn const_int_value(&mut self, expr: &Expr) -> Option<u128> {
        match &expr.kind {
            ExprKind::Int { value, .. } => {
                self.record_expr_type(expr.id, &Type::Usize);
                Some(*value)
            }
            ExprKind::Path(path) => {
                let target = resolve::resolve_value_path(
                    self.world,
                    self.module,
                    &self.generics,
                    self.self_ty.as_ref(),
                    path,
                );
                if let Ok(ValuePath::Item(id)) = target {
                    let info = self.facts.consts.get(&id).cloned();
                    if let Some(info) = info {
                        if let ConstValue::Int(value) = &info.value {
                            if *value >= 0 {
                                self.facts
                                    .resolutions
                                    .insert(path.id, Resolution::Item { item: id });
                                self.record_expr_type(expr.id, &info.ty);
                                return Some(*value as u128);
                            }
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn check_tuple(&mut self, items: &[Expr]) -> Type {
        if items.is_empty() {
            return Type::Unit;
        }
        let types = self.check_expr_list(items, None);
        Type::Tuple(types)
    }

    fn check_struct_lit(
        &mut self,
        expr: &Expr,
        path: &Path,
        fields: &[FieldInit],
        expected: Option<&Type>,
    ) -> Type {
        let target = match resolve::resolve_value_path(
            self.world,
            self.module,
            &self.generics,
            self.self_ty.as_ref(),
            path,
        ) {
            Ok(target) => target,
            Err(error) => {
                self.emit_path_error(path.span, &error);
                self.facts.resolutions.insert(path.id, Resolution::Error);
                for field in fields {
                    self.check_expr(&field.value, None);
                }
                return Type::Error;
            }
        };
        // Le codegen exige un fait de résolution pour le chemin du littéral.
        match &target {
            ValuePath::Type(Type::Nominal { def, .. }) => {
                self.facts
                    .resolutions
                    .insert(path.id, Resolution::Item { item: *def });
            }
            ValuePath::Variant { enum_item, variant } => {
                self.facts.resolutions.insert(
                    path.id,
                    Resolution::Variant {
                        enum_item: *enum_item,
                        variant: *variant,
                    },
                );
            }
            ValuePath::BuiltinConst(ctor_path) => {
                self.facts.resolutions.insert(
                    path.id,
                    Resolution::BuiltinConst {
                        path: (*ctor_path).to_string(),
                    },
                );
            }
            _ => {
                self.facts.resolutions.insert(path.id, Resolution::Error);
            }
        }
        match target {
            ValuePath::Type(Type::Nominal { def, name, args }) => {
                if self.world.nominal_kind(def) != Some(NominalKind::Struct) {
                    self.error(
                        "E-RES-UNKNOWN",
                        path.span,
                        format!("`{name}` is not a struct"),
                    );
                    for field in fields {
                        self.check_expr(&field.value, None);
                    }
                    return Type::Error;
                }
                let info = match self.facts.structs.get(&def) {
                    Some(info) => info.clone(),
                    None => return Type::Error,
                };
                let mut subst: BTreeMap<usize, Type> = BTreeMap::new();
                if let Some(Type::Nominal {
                    def: exp_def,
                    args: exp_args,
                    ..
                }) = expected
                {
                    if *exp_def == def && exp_args.len() == info.generics.len() {
                        for (i, arg) in exp_args.iter().enumerate() {
                            subst.insert(i, arg.clone());
                        }
                    }
                }
                if !args.is_empty() && args.len() == info.generics.len() {
                    for (i, arg) in args.iter().enumerate() {
                        subst.insert(i, arg.clone());
                    }
                }
                let fields_ok = self.check_struct_fields(
                    expr,
                    fields,
                    &info.fields,
                    &info.generics,
                    &def,
                    &mut subst,
                );
                let _ = fields_ok;
                let mut final_args = Vec::with_capacity(info.generics.len());
                for index in 0..info.generics.len() {
                    match subst.get(&index) {
                        Some(t) => final_args.push(self.resolve_vars(t)),
                        None => final_args.push(self.fresh_var(expr.span)),
                    }
                }
                Type::Nominal {
                    def,
                    name: info.name.clone(),
                    args: final_args,
                }
            }
            ValuePath::Variant { enum_item, variant } => {
                let info = match self.facts.enums.get(&enum_item) {
                    Some(info) => info.clone(),
                    None => return Type::Error,
                };
                let variant_info = match info.variants.get(variant) {
                    Some(v) => v.clone(),
                    None => return Type::Error,
                };
                if variant_info.kind != crate::facts::VariantKind::Struct {
                    self.error(
                        "E-TYP-FIELD",
                        path.span,
                        format!(
                            "variant `{}::{}` has no named fields",
                            info.name, variant_info.name
                        ),
                    );
                    for field in fields {
                        self.check_expr(&field.value, None);
                    }
                    return Type::Error;
                }
                let mut subst: BTreeMap<usize, Type> = BTreeMap::new();
                if let Some(Type::Nominal {
                    def: exp_def,
                    args: exp_args,
                    ..
                }) = expected
                {
                    if *exp_def == enum_item && exp_args.len() == info.generics.len() {
                        for (i, arg) in exp_args.iter().enumerate() {
                            subst.insert(i, arg.clone());
                        }
                    }
                }
                self.check_struct_fields(
                    expr,
                    fields,
                    &variant_info.fields,
                    &info.generics,
                    &enum_item,
                    &mut subst,
                );
                let mut final_args = Vec::with_capacity(info.generics.len());
                for index in 0..info.generics.len() {
                    match subst.get(&index) {
                        Some(t) => final_args.push(t.clone()),
                        None => {
                            final_args.push(Type::Error);
                            self.error(
                                "E-TYP-AMBIGUOUS",
                                expr.span,
                                format!("cannot infer the generic arguments of `{}`", info.name),
                            );
                        }
                    }
                }
                Type::Nominal {
                    def: enum_item,
                    name: info.name.clone(),
                    args: final_args,
                }
            }
            ValuePath::BuiltinConst(path) => self.builtin_unit_variant_type(expr, path, expected),
            _ => {
                self.error(
                    "E-RES-UNKNOWN",
                    path.span,
                    "struct literal must target a struct or a struct variant".to_string(),
                );
                for field in fields {
                    self.check_expr(&field.value, None);
                }
                Type::Error
            }
        }
    }

    fn check_struct_fields(
        &mut self,
        expr: &Expr,
        provided: &[FieldInit],
        declared: &[crate::facts::FieldInfo],
        generics: &[String],
        def: &NodeId,
        subst: &mut BTreeMap<usize, Type>,
    ) -> bool {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for field in provided {
            if !seen.insert(field.name.name.as_str()) {
                self.error(
                    "E-TYP-FIELD",
                    field.name.span,
                    format!("duplicate field `{}`", field.name.name),
                );
            }
            let Some(decl) = declared.iter().find(|d| d.name == field.name.name) else {
                self.error(
                    "E-TYP-FIELD",
                    field.name.span,
                    format!("no field `{}` in this aggregate", field.name.name),
                );
                self.check_expr(&field.value, None);
                continue;
            };
            let decl = decl.clone();
            if !decl.public && !self.aggregate_field_visible(def) {
                self.error(
                    "E-RES-PRIVATE",
                    field.name.span,
                    format!("field `{}` is private", decl.name),
                );
            }
            let expected_ty = substitute(&decl.ty, subst);
            let value_ty = self.check_arg(&field.value, &expected_ty);
            // TYP-1.6: `!` must not determine a generic field type. Without
            // this check, `W { v: panic("x") }` leaks the unresolved `T` into
            // the generated Rust (E0425). `check_arg`/`coerce` returns the
            // expected type for a `!` argument, so the raw expression type is
            // read from the facts recorded by `check_expr`.
            let raw_value_ty = self
                .facts
                .expr_types
                .get(&field.value.id)
                .cloned()
                .unwrap_or_else(|| value_ty.clone());
            if contains_never(&raw_value_ty)
                && (matches!(expected_ty, Type::Param { .. } | Type::Error)
                    || has_unbound_param(&expected_ty))
            {
                self.error(
                    "E-TYP-AMBIGUOUS",
                    field.value.span,
                    "cannot infer a generic field type from a diverging expression; \
                     annotate the field type or use a non-diverging value"
                        .to_string(),
                );
            }
            if !self.unify_pattern(&decl.ty, &value_ty, subst) {
                self.error(
                    "E-TYP-MISMATCH",
                    field.value.span,
                    format!(
                        "field `{}`: expected `{}`, found `{}`",
                        field.name.name, expected_ty, value_ty
                    ),
                );
            }
        }
        let missing: Vec<String> = declared
            .iter()
            .filter(|d| !seen.contains(d.name.as_str()))
            .map(|d| d.name.clone())
            .collect();
        if !missing.is_empty() {
            self.error(
                "E-TYP-MISSING-FIELD",
                expr.span,
                format!("missing field(s): {}", missing.join(", ")),
            );
        }
        let _ = generics;
        true
    }

    fn aggregate_field_visible(&self, def: &NodeId) -> bool {
        let def_module = self.world.item_module.get(def).copied().unwrap_or(0);
        let mut current = Some(self.module);
        while let Some(m) = current {
            if m == def_module {
                return true;
            }
            current = self.world.modules[m].parent;
        }
        false
    }

    // ------------------------------------------------------------------
    // Patterns et exhaustivité
    // ------------------------------------------------------------------

    fn check_pattern(&mut self, pat: &Pat, expected: &Type) -> Type {
        let ty = match &pat.kind {
            PatKind::Wild => expected.clone(),
            PatKind::Binding { mutable, name } => {
                self.bind_local(&name.name, expected.clone(), *mutable, false);
                expected.clone()
            }
            PatKind::Lit(lit) => self.check_lit_pattern(pat, lit, expected),
            PatKind::Tuple(items) => {
                if let Type::Tuple(types) = expected {
                    if types.len() != items.len() {
                        // Arity, not a type mismatch: every other arity check in
                        // this file (calls, variants, other patterns) reports
                        // `E-TYP-ARITY`, and `spec/language/features.md`
                        // F-SYN-030 mandates that code here too.
                        self.error(
                            "E-TYP-ARITY",
                            pat.span,
                            format!(
                                "tuple pattern has {} element(s), type has {}",
                                items.len(),
                                types.len()
                            ),
                        );
                    }
                    for (item, ty) in items.iter().zip(types.iter()) {
                        self.check_pattern(item, ty);
                    }
                    expected.clone()
                } else {
                    self.error(
                        "E-TYP-MISMATCH",
                        pat.span,
                        format!("tuple pattern cannot match type `{expected}`"),
                    );
                    for item in items {
                        self.check_pattern(item, &Type::Error);
                    }
                    Type::Error
                }
            }
            PatKind::Path(path) => self.check_path_pattern(pat, path, expected),
            PatKind::Variant { path, fields } => {
                self.check_variant_pattern(pat, path, fields, expected)
            }
        };
        self.facts.pat_types.insert(pat.id, ty.clone());
        ty
    }

    fn check_lit_pattern(&mut self, pat: &Pat, lit: &LitPat, expected: &Type) -> Type {
        match lit {
            LitPat::Int { value, suffix, neg } => {
                let ty = match suffix {
                    Some(suffix) => int_type(*suffix),
                    None => match expected {
                        Type::I8
                        | Type::I16
                        | Type::I32
                        | Type::I64
                        | Type::I128
                        | Type::Isize
                        | Type::U8
                        | Type::U16
                        | Type::U32
                        | Type::U64
                        | Type::U128
                        | Type::Usize => expected.clone(),
                        _ => Type::I32,
                    },
                };
                // TYP-1.6: `!` coerces to every type, so a literal pattern on a
                // diverging scrutinee is not a type mismatch (the bool, char and
                // text patterns below already exempt `!`/`<error>`). Without
                // this exemption `match panic("x") { 1 => …, _ => … }` was
                // rejected while the same match with `true`/`'a'`/`"a"` was not.
                if !expected.is_int() && !matches!(expected, Type::Error | Type::Never) {
                    self.error(
                        "E-TYP-MISMATCH",
                        pat.span,
                        format!("integer pattern cannot match type `{expected}`"),
                    );
                }
                // G-6.37: the sign decides which bound applies. A negative
                // pattern never fits an unsigned type, and `-128` fits `i8`
                // while `128` does not (`int_bounds` gives the magnitude of the
                // minimum). Before G-6.37 was implemented the checker guessed a
                // negative from a two's-complement `value > i128::MAX`, which
                // also accepted an out-of-range positive literal.
                let fits = if *neg {
                    negative_value_fits(*value, &ty)
                } else {
                    int_value_fits(*value, &ty)
                };
                if !fits {
                    let shown = if *neg {
                        format!("-{value}")
                    } else {
                        format!("{value}")
                    };
                    self.error(
                        "E-TYP-LITERAL-RANGE",
                        pat.span,
                        format!("integer pattern `{shown}` does not fit in `{ty}`"),
                    );
                }
                ty
            }
            // G-6.36: a float pattern compares with `==`; the literal type is
            // the suffix, else the scrutinee type, else `f64` (the default of
            // float expressions). `NaN` never matches and `-0.0` matches `0.0`
            // because no bit comparison is made.
            LitPat::Float { suffix, .. } => {
                let ty = match suffix {
                    Some(FloatTy::F32) => Type::F32,
                    Some(FloatTy::F64) => Type::F64,
                    None => match expected {
                        Type::F32 => Type::F32,
                        _ => Type::F64,
                    },
                };
                if !matches!(expected, Type::F32 | Type::F64 | Type::Error | Type::Never) {
                    self.error(
                        "E-TYP-MISMATCH",
                        pat.span,
                        format!("float pattern cannot match type `{expected}`"),
                    );
                }
                ty
            }
            LitPat::Bool(_) => {
                if *expected != Type::Bool && !matches!(expected, Type::Error | Type::Never) {
                    self.error(
                        "E-TYP-MISMATCH",
                        pat.span,
                        format!("`bool` pattern cannot match type `{expected}`"),
                    );
                }
                Type::Bool
            }
            LitPat::Char(_) => {
                if *expected != Type::Char && !matches!(expected, Type::Error | Type::Never) {
                    self.error(
                        "E-TYP-MISMATCH",
                        pat.span,
                        format!("`char` pattern cannot match type `{expected}`"),
                    );
                }
                Type::Char
            }
            LitPat::Str(_) => {
                if !is_text_like(expected) && !matches!(expected, Type::Error | Type::Never) {
                    self.error(
                        "E-TYP-MISMATCH",
                        pat.span,
                        format!("text pattern cannot match type `{expected}`"),
                    );
                }
                Type::str_ref()
            }
        }
    }

    fn check_path_pattern(&mut self, pat: &Pat, path: &Path, expected: &Type) -> Type {
        match resolve::resolve_value_path(
            self.world,
            self.module,
            &self.generics,
            self.self_ty.as_ref(),
            path,
        ) {
            Ok(ValuePath::Variant { enum_item, variant }) => {
                self.variant_pats.insert(pat.id, (enum_item, variant));
                self.facts
                    .resolutions
                    .insert(path.id, Resolution::Variant { enum_item, variant });
                self.pattern_enum_ty(pat.span, enum_item, expected)
            }
            Ok(ValuePath::BuiltinConst(ctor_path)) => {
                self.facts.resolutions.insert(
                    path.id,
                    Resolution::BuiltinConst {
                        path: ctor_path.to_string(),
                    },
                );
                match ctor_path {
                    "Option::None" => match expected {
                        Type::Option(_) => expected.clone(),
                        _ => {
                            self.error(
                                "E-TYP-MISMATCH",
                                pat.span,
                                format!("`None` pattern cannot match type `{expected}`"),
                            );
                            Type::Error
                        }
                    },
                    _ => Type::Error,
                }
            }
            Ok(ValuePath::Item(id)) => {
                if let Some(info) = self.facts.consts.get(&id) {
                    let ty = info.ty.clone();
                    self.facts
                        .resolutions
                        .insert(path.id, Resolution::Item { item: id });
                    if !Self::compatible(&ty, expected) {
                        self.error(
                            "E-TYP-MISMATCH",
                            pat.span,
                            format!("constant pattern has type `{ty}`, expected `{expected}`"),
                        );
                    }
                    ty
                } else {
                    self.error(
                        "E-RES-UNKNOWN",
                        pat.span,
                        "constant pattern must reference a `const`".to_string(),
                    );
                    Type::Error
                }
            }
            Ok(other) => {
                let _ = other;
                self.error(
                    "E-RES-UNKNOWN",
                    pat.span,
                    "pattern does not name a variant or a constant".to_string(),
                );
                Type::Error
            }
            Err(error) => {
                self.emit_path_error(pat.span, &error);
                Type::Error
            }
        }
    }

    fn pattern_enum_ty(&mut self, span: Span, enum_item: NodeId, expected: &Type) -> Type {
        match expected {
            Type::Nominal { def, .. } if *def == enum_item => expected.clone(),
            Type::Error | Type::Never => expected.clone(),
            _ => {
                self.error(
                    "E-TYP-MISMATCH",
                    span,
                    format!("variant pattern does not match type `{expected}`"),
                );
                Type::Error
            }
        }
    }

    fn check_variant_pattern(
        &mut self,
        pat: &Pat,
        path: &Path,
        fields: &VariantPatFields,
        expected: &Type,
    ) -> Type {
        let target = match resolve::resolve_value_path(
            self.world,
            self.module,
            &self.generics,
            self.self_ty.as_ref(),
            path,
        ) {
            Ok(target) => target,
            Err(error) => {
                self.emit_path_error(pat.span, &error);
                for field in variant_pat_patterns(fields) {
                    self.check_pattern(field, &Type::Error);
                }
                return Type::Error;
            }
        };
        match target {
            ValuePath::Variant { enum_item, variant } => {
                self.variant_pats.insert(pat.id, (enum_item, variant));
                self.facts
                    .resolutions
                    .insert(path.id, Resolution::Variant { enum_item, variant });
                let info = match self.facts.enums.get(&enum_item) {
                    Some(info) => info.clone(),
                    None => return Type::Error,
                };
                let variant_info = match info.variants.get(variant) {
                    Some(v) => v.clone(),
                    None => return Type::Error,
                };
                let subst = match expected {
                    Type::Nominal { def, args, .. } if *def == enum_item => {
                        let mut map = BTreeMap::new();
                        for (i, arg) in args.iter().enumerate() {
                            map.insert(i, arg.clone());
                        }
                        map
                    }
                    _ => BTreeMap::new(),
                };
                self.check_variant_payload(
                    pat,
                    fields,
                    &variant_info,
                    &subst,
                    &info.name,
                    &variant_info.name,
                );
                match expected {
                    Type::Nominal { def, .. } if *def == enum_item => expected.clone(),
                    Type::Error | Type::Never => expected.clone(),
                    _ => {
                        self.error(
                            "E-TYP-MISMATCH",
                            pat.span,
                            format!("variant pattern does not match type `{expected}`"),
                        );
                        Type::Error
                    }
                }
            }
            ValuePath::BuiltinCtor {
                path: ctor_path, ..
            } => {
                self.facts.resolutions.insert(
                    path.id,
                    Resolution::BuiltinFn {
                        path: ctor_path.to_string(),
                    },
                );
                self.check_builtin_variant_payload(pat, ctor_path, fields, expected)
            }
            ValuePath::BuiltinConst(ctor_path) => {
                self.facts.resolutions.insert(
                    path.id,
                    Resolution::BuiltinConst {
                        path: ctor_path.to_string(),
                    },
                );
                self.check_builtin_variant_payload(pat, ctor_path, fields, expected)
            }
            _ => {
                self.error(
                    "E-RES-UNKNOWN",
                    pat.span,
                    "pattern does not name a variant".to_string(),
                );
                for field in variant_pat_patterns(fields) {
                    self.check_pattern(field, &Type::Error);
                }
                Type::Error
            }
        }
    }

    fn check_variant_payload(
        &mut self,
        pat: &Pat,
        fields: &VariantPatFields,
        variant_info: &crate::facts::VariantInfo,
        subst: &BTreeMap<usize, Type>,
        enum_name: &str,
        variant_name: &str,
    ) {
        match fields {
            VariantPatFields::Tuple(pats) => {
                if variant_info.kind != crate::facts::VariantKind::Tuple
                    || pats.len() != variant_info.tuple_types.len()
                {
                    self.error(
                        "E-TYP-ARITY",
                        pat.span,
                        format!(
                            "variant `{enum_name}::{variant_name}` expects {} field(s), found {}",
                            variant_info.tuple_types.len(),
                            pats.len()
                        ),
                    );
                }
                for (sub, ty) in pats.iter().zip(variant_info.tuple_types.iter()) {
                    let ty = substitute(ty, subst);
                    self.check_pattern(sub, &ty);
                }
            }
            VariantPatFields::Struct(field_pats) => {
                if variant_info.kind != crate::facts::VariantKind::Struct {
                    self.error(
                        "E-TYP-ARITY",
                        pat.span,
                        format!("variant `{enum_name}::{variant_name}` has no named fields"),
                    );
                }
                let mut seen: BTreeSet<&str> = BTreeSet::new();
                for field in field_pats {
                    seen.insert(field.name.name.as_str());
                    match variant_info
                        .fields
                        .iter()
                        .find(|f| f.name == field.name.name)
                    {
                        Some(decl) => {
                            let ty = substitute(&decl.ty, subst);
                            self.check_pattern(&field.pat, &ty);
                        }
                        None => {
                            self.error(
                                "E-TYP-FIELD",
                                field.name.span,
                                format!("no field `{}` in this variant", field.name.name),
                            );
                            self.check_pattern(&field.pat, &Type::Error);
                        }
                    }
                }
                let missing: Vec<String> = variant_info
                    .fields
                    .iter()
                    .filter(|f| !seen.contains(f.name.as_str()))
                    .map(|f| f.name.clone())
                    .collect();
                if !missing.is_empty() {
                    self.error(
                        "E-TYP-MISSING-FIELD",
                        pat.span,
                        format!("missing field(s): {}", missing.join(", ")),
                    );
                }
            }
        }
    }

    fn check_builtin_variant_payload(
        &mut self,
        pat: &Pat,
        path: &str,
        fields: &VariantPatFields,
        expected: &Type,
    ) -> Type {
        match path {
            "Option::Some" => {
                let inner = match expected {
                    Type::Option(inner) => (**inner).clone(),
                    _ => {
                        self.error(
                            "E-TYP-MISMATCH",
                            pat.span,
                            format!("`Some` pattern cannot match type `{expected}`"),
                        );
                        Type::Error
                    }
                };
                match fields {
                    VariantPatFields::Tuple(pats) => {
                        if pats.len() != 1 {
                            self.error(
                                "E-TYP-ARITY",
                                pat.span,
                                "`Some` pattern expects exactly 1 field".to_string(),
                            );
                        }
                        for sub in pats {
                            self.check_pattern(sub, &inner);
                        }
                    }
                    VariantPatFields::Struct(_) => {
                        self.error(
                            "E-TYP-ARITY",
                            pat.span,
                            "`Some` pattern takes a tuple field".to_string(),
                        );
                    }
                }
                expected.clone()
            }
            "Result::Ok" | "Result::Err" => {
                let payload = match (path, expected) {
                    ("Result::Ok", Type::Result(ok, _)) => (**ok).clone(),
                    ("Result::Err", Type::Result(_, err)) => (**err).clone(),
                    _ => {
                        self.error(
                            "E-TYP-MISMATCH",
                            pat.span,
                            format!("`{path}` pattern cannot match type `{expected}`"),
                        );
                        Type::Error
                    }
                };
                match fields {
                    VariantPatFields::Tuple(pats) => {
                        if pats.len() != 1 {
                            self.error(
                                "E-TYP-ARITY",
                                pat.span,
                                format!("`{path}` pattern expects exactly 1 field"),
                            );
                        }
                        for sub in pats {
                            self.check_pattern(sub, &payload);
                        }
                    }
                    VariantPatFields::Struct(_) => {
                        self.error(
                            "E-TYP-ARITY",
                            pat.span,
                            format!("`{path}` pattern takes a tuple field"),
                        );
                    }
                }
                expected.clone()
            }
            "Option::None" => {
                if !matches!(expected, Type::Option(_)) {
                    self.error(
                        "E-TYP-MISMATCH",
                        pat.span,
                        format!("`None` pattern cannot match type `{expected}`"),
                    );
                }
                // TYP-10.2 / G-6.29: `None` is a unit constructor. Its payload
                // was never validated, so `match o { None(v) => … }` on an
                // `Option<i32>` was accepted while every user unit variant
                // correctly reports E-TYP-ARITY. A payload on `None` is never
                // reachable at run time.
                match fields {
                    VariantPatFields::Tuple(pats) if pats.is_empty() => {}
                    VariantPatFields::Tuple(pats) => {
                        self.error(
                            "E-TYP-ARITY",
                            pat.span,
                            format!("`None` pattern takes no field, {} given", pats.len()),
                        );
                        for sub in pats {
                            self.check_pattern(sub, &Type::Error);
                        }
                    }
                    VariantPatFields::Struct(struct_fields) => {
                        self.error(
                            "E-TYP-ARITY",
                            pat.span,
                            "`None` pattern takes no field".to_string(),
                        );
                        for field in struct_fields {
                            self.check_pattern(&field.pat, &Type::Error);
                        }
                    }
                }
                expected.clone()
            }
            _ => Type::Error,
        }
    }

    // ------------------------------------------------------------------
    // Constantes
    // ------------------------------------------------------------------

    fn check_const(&mut self, module: ModuleId, item: NodeId, c: &ConstItem) {
        self.module = module;
        self.generics.clear();
        self.self_ty = None;
        self.fn_ret = Type::Unit;
        self.current_fn = None;
        self.scopes.clear();
        self.push_scope();
        self.in_const = true;
        let declared = self
            .facts
            .type_nodes
            .get(&c.ty.id)
            .cloned()
            .unwrap_or_else(|| self.resolve_type(&c.ty));
        // G-5.11 / TYP-1.6: a constant of type `!` is not constructible.
        if crate::types::contains_never(&declared) {
            self.error(
                "E-TYP-MISMATCH",
                c.ty.span,
                "the never type (`!`) is not a valid constant type in Core 1.0".to_string(),
            );
        }
        let (value_ty, value) = self.check_const_expr(&c.value, Some(&declared));
        if !Self::compatible(&value_ty, &declared) {
            self.error(
                "E-TYP-MISMATCH",
                c.value.span,
                format!("constant value has type `{value_ty}`, declared `{declared}`"),
            );
        }
        if let Some(value) = value {
            let rust_path = self
                .facts
                .item_paths
                .get(&item)
                .cloned()
                .unwrap_or_else(|| {
                    let module_path = self.world.modules[module].path.clone();
                    resolve::join_path(&module_path, &resolve::rust_ident(&c.name.name))
                });
            self.facts.consts.insert(
                item,
                ConstInfo {
                    item,
                    rust_path,
                    name: c.name.name.clone(),
                    ty: declared,
                    value,
                },
            );
        }
        self.in_const = false;
        self.pop_scope();
    }

    fn check_const_expr(
        &mut self,
        expr: &Expr,
        expected: Option<&Type>,
    ) -> (Type, Option<ConstValue>) {
        let (ty, value) = match &expr.kind {
            ExprKind::Int { value, suffix } => {
                let ty = self.check_int_literal(expr, *value, *suffix, expected);
                let value = if ty.is_int() && int_value_fits(*value, &ty) {
                    Some(ConstValue::Int(int_literal_to_i128(*value, &ty)))
                } else {
                    None
                };
                (ty, value)
            }
            ExprKind::Bool(value) => (Type::Bool, Some(ConstValue::Bool(*value))),
            ExprKind::Char(value) => (Type::Char, Some(ConstValue::Char(*value))),
            ExprKind::Str(value) => (Type::str_ref(), Some(ConstValue::Str(value.clone()))),
            ExprKind::Float { text, suffix } => {
                let ty = self.check_float_literal(expr, text, *suffix, expected);
                self.error(
                    "E-RES-CONST-NOT-CONST",
                    expr.span,
                    "floating-point constants are not representable in Core 1.0 facts".to_string(),
                );
                (ty, None)
            }
            ExprKind::Unary { op, expr: operand } => match op {
                UnOp::Neg => {
                    // TYP-13.2 / G-6.37: `const MIN: i8 = -128;` is valid even
                    // though the positive magnitude 128 does not fit `i8`.
                    // Recursing first range-checked `128` and then reported a
                    // spurious E-RES-CONST-NOT-CONST ("not a constant"), while
                    // the same expression was accepted in a `let` (`check_unary`
                    // handles the sign before checking the magnitude).
                    if let ExprKind::Int { value, suffix } = &operand.kind {
                        let ty = match suffix {
                            Some(suffix) => int_type(*suffix),
                            None => match expected {
                                Some(t) if t.is_int() => t.clone(),
                                _ => Type::I32,
                            },
                        };
                        self.facts.expr_types.insert(operand.id, ty.clone());
                        if !ty.is_signed_int() {
                            self.error(
                                "E-TYP-MISMATCH",
                                expr.span,
                                format!("cannot negate a value of type `{ty}`"),
                            );
                            return (ty, None);
                        }
                        let (min_magnitude, _, _) = int_bounds(&ty);
                        if *value > min_magnitude {
                            self.error(
                                "E-TYP-LITERAL-RANGE",
                                operand.span,
                                format!("integer literal `-{value}` does not fit in `{ty}`"),
                            );
                            return (ty, None);
                        }
                        let magnitude = *value;
                        let negated = if magnitude == 1u128 << 127 {
                            i128::MIN
                        } else {
                            -(magnitude as i128)
                        };
                        return (ty, Some(ConstValue::Int(negated)));
                    }
                    let (operand_ty, operand_value) = self.check_const_expr(operand, expected);
                    match operand_value {
                        Some(ConstValue::Int(v)) => {
                            let ty = if operand_ty.is_int() {
                                operand_ty.clone()
                            } else {
                                Type::I32
                            };
                            match v.checked_neg() {
                                Some(neg) if int_i128_fits(neg, &ty) => {
                                    (ty, Some(ConstValue::Int(neg)))
                                }
                                _ => {
                                    self.error(
                                        "E-TYP-CONST",
                                        expr.span,
                                        "constant overflow".to_string(),
                                    );
                                    (ty, None)
                                }
                            }
                        }
                        _ => {
                            self.error(
                                "E-RES-CONST-NOT-CONST",
                                expr.span,
                                "unary minus requires an integer constant".to_string(),
                            );
                            (operand_ty, None)
                        }
                    }
                }
                UnOp::Not => {
                    let (ty, value) = self.check_const_expr(operand, expected);
                    let out = match value {
                        Some(ConstValue::Bool(v)) => Some(ConstValue::Bool(!v)),
                        Some(ConstValue::Int(v)) => Some(ConstValue::Int(!v)),
                        _ => None,
                    };
                    (ty, out)
                }
                _ => {
                    // G-5.11 (MUST): `const_expr` has no variable, no call, no
                    // field access, no index, no `?`, no block; those forms are
                    // `E-TYP-CONST`. `E-RES-CONST-NOT-CONST` is reserved for a
                    // value the checker's facts cannot represent (e.g. a float).
                    self.error(
                        "E-TYP-CONST",
                        expr.span,
                        "references are not constant expressions".to_string(),
                    );
                    (Type::Error, None)
                }
            },
            ExprKind::Binary { op, lhs, rhs } => {
                let (l_ty, l_val) = self.check_const_expr(lhs, expected);
                let (r_ty, r_val) = self.check_const_expr(rhs, Some(&l_ty));
                let _ = r_ty;
                self.const_binary(expr, *op, &l_ty, l_val, r_val)
            }
            ExprKind::Cast { expr: operand, ty } => {
                let (source_ty, source_val) = self.check_const_expr(operand, None);
                let target = self.resolve_type(ty);
                if !cast_allowed(&source_ty, &target) {
                    self.error(
                        "E-TYP-CAST",
                        operand.span,
                        format!("cast from `{source_ty}` to `{target}` is not allowed in Core 1.0"),
                    );
                    return (Type::Error, None);
                }
                let value = source_val.and_then(|v| cast_const_value(v, &source_ty, &target));
                (target, value)
            }
            ExprKind::Block(_) => {
                // G-5.11 (MUST): a block is not a `const_expr`.
                self.error(
                    "E-TYP-CONST",
                    expr.span,
                    "blocks are not constant expressions".to_string(),
                );
                (Type::Error, None)
            }
            _ => {
                // G-5.11 / G-5.13 (MUST): variable, call, field, index, `?` and
                // a reference to another `const` are `E-TYP-CONST`.
                self.error(
                    "E-TYP-CONST",
                    expr.span,
                    "this expression is not allowed in a constant".to_string(),
                );
                (Type::Error, None)
            }
        };
        self.record_expr_type(expr.id, &ty);
        (ty, value)
    }

    fn const_binary(
        &mut self,
        expr: &Expr,
        op: ast::BinOp,
        ty: &Type,
        lhs: Option<ConstValue>,
        rhs: Option<ConstValue>,
    ) -> (Type, Option<ConstValue>) {
        use ast::BinOp as B;
        match op {
            B::Add | B::Sub | B::Mul | B::Div | B::Rem => {
                if let (Some(ConstValue::Int(a)), Some(ConstValue::Int(b))) = (&lhs, &rhs) {
                    let result = match op {
                        B::Add => a.checked_add(*b),
                        B::Sub => a.checked_sub(*b),
                        B::Mul => a.checked_mul(*b),
                        B::Div => {
                            if *b == 0 {
                                self.error(
                                    "E-TYP-CONST",
                                    expr.span,
                                    "division by zero in constant".to_string(),
                                );
                                None
                            } else {
                                a.checked_div(*b)
                            }
                        }
                        B::Rem => {
                            if *b == 0 {
                                self.error(
                                    "E-TYP-CONST",
                                    expr.span,
                                    "remainder by zero in constant".to_string(),
                                );
                                None
                            } else {
                                a.checked_rem(*b)
                            }
                        }
                        _ => None,
                    };
                    match result {
                        Some(v) if int_i128_fits(v, ty) => (ty.clone(), Some(ConstValue::Int(v))),
                        Some(_) => {
                            self.error("E-TYP-CONST", expr.span, "constant overflow".to_string());
                            (ty.clone(), None)
                        }
                        None => {
                            self.error(
                                "E-TYP-CONST",
                                expr.span,
                                "constant evaluation overflow".to_string(),
                            );
                            (ty.clone(), None)
                        }
                    }
                } else if let (Some(ConstValue::Int(a)), Some(ConstValue::Int(b))) = (&lhs, &rhs) {
                    let _ = (a, b);
                    (ty.clone(), None)
                } else {
                    (ty.clone(), None)
                }
            }
            B::Eq | B::Ne | B::Lt | B::Le | B::Gt | B::Ge | B::And | B::Or => {
                let value = match (op, &lhs, &rhs) {
                    (B::And, Some(ConstValue::Bool(a)), Some(ConstValue::Bool(b))) => {
                        Some(ConstValue::Bool(*a && *b))
                    }
                    (B::Or, Some(ConstValue::Bool(a)), Some(ConstValue::Bool(b))) => {
                        Some(ConstValue::Bool(*a || *b))
                    }
                    (_, Some(ConstValue::Int(a)), Some(ConstValue::Int(b))) => {
                        let b = match op {
                            B::Eq => a == b,
                            B::Ne => a != b,
                            B::Lt => a < b,
                            B::Le => a <= b,
                            B::Gt => a > b,
                            B::Ge => a >= b,
                            _ => false,
                        };
                        Some(ConstValue::Bool(b))
                    }
                    _ => None,
                };
                (Type::Bool, value)
            }
            B::BitAnd | B::BitOr | B::BitXor | B::Shl | B::Shr => {
                if let (Some(ConstValue::Int(a)), Some(ConstValue::Int(b))) = (&lhs, &rhs) {
                    let result = match op {
                        B::BitAnd => Some(a & b),
                        B::BitOr => Some(a | b),
                        B::BitXor => Some(a ^ b),
                        B::Shl => {
                            if *b < 0 || *b >= 128 {
                                None
                            } else {
                                a.checked_shl(*b as u32)
                            }
                        }
                        B::Shr => {
                            if *b < 0 || *b >= 128 {
                                None
                            } else {
                                a.checked_shr(*b as u32)
                            }
                        }
                        _ => None,
                    };
                    match result {
                        Some(v) if int_i128_fits(v, ty) => (ty.clone(), Some(ConstValue::Int(v))),
                        _ => {
                            self.error(
                                "E-TYP-CONST",
                                expr.span,
                                "constant evaluation overflow".to_string(),
                            );
                            (ty.clone(), None)
                        }
                    }
                } else {
                    (ty.clone(), None)
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Aides libres
// ---------------------------------------------------------------------------

fn expr_span_match(scrutinee: &Expr, arms: &[MatchArm]) -> Span {
    match arms.first() {
        Some(arm) => arm.span.join(scrutinee.span),
        None => scrutinee.span,
    }
}

fn variant_pat_patterns(fields: &VariantPatFields) -> Vec<&Pat> {
    match fields {
        VariantPatFields::Tuple(pats) => pats.iter().collect(),
        VariantPatFields::Struct(fields) => fields.iter().map(|f| &f.pat).collect(),
    }
}

fn int_type(ty: IntTy) -> Type {
    match ty {
        IntTy::I8 => Type::I8,
        IntTy::I16 => Type::I16,
        IntTy::I32 => Type::I32,
        IntTy::I64 => Type::I64,
        IntTy::I128 => Type::I128,
        IntTy::U8 => Type::U8,
        IntTy::U16 => Type::U16,
        IntTy::U32 => Type::U32,
        IntTy::U64 => Type::U64,
        IntTy::U128 => Type::U128,
        IntTy::Isize => Type::Isize,
        IntTy::Usize => Type::Usize,
    }
}

/// Bornes d'un entier : (magnitude du minimum, maximum, signé).
fn int_bounds(ty: &Type) -> (u128, u128, bool) {
    let bits = ty.int_bits().unwrap_or(32);
    if ty.is_signed_int() {
        let min_magnitude = 1u128 << (bits - 1);
        let max = min_magnitude - 1;
        (min_magnitude, max, true)
    } else {
        let max = if bits == 128 {
            u128::MAX
        } else {
            (1u128 << bits) - 1
        };
        (0, max, false)
    }
}

fn int_value_fits(value: u128, ty: &Type) -> bool {
    if !ty.is_int() {
        return false;
    }
    let (_, max, _) = int_bounds(ty);
    value <= max
}

/// G-6.37: `-value` fits `ty` when `ty` is signed and the magnitude does not
/// exceed the magnitude of its minimum (`-128` fits `i8`, `-129` does not, and
/// no negative pattern fits an unsigned type).
fn negative_value_fits(value: u128, ty: &Type) -> bool {
    if !ty.is_int() {
        return false;
    }
    let (min_magnitude, _, signed) = int_bounds(ty);
    signed && value <= min_magnitude
}

fn int_i128_fits(value: i128, ty: &Type) -> bool {
    if !ty.is_int() {
        return false;
    }
    let (min_magnitude, max, signed) = int_bounds(ty);
    if signed {
        value >= -(min_magnitude as i128) && value <= max as i128
    } else {
        value >= 0 && value as u128 <= max
    }
}

fn int_literal_to_i128(value: u128, ty: &Type) -> i128 {
    let (_, max, signed) = int_bounds(ty);
    if signed && value > max {
        // Complément à deux : `-128i8` arrive ici avec la valeur 128.
        if value == 1u128 << 127 {
            i128::MIN
        } else {
            -(value as i128)
        }
    } else {
        value as i128
    }
}

fn deref_type(ty: &Type) -> Type {
    let mut current = ty.clone();
    while let Type::Ref { inner, .. } = current {
        current = *inner;
    }
    current
}

fn is_text_like(ty: &Type) -> bool {
    match ty {
        Type::String | Type::Str => true,
        Type::Ref { inner, .. } => matches!(**inner, Type::Str),
        _ => false,
    }
}

fn cast_allowed(source: &Type, target: &Type) -> bool {
    let source_scalar =
        source.is_int() || source.is_float() || matches!(source, Type::Bool | Type::Char);
    if source == target && source_scalar {
        return true;
    }
    match (source, target) {
        (Type::Never, _) => true,
        (s, t) if s.is_int() && t.is_int() => true,
        (s, Type::F32) | (s, Type::F64) if s.is_int() || s == &Type::F32 || s == &Type::F64 => true,
        (Type::F32, Type::F64) | (Type::F64, Type::F32) => true,
        (Type::F32 | Type::F64, t) if t.is_int() => true,
        (Type::Bool, t) if t.is_int() => true,
        (Type::Char, t) if t.is_int() => true,
        _ => false,
    }
}

fn is_untyped_num_literal(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Int { suffix: None, .. } | ExprKind::Float { suffix: None, .. } => true,
        ExprKind::Unary {
            op: UnOp::Neg,
            expr,
        } => is_untyped_num_literal(expr),
        _ => false,
    }
}

fn is_negative_int_literal(expr: &Expr) -> bool {
    matches!(
        &expr.kind,
        ExprKind::Unary {
            op: UnOp::Neg,
            expr,
        } if matches!(expr.kind, ExprKind::Int { .. })
    )
}

fn method_self_kind(ty: &Type) -> Option<MethodSelfKind> {
    Some(match ty {
        Type::String => MethodSelfKind::String,
        Type::Str => MethodSelfKind::StrRef,
        Type::Ref { inner, .. } if matches!(**inner, Type::Str) => MethodSelfKind::StrRef,
        Type::Slice(_) => MethodSelfKind::Slice,
        Type::Vec(_) => MethodSelfKind::Vec,
        Type::Option(_) => MethodSelfKind::Option,
        Type::Result(_, _) => MethodSelfKind::Result,
        Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128 | Type::Isize => {
            MethodSelfKind::SignedInt
        }
        Type::U8 | Type::U16 | Type::U32 | Type::U64 | Type::U128 | Type::Usize => {
            MethodSelfKind::Int
        }
        Type::F64 => MethodSelfKind::F64,
        Type::F32 => MethodSelfKind::F32,
        Type::Bool => MethodSelfKind::Bool,
        Type::Char => MethodSelfKind::Char,
        _ => return None,
    })
}

fn method_env_for(ty: &Type) -> Vec<(&'static str, Type)> {
    match ty {
        Type::Vec(elem) => vec![("Self", ty.clone()), ("T", (**elem).clone())],
        Type::Option(inner) => vec![("Self", ty.clone()), ("T", (**inner).clone())],
        Type::Result(ok, err) => vec![
            ("Self", ty.clone()),
            ("T", (**ok).clone()),
            ("E", (**err).clone()),
        ],
        Type::Slice(elem) => vec![("Self", ty.clone()), ("T", (**elem).clone())],
        _ => vec![("Self", ty.clone())],
    }
}

fn nominal_def(ty: &Type) -> Option<NodeId> {
    match ty {
        Type::Nominal { def, .. } => Some(*def),
        _ => None,
    }
}

fn nominal_subst(world: &World, def: &NodeId, args: &[Type]) -> BTreeMap<usize, Type> {
    let generics = world.type_generics(*def);
    let mut subst = BTreeMap::new();
    for (index, arg) in args.iter().enumerate() {
        if index < generics.len() {
            subst.insert(index, arg.clone());
        }
    }
    subst
}

fn collect_generic_args(
    generics: &[String],
    subst: &BTreeMap<usize, Type>,
    report: &mut dyn FnMut(Span),
    span: Span,
) -> Vec<Type> {
    let mut out = Vec::with_capacity(generics.len());
    for index in 0..generics.len() {
        match subst.get(&index) {
            Some(t) => out.push(t.clone()),
            None => {
                out.push(Type::Error);
                report(span);
            }
        }
    }
    out
}

fn substitute(ty: &Type, subst: &BTreeMap<usize, Type>) -> Type {
    match ty {
        Type::Param { index, .. } => subst.get(index).cloned().unwrap_or_else(|| ty.clone()),
        Type::Ref { mutable, inner } => Type::Ref {
            mutable: *mutable,
            inner: Box::new(substitute(inner, subst)),
        },
        Type::Array { elem, len } => Type::Array {
            elem: Box::new(substitute(elem, subst)),
            len: *len,
        },
        Type::Slice(inner) => Type::Slice(Box::new(substitute(inner, subst))),
        Type::Tuple(items) => Type::Tuple(items.iter().map(|t| substitute(t, subst)).collect()),
        Type::Vec(inner) => Type::Vec(Box::new(substitute(inner, subst))),
        Type::Option(inner) => Type::Option(Box::new(substitute(inner, subst))),
        Type::Result(ok, err) => Type::Result(
            Box::new(substitute(ok, subst)),
            Box::new(substitute(err, subst)),
        ),
        Type::Nominal { def, name, args } => Type::Nominal {
            def: *def,
            name: name.clone(),
            args: args.iter().map(|t| substitute(t, subst)).collect(),
        },
        other => other.clone(),
    }
}

fn has_unbound_param(ty: &Type) -> bool {
    match ty {
        Type::Param { .. } => true,
        Type::Ref { inner, .. } => has_unbound_param(inner),
        Type::Array { elem, .. } => has_unbound_param(elem),
        Type::Slice(inner) => has_unbound_param(inner),
        Type::Tuple(items) => items.iter().any(has_unbound_param),
        Type::Vec(inner) => has_unbound_param(inner),
        Type::Option(inner) => has_unbound_param(inner),
        Type::Result(ok, err) => has_unbound_param(ok) || has_unbound_param(err),
        Type::Nominal { args, .. } => args.iter().any(has_unbound_param),
        _ => false,
    }
}

/// Résout les variables d'inférence connues ; les variables libres restent
/// telles quelles (elles peuvent être liées plus tard).
fn resolve_vars_keep(vars: &[Option<Type>], ty: &Type) -> Type {
    resolve_vars_impl(vars, ty, None, 0)
}

/// Résout les variables d'inférence pour de bon : une variable libre devient
/// `Error` et son index est enregistré.
fn resolve_vars_final(vars: &[Option<Type>], ty: &Type, unresolved: &mut BTreeSet<usize>) -> Type {
    resolve_vars_impl(vars, ty, Some(unresolved), 0)
}

fn resolve_vars_impl(
    vars: &[Option<Type>],
    ty: &Type,
    mut unresolved: Option<&mut BTreeSet<usize>>,
    depth: usize,
) -> Type {
    if depth > 64 {
        return Type::Error;
    }
    if let Some(index) = infer_var_index(ty) {
        return match vars.get(index).and_then(|v| v.clone()) {
            Some(bound) => resolve_vars_impl(vars, &bound, unresolved, depth + 1),
            None => match unresolved.as_deref_mut() {
                Some(set) => {
                    set.insert(index);
                    Type::Error
                }
                None => ty.clone(),
            },
        };
    }
    match ty {
        Type::Ref { mutable, inner } => Type::Ref {
            mutable: *mutable,
            inner: Box::new(resolve_vars_impl(vars, inner, unresolved, depth + 1)),
        },
        Type::Array { elem, len } => Type::Array {
            elem: Box::new(resolve_vars_impl(vars, elem, unresolved, depth + 1)),
            len: *len,
        },
        Type::Slice(inner) => Type::Slice(Box::new(resolve_vars_impl(
            vars,
            inner,
            unresolved,
            depth + 1,
        ))),
        Type::Tuple(items) => Type::Tuple(
            items
                .iter()
                .map(|t| resolve_vars_impl(vars, t, unresolved.as_deref_mut(), depth + 1))
                .collect(),
        ),
        Type::Vec(inner) => Type::Vec(Box::new(resolve_vars_impl(
            vars,
            inner,
            unresolved,
            depth + 1,
        ))),
        Type::Option(inner) => Type::Option(Box::new(resolve_vars_impl(
            vars,
            inner,
            unresolved,
            depth + 1,
        ))),
        Type::Result(ok, err) => Type::Result(
            Box::new(resolve_vars_impl(
                vars,
                ok,
                unresolved.as_deref_mut(),
                depth + 1,
            )),
            Box::new(resolve_vars_impl(vars, err, unresolved, depth + 1)),
        ),
        Type::Nominal { def, name, args } => Type::Nominal {
            def: *def,
            name: name.clone(),
            args: args
                .iter()
                .map(|t| resolve_vars_impl(vars, t, unresolved.as_deref_mut(), depth + 1))
                .collect(),
        },
        other => other.clone(),
    }
}

fn cast_const_value(value: ConstValue, source: &Type, target: &Type) -> Option<ConstValue> {
    match value {
        ConstValue::Int(v) => {
            if target.is_int() {
                Some(ConstValue::Int(truncate_int(v, target)))
            } else if matches!(target, Type::Bool) {
                None
            } else {
                None
            }
        }
        ConstValue::Bool(v) => {
            if target.is_int() {
                Some(ConstValue::Int(if v { 1 } else { 0 }))
            } else {
                None
            }
        }
        ConstValue::Char(v) => {
            if target.is_int() {
                Some(ConstValue::Int(v as i128))
            } else {
                None
            }
        }
        ConstValue::Str(_) => None,
    }
    .filter(|_| !matches!(source, Type::Never))
}

fn truncate_int(value: i128, target: &Type) -> i128 {
    let bits = target.int_bits().unwrap_or(128).min(128);
    if bits == 128 {
        return value;
    }
    let mask = (1i128 << bits) - 1;
    let bits_value = value & mask;
    if target.is_signed_int() {
        let sign = 1i128 << (bits - 1);
        if bits_value & sign != 0 {
            bits_value - (1i128 << bits)
        } else {
            bits_value
        }
    } else {
        bits_value
    }
}

// ---------------------------------------------------------------------------
// Exhaustivité
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum Coverage {
    Wild,
    Bool {
        true_seen: bool,
        false_seen: bool,
    },
    Enum {
        def: NodeId,
        variants: Vec<String>,
        seen: BTreeSet<usize>,
    },
    Option {
        some: bool,
        none: bool,
    },
    Result {
        ok: bool,
        err: bool,
    },
    /// A tuple is a product type: one irrefutable tuple pattern covers every
    /// value. Refutable component patterns (literals, variants) do not.
    Tuple {
        covered: bool,
    },
    /// Type sans énumération possible : exige `_`.
    RequiresWild,
}

/// A pattern matches every value of its type (and therefore must be the last
/// arm of a `match`). Used for tuple coverage: `(a, b)` is irrefutable, while
/// `(1, b)` is not.
fn pattern_is_irrefutable(pat: &Pat) -> bool {
    match &pat.kind {
        PatKind::Wild | PatKind::Binding { .. } => true,
        PatKind::Tuple(items) => items.iter().all(pattern_is_irrefutable),
        _ => false,
    }
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

impl Coverage {
    fn new(scrutinee: &Type, facts: &Facts) -> Coverage {
        match scrutinee {
            Type::Bool => Coverage::Bool {
                true_seen: false,
                false_seen: false,
            },
            Type::Nominal { def, .. } => match facts.enums.get(def) {
                Some(info) => Coverage::Enum {
                    def: *def,
                    variants: info.variants.iter().map(|v| v.name.clone()).collect(),
                    seen: BTreeSet::new(),
                },
                None => Coverage::RequiresWild,
            },
            Type::Option(_) => Coverage::Option {
                some: false,
                none: false,
            },
            Type::Result(_, _) => Coverage::Result {
                ok: false,
                err: false,
            },
            Type::Tuple(_) => Coverage::Tuple { covered: false },
            Type::Error | Type::Never => Coverage::Wild,
            _ => Coverage::RequiresWild,
        }
    }

    fn is_exhaustive(&self) -> bool {
        match self {
            Coverage::Wild => true,
            Coverage::Bool {
                true_seen,
                false_seen,
            } => *true_seen && *false_seen,
            Coverage::Enum {
                def: _,
                variants,
                seen,
            } => seen.len() >= variants.len(),
            Coverage::Option { some, none } => *some && *none,
            Coverage::Result { ok, err } => *ok && *err,
            Coverage::Tuple { covered } => *covered,
            Coverage::RequiresWild => false,
        }
    }

    fn describe_missing(&self) -> String {
        match self {
            Coverage::Bool {
                true_seen,
                false_seen,
            } => {
                let mut missing = Vec::new();
                if !true_seen {
                    missing.push("true");
                }
                if !false_seen {
                    missing.push("false");
                }
                format!("non-exhaustive match: missing {}", missing.join(", "))
            }
            Coverage::Enum {
                def: _,
                variants,
                seen,
            } => {
                let missing: Vec<&str> = variants
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| !seen.contains(i))
                    .map(|(_, n)| n.as_str())
                    .collect();
                format!(
                    "non-exhaustive match: missing variant(s) {}",
                    missing.join(", ")
                )
            }
            Coverage::Option { some, none } => {
                let mut missing = Vec::new();
                if !some {
                    missing.push("Some");
                }
                if !none {
                    missing.push("None");
                }
                format!("non-exhaustive match: missing {}", missing.join(", "))
            }
            Coverage::Result { ok, err } => {
                let mut missing = Vec::new();
                if !ok {
                    missing.push("Ok");
                }
                if !err {
                    missing.push("Err");
                }
                format!("non-exhaustive match: missing {}", missing.join(", "))
            }
            _ => "non-exhaustive match: add a `_` arm".to_string(),
        }
    }
}

impl<'p, 'w, 'f> TypeChecker<'p, 'w, 'f> {
    fn cover_pattern(&mut self, coverage: &mut Coverage, pat: &Pat) {
        // G-6.36: codegen lowers a float literal sub-pattern to a guard, and a
        // guarded arm covers nothing. Treating it as coverage here would accept
        // `match o { Some(1.5) => …, None => … }` and emit a Rust match that
        // `rustc` rejects as non-exhaustive.
        if pattern_contains_float(pat) {
            return;
        }
        match coverage {
            Coverage::Wild => {}
            Coverage::Tuple { covered } => {
                if pattern_is_irrefutable(pat) {
                    *covered = true;
                }
            }
            Coverage::Bool {
                true_seen,
                false_seen,
            } => match &pat.kind {
                PatKind::Lit(LitPat::Bool(true)) => *true_seen = true,
                PatKind::Lit(LitPat::Bool(false)) => *false_seen = true,
                PatKind::Wild | PatKind::Binding { .. } => {
                    *true_seen = true;
                    *false_seen = true;
                }
                _ => {}
            },
            Coverage::Enum {
                def,
                variants,
                seen,
            } => match &pat.kind {
                PatKind::Wild | PatKind::Binding { .. } => {
                    for index in 0..variants.len() {
                        seen.insert(index);
                    }
                }
                _ => {
                    if let Some((enum_item, index)) = self.variant_pats.get(&pat.id) {
                        if enum_item == def {
                            seen.insert(*index);
                        }
                    }
                }
            },
            Coverage::Option { some, none } => match pattern_kind(pat) {
                PatternKind::Some => *some = true,
                PatternKind::None => *none = true,
                PatternKind::Wild => {
                    *some = true;
                    *none = true;
                }
                _ => {}
            },
            Coverage::Result { ok, err } => match pattern_kind(pat) {
                PatternKind::Ok => *ok = true,
                PatternKind::Err => *err = true,
                PatternKind::Wild => {
                    *ok = true;
                    *err = true;
                }
                _ => {}
            },
            Coverage::RequiresWild => {
                if matches!(pat.kind, PatKind::Wild | PatKind::Binding { .. }) {
                    *coverage = Coverage::Wild;
                }
            }
        }
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
