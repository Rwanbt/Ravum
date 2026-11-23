use super::*;

/// Calcule `Facts::static_str_fns` par point fixe (monotone, borné par le
/// nombre de candidats).
pub(super) fn compute_static_str_fns(program: &ast::Program, facts: &mut Facts) {
    let mut candidates: Vec<(&FnItem, NodeId)> = Vec::new();
    collect_static_str_candidates(&program.root, facts, &mut candidates);
    facts.static_str_fns.clear();
    // T13 step 2: a reused body brings its previous status (its body was
    // not typed again, so it cannot be evaluated here).
    let reused: Vec<NodeId> = facts
        .reuse_bodies
        .iter()
        .filter(|(_, r)| r.static_str)
        .map(|(id, _)| *id)
        .collect();
    facts.static_str_fns.extend(reused);
    candidates.retain(|(_, id)| !facts.reuse_bodies.contains_key(id));
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

pub(super) fn collect_static_str_candidates<'a>(
    module: &'a Module,
    facts: &Facts,
    out: &mut Vec<(&'a FnItem, NodeId)>,
) {
    for item in &module.items {
        match &item.kind {
            ItemKind::Fn(f) if fn_sig_returns_static_str(item.id, f, facts) => {
                out.push((f, item.id));
            }
            ItemKind::Mod(inner) => collect_static_str_candidates(inner, facts, out),
            ItemKind::Impl(imp) => {
                for (sub, f) in imp.methods() {
                    if fn_sig_returns_static_str(sub.id, f, facts) {
                        out.push((f, sub.id));
                    }
                }
            }
            _ => {}
        }
    }
}

pub(super) fn fn_sig_returns_static_str(item: NodeId, f: &FnItem, facts: &Facts) -> bool {
    let ret = facts
        .fn_sigs
        .get(&item)
        .map(|sig| sig.ret.clone())
        .or_else(|| facts.type_nodes.get(&f.ret.id).cloned());
    matches!(ret, Some(ty) if is_static_str_type(&ty))
}

pub(super) fn is_static_str_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Ref {
            mutable: false,
            inner,
        } if matches!(**inner, Type::Str)
    )
}

/// `true` when every path returning from the function yields a static text.
pub(super) fn fn_returns_static_text(f: &FnItem, facts: &Facts) -> bool {
    let mut seen_return = false;
    // Names bound by `let` to a static `&str` in the current block, in order
    // (D13 rule 1: a copy of a static value is static). Soundness: `own.rs`
    // rejects any non-static `&str` assigned to a local it declared static
    // (`E-OWN-REF-ESCAPE`), and an uninitialised `&str` local is rejected
    // outright, so in an accepted program every assignment to such a local is
    // static too; the environment needs no may-analysis.
    let mut env: BTreeSet<String> = BTreeSet::new();
    if !block_returns_are_static(&f.body, facts, &mut seen_return, &mut env) {
        return false;
    }
    match &f.body.tail {
        Some(tail) => is_static_text_expr(tail, facts, &env),
        // No tail: a well-typed `&str` function must return explicitly on every
        // path; all those returns were checked above.
        None => seen_return,
    }
}

/// Checks every explicit `return` reachable in the block (including nested
/// blocks and expressions) and records that at least one exists. Collects the
/// static `&str` locals of the block into `env` as their `let` is reached.
pub(super) fn block_returns_are_static(
    block: &Block,
    facts: &Facts,
    seen: &mut bool,
    env: &mut BTreeSet<String>,
) -> bool {
    for stmt in &block.stmts {
        let ok = match &stmt.kind {
            StmtKind::Let(l) => match &l.init {
                Some(init) => {
                    if !expr_returns_are_static(init, facts, seen, env) {
                        return false;
                    }
                    if let PatKind::Binding { name, .. } = &l.pat.kind {
                        // Inferred `let` bindings may have no `pat_types` entry;
                        // fall back on the initialiser's type (same rule as
                        // `own::check_let`).
                        let is_text = is_borrowed_text_type(facts.pat_types.get(&l.pat.id))
                            || is_borrowed_text_type(facts.expr_types.get(&init.id));
                        if is_text && is_static_text_expr(init, facts, env) {
                            env.insert(name.name.clone());
                        } else {
                            env.remove(&name.name);
                        }
                    }
                    true
                }
                // `let x: &str;` is rejected by `own.rs` (an uninitialised
                // reference local escapes immediately), so only a shadowing
                // binding can be removed here.
                None => {
                    if let PatKind::Binding { name, .. } = &l.pat.kind {
                        env.remove(&name.name);
                    }
                    true
                }
            },
            StmtKind::Expr { expr, .. } => expr_returns_are_static(expr, facts, seen, env),
        };
        if !ok {
            return false;
        }
    }
    match &block.tail {
        Some(tail) => expr_returns_are_static(tail, facts, seen, env),
        None => true,
    }
}

pub(super) fn expr_returns_are_static(
    expr: &Expr,
    facts: &Facts,
    seen: &mut bool,
    env: &BTreeSet<String>,
) -> bool {
    match &expr.kind {
        ExprKind::Return(Some(value)) => {
            *seen = true;
            is_static_text_expr(value, facts, env)
                && expr_returns_are_static(value, facts, seen, env)
        }
        ExprKind::Return(None) => false,
        ExprKind::Call { callee, args } => {
            expr_returns_are_static(callee, facts, seen, env)
                && args
                    .iter()
                    .all(|arg| expr_returns_are_static(arg, facts, seen, env))
        }
        ExprKind::MethodCall { receiver, args, .. } => {
            expr_returns_are_static(receiver, facts, seen, env)
                && args
                    .iter()
                    .all(|arg| expr_returns_are_static(arg, facts, seen, env))
        }
        ExprKind::Field { base, .. } | ExprKind::TupleIndex { base, .. } => {
            expr_returns_are_static(base, facts, seen, env)
        }
        ExprKind::Index { base, index } => {
            expr_returns_are_static(base, facts, seen, env)
                && expr_returns_are_static(index, facts, seen, env)
        }
        ExprKind::Unary { expr, .. }
        | ExprKind::Cast { expr, .. }
        | ExprKind::Try(expr)
        | ExprKind::Await(expr) => expr_returns_are_static(expr, facts, seen, env),
        ExprKind::Binary { lhs, rhs, .. } => {
            expr_returns_are_static(lhs, facts, seen, env)
                && expr_returns_are_static(rhs, facts, seen, env)
        }
        ExprKind::Assign { target, value } | ExprKind::AssignOp { target, value, .. } => {
            expr_returns_are_static(target, facts, seen, env)
                && expr_returns_are_static(value, facts, seen, env)
        }
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => {
            expr_returns_are_static(cond, facts, seen, env)
                && block_returns_are_static(then_block, facts, seen, &mut env.clone())
                && else_branch
                    .as_ref()
                    .map(|branch| expr_returns_are_static(branch, facts, seen, env))
                    .unwrap_or(true)
        }
        ExprKind::Match { scrutinee, arms } => {
            expr_returns_are_static(scrutinee, facts, seen, env)
                && arms.iter().all(|arm| {
                    let mut arm_env = env.clone();
                    collect_static_text_pat_bindings(
                        &arm.pat,
                        facts,
                        false,
                        is_static_text_expr(scrutinee, facts, env),
                        &mut arm_env,
                    );
                    expr_returns_are_static(&arm.body, facts, seen, &arm_env)
                })
        }
        ExprKind::While { cond, body } => {
            expr_returns_are_static(cond, facts, seen, env)
                && block_returns_are_static(body, facts, seen, &mut env.clone())
        }
        ExprKind::Loop { body } => block_returns_are_static(body, facts, seen, &mut env.clone()),
        ExprKind::For { pat, iter, body } => {
            let mut body_env = env.clone();
            // A `for` item of type `&str` is read out of the iterable, hence
            // static by rule 2 (same decision as `own.rs`).
            collect_static_text_pat_bindings(pat, facts, true, false, &mut body_env);
            expr_returns_are_static(iter, facts, seen, env)
                && block_returns_are_static(body, facts, seen, &mut body_env)
        }
        ExprKind::Block(block) | ExprKind::Unsafe(block) => {
            block_returns_are_static(block, facts, seen, &mut env.clone())
        }
        ExprKind::Tuple(items) | ExprKind::Array(items) => items
            .iter()
            .all(|item| expr_returns_are_static(item, facts, seen, env)),
        ExprKind::ArrayRepeat { value, len } => {
            expr_returns_are_static(value, facts, seen, env)
                && expr_returns_are_static(len, facts, seen, env)
        }
        ExprKind::StructLit { fields, .. } => fields
            .iter()
            .all(|field| expr_returns_are_static(&field.value, facts, seen, env)),
        ExprKind::Range { start, end, .. } => start
            .iter()
            .chain(end.iter())
            .all(|bound| expr_returns_are_static(bound, facts, seen, env)),
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

/// Add the `&str` bindings a pattern extracts to `env` (D13 rule 1).
/// `nested` is `true` once the pattern destructures a *component* of the value
/// (static by rule 2); a top-level binding copies the value itself, so it is
/// static only when the matched expression is (`top_static`).
pub(super) fn collect_static_text_pat_bindings(
    pat: &Pat,
    facts: &Facts,
    nested: bool,
    top_static: bool,
    env: &mut BTreeSet<String>,
) {
    match &pat.kind {
        PatKind::Binding { name, .. } => {
            if is_borrowed_text_type(facts.pat_types.get(&pat.id)) && (nested || top_static) {
                env.insert(name.name.clone());
            } else {
                // The binding shadows an outer name; it is not a static text, so
                // the outer fact must not leak into the arm/body.
                env.remove(&name.name);
            }
        }
        PatKind::Tuple(items) => {
            for item in items {
                collect_static_text_pat_bindings(item, facts, true, top_static, env);
            }
        }
        PatKind::Variant { fields, .. } => match fields {
            ravum_lang_syntax::ast::VariantPatFields::Tuple(items) => {
                for item in items {
                    collect_static_text_pat_bindings(item, facts, true, top_static, env);
                }
            }
            ravum_lang_syntax::ast::VariantPatFields::Struct(fields) => {
                for f in fields {
                    collect_static_text_pat_bindings(&f.pat, facts, true, top_static, env);
                }
            }
        },
        PatKind::Wild | PatKind::Lit(_) | PatKind::Path(_) => {}
    }
}

/// `true` when the expression certainly evaluates to a `'static` text value.
pub(super) fn is_static_text_expr(expr: &Expr, facts: &Facts, env: &BTreeSet<String>) -> bool {
    match &expr.kind {
        ExprKind::Str(_) => true,
        // A local `let` bound to a static text is a copy, hence static
        // (D13 rule 1; see `fn_returns_static_text` for the soundness note).
        // Local paths are not in `facts.resolutions` (only module-level value
        // paths are), so the syntactic single-segment name is used.
        ExprKind::Path(path) => path_local_name(path)
            .map(|name| env.contains(name))
            .unwrap_or(false),
        // Interprocedural propagation: only user functions classified by the
        // fixpoint; never a builtin (`as_str` and friends must stay non-static).
        ExprKind::Call { .. } => matches!(
            facts.calls.get(&expr.id),
            Some(CallTarget::UserFn { item }) if facts.static_str_fns.contains(item)
        ),
        // D13 rule 1 — projection preserves provenance. A `&str` read out of an
        // aggregate is static by construction because OWN-6.1/3.3 forbid storing
        // a borrowed `&str` in one (every storage site is checked by `own.rs`).
        ExprKind::Field { .. } | ExprKind::TupleIndex { .. } | ExprKind::Index { .. } => {
            is_borrowed_text_type(facts.expr_types.get(&expr.id))
        }
        // D13 rule 1 — an `Option`/`Result` payload is read out of an aggregate;
        // `str::as_str` is the identity on a text reference. `String::as_str`
        // stays refused (OWN-6.3).
        ExprKind::MethodCall { receiver, args, .. } => match facts.methods.get(&expr.id) {
            Some(MethodTarget::Builtin { path, .. }) if path.as_str() == "str::as_str" => {
                is_static_text_expr(receiver, facts, env)
            }
            Some(MethodTarget::Builtin { path, .. })
                if is_borrowed_text_type(facts.expr_types.get(&expr.id)) =>
            {
                match path.as_str() {
                    "Option::unwrap" | "Result::unwrap" => true,
                    "Option::unwrap_or" | "Result::unwrap_or" => args
                        .first()
                        .map(|default| is_static_text_expr(default, facts, env))
                        .unwrap_or(false),
                    _ => false,
                }
            }
            Some(MethodTarget::User { item }) => facts.static_str_fns.contains(item),
            _ => false,
        },
        ExprKind::If {
            then_block,
            else_branch: Some(else_branch),
            ..
        } => {
            block_value_is_static(then_block, facts, env)
                && is_static_text_expr(else_branch, facts, env)
        }
        ExprKind::Match { scrutinee, arms } => {
            !arms.is_empty()
                && arms.iter().all(|arm| {
                    let mut arm_env = env.clone();
                    collect_static_text_pat_bindings(
                        &arm.pat,
                        facts,
                        false,
                        is_static_text_expr(scrutinee, facts, env),
                        &mut arm_env,
                    );
                    is_static_text_expr(&arm.body, facts, &arm_env)
                })
        }
        ExprKind::Block(block) | ExprKind::Unsafe(block) => {
            block_value_is_static(block, facts, env)
        }
        // A diverging branch that returns a static text contributes a static
        // value on every path that actually produces one.
        ExprKind::Return(Some(value)) => is_static_text_expr(value, facts, env),
        _ => false,
    }
}

/// `true` for the borrowed-text type `&str` (mirror of `own::is_borrowed_text`).
pub(super) fn is_borrowed_text_type(ty: Option<&Type>) -> bool {
    matches!(
        ty,
        Some(Type::Ref {
            mutable: false,
            inner,
        }) if matches!(**inner, Type::Str)
    )
}

/// The name of a plain single-segment path (`s`), i.e. a local binding, as
/// opposed to a qualified item path (`m::f`).
pub(super) fn path_local_name(path: &Path) -> Option<&str> {
    if path.root == PathRoot::Plain && path.segments.len() == 1 {
        Some(path.segments[0].name.name.as_str())
    } else {
        None
    }
}

/// The value of a block is its tail expression, read in the environment its
/// `let` statements build; a block with no tail produces `()` (never a static
/// text). Conservative by design.
pub(super) fn block_value_is_static(block: &Block, facts: &Facts, env: &BTreeSet<String>) -> bool {
    let mut inner = env.clone();
    let mut seen = false;
    if !block_returns_are_static(block, facts, &mut seen, &mut inner) {
        // A non-static `return` inside the block breaks the function contract;
        // the block value cannot be trusted as static either.
        return false;
    }
    block
        .tail
        .as_ref()
        .map(|tail| is_static_text_expr(tail, facts, &inner))
        .unwrap_or(false)
}
