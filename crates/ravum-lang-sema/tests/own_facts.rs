//! RFC-OWN-FACTS-1, step A — the checker records its ownership decisions.
//!
//! `own.rs` used to discard the decision it takes for every expression (move,
//! copy, borrow) and the backend re-derived it. This test drives the real
//! loader + checker on a fixture and asserts the recorded facts: a move, a
//! copy, a borrow, and a projected move with its place (`p.b`).
//!
//! Nothing consumes the facts yet (step A); this file is the contract that steps
//! B and C will compare against.

use ravum_lang_sema::facts::{OwnFacts, Projection, UseKind};
use ravum_lang_sema::loader::{load_package_with_options, LoadOptions};
use ravum_lang_sema::syntax::ast::{Block, Expr, ExprKind, ItemKind, Module, NodeId, StmtKind};
use ravum_lang_sema::syntax::source::SourceMap;
use ravum_lang_sema::syntax::span::Span;
use ravum_lang_sema::{check, CheckOptions, Profile};
use std::path::PathBuf;

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> TempDir {
        let path = std::env::temp_dir().join(format!(
            "ravum-own-facts-{}-{}-{label}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("src")).expect("cannot create the package");
        std::fs::write(
            path.join("ravum.toml"),
            "[package]\nname = \"ownfacts\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
        )
        .expect("cannot write the manifest");
        std::fs::write(
            path.join("src/main.rv"),
            r#"struct P { a: i32, b: String }

fn take(content: String) -> usize { return content.len(); }

fn len_of(content: &str) -> usize { return content.len(); }

fn main() {
    let text = String::from("abc");
    let n = take(text);
    let x = 1;
    let y = x;
    let p = P { a: 7, b: String::from("bee") };
    let b = p.b;
    let text2 = String::from("z");
    let m = len_of(text2.as_str());
    let s = "static";
    let t = s;
}
"#,
        )
        .expect("cannot write main.rv");
        TempDir { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn collect_exprs(module: &Module, out: &mut Vec<(NodeId, Span)>) {
    for item in &module.items {
        match &item.kind {
            ItemKind::Fn(function) => walk_block(&function.body, out),
            ItemKind::Impl(implementation) => {
                for member in &implementation.items {
                    if let ItemKind::Fn(function) = &member.kind {
                        walk_block(&function.body, out);
                    }
                }
            }
            ItemKind::Mod(inner) => collect_exprs(inner, out),
            _ => {}
        }
    }
}

fn walk_block(block: &Block, out: &mut Vec<(NodeId, Span)>) {
    for stmt in &block.stmts {
        match &stmt.kind {
            StmtKind::Let(let_stmt) => {
                if let Some(init) = &let_stmt.init {
                    walk_expr(init, out);
                }
            }
            StmtKind::Expr { expr, .. } => walk_expr(expr, out),
        }
    }
    if let Some(tail) = &block.tail {
        walk_expr(tail, out);
    }
}

fn walk_expr(expr: &Expr, out: &mut Vec<(NodeId, Span)>) {
    out.push((expr.id, expr.span));
    match &expr.kind {
        ExprKind::Call { callee, args } => {
            walk_expr(callee, out);
            for arg in args {
                walk_expr(arg, out);
            }
        }
        ExprKind::MethodCall { receiver, args, .. } => {
            walk_expr(receiver, out);
            for arg in args {
                walk_expr(arg, out);
            }
        }
        ExprKind::Field { base, .. } | ExprKind::TupleIndex { base, .. } => {
            walk_expr(base, out);
        }
        ExprKind::Index { base, index } => {
            walk_expr(base, out);
            walk_expr(index, out);
        }
        ExprKind::Unary { expr, .. } | ExprKind::Cast { expr, .. } | ExprKind::Try(expr) => {
            walk_expr(expr, out);
        }
        ExprKind::Binary { lhs, rhs, .. } => {
            walk_expr(lhs, out);
            walk_expr(rhs, out);
        }
        ExprKind::Assign { target, value } | ExprKind::AssignOp { target, value, .. } => {
            walk_expr(target, out);
            walk_expr(value, out);
        }
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => {
            walk_expr(cond, out);
            walk_block(then_block, out);
            if let Some(else_branch) = else_branch {
                walk_expr(else_branch, out);
            }
        }
        ExprKind::Match { scrutinee, arms } => {
            walk_expr(scrutinee, out);
            for arm in arms {
                walk_expr(&arm.body, out);
            }
        }
        ExprKind::While { cond, body } => {
            walk_expr(cond, out);
            walk_block(body, out);
        }
        ExprKind::Loop { body } => walk_block(body, out),
        ExprKind::For { iter, body, .. } => {
            walk_expr(iter, out);
            walk_block(body, out);
        }
        ExprKind::Block(block) | ExprKind::Unsafe(block) => walk_block(block, out),
        ExprKind::Tuple(items) | ExprKind::Array(items) => {
            for item in items {
                walk_expr(item, out);
            }
        }
        ExprKind::ArrayRepeat { value, len } => {
            walk_expr(value, out);
            walk_expr(len, out);
        }
        ExprKind::StructLit { fields, .. } => {
            for field in fields {
                walk_expr(&field.value, out);
            }
        }
        ExprKind::Range { start, end, .. } => {
            if let Some(start) = start {
                walk_expr(start, out);
            }
            if let Some(end) = end {
                walk_expr(end, out);
            }
        }
        ExprKind::Return(Some(value)) => walk_expr(value, out),
        _ => {}
    }
}

fn fixture() -> (OwnFacts, Vec<(NodeId, Span)>, SourceMap) {
    let package = TempDir::new("facts");
    let loaded = load_package_with_options(
        &package.path,
        &LoadOptions {
            locked: false,
            overrides: std::collections::BTreeMap::new(),
        },
    );
    assert!(
        loaded.diags.is_empty(),
        "loader diagnostics: {:?}",
        loaded
            .diags
            .iter()
            .map(|d| d.code.clone())
            .collect::<Vec<_>>()
    );
    let program = loaded.program.expect("no program");
    let result = check(
        &program,
        &loaded.sources,
        &CheckOptions {
            profile: Profile::Core,
            edition: ravum_lang_sema::syntax::LANGUAGE_EDITION.to_string(),
        },
    );
    let errors: Vec<String> = result
        .diags
        .iter()
        .filter(|diag| diag.code.starts_with("E-"))
        .map(|diag| diag.code.clone())
        .collect();
    assert!(errors.is_empty(), "checker errors: {errors:?}");
    let checked = result.checked.expect("no CheckedProgram");
    let mut exprs = Vec::new();
    collect_exprs(&checked.program.root, &mut exprs);
    (checked.facts.own, exprs, loaded.sources)
}

fn expr_id_by_text(exprs: &[(NodeId, Span)], sources: &SourceMap, text: &str) -> NodeId {
    let mut found: Vec<NodeId> = Vec::new();
    for (id, span) in exprs {
        let Some(file) = sources.file(span.file) else {
            continue;
        };
        let start = span.start as usize;
        let end = span.end as usize;
        if end <= file.text.len() && &file.text[start..end] == text {
            found.push(*id);
        }
    }
    assert_eq!(
        found.len(),
        1,
        "expected exactly one expression `{text}`, found {}: {found:?}",
        found.len()
    );
    found[0]
}

#[test]
fn own_facts_record_move_copy_borrow_and_places() {
    let (own, exprs, sources) = fixture();

    let text = expr_id_by_text(&exprs, &sources, "text");
    assert_eq!(
        own.use_kind.get(&text),
        Some(&UseKind::Move),
        "`take(text)` moves the String"
    );

    let x = expr_id_by_text(&exprs, &sources, "x");
    assert_eq!(
        own.use_kind.get(&x),
        Some(&UseKind::Copy),
        "`let y = x` copies an i32"
    );

    let s = expr_id_by_text(&exprs, &sources, "s");
    assert_eq!(
        own.use_kind.get(&s),
        Some(&UseKind::Copy),
        "`let t = s` copies a `&'static str`"
    );

    let receiver = expr_id_by_text(&exprs, &sources, "text2");
    assert_eq!(
        own.use_kind.get(&receiver),
        Some(&UseKind::Borrow),
        "a `&self` builtin receiver is borrowed"
    );

    let field = expr_id_by_text(&exprs, &sources, "p.b");
    assert_eq!(
        own.use_kind.get(&field),
        Some(&UseKind::Move),
        "`let b = p.b` moves a String field"
    );
    let place = own
        .place
        .get(&field)
        .expect("the moved place must be recorded");
    assert_eq!(
        place.steps,
        vec![Projection::Field("b".to_string())],
        "the place of `p.b` is the field projection of its binding"
    );
    assert!(
        !place.binding.is_dummy(),
        "the place must name its binding's declaration span"
    );

    // Every value use is a place: the backend will not need a second model.
    for id in own.use_kind.keys() {
        assert!(
            own.place.contains_key(id),
            "use without place for expression {id}"
        );
    }
}

#[test]
fn own_facts_are_deterministic() {
    let (first, _, _) = fixture();
    let (second, _, _) = fixture();
    assert_eq!(
        format!("{first:?}"),
        format!("{second:?}"),
        "two checks of the same source must record the same ownership facts"
    );
    assert!(
        !first.use_kind.is_empty(),
        "the fixture must produce facts (non-vacuity)"
    );
}
