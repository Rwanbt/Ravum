use super::{
    check_ownership, check_stored_reference_fields, collect_fns, publish_copy_type, OwnChecker,
};
use crate::facts::Facts;
use crate::facts::{CheckOptions, CheckResult, CheckedProgram, OwnFacts, Profile};
use crate::loader::{load_package_with_options, LoadOptions};
use crate::types::Type;
use ravum_lang_syntax::ast::{FnItem, Item};
use ravum_lang_syntax::diag::DiagnosticBag;
use ravum_lang_syntax::parser;
use ravum_lang_syntax::source::SourceMap;
use std::path::{Path, PathBuf};

/// Parse and check a complete program. The parser/checker are real: parse
/// errors are a test failure, and ownership diagnostics only exist when the
/// earlier phases succeeded (OWN-11.1).
fn run(src: &str) -> CheckResult {
    run_at("src/lib.rv", src)
}

/// Same, as a binary crate (the conformance corpus uses `src/main.rv`).
fn run_bin(src: &str) -> CheckResult {
    run_at("src/main.rv", src)
}

fn run_at(path: &str, src: &str) -> CheckResult {
    let mut sources = SourceMap::new();
    let file = sources.add(path, src);
    let parsed = parser::parse_file(file, src);
    let parse_codes: Vec<String> = parsed.diags.iter().map(|d| d.code.clone()).collect();
    assert!(
        parsed.diags.is_empty(),
        "unexpected parse diagnostics {parse_codes:?}:\n{}",
        parsed.diags.render(&sources)
    );
    let options = CheckOptions {
        profile: Profile::Core,
        edition: ravum_lang_syntax::LANGUAGE_EDITION.to_string(),
    };
    crate::check::check(&parsed.program, &sources, &options)
}

fn own_codes(result: &CheckResult) -> Vec<String> {
    result
        .diags
        .iter()
        .filter(|d| d.code.starts_with("E-OWN-"))
        .map(|d| d.code.clone())
        .collect()
}

fn failures(result: &CheckResult) -> Vec<(String, String)> {
    result
        .diags
        .iter()
        .map(|d| (d.code.clone(), d.message.clone()))
        .collect()
}

/// The program is accepted and produces no ownership diagnostic.
fn accepted(src: &str) {
    let result = run(src);
    let codes = own_codes(&result);
    assert!(
        codes.is_empty(),
        "unexpected ownership diagnostics {codes:?}:\n{:?}",
        failures(&result)
    );
    assert!(
        result.checked.is_some(),
        "expected a checked program, got {:?}",
        failures(&result)
    );
}

/// The program is rejected and the exact ownership code is present.
fn rejected(src: &str, code: &str) {
    let result = run(src);
    assert!(
        result.checked.is_none(),
        "expected rejection for {code}, but the program was accepted"
    );
    assert!(
        result.diags.iter().any(|d| d.code == code),
        "expected {code}, got {:?}",
        failures(&result)
    );
}

// -- Use after move / double move / move through calls ------------------

#[test]
fn slices_are_mobile_because_own_2_1_does_not_list_them() {
    // OWN-2.1: Copy = primitives, references, tuples/arrays of Copy, structs and
    // enums whose fields are all Copy. OWN-2.2: everything else is mobile.
    // A slice was classified Copy by this module while the backend classified it
    // as mobile; the RFC-OWN-FACTS-2 step-C1 guard surfaced the drift.
    let facts = Facts::default();
    assert!(!super::is_copy_type(
        &Type::Slice(Box::new(Type::I32)),
        &facts
    ));
    // A reference to a slice stays Copy, so `&[i32]` keeps working.
    assert!(super::is_copy_type(
        &Type::Ref {
            mutable: false,
            inner: Box::new(Type::Slice(Box::new(Type::I32))),
        },
        &facts
    ));
}

#[test]
fn use_after_move_is_rejected() {
    rejected(
        r#"
fn f(s: String) -> usize {
    let t = s;
    return s.len();
}
"#,
        "E-OWN-USE-AFTER-MOVE",
    );
}

#[test]
fn double_move_is_rejected() {
    rejected(
        r#"
fn f(s: String) {
    let a = s;
    let b = s;
}
"#,
        "E-OWN-DOUBLE-MOVE",
    );
}

#[test]
fn move_into_call_then_use_is_rejected() {
    rejected(
        r#"
fn g(t: String) {}
fn f(s: String) {
    g(s);
    g(s);
}
"#,
        "E-OWN-USE-AFTER-MOVE",
    );
}

#[test]
fn copy_values_can_be_reused() {
    accepted(
        r#"
fn f(x: i32) -> i32 {
    let a = x;
    let b = x;
    return a + b;
}
"#,
    );
}

#[test]
fn copy_struct_can_be_reused() {
    accepted(
        r#"
struct Point { x: i32, y: i32 }
fn f(p: Point) -> i32 {
    let q = p;
    return p.x + q.y;
}
"#,
    );
}

#[test]
fn reassignment_after_move_is_allowed() {
    accepted(
        r#"
fn f() -> String {
    let mut s = String::from("a");
    let t = s;
    s = String::from("b");
    return t;
}
"#,
    );
}

// -- Borrow conflicts ----------------------------------------------------

#[test]
fn exclusive_borrow_conflicts_with_read() {
    rejected(
        r#"
fn h(a: &mut i32, b: i32) {}
fn f() {
    let mut x = 1;
    h(&mut x, x);
}
"#,
        "E-OWN-BORROW-CONFLICT",
    );
}

#[test]
fn two_exclusive_arguments_conflict() {
    rejected(
        r#"
fn h(a: &mut i32, b: &mut i32) {}
fn f() {
    let mut x = 1;
    h(&mut x, &mut x);
}
"#,
        "E-OWN-BORROW-CONFLICT",
    );
}

#[test]
fn shared_and_exclusive_arguments_conflict() {
    rejected(
        r#"
fn h(a: &i32, b: &mut i32) {}
fn f() {
    let mut x = 1;
    h(&x, &mut x);
}
"#,
        "E-OWN-BORROW-CONFLICT",
    );
}

#[test]
fn move_during_shared_borrow_conflicts() {
    rejected(
        r#"
fn h(a: &String, b: String) {}
fn f() {
    let s = String::from("a");
    h(&s, s);
}
"#,
        "E-OWN-BORROW-CONFLICT",
    );
}

#[test]
fn two_shared_arguments_are_allowed() {
    accepted(
        r#"
fn h(a: &i32, b: &i32) -> i32 { return *a + *b; }
fn f() -> i32 {
    let x = 1;
    return h(&x, &x);
}
"#,
    );
}

#[test]
fn disjoint_fields_can_be_borrowed_exclusively() {
    accepted(
        r#"
struct P { x: i32, y: i32 }
fn h(a: &mut i32, b: &mut i32) {}
fn f() {
    let mut p = P { x: 1, y: 2 };
    h(&mut p.x, &mut p.y);
}
"#,
    );
}

#[test]
fn sequential_method_calls_do_not_overlap() {
    accepted(
        r#"
fn f() {
    let mut v: Vec<i32> = Vec::new();
    v.push(1);
    v.push(2);
}
"#,
    );
}

// -- Reference escape ----------------------------------------------------

#[test]
fn reference_cannot_be_stored_in_a_let() {
    rejected(
        r#"
fn f() {
    let x = 1;
    let r = &x;
}
"#,
        "E-OWN-REF-ESCAPE",
    );
}

#[test]
fn reference_parameter_cannot_be_returned() {
    rejected(
        r#"
fn f(x: &i32) -> &i32 { return x; }
"#,
        "E-OWN-REF-ESCAPE",
    );
}

#[test]
fn reference_parameter_cannot_be_stored() {
    rejected(
        r#"
fn f(x: &i32) {
    let y = x;
}
"#,
        "E-OWN-REF-ESCAPE",
    );
}

#[test]
fn as_str_cannot_be_stored() {
    rejected(
        r#"
fn f(s: String) {
    let t = s.as_str();
}
"#,
        "E-OWN-REF-ESCAPE",
    );
}

#[test]
fn string_literal_is_static_and_storable() {
    accepted(
        r#"
fn f() -> &str {
    let l = "litteral";
    return l;
}
"#,
    );
}

#[test]
fn temporary_reference_in_call_is_allowed() {
    accepted(
        r#"use std::io;

fn f(s: String) {
    io::println(s.as_str());
}
"#,
    );
}

#[test]
fn push_str_copies_the_pointee_and_accepts_a_temporary_reference() {
    accepted(
        r#"
fn f(mut s: String, t: String) {
    s.push_str(t.as_str());
}
"#,
    );
}

// -- Provenance interprocédurale du texte statique (finding F1) -----------

#[test]
fn static_text_from_function_is_storable() {
    accepted(
        r#"
fn lit() -> &str { "a" }
fn f() -> &str {
    let text = lit();
    return text;
}
"#,
    );
}

#[test]
fn static_text_from_return_only_function_is_storable() {
    // The `lit` body has no tail expression: every explicit return is
    // static, which is enough for the fixpoint.
    accepted(
        r#"
fn lit() -> &str { return "a"; }
fn f() -> &str {
    let text = lit();
    return text;
}
"#,
    );
}

#[test]
fn static_text_from_function_with_if_is_storable() {
    accepted(
        r#"
fn via_if(c: bool) -> &str {
    if c { "a" } else { "b" }
}
fn f(c: bool) -> &str {
    let text = via_if(c);
    return text;
}
"#,
    );
}

#[test]
fn static_text_from_function_with_match_is_storable() {
    accepted(
        r#"
fn via_match(c: bool) -> &str {
    match c {
        true => "a",
        false => "b",
    }
}
fn f(c: bool) -> &str {
    let text = via_match(c);
    return text;
}
"#,
    );
}

#[test]
fn non_static_text_function_stays_refused() {
    rejected(
        r#"
fn bad(s: &str) -> &str { s }
fn f(s: &str) -> &str {
    let text = bad(s);
    return text;
}
"#,
        "E-OWN-REF-ESCAPE",
    );
}

#[test]
fn recursive_text_function_is_not_classified_static() {
    // No static base: the fixpoint must never add `rec`, so both its own
    // return and the caller's `let` are conservatively refused.
    rejected(
        r#"
fn rec() -> &str { rec() }
fn f() -> &str {
    let text = rec();
    return text;
}
"#,
        "E-OWN-REF-ESCAPE",
    );
}

// -- D13: projected text is static by construction -----------------------

#[test]
fn static_text_array_element_can_be_returned() {
    // RFC-D13-STATICITY rule 1: the element of a `[&str; 2]` is a
    // `&'static str` because rule 2 forbids storing a borrowed `&str` there.
    accepted(
        r#"
fn pick(a: [&str; 2]) -> &str {
    return a[1];
}
"#,
    );
}

#[test]
fn static_text_array_element_can_be_bound_then_returned() {
    accepted(
        r#"
fn pick(a: [&str; 2]) -> &str {
    let s = a[1];
    return s;
}
"#,
    );
}

#[test]
fn static_text_struct_field_can_be_returned() {
    accepted(
        r#"
struct Card { text: &str }
fn label(c: Card) -> &str {
    return c.text;
}
"#,
    );
}

#[test]
fn static_text_vec_element_can_be_returned() {
    accepted(
        r#"
fn pick(v: Vec<&str>) -> &str {
    return v[0];
}
"#,
    );
}

#[test]
fn static_text_option_payload_can_be_unwrapped() {
    accepted(
        r#"
fn pick(o: Option<&str>) -> &str {
    return o.unwrap();
}
"#,
    );
}

#[test]
fn static_text_match_payload_can_be_returned() {
    accepted(
        r#"
fn pick(o: Option<&str>) -> &str {
    match o {
        Some(t) => { return t; }
        None => { return "none"; }
    }
}
"#,
    );
}

#[test]
fn str_as_str_identity_of_static_text_is_storable() {
    // `str::as_str` is the identity on a `&str`, so a static receiver stays
    // static (unlike `String::as_str`, which borrows a local `String`).
    accepted(
        r#"
fn f() -> &str {
    let t = "x";
    return t.as_str();
}
"#,
    );
}

#[test]
fn string_as_str_in_an_array_stays_refused() {
    // Rule 2 (negative): the storage site that makes rule 1 true.
    rejected(
        r#"
fn f(s: String) {
    let a = [s.as_str()];
}
"#,
        "E-OWN-REF-ESCAPE",
    );
}

#[test]
fn borrowed_text_parameter_in_an_aggregate_stays_refused() {
    rejected(
        r#"
fn bad(s: &str) -> (&str, i32) {
    return (s, 1);
}
"#,
        "E-OWN-REF-ESCAPE",
    );
}

#[test]
fn top_level_match_binding_of_a_parameter_stays_refused() {
    // A top-level binding copies the value; it is not a projection out of an
    // aggregate, so a borrowed `&str` parameter stays non-static.
    rejected(
        r#"
fn bad(s: &str) -> &str {
    match s {
        x => { return x; }
    }
}
"#,
        "E-OWN-REF-ESCAPE",
    );
}

// -- Definitive initialisation ------------------------------------------

#[test]
fn init_on_both_paths_is_accepted() {
    accepted(
        r#"
fn f(c: bool) -> i32 {
    let mut x: i32;
    if c { x = 1; } else { x = 2; }
    return x;
}
"#,
    );
}

#[test]
fn init_on_one_path_is_rejected() {
    rejected(
        r#"
fn f(c: bool) -> i32 {
    let mut x: i32;
    if c { x = 1; }
    return x;
}
"#,
        "E-OWN-UNINIT",
    );
}

#[test]
fn read_before_init_is_rejected() {
    rejected(
        r#"
fn f() -> i32 {
    let x: i32;
    return x;
}
"#,
        "E-OWN-UNINIT",
    );
}

#[test]
fn borrow_before_init_is_rejected() {
    rejected(
        r#"
fn f() {
    let x: i32;
    let r = &x;
}
"#,
        "E-OWN-UNINIT",
    );
}

// -- Drop ----------------------------------------------------------------

#[test]
fn use_after_drop_is_rejected() {
    rejected(
        r#"use std::io;

fn f() {
    let s = String::from("a");
    drop(s);
    io::println(s.as_str());
}
"#,
        "E-OWN-USE-AFTER-MOVE",
    );
}

#[test]
fn drop_of_copy_is_a_noop() {
    accepted(
        r#"
fn f() {
    let n = 1;
    drop(n);
    let m = n + 1;
}
"#,
    );
}

// -- Loops ---------------------------------------------------------------

#[test]
fn move_in_loop_is_rejected() {
    rejected(
        r#"
fn f(v: Vec<i32>) {
    let s = String::from("x");
    for i in 0usize .. v.len() {
        drop(s);
    }
}
"#,
        "E-OWN-USE-AFTER-MOVE",
    );
}

#[test]
fn borrowing_a_loop_invariant_is_allowed() {
    accepted(
        r#"
fn f(v: Vec<i32>) -> usize {
    let s = String::from("x");
    let mut n: usize = 0;
    for i in 0usize .. v.len() {
        n += s.len();
    }
    return n;
}
"#,
    );
}

#[test]
fn init_in_loop_body_is_rejected_after_the_loop() {
    rejected(
        r#"
fn f(c: bool) -> i32 {
    let mut x: i32;
    while c { x = 1; }
    return x;
}
"#,
        "E-OWN-UNINIT",
    );
}

// -- Partial moves (design corrige : autorises et suivis) ----------------

#[test]
fn struct_field_move_is_accepted() {
    accepted(
        r#"
struct D { titre: String, n: i32 }
fn into_titre(d: D) -> String {
    return d.titre;
}
"#,
    );
}

#[test]
fn other_fields_remain_usable_after_partial_move() {
    accepted(
        r#"
struct D { titre: String, n: i32 }
fn f(d: D) -> i32 {
    let t = d.titre;
    return d.n;
}
"#,
    );
}

#[test]
fn whole_struct_use_after_partial_move_is_rejected() {
    rejected(
        r#"
struct D { titre: String, n: i32 }
fn f(d: D) -> D {
    let t = d.titre;
    return d;
}
"#,
        "E-OWN-PARTIAL-MOVE",
    );
}

#[test]
fn moved_field_cannot_be_read_again() {
    rejected(
        r#"
struct D { titre: String }
fn f(d: D) -> usize {
    let t = d.titre;
    return d.titre.len();
}
"#,
        "E-OWN-USE-AFTER-MOVE",
    );
}

#[test]
fn tuple_element_move_is_accepted() {
    accepted(
        r#"
fn f(t: (String, i32)) -> String {
    return t.0;
}
"#,
    );
}

#[test]
fn tuple_whole_use_after_partial_move_is_rejected() {
    rejected(
        r#"
fn f(t: (String, i32)) -> (String, i32) {
    let s = t.0;
    return t;
}
"#,
        "E-OWN-PARTIAL-MOVE",
    );
}

#[test]
fn moving_a_field_out_of_a_borrow_is_rejected() {
    rejected(
        r#"
struct D { titre: String }
fn f(d: &mut D) -> String {
    return d.titre;
}
"#,
        "E-OWN-MOVE-OUT-OF-BORROW",
    );
}

#[test]
fn moving_a_vec_element_by_index_is_rejected() {
    rejected(
        r#"
fn f(v: Vec<String>) -> String {
    return v[0];
}
"#,
        "E-OWN-PARTIAL-MOVE",
    );
}

#[test]
fn opaque_index_after_array_partial_move_is_rejected() {
    rejected(
        r#"
fn f(a: [String; 2], i: usize) -> usize {
    let s = a[0];
    return a[i].len();
}
"#,
        "E-OWN-USE-AFTER-MOVE",
    );
}

#[test]
fn distinct_literal_array_element_stays_usable_after_partial_move() {
    accepted(
        r#"
fn f(a: [String; 2]) -> usize {
    let s = a[0];
    return a[1].len();
}
"#,
    );
}

// -- OWN-5.11/OWN-5.12: element re-initialisation and opaque indexes -----

#[test]
fn moved_array_element_can_be_reinitialised_when_siblings_are_initialised() {
    accepted(
        r#"
fn f() -> usize {
    let mut a: [String; 2] = [String::from("a"), String::from("b")];
    let s: String = a[0];
    a[0] = String::from("x");
    return a[1].len();
}
"#,
    );
}

#[test]
fn moved_array_element_cannot_be_reinitialised_after_a_sibling_moved() {
    rejected(
        r#"
fn f() {
    let mut a: [String; 2] = [String::from("a"), String::from("b")];
    let s: String = a[0];
    let t: String = a[1];
    a[0] = String::from("x");
}
"#,
        "E-OWN-PARTIAL-MOVE",
    );
}

#[test]
fn opaque_index_write_after_partial_move_is_rejected() {
    rejected(
        r#"
fn f(i: usize) {
    let mut a: [String; 2] = [String::from("a"), String::from("b")];
    let s: String = a[0];
    a[i] = String::from("x");
}
"#,
        "E-OWN-PARTIAL-MOVE",
    );
}

#[test]
fn writing_an_initialised_element_after_a_sibling_moved_stays_allowed() {
    accepted(
        r#"
fn f() -> usize {
    let mut a: [String; 2] = [String::from("a"), String::from("b")];
    let s: String = a[0];
    a[1] = String::from("x");
    return s.len();
}
"#,
    );
}

#[test]
fn array_element_under_a_field_can_be_reinitialised() {
    accepted(
        r#"
struct Holder { items: [String; 2] }
fn f() -> usize {
    let mut h = Holder { items: [String::from("a"), String::from("b")] };
    let s: String = h.items[0];
    h.items[0] = String::from("x");
    return h.items[1].len();
}
"#,
    );
}

#[test]
fn array_element_under_a_tuple_can_be_reinitialised() {
    accepted(
        r#"
fn f() -> usize {
    let mut t: ([String; 2], i32) = ([String::from("a"), String::from("b")], 1);
    let s: String = t.0[0];
    t.0[0] = String::from("x");
    return t.0[1].len();
}
"#,
    );
}

#[test]
fn nested_array_element_can_be_reinitialised() {
    accepted(
        r#"
fn f() -> usize {
    let mut a: [[String; 2]; 2] = [
        [String::from("a"), String::from("b")],
        [String::from("c"), String::from("d")],
    ];
    let s: String = a[0][0];
    a[0][0] = String::from("x");
    return a[1][1].len();
}
"#,
    );
}

// -- Reference parameters / reborrows ------------------------------------

#[test]
fn shared_reference_parameter_usable_throughout() {
    accepted(
        r#"
fn compte(v: &Vec<i32>) -> usize {
    return v.len();
}
"#,
    );
}

#[test]
fn exclusive_reference_parameter_can_be_reborrowed() {
    accepted(
        r#"
fn ajoute(v: &mut Vec<i32>) {
    v.push(1);
    v.push(2);
}
"#,
    );
}

// -- Corpus de réfutation `conformance/cases/ownership` ------------------
// Ces cas sont la porte T4 : leurs codes attendus doivent rester exacts.

fn conformance_fails(src: &str, code: &str) {
    let result = run_bin(src);
    assert!(
        result.checked.is_none(),
        "expected rejection for {code}, but the program was accepted"
    );
    // The conformance runner compares the *set of error codes* exactly
    // (warnings are not part of `expect_codes`); online ownership must not
    // add extra errors to the case. `W-OWN-UNUSED-VARIABLE` may legitimately
    // accompany a rejection, so only `E-*` codes are compared here.
    let actual: std::collections::BTreeSet<String> = result
        .diags
        .iter()
        .map(|d| d.code.clone())
        .filter(|code| code.starts_with("E-"))
        .collect();
    let expected: std::collections::BTreeSet<String> = std::iter::once(code.to_string()).collect();
    assert_eq!(
        actual,
        expected,
        "conformance case expects exactly {{{code}}}, got {:?}",
        failures(&result)
    );
}

#[test]
fn conformance_use_after_move() {
    conformance_fails(
        r#"
fn h(s: String) -> usize {
    let t = s;
    return s.len();
}

fn main() {
}
"#,
        "E-OWN-USE-AFTER-MOVE",
    );
}

#[test]
fn conformance_double_move() {
    conformance_fails(
        r#"
fn f(s: String) {
    let a = s;
    let b = s;
}

fn main() {
}
"#,
        "E-OWN-DOUBLE-MOVE",
    );
}

#[test]
fn conformance_move_in_call() {
    conformance_fails(
        r#"
use std::io;

fn take(s: String) {
}

fn main() {
    let s = String::from("a");
    take(s);
    io::println(s.as_str());
}
"#,
        "E-OWN-USE-AFTER-MOVE",
    );
}

#[test]
fn conformance_drop_then_use() {
    conformance_fails(
        r#"
use std::io;

fn main() {
    let s = String::from("a");
    drop(s);
    io::println(s.as_str());
}
"#,
        "E-OWN-USE-AFTER-MOVE",
    );
}

#[test]
fn conformance_borrow_conflict_shared_mut() {
    conformance_fails(
        r#"
fn f2(a: &i32, b: &mut i32) {
}

fn main() {
    let mut x = 1;
    f2(&x, &mut x);
}
"#,
        "E-OWN-BORROW-CONFLICT",
    );
}

#[test]
fn conformance_borrow_conflict_two_mut() {
    conformance_fails(
        r#"
fn f(a: &mut i32, b: &mut i32) {
}

fn main() {
    let mut x = 1;
    f(&mut x, &mut x);
}
"#,
        "E-OWN-BORROW-CONFLICT",
    );
}

#[test]
fn conformance_ref_escape() {
    conformance_fails(
        r#"
fn bad(v: &Vec<i32>) -> &Vec<i32> {
    return v;
}

fn main() {
}
"#,
        "E-OWN-REF-ESCAPE",
    );
}

#[test]
fn conformance_copy_ok() {
    let result = run_bin(
        r#"
struct Point { x: i32, y: i32 }

fn f(p: Point) -> Point {
    let q = p;
    return p;
}

fn main() {
    let a = 1;
    let b = a;
    let p = Point { x: 1, y: 2 };
    let q = p;
    let s = f(q);
}
"#,
    );
    assert!(
        result.checked.is_some(),
        "expected acceptance, got {:?}",
        failures(&result)
    );
}

#[test]
fn conformance_conditional_init() {
    conformance_fails(
        r#"
fn f(c: bool) -> i32 {
    let x: i32;
    if c {
        x = 1;
    }
    return x;
}

fn main() {
}
"#,
        "E-OWN-UNINIT",
    );
}

// ------------------------------------------------------------------
// Fact-accumulation equivalence (wave 3, agent A)
// ------------------------------------------------------------------

/// The accumulation used before the 2026-11-14 fix, kept here as the oracle
/// of `own_facts_are_identical_to_the_append_accumulation`. Every function
/// records into its own `OwnFacts`; the program-wide tables are then rebuilt
/// with `BTreeMap::append` after each `check_fn`, which is what made the
/// phase quadratic. Test-only: nothing in the product calls this.
fn legacy_check_ownership(checked: &CheckedProgram, diags: &mut DiagnosticBag) -> OwnFacts {
    check_stored_reference_fields(&checked.program.root, &checked.facts, diags);
    let mut fns: Vec<(&Item, &FnItem)> = Vec::new();
    collect_fns(&checked.program.root, &mut fns);
    let mut own = OwnFacts::default();
    publish_copy_type(&checked.facts, &mut own);
    for (item, f) in fns {
        // The binding interner is program-wide by definition (an identity is
        // unique per program): it is threaded through, everything else is
        // per-function and appended.
        let mut per_fn = OwnFacts {
            bindings: std::mem::take(&mut own.bindings),
            ..OwnFacts::default()
        };
        let mut checker = OwnChecker::new(&checked.facts, diags, &mut per_fn);
        checker.check_fn(item, f);
        own.use_kind.append(&mut per_fn.use_kind);
        own.place.append(&mut per_fn.place);
        own.element_reinit.append(&mut per_fn.element_reinit);
        own.copy_type.append(&mut per_fn.copy_type);
        own.owned_position.append(&mut per_fn.owned_position);
        own.consumed.append(&mut per_fn.consumed);
        own.binding_identity.append(&mut per_fn.binding_identity);
        own.bindings = per_fn.bindings;
    }
    own
}

/// Serialized form of an ownership decision set: the whole proof is a
/// string equality, so a difference is reported verbatim.
fn serialize_facts(facts: &OwnFacts) -> String {
    format!("{facts:#?}")
}

/// Serialized form of the ownership diagnostics of one run.
fn serialize_diags(diags: &DiagnosticBag) -> String {
    diags
        .iter()
        .map(|diag| format!("{diag:?}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/ravum-lang-sema must live two levels below the workspace root")
        .to_path_buf()
}

/// A throwaway copy of one corpus package. The loader writes `ravum.lock`
/// next to the manifest, so the corpus itself is never touched.
struct TempPackage {
    path: PathBuf,
}

impl TempPackage {
    fn copy_of(source: &Path, label: &str) -> TempPackage {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let label: String = label
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let path = std::env::temp_dir().join(format!(
            "ravum-own-equivalence-{}-{unique}-{label}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        copy_tree(source, &path).expect("cannot copy the corpus package");
        TempPackage { path }
    }
}

impl Drop for TempPackage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Run the real loader + checker on a corpus package, then compare the two
/// accumulations on the accepted program: the ownership facts and the
/// ownership diagnostics must be byte-identical. Returns `true` when the
/// package recorded at least one fact (a few accepted programs, e.g. a bare
/// `println` of a literal, legitimately record none).
fn compare_package(label: &str, source: &Path, project: &str) -> Result<bool, String> {
    let package = TempPackage::copy_of(source, label);
    let loaded = load_package_with_options(&package.path.join(project), &LoadOptions::default());
    if loaded.diags.has_errors() {
        return Err(format!(
            "{label}: loader errors {:?}",
            loaded
                .diags
                .iter()
                .map(|d| d.code.clone())
                .collect::<Vec<_>>()
        ));
    }
    let Some(program) = loaded.program else {
        return Err(format!("{label}: the loader produced no program"));
    };
    let result = crate::check::check(
        &program,
        &loaded.sources,
        &CheckOptions {
            profile: Profile::Core,
            edition: ravum_lang_syntax::LANGUAGE_EDITION.to_string(),
        },
    );
    let Some(checked) = result.checked else {
        return Err(format!(
            "{label}: rejected by check {:?}",
            result
                .diags
                .iter()
                .map(|d| d.code.clone())
                .collect::<Vec<_>>()
        ));
    };

    let mut production_diags = DiagnosticBag::new();
    let production = check_ownership(&checked, &mut production_diags);
    let mut legacy_diags = DiagnosticBag::new();
    let legacy = legacy_check_ownership(&checked, &mut legacy_diags);

    let production_facts = serialize_facts(&production);
    let legacy_facts = serialize_facts(&legacy);
    if production_facts != legacy_facts {
        return Err(format!(
            "{label}: the recorded ownership facts differ from the append accumulation"
        ));
    }
    let production_render = serialize_diags(&production_diags);
    let legacy_render = serialize_diags(&legacy_diags);
    if production_render != legacy_render {
        return Err(format!(
            "{label}: the ownership diagnostics differ from the append accumulation"
        ));
    }
    Ok(!production.use_kind.is_empty() || !production.place.is_empty())
}

fn sorted_subdirs(dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", dir.display()))
        .map(|entry| entry.expect("cannot read a directory entry").path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// Every `case.json` below `conformance/cases`, in walk order.
fn conformance_case_dirs(cases_root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", dir.display()))
            .map(|entry| entry.expect("cannot read a directory entry").path())
            .collect();
        entries.sort();
        if dir.join("case.json").is_file() {
            out.push(dir.to_path_buf());
            return;
        }
        for entry in entries {
            if entry.is_dir() {
                walk(&entry, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(cases_root, &mut out);
    out
}

fn case_mode_and_project(case_dir: &Path) -> (String, String) {
    let text = std::fs::read_to_string(case_dir.join("case.json"))
        .unwrap_or_else(|error| panic!("cannot read case.json of {}: {error}", case_dir.display()));
    let value: serde_json::Value =
        serde_json::from_str(&text).expect("case.json must be valid JSON");
    let mode = value
        .get("mode")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("compile-pass")
        .to_string();
    let project = value
        .get("project")
        .and_then(serde_json::Value::as_str)
        .filter(|project| !project.is_empty())
        .unwrap_or(".")
        .to_string();
    (mode, project)
}

/// The ownership facts and diagnostics must not depend on how the per-file
/// decisions are accumulated: the quadratic `BTreeMap::append` loop and the
/// shared accumulator of today must produce the same serialized tables on
/// every accepted package of `examples/` and of the conformance corpus.
///
/// Acceptance perimeter: the 10 examples and every `compile-pass` /
/// `run-pass` case (the two modes whose program must pass `check`); a case
/// that fails to load or to check is a failure of this test, not a skip, so
/// the comparison can never be silently vacuous.
#[test]
fn own_facts_are_identical_to_the_append_accumulation() {
    let root = workspace_root();
    let mut compared = 0usize;
    let mut with_facts = 0usize;
    let mut expected = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for dir in sorted_subdirs(&root.join("examples")) {
        // `package-demo` keeps its binary package under `app/`.
        let project = if dir.join("ravum.toml").is_file() {
            "."
        } else {
            "app"
        };
        let label = format!(
            "example/{}",
            dir.file_name().unwrap_or_default().to_string_lossy()
        );
        expected += 1;
        match compare_package(&label, &dir, project) {
            Ok(recorded) => {
                compared += 1;
                with_facts += usize::from(recorded);
            }
            Err(error) => failures.push(error),
        }
    }

    for case_dir in conformance_case_dirs(&root.join("conformance").join("cases")) {
        let (mode, project) = case_mode_and_project(&case_dir);
        if mode != "compile-pass" && mode != "run-pass" {
            continue;
        }
        let label = case_dir
            .strip_prefix(root.join("conformance").join("cases"))
            .unwrap_or(&case_dir)
            .display()
            .to_string()
            .replace('\\', "/");
        expected += 1;
        match compare_package(&format!("conformance/{label}"), &case_dir, &project) {
            Ok(recorded) => {
                compared += 1;
                with_facts += usize::from(recorded);
            }
            Err(error) => failures.push(error),
        }
    }

    assert!(
        failures.is_empty(),
        "the shared accumulator changed {} of {} packages:\n{}",
        failures.len(),
        expected,
        failures.join("\n")
    );
    assert_eq!(
        compared, expected,
        "every accepted corpus package must be compared (non-vacuity)"
    );
    assert!(
            with_facts >= 150,
            "the equivalence proof must compare non-empty fact tables, got {with_facts} of {compared} packages"
        );
    println!(
            "own facts equivalence: {compared} packages compared, {with_facts} recorded facts, 0 difference"
        );
}

/// The parser bounds the tree height well below the analysis' own depth
/// guard, so no parsed program reaches it. If a tree does anyway, the
/// function is refused: the guard used to skip what lay below in silence,
/// which accepted a double move the generated Rust then failed on (E0382).
#[test]
fn the_depth_guard_refuses_the_function_instead_of_skipping_it() {
    let result = run("fn f(s: String) -> usize { let t = s; t.len() }\n");
    let checked = result.checked.expect("the fixture checks");
    let mut fns: Vec<(&Item, &FnItem)> = Vec::new();
    collect_fns(&checked.program.root, &mut fns);
    let (_, f) = fns[0];

    let mut diags = DiagnosticBag::new();
    let mut own = OwnFacts::default();
    let mut checker = OwnChecker::new(&checked.facts, &mut diags, &mut own);
    checker.depth = super::MAX_DEPTH;
    // Every entry point of the recursion refuses, and reports once.
    checker.analyze_block(&f.body, true);
    checker.analyze_stmt(&f.body.stmts[0]);
    let tail = f.body.tail.as_ref().expect("the fixture has a tail");
    checker.eval_value(tail);
    assert!(checker.eval_place_expr(tail).is_none());
    assert!(checker.too_deep);

    let codes: Vec<&str> = diags.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(codes, ["E-OWN-LIMIT"]);
}

// ----------------------------------------------------------------------
// Path algebra: the pure predicates every move/borrow decision rests on.
// ----------------------------------------------------------------------

mod path_algebra {
    use super::super::{
        classify_partial, diverges_disjoint, field_type, join, places_overlap, prefix_compatible,
        step_compatible, IndexKey, PartialRel, Place, SlotState, State, Step,
    };
    use crate::facts::Facts;
    use crate::types::Type;

    fn field(name: &str) -> Step {
        Step::Field(name.to_string())
    }
    fn lit(index: u128) -> Step {
        Step::Index(IndexKey::Lit(index))
    }
    fn opaque() -> Step {
        Step::Index(IndexKey::Opaque)
    }

    #[test]
    fn two_steps_denote_the_same_position_only_when_nothing_tells_them_apart() {
        assert!(step_compatible(&Step::Deref, &Step::Deref));
        assert!(step_compatible(&field("a"), &field("a")));
        assert!(!step_compatible(&field("a"), &field("b")));
        assert!(step_compatible(&Step::Tuple(0), &Step::Tuple(0)));
        assert!(!step_compatible(&Step::Tuple(0), &Step::Tuple(1)));
        assert!(step_compatible(&lit(1), &lit(1)));
        assert!(!step_compatible(&lit(1), &lit(2)));
        // A computed index may equal any index.
        assert!(step_compatible(&opaque(), &lit(2)));
        assert!(step_compatible(&lit(2), &opaque()));
        // A dereference is never the same position as a projection.
        assert!(!step_compatible(&Step::Deref, &field("a")));
        assert!(!step_compatible(&lit(0), &Step::Deref));
        // Mismatched projection kinds: nothing proves them apart.
        assert!(step_compatible(&field("a"), &Step::Tuple(0)));
        assert!(step_compatible(&Step::Tuple(0), &lit(0)));
    }

    #[test]
    fn a_divergence_proves_disjointness_only_on_comparable_steps() {
        assert!(diverges_disjoint(&[field("a")], &[field("b")]));
        assert!(diverges_disjoint(&[Step::Tuple(0)], &[Step::Tuple(1)]));
        assert!(diverges_disjoint(&[lit(0)], &[lit(1)]));
        assert!(diverges_disjoint(&[Step::Deref], &[field("a")]));
        assert!(diverges_disjoint(&[field("a")], &[Step::Deref]));
        // Under a computed index a later divergence proves nothing.
        assert!(!diverges_disjoint(
            &[opaque(), field("a")],
            &[opaque(), field("b")]
        ));
        assert!(!diverges_disjoint(
            &[lit(0), field("a")],
            &[opaque(), field("b")]
        ));
        // Identical or prefix paths never diverge.
        assert!(!diverges_disjoint(&[field("a")], &[field("a")]));
        assert!(!diverges_disjoint(&[field("a")], &[field("a"), field("b")]));
        assert!(!diverges_disjoint(&[], &[field("a")]));
    }

    #[test]
    fn a_place_is_classified_against_a_moved_path() {
        let moved = [field("a"), lit(0)];
        assert_eq!(
            classify_partial(&[field("a"), lit(0)], &moved),
            PartialRel::Exact
        );
        assert_eq!(
            classify_partial(&[field("a"), lit(0), field("x")], &moved),
            PartialRel::Extension
        );
        assert_eq!(
            classify_partial(&[field("a")], &moved),
            PartialRel::Ancestor
        );
        assert_eq!(
            classify_partial(&[field("b")], &moved),
            PartialRel::Disjoint
        );
        assert_eq!(
            classify_partial(&[field("a"), lit(1)], &moved),
            PartialRel::Disjoint
        );
        // Same length, a computed index on one side, then a field that differs:
        // the paths overlap without being comparable, which is refused like an
        // extension.
        assert_eq!(
            classify_partial(&[opaque(), field("x")], &[lit(0), field("y")]),
            PartialRel::Extension
        );
        assert!(prefix_compatible(&[field("a")], &moved));
        assert!(!prefix_compatible(&moved, &[field("a")]));
        assert!(!prefix_compatible(&[field("b")], &moved));
    }

    #[test]
    fn two_places_overlap_unless_a_step_separates_them() {
        let place = |slot, steps: &[Step]| Place {
            slot,
            steps: steps.to_vec(),
        };
        let first: super::super::SlotId = 0;
        let other: super::super::SlotId = 1;
        assert!(!places_overlap(&place(first, &[]), &place(other, &[])));
        assert!(places_overlap(
            &place(first, &[]),
            &place(first, &[field("a")])
        ));
        assert!(places_overlap(
            &place(first, &[Step::Deref]),
            &place(first, &[Step::Deref])
        ));
        assert!(!places_overlap(
            &place(first, &[field("a")]),
            &place(first, &[field("b")])
        ));
        assert!(places_overlap(
            &place(first, &[Step::Tuple(1)]),
            &place(first, &[Step::Tuple(1)])
        ));
        assert!(!places_overlap(
            &place(first, &[Step::Tuple(0)]),
            &place(first, &[Step::Tuple(1)])
        ));
        assert!(!places_overlap(
            &place(first, &[lit(0)]),
            &place(first, &[lit(1)])
        ));
        assert!(places_overlap(
            &place(first, &[lit(0)]),
            &place(first, &[lit(0), field("a")])
        ));
        assert!(places_overlap(
            &place(first, &[opaque()]),
            &place(first, &[lit(1)])
        ));
        assert!(!places_overlap(
            &place(first, &[Step::Deref]),
            &place(first, &[field("a")])
        ));
        assert!(!places_overlap(
            &place(first, &[lit(0)]),
            &place(first, &[Step::Deref])
        ));
        // A field against a tuple index: the kinds differ, stay conservative.
        assert!(places_overlap(
            &place(first, &[field("a")]),
            &place(first, &[Step::Tuple(0)])
        ));
    }

    #[test]
    fn a_join_keeps_the_slots_known_on_one_side_only() {
        let left_only: super::super::SlotId = 0;
        let right_only: super::super::SlotId = 1;
        let mut left = State::default();
        left.slots.insert(left_only, SlotState::initialised());
        let mut right = State::default();
        right.slots.insert(right_only, SlotState::default());
        let joined = join(&left, &right);
        assert!(joined.slots[&left_only].init);
        assert!(!joined.slots[&right_only].init);
        assert_eq!(joined.slots.len(), 2);
    }

    #[test]
    fn a_field_of_an_unknown_type_has_an_unknown_type() {
        let facts = Facts::default();
        assert_eq!(field_type(&Type::I32, "a", &facts), Type::Error);
        let unknown_struct = Type::Nominal {
            def: 7,
            name: "Missing".to_string(),
            args: Vec::new(),
        };
        assert_eq!(field_type(&unknown_struct, "a", &facts), Type::Error);
    }
}

/// Without resolution facts, a single plain path is still looked up by name
/// and a qualified one (`crate::K`) is never taken for a local.
#[test]
fn paths_without_resolution_facts_fall_back_to_plain_names_only() {
    let result =
        run("const K: i32 = 1;\nfn f() -> i32 { let x = 2; let y = crate::K; return x + y; }\n");
    let mut checked = result.checked.expect("the fixture must check");
    checked.facts.resolutions.clear();
    let mut diags = DiagnosticBag::new();
    let _ = super::check_ownership(&checked, &mut diags);
    assert!(!diags.has_errors());
}

// -- E6: the checker's invariants, driven directly ----------------------------

/// A checker with no function entered: no scope is open, no slot exists.
fn bare_checker(test: impl FnOnce(&mut OwnChecker)) {
    let facts = Facts::default();
    let mut diags = DiagnosticBag::new();
    let mut own = OwnFacts::default();
    let mut checker = OwnChecker::new(&facts, &mut diags, &mut own);
    test(&mut checker);
}

#[test]
fn scopes_and_unknown_slots_are_handled_without_inventing_state() {
    use super::{Access, Place, Step};
    use ravum_lang_syntax::span::Span;
    bare_checker(|checker| {
        // `check_fn` opens the function scope first; without one, popping
        // and declaring change nothing but the slot table.
        checker.pop_scope();
        let slot = checker.declare_slot("x", Type::I32, false, Span::dummy());
        assert!(checker.lookup("x").is_none());
        // A slot without a state (every place starts from `lookup`, which
        // only returns declared slots).
        let ghost = Place {
            slot: slot + 100,
            steps: Vec::new(),
        };
        let ghost_field = Place {
            slot: slot + 100,
            steps: vec![Step::Field("f".to_string())],
        };
        assert!(!checker.check_availability(&ghost, Span::dummy(), Access::Read));
        checker.write_place(&ghost, Span::dummy(), 1);
        checker.write_place(&ghost_field, Span::dummy(), 1);
        checker.perform_move(&ghost, Span::dummy());
        checker.record_partial_move(&ghost_field, Span::dummy());
        checker.mark_used(slot + 100);
        assert_eq!(checker.type_at(&ghost, 0), Type::Error);
        assert_eq!(checker.display_place(&ghost), "<inconnu>");
        let ghost_index = Place {
            slot: slot + 100,
            steps: vec![Step::Index(super::IndexKey::Lit(0))],
        };
        assert!(matches!(
            checker.element_write(&ghost_index),
            super::ElementWrite::Plain
        ));
    });
}

#[test]
fn steps_of_different_kinds_prove_no_disjointness() {
    use super::state::diverges_disjoint;
    use super::Step;
    assert!(!diverges_disjoint(
        &[Step::Field("a".to_string())],
        &[Step::Tuple(0)]
    ));
    assert!(diverges_disjoint(
        &[Step::Field("a".to_string())],
        &[Step::Field("b".to_string())]
    ));
}

#[test]
fn moving_out_through_a_slice_index_is_refused() {
    use super::{Place, Step};
    use ravum_lang_syntax::span::Span;
    bare_checker(|checker| {
        checker.push_scope();
        let slot = checker.declare_slot(
            "s",
            Type::Slice(Box::new(Type::String)),
            false,
            Span::dummy(),
        );
        let place = Place {
            slot,
            steps: vec![Step::Index(super::IndexKey::Lit(0))],
        };
        checker.perform_move(&place, Span::dummy());
    });
}

#[test]
fn an_element_write_into_a_moved_element_extension_restores_nothing() {
    use super::{Place, Step};
    use ravum_lang_syntax::span::Span;
    bare_checker(|checker| {
        checker.push_scope();
        let slot = checker.declare_slot(
            "a",
            Type::Array {
                elem: Box::new(Type::Tuple(vec![Type::String, Type::String])),
                len: 2,
            },
            false,
            Span::dummy(),
        );
        checker
            .state
            .slots
            .insert(slot, super::SlotState::initialised());
        let element = Place {
            slot,
            steps: vec![Step::Index(super::IndexKey::Lit(0))],
        };
        checker.record_partial_move(&element, Span::dummy());
        let inside = Place {
            slot,
            steps: vec![Step::Index(super::IndexKey::Lit(0)), Step::Tuple(1)],
        };
        let _ = checker.element_write(&inside);
    });
}

/// Check `src`, then let `edit` alter the checked AST before ownership runs
/// again: the analysis must finish on shapes typeck never hands it.
fn ownership_after(src: &str, edit: impl FnOnce(&mut ravum_lang_syntax::ast::Block)) {
    use ravum_lang_syntax::ast::ItemKind;
    let mut checked = run(src).checked.expect("the fixture must check");
    let ItemKind::Fn(f) = &mut checked.program.root.items[0].kind else {
        panic!("expected a function");
    };
    edit(&mut f.body);
    let mut diags = DiagnosticBag::new();
    let _ = check_ownership(&checked, &mut diags);
}

fn diverging() -> ravum_lang_syntax::ast::ExprKind {
    ravum_lang_syntax::ast::ExprKind::Return(None)
}

#[test]
fn ownership_finishes_on_shapes_typeck_refuses() {
    use ravum_lang_syntax::ast::{ExprKind, StmtKind};
    // `&&` whose left operand diverges while its right one does not.
    ownership_after(
        "fn f(a: bool) -> bool { let b = a && true; return b; }",
        |body| {
            let StmtKind::Let(l) = &mut body.stmts[0].kind else {
                panic!()
            };
            let ExprKind::Binary { lhs, .. } = &mut l.init.as_mut().unwrap().kind else {
                panic!()
            };
            lhs.kind = diverging();
        },
    );
    // An assignment whose target is not a place.
    ownership_after("fn f() { let mut x = 1; x = 2; }", |body| {
        let StmtKind::Expr { expr, .. } = &mut body.stmts[1].kind else {
            panic!()
        };
        let ExprKind::Assign { target, .. } = &mut expr.kind else {
            panic!()
        };
        target.kind = ExprKind::Int {
            value: 1,
            suffix: None,
        };
    });
    // A `while` whose condition diverges, with a `break` in its body.
    ownership_after("fn f(c: bool) { while c { break; } let z = 1; }", |body| {
        let StmtKind::Expr { expr, .. } = &mut body.stmts[0].kind else {
            panic!()
        };
        let ExprKind::While { cond, .. } = &mut expr.kind else {
            panic!()
        };
        cond.kind = diverging();
    });
    // A `for` whose iterator diverges, with a `break` in its body.
    ownership_after(
        "fn f(v: Vec<i32>) { for x in v { break; } let z = 1; }",
        |body| {
            let StmtKind::Expr { expr, .. } = &mut body.stmts[0].kind else {
                panic!()
            };
            let ExprKind::For { iter, .. } = &mut expr.kind else {
                panic!()
            };
            iter.kind = diverging();
        },
    );
}

#[test]
#[should_panic(expected = "invariant: an available place has a slot state")]
fn the_state_of_a_slot_without_one_is_an_invariant_failure() {
    use super::Place;
    bare_checker(|checker| {
        let _ = checker.slot_state_mut(&Place {
            slot: 77,
            steps: Vec::new(),
        });
    });
}
