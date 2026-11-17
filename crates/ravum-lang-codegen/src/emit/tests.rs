use super::*;
use crate::GenOptions;
use ravum_lang_sema::facts::{
    CallTarget, CheckedProgram, ConstInfo, ConstValue, Facts, FieldInfo, FileInfo, MethodTarget,
    Profile, Resolution, StructInfo,
};
use ravum_lang_sema::types::Type as STy;
use ravum_lang_syntax::ast::{
    BinOp, Block, Expr, ExprKind, FieldDef, FieldInit, FnItem, ImplItem, Item, ItemKind, LetStmt,
    Module, Param, ParamKind, Pat, PatKind, Path, PathRoot, PathSegment, Program, Receiver, Stmt,
    StmtKind, StructItem, Type as AstType, TypeKind, UnOp, VariantDefFields, VariantPatFields, Vis,
};
use ravum_lang_syntax::span::Span;

fn span() -> Span {
    Span::new(0, 0, 3)
}

// -- minimal hand-built AST/Facts ----------------------------------------

struct Builder {
    next: NodeId,
    items: Vec<Item>,
    facts: Facts,
    files: Vec<FileInfo>,
}

impl Builder {
    fn new() -> Builder {
        Builder {
            next: 1,
            items: Vec::new(),
            facts: Facts::default(),
            files: vec![FileInfo {
                path: "main.rv".to_string(),
                line_starts: vec![0, 10, 20, 40, 80],
            }],
        }
    }

    fn id(&mut self) -> NodeId {
        let id = self.next;
        self.next += 1;
        id
    }

    fn ident(&mut self, name: &str) -> ravum_lang_syntax::ast::Ident {
        ravum_lang_syntax::ast::Ident::new(name, span())
    }

    fn path(&mut self, names: &[&str]) -> Path {
        let id = self.id();
        let mut segments = Vec::new();
        for name in names {
            let ident = self.ident(name);
            segments.push(PathSegment {
                name: ident,
                args: Vec::new(),
            });
        }
        Path {
            id,
            root: PathRoot::Plain,
            segments,
            span: span(),
        }
    }

    fn expr(&mut self, kind: ExprKind, ty: Option<STy>) -> Expr {
        let id = self.id();
        if let Some(ty) = ty {
            self.facts.expr_types.insert(id, ty);
        }
        if matches!(kind, ExprKind::SelfValue) {
            // As `typeck` does (`check_self_value`): `self` resolves to a local.
            self.facts.resolutions.insert(
                id,
                Resolution::Local {
                    name: "self".to_string(),
                    mutable: false,
                    decl: span(),
                },
            );
        }
        Expr {
            id,
            kind,
            span: span(),
        }
    }

    fn pat(&mut self, kind: PatKind, ty: Option<STy>) -> Pat {
        let id = self.id();
        if let Some(ty) = ty {
            self.facts.pat_types.insert(id, ty);
        }
        Pat {
            id,
            kind,
            span: span(),
        }
    }

    fn ty(&mut self, kind: TypeKind, resolved: Option<STy>) -> AstType {
        let id = self.id();
        if let Some(resolved) = resolved {
            self.facts.type_nodes.insert(id, resolved);
        }
        AstType {
            id,
            kind,
            span: span(),
        }
    }

    fn named_type(&mut self, name: &str, resolved: Option<STy>) -> AstType {
        let path = self.path(&[name]);
        self.ty(TypeKind::Path(path), resolved)
    }

    fn block(&mut self, stmts: Vec<Stmt>, tail: Option<Expr>) -> Block {
        Block {
            id: self.id(),
            stmts,
            tail: tail.map(Box::new),
            span: span(),
        }
    }

    fn var(&mut self, name: &str, ty: Option<STy>) -> Expr {
        let path = self.path(&[name]);
        self.facts.resolutions.insert(
            path.id,
            Resolution::Local {
                name: name.to_string(),
                mutable: false,
                decl: span(),
            },
        );
        self.expr(ExprKind::Path(path), ty)
    }

    fn int(&mut self, value: u128, ty: Option<STy>) -> Expr {
        self.expr(
            ExprKind::Int {
                value,
                suffix: None,
            },
            ty,
        )
    }

    fn text(&mut self, value: &str) -> Expr {
        self.expr(ExprKind::Str(value.to_string()), None)
    }

    fn unit(&mut self) -> Expr {
        self.expr(ExprKind::Unit, None)
    }

    fn callee(&mut self, name: &str) -> Expr {
        let path = self.path(&[name]);
        self.expr(ExprKind::Path(path), None)
    }

    fn call(&mut self, callee: Expr, args: Vec<Expr>, target: CallTarget) -> Expr {
        let id = self.id();
        self.facts.calls.insert(id, target);
        Expr {
            id,
            kind: ExprKind::Call {
                callee: Box::new(callee),
                args,
            },
            span: span(),
        }
    }

    fn method(
        &mut self,
        receiver: Expr,
        name: &str,
        args: Vec<Expr>,
        target: MethodTarget,
    ) -> Expr {
        let method = self.ident(name);
        let id = self.id();
        self.facts.methods.insert(id, target);
        Expr {
            id,
            kind: ExprKind::MethodCall {
                receiver: Box::new(receiver),
                method,
                args,
            },
            span: span(),
        }
    }

    fn let_stmt(&mut self, name: &str, ty: Option<AstType>, init: Expr) -> Stmt {
        let ident = self.ident(name);
        let pattern = self.pat(
            PatKind::Binding {
                mutable: false,
                name: ident,
            },
            None,
        );
        Stmt {
            id: self.id(),
            kind: StmtKind::Let(LetStmt {
                pat: pattern,
                ty,
                init: Some(init),
            }),
            span: span(),
        }
    }

    fn expr_stmt(&mut self, expr: Expr) -> Stmt {
        Stmt {
            id: self.id(),
            kind: StmtKind::Expr { expr, semi: true },
            span: span(),
        }
    }

    fn finish(mut self, main: Option<NodeId>, tests: Vec<NodeId>) -> CheckedProgram {
        let root_id = self.id();
        self.facts.main_fn = main;
        self.facts.test_fns = tests;
        let program = Program {
            file: 0,
            root: Module {
                id: root_id,
                name: None,
                items: self.items,
                span: span(),
            },
        };
        let mut checked = CheckedProgram {
            program,
            facts: self.facts,
            files: self.files,
            profile: Profile::Core,
        };
        fixture_facts::publish_ownership_facts(&mut checked);
        checked
    }
}

fn make_param(builder: &mut Builder, name: &str, ty: AstType) -> Param {
    let id = builder.id();
    Param {
        id,
        kind: ParamKind::Binding {
            mutable: false,
            name: builder.ident(name),
            ty,
        },
        span: span(),
    }
}

fn make_receiver(builder: &mut Builder, receiver: Receiver) -> Param {
    let id = builder.id();
    Param {
        id,
        kind: ParamKind::Receiver(receiver),
        span: span(),
    }
}

// Mirrors the AST constructor one-for-one; grouping these test-only fields
// into a struct would make every fixture harder to read without changing
// the code under test.
#[allow(clippy::too_many_arguments)]
fn make_fn(
    builder: &mut Builder,
    id: NodeId,
    name: &str,
    visibility: Vis,
    is_test: bool,
    params: Vec<Param>,
    ret: AstType,
    body: Block,
) -> Item {
    let name = builder.ident(name);
    Item {
        id,
        vis: visibility,
        kind: ItemKind::Fn(FnItem {
            name,
            generics: Vec::new(),
            params,
            ret,
            effects: None,
            body,
            is_test,
            span: span(),
        }),
        span: span(),
    }
}

/// Step B non-vacuity for places: corrupting a recorded projection must be
/// rejected in debug builds.
#[test]
#[cfg(debug_assertions)]
fn place_facts_disagreement_is_rejected_in_debug_builds() {
    use ravum_lang_sema::facts::{CheckOptions, Projection};
    use ravum_lang_sema::syntax::{parser, source::SourceMap};

    let source = "fn main() { let a = [1, 2, 3]; let x = a[1]; }";
    let mut sources = SourceMap::new();
    let file = sources.add("main.rv", source);
    let parsed = parser::parse_file(file, source);
    let result = ravum_lang_sema::check(&parsed.program, &sources, &CheckOptions::default());
    let mut checked = result.checked.expect("the fixture program must check");
    let id = *checked
        .facts
        .own
        .place
        .iter()
        .find(|(_, place)| matches!(place.steps.first(), Some(Projection::Index(_))))
        .map(|(id, _)| id)
        .expect("the fixture must record the index place `a[1]`");
    checked
        .facts
        .own
        .place
        .get_mut(&id)
        .expect("place exists")
        .steps
        .push(Projection::Deref);
    let error = crate::generate(&checked, &GenOptions::default())
        .expect_err("a corrupted place must be rejected by the step-B place guard");
    assert!(
        error.message.contains("syntactic place"),
        "unexpected error: {}",
        error.message
    );
}

/// Step B non-vacuity for method receivers: the guard now verifies the
/// auto-deref-adjusted place instead of skipping the receiver.
#[test]
#[cfg(debug_assertions)]
fn method_receiver_place_disagreement_is_rejected_in_debug_builds() {
    use ravum_lang_sema::facts::{CheckOptions, Projection};
    use ravum_lang_sema::syntax::{parser, source::SourceMap};

    let source = "fn first(xs: &[i32]) -> usize { return xs.len(); }\n\
                      fn main() { let a = [1, 2, 3]; let n = first(&a); }";
    let mut sources = SourceMap::new();
    let file = sources.add("main.rv", source);
    let parsed = parser::parse_file(file, source);
    let result = ravum_lang_sema::check(&parsed.program, &sources, &CheckOptions::default());
    let mut checked = result.checked.expect("the fixture program must check");
    let mut exprs: BTreeMap<NodeId, &Expr> = BTreeMap::new();
    let mut receivers: BTreeSet<NodeId> = BTreeSet::new();
    collect_module_exprs(&checked.program.root, &mut exprs, &mut receivers);
    let id = *receivers
        .iter()
        .next()
        .expect("the fixture must contain a method receiver");
    let place = checked
        .facts
        .own
        .place
        .get_mut(&id)
        .expect("the receiver's place must be recorded");
    assert_eq!(
        place.steps,
        vec![Projection::Deref],
        "the fixture must exercise the implicit auto-deref of `xs.len()`"
    );
    place.steps.push(Projection::Field("nope".to_string()));
    let error = crate::generate(&checked, &GenOptions::default())
        .expect_err("a corrupted receiver place must be rejected by the step-B guard");
    assert!(
        error.message.contains("syntactic place"),
        "unexpected error: {}",
        error.message
    );
}

/// Step B non-vacuity for reference arguments: a reborrowed `&[i32]`
/// argument records an auto-deref place and the guard now verifies it
/// instead of skipping the argument.
#[test]
#[cfg(debug_assertions)]
fn reference_argument_place_disagreement_is_rejected_in_debug_builds() {
    use ravum_lang_sema::facts::{CheckOptions, Projection};
    use ravum_lang_sema::syntax::{parser, source::SourceMap};

    let source = "fn first(xs: &[i32]) -> i32 { return xs[0]; }\n\
                      fn second(ys: &[i32]) -> i32 { return first(ys); }\n\
                      fn main() { let a = [1, 2, 3]; let v = second(&a); }";
    let mut sources = SourceMap::new();
    let file = sources.add("main.rv", source);
    let parsed = parser::parse_file(file, source);
    let result = ravum_lang_sema::check(&parsed.program, &sources, &CheckOptions::default());
    let mut checked = result.checked.expect("the fixture program must check");
    let mut exprs: BTreeMap<NodeId, &Expr> = BTreeMap::new();
    let mut receivers: BTreeSet<NodeId> = BTreeSet::new();
    collect_module_exprs(&checked.program.root, &mut exprs, &mut receivers);
    let id = *exprs
        .iter()
        .find(|(id, expr)| {
            matches!(
                &expr.kind,
                ExprKind::Path(path)
                    if path.segments.len() == 1 && path.segments[0].name.name == "ys"
            ) && checked
                .facts
                .own
                .place
                .get(id)
                .is_some_and(|place| place.steps == vec![Projection::Deref])
        })
        .map(|(id, _)| id)
        .expect("the fixture must record `ys` with its implicit auto-deref");
    let place = checked
        .facts
        .own
        .place
        .get_mut(&id)
        .expect("the argument's place must be recorded");
    place.steps.push(Projection::Field("nope".to_string()));
    let error = crate::generate(&checked, &GenOptions::default())
        .expect_err("a corrupted reference-argument place must be rejected by the step-B guard");
    assert!(
        error.message.contains("syntactic place"),
        "unexpected error: {}",
        error.message
    );
}

/// Step B non-vacuity for the binding identity: the guard compares the
/// declaration span recorded by `OwnFacts` with the resolved local's span.
#[test]
#[cfg(debug_assertions)]
fn binding_identity_disagreement_is_rejected_in_debug_builds() {
    use ravum_lang_sema::facts::CheckOptions;
    use ravum_lang_sema::syntax::span::Span;
    use ravum_lang_sema::syntax::{parser, source::SourceMap};

    let source = "fn main() { let a: i32 = 1; let b = a; }";
    let mut sources = SourceMap::new();
    let file = sources.add("main.rv", source);
    let parsed = parser::parse_file(file, source);
    let result = ravum_lang_sema::check(&parsed.program, &sources, &CheckOptions::default());
    let mut checked = result.checked.expect("the fixture program must check");
    let mut exprs: BTreeMap<NodeId, &Expr> = BTreeMap::new();
    let mut receivers: BTreeSet<NodeId> = BTreeSet::new();
    collect_module_exprs(&checked.program.root, &mut exprs, &mut receivers);
    let id = *exprs
        .iter()
        .find(|(id, expr)| {
            matches!(
                &expr.kind,
                ExprKind::Path(path)
                    if path.segments.len() == 1 && path.segments[0].name.name == "a"
            ) && checked.facts.own.place.contains_key(id)
        })
        .map(|(id, _)| id)
        .expect("the fixture must record the place of `a`");
    checked
        .facts
        .own
        .place
        .get_mut(&id)
        .expect("place exists")
        .binding = Span::dummy();
    let error = crate::generate(&checked, &GenOptions::default())
        .expect_err("a corrupted binding must be rejected by the step-B guard");
    assert!(
        error.message.contains("binding"),
        "unexpected error: {}",
        error.message
    );
}

/// Step B non-vacuity: a disagreement between `OwnFacts` and the backend's
/// classification must be an error in debug builds, not a silent lowering.
#[test]
#[cfg(debug_assertions)]
fn ownership_facts_disagreement_is_rejected_in_debug_builds() {
    let mut checked = build_program_d();
    let id = 999_999;
    checked.facts.expr_types.insert(id, STy::String);
    checked
        .facts
        .own
        .use_kind
        .insert(id, ravum_lang_sema::facts::UseKind::Copy);
    let error = crate::generate(&checked, &GenOptions::default())
        .expect_err("a Copy decision on a String must be rejected by the step-B guard");
    assert!(
        error.message.contains("ownership facts disagree"),
        "unexpected error: {}",
        error.message
    );
}

/// Step B non-vacuity, second direction: a Move decision on a **Copy** type
/// must also be rejected. The previous test only covers Copy-on-mobile; the
/// 2026-11-14 mutation campaign found that the `UseKind::Move if copies`
/// guard had no test, so replacing its `copies` condition with `false`
/// survived.
#[test]
#[cfg(debug_assertions)]
fn ownership_facts_move_on_a_copy_type_is_rejected_in_debug_builds() {
    let mut checked = build_program_d();
    let id = 999_998;
    checked.facts.expr_types.insert(id, STy::I32);
    checked
        .facts
        .own
        .use_kind
        .insert(id, ravum_lang_sema::facts::UseKind::Move);
    let error = crate::generate(&checked, &GenOptions::default())
        .expect_err("a Move decision on i32 must be rejected by the step-B guard");
    assert!(
        error.message.contains("ownership facts disagree"),
        "unexpected error: {}",
        error.message
    );
}

fn generate_ok(checked: &CheckedProgram, options: &GenOptions) -> String {
    crate::generate(checked, options).expect("code generation must succeed")
}

mod emitter_type_contracts;
mod emitter_value_contracts;
mod fixture_facts;
mod name_contracts;
mod own_facts_guard;
mod path_walk_contracts;
mod pattern_contracts;
mod programs_abcd;
mod programs_e;

use programs_abcd::*;
use programs_e::*;

// -- structural tests ---------------------------------------------------

#[test]
fn banner_modules_and_entry_point() {
    let program = build_program_a();
    let code = generate_ok(&program.checked, &GenOptions::default());
    assert!(
        code.starts_with(&format!(
            "// Generated by ravum-lang {} from ravum_program; do not edit.",
            VERSION
        )),
        "unexpected banner:\n{}",
        &code[..code.len().min(200)]
    );
    assert!(code.contains("#![forbid(unsafe_code)]"));
    assert!(code.contains("pub mod rvm_root {"));
    assert!(code.contains("pub fn rv_user_main() -> Result<(), crate::Error>"));
    assert!(code.contains("fn main() -> std::process::ExitCode"));
    assert!(code.contains("crate::rv_install_panic_hook();"));
    assert!(code.contains("eprintln!(\"error: {}\", rv_error.message);"));
    assert!(code.contains("crate::rvm_root::add(2i32, 3i32)"));
}

#[test]
fn structs_impls_and_builtins_are_mapped() {
    let program = build_program_a();
    let code = generate_ok(&program.checked, &GenOptions::default());
    assert!(code.contains("#[derive(Debug, Clone, PartialEq, Copy)]"));
    assert!(code.contains("pub struct Point {"));
    assert!(code.contains("pub x: i32,"));
    assert!(code.contains("impl crate::rvm_root::Point {"));
    assert!(code.contains("fn sum(&self) -> i32"));
    assert!(code.contains("crate::rv_println(\"hello\");"));
    assert!(code.contains("crate::rvm_root::Point { x: 1i32, y: 2i32 }"));
    assert!(code.contains("p.sum()"));
    assert!(code.contains("format!(\"{}\", s)"));
    assert!(code.contains("crate::rv_println(&t);"));
    assert!(code.contains("Ok(())"));
    let _ = &program.sum_id;
}

#[test]
fn no_unsafe_constructs_are_generated() {
    let corpus = [
        generate_ok(&build_program_a().checked, &GenOptions::default()),
        generate_ok(&build_program_b(), &GenOptions::default()),
        generate_ok(&build_program_c(), &GenOptions::default()),
        generate_ok(&build_program_d(), &GenOptions::default()),
    ];
    for code in &corpus {
        for forbidden in [
            "unsafe fn",
            "unsafe {",
            "transmute(",
            "*const ",
            "*mut ",
            "as *const",
            "as *mut",
            "wrapping_add(",
            "wrapping_sub(",
            "wrapping_mul(",
        ] {
            assert!(
                !code.contains(forbidden),
                "generated code must not contain `{forbidden}`"
            );
        }
        assert!(!code.contains("std::ptr"));
    }
}

#[test]
fn generation_is_deterministic() {
    let program = build_program_a();
    let options = GenOptions::default();
    let first = generate_ok(&program.checked, &options);
    let second = generate_ok(&program.checked, &options);
    assert_eq!(first, second);
    let third = crate::generate(&program.checked, &options).expect("generation");
    assert_eq!(first, third);
}

#[test]
fn source_markers_follow_the_option() {
    let program = build_program_a();
    let with_markers = generate_ok(&program.checked, &GenOptions::default());
    assert!(with_markers.contains("// rv: main.rv:1"));

    let options = GenOptions {
        source_comments: false,
        ..Default::default()
    };
    let without_markers = generate_ok(&program.checked, &options);
    assert!(!without_markers.contains("// rv:"));
}

#[test]
fn operator_precedence_is_explicit() {
    let code = generate_ok(&build_program_d(), &GenOptions::default());
    assert!(
        code.contains("a + (b * c)"),
        "expected explicit parentheses in:\n{code}"
    );
}

#[test]
fn test_mode_generates_a_runner() {
    let options = GenOptions {
        test_mode: true,
        ..Default::default()
    };
    let code = generate_ok(&build_program_b(), &options);
    assert!(code.contains("crate::rv_install_test_panic_hook();"));
    assert!(code.contains("crate::rv_test(\"passes\""));
    assert!(code.contains("crate::rv_test(\"fails\""));
    // `--filter` forwarding contract (`args.rs`) and the rustc-compatible
    // summary line are emitted by the runner itself.
    assert!(code.contains("strip_prefix(\"--filter=\")"));
    assert!(code.contains("rv_argument == \"--filter\""));
    assert!(code.contains("println!(\"running {} tests\", rv_selected);"));
    assert!(code.contains(
        "println!(\"test result: {}; {} passed; {} failed; {} ignored; {} filtered out\","
    ));
    // The user `main` is not called in test mode.
    assert!(!code.contains("rv_user_main()"));
}

#[test]
fn typed_literals_use_the_inferred_integer_type() {
    let mut b = Builder::new();
    let value_id = b.id();
    let literal = b.int(5, Some(STy::I64));
    let body = b.block(Vec::new(), Some(literal));
    let ret = b.named_type("i64", Some(STy::I64));
    let item = make_fn(
        &mut b,
        value_id,
        "value",
        Vis::Public(span()),
        false,
        Vec::new(),
        ret,
        body,
    );
    b.items.push(item);
    let program = b.finish(None, Vec::new());
    let code = generate_ok(&program, &GenOptions::default());
    assert!(code.contains("5i64"), "expected a typed literal:\n{code}");
}

// -- explicit failures --------------------------------------------------

#[test]
fn missing_call_fact_is_an_explicit_error() {
    let program = build_program_a();
    let mut facts_only = program.checked.clone();
    facts_only.facts.calls.remove(&program.println_call_id);
    let error = crate::generate(&facts_only, &GenOptions::default())
        .expect_err("missing call fact must fail");
    assert!(
        error.message.contains("missing call fact"),
        "unexpected error: {}",
        error.message
    );
}

#[test]
fn unknown_builtin_is_an_explicit_error() {
    let program = build_program_a();
    let mut facts_only = program.checked.clone();
    facts_only.facts.calls.insert(
        program.println_call_id,
        CallTarget::BuiltinFn {
            path: "io::bogus".to_string(),
        },
    );
    let error = crate::generate(&facts_only, &GenOptions::default())
        .expect_err("unknown builtin must fail");
    assert!(
        error.message.contains("unsupported builtin function"),
        "unexpected error: {}",
        error.message
    );
}

#[test]
fn unknown_nominal_type_is_an_explicit_error() {
    let mut program = build_program_a();
    // Drop every resolved type node of the `Point` annotation.
    let ids: Vec<NodeId> = program.checked.facts.type_nodes.keys().copied().collect();
    for id in ids {
        program.checked.facts.type_nodes.remove(&id);
    }
    let error = crate::generate(&program.checked, &GenOptions::default())
        .expect_err("unresolved nominal type must fail");
    assert!(
        error.message.contains("missing resolved type")
            || error.message.contains("cannot resolve nominal type"),
        "unexpected error: {}",
        error.message
    );
    let _ = program.point_id;
}

// -- rustc-backed tests -------------------------------------------------

fn rustc_available() -> bool {
    match std::process::Command::new("rustc")
        .arg("--version")
        .output()
    {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

struct TempDir {
    path: std::path::PathBuf,
}

impl TempDir {
    fn new(label: &str) -> TempDir {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.subsec_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "ravum-codegen-{}-{}-{}-{}",
            std::process::id(),
            unique,
            nanos,
            label
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("cannot create the temporary directory");
        TempDir { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn compile(code: &str, label: &str) -> std::path::PathBuf {
    let dir = TempDir::new(label);
    let source = dir.path.join("program.rs");
    std::fs::write(&source, code).expect("cannot write the generated program");
    let binary = dir.path.join(if cfg!(windows) {
        "program.exe"
    } else {
        "program"
    });
    let output = std::process::Command::new("rustc")
        .arg("--edition")
        .arg("2021")
        .arg("-C")
        .arg("overflow-checks=on")
        .arg("-o")
        .arg(&binary)
        .arg(&source)
        .output()
        .expect("cannot run rustc");
    assert!(
        output.status.success(),
        "rustc rejected `{label}`:\n{}\n--- generated ---\n{}",
        String::from_utf8_lossy(&output.stderr),
        code
    );
    // Keep the directory alive by leaking it: the test binary ends right
    // after, and the OS cleans the temporary directory. This avoids keeping
    // a self-referential path in the returned value.
    std::mem::forget(dir);
    binary
}

fn run(binary: &std::path::Path) -> (String, String, i32) {
    run_with_args(binary, &[])
}

fn run_with_args(binary: &std::path::Path, args: &[&str]) -> (String, String, i32) {
    let output = std::process::Command::new(binary)
        .args(args)
        .output()
        .expect("cannot run the compiled program");
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    )
}

#[test]
fn prelude_compiles_alone_with_rustc() {
    if !rustc_available() {
        eprintln!("SKIP prelude_compiles_alone_with_rustc: rustc not available");
        return;
    }
    let mut program = String::from(PRELUDE);
    program.push_str("\nfn main() {\n    crate::rv_install_panic_hook();\n}\n");
    let binary = compile(&program, "prelude");
    let (stdout, stderr, code) = run(&binary);
    assert!(stdout.is_empty(), "prelude prints nothing: {stdout}");
    assert!(stderr.is_empty(), "prelude writes nothing: {stderr}");
    assert_eq!(code, 0);
}

#[test]
fn generated_program_compiles_and_runs() {
    if !rustc_available() {
        eprintln!("SKIP generated_program_compiles_and_runs: rustc not available");
        return;
    }
    let code = generate_ok(&build_program_a().checked, &GenOptions::default());
    let binary = compile(&code, "program-a");
    let (stdout, stderr, code) = run(&binary);
    assert_eq!(stdout, "hello\n3\n", "stderr: {stderr}");
    assert_eq!(code, 0, "stderr: {stderr}");
}

#[test]
fn error_returning_main_exits_with_one() {
    if !rustc_available() {
        eprintln!("SKIP error_returning_main_exits_with_one: rustc not available");
        return;
    }
    let code = generate_ok(&build_program_c(), &GenOptions::default());
    let binary = compile(&code, "program-c");
    let (stdout, stderr, code) = run(&binary);
    assert_eq!(stdout, "");
    assert_eq!(stderr, "error: boom\n");
    assert_eq!(code, 1);
}

#[test]
fn generated_test_runner_reports_results() {
    if !rustc_available() {
        eprintln!("SKIP generated_test_runner_reports_results: rustc not available");
        return;
    }
    let options = GenOptions {
        test_mode: true,
        ..Default::default()
    };
    let code = generate_ok(&build_program_b(), &options);
    let binary = compile(&code, "program-b");
    let (stdout, stderr, code) = run(&binary);
    assert!(stdout.contains("test passes ... ok"), "stdout: {stdout}");
    assert!(stdout.contains("test fails ... FAILED"), "stdout: {stdout}");
    assert!(stdout.contains("1 passed; 1 failed"), "stdout: {stdout}");
    assert!(
        stdout.contains("assertion failed at main.rv:1"),
        "stdout: {stdout}"
    );
    assert_eq!(code, 1, "stderr: {stderr}");
}

#[test]
fn panicking_main_reports_panic_and_exit_one() {
    if !rustc_available() {
        eprintln!("SKIP panicking_main_reports_panic_and_exit_one: rustc not available");
        return;
    }
    let mut b = Builder::new();
    let main_id = b.id();
    let callee = b.callee("panic");
    let text = b.text("kaboom");
    let panic_call = b.call(
        callee,
        vec![text],
        CallTarget::BuiltinFn {
            path: "std::panic::panic".to_string(),
        },
    );
    let body = b.block(Vec::new(), Some(panic_call));
    let unit_ret = b.ty(TypeKind::Unit, Some(STy::Unit));
    let main_item = make_fn(
        &mut b,
        main_id,
        "main",
        Vis::Private,
        false,
        Vec::new(),
        unit_ret,
        body,
    );
    b.items.push(main_item);
    let program = b.finish(Some(main_id), Vec::new());
    let code = generate_ok(&program, &GenOptions::default());
    let binary = compile(&code, "program-panic");
    let (_stdout, stderr, code) = run(&binary);
    assert_eq!(stderr, "panic: kaboom\n");
    assert_eq!(code, 1);
}

/// End-to-end: source -> loader -> checker -> codegen -> rustc -> run.
///
/// Proves the real pipeline on the shipped examples (`hello`, `fib`,
/// `calculator`): argument passing, `Result` main, error exit codes and `?`.
#[test]
fn e2e_examples_source_to_binary() {
    if !rustc_available() {
        eprintln!("SKIP e2e_examples_source_to_binary: rustc not available");
        return;
    }
    let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    // (example, program args, expected stdout, expected stderr fragment, exit code)
    let cases: &[(&str, &[&str], &str, &str, i32)] = &[
        ("hello", &[], "hello, ravum\n", "", 0),
        ("fib", &[], "0\n1\n1\n2\n3\n5\n8\n13\n21\n34\n55\n", "", 0),
        ("calculator", &["1", "+", "2"], "3\n", "", 0),
        ("calculator", &["1", "/", "0"], "", "division by zero", 1),
    ];

    for &(name, args, expected_stdout, expected_stderr, expected_status) in cases {
        let directory = examples.join(name);
        assert!(
            directory.join("ravum.toml").exists(),
            "example `{name}` is missing at {}",
            directory.display()
        );
        let loaded = ravum_lang_sema::load_package(&directory);
        assert!(
            loaded.diags.is_empty(),
            "loader diagnostics for `{name}`:\n{}",
            loaded.diags.render(&loaded.sources)
        );
        let program = loaded
            .program
            .expect("the loader produced no program for a valid package");
        let check_options = ravum_lang_sema::facts::CheckOptions::default();
        let checked = ravum_lang_sema::check(&program, &loaded.sources, &check_options);
        assert!(
            checked.diags.is_empty(),
            "checker diagnostics for `{name}`:\n{}",
            checked.diags.render(&loaded.sources)
        );
        let checked = checked
            .checked
            .expect("the checker produced no CheckedProgram");
        let options = GenOptions {
            crate_name: name.to_string(),
            ..Default::default()
        };
        let code = generate_ok(&checked, &options);
        let binary = compile(&code, name);
        let (stdout, stderr, status) = run_with_args(&binary, args);
        assert_eq!(
            stdout, expected_stdout,
            "example `{name}` stdout mismatch (stderr: {stderr})\n--- generated ---\n{code}"
        );
        if !expected_stderr.is_empty() {
            assert!(
                stderr.contains(expected_stderr),
                "example `{name}` stderr mismatch: {stderr}"
            );
        }
        assert_eq!(
            status, expected_status,
            "example `{name}` exit code mismatch (stderr: {stderr})"
        );
    }
}

/// End-to-end FFI: `extern "C"` declaration, `unsafe { ... }` call, real
/// C runtime symbol, generated extern wrapper compiled by rustc.
#[test]
fn e2e_extern_ffi_compiles_and_runs() {
    if !rustc_available() {
        eprintln!("SKIP e2e_extern_ffi_compiles_and_runs: rustc not available");
        return;
    }
    let dir = std::env::temp_dir().join(format!("ravum-ffi-e2e-{}", std::process::id()));
    let src = dir.join("src");
    std::fs::create_dir_all(&src).expect("cannot create FFI test package");
    std::fs::write(
        dir.join("ravum.toml"),
        "[package]\nname = \"ffi-abs\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
    )
    .expect("cannot write manifest");
    std::fs::write(
        src.join("main.rv"),
        r#"use std::io;

extern "C" {
    fn abs(x: i32) -> i32;
}

fn main() {
    let value = unsafe { abs(-9) };
    io::println(&value.to_string());
}
"#,
    )
    .expect("cannot write source");

    let loaded = ravum_lang_sema::load_package(&dir);
    assert!(
        loaded.diags.is_empty(),
        "loader diagnostics:\n{}",
        loaded.diags.render(&loaded.sources)
    );
    let program = loaded.program.expect("no program for the FFI package");
    let options = ravum_lang_sema::facts::CheckOptions::default();
    let checked = ravum_lang_sema::check(&program, &loaded.sources, &options);
    assert!(
        checked.diags.is_empty(),
        "checker diagnostics:\n{}",
        checked.diags.render(&loaded.sources)
    );
    let checked = checked.checked.expect("no CheckedProgram");
    let code = generate_ok(&checked, &GenOptions::default());
    assert!(
        code.contains("#![deny(unsafe_code)]"),
        "FFI programs must keep `deny(unsafe_code)` at crate level:\n{code}"
    );
    assert!(
        code.contains("#[allow(unsafe_code)]"),
        "the generated wrapper must carry the only local `allow(unsafe_code)`:\n{code}"
    );
    assert!(
        code.contains("unsafe { rvm_ffi_"),
        "the generated extern wrapper must contain the unsafe call:\n{code}"
    );

    let binary = compile(&code, "ffi-abs");
    let (stdout, stderr, status) = run(&binary);
    assert_eq!(stdout, "9\n", "stderr: {stderr}\n--- generated ---\n{code}");
    assert_eq!(status, 0, "stderr: {stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

// -- program E: broad language surface, compiled and executed -----------

fn method_builtin(
    b: &mut Builder,
    receiver: Expr,
    name: &str,
    args: Vec<Expr>,
    path: &str,
    ret: STy,
) -> Expr {
    b.method(
        receiver,
        name,
        args,
        MethodTarget::Builtin {
            path: path.to_string(),
            ret,
        },
    )
}

fn free_call(b: &mut Builder, name: &str, args: Vec<Expr>, path: &str) -> Expr {
    let callee = b.callee(name);
    b.call(
        callee,
        args,
        CallTarget::BuiltinFn {
            path: path.to_string(),
        },
    )
}

fn user_call(b: &mut Builder, name: &str, args: Vec<Expr>, item: NodeId) -> Expr {
    let callee = b.callee(name);
    b.call(callee, args, CallTarget::UserFn { item })
}

fn print_value(b: &mut Builder, value: Expr, owner: &str) -> Stmt {
    let path = format!("{owner}::to_string");
    let text = method_builtin(b, value, "to_string", Vec::new(), &path, STy::String);
    let borrowed = b.expr(
        ExprKind::Unary {
            op: UnOp::Ref,
            expr: Box::new(text),
        },
        Some(STy::Ref {
            mutable: false,
            inner: Box::new(STy::String),
        }),
    );
    let call = free_call(b, "println", vec![borrowed], "std::io::println");
    b.expr_stmt(call)
}

fn generic_path(b: &mut Builder, name: &str, args: Vec<AstType>) -> Path {
    let id = b.id();
    let ident = b.ident(name);
    Path {
        id,
        root: PathRoot::Plain,
        segments: vec![PathSegment { name: ident, args }],
        span: span(),
    }
}

fn binding_pat(b: &mut Builder, name: &str, mutable: bool) -> Pat {
    let ident = b.ident(name);
    b.pat(
        PatKind::Binding {
            mutable,
            name: ident,
        },
        None,
    )
}

fn variant_pattern(
    b: &mut Builder,
    enum_item: NodeId,
    variant: usize,
    names: &[&str],
    fields: VariantPatFields,
) -> Pat {
    let path = b.path(names);
    b.facts
        .resolutions
        .insert(path.id, Resolution::Variant { enum_item, variant });
    b.pat(PatKind::Variant { path, fields }, None)
}

fn unit_variant_pattern(b: &mut Builder, enum_item: NodeId, variant: usize, names: &[&str]) -> Pat {
    let path = b.path(names);
    b.facts
        .resolutions
        .insert(path.id, Resolution::Variant { enum_item, variant });
    b.pat(PatKind::Path(path), None)
}

// -- fact-path mapping and boolean operators ---------------------------

#[test]
fn fact_item_paths_are_rewritten_into_emitted_modules() {
    let program = build_program_e();
    let options = GenOptions::default();
    let index = Index::build(&program.checked);
    let emitter = Emitter {
        checked: &program.checked,
        options: &options,
        index,
        array_moves: std::cell::RefCell::new(ArrayMovePlan::default()),
        shadowed_names: std::cell::RefCell::new(BTreeSet::new()),
    };
    assert_eq!(
        emitter.rewrite_fact_path("crate::area"),
        "crate::rvm_root::area"
    );
    assert_eq!(
        emitter.rewrite_fact_path("crate::Shape::Circle"),
        "crate::rvm_root::Shape::Circle"
    );
    assert_eq!(
        emitter.rewrite_fact_path("crate::util::twice"),
        "crate::rvm_root::rvm_util::twice"
    );
    assert_eq!(
        emitter.rewrite_fact_path("crate::LIMIT"),
        "crate::rvm_root::LIMIT"
    );
    // Reserved Rust words stay mangled after rewriting.
    assert_eq!(
        emitter.rewrite_fact_path("crate::type"),
        "crate::rvm_root::r#type"
    );
}

#[test]
fn short_circuit_operators_are_preserved() {
    let mut b = Builder::new();
    let logic_id = b.id();
    let a_ty = b.named_type("bool", Some(STy::Bool));
    let b_ty = b.named_type("bool", Some(STy::Bool));
    let c_ty = b.named_type("bool", Some(STy::Bool));
    let param_a = make_param(&mut b, "a", a_ty);
    let param_b = make_param(&mut b, "b", b_ty);
    let param_c = make_param(&mut b, "c", c_ty);
    let a_value = b.var("a", Some(STy::Bool));
    let b_value = b.var("b", Some(STy::Bool));
    let and_expr = b.expr(
        ExprKind::Binary {
            op: BinOp::And,
            lhs: Box::new(a_value),
            rhs: Box::new(b_value),
        },
        Some(STy::Bool),
    );
    let c_value = b.var("c", Some(STy::Bool));
    let or_expr = b.expr(
        ExprKind::Binary {
            op: BinOp::Or,
            lhs: Box::new(and_expr),
            rhs: Box::new(c_value),
        },
        Some(STy::Bool),
    );
    let body = b.block(Vec::new(), Some(or_expr));
    let ret = b.named_type("bool", Some(STy::Bool));
    let item = make_fn(
        &mut b,
        logic_id,
        "logic",
        Vis::Public(span()),
        false,
        vec![param_a, param_b, param_c],
        ret,
        body,
    );
    b.items.push(item);
    let program = b.finish(None, Vec::new());
    let code = generate_ok(&program, &GenOptions::default());
    assert!(code.contains("(a && b) || c"), "generated:\n{code}");
}

// -- test runner with `Result`-returning tests --------------------------

#[test]
fn test_runner_supports_result_returning_tests() {
    if !rustc_available() {
        eprintln!("SKIP test_runner_supports_result_returning_tests: rustc not available");
        return;
    }
    let mut b = Builder::new();
    let result_ret = |b: &mut Builder| {
        let path = b.path(&["Result"]);
        b.ty(
            TypeKind::Path(path),
            Some(STy::Result(
                Box::new(STy::Unit),
                Box::new(STy::Nominal {
                    def: 950_001,
                    name: "Error".to_string(),
                    args: Vec::new(),
                }),
            )),
        )
    };

    // test fn happy() -> Result<(), Error> { Ok(()) }
    let happy_id = b.id();
    let ok_callee = b.callee("Ok");
    let unit_value = b.unit();
    let ok_call = b.call(
        ok_callee,
        vec![unit_value],
        // The checker records a prelude constructor as a builtin call.
        CallTarget::BuiltinFn {
            path: "Result::Ok".to_string(),
        },
    );
    let happy_body = b.block(Vec::new(), Some(ok_call));
    let happy_ret = result_ret(&mut b);
    let happy_item = make_fn(
        &mut b,
        happy_id,
        "happy",
        Vis::Private,
        true,
        Vec::new(),
        happy_ret,
        happy_body,
    );
    b.items.push(happy_item);

    // test fn sad() -> Result<(), Error> { Err(Error::new("nope")) }
    let sad_id = b.id();
    let new_callee = b.callee("Error::new");
    let nope = b.text("nope");
    let error_call = b.call(
        new_callee,
        vec![nope],
        CallTarget::BuiltinFn {
            path: "Error::new".to_string(),
        },
    );
    let err_callee = b.callee("Err");
    let err_call = b.call(
        err_callee,
        vec![error_call],
        // The checker records a prelude constructor as a builtin call.
        CallTarget::BuiltinFn {
            path: "Result::Err".to_string(),
        },
    );
    let sad_body = b.block(Vec::new(), Some(err_call));
    let sad_ret = result_ret(&mut b);
    let sad_item = make_fn(
        &mut b,
        sad_id,
        "sad",
        Vis::Private,
        true,
        Vec::new(),
        sad_ret,
        sad_body,
    );
    b.items.push(sad_item);

    let program = b.finish(None, vec![happy_id, sad_id]);
    let options = GenOptions {
        test_mode: true,
        ..Default::default()
    };
    let code = generate_ok(&program, &options);
    assert!(code.contains("crate::rv_test_result(\"happy\""));
    assert!(code.contains("crate::rv_test_result(\"sad\""));
    let binary = compile(&code, "program-result-tests");
    let (stdout, stderr, status) = run(&binary);
    assert!(stdout.contains("test happy ... ok"), "stdout: {stdout}");
    assert!(stdout.contains("test sad ... FAILED"), "stdout: {stdout}");
    assert!(stdout.contains("error: nope"), "stdout: {stdout}");
    assert!(stdout.contains("1 passed; 1 failed"), "stdout: {stdout}");
    assert_eq!(status, 1, "stderr: {stderr}");
}

// -- entry-point edge cases ---------------------------------------------

#[test]
fn main_detection_falls_back_to_the_declared_name() {
    let mut program = build_program_a();
    // A checked program normally sets `main_fn`; the generator must still
    // find `fn main` in the root module when the fact is absent.
    program.checked.facts.main_fn = None;
    let code = generate_ok(&program.checked, &GenOptions::default());
    assert!(code.contains("match crate::rvm_root::rv_user_main() {"));
}

#[test]
fn missing_main_generates_an_explicit_stub() {
    let program = build_program_d();
    let code = generate_ok(&program, &GenOptions::default());
    assert!(code.contains("this Ravum program has no `main` function"));
    assert!(code.contains("std::process::ExitCode::from(1)"));
}

#[test]
fn invalid_main_return_type_is_an_explicit_error() {
    let mut b = Builder::new();
    let main_id = b.id();
    let value = b.int(1, Some(STy::I32));
    let body = b.block(Vec::new(), Some(value));
    let ret = b.named_type("i32", Some(STy::I32));
    let item = make_fn(
        &mut b,
        main_id,
        "main",
        Vis::Private,
        false,
        Vec::new(),
        ret,
        body,
    );
    b.items.push(item);
    let program = b.finish(Some(main_id), Vec::new());
    let error = crate::generate(&program, &GenOptions::default())
        .expect_err("`main -> i32` must be rejected by code generation");
    assert!(
        error.message.contains("`main` must return"),
        "unexpected error: {}",
        error.message
    );
}

#[test]
fn banner_uses_the_requested_crate_name() {
    let program = build_program_a();
    let options = GenOptions {
        crate_name: "hello_world".to_string(),
        ..Default::default()
    };
    let code = generate_ok(&program.checked, &options);
    assert!(code.starts_with("// Generated by ravum-lang "));
    assert!(code.contains("from hello_world; do not edit."));
}

/// End-to-end `--test`: `ravum-lang test` semantics on `examples/tests-demo`
/// (unit tests, `assert`/`assert_eq`, owned strings, and a test function
/// returning `Result<(), Error>`). The user `main` must not run.
#[test]
fn e2e_tests_demo_runner() {
    if !rustc_available() {
        eprintln!("SKIP e2e_tests_demo_runner: rustc not available");
        return;
    }
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/tests-demo");
    assert!(
        directory.join("ravum.toml").exists(),
        "example `tests-demo` is missing at {}",
        directory.display()
    );
    let loaded = ravum_lang_sema::load_package(&directory);
    assert!(
        loaded.diags.is_empty(),
        "loader diagnostics for `tests-demo`:\n{}",
        loaded.diags.render(&loaded.sources)
    );
    let program = loaded
        .program
        .expect("the loader produced no program for a valid package");
    let check_options = ravum_lang_sema::facts::CheckOptions::default();
    let checked = ravum_lang_sema::check(&program, &loaded.sources, &check_options);
    assert!(
        checked.diags.is_empty(),
        "checker diagnostics for `tests-demo`:\n{}",
        checked.diags.render(&loaded.sources)
    );
    let checked = checked
        .checked
        .expect("the checker produced no CheckedProgram");
    let options = GenOptions {
        test_mode: true,
        crate_name: "tests_demo".to_string(),
        ..Default::default()
    };
    let code = generate_ok(&checked, &options);
    let binary = compile(&code, "tests-demo");
    let (stdout, stderr, status) = run(&binary);
    for expected in [
        "test addition_works ... ok",
        "test strings_work ... ok",
        "test result_propagation_works ... ok",
        "3 passed; 0 failed",
    ] {
        assert!(
            stdout.contains(expected),
            "missing `{expected}` in test output:\n{stdout}\n--- generated ---\n{code}"
        );
    }
    assert!(
        !stdout.contains("run `ravum-lang test`"),
        "the user `main` must not run in test mode: {stdout}"
    );
    assert_eq!(status, 0, "stderr: {stderr}");
}

/// `Buf::raw` appends the prelude as is: the next generated line must start on
/// a line of its own.
#[test]
fn the_prelude_ends_with_a_newline() {
    assert!(PRELUDE.ends_with('\n'));
}

// -- E6 coverage: pure helpers of builtins.rs ---------------------------------

#[test]
fn builtin_owner_names_drop_references_and_type_arguments() {
    use super::builtins::normalize_owner;
    assert_eq!(normalize_owner("Vec<i32>"), "Vec");
    assert_eq!(normalize_owner("&str"), "str");
    assert_eq!(normalize_owner("& mut Vec<String>"), "Vec");
    assert_eq!(normalize_owner("  String "), "String");
}

#[test]
fn a_method_of_an_unknown_owner_is_never_passed_through() {
    use super::builtins::builtin_method_is_direct;
    assert!(builtin_method_is_direct("Vec", "len"));
    assert!(!builtin_method_is_direct("Deque", "len"));
    assert!(!builtin_method_is_direct("", "len"));
}

#[test]
fn prelude_constructor_paths_are_recognised_by_their_last_segments() {
    use super::builtins::is_option_or_result_ctor;
    assert!(is_option_or_result_ctor("Some"));
    assert!(is_option_or_result_ctor("Option::None"));
    assert!(is_option_or_result_ctor("std::Result::Err"));
    assert!(!is_option_or_result_ctor("E::Some"));
    assert!(!is_option_or_result_ctor(""));
    assert!(!is_option_or_result_ctor("Option::"));
}
