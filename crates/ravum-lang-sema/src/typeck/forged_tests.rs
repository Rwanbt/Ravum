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
    let n = p.get() + q.get();
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
    for callee in ["f", "get", "make"] {
        let _ = typecheck_broken(|facts| facts.fn_sigs.retain(|_, sig| sig.name != callee));
    }
}
