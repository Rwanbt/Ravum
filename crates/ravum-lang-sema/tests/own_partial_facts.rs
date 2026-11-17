//! The ownership phase is total: `check_ownership` is a public entry point, and
//! it must neither panic nor invent a diagnostic when the fact tables are
//! incomplete or when the tree is one the type checker would have refused
//! (OWN-11.1 keeps such programs away from it inside `check`, not elsewhere).
//!
//! Each test pins what the phase reports on such an input.

use ravum_lang_sema::check_ownership;
use ravum_lang_sema::facts::{CheckOptions, CheckedProgram, Facts, Profile};
use ravum_lang_sema::syntax::diag::DiagnosticBag;
use ravum_lang_sema::syntax::{parser, source::SourceMap};

/// Ownership diagnostics for a tree that parses, analysed with no facts at
/// all: every type is unknown, every call unresolved.
fn without_facts(source: &str) -> Vec<String> {
    let mut sources = SourceMap::new();
    let file = sources.add("main.rv", source);
    let parsed = parser::parse_file(file, source);
    assert!(parsed.diags.is_empty(), "the fixture must parse");
    let checked = CheckedProgram {
        program: parsed.program,
        facts: Facts::default(),
        files: Vec::new(),
        profile: Profile::Core,
    };
    let mut diags = DiagnosticBag::new();
    check_ownership(&checked, &mut diags);
    diags.iter().map(|diag| diag.code.clone()).collect()
}

/// Ownership diagnostics for a program that checks, re-analysed after `strip`
/// removed part of its facts.
fn with_stripped_facts(source: &str, strip: fn(&mut Facts)) -> Vec<String> {
    let mut sources = SourceMap::new();
    let file = sources.add("main.rv", source);
    let parsed = parser::parse_file(file, source);
    assert!(parsed.diags.is_empty(), "the fixture must parse");
    let result = ravum_lang_sema::check(&parsed.program, &sources, &CheckOptions::default());
    let mut checked = result.checked.expect("the fixture must check");
    strip(&mut checked.facts);
    let mut diags = DiagnosticBag::new();
    check_ownership(&checked, &mut diags);
    diags.iter().map(|diag| diag.code.clone()).collect()
}

const NONE: [&str; 0] = [];

const METHODS: &str = "struct S { n: String }\n\
    impl S { fn a(self) -> String { self.n } fn b(&self) -> usize { self.n.len() } \
    fn c(&mut self) { self.n = String::from(\"x\"); } }\n\
    fn free(s: String) -> String { let t = s; t }\n\
    fn keep(s: String) { let a = s; }\n fn main() { }";
const CALLS: &str = "fn g(a: String, b: &String) { }\n\
    fn f(s: String, t: String) { g(s, &t); let n = t.len(); }\n fn main() { }";

/// Without `fn_sigs` the parameters are rebuilt from the syntax: receivers of
/// the three kinds, typed bindings with and without their resolved type.
#[test]
fn a_missing_signature_table_falls_back_on_the_syntax() {
    assert_eq!(with_stripped_facts(METHODS, |f| f.fn_sigs.clear()), NONE);
    assert_eq!(
        with_stripped_facts(METHODS, |f| {
            f.fn_sigs.clear();
            f.type_nodes.clear();
        }),
        NONE
    );
}

/// An unresolved call or method call evaluates its arguments and reports
/// nothing about a callee it does not know.
#[test]
fn unresolved_calls_are_analysed_without_a_verdict_on_the_callee() {
    assert_eq!(
        with_stripped_facts(CALLS, |f| {
            f.calls.clear();
            f.methods.clear();
        }),
        NONE
    );
}

/// Without resolutions a single-segment path is still looked up as a local;
/// without expression or pattern types every value is of unknown type, and an
/// unknown type is never reported as moved.
#[test]
fn missing_resolutions_and_types_never_produce_a_move_error() {
    assert_eq!(with_stripped_facts(CALLS, |f| f.resolutions.clear()), NONE);
    assert_eq!(
        with_stripped_facts(METHODS, |f| f.resolutions.clear()),
        NONE
    );
    for fixture in [CALLS, METHODS] {
        assert_eq!(
            with_stripped_facts(fixture, |f| {
                f.expr_types.clear();
                f.pat_types.clear();
            }),
            NONE
        );
    }
    assert_eq!(with_stripped_facts(METHODS, |f| f.structs.clear()), NONE);
}

/// Trees the type checker refuses: a jump outside a loop, a diverging operand
/// of `&&`, a call whose callee is not a path, a loop whose header diverges.
#[test]
fn trees_refused_by_the_type_checker_are_walked_without_a_diagnostic() {
    for source in [
        "fn f() { break; continue; }",
        "fn f(c: bool) -> bool { (return true) && c }",
        "fn g() -> i32 { 1 }\nfn f() { let a = g()(); }",
        "fn f() -> i32 { while (return 1) { break; } 2 }",
        "fn f() -> i32 { for i in (return 1) { break; } 2 }",
        "struct S { n: i32 }\nimpl S { fn a(self) -> i32 { self.n } fn b(&self) -> i32 { self.n } \
         fn c(&mut self) { self.n = 1; } }",
    ] {
        assert_eq!(without_facts(source), NONE, "{source}");
    }
}

/// With no facts every operand is of unknown type, so nothing is a move; the
/// borrows are syntactic and their conflicts are still seen.
#[test]
fn syntactic_borrow_conflicts_are_reported_even_without_types() {
    let codes = without_facts(
        "fn f(s: i32) { let a = s; g(a, &a, a.b); a.m(a, &a); h(a)(a); let v = a[a]; \
         let d = *a; let e = a.0; let t = [a; 3]; a = 2; a += 1; }",
    );
    assert_eq!(codes, ["E-OWN-BORROW-CONFLICT", "E-OWN-BORROW-CONFLICT"]);
}
