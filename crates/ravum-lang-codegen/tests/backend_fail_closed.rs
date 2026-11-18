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
    CallTarget, CheckOptions, CheckedProgram, MethodTarget, OwnPlace, Profile, Resolution,
};
use ravum_lang_sema::syntax::ast::{Expr, ExprKind, ItemKind, ParamKind, Receiver, StmtKind};
use ravum_lang_sema::syntax::span::Span;
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
            binding: Span::dummy(),
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

#[test]
fn a_builtin_constant_other_than_none_is_refused() {
    let mut program = checked(
        "fn main() { let o: Option<i32> = None; drop(o); }\n",
        Profile::Core,
    );
    for resolution in program.facts.resolutions.values_mut() {
        if let Resolution::BuiltinConst { path } = resolution {
            *path = "math::PI".to_string();
        }
    }
    let message = refusal(&program, &GenOptions::default());
    assert!(
        message.contains("unsupported builtin constant `math::PI`"),
        "{message}"
    );
}

#[test]
fn an_extern_function_with_a_receiver_is_refused() {
    let mut program = checked(
        "extern \"C\" { fn c_abs(x: i32) -> i32; }\nfn main() { }\n",
        Profile::Core,
    );
    for item in &mut program.program.root.items {
        if let ItemKind::Extern(block) = &mut item.kind {
            block.fns[0].params[0].kind = ParamKind::Receiver(Receiver::Shared);
        }
    }
    let message = refusal(&program, &GenOptions::default());
    assert!(
        message.contains("`self` is not allowed in an extern function"),
        "{message}"
    );
}

/// Without a source table, markers and guard messages fall back to a neutral
/// location instead of indexing a file that does not exist.
#[test]
fn a_program_without_source_files_still_generates_and_locates_neutrally() {
    let mut program = checked("fn main() { let x = 1 + 2; }\n", Profile::Core);
    program.files.clear();
    let code = generate(&program, &GenOptions::default()).expect("generation succeeds");
    assert!(!code.contains("// rv source:"), "{code}");
    assert!(code.contains("// rv: <unknown>:0"), "{code}");

    // A span without a position produces no marker at all.
    for item in &mut program.program.root.items {
        item.span = Span::dummy();
    }
    let code = generate(&program, &GenOptions::default()).expect("generation succeeds");
    assert!(code.contains("fn main"), "{code}");

    let literal = *program
        .facts
        .expr_types
        .keys()
        .find(|id| !program.facts.own.place.contains_key(id))
        .expect("the fixture has an expression without a place");
    program.facts.own.place.insert(
        literal,
        OwnPlace {
            binding: Span::dummy(),
            steps: Vec::new(),
        },
    );
    let message = refusal(&program, &GenOptions::default());
    assert!(message.contains("unknown-file"), "{message}");
}

#[test]
fn an_item_other_than_a_function_inside_an_impl_is_refused() {
    let mut program = checked(
        "const C: i32 = 1;\nstruct S { }\nimpl S { fn f(&self) { } }\nfn main() { }\n",
        Profile::Core,
    );
    let constant = program.program.root.items[0].clone();
    for item in &mut program.program.root.items {
        if let ItemKind::Impl(block) = &mut item.kind {
            block.items.push(constant.clone());
        }
    }
    let message = refusal(&program, &GenOptions::default());
    assert!(
        message.contains("unsupported item inside `impl`"),
        "{message}"
    );
}

#[test]
fn a_let_without_initializer_or_known_type_is_refused() {
    let mut program = checked("fn main() { let x: i32; x = 1; drop(x); }\n", Profile::Core);
    for item in &mut program.program.root.items {
        if let ItemKind::Fn(f) = &mut item.kind {
            if let StmtKind::Let(l) = &mut f.body.stmts[0].kind {
                l.ty = None;
                program.facts.pat_types.remove(&l.pat.id);
            }
        }
    }
    let message = refusal(&program, &GenOptions::default());
    assert!(
        message.contains("`let` without initializer requires a type annotation"),
        "{message}"
    );
}

/// The index and the item emitter fall back on the syntax when a fact table
/// is partial: return kinds from the signature or the written type, test
/// order from the source, a constant from its literal.
#[test]
fn partial_item_facts_fall_back_on_the_syntax() {
    let mut program = checked(
        "const C: i32 = 7;\ntest fn t() { }\ntest fn u() -> Result<(), Error> { return Ok(()); }\n\
         fn main() { let x = C; drop(x); }\n",
        Profile::Core,
    );
    let mut rets = Vec::new();
    for item in &program.program.root.items {
        if let ItemKind::Fn(f) = &item.kind {
            // Only `()` returns: a written type still needs its resolved type
            // to be rendered at all.
            if f.name.name != "u" {
                rets.push((item.id, f.ret.id));
            }
        }
    }
    program.facts.test_fns.clear();
    program.facts.consts.clear();
    for (_, ret) in &rets {
        program.facts.type_nodes.remove(ret);
    }
    let options = GenOptions {
        test_mode: true,
        ..GenOptions::default()
    };
    let code = generate(&program, &options).expect("generation from the signatures");
    assert!(code.contains("7"), "{code}");
    for (item, _) in &rets {
        program.facts.fn_sigs.remove(item);
    }
    let code = generate(&program, &options).expect("generation from the syntax");
    assert!(code.contains("fn t"), "{code}");
}

#[test]
fn a_struct_literal_or_rooted_path_without_resolution_is_refused() {
    let mut program = checked(
        "struct P { n: i32 }\nfn main() { let p = P { n: 1 }; drop(p); }\n",
        Profile::Core,
    );
    let mut literal_path = None;
    for item in &program.program.root.items {
        if let ItemKind::Fn(f) = &item.kind {
            if let StmtKind::Let(l) = &f.body.stmts[0].kind {
                if let Some(Expr {
                    kind: ExprKind::StructLit { path, .. },
                    ..
                }) = &l.init
                {
                    literal_path = Some(path.id);
                }
            }
        }
    }
    program
        .facts
        .resolutions
        .remove(&literal_path.expect("the fixture has a struct literal"));
    let message = refusal(&program, &GenOptions::default());
    assert!(
        message.contains("missing resolution fact for struct literal path `P`"),
        "{message}"
    );

    // A rooted literal path whose resolution is not a struct falls back on
    // the written path, which a rooted path cannot be.
    let mut program = checked(
        "struct P { n: i32 }\nfn main() { let p = crate::P { n: 1 }; drop(p); }\n",
        Profile::Core,
    );
    let mut literal_path = None;
    for item in &program.program.root.items {
        if let ItemKind::Fn(f) = &item.kind {
            if let StmtKind::Let(l) = &f.body.stmts[0].kind {
                if let Some(Expr {
                    kind: ExprKind::StructLit { path, .. },
                    ..
                }) = &l.init
                {
                    literal_path = Some(path.id);
                }
            }
        }
    }
    program.facts.resolutions.insert(
        literal_path.expect("the fixture has a struct literal"),
        Resolution::Module,
    );
    let message = refusal(&program, &GenOptions::default());
    assert!(
        message.contains("cannot emit rooted path `crate::P`"),
        "{message}"
    );
}

/// Ownership facts recorded on a node the program does not contain are
/// ignored by the debug guard (they cannot disagree with any emitted code).
#[test]
fn ownership_facts_on_unknown_nodes_are_ignored() {
    let mut program = checked(
        "fn main() { let s = String::from(\"a\"); let t = s; drop(t); }\n",
        Profile::Core,
    );
    let stray = u64::MAX - 7;
    program.facts.own.place.insert(
        stray,
        OwnPlace {
            binding: Span::dummy(),
            steps: Vec::new(),
        },
    );
    if let Some(binding) = program.facts.own.binding_identity.values().next().cloned() {
        program.facts.own.binding_identity.insert(stray, binding);
    }
    generate(&program, &GenOptions::default()).expect("stray facts are ignored");
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

/// A variant index the enum does not have is refused in every position (value
/// path, pattern, variant pattern, constructor call) instead of falling back
/// to the source text of the path.
#[test]
fn a_variant_index_outside_its_enum_is_refused() {
    let source = "enum E { A, B(i32) }\n\
                  fn main() {\n\
                  let a = E::A;\n\
                  let b = E::B(1);\n\
                  let n = match a { E::A => 0, E::B(x) => x };\n\
                  let m = match b { E::B(x) => x, _ => 0 };\n\
                  }\n";
    let program = checked(source, Profile::Core);
    // Break the resolutions (value paths and patterns) and the calls apart.
    let mut broken = program.clone();
    for resolution in broken.facts.resolutions.values_mut() {
        if let Resolution::Variant { variant, .. } = resolution {
            *variant = 99;
        }
    }
    let message = refusal(&broken, &GenOptions::default());
    assert!(message.contains("unknown variant 99"), "{message}");
    let mut broken = program;
    for target in broken.facts.calls.values_mut() {
        if let CallTarget::VariantCtor { variant, .. } = target {
            *variant = 99;
        }
    }
    let message = refusal(&broken, &GenOptions::default());
    assert!(message.contains("unknown variant 99"), "{message}");
}

/// A member of an `impl` that is not a function (a hand-built AST past the
/// checker) is refused instead of being skipped.
#[test]
fn a_non_function_member_of_an_impl_is_refused() {
    let mut program = checked(
        "struct S { n: i32 }\nconst K: i32 = 1;\nimpl S { fn get(&self) -> i32 { return self.n; } }\nfn main() { }\n",
        Profile::Core,
    );
    let constant = program.program.root.items[1].clone();
    let ItemKind::Impl(imp) = &mut program.program.root.items[2].kind else {
        panic!("expected the impl");
    };
    imp.items.push(constant);
    let message = refusal(&program, &GenOptions::default());
    assert!(
        message.contains("unsupported item inside `impl`"),
        "{message}"
    );
}
