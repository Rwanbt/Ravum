//! Inférence et vérification des effets.
//!
//! Les effets sont calculés **bottom-up** pour chaque fonction (effets directs
//! des appels builtin et des constructions allouantes), puis composés sur le
//! graphe d'appels jusqu'à point fixe (récursion et appels mutuels compris).
//! Une déclaration `effects(...)` est un **majorant** : tout effet inféré absent
//! de la déclaration est `E-EFF-UNDECLARED`.
//!
//! Profils : `core` ne restreint rien ; `pure` et `rt` interdisent les effets
//! listés par [`Profile::forbidden`] à la fonction d'entrée (`main`, ou chaque
//! test si le programme est une bibliothèque). Le diagnostic `E-EFF-PROFILE`
//! porte une note avec la chaîne d'appel et la fonction fautive.
//!
//! Limite documentée : ceci est une analyse **statique de capacités**, pas un
//! bac à sable. Elle ne prouve pas l'absence d'effets non modélisés (timeouts,
//! signaux, panics intrinsèques) et ne restreint pas l'exécution.

use crate::builtins;
use crate::facts::{Effect, EffectSet, Facts, MethodTarget, Profile};
use ravum_lang_syntax::ast::NodeId;
use ravum_lang_syntax::ast::{
    Block, Expr, ExprKind, FnItem, ItemKind, MatchArm, Program, StmtKind,
};
use ravum_lang_syntax::diag::DiagnosticBag;
use ravum_lang_syntax::span::Span;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Effets directs d'une fonction, ses arêtes d'appel et les sources des effets
/// (pour les notes de profil).
#[derive(Default, Clone)]
struct FnEffects {
    direct: EffectSet,
    sources: Vec<(Effect, String)>,
    edges: Vec<NodeId>,
}

/// Compose les effets de toutes les fonctions, vérifie les bornes déclarées et
/// le profil. Met à jour `FnSig::effects` et `Facts::effects`.
pub fn infer_and_check(
    program: &Program,
    facts: &mut Facts,
    profile: Profile,
    diags: &mut DiagnosticBag,
) {
    let fns = collect_fns(program);
    let mut direct: BTreeMap<NodeId, FnEffects> = BTreeMap::new();
    for (id, f) in &fns {
        let mut effects = FnEffects::default();
        walk_block(&f.body, facts, &mut effects);
        direct.insert(*id, effects);
    }
    // Point fixe sur le graphe d'appels (monotone, borné par 10 effets).
    let mut composed: BTreeMap<NodeId, EffectSet> = direct
        .iter()
        .map(|(id, e)| (*id, e.direct.clone()))
        .collect();
    loop {
        let mut changed = false;
        let ids: Vec<NodeId> = direct.keys().copied().collect();
        for id in ids {
            let mut set = direct[&id].direct.clone();
            for callee in &direct[&id].edges {
                if let Some(callee_set) = composed.get(callee) {
                    set.extend(callee_set);
                }
            }
            let previous = composed.get(&id).cloned().unwrap_or_default();
            if set != previous {
                composed.insert(id, set);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    for (id, set) in &composed {
        facts.effects.insert(*id, set.clone());
        if let Some(sig) = facts.fn_sigs.get_mut(id) {
            sig.effects = set.clone();
        }
    }

    // Bornes déclarées : la déclaration est un majorant.
    let declared: Vec<(NodeId, String, EffectSet, Span)> = fns
        .iter()
        .filter_map(|(id, f)| {
            let sig = facts.fn_sigs.get(id)?;
            let decl = sig.declared_effects.clone()?;
            Some((*id, sig.name.clone(), decl, f.span))
        })
        .collect();
    for (id, name, decl, span) in declared {
        let inference = composed.get(&id).cloned().unwrap_or_default();
        let missing = inference.difference(&decl);
        if !missing.is_empty() {
            let names: Vec<&str> = missing.iter().map(|e| e.name()).collect();
            diags.push(
                ravum_lang_syntax::diag::Diagnostic::error(
                    "E-EFF-UNDECLARED",
                    span,
                    format!(
                        "function `{name}` has inferred effects not covered by its declaration"
                    ),
                )
                .with_note(
                    format!("missing declared effect(s): {}", names.join(", ")),
                    None,
                )
                .with_note(format!("inferred: {}", inference.display()), None),
            );
        }
    }

    // Profil : effet interdit atteint depuis l'entrée.
    let Some(forbidden) = profile.forbidden() else {
        return;
    };
    let entries: Vec<(NodeId, String)> = match facts.main_fn {
        Some(main) => vec![(
            main,
            facts
                .fn_sigs
                .get(&main)
                .map(|s| s.name.clone())
                .unwrap_or_else(|| "main".to_string()),
        )],
        None => facts
            .test_fns
            .iter()
            .map(|id| {
                (
                    *id,
                    facts
                        .fn_sigs
                        .get(id)
                        .map(|s| s.name.clone())
                        .unwrap_or_else(|| "<test>".to_string()),
                )
            })
            .collect(),
    };
    for (entry, entry_name) in entries {
        if let Some((chain, effect, source)) = find_violation(entry, &forbidden, &direct, facts) {
            let span = fns.get(&entry).map(|f| f.span).unwrap_or(Span::dummy());
            let chain_text: Vec<String> = chain
                .iter()
                .map(|id| {
                    facts
                        .fn_sigs
                        .get(id)
                        .map(|s| s.name.clone())
                        .unwrap_or_else(|| format!("fn#{id}"))
                })
                .collect();
            diags.push(
                ravum_lang_syntax::diag::Diagnostic::error(
                    "E-EFF-PROFILE",
                    span,
                    format!(
                        "profile `{}` forbids effect `{}` reachable from `{entry_name}`",
                        profile.name(),
                        effect.name()
                    ),
                )
                .with_note(format!("call chain: {}", chain_text.join(" -> ")), None)
                .with_note(format!("offending call: {source}"), None),
            );
        }
    }
}

fn collect_fns<'a>(program: &'a Program) -> BTreeMap<NodeId, &'a FnItem> {
    let mut out = BTreeMap::new();
    collect_module_fns(&program.root, &mut out);
    out
}

fn collect_module_fns<'a>(
    module: &'a ravum_lang_syntax::ast::Module,
    out: &mut BTreeMap<NodeId, &'a FnItem>,
) {
    for item in &module.items {
        match &item.kind {
            ItemKind::Fn(f) => {
                out.insert(item.id, f);
            }
            ItemKind::Mod(inner) => collect_module_fns(inner, out),
            ItemKind::Impl(imp) => {
                for member in &imp.items {
                    if let ItemKind::Fn(f) = &member.kind {
                        out.insert(member.id, f);
                    }
                }
            }
            _ => {}
        }
    }
}

fn walk_block(block: &Block, facts: &Facts, out: &mut FnEffects) {
    for stmt in &block.stmts {
        match &stmt.kind {
            StmtKind::Let(lets) => {
                if let Some(init) = &lets.init {
                    walk_expr(init, facts, out);
                }
            }
            StmtKind::Expr { expr, .. } => walk_expr(expr, facts, out),
        }
    }
    if let Some(tail) = &block.tail {
        walk_expr(tail, facts, out);
    }
}

fn walk_expr(expr: &Expr, facts: &Facts, out: &mut FnEffects) {
    match &expr.kind {
        ExprKind::Call { callee, args } => {
            match facts.calls.get(&expr.id) {
                Some(crate::facts::CallTarget::UserFn { item }) => out.edges.push(*item),
                Some(crate::facts::CallTarget::BuiltinFn { path }) => add_builtin_path(path, out),
                Some(crate::facts::CallTarget::VariantCtor { .. })
                | Some(crate::facts::CallTarget::Error)
                | None => {}
            }
            walk_expr(callee, facts, out);
            for arg in args {
                walk_expr(arg, facts, out);
            }
        }
        ExprKind::MethodCall { receiver, args, .. } => {
            match facts.methods.get(&expr.id) {
                Some(MethodTarget::User { item }) => out.edges.push(*item),
                Some(MethodTarget::Builtin { path, .. }) => add_method_path(path, out),
                Some(MethodTarget::Error) | None => {}
            }
            walk_expr(receiver, facts, out);
            for arg in args {
                walk_expr(arg, facts, out);
            }
        }
        ExprKind::Path(_) => {}
        ExprKind::Field { base, .. } => walk_expr(base, facts, out),
        ExprKind::TupleIndex { base, .. } => walk_expr(base, facts, out),
        ExprKind::Index { base, index } => {
            walk_expr(base, facts, out);
            walk_expr(index, facts, out);
        }
        ExprKind::Unary { expr, .. } => walk_expr(expr, facts, out),
        ExprKind::Binary { lhs, rhs, .. } => {
            walk_expr(lhs, facts, out);
            walk_expr(rhs, facts, out);
        }
        ExprKind::Assign { target, value } | ExprKind::AssignOp { target, value, .. } => {
            walk_expr(target, facts, out);
            walk_expr(value, facts, out);
        }
        ExprKind::Cast { expr, .. } => walk_expr(expr, facts, out),
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => {
            walk_expr(cond, facts, out);
            walk_block(then_block, facts, out);
            if let Some(else_branch) = else_branch {
                walk_expr(else_branch, facts, out);
            }
        }
        ExprKind::Match { scrutinee, arms } => {
            walk_expr(scrutinee, facts, out);
            for arm in arms {
                walk_match_arm(arm, facts, out);
            }
        }
        ExprKind::While { cond, body } => {
            walk_expr(cond, facts, out);
            walk_block(body, facts, out);
        }
        ExprKind::Loop { body } => walk_block(body, facts, out),
        ExprKind::For { iter, body, .. } => {
            walk_expr(iter, facts, out);
            walk_block(body, facts, out);
        }
        ExprKind::Block(block) => walk_block(block, facts, out),
        ExprKind::Tuple(items) | ExprKind::Array(items) => {
            for item in items {
                walk_expr(item, facts, out);
            }
        }
        ExprKind::ArrayRepeat { value, len } => {
            walk_expr(value, facts, out);
            walk_expr(len, facts, out);
        }
        ExprKind::StructLit { fields, .. } => {
            for field in fields {
                walk_expr(&field.value, facts, out);
            }
        }
        ExprKind::Range { start, end, .. } => {
            if let Some(start) = start {
                walk_expr(start, facts, out);
            }
            if let Some(end) = end {
                walk_expr(end, facts, out);
            }
        }
        ExprKind::Try(inner) => walk_expr(inner, facts, out),
        ExprKind::Return(value) => {
            if let Some(value) = value {
                walk_expr(value, facts, out);
            }
        }
        ExprKind::Unit
        | ExprKind::Int { .. }
        | ExprKind::Float { .. }
        | ExprKind::Bool(_)
        | ExprKind::Char(_)
        | ExprKind::Str(_)
        | ExprKind::SelfValue
        | ExprKind::Break
        | ExprKind::Continue => {}
    }
}

fn walk_match_arm(arm: &MatchArm, facts: &Facts, out: &mut FnEffects) {
    walk_expr(&arm.body, facts, out);
}

fn add_builtin_path(path: &str, out: &mut FnEffects) {
    let effects: Option<&'static [Effect]> = builtins::builtin_fn(path)
        .map(|sig| sig.effects)
        .or_else(|| builtins::builtin_assoc_fn(path).map(|sig| sig.effects))
        .or_else(|| builtins::builtin_method_effects(path));
    if let Some(effects) = effects {
        for effect in effects {
            out.direct.insert(*effect);
            out.sources.push((*effect, format!("call to `{path}`")));
        }
    }
}

fn add_method_path(path: &str, out: &mut FnEffects) {
    if let Some(effects) = builtins::builtin_method_effects(path) {
        for effect in effects {
            out.direct.insert(*effect);
            out.sources.push((*effect, format!("call to `{path}`")));
        }
    }
}

/// BFS depuis l'entrée vers la première fonction qui produit directement un
/// effet interdit. Renvoie la chaîne d'appel, l'effet et la source.
fn find_violation(
    entry: NodeId,
    forbidden: &EffectSet,
    direct: &BTreeMap<NodeId, FnEffects>,
    facts: &Facts,
) -> Option<(Vec<NodeId>, Effect, String)> {
    let mut queue: VecDeque<(NodeId, Vec<NodeId>)> = VecDeque::new();
    queue.push_back((entry, vec![entry]));
    let mut visited: BTreeSet<NodeId> = BTreeSet::new();
    while let Some((node, chain)) = queue.pop_front() {
        if !visited.insert(node) {
            continue;
        }
        if let Some(effects) = direct.get(&node) {
            if let Some((effect, source)) = effects
                .sources
                .iter()
                .find(|(effect, _)| forbidden.contains(*effect))
            {
                return Some((chain, *effect, source.clone()));
            }
            for callee in &effects.edges {
                if direct.contains_key(callee) {
                    let mut next = chain.clone();
                    next.push(*callee);
                    queue.push_back((*callee, next));
                }
            }
        }
        let _ = facts;
    }
    None
}

/// Effet produit par une expression isolée (utile aux tests unitaires).
pub fn empty() -> EffectSet {
    EffectSet::default()
}
