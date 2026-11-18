//! Ownership facts of the hand-assembled fixtures.

use super::super::*;
use ravum_lang_sema::facts::CheckedProgram;

/// A hand-assembled fixture rarely records the type of its `let` bindings.
/// `typeck` always does (annotation, else initialiser), and the ownership
/// checker reads it to decide move versus copy: record it the same way.
fn fill_let_binding_types(block: &Block, facts: &mut ravum_lang_sema::facts::Facts) {
    for statement in &block.stmts {
        let StmtKind::Let(declaration) = &statement.kind else {
            continue;
        };
        if !matches!(declaration.pat.kind, PatKind::Binding { .. })
            || facts.pat_types.contains_key(&declaration.pat.id)
        {
            continue;
        }
        let annotated = declaration
            .ty
            .as_ref()
            .and_then(|annotation| facts.type_nodes.get(&annotation.id).cloned());
        let inferred = declaration
            .init
            .as_ref()
            .and_then(|init| facts.expr_types.get(&init.id).cloned());
        if let Some(ty) = annotated.or(inferred) {
            facts.pat_types.insert(declaration.pat.id, ty);
        }
    }
}

/// Every block of a module: function bodies and the blocks nested in them.
fn collect_blocks<'a>(module: &'a Module, out: &mut Vec<&'a Block>) {
    fn in_block<'a>(block: &'a Block, out: &mut Vec<&'a Block>) {
        out.push(block);
        for statement in &block.stmts {
            match &statement.kind {
                StmtKind::Let(declaration) => {
                    if let Some(init) = &declaration.init {
                        in_expr(init, out);
                    }
                }
                StmtKind::Expr { expr, .. } => in_expr(expr, out),
            }
        }
        if let Some(tail) = &block.tail {
            in_expr(tail, out);
        }
    }
    fn in_expr<'a>(expr: &'a Expr, out: &mut Vec<&'a Block>) {
        match &expr.kind {
            ExprKind::Block(block) | ExprKind::Unsafe(block) | ExprKind::Loop { body: block } => {
                in_block(block, out);
            }
            ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => {
                in_expr(cond, out);
                in_block(then_block, out);
                if let Some(else_branch) = else_branch {
                    in_expr(else_branch, out);
                }
            }
            ExprKind::While { cond, body } => {
                in_expr(cond, out);
                in_block(body, out);
            }
            ExprKind::For { iter, body, .. } => {
                in_expr(iter, out);
                in_block(body, out);
            }
            ExprKind::Match { scrutinee, arms } => {
                in_expr(scrutinee, out);
                for arm in arms {
                    in_expr(&arm.body, out);
                }
            }
            _ => {}
        }
    }
    for item in &module.items {
        match &item.kind {
            ItemKind::Fn(function) => in_block(&function.body, out),
            ItemKind::Impl(implementation) => {
                for member in &implementation.items {
                    if let ItemKind::Fn(function) = &member.kind {
                        in_block(&function.body, out);
                    }
                }
            }
            ItemKind::Mod(inner) => collect_blocks(inner, out),
            _ => {}
        }
    }
}

/// RFC-OWN-FACTS-2: a hand-assembled fixture publishes the ownership facts the
/// way production does — by running the real checker on it — so the step-B
/// guard compares like with like instead of passing vacuously on empty tables.
pub(super) fn publish_ownership_facts(checked: &mut CheckedProgram) {
    let mut blocks: Vec<&Block> = Vec::new();
    collect_blocks(&checked.program.root, &mut blocks);
    for block in blocks {
        fill_let_binding_types(block, &mut checked.facts);
    }
    let mut diagnostics = ravum_lang_sema::syntax::diag::DiagnosticBag::new();
    checked.facts.own = ravum_lang_sema::check_ownership(checked, &mut diagnostics);
}
