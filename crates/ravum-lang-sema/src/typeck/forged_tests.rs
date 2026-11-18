//! Typeck trusts the tables `resolve` fills: a signature for every function
//! and method, an entry for every struct and enum, a Rust path for every
//! item. Each test resolves a real program, empties one table the way a
//! defect upstream would, and requires typeck to finish without panicking;
//! it answers `Type::Error` where it cannot type a use, never a guess.

use crate::facts::Facts;
use crate::resolve::{self, World};
use ravum_lang_syntax::diag::DiagnosticBag;
use ravum_lang_syntax::source::SourceMap;

const SOURCE: &str = "\
struct P<T> { v: T }
impl<T> P<T> {
    fn get(&self) -> i32 { return 1; }
    fn add(&self, k: i32) -> i32 { return k; }
    fn make(v: T) -> P<T> { return P { v: v }; }
}
enum E { A, B(i32), C { x: i32 } }
const K: i32 = 2;
fn f(e: E) -> i32 {
    let p = P { v: 1 };
    let q = P::make(2);
    let a = E::A;
    let b = E::B(K);
    let c = E::C { x: 3 };
    let n = p.get() + q.add(1);
    match e { E::A => n, E::B(v) => v, E::C { x } => x }
}
fn main() { let r = f(E::A); }
test fn t() { }
";

/// Typeck of `SOURCE` after `breaks` altered the resolved facts.
fn typecheck_broken(breaks: impl FnOnce(&mut Facts)) -> bool {
    let mut sources = SourceMap::new();
    let file = sources.add("src/main.rv", SOURCE);
    let parsed = ravum_lang_syntax::parser::parse_file(file, SOURCE);
    assert!(parsed.diags.is_empty(), "the fixture must parse");
    let mut world = World::build(&parsed.program);
    let mut facts = Facts::default();
    let mut diags = DiagnosticBag::new();
    assert!(resolve::resolve_with_world(
        &mut world, &mut facts, &mut diags
    ));
    assert!(!diags.has_errors(), "the fixture must resolve");
    breaks(&mut facts);
    super::typecheck_with_world(
        &parsed.program,
        &mut facts,
        &world,
        Some(&sources),
        &mut diags,
    ) && !diags.has_errors()
}

#[test]
fn the_intact_fixture_checks() {
    assert!(typecheck_broken(|_| {}));
}

#[test]
fn typeck_survives_missing_signatures() {
    // Calls, method calls, `main`, the test and every body lose their
    // signature: nothing is typed against an invented one.
    let _ = typecheck_broken(|facts| facts.fn_sigs.clear());
}

#[test]
fn typeck_survives_missing_struct_and_enum_entries() {
    let _ = typecheck_broken(|facts| facts.structs.clear());
    let _ = typecheck_broken(|facts| facts.enums.clear());
    let _ = typecheck_broken(|facts| {
        for info in facts.enums.values_mut() {
            info.variants.clear();
        }
    });
}

#[test]
fn a_constant_without_a_recorded_path_gets_its_module_path() {
    let mut sources = SourceMap::new();
    let file = sources.add("src/main.rv", SOURCE);
    let parsed = ravum_lang_syntax::parser::parse_file(file, SOURCE);
    let mut world = World::build(&parsed.program);
    let mut facts = Facts::default();
    let mut diags = DiagnosticBag::new();
    assert!(resolve::resolve_with_world(
        &mut world, &mut facts, &mut diags
    ));
    facts.item_paths.clear();
    assert!(super::typecheck_with_world(
        &parsed.program,
        &mut facts,
        &world,
        Some(&sources),
        &mut diags
    ));
    let paths: Vec<&str> = facts
        .consts
        .values()
        .map(|c| c.rust_path.as_str())
        .collect();
    assert_eq!(paths.len(), 1);
    assert!(paths[0].ends_with("K"), "{paths:?}");
}

#[test]
fn typeck_survives_a_callee_without_its_signature() {
    // Only the called function and methods lose their signature, so the
    // bodies that call them are still typed.
    for callee in ["f", "get", "add", "make"] {
        let _ = typecheck_broken(|facts| facts.fn_sigs.retain(|_, sig| sig.name != callee));
    }
}

/// A fresh checker over `SOURCE`, outside any function.
fn with_checker(test: impl FnOnce(&mut super::TypeChecker)) {
    let mut sources = SourceMap::new();
    let file = sources.add("src/main.rv", SOURCE);
    let parsed = ravum_lang_syntax::parser::parse_file(file, SOURCE);
    let mut world = World::build(&parsed.program);
    let mut facts = Facts::default();
    let mut diags = DiagnosticBag::new();
    assert!(resolve::resolve_with_world(
        &mut world, &mut facts, &mut diags
    ));
    let mut checker = super::TypeChecker::new(&parsed.program, &mut facts, &world, &mut diags);
    test(&mut checker);
}

#[test]
#[should_panic(expected = "invariant: a local is bound inside an open scope")]
fn binding_a_local_outside_every_scope_is_an_invariant_failure() {
    with_checker(|checker| {
        checker.bind_local(
            "x",
            crate::types::Type::I32,
            false,
            false,
            ravum_lang_syntax::span::Span::dummy(),
        )
    });
}

#[test]
#[should_panic(expected = "invariant: an inference variable is created before it is bound")]
fn binding_an_inference_variable_never_created_is_an_invariant_failure() {
    with_checker(|checker| {
        checker.bind_var(7, &crate::types::Type::I32);
    });
}

#[test]
fn the_occurs_check_follows_bound_variables() {
    // `$0 := Vec<$1>`: `$1` occurs in `$0`, through the binding of `$0`.
    with_checker(|checker| {
        let span = ravum_lang_syntax::span::Span::dummy();
        let first = checker.fresh_var(span);
        let second = checker.fresh_var(span);
        checker.vars[0] = Some(crate::types::Type::Vec(Box::new(second)));
        assert!(checker.var_occurs(1, &first));
        assert!(!checker.var_occurs(2, &first));
    });
}

#[test]
fn a_dereference_of_a_non_reference_is_not_a_mutable_place() {
    // Typeck refuses `*e` unless `e` is a reference; should a non-reference
    // type still be recorded, the place is read as immutable, never mutable.
    use ravum_lang_syntax::ast::{Expr, ExprKind, UnOp};
    let span = ravum_lang_syntax::span::Span::dummy();
    let inner = Expr {
        id: 4_000_000_001,
        kind: ExprKind::Int {
            value: 1,
            suffix: None,
        },
        span,
    };
    let deref = Expr {
        id: 4_000_000_002,
        kind: ExprKind::Unary {
            op: UnOp::Deref,
            expr: Box::new(inner),
        },
        span,
    };
    with_checker(|checker| {
        checker
            .facts
            .expr_types
            .insert(4_000_000_001, crate::types::Type::I32);
        assert_eq!(checker.place_mutability(&deref), Some(false));
    });
}

#[test]
fn a_method_call_on_a_non_nominal_receiver_binds_no_impl_parameter() {
    // Method entries come from `impl` blocks on nominal types, and typeck
    // looks a method up on the receiver's nominal type; should another type
    // reach the call, no impl parameter is bound from it.
    use ravum_lang_syntax::ast::{Expr, ExprKind};
    let span = ravum_lang_syntax::span::Span::dummy();
    let receiver = Expr {
        id: 4_000_000_011,
        kind: ExprKind::Int {
            value: 1,
            suffix: None,
        },
        span,
    };
    with_checker(|checker| {
        let entry = checker
            .world
            .impls
            .values()
            .flatten()
            .find(|entry| entry.name == "get")
            .cloned()
            .expect("the `get` method");
        let (item, _) =
            checker.check_user_method_call(&entry, &crate::types::Type::I32, &receiver, &[], &span);
        assert_eq!(item, entry.item);
    });
}

#[test]
fn a_method_call_without_a_target_is_not_static_text() {
    // `"x".as_str()` is static text through `str::as_str`; once its method
    // fact is gone, the classification refuses to call it static.
    let src = "fn g() -> &str { return \"x\".as_str(); }\n";
    let mut sources = SourceMap::new();
    let file = sources.add("src/lib.rv", src);
    let parsed = ravum_lang_syntax::parser::parse_file(file, src);
    let mut world = World::build(&parsed.program);
    let mut facts = Facts::default();
    let mut diags = DiagnosticBag::new();
    assert!(resolve::resolve_with_world(
        &mut world, &mut facts, &mut diags
    ));
    assert!(super::typecheck_with_world(
        &parsed.program,
        &mut facts,
        &world,
        Some(&sources),
        &mut diags
    ));
    assert_eq!(facts.static_str_fns.len(), 1);
    facts.methods.clear();
    facts.static_str_fns.clear();
    super::compute_static_str_fns(&parsed.program, &mut facts);
    assert!(facts.static_str_fns.is_empty());
}

#[test]
fn try_and_return_outside_a_function_are_refused() {
    // `check_const_expr` never hands `?` or `return` to these checks, and every
    // body sets `current_fn`; outside a function they refuse instead of typing
    // against an absent return type.
    use ravum_lang_syntax::ast::{Expr, ExprKind};
    let span = ravum_lang_syntax::span::Span::dummy();
    let value = Expr {
        id: 4_000_000_021,
        kind: ExprKind::Int {
            value: 1,
            suffix: None,
        },
        span,
    };
    with_checker(|checker| {
        assert_eq!(checker.check_try(&value, &value), crate::types::Type::Error);
        assert_eq!(
            checker.check_return(Some(&value)),
            crate::types::Type::Never
        );
        assert_eq!(checker.check_return(None), crate::types::Type::Never);
        let codes: Vec<&str> = checker.diags.iter().map(|d| d.code.as_str()).collect();
        assert_eq!(codes, ["E-TYP-TRY", "E-TYP-RETURN", "E-TYP-RETURN"]);
    });
}

#[test]
fn impl_generics_bind_only_from_a_nominal_target_with_parameters() {
    // `resolve_impl` admits only a nominal target whose arguments are the impl
    // parameters; any other entry binds nothing.
    use crate::types::Type;
    with_checker(|checker| {
        let mut entry = checker
            .world
            .impls
            .values()
            .flatten()
            .find(|entry| entry.name == "get")
            .cloned()
            .expect("the `get` method");
        let mut subst = std::collections::BTreeMap::new();
        checker.bind_impl_generics(&entry, &[Type::I32], &mut subst);
        assert_eq!(subst.get(&0), Some(&Type::I32));
        let mut subst = std::collections::BTreeMap::new();
        entry.self_ty = Type::I32;
        checker.bind_impl_generics(&entry, &[Type::I32], &mut subst);
        assert!(subst.is_empty());
        if let Type::Nominal { args, .. } = &mut checker
            .world
            .impls
            .values()
            .flatten()
            .find(|entry| entry.name == "get")
            .cloned()
            .expect("the `get` method")
            .self_ty
        {
            args[0] = Type::Bool;
            let mut pinned = entry.clone();
            pinned.self_ty = Type::Nominal {
                def: 0,
                name: "P".to_string(),
                args: args.clone(),
            };
            let mut subst = std::collections::BTreeMap::new();
            checker.bind_impl_generics(&pinned, &[Type::I32], &mut subst);
            assert!(subst.is_empty());
        }
    });
}
