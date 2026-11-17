//! The backend refuses, with a message that names the defect, every input the
//! checker would never hand it: a call or method whose target is unresolved, a
//! builtin call missing its argument, an unknown builtin method, an ownership
//! place on an expression that is not a place, a test function with a return
//! type the runner cannot report.
//!
//! Each test checks a real program, then breaks one fact table or one AST
//! node the way a defect upstream would, and requires `generate` to fail
//! instead of emitting Rust that guesses. The last tests cover output shapes a
//! checked program can reach but no other test generated in process.

use ravum_lang_codegen::{generate, GenOptions};
use ravum_lang_sema::facts::{
    CallTarget, CheckOptions, CheckedProgram, MethodTarget, OwnPlace, Profile,
};
use ravum_lang_sema::syntax::ast::{Expr, ExprKind, ItemKind, StmtKind};
use ravum_lang_sema::types::Type;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

struct Package {
    dir: PathBuf,
}

impl Drop for Package {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Load and check `source` as `src/main.rv` of a temporary package.
fn checked(source: &str, profile: Profile) -> CheckedProgram {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let package = Package {
        dir: std::env::temp_dir().join(format!(
            "ravum-backend-fail-closed-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        )),
    };
    std::fs::create_dir_all(package.dir.join("src")).expect("cannot create the package");
    std::fs::write(
        package.dir.join("ravum.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
    )
    .expect("cannot write the manifest");
    std::fs::write(package.dir.join("src/main.rv"), source).expect("cannot write the source");
    let loaded = ravum_lang_sema::load_package(&package.dir);
    assert!(!loaded.diags.has_errors(), "the fixture must load");
    let program = loaded.program.expect("the fixture must load");
    let options = CheckOptions {
        profile,
        ..CheckOptions::default()
    };
    let result = ravum_lang_sema::check(&program, &loaded.sources, &options);
    let errors: Vec<String> = result
        .diags
        .iter()
        .filter(|diag| diag.code.starts_with("E-"))
        .map(|diag| format!("{}: {}", diag.code, diag.message))
        .collect();
    assert!(errors.is_empty(), "the fixture must check: {errors:?}");
    result.checked.expect("the fixture must check")
}

fn refusal(checked: &CheckedProgram, options: &GenOptions) -> String {
    match generate(checked, options) {
        Ok(_) => panic!("the backend generated code for a broken program"),
        Err(error) => error.message,
    }
}

/// The first expression statement of `main`.
fn first_main_expr(checked: &mut CheckedProgram) -> &mut Expr {
    for item in &mut checked.program.root.items {
        if let ItemKind::Fn(f) = &mut item.kind {
            if f.name.name == "main" {
                for stmt in &mut f.body.stmts {
                    if let StmtKind::Expr { expr, .. } = &mut stmt.kind {
                        return expr;
                    }
                }
            }
        }
    }
    panic!("the fixture has an expression statement in `main`")
}

#[test]
fn a_call_with_an_unresolved_target_is_refused() {
    let mut program = checked("fn f() { }\nfn main() { f(); }\n", Profile::Core);
    for target in program.facts.calls.values_mut() {
        *target = CallTarget::Error;
    }
    let message = refusal(&program, &GenOptions::default());
    assert!(message.contains("has an unresolved target"), "{message}");
}

#[test]
fn a_method_call_with_an_unresolved_target_is_refused() {
    let mut program = checked(
        "fn main() { let s = String::from(\"a\"); let n = s.len(); }\n",
        Profile::Core,
    );
    for target in program.facts.methods.values_mut() {
        *target = MethodTarget::Error;
    }
    let message = refusal(&program, &GenOptions::default());
    assert!(message.contains("has an unresolved target"), "{message}");
}

#[test]
fn an_unknown_builtin_method_is_refused() {
    let mut program = checked(
        "fn main() { let s = String::from(\"a\"); let n = s.len(); }\n",
        Profile::Core,
    );
    for target in program.facts.methods.values_mut() {
        *target = MethodTarget::Builtin {
            path: "String::nothing".to_string(),
            ret: Type::Usize,
        };
    }
    let message = refusal(&program, &GenOptions::default());
    assert!(
        message.contains("unsupported builtin method `String::nothing`"),
        "{message}"
    );

    for target in program.facts.methods.values_mut() {
        *target = MethodTarget::Builtin {
            path: "nothing".to_string(),
            ret: Type::Usize,
        };
    }
    let message = refusal(&program, &GenOptions::default());
    assert!(
        message.contains("malformed builtin method path `nothing`"),
        "{message}"
    );
}

#[test]
fn a_builtin_call_missing_its_argument_is_refused() {
    let mut program = checked("fn main() { io::println(\"a\"); }\n", Profile::Core);
    if let ExprKind::Call { args, .. } = &mut first_main_expr(&mut program).kind {
        args.clear();
    }
    let message = refusal(&program, &GenOptions::default());
    assert!(message.contains("missing its text argument"), "{message}");

    let mut program = checked(
        "fn main() { let n = string::parse_i64(\"4\"); }\n",
        Profile::Core,
    );
    for item in &mut program.program.root.items {
        if let ItemKind::Fn(f) = &mut item.kind {
            if let StmtKind::Let(l) = &mut f.body.stmts[0].kind {
                if let Some(Expr {
                    kind: ExprKind::Call { args, .. },
                    ..
                }) = &mut l.init
                {
                    args.clear();
                }
            }
        }
    }
    let message = refusal(&program, &GenOptions::default());
    assert!(message.contains("missing its text argument"), "{message}");
}

#[test]
fn a_two_argument_builtin_missing_an_argument_is_refused() {
    let mut program = checked(
        "fn main() effects(fs) { let r = fs::write_string(\"a\", \"b\"); }\n",
        Profile::Core,
    );
    for item in &mut program.program.root.items {
        if let ItemKind::Fn(f) = &mut item.kind {
            if let StmtKind::Let(l) = &mut f.body.stmts[0].kind {
                if let Some(Expr {
                    kind: ExprKind::Call { args, .. },
                    ..
                }) = &mut l.init
                {
                    args.clear();
                }
            }
        }
    }
    let message = refusal(&program, &GenOptions::default());
    assert!(message.contains("missing argument #1"), "{message}");
}

/// Debug-build guard (RFC-OWN-FACTS step B): a place recorded on an
/// expression that has no syntactic place is a checker defect.
#[test]
fn an_ownership_place_on_a_non_place_expression_is_refused() {
    let mut program = checked("fn main() { let x = 1 + 2; }\n", Profile::Core);
    let literal = *program
        .facts
        .expr_types
        .keys()
        .find(|id| !program.facts.own.place.contains_key(id))
        .expect("the fixture has an expression without a place");
    program.facts.own.place.insert(
        literal,
        OwnPlace {
            binding: ravum_lang_sema::syntax::span::Span::dummy(),
            steps: Vec::new(),
        },
    );
    let message = refusal(&program, &GenOptions::default());
    assert!(message.contains("has no syntactic place"), "{message}");
}

#[test]
fn a_test_function_with_an_unreportable_return_type_is_refused() {
    let mut program = checked("test fn t() { }\nfn main() { }\n", Profile::Core);
    let mut ret = None;
    for item in &program.program.root.items {
        if let ItemKind::Fn(f) = &item.kind {
            if f.is_test {
                ret = Some(f.ret.id);
            }
        }
    }
    program
        .facts
        .type_nodes
        .insert(ret.expect("the fixture has a test"), Type::I32);
    let options = GenOptions {
        test_mode: true,
        ..GenOptions::default()
    };
    let message = refusal(&program, &options);
    assert!(
        message.contains("must return `()` or `Result<(), Error>`"),
        "{message}"
    );
}

// ---------------------------------------------------------------------------
// Reachable output shapes.
// ---------------------------------------------------------------------------

fn generated(source: &str, options: &GenOptions) -> String {
    generate(&checked(source, Profile::Core), options).expect("code generation must succeed")
}

#[test]
fn text_parsing_is_lowered_for_borrowed_and_literal_text() {
    let code = generated(
        "fn main() { let s = String::from(\"4\"); \
         let n = string::parse_i64(s.as_str()); let o = string::parse_i64(\"5\"); \
         let f = string::parse_f64(\"1.5\"); }\n",
        &GenOptions::default(),
    );
    assert!(code.contains("parse::<i64>()"), "{code}");
    assert!(code.contains("parse::<f64>()"), "{code}");
}

#[test]
fn an_empty_crate_name_falls_back_to_the_default_banner() {
    let code = generated(
        "fn main() { }\n",
        &GenOptions {
            crate_name: String::new(),
            ..GenOptions::default()
        },
    );
    assert!(code.contains("ravum_program"), "{code}");
}
