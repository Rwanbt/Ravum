//! Independent audit of the generated Rust backend (reference backend B).
//!
//! Every claim of `docs/audit/BACKEND-AUDIT.md` is backed here by observed
//! behaviour: programs are built as hand-made `CheckedProgram` values, generated
//! with the public `ravum_lang_codegen::generate`, compiled with the real
//! `rustc --edition 2021` and executed. Where a behaviour diverges from the
//! Ravum Core 1.0 specification, the test asserts the *observed* behaviour and
//! names the finding so the audit cannot silently turn into a wish list.
//!
//! Tests do not modify `src/`: the backend is audited as delivered.

use ravum_lang_codegen::{generate, CodegenError, GenOptions};
use ravum_lang_sema::facts::{
    CallTarget, CheckedProgram, Facts, FieldInfo, FileInfo, MethodTarget, Profile, Resolution,
    StructInfo,
};
use ravum_lang_sema::types::Type as STy;
use ravum_lang_syntax::ast::{
    BinOp, Block, Expr, ExprKind, ExternBlock, ExternFn, FieldDef, FieldInit, FnItem, ImplItem,
    Item, ItemKind, LetStmt, Module, NodeId, Param, ParamKind, Pat, PatKind, Path, PathRoot,
    PathSegment, Program, Receiver, Stmt, StmtKind, StructItem, Type as AstType, TypeKind, UnOp,
    Vis,
};
use ravum_lang_syntax::span::Span;

// ---------------------------------------------------------------------------
// Hand-made checked programs (public API only).
// ---------------------------------------------------------------------------

struct B {
    next: std::cell::Cell<NodeId>,
    items: std::cell::RefCell<Vec<Item>>,
    facts: std::cell::RefCell<Facts>,
    files: Vec<FileInfo>,
}

fn span() -> Span {
    Span::new(0, 0, 3)
}

impl B {
    fn new() -> B {
        B {
            next: std::cell::Cell::new(1),
            items: std::cell::RefCell::new(Vec::new()),
            facts: std::cell::RefCell::new(Facts::default()),
            files: vec![FileInfo {
                path: "main.rv".to_string(),
                line_starts: vec![0, 10, 20, 40, 80],
            }],
        }
    }

    fn id(&self) -> NodeId {
        let id = self.next.get();
        self.next.set(id + 1);
        id
    }

    fn ident(&self, name: &str) -> ravum_lang_syntax::ast::Ident {
        ravum_lang_syntax::ast::Ident::new(name, span())
    }

    fn path(&self, names: &[&str]) -> Path {
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

    fn generic_path(&self, name: &str, args: Vec<AstType>) -> Path {
        let id = self.id();
        let ident = self.ident(name);
        Path {
            id,
            root: PathRoot::Plain,
            segments: vec![PathSegment { name: ident, args }],
            span: span(),
        }
    }

    fn ty(&self, kind: TypeKind, resolved: Option<STy>) -> AstType {
        let id = self.id();
        if let Some(resolved) = resolved {
            self.facts.borrow_mut().type_nodes.insert(id, resolved);
        }
        AstType {
            id,
            kind,
            span: span(),
        }
    }

    fn named_ty(&self, name: &str, resolved: Option<STy>) -> AstType {
        let path = self.path(&[name]);
        self.ty(TypeKind::Path(path), resolved)
    }

    fn e(&self, kind: ExprKind, ty: Option<STy>) -> Expr {
        let id = self.id();
        if let Some(ty) = ty {
            self.facts.borrow_mut().expr_types.insert(id, ty);
        }
        Expr {
            id,
            kind,
            span: span(),
        }
    }

    fn p(&self, kind: PatKind, ty: Option<STy>) -> Pat {
        let id = self.id();
        if let Some(ty) = ty {
            self.facts.borrow_mut().pat_types.insert(id, ty);
        }
        Pat {
            id,
            kind,
            span: span(),
        }
    }

    fn block(&self, stmts: Vec<Stmt>, tail: Option<Expr>) -> Block {
        Block {
            id: self.id(),
            stmts,
            tail: tail.map(Box::new),
            span: span(),
        }
    }

    fn bind_pat(&self, name: &str, mutable: bool) -> Pat {
        let ident = self.ident(name);
        self.p(
            PatKind::Binding {
                mutable,
                name: ident,
            },
            None,
        )
    }

    fn var(&self, name: &str, ty: Option<STy>) -> Expr {
        let path = self.path(&[name]);
        self.facts.borrow_mut().resolutions.insert(
            path.id,
            Resolution::Local {
                name: name.to_string(),
                mutable: false,
            },
        );
        self.e(ExprKind::Path(path), ty)
    }

    fn int(&self, value: u128, ty: Option<STy>) -> Expr {
        self.e(
            ExprKind::Int {
                value,
                suffix: None,
            },
            ty,
        )
    }

    fn float(&self, text: &str, ty: Option<STy>) -> Expr {
        self.e(
            ExprKind::Float {
                text: text.to_string(),
                suffix: None,
            },
            ty,
        )
    }

    fn text(&self, value: &str) -> Expr {
        self.e(ExprKind::Str(value.to_string()), None)
    }

    fn chr(&self, value: char) -> Expr {
        self.e(ExprKind::Char(value), Some(STy::Char))
    }

    fn boolean(&self, value: bool) -> Expr {
        self.e(ExprKind::Bool(value), Some(STy::Bool))
    }

    fn unit(&self) -> Expr {
        self.e(ExprKind::Unit, Some(STy::Unit))
    }

    fn callee(&self, name: &str) -> Expr {
        let path = self.path(&[name]);
        self.e(ExprKind::Path(path), None)
    }

    fn call_free(&self, callee: &str, args: Vec<Expr>, path: &str) -> Expr {
        let callee = self.callee(callee);
        self.call_target(
            callee,
            args,
            CallTarget::BuiltinFn {
                path: path.to_string(),
            },
        )
    }

    fn call_user(&self, callee: &str, args: Vec<Expr>, item: NodeId) -> Expr {
        let callee = self.callee(callee);
        self.call_target(callee, args, CallTarget::UserFn { item })
    }

    fn call_variant(
        &self,
        callee: &str,
        args: Vec<Expr>,
        enum_item: NodeId,
        variant: usize,
    ) -> Expr {
        let callee = self.callee(callee);
        self.call_target(callee, args, CallTarget::VariantCtor { enum_item, variant })
    }

    fn call_target(&self, callee: Expr, args: Vec<Expr>, target: CallTarget) -> Expr {
        let id = self.id();
        self.facts.borrow_mut().calls.insert(id, target);
        Expr {
            id,
            kind: ExprKind::Call {
                callee: Box::new(callee),
                args,
            },
            span: span(),
        }
    }

    fn method_builtin(
        &self,
        receiver: Expr,
        name: &str,
        args: Vec<Expr>,
        path: &str,
        ret: STy,
    ) -> Expr {
        self.method_target(
            receiver,
            name,
            args,
            MethodTarget::Builtin {
                path: path.to_string(),
                ret,
            },
        )
    }

    fn method_user(&self, receiver: Expr, name: &str, args: Vec<Expr>, item: NodeId) -> Expr {
        self.method_target(receiver, name, args, MethodTarget::User { item })
    }

    fn method_target(
        &self,
        receiver: Expr,
        name: &str,
        args: Vec<Expr>,
        target: MethodTarget,
    ) -> Expr {
        let method = self.ident(name);
        let id = self.id();
        self.facts.borrow_mut().methods.insert(id, target);
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

    fn let_stmt(&self, name: &str, ty: Option<AstType>, init: Expr) -> Stmt {
        let pattern = self.bind_pat(name, false);
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

    fn let_stmt_mut(&self, name: &str, ty: Option<AstType>, init: Expr) -> Stmt {
        let pattern = self.bind_pat(name, true);
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

    fn stmt(&self, expr: Expr) -> Stmt {
        Stmt {
            id: self.id(),
            kind: StmtKind::Expr { expr, semi: true },
            span: span(),
        }
    }

    fn finish(self, main: Option<NodeId>, tests: Vec<NodeId>) -> CheckedProgram {
        let root_id = self.id();
        let mut facts = self.facts.into_inner();
        facts.main_fn = main;
        facts.test_fns = tests;
        let items = self.items.into_inner();
        let program = Program {
            file: 0,
            root: Module {
                id: root_id,
                name: None,
                items,
                span: span(),
            },
        };
        CheckedProgram {
            program,
            facts,
            files: self.files,
            profile: Profile::Core,
        }
    }
}

fn make_param(b: &B, name: &str, ty: AstType) -> Param {
    let id = b.id();
    Param {
        id,
        kind: ParamKind::Binding {
            mutable: false,
            name: b.ident(name),
            ty,
        },
        span: span(),
    }
}

fn make_receiver(b: &B, receiver: Receiver) -> Param {
    let id = b.id();
    Param {
        id,
        kind: ParamKind::Receiver(receiver),
        span: span(),
    }
}

#[allow(clippy::too_many_arguments)]
fn make_fn(
    b: &B,
    id: NodeId,
    name: &str,
    vis: Vis,
    is_test: bool,
    params: Vec<Param>,
    ret: AstType,
    body: Block,
) -> Item {
    let name = b.ident(name);
    Item {
        id,
        vis,
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

fn i32_ty(b: &B) -> AstType {
    b.named_ty("i32", Some(STy::I32))
}

fn usize_ty(b: &B) -> AstType {
    b.named_ty("usize", Some(STy::Usize))
}

fn unit_ret(b: &B) -> AstType {
    b.ty(TypeKind::Unit, Some(STy::Unit))
}

fn result_unit_ret(b: &B) -> AstType {
    let path = b.path(&["Result"]);
    b.ty(
        TypeKind::Path(path),
        Some(STy::Result(
            Box::new(STy::Unit),
            Box::new(STy::Nominal {
                def: 900_001,
                name: "Error".to_string(),
                args: Vec::new(),
            }),
        )),
    )
}

fn ok_unit(b: &B) -> Expr {
    let unit = b.unit();
    b.call_variant("Ok", vec![unit], 900_002, 0)
}

fn println_expr(b: &B, argument: Expr) -> Expr {
    b.call_free("println", vec![argument], "std::io::println")
}

fn println_text(b: &B, text: &str) -> Stmt {
    let value = b.text(text);
    let call = println_expr(b, value);
    b.stmt(call)
}

/// `println(&value.to_string())` for a `Display` value of the given owner type.
fn println_value(b: &B, value: Expr, owner: &str) -> Stmt {
    let path = format!("{owner}::to_string");
    let text = b.method_builtin(value, "to_string", Vec::new(), &path, STy::String);
    let borrowed = b.e(
        ExprKind::Unary {
            op: UnOp::Ref,
            expr: Box::new(text),
        },
        Some(STy::Ref {
            mutable: false,
            inner: Box::new(STy::String),
        }),
    );
    let call = println_expr(b, borrowed);
    b.stmt(call)
}

fn gen(checked: &CheckedProgram) -> String {
    generate(checked, &GenOptions::default()).expect("code generation must succeed")
}

fn gen_with(checked: &CheckedProgram, options: &GenOptions) -> String {
    generate(checked, options).expect("code generation must succeed")
}

fn gen_err(checked: &CheckedProgram) -> CodegenError {
    generate(checked, &GenOptions::default()).expect_err("code generation must fail")
}

// ---------------------------------------------------------------------------
// rustc harness: compile and run the generated file for real.
// ---------------------------------------------------------------------------

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
            "ravum-audit-{}-{}-{}-{}",
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

struct Built {
    _dir: TempDir,
    binary: std::path::PathBuf,
}

/// Compile the generated Rust. `overflow_checks` mirrors the CLI choice
/// (`Some(true)` = `-C overflow-checks=on`); `Some(false)` documents what a
/// build that drops the flag would do.
fn compile(code: &str, label: &str, overflow_checks: Option<bool>) -> Result<Built, String> {
    let dir = TempDir::new(label);
    let source = dir.path.join("program.rs");
    std::fs::write(&source, code).expect("cannot write the generated program");
    let binary = dir.path.join(if cfg!(windows) {
        "program.exe"
    } else {
        "program"
    });
    let mut command = std::process::Command::new("rustc");
    command.arg("--edition").arg("2021");
    match overflow_checks {
        Some(true) => {
            command.arg("-C").arg("overflow-checks=on");
        }
        Some(false) => {
            command.arg("-C").arg("overflow-checks=off");
        }
        None => {}
    }
    let output = command
        .arg("-o")
        .arg(&binary)
        .arg(&source)
        .output()
        .expect("cannot run rustc");
    if output.status.success() {
        Ok(Built { _dir: dir, binary })
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

fn must_compile(code: &str, label: &str) -> Built {
    match compile(code, label, Some(true)) {
        Ok(built) => built,
        Err(stderr) => panic!("rustc rejected `{label}`:\n{stderr}\n--- generated ---\n{code}"),
    }
}

fn run(binary: &std::path::Path, args: &[&str]) -> (String, String, i32) {
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

fn skip(label: &str) -> bool {
    if rustc_available() {
        return false;
    }
    eprintln!("SKIP {label}: rustc not available");
    true
}

// ---------------------------------------------------------------------------
// A representative program shared by the structural tests.
// ---------------------------------------------------------------------------

struct Sample {
    checked: CheckedProgram,
}

fn sample_program() -> Sample {
    let b = B::new();

    // struct Point { pub x: i32 }
    let point_id = b.id();
    let point_ty = STy::Nominal {
        def: point_id,
        name: "Point".to_string(),
        args: Vec::new(),
    };
    let x_ty = i32_ty(&b);
    let point_name = b.ident("Point");
    let x_name = b.ident("x");
    let field_id = b.id();
    let struct_item = Item {
        id: point_id,
        vis: Vis::Public(span()),
        kind: ItemKind::Struct(StructItem {
            name: point_name,
            generics: Vec::new(),
            fields: vec![FieldDef {
                id: field_id,
                vis: Vis::Public(span()),
                name: x_name,
                ty: x_ty,
                span: span(),
            }],
            span: span(),
        }),
        span: span(),
    };
    b.items.borrow_mut().push(struct_item);
    b.facts.borrow_mut().structs.insert(
        point_id,
        StructInfo {
            item: point_id,
            rust_path: "crate::Point".to_string(),
            name: "Point".to_string(),
            generics: Vec::new(),
            fields: vec![FieldInfo {
                name: "x".to_string(),
                rust_name: "x".to_string(),
                ty: STy::I32,
                public: true,
            }],
            is_copy: false,
        },
    );

    // impl Point { fn get(&self) -> i32 { self.x } }
    let get_id = b.id();
    let self_value = b.e(ExprKind::SelfValue, Some(point_ty.clone()));
    let x_field = b.ident("x");
    let field_expr = b.e(
        ExprKind::Field {
            base: Box::new(self_value),
            name: x_field,
        },
        Some(STy::I32),
    );
    let get_body = b.block(Vec::new(), Some(field_expr));
    let get_ret = i32_ty(&b);
    let receiver = make_receiver(&b, Receiver::Shared);
    let get_item = make_fn(
        &b,
        get_id,
        "get",
        Vis::Public(span()),
        false,
        vec![receiver],
        get_ret,
        get_body,
    );
    let self_ty = b.named_ty("Point", Some(point_ty.clone()));
    let impl_item = Item {
        id: b.id(),
        vis: Vis::Private,
        kind: ItemKind::Impl(ImplItem {
            generics: Vec::new(),
            self_ty,
            items: vec![get_item],
            span: span(),
        }),
        span: span(),
    };
    b.items.borrow_mut().push(impl_item);

    // fn main() -> Result<(), Error>
    let main_id = b.id();
    let annotation = b.named_ty("Point", Some(point_ty.clone()));
    let literal_path = b.path(&["Point"]);
    b.facts
        .borrow_mut()
        .resolutions
        .insert(literal_path.id, Resolution::Item { item: point_id });
    let field_init_name = b.ident("x");
    let seven = b.int(7, Some(STy::I32));
    let literal = b.e(
        ExprKind::StructLit {
            path: literal_path,
            fields: vec![FieldInit {
                name: field_init_name,
                value: seven,
            }],
        },
        Some(point_ty.clone()),
    );
    let point_let = b.let_stmt("p", Some(annotation), literal);

    let receiver_p = b.var("p", Some(point_ty.clone()));
    let get_call = b.method_user(receiver_p, "get", Vec::new(), get_id);
    let int_annotation = i32_ty(&b);
    let value_let = b.let_stmt("v", Some(int_annotation), get_call);

    let receiver_v = b.var("v", Some(STy::I32));
    let to_string = b.method_builtin(
        receiver_v,
        "to_string",
        Vec::new(),
        "i32::to_string",
        STy::String,
    );
    let string_annotation = b.named_ty("String", Some(STy::String));
    let text_let = b.let_stmt("s", Some(string_annotation), to_string);

    let receiver_s = b.var("s", Some(STy::String));
    let print_call = b.call_free("println", vec![receiver_s], "std::io::println");
    let print_stmt = b.stmt(print_call);
    let tail = ok_unit(&b);
    let body = b.block(vec![point_let, value_let, text_let, print_stmt], Some(tail));
    let main_ret = result_unit_ret(&b);
    let main_item = make_fn(
        &b,
        main_id,
        "main",
        Vis::Private,
        false,
        Vec::new(),
        main_ret,
        body,
    );
    b.items.borrow_mut().push(main_item);

    Sample {
        checked: b.finish(Some(main_id), Vec::new()),
    }
}

// ---------------------------------------------------------------------------
// 1. Structural guarantees: safe output, deterministic output.
// ---------------------------------------------------------------------------

#[test]
fn audit_generated_file_is_safe_no_unsafe_constructs() {
    let code = gen(&sample_program().checked);
    assert!(
        code.contains("#![forbid(unsafe_code)]"),
        "the generated crate must forbid unsafe code"
    );
    for forbidden in [
        "unsafe fn",
        "unsafe {",
        "transmute(",
        "*const ",
        "*mut ",
        "as *const",
        "as *mut",
        "std::ptr",
        "wrapping_add(",
        "wrapping_sub(",
        "wrapping_mul(",
    ] {
        assert!(
            !code.contains(forbidden),
            "generated code must not contain `{forbidden}`"
        );
    }
    // The runtime prelude is embedded and is safe as well.
    assert!(code.contains("pub struct Error"));
    // Audit markers are present.
    assert!(code.contains("// rv: main.rv:1"));
}

#[test]
fn audit_generated_output_is_deterministic() {
    let sample = sample_program();
    let first = gen(&sample.checked);
    let second = gen(&sample.checked);
    assert_eq!(first, second, "two generations of the same input differ");
    let third = gen_with(&sample.checked, &GenOptions::default());
    assert_eq!(first, third);
    // Marker placement is driven by options only.
    let mut options = GenOptions::default();
    options.source_comments = false;
    let without_markers = gen_with(&sample.checked, &options);
    assert!(!without_markers.contains("// rv:"));
    assert_eq!(without_markers, gen_with(&sample.checked, &options));
}

// ---------------------------------------------------------------------------
// 2. Fail-closed behaviour when a required fact is missing.
// ---------------------------------------------------------------------------

#[test]
fn audit_missing_required_facts_are_errors_not_guesses() {
    // A call target.
    let sample = sample_program();
    let mut checked = sample.checked.clone();
    assert!(!checked.facts.calls.is_empty());
    checked.facts.calls.pop_first();
    let error = gen_err(&checked);
    assert!(
        error.message.contains("missing call fact"),
        "unexpected error: {}",
        error.message
    );

    // A method target.
    let sample = sample_program();
    let mut checked = sample.checked.clone();
    checked.facts.methods.pop_first();
    let error = gen_err(&checked);
    assert!(
        error.message.contains("missing method fact"),
        "unexpected error: {}",
        error.message
    );

    // A path resolution.
    let sample = sample_program();
    let mut checked = sample.checked.clone();
    checked.facts.resolutions.clear();
    let error = gen_err(&checked);
    assert!(
        error.message.contains("missing resolution fact"),
        "unexpected error: {}",
        error.message
    );

    // A resolved nominal type.
    let sample = sample_program();
    let mut checked = sample.checked.clone();
    checked.facts.type_nodes.clear();
    let error = gen_err(&checked);
    assert!(
        error.message.contains("missing resolved type")
            || error.message.contains("cannot resolve nominal type"),
        "unexpected error: {}",
        error.message
    );
}

#[test]
fn audit_missing_literal_type_falls_back_to_rust_defaults_that_match_ravum() {
    // Documented fail-open path: when `Facts::expr_types` has no entry for a
    // literal, no suffix is emitted and Rust infers its default. Ravum Core 1.0
    // defaults are the same (i32 / f64, §3.2), so the behaviour is identical;
    // a checker that inferred another literal type must provide the fact.
    let b = B::new();
    let main_id = b.id();
    let int_literal = b.int(5, None);
    let int_let = b.let_stmt("x", None, int_literal);
    let float_literal = b.float("1.5", None);
    let float_let = b.let_stmt("y", None, float_literal);
    let x_value = b.var("x", None);
    let print_x = println_value(&b, x_value, "i32");
    let y_value = b.var("y", None);
    let print_y = println_value(&b, y_value, "f64");
    let tail = ok_unit(&b);
    let body = b.block(vec![int_let, float_let, print_x, print_y], Some(tail));
    let main_ret = result_unit_ret(&b);
    let main_item = make_fn(
        &b,
        main_id,
        "main",
        Vis::Private,
        false,
        Vec::new(),
        main_ret,
        body,
    );
    b.items.borrow_mut().push(main_item);
    let checked = b.finish(Some(main_id), Vec::new());

    let code = gen(&checked);
    assert!(
        code.contains("let x = 5;"),
        "unexpected literal form:\n{code}"
    );
    if skip("audit_missing_literal_type_falls_back_to_rust_defaults_that_match_ravum") {
        return;
    }
    let built = must_compile(&code, "literal-defaults");
    let (stdout, stderr, status) = run(&built.binary, &[]);
    assert_eq!(stdout, "5\n1.5\n", "stderr: {stderr}");
    assert_eq!(status, 0, "stderr: {stderr}");
}

// ---------------------------------------------------------------------------
// 3. Integer overflow: runtime panic under `-C overflow-checks=on`,
//    silent wrap when the flag is dropped (documented dependency, F1).
// ---------------------------------------------------------------------------

fn overflow_program() -> CheckedProgram {
    let b = B::new();
    let main_id = b.id();

    // let argc: usize = rv_args().len();
    let args_call = b.call_free("args", Vec::new(), "std::env::args");
    let len_call = b.method_builtin(args_call, "len", Vec::new(), "Vec::len", STy::Usize);
    let argc_ty = usize_ty(&b);
    let argc_let = b.let_stmt("argc", Some(argc_ty), len_call);

    // let base: i32 = argc as i32;
    let argc_value = b.var("argc", Some(STy::Usize));
    let cast_ty = i32_ty(&b);
    let cast = b.e(
        ExprKind::Cast {
            expr: Box::new(argc_value),
            ty: cast_ty,
        },
        Some(STy::I32),
    );
    let base_ty = i32_ty(&b);
    let base_let = b.let_stmt("base", Some(base_ty), cast);

    // let total: i32 = base + 2147483647;  (runtime operand => runtime overflow)
    let base_value = b.var("base", Some(STy::I32));
    let max = b.int(2147483647, Some(STy::I32));
    let add = b.e(
        ExprKind::Binary {
            op: BinOp::Add,
            lhs: Box::new(base_value),
            rhs: Box::new(max),
        },
        Some(STy::I32),
    );
    let total_ty = i32_ty(&b);
    let total_let = b.let_stmt("total", Some(total_ty), add);
    let total_value = b.var("total", Some(STy::I32));
    let print_total = println_value(&b, total_value, "i32");

    let tail = ok_unit(&b);
    let body = b.block(vec![argc_let, base_let, total_let, print_total], Some(tail));
    let main_ret = result_unit_ret(&b);
    let main_item = make_fn(
        &b,
        main_id,
        "main",
        Vis::Private,
        false,
        Vec::new(),
        main_ret,
        body,
    );
    b.items.borrow_mut().push(main_item);
    b.finish(Some(main_id), Vec::new())
}

#[test]
fn audit_integer_overflow_panics_only_with_overflow_checks() {
    if skip("audit_integer_overflow_panics_only_with_overflow_checks") {
        return;
    }
    let code = gen(&overflow_program());

    // CLI contract: `-C overflow-checks=on`.
    let built = must_compile(&code, "overflow-on");
    let (stdout, stderr, status) = run(&built.binary, &[]);
    assert_eq!(stdout, "", "no value must be printed: {stdout}");
    assert_eq!(
        stderr, "panic: attempt to add with overflow\n",
        "unexpected panic payload: {stderr}"
    );
    assert_eq!(status, 1, "panic must exit with code 1");

    // F1: the emitted file does not pin overflow checks. A build that drops the
    // flag (e.g. `rustc -O`, where debug assertions are off) wraps silently.
    let built = compile(&code, "overflow-off", Some(false))
        .unwrap_or_else(|stderr| panic!("rustc rejected the same file with checks off:\n{stderr}"));
    let (stdout, stderr, status) = run(&built.binary, &[]);
    assert_eq!(status, 0, "stderr: {stderr}");
    assert_eq!(
        stdout, "-2147483648\n",
        "without overflow checks Rust wraps (documented divergence)"
    );
}

// ---------------------------------------------------------------------------
// 4. F2: constant-foldable panics are rejected at compile time by rustc's
//    deny-by-default lints, although Ravum specifies a runtime panic.
// ---------------------------------------------------------------------------

#[test]
fn audit_constant_foldable_panics_are_runtime_panics() {
    if skip("audit_constant_foldable_panics_are_runtime_panics") {
        return;
    }

    // Addition overflow with constant operands.
    let code = {
        let b = B::new();
        let main_id = b.id();
        let max_ty = i32_ty(&b);
        let max_literal = b.int(2147483647, Some(STy::I32));
        let max_let = b.let_stmt("max", Some(max_ty), max_literal);
        let max_value = b.var("max", Some(STy::I32));
        let one = b.int(1, Some(STy::I32));
        let add = b.e(
            ExprKind::Binary {
                op: BinOp::Add,
                lhs: Box::new(max_value),
                rhs: Box::new(one),
            },
            Some(STy::I32),
        );
        let over_ty = i32_ty(&b);
        let over_let = b.let_stmt("over", Some(over_ty), add);
        let over_value = b.var("over", Some(STy::I32));
        let print_over = println_value(&b, over_value, "i32");
        let tail = ok_unit(&b);
        let body = b.block(vec![max_let, over_let, print_over], Some(tail));
        let main_ret = result_unit_ret(&b);
        b.items.borrow_mut().push(make_fn(
            &b,
            main_id,
            "main",
            Vis::Private,
            false,
            Vec::new(),
            main_ret,
            body,
        ));
        gen(&b.finish(Some(main_id), Vec::new()))
    };
    // The generated crate allows `arithmetic_overflow`, so the operation
    // compiles and panics at run time, as Ravum §3.3 requires (audit finding F2
    // fixed).
    let built = must_compile(&code, "const-add-overflow");
    let (_stdout, stderr, status) = run(&built.binary, &[]);
    assert!(
        stderr.contains("attempt to add with overflow") || stderr.contains("overflow"),
        "expected a runtime overflow panic, got: {stderr}"
    );
    assert_eq!(status, 1, "overflow panic must exit 1: {stderr}");

    // Array index provably out of bounds.
    let code = {
        let b = B::new();
        let main_id = b.id();
        let element = i32_ty(&b);
        let len = b.int(3, Some(STy::Usize));
        let array_ty = b.ty(
            TypeKind::Array {
                elem: Box::new(element),
                len: Box::new(len),
            },
            Some(STy::Array {
                elem: Box::new(STy::I32),
                len: 3,
            }),
        );
        let one = b.int(1, Some(STy::I32));
        let two = b.int(2, Some(STy::I32));
        let three = b.int(3, Some(STy::I32));
        let array = b.e(
            ExprKind::Array(vec![one, two, three]),
            Some(STy::Array {
                elem: Box::new(STy::I32),
                len: 3,
            }),
        );
        let array_let = b.let_stmt("a", Some(array_ty), array);
        let base = b.var(
            "a",
            Some(STy::Array {
                elem: Box::new(STy::I32),
                len: 3,
            }),
        );
        let index = b.int(5, Some(STy::Usize));
        let indexed = b.e(
            ExprKind::Index {
                base: Box::new(base),
                index: Box::new(index),
            },
            Some(STy::I32),
        );
        let print_indexed = println_value(&b, indexed, "i32");
        let tail = ok_unit(&b);
        let body = b.block(vec![array_let, print_indexed], Some(tail));
        let main_ret = result_unit_ret(&b);
        b.items.borrow_mut().push(make_fn(
            &b,
            main_id,
            "main",
            Vis::Private,
            false,
            Vec::new(),
            main_ret,
            body,
        ));
        gen(&b.finish(Some(main_id), Vec::new()))
    };
    // Constant out-of-bounds index: allowed by `unconditional_panic`, panics at
    // run time with exit 1.
    let built = must_compile(&code, "const-index-oob");
    let (_stdout, stderr, status) = run(&built.binary, &[]);
    assert!(
        stderr.contains("index out of bounds") || stderr.contains("panic"),
        "expected a runtime index panic, got: {stderr}"
    );
    assert_eq!(status, 1, "index panic must exit 1: {stderr}");
}

// ---------------------------------------------------------------------------
// 5. Float display: Rust shortest round-trip formatting.
// ---------------------------------------------------------------------------

#[test]
fn audit_float_display_negative_zero_nan_infinity() {
    if skip("audit_float_display_negative_zero_nan_infinity") {
        return;
    }
    let b = B::new();
    let main_id = b.id();

    let zero_float = b.float("0.0", Some(STy::F64));
    let zero_ty = b.named_ty("f64", Some(STy::F64));
    let zero_let = b.let_stmt("zero", Some(zero_ty), zero_float);

    let neg_float = b.float("0.0", Some(STy::F64));
    let negative = b.e(
        ExprKind::Unary {
            op: UnOp::Neg,
            expr: Box::new(neg_float),
        },
        Some(STy::F64),
    );
    let neg_ty = b.named_ty("f64", Some(STy::F64));
    let neg_let = b.let_stmt("negative_zero", Some(neg_ty), negative);

    let one_float = b.float("1.0", Some(STy::F64));
    let three_float = b.float("3.0", Some(STy::F64));
    let third = b.e(
        ExprKind::Binary {
            op: BinOp::Div,
            lhs: Box::new(one_float),
            rhs: Box::new(three_float),
        },
        Some(STy::F64),
    );
    let third_ty = b.named_ty("f64", Some(STy::F64));
    let third_let = b.let_stmt("third", Some(third_ty), third);

    let zero_a = b.var("zero", Some(STy::F64));
    let zero_b = b.var("zero", Some(STy::F64));
    let nan = b.e(
        ExprKind::Binary {
            op: BinOp::Div,
            lhs: Box::new(zero_a),
            rhs: Box::new(zero_b),
        },
        Some(STy::F64),
    );
    let nan_ty = b.named_ty("f64", Some(STy::F64));
    let nan_let = b.let_stmt("nan", Some(nan_ty), nan);

    let one_inf = b.float("1.0", Some(STy::F64));
    let zero_inf = b.var("zero", Some(STy::F64));
    let infinity = b.e(
        ExprKind::Binary {
            op: BinOp::Div,
            lhs: Box::new(one_inf),
            rhs: Box::new(zero_inf),
        },
        Some(STy::F64),
    );
    let infinity_ty = b.named_ty("f64", Some(STy::F64));
    let infinity_let = b.let_stmt("infinity", Some(infinity_ty), infinity);

    let one_ninf = b.float("1.0", Some(STy::F64));
    let negative_one = b.e(
        ExprKind::Unary {
            op: UnOp::Neg,
            expr: Box::new(one_ninf),
        },
        Some(STy::F64),
    );
    let zero_ninf = b.var("zero", Some(STy::F64));
    let negative_infinity = b.e(
        ExprKind::Binary {
            op: BinOp::Div,
            lhs: Box::new(negative_one),
            rhs: Box::new(zero_ninf),
        },
        Some(STy::F64),
    );
    let negative_infinity_ty = b.named_ty("f64", Some(STy::F64));
    let negative_infinity_let = b.let_stmt(
        "negative_infinity",
        Some(negative_infinity_ty),
        negative_infinity,
    );

    let big_float = b.float("1e20", Some(STy::F64));
    let big_ty = b.named_ty("f64", Some(STy::F64));
    let big_let = b.let_stmt("big", Some(big_ty), big_float);

    let mut statements = vec![
        zero_let,
        neg_let,
        third_let,
        nan_let,
        infinity_let,
        negative_infinity_let,
        big_let,
    ];
    for name in [
        "negative_zero",
        "third",
        "nan",
        "infinity",
        "negative_infinity",
        "big",
    ] {
        let value = b.var(name, Some(STy::F64));
        statements.push(println_value(&b, value, "f64"));
    }
    let tail = ok_unit(&b);
    let body = b.block(statements, Some(tail));
    let main_ret = result_unit_ret(&b);
    b.items.borrow_mut().push(make_fn(
        &b,
        main_id,
        "main",
        Vis::Private,
        false,
        Vec::new(),
        main_ret,
        body,
    ));
    let checked = b.finish(Some(main_id), Vec::new());

    let code = gen(&checked);
    let built = must_compile(&code, "float-display");
    let (stdout, stderr, status) = run(&built.binary, &[]);
    assert_eq!(
        stdout, "-0\n0.3333333333333333\nNaN\ninf\n-inf\n100000000000000000000\n",
        "Rust shortest-round-trip Display (stderr: {stderr})"
    );
    assert_eq!(status, 0, "IEEE-754 NaN/infinity must not trap: {stderr}");
}

// ---------------------------------------------------------------------------
// 6. Evaluation order: call arguments and binary operands, short-circuit.
// ---------------------------------------------------------------------------

fn tick_fn(b: &B) -> NodeId {
    // fn tick(label: i32) -> i32 { println(&label.to_string()); label }
    let tick_id = b.id();
    let label_ty = i32_ty(b);
    let param = make_param(b, "label", label_ty);
    let label_value = b.var("label", Some(STy::I32));
    let print = println_value(b, label_value, "i32");
    let tail = b.var("label", Some(STy::I32));
    let body = b.block(vec![print], Some(tail));
    let ret = i32_ty(b);
    let item = make_fn(
        b,
        tick_id,
        "tick",
        Vis::Private,
        false,
        vec![param],
        ret,
        body,
    );
    b.items.borrow_mut().push(item);
    tick_id
}

#[test]
fn audit_evaluation_order_is_left_to_right_and_short_circuits() {
    if skip("audit_evaluation_order_is_left_to_right_and_short_circuits") {
        return;
    }
    let b = B::new();
    let tick_id = tick_fn(&b);
    let main_id = b.id();

    // fn two(a: i32, b: i32) -> i32 { a + b }
    let two_id = b.id();
    let a_ty = i32_ty(&b);
    let b_ty = i32_ty(&b);
    let param_a = make_param(&b, "a", a_ty);
    let param_b = make_param(&b, "b", b_ty);
    let a_value = b.var("a", Some(STy::I32));
    let b_value = b.var("b", Some(STy::I32));
    let sum = b.e(
        ExprKind::Binary {
            op: BinOp::Add,
            lhs: Box::new(a_value),
            rhs: Box::new(b_value),
        },
        Some(STy::I32),
    );
    let two_body = b.block(Vec::new(), Some(sum));
    let two_ret = i32_ty(&b);
    b.items.borrow_mut().push(make_fn(
        &b,
        two_id,
        "two",
        Vis::Private,
        false,
        vec![param_a, param_b],
        two_ret,
        two_body,
    ));

    let mut statements = Vec::new();
    // let sum: i32 = two(tick(1), tick(2));
    let tick_one = b.call_user("tick", vec![b.int(1, Some(STy::I32))], tick_id);
    let tick_two = b.call_user("tick", vec![b.int(2, Some(STy::I32))], tick_id);
    let two_call = b.call_user("two", vec![tick_one, tick_two], two_id);
    let sum_ty = i32_ty(&b);
    statements.push(b.let_stmt("sum", Some(sum_ty), two_call));
    let sum_value = b.var("sum", Some(STy::I32));
    statements.push(println_value(&b, sum_value, "i32"));

    // Short-circuit: the RHS must not run for `false && _` and `true || _`.
    let false_rhs = {
        let tick = b.call_user("tick", vec![b.int(3, Some(STy::I32))], tick_id);
        let zero = b.int(0, Some(STy::I32));
        b.e(
            ExprKind::Binary {
                op: BinOp::Gt,
                lhs: Box::new(tick),
                rhs: Box::new(zero),
            },
            Some(STy::Bool),
        )
    };
    let short_and = b.e(
        ExprKind::Binary {
            op: BinOp::And,
            lhs: Box::new(b.boolean(false)),
            rhs: Box::new(false_rhs),
        },
        Some(STy::Bool),
    );
    let bool_ty = b.named_ty("bool", Some(STy::Bool));
    statements.push(b.let_stmt("skipped_and", Some(bool_ty), short_and));

    let true_rhs = {
        let tick = b.call_user("tick", vec![b.int(4, Some(STy::I32))], tick_id);
        let zero = b.int(0, Some(STy::I32));
        b.e(
            ExprKind::Binary {
                op: BinOp::Gt,
                lhs: Box::new(tick),
                rhs: Box::new(zero),
            },
            Some(STy::Bool),
        )
    };
    let short_or = b.e(
        ExprKind::Binary {
            op: BinOp::Or,
            lhs: Box::new(b.boolean(true)),
            rhs: Box::new(true_rhs),
        },
        Some(STy::Bool),
    );
    let bool_ty = b.named_ty("bool", Some(STy::Bool));
    statements.push(b.let_stmt("skipped_or", Some(bool_ty), short_or));

    // Evaluated RHS of `&&`: tick(5) runs.
    let evaluated_rhs = {
        let tick = b.call_user("tick", vec![b.int(5, Some(STy::I32))], tick_id);
        let zero = b.int(0, Some(STy::I32));
        b.e(
            ExprKind::Binary {
                op: BinOp::Gt,
                lhs: Box::new(tick),
                rhs: Box::new(zero),
            },
            Some(STy::Bool),
        )
    };
    let evaluated_and = b.e(
        ExprKind::Binary {
            op: BinOp::And,
            lhs: Box::new(b.boolean(true)),
            rhs: Box::new(evaluated_rhs),
        },
        Some(STy::Bool),
    );
    let bool_ty = b.named_ty("bool", Some(STy::Bool));
    statements.push(b.let_stmt("evaluated_and", Some(bool_ty), evaluated_and));

    // Binary operands: tick(6) then tick(7).
    let left = b.call_user("tick", vec![b.int(6, Some(STy::I32))], tick_id);
    let right = b.call_user("tick", vec![b.int(7, Some(STy::I32))], tick_id);
    let difference = b.e(
        ExprKind::Binary {
            op: BinOp::Sub,
            lhs: Box::new(left),
            rhs: Box::new(right),
        },
        Some(STy::I32),
    );
    let difference_ty = i32_ty(&b);
    statements.push(b.let_stmt("difference", Some(difference_ty), difference));
    let difference_value = b.var("difference", Some(STy::I32));
    statements.push(println_value(&b, difference_value, "i32"));

    let tail = ok_unit(&b);
    let body = b.block(statements, Some(tail));
    let main_ret = result_unit_ret(&b);
    b.items.borrow_mut().push(make_fn(
        &b,
        main_id,
        "main",
        Vis::Private,
        false,
        Vec::new(),
        main_ret,
        body,
    ));
    let checked = b.finish(Some(main_id), Vec::new());

    let code = gen(&checked);
    let built = must_compile(&code, "evaluation-order");
    let (stdout, stderr, status) = run(&built.binary, &[]);
    assert_eq!(
        stdout, "1\n2\n3\n5\n6\n7\n-1\n",
        "arguments/operands left-to-right and `&&`/`||` short-circuit (stderr: {stderr})"
    );
    assert_eq!(status, 0, "stderr: {stderr}");
}

// ---------------------------------------------------------------------------
// 7. F3: assignment evaluates the value before the place (Rust rule), while
//    Ravum §3.3 states left-to-right evaluation.
// ---------------------------------------------------------------------------

#[test]
fn audit_assignment_evaluates_rhs_before_place() {
    if skip("audit_assignment_evaluates_rhs_before_place") {
        return;
    }
    let b = B::new();
    let main_id = b.id();

    // fn idx() -> usize { println("idx"); 0 }
    let idx_id = b.id();
    let print_idx = println_text(&b, "idx");
    let zero_usize = b.int(0, Some(STy::Usize));
    let idx_body = b.block(vec![print_idx], Some(zero_usize));
    let idx_ret = usize_ty(&b);
    b.items.borrow_mut().push(make_fn(
        &b,
        idx_id,
        "idx",
        Vis::Private,
        false,
        Vec::new(),
        idx_ret,
        idx_body,
    ));

    // fn val() -> i32 { println("val"); 9 }
    let val_id = b.id();
    let print_val = println_text(&b, "val");
    let nine = b.int(9, Some(STy::I32));
    let val_body = b.block(vec![print_val], Some(nine));
    let val_ret = i32_ty(&b);
    b.items.borrow_mut().push(make_fn(
        &b,
        val_id,
        "val",
        Vis::Private,
        false,
        Vec::new(),
        val_ret,
        val_body,
    ));

    // let mut a: [i32; 1] = [0];
    let element = i32_ty(&b);
    let len = b.int(1, Some(STy::Usize));
    let array_ty = b.ty(
        TypeKind::Array {
            elem: Box::new(element),
            len: Box::new(len),
        },
        Some(STy::Array {
            elem: Box::new(STy::I32),
            len: 1,
        }),
    );
    let zero_value = b.int(0, Some(STy::I32));
    let array = b.e(
        ExprKind::Array(vec![zero_value]),
        Some(STy::Array {
            elem: Box::new(STy::I32),
            len: 1,
        }),
    );
    let array_let = b.let_stmt_mut("a", Some(array_ty), array);

    // a[idx()] = val();
    let target = {
        let base = b.var(
            "a",
            Some(STy::Array {
                elem: Box::new(STy::I32),
                len: 1,
            }),
        );
        let index = b.call_user("idx", Vec::new(), idx_id);
        b.e(
            ExprKind::Index {
                base: Box::new(base),
                index: Box::new(index),
            },
            Some(STy::I32),
        )
    };
    let value = b.call_user("val", Vec::new(), val_id);
    let assign = b.e(
        ExprKind::Assign {
            target: Box::new(target),
            value: Box::new(value),
        },
        Some(STy::Unit),
    );
    let assign_stmt = b.stmt(assign);

    let first = {
        let base = b.var(
            "a",
            Some(STy::Array {
                elem: Box::new(STy::I32),
                len: 1,
            }),
        );
        let index = b.int(0, Some(STy::Usize));
        b.e(
            ExprKind::Index {
                base: Box::new(base),
                index: Box::new(index),
            },
            Some(STy::I32),
        )
    };
    let print_first = println_value(&b, first, "i32");

    let tail = ok_unit(&b);
    let body = b.block(vec![array_let, assign_stmt, print_first], Some(tail));
    let main_ret = result_unit_ret(&b);
    b.items.borrow_mut().push(make_fn(
        &b,
        main_id,
        "main",
        Vis::Private,
        false,
        Vec::new(),
        main_ret,
        body,
    ));
    let checked = b.finish(Some(main_id), Vec::new());

    let code = gen(&checked);
    let built = must_compile(&code, "assignment-order");
    let (stdout, stderr, status) = run(&built.binary, &[]);
    // Observed Rust order: value first, then the place expression. Ravum §3.3
    // says "évaluation de gauche à droite"; this is finding F3 (low severity:
    // only observable with side effects inside the assignment place).
    assert_eq!(stdout, "val\nidx\n9\n", "observed order (stderr: {stderr})");
    assert_eq!(status, 0, "stderr: {stderr}");
}

// ---------------------------------------------------------------------------
// 8. Index and slice bounds: runtime panic with the Rust payload.
// ---------------------------------------------------------------------------

#[test]
fn audit_index_out_of_bounds_panics_at_runtime() {
    if skip("audit_index_out_of_bounds_panics_at_runtime") {
        return;
    }
    let b = B::new();
    let main_id = b.id();

    // let mut v: Vec<i32> = Vec::new(); v.push(1); v.push(2); v.push(3);
    let new_call = b.call_free("Vec::new", Vec::new(), "Vec::new");
    let element = i32_ty(&b);
    let vec_path = b.generic_path("Vec", vec![element]);
    let vec_ty = b.ty(TypeKind::Path(vec_path), Some(STy::Vec(Box::new(STy::I32))));
    let v_let = b.let_stmt_mut("v", Some(vec_ty), new_call);
    let mut statements = vec![v_let];
    for value in [1u128, 2, 3] {
        let receiver = b.var("v", Some(STy::Vec(Box::new(STy::I32))));
        let argument = b.int(value, Some(STy::I32));
        let push = b.method_builtin(receiver, "push", vec![argument], "Vec::push", STy::Unit);
        let push_stmt = b.stmt(push);
        statements.push(push_stmt);
    }

    // let argc: usize = rv_args().len(); let index: usize = argc + 4; (runtime)
    let args_call = b.call_free("args", Vec::new(), "std::env::args");
    let len_call = b.method_builtin(args_call, "len", Vec::new(), "Vec::len", STy::Usize);
    let argc_ty = usize_ty(&b);
    let argc_let = b.let_stmt("argc", Some(argc_ty), len_call);
    statements.push(argc_let);
    let argc_value = b.var("argc", Some(STy::Usize));
    let four = b.int(4, Some(STy::Usize));
    let index_expr = b.e(
        ExprKind::Binary {
            op: BinOp::Add,
            lhs: Box::new(argc_value),
            rhs: Box::new(four),
        },
        Some(STy::Usize),
    );
    let index_ty = usize_ty(&b);
    let index_let = b.let_stmt("index", Some(index_ty), index_expr);
    statements.push(index_let);

    let indexed = {
        let base = b.var("v", Some(STy::Vec(Box::new(STy::I32))));
        let index = b.var("index", Some(STy::Usize));
        b.e(
            ExprKind::Index {
                base: Box::new(base),
                index: Box::new(index),
            },
            Some(STy::I32),
        )
    };
    let print_indexed = println_value(&b, indexed, "i32");
    statements.push(print_indexed);

    let tail = ok_unit(&b);
    let body = b.block(statements, Some(tail));
    let main_ret = result_unit_ret(&b);
    b.items.borrow_mut().push(make_fn(
        &b,
        main_id,
        "main",
        Vis::Private,
        false,
        Vec::new(),
        main_ret,
        body,
    ));
    let checked = b.finish(Some(main_id), Vec::new());
    let code = gen(&checked);
    let built = must_compile(&code, "index-oob");
    let (stdout, stderr, status) = run(&built.binary, &[]);
    assert_eq!(stdout, "");
    assert!(
        stderr.starts_with("panic: index out of bounds"),
        "unexpected payload: {stderr}"
    );
    assert_eq!(status, 1, "OOB index must exit with code 1");
}

#[test]
fn audit_slice_range_out_of_bounds_panics_at_runtime() {
    if skip("audit_slice_range_out_of_bounds_panics_at_runtime") {
        return;
    }
    let b = B::new();
    let main_id = b.id();

    // fn take_first(xs: &[i32]) -> i32 { xs[0] }
    let take_id = b.id();
    let slice_inner = b.named_ty("i32", Some(STy::I32));
    let slice_ty = b.ty(
        TypeKind::Slice(Box::new(slice_inner)),
        Some(STy::Slice(Box::new(STy::I32))),
    );
    let param_ty = b.ty(
        TypeKind::Ref {
            mutable: false,
            inner: Box::new(slice_ty),
        },
        Some(STy::Ref {
            mutable: false,
            inner: Box::new(STy::Slice(Box::new(STy::I32))),
        }),
    );
    let param = make_param(&b, "xs", param_ty);
    let first = {
        let base = b.var(
            "xs",
            Some(STy::Ref {
                mutable: false,
                inner: Box::new(STy::Slice(Box::new(STy::I32))),
            }),
        );
        let index = b.int(0, Some(STy::Usize));
        b.e(
            ExprKind::Index {
                base: Box::new(base),
                index: Box::new(index),
            },
            Some(STy::I32),
        )
    };
    let take_body = b.block(Vec::new(), Some(first));
    let take_ret = i32_ty(&b);
    b.items.borrow_mut().push(make_fn(
        &b,
        take_id,
        "take_first",
        Vis::Private,
        false,
        vec![param],
        take_ret,
        take_body,
    ));

    // let mut v: Vec<i32> = Vec::new(); v.push(1); v.push(2); v.push(3);
    let new_call = b.call_free("Vec::new", Vec::new(), "Vec::new");
    let element = i32_ty(&b);
    let vec_path = b.generic_path("Vec", vec![element]);
    let vec_ty = b.ty(TypeKind::Path(vec_path), Some(STy::Vec(Box::new(STy::I32))));
    let v_let = b.let_stmt_mut("v", Some(vec_ty), new_call);
    let mut statements = vec![v_let];
    for value in [1u128, 2, 3] {
        let receiver = b.var("v", Some(STy::Vec(Box::new(STy::I32))));
        let argument = b.int(value, Some(STy::I32));
        let push = b.method_builtin(receiver, "push", vec![argument], "Vec::push", STy::Unit);
        let push_stmt = b.stmt(push);
        statements.push(push_stmt);
    }

    // let start: usize = rv_args().len() - 1; let end: usize = start + 9;
    let args_call = b.call_free("args", Vec::new(), "std::env::args");
    let len_call = b.method_builtin(args_call, "len", Vec::new(), "Vec::len", STy::Usize);
    let argc_ty = usize_ty(&b);
    let argc_let = b.let_stmt("argc", Some(argc_ty), len_call);
    statements.push(argc_let);
    let argc_value = b.var("argc", Some(STy::Usize));
    let one = b.int(1, Some(STy::Usize));
    let start_expr = b.e(
        ExprKind::Binary {
            op: BinOp::Sub,
            lhs: Box::new(argc_value),
            rhs: Box::new(one),
        },
        Some(STy::Usize),
    );
    let start_ty = usize_ty(&b);
    let start_let = b.let_stmt("start", Some(start_ty), start_expr);
    statements.push(start_let);
    let start_value = b.var("start", Some(STy::Usize));
    let nine = b.int(9, Some(STy::Usize));
    let end_expr = b.e(
        ExprKind::Binary {
            op: BinOp::Add,
            lhs: Box::new(start_value),
            rhs: Box::new(nine),
        },
        Some(STy::Usize),
    );
    let end_ty = usize_ty(&b);
    let end_let = b.let_stmt("end", Some(end_ty), end_expr);
    statements.push(end_let);

    // take_first(&v[start..end])
    let slice = {
        let base = b.var("v", Some(STy::Vec(Box::new(STy::I32))));
        let start = b.var("start", Some(STy::Usize));
        let end = b.var("end", Some(STy::Usize));
        let range = b.e(
            ExprKind::Range {
                start: Some(Box::new(start)),
                end: Some(Box::new(end)),
                inclusive: false,
            },
            None,
        );
        let indexed = b.e(
            ExprKind::Index {
                base: Box::new(base),
                index: Box::new(range),
            },
            Some(STy::Slice(Box::new(STy::I32))),
        );
        b.e(
            ExprKind::Unary {
                op: UnOp::Ref,
                expr: Box::new(indexed),
            },
            Some(STy::Ref {
                mutable: false,
                inner: Box::new(STy::Slice(Box::new(STy::I32))),
            }),
        )
    };
    let call = b.call_user("take_first", vec![slice], take_id);
    let print_call = println_value(&b, call, "i32");
    statements.push(print_call);

    let tail = ok_unit(&b);
    let body = b.block(statements, Some(tail));
    let main_ret = result_unit_ret(&b);
    b.items.borrow_mut().push(make_fn(
        &b,
        main_id,
        "main",
        Vis::Private,
        false,
        Vec::new(),
        main_ret,
        body,
    ));
    let checked = b.finish(Some(main_id), Vec::new());
    let code = gen(&checked);
    let built = must_compile(&code, "slice-oob");
    let (stdout, stderr, status) = run(&built.binary, &[]);
    assert_eq!(stdout, "");
    assert!(
        stderr.starts_with("panic: range end index 9 out of range"),
        "unexpected payload: {stderr}"
    );
    assert_eq!(status, 1, "slice OOB must exit with code 1");
}

// ---------------------------------------------------------------------------
// 9. Casts follow Rust's documented, saturating semantics.
// ---------------------------------------------------------------------------

#[test]
fn audit_casts_are_rust_defined_and_saturating() {
    if skip("audit_casts_are_rust_defined_and_saturating") {
        return;
    }
    let b = B::new();
    let main_id = b.id();
    let mut statements = Vec::new();

    // let a: i32 = 3.9 as i32;  => 3
    let float = b.float("3.9", Some(STy::F64));
    let i32_node = i32_ty(&b);
    let cast_a = b.e(
        ExprKind::Cast {
            expr: Box::new(float),
            ty: i32_node,
        },
        Some(STy::I32),
    );
    let a_ty = i32_ty(&b);
    statements.push(b.let_stmt("a", Some(a_ty), cast_a));
    let a_value = b.var("a", Some(STy::I32));
    statements.push(println_value(&b, a_value, "i32"));

    // let b: u8 = -1.0 as u8;  => 0 (saturating)
    let one_float = b.float("1.0", Some(STy::F64));
    let negative = b.e(
        ExprKind::Unary {
            op: UnOp::Neg,
            expr: Box::new(one_float),
        },
        Some(STy::F64),
    );
    let u8_node = b.named_ty("u8", Some(STy::U8));
    let cast_b = b.e(
        ExprKind::Cast {
            expr: Box::new(negative),
            ty: u8_node,
        },
        Some(STy::U8),
    );
    let b_ty = b.named_ty("u8", Some(STy::U8));
    statements.push(b.let_stmt("b", Some(b_ty), cast_b));
    let b_value = b.var("b", Some(STy::U8));
    statements.push(println_value(&b, b_value, "u8"));

    // let c: u8 = 300.0 as u8;  => 255
    let big_float = b.float("300.0", Some(STy::F64));
    let u8_node = b.named_ty("u8", Some(STy::U8));
    let cast_c = b.e(
        ExprKind::Cast {
            expr: Box::new(big_float),
            ty: u8_node,
        },
        Some(STy::U8),
    );
    let c_ty = b.named_ty("u8", Some(STy::U8));
    statements.push(b.let_stmt("c", Some(c_ty), cast_c));
    let c_value = b.var("c", Some(STy::U8));
    statements.push(println_value(&b, c_value, "u8"));

    // let d: i64 = 1e20 as i64;  => i64::MAX
    let huge = b.float("1e20", Some(STy::F64));
    let i64_node = b.named_ty("i64", Some(STy::I64));
    let cast_d = b.e(
        ExprKind::Cast {
            expr: Box::new(huge),
            ty: i64_node,
        },
        Some(STy::I64),
    );
    let d_ty = b.named_ty("i64", Some(STy::I64));
    statements.push(b.let_stmt("d", Some(d_ty), cast_d));
    let d_value = b.var("d", Some(STy::I64));
    statements.push(println_value(&b, d_value, "i64"));

    // let e: u8 = 300 as u8;  => 44 (truncation)
    let int_literal = b.int(300, Some(STy::I32));
    let u8_node = b.named_ty("u8", Some(STy::U8));
    let cast_e = b.e(
        ExprKind::Cast {
            expr: Box::new(int_literal),
            ty: u8_node,
        },
        Some(STy::U8),
    );
    let e_ty = b.named_ty("u8", Some(STy::U8));
    statements.push(b.let_stmt("e", Some(e_ty), cast_e));
    let e_value = b.var("e", Some(STy::U8));
    statements.push(println_value(&b, e_value, "u8"));

    let tail = ok_unit(&b);
    let body = b.block(statements, Some(tail));
    let main_ret = result_unit_ret(&b);
    b.items.borrow_mut().push(make_fn(
        &b,
        main_id,
        "main",
        Vis::Private,
        false,
        Vec::new(),
        main_ret,
        body,
    ));
    let checked = b.finish(Some(main_id), Vec::new());
    let code = gen(&checked);
    let built = must_compile(&code, "casts");
    let (stdout, stderr, status) = run(&built.binary, &[]);
    assert_eq!(
        stdout, "3\n0\n255\n9223372036854775807\n44\n",
        "saturating/truncating casts (stderr: {stderr})"
    );
    assert_eq!(status, 0, "stderr: {stderr}");
}

// ---------------------------------------------------------------------------
// 10. Drop: `drop(x)` is immediate; scope drop order is unobservable.
// ---------------------------------------------------------------------------

#[test]
fn audit_drop_is_immediate_and_scope_order_is_unobservable() {
    if skip("audit_drop_is_immediate_and_scope_order_is_unobservable") {
        return;
    }
    let b = B::new();
    let main_id = b.id();

    let from_call = b.call_free("String::from", vec![b.text("payload")], "String::from");
    let string_ty = b.named_ty("String", Some(STy::String));
    let s_let = b.let_stmt("s", Some(string_ty), from_call);
    let before = println_text(&b, "before");
    let drop_call = {
        let owned = b.var("s", Some(STy::String));
        b.call_free("drop", vec![owned], "drop")
    };
    let drop_stmt = b.stmt(drop_call);
    let after = println_text(&b, "after");

    let tail = ok_unit(&b);
    let body = b.block(vec![s_let, before, drop_stmt, after], Some(tail));
    let main_ret = result_unit_ret(&b);
    b.items.borrow_mut().push(make_fn(
        &b,
        main_id,
        "main",
        Vis::Private,
        false,
        Vec::new(),
        main_ret,
        body,
    ));
    let checked = b.finish(Some(main_id), Vec::new());
    let code = gen(&checked);
    assert!(code.contains("drop(s);"), "explicit drop mapping:\n{code}");
    let built = must_compile(&code, "drop");
    let (stdout, stderr, status) = run(&built.binary, &[]);
    assert_eq!(
        stdout, "before\nafter\n",
        "drop must not print (stderr: {stderr})"
    );
    assert_eq!(status, 0, "stderr: {stderr}");
}

// ---------------------------------------------------------------------------
// 11. Panic payloads and exit codes.
// ---------------------------------------------------------------------------

fn panic_program(kind: PanicKind) -> CheckedProgram {
    let b = B::new();
    let main_id = b.id();
    let statements;
    match kind {
        PanicKind::Assert => {
            let condition = b.boolean(false);
            let call = b.call_free("assert", vec![condition], "std::panic::assert");
            statements = vec![b.stmt(call)];
        }
        PanicKind::Panic => {
            let message = b.text("boom");
            let call = b.call_free("panic", vec![message], "std::panic::panic");
            statements = vec![b.stmt(call)];
        }
        PanicKind::OptionUnwrap => {
            let path = b.path(&["None"]);
            b.facts.borrow_mut().resolutions.insert(
                path.id,
                Resolution::BuiltinConst {
                    path: "Option::None".to_string(),
                },
            );
            let none = b.e(ExprKind::Path(path), Some(STy::Option(Box::new(STy::I32))));
            let option_ty = {
                let element = i32_ty(&b);
                let option_path = b.generic_path("Option", vec![element]);
                b.ty(
                    TypeKind::Path(option_path),
                    Some(STy::Option(Box::new(STy::I32))),
                )
            };
            let none_let = b.let_stmt("maybe", Some(option_ty), none);
            let receiver = b.var("maybe", Some(STy::Option(Box::new(STy::I32))));
            let unwrap =
                b.method_builtin(receiver, "unwrap", Vec::new(), "Option::unwrap", STy::I32);
            let unwrap_ty = i32_ty(&b);
            let unwrap_let = b.let_stmt("value", Some(unwrap_ty), unwrap);
            statements = vec![none_let, unwrap_let];
        }
        PanicKind::ResultUnwrap => {
            let error_call = b.call_free("Error::new", vec![b.text("x")], "Error::new");
            let result_expr = b.call_variant("Err", vec![error_call], 901_000, 1);
            let result_ty = b.ty(
                TypeKind::Path(b.path(&["Result"])),
                Some(STy::Result(
                    Box::new(STy::I32),
                    Box::new(STy::Nominal {
                        def: 901_001,
                        name: "Error".to_string(),
                        args: Vec::new(),
                    }),
                )),
            );
            let result_let = b.let_stmt("result", Some(result_ty), result_expr);
            let receiver = b.var(
                "result",
                Some(STy::Result(
                    Box::new(STy::I32),
                    Box::new(STy::Nominal {
                        def: 901_001,
                        name: "Error".to_string(),
                        args: Vec::new(),
                    }),
                )),
            );
            let unwrap =
                b.method_builtin(receiver, "unwrap", Vec::new(), "Result::unwrap", STy::I32);
            let unwrap_ty = i32_ty(&b);
            let unwrap_let = b.let_stmt("value", Some(unwrap_ty), unwrap);
            statements = vec![result_let, unwrap_let];
        }
    }
    let tail = ok_unit(&b);
    let body = b.block(statements, Some(tail));
    let main_ret = result_unit_ret(&b);
    b.items.borrow_mut().push(make_fn(
        &b,
        main_id,
        "main",
        Vis::Private,
        false,
        Vec::new(),
        main_ret,
        body,
    ));
    b.finish(Some(main_id), Vec::new())
}

#[derive(Clone, Copy)]
enum PanicKind {
    Assert,
    Panic,
    OptionUnwrap,
    ResultUnwrap,
}

#[test]
fn audit_panic_payloads_and_exit_codes() {
    if skip("audit_panic_payloads_and_exit_codes") {
        return;
    }
    let cases = [
        (
            PanicKind::Assert,
            "panic: assertion failed at main.rv:1\n",
            "assert",
        ),
        (PanicKind::Panic, "panic: boom\n", "panic"),
        (
            PanicKind::OptionUnwrap,
            "panic: called `Option::unwrap()` on a `None` value\n",
            "option-unwrap",
        ),
        (
            PanicKind::ResultUnwrap,
            "panic: called `Result::unwrap()` on an `Err` value: Error { message: \"x\" }\n",
            "result-unwrap",
        ),
    ];
    for (kind, expected, label) in cases {
        let checked = panic_program(kind);
        let code = gen(&checked);
        let built = must_compile(&code, label);
        let (stdout, stderr, status) = run(&built.binary, &[]);
        assert_eq!(stdout, "", "panic test must not print on stdout ({label})");
        assert_eq!(stderr, expected, "unexpected payload for `{label}`");
        assert_eq!(status, 1, "panic must exit with code 1 (`{label}`)");
    }
}

// ---------------------------------------------------------------------------
// 12. Test runner: text format, summary, exit codes, main not executed.
// ---------------------------------------------------------------------------

fn test_runner_program() -> CheckedProgram {
    let b = B::new();

    // test fn ok_one() { println("ran ok_one"); }  (prints, then succeeds)
    let ok_id = b.id();
    let ok_print = println_text(&b, "ran ok_one");
    let ok_body = b.block(vec![ok_print], None);
    let ok_ret = unit_ret(&b);
    b.items.borrow_mut().push(make_fn(
        &b,
        ok_id,
        "ok_one",
        Vis::Private,
        true,
        Vec::new(),
        ok_ret,
        ok_body,
    ));

    // test fn assert_fails() { assert(false); }
    let assert_id = b.id();
    let condition = b.boolean(false);
    let assert_call = b.call_free("assert", vec![condition], "std::panic::assert");
    let assert_body = b.block(vec![b.stmt(assert_call)], None);
    let assert_ret = unit_ret(&b);
    b.items.borrow_mut().push(make_fn(
        &b,
        assert_id,
        "assert_fails",
        Vis::Private,
        true,
        Vec::new(),
        assert_ret,
        assert_body,
    ));

    // test fn result_fails() -> Result<(), Error> { Err(Error::new("nope")) }
    let result_id = b.id();
    let error_call = b.call_free("Error::new", vec![b.text("nope")], "Error::new");
    let err_expr = b.call_variant("Err", vec![error_call], 902_000, 1);
    let result_body = b.block(Vec::new(), Some(err_expr));
    let result_ret = result_unit_ret(&b);
    b.items.borrow_mut().push(make_fn(
        &b,
        result_id,
        "result_fails",
        Vis::Private,
        true,
        Vec::new(),
        result_ret,
        result_body,
    ));

    // fn main() { println("USER MAIN"); }
    let main_id = b.id();
    let main_print = println_text(&b, "USER MAIN");
    let main_body = b.block(vec![main_print], None);
    let main_ret = unit_ret(&b);
    b.items.borrow_mut().push(make_fn(
        &b,
        main_id,
        "main",
        Vis::Private,
        false,
        Vec::new(),
        main_ret,
        main_body,
    ));

    b.finish(Some(main_id), vec![ok_id, assert_id, result_id])
}

#[test]
fn audit_test_runner_format_summary_and_exit_codes() {
    if skip("audit_test_runner_format_summary_and_exit_codes") {
        return;
    }
    let mut options = GenOptions::default();
    options.test_mode = true;
    let code = gen_with(&test_runner_program(), &options);
    let built = must_compile(&code, "runner");
    let (stdout, stderr, status) = run(&built.binary, &[]);
    let expected = concat!(
        "ran ok_one\n",
        "test ok_one ... ok\n",
        "test assert_fails ... FAILED\n",
        "    panicked: assertion failed at main.rv:1\n",
        "test result_fails ... FAILED\n",
        "    error: nope\n",
        "\n",
        "1 passed; 2 failed\n",
    );
    assert_eq!(stdout, expected, "runner output (stderr: {stderr})");
    assert_eq!(status, 1, "one failed test must exit with code 1");
    assert!(
        !stdout.contains("USER MAIN"),
        "the user `main` must not run in test mode"
    );
}

#[test]
fn audit_test_runner_all_pass_exits_zero() {
    if skip("audit_test_runner_all_pass_exits_zero") {
        return;
    }
    let b = B::new();
    let first_id = b.id();
    let first_print = println_text(&b, "first");
    let first_body = b.block(vec![first_print], None);
    let first_ret = unit_ret(&b);
    b.items.borrow_mut().push(make_fn(
        &b,
        first_id,
        "first",
        Vis::Private,
        true,
        Vec::new(),
        first_ret,
        first_body,
    ));
    let second_id = b.id();
    let second_print = println_text(&b, "second");
    let second_body = b.block(vec![second_print], None);
    let second_ret = unit_ret(&b);
    b.items.borrow_mut().push(make_fn(
        &b,
        second_id,
        "second",
        Vis::Private,
        true,
        Vec::new(),
        second_ret,
        second_body,
    ));
    let checked = b.finish(None, vec![first_id, second_id]);

    let mut options = GenOptions::default();
    options.test_mode = true;
    let code = gen_with(&checked, &options);
    let built = must_compile(&code, "runner-pass");
    let (stdout, stderr, status) = run(&built.binary, &[]);
    assert_eq!(
        stdout, "first\ntest first ... ok\nsecond\ntest second ... ok\n\n2 passed; 0 failed\n",
        "runner output (stderr: {stderr})"
    );
    assert_eq!(status, 0, "all tests passing must exit with code 0");
}

// ---------------------------------------------------------------------------
// 13. F4: `exit()` inside a test bypasses the runner (no summary, silent
//     truncation of the remaining tests).
// ---------------------------------------------------------------------------

#[test]
fn audit_exit_inside_a_test_bypasses_the_runner() {
    if skip("audit_exit_inside_a_test_bypasses_the_runner") {
        return;
    }
    let b = B::new();
    // test fn quits() { exit(0); }
    let quit_id = b.id();
    let zero = b.int(0, Some(STy::I32));
    let exit_call = b.call_free("exit", vec![zero], "std::process::exit");
    let quit_body = b.block(vec![b.stmt(exit_call)], None);
    let quit_ret = unit_ret(&b);
    b.items.borrow_mut().push(make_fn(
        &b,
        quit_id,
        "quits",
        Vis::Private,
        true,
        Vec::new(),
        quit_ret,
        quit_body,
    ));
    // test fn after() { println("after ran"); }
    let after_id = b.id();
    let after_print = println_text(&b, "after ran");
    let after_body = b.block(vec![after_print], None);
    let after_ret = unit_ret(&b);
    b.items.borrow_mut().push(make_fn(
        &b,
        after_id,
        "after",
        Vis::Private,
        true,
        Vec::new(),
        after_ret,
        after_body,
    ));
    let checked = b.finish(None, vec![quit_id, after_id]);

    let mut options = GenOptions::default();
    options.test_mode = true;
    let code = gen_with(&checked, &options);
    let built = must_compile(&code, "runner-exit");
    let (stdout, stderr, status) = run(&built.binary, &[]);
    // Observed: the process exits from inside the test, before the runner can
    // print a result line or the summary; the following test never runs.
    assert_eq!(stdout, "", "runner output: {stdout}");
    assert_eq!(status, 0, "the test's exit code wins (stderr: {stderr})");
    assert!(!stdout.contains("passed;"), "no summary is printed");
}

// ---------------------------------------------------------------------------
// 14. String and char escaping round-trips byte for byte.
// ---------------------------------------------------------------------------

#[test]
fn audit_string_and_char_escaping_round_trips() {
    if skip("audit_string_and_char_escaping_round_trips") {
        return;
    }
    let b = B::new();
    let main_id = b.id();

    let payload = "A\"B\\C\nD\tE\0F\u{1}G é 😀";
    let from_call = b.call_free("String::from", vec![b.text(payload)], "String::from");
    let string_ty = b.named_ty("String", Some(STy::String));
    let payload_let = b.let_stmt("s", Some(string_ty), from_call);
    let payload_value = b.var("s", Some(STy::String));
    let print_payload = println_expr(&b, payload_value);
    let print_payload_stmt = b.stmt(print_payload);

    let quote_char = b.chr('\'');
    let char_ty = b.named_ty("char", Some(STy::Char));
    let char_let = b.let_stmt("c", Some(char_ty), quote_char);
    let char_value = b.var("c", Some(STy::Char));
    let print_char = println_value(&b, char_value, "char");

    let tail = ok_unit(&b);
    let body = b.block(
        vec![payload_let, print_payload_stmt, char_let, print_char],
        Some(tail),
    );
    let main_ret = result_unit_ret(&b);
    b.items.borrow_mut().push(make_fn(
        &b,
        main_id,
        "main",
        Vis::Private,
        false,
        Vec::new(),
        main_ret,
        body,
    ));
    let checked = b.finish(Some(main_id), Vec::new());
    let code = gen(&checked);
    let built = must_compile(&code, "escaping");
    let (stdout, stderr, status) = run(&built.binary, &[]);
    let expected = format!("{payload}\n'\n");
    assert_eq!(
        stdout.as_bytes(),
        expected.as_bytes(),
        "byte-exact escaping (stderr: {stderr})"
    );
    assert_eq!(status, 0, "stderr: {stderr}");
}

// ---------------------------------------------------------------------------
// 15. Coercions at call sites: `&String -> &str`, `&[T; N] -> &[T]`.
// ---------------------------------------------------------------------------

#[test]
fn audit_coercions_apply_at_call_sites() {
    if skip("audit_coercions_apply_at_call_sites") {
        return;
    }
    let b = B::new();
    let main_id = b.id();

    // fn text_len(s: &str) -> usize { s.len() }
    let text_len_id = b.id();
    let str_ty = b.named_ty("str", Some(STy::Str));
    let text_param_ty = b.ty(
        TypeKind::Ref {
            mutable: false,
            inner: Box::new(str_ty),
        },
        Some(STy::Ref {
            mutable: false,
            inner: Box::new(STy::Str),
        }),
    );
    let text_param = make_param(&b, "s", text_param_ty);
    let text_value = b.var(
        "s",
        Some(STy::Ref {
            mutable: false,
            inner: Box::new(STy::Str),
        }),
    );
    let len = b.method_builtin(text_value, "len", Vec::new(), "str::len", STy::Usize);
    let text_len_body = b.block(Vec::new(), Some(len));
    let text_len_ret = usize_ty(&b);
    b.items.borrow_mut().push(make_fn(
        &b,
        text_len_id,
        "text_len",
        Vis::Private,
        false,
        vec![text_param],
        text_len_ret,
        text_len_body,
    ));

    // fn slice_first(xs: &[i32]) -> i32 { xs[0] }
    let slice_first_id = b.id();
    let element = b.named_ty("i32", Some(STy::I32));
    let slice_ty = b.ty(
        TypeKind::Slice(Box::new(element)),
        Some(STy::Slice(Box::new(STy::I32))),
    );
    let slice_param_ty = b.ty(
        TypeKind::Ref {
            mutable: false,
            inner: Box::new(slice_ty),
        },
        Some(STy::Ref {
            mutable: false,
            inner: Box::new(STy::Slice(Box::new(STy::I32))),
        }),
    );
    let slice_param = make_param(&b, "xs", slice_param_ty);
    let first = {
        let base = b.var(
            "xs",
            Some(STy::Ref {
                mutable: false,
                inner: Box::new(STy::Slice(Box::new(STy::I32))),
            }),
        );
        let index = b.int(0, Some(STy::Usize));
        b.e(
            ExprKind::Index {
                base: Box::new(base),
                index: Box::new(index),
            },
            Some(STy::I32),
        )
    };
    let slice_first_body = b.block(Vec::new(), Some(first));
    let slice_first_ret = i32_ty(&b);
    b.items.borrow_mut().push(make_fn(
        &b,
        slice_first_id,
        "slice_first",
        Vis::Private,
        false,
        vec![slice_param],
        slice_first_ret,
        slice_first_body,
    ));

    // let s: String = String::from("abc");
    let from_call = b.call_free("String::from", vec![b.text("abc")], "String::from");
    let string_ty = b.named_ty("String", Some(STy::String));
    let string_let = b.let_stmt("s", Some(string_ty), from_call);

    // let n: usize = text_len(&s);
    let borrow_string = {
        let value = b.var("s", Some(STy::String));
        b.e(
            ExprKind::Unary {
                op: UnOp::Ref,
                expr: Box::new(value),
            },
            Some(STy::Ref {
                mutable: false,
                inner: Box::new(STy::String),
            }),
        )
    };
    let text_len_call = b.call_user("text_len", vec![borrow_string], text_len_id);
    let n_ty = usize_ty(&b);
    let n_let = b.let_stmt("n", Some(n_ty), text_len_call);
    let n_value = b.var("n", Some(STy::Usize));
    let print_n = println_value(&b, n_value, "usize");

    // let arr: [i32; 2] = [4, 5];
    let array_element = i32_ty(&b);
    let array_len = b.int(2, Some(STy::Usize));
    let array_ty = b.ty(
        TypeKind::Array {
            elem: Box::new(array_element),
            len: Box::new(array_len),
        },
        Some(STy::Array {
            elem: Box::new(STy::I32),
            len: 2,
        }),
    );
    let four = b.int(4, Some(STy::I32));
    let five = b.int(5, Some(STy::I32));
    let array = b.e(
        ExprKind::Array(vec![four, five]),
        Some(STy::Array {
            elem: Box::new(STy::I32),
            len: 2,
        }),
    );
    let array_let = b.let_stmt("arr", Some(array_ty), array);

    // let m: i32 = slice_first(&arr);
    let borrow_array = {
        let value = b.var(
            "arr",
            Some(STy::Array {
                elem: Box::new(STy::I32),
                len: 2,
            }),
        );
        b.e(
            ExprKind::Unary {
                op: UnOp::Ref,
                expr: Box::new(value),
            },
            Some(STy::Ref {
                mutable: false,
                inner: Box::new(STy::Array {
                    elem: Box::new(STy::I32),
                    len: 2,
                }),
            }),
        )
    };
    let slice_call = b.call_user("slice_first", vec![borrow_array], slice_first_id);
    let m_ty = i32_ty(&b);
    let m_let = b.let_stmt("m", Some(m_ty), slice_call);
    let m_value = b.var("m", Some(STy::I32));
    let print_m = println_value(&b, m_value, "i32");

    let tail = ok_unit(&b);
    let body = b.block(
        vec![string_let, n_let, print_n, array_let, m_let, print_m],
        Some(tail),
    );
    let main_ret = result_unit_ret(&b);
    b.items.borrow_mut().push(make_fn(
        &b,
        main_id,
        "main",
        Vis::Private,
        false,
        Vec::new(),
        main_ret,
        body,
    ));
    let checked = b.finish(Some(main_id), Vec::new());
    let code = gen(&checked);
    let built = must_compile(&code, "coercions");
    let (stdout, stderr, status) = run(&built.binary, &[]);
    assert_eq!(stdout, "3\n4\n", "call-site coercions (stderr: {stderr})");
    assert_eq!(status, 0, "stderr: {stderr}");
}

// ---------------------------------------------------------------------------
// 16. F0: `#![forbid(unsafe_code)]` is conditional. FFI programs (`extern "C"`)
//     relax it to `#![allow(unsafe_code)]` and the generator emits one `unsafe`
//     block per extern wrapper. This is the only place where generated code is
//     not safe-by-construction, so it is audited explicitly.
// ---------------------------------------------------------------------------

struct ExternProgram {
    checked: CheckedProgram,
    extern_id: NodeId,
}

fn extern_program(use_text_parameter: bool) -> ExternProgram {
    let b = B::new();
    let extern_id = b.id();
    let params;
    let ret;
    let symbol;
    let callee_name;
    if use_text_parameter {
        let str_ty = b.named_ty("str", Some(STy::Str));
        let text_ty = b.ty(
            TypeKind::Ref {
                mutable: false,
                inner: Box::new(str_ty),
            },
            Some(STy::Ref {
                mutable: false,
                inner: Box::new(STy::Str),
            }),
        );
        params = vec![make_param(&b, "text", text_ty)];
        let ret_inner = b.named_ty("str", Some(STy::Str));
        ret = b.ty(
            TypeKind::Ref {
                mutable: false,
                inner: Box::new(ret_inner),
            },
            Some(STy::Ref {
                mutable: false,
                inner: Box::new(STy::Str),
            }),
        );
        symbol = "getenv";
        callee_name = "getenv";
    } else {
        let value_ty = i32_ty(&b);
        params = vec![make_param(&b, "value", value_ty)];
        ret = i32_ty(&b);
        symbol = "abs";
        callee_name = "abs";
    }
    let fn_name = b.ident(callee_name);
    let extern_fn = ExternFn {
        id: extern_id,
        name: fn_name,
        params,
        ret,
        span: span(),
    };
    let abi = b.ident("C");
    let extern_item = Item {
        id: b.id(),
        vis: Vis::Public(span()),
        kind: ItemKind::Extern(ExternBlock {
            abi,
            fns: vec![extern_fn],
            span: span(),
        }),
        span: span(),
    };
    b.items.borrow_mut().push(extern_item);
    b.facts
        .borrow_mut()
        .extern_symbols
        .insert(extern_id, symbol.to_string());
    b.facts
        .borrow_mut()
        .item_paths
        .insert(extern_id, format!("crate::rvm_ffi_wrap_{extern_id}"));

    let main_id = b.id();
    let body = if use_text_parameter {
        // No call: the declaration alone is audited; calling an ABI-mismatched
        // signature would be undefined behaviour, so the test never runs it.
        let print = println_text(&b, "declared");
        b.block(vec![print], None)
    } else {
        let negative = b.e(
            ExprKind::Unary {
                op: UnOp::Neg,
                expr: Box::new(b.int(9, Some(STy::I32))),
            },
            Some(STy::I32),
        );
        let call = b.call_user(callee_name, vec![negative], extern_id);
        let print = println_value(&b, call, "i32");
        b.block(vec![print], None)
    };
    let main_ret = unit_ret(&b);
    let main_item = make_fn(
        &b,
        main_id,
        "main",
        Vis::Private,
        false,
        Vec::new(),
        main_ret,
        body,
    );
    b.items.borrow_mut().push(main_item);
    ExternProgram {
        checked: b.finish(Some(main_id), Vec::new()),
        extern_id,
    }
}

#[test]
fn audit_ffi_relaxes_forbid_unsafe_and_calls_a_real_c_symbol() {
    if skip("audit_ffi_relaxes_forbid_unsafe_and_calls_a_real_c_symbol") {
        return;
    }
    let program = extern_program(false);
    let code = gen(&program.checked);
    assert!(
        code.contains("#![deny(unsafe_code)]"),
        "FFI programs keep a crate-level `deny(unsafe_code)`:\n{code}"
    );
    assert!(
        !code.contains("#![allow(unsafe_code)]"),
        "the crate-level `allow` was replaced by a local one:\n{code}"
    );
    assert!(
        code.contains("#[allow(unsafe_code)]"),
        "each generated wrapper carries the only local `allow`:\n{code}"
    );
    assert!(code.contains("extern \"C\" {"));
    assert!(code.contains("#[link_name = \"abs\"]"));
    assert!(
        code.contains("unsafe { rvm_ffi_"),
        "the generated wrapper is the only unsafe block:\n{code}"
    );
    // Exactly one unsafe block for one extern function.
    assert_eq!(
        code.matches("unsafe {").count(),
        1,
        "one extern declaration => one generated unsafe block"
    );
    let built = must_compile(&code, "ffi-abs-audit");
    let (stdout, stderr, status) = run(&built.binary, &[]);
    assert_eq!(stdout, "9\n", "stderr: {stderr}\n--- generated ---\n{code}");
    assert_eq!(status, 0, "stderr: {stderr}");
    let _ = program.extern_id;
}

#[test]
fn audit_ffi_rejects_non_ffi_safe_types() {
    let program = extern_program(true);
    let error = gen_err(&program.checked);
    assert!(
        error.message.contains("non-FFI-safe"),
        "the backend must reject ABI-unsafe extern signatures, got: {}",
        error.message
    );
}
