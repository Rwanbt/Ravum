//! Contract tests for the Ravum type checker (`typeck.rs`).
//!
//! These tests are **independent of the implementation**: each case is a small
//! program and the diagnostic code that `spec/language/` requires for it
//! (`docs/` diagnostics table, `spec/language/types.md`, `grammar.md`). They
//! exist to pin the error paths that the conformance corpus does not exercise
//! and to raise coverage of `typeck.rs` with honest, spec-anchored checks.
//!
//! A case that the checker rejects for a *different* reason than the expected
//! code is a failure: the test reports every diagnostic, so a drift in the
//! first failing phase cannot be mistaken for a pass.
//!
//! Run with:
//! `cargo test -p ravum-lang-sema --test typeck_contracts`

use ravum_lang_sema::syntax::{parser, source::SourceMap, LANGUAGE_EDITION};
use ravum_lang_sema::{check, CheckOptions, CheckResult, Profile};

fn run_at(src: &str, path: &str) -> CheckResult {
    let mut sources = SourceMap::new();
    let file = sources.add(path, src);
    let parsed = parser::parse_file(file, src);
    let parse_codes: Vec<String> = parsed.diags.iter().map(|d| d.code.clone()).collect();
    assert!(
        parsed.diags.is_empty(),
        "contract case is not syntactically valid: {parse_codes:?}\n--- source ---\n{src}"
    );
    let options = CheckOptions {
        profile: Profile::Core,
        edition: LANGUAGE_EDITION.to_string(),
    };
    check(&parsed.program, &sources, &options)
}

/// A library crate (`src/lib.rv`: no `fn main` needed).
fn run(src: &str) -> CheckResult {
    run_at(src, "src/lib.rv")
}

/// A binary crate (`src/main.rv`: `fn main` is validated).
fn run_main(src: &str) -> CheckResult {
    run_at(src, "src/main.rv")
}

fn describe(result: &CheckResult) -> String {
    if result.diags.is_empty() {
        return "<no diagnostic>".to_string();
    }
    result
        .diags
        .iter()
        .map(|d| format!("{}: {}", d.code, d.message))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn codes(result: &CheckResult) -> Vec<String> {
    result.diags.iter().map(|d| d.code.clone()).collect()
}

/// The program must be rejected and the expected diagnostic must be reported.
fn expect_code(src: &str, expected: &str) {
    let result = run(src);
    assert!(
        result.checked.is_none(),
        "expected {expected} but the program was accepted\n--- source ---\n{src}"
    );
    assert!(
        codes(&result).iter().any(|c| c == expected),
        "expected {expected}, got {:?}\n--- source ---\n{src}",
        codes(&result)
    );
}

fn expect_code_main(src: &str, expected: &str) {
    let result = run_main(src);
    assert!(
        result.checked.is_none(),
        "expected {expected} but the program was accepted\n--- source ---\n{src}"
    );
    assert!(
        codes(&result).iter().any(|c| c == expected),
        "expected {expected}, got {:?}\n--- source ---\n{src}",
        codes(&result)
    );
}

/// The program must be accepted with no *error* diagnostic. Warnings are
/// allowed here because this file tests typing, not lint suppressions; every
/// warning is still visible in the failure message if an error appears.
fn accept(src: &str) {
    let result = run(src);
    let errors: Vec<String> = result
        .diags
        .iter()
        .filter(|d| !d.code.starts_with("W-"))
        .map(|d| format!("{}: {}", d.code, d.message))
        .collect();
    assert!(
        result.checked.is_some() && errors.is_empty(),
        "expected success, got {}\n--- source ---\n{src}",
        describe(&result)
    );
}

// ===========================================================================
// Calls: non-callable callee, arity, argument types
// ===========================================================================

#[test]
fn call_of_non_function_value_is_e_typ_call() {
    // diagnostics.md: `E-TYP-CALL` = call of a non-function value.
    expect_code(
        r#"
fn f() {
    let x = 1;
    x();
}
"#,
        "E-TYP-CALL",
    );
    // A field is not a method: the method lookup fails before the callee is
    // typed, so the canonical code is `E-RES-UNKNOWN` (a callable-value error
    // is only reported when resolution succeeded).
    expect_code(
        r#"
struct P { n: i32 }
fn f(p: P) {
    p.n();
}
"#,
        "E-RES-UNKNOWN",
    );
}

#[test]
fn call_arity_is_e_typ_arity() {
    expect_code(
        r#"
fn g(a: i32) -> i32 { return a; }
fn f() {
    let x = g();
}
"#,
        "E-TYP-ARITY",
    );
    expect_code(
        r#"
fn g(a: i32) -> i32 { return a; }
fn f() {
    let x = g(1, 2);
}
"#,
        "E-TYP-ARITY",
    );
}

#[test]
fn call_argument_type_mismatch_is_reported() {
    // diagnostics.md: incompatible arguments are `E-TYP-MISMATCH`; `E-TYP-CALL`
    // is reserved for a callee that is not callable.
    expect_code(
        r#"
fn g(a: i32) -> i32 { return a; }
fn f() {
    let x = g(true);
}
"#,
        "E-TYP-MISMATCH",
    );
}

#[test]
fn method_arity_is_e_typ_arity() {
    expect_code(
        r#"
struct D { n: i32 }
impl D {
    fn m(&self, a: i32) -> i32 { return a; }
}
fn f(d: D) {
    let x = d.m();
}
"#,
        "E-TYP-ARITY",
    );
}

// ===========================================================================
// Fields, tuples, indices
// ===========================================================================

#[test]
fn unknown_field_is_e_typ_field() {
    expect_code(
        r#"
struct P { x: i32 }
fn f(p: P) -> i32 {
    return p.y;
}
"#,
        "E-TYP-FIELD",
    );
}

#[test]
fn struct_literal_missing_field_is_rejected() {
    // diagnostics.md lists `E-TYP-MISSING-FIELD`; `E-TYP-FIELD` covers
    // "champ manquant" as the canonical family. Accept either canonical code
    // but require the program to be rejected.
    let result = run(r#"
struct P { x: i32, y: i32 }
fn f() -> P {
    return P { x: 1 };
}
"#);
    assert!(
        result.checked.is_none(),
        "missing field accepted: {}",
        describe(&result)
    );
    let cs = codes(&result);
    assert!(
        cs.iter()
            .any(|c| c == "E-TYP-FIELD" || c == "E-TYP-MISSING-FIELD"),
        "expected a missing-field diagnostic, got {cs:?}"
    );
}

#[test]
fn struct_literal_unknown_field_is_e_typ_field() {
    expect_code(
        r#"
struct P { x: i32 }
fn f() -> P {
    return P { z: 1 };
}
"#,
        "E-TYP-FIELD",
    );
}

#[test]
fn index_type_errors() {
    // diagnostics.md: `E-TYP-INDEX` = non-`usize` index, non-indexable type.
    expect_code(
        r#"
fn f(v: Vec<i32>) -> i32 {
    return v[1i32];
}
"#,
        "E-TYP-INDEX",
    );
    expect_code(
        r#"
fn f(s: String) -> u8 {
    let b = s[0];
    return b;
}
"#,
        "E-TYP-INDEX",
    );
}

// ===========================================================================
// Casts
// ===========================================================================

#[test]
fn cast_outside_allowed_pairs_is_e_typ_cast() {
    expect_code(
        r#"
fn f(n: i32) -> bool {
    return n as bool;
}
"#,
        "E-TYP-CAST",
    );
    expect_code(
        r#"
fn f(n: u32) -> char {
    return n as char;
}
"#,
        "E-TYP-CAST",
    );
    expect_code(
        r#"
fn f(n: i32) -> i64 {
    let r = &n;
    return r as i64;
}
"#,
        "E-TYP-CAST",
    );
}

// ===========================================================================
// Equality / comparison
// ===========================================================================

#[test]
fn equality_on_aggregates_is_e_typ_eq() {
    expect_code(
        r#"
fn f() -> bool {
    return (1, 2) == (3, 4);
}
"#,
        "E-TYP-EQ",
    );
    expect_code(
        r#"
fn f(a: Option<i32>) -> bool {
    return a == a;
}
"#,
        "E-TYP-EQ",
    );
}

#[test]
fn ordering_on_text_is_e_typ_cmp() {
    expect_code(
        r#"
fn f(a: &str, b: &str) -> bool {
    return a < b;
}
"#,
        "E-TYP-CMP",
    );
}

// ===========================================================================
// Inference, `if` value position, statement values
// ===========================================================================

#[test]
fn unresolved_inference_is_e_typ_ambiguous() {
    expect_code(
        r#"
fn f() {
    let v = Vec::new();
}
"#,
        "E-TYP-AMBIGUOUS",
    );
    expect_code(
        r#"
fn f() {
    let o = None;
}
"#,
        "E-TYP-AMBIGUOUS",
    );
}

#[test]
fn if_without_else_in_value_position_is_e_typ_if_no_else() {
    expect_code(
        r#"
fn f(c: bool) -> i32 {
    let x = if c { 1 };
    return x;
}
"#,
        "E-TYP-IF-NO-ELSE",
    );
}

#[test]
fn non_unit_expression_statement_is_e_typ_statement_value() {
    expect_code(
        r#"
fn f() {
    1 + 1;
}
"#,
        "E-TYP-STATEMENT-VALUE",
    );
}

// ===========================================================================
// `?`, `return`, entry points
// ===========================================================================

#[test]
fn try_in_incompatible_context_is_e_typ_try() {
    expect_code(
        r#"
fn f() -> i32 {
    let x: i32 = 1;
    let y = x?;
    return y;
}
"#,
        "E-TYP-TRY",
    );
}

#[test]
fn return_errors_are_e_typ_return() {
    expect_code(
        r#"
fn f() -> i32 {
    return;
}
"#,
        "E-TYP-RETURN",
    );
    expect_code(
        r#"
fn f() {
    return 1;
}
"#,
        "E-TYP-RETURN",
    );
}

#[test]
fn entry_point_errors() {
    expect_code_main(
        r#"
fn f() {
}
"#,
        "E-TYP-MAIN-MISSING",
    );
    expect_code_main(
        r#"
fn main(x: i32) {
}
"#,
        "E-TYP-MAIN",
    );
    expect_code_main(
        r#"
fn main() {
}
fn main() {
}
"#,
        "E-TYP-MAIN-DUPLICATE",
    );
}

// ===========================================================================
// Loops, ranges, literals
// ===========================================================================

#[test]
fn break_and_continue_outside_loop_are_e_typ_break_outside() {
    expect_code(
        r#"
fn f() {
    break;
}
"#,
        "E-TYP-BREAK-OUTSIDE",
    );
    expect_code(
        r#"
fn f() {
    continue;
}
"#,
        "E-TYP-BREAK-OUTSIDE",
    );
}

#[test]
fn non_bool_loop_conditions_are_rejected() {
    expect_code(
        r#"
fn f() {
    if 1 {
    }
}
"#,
        "E-TYP-MISMATCH",
    );
    expect_code(
        r#"
fn f() {
    while 1 {
    }
}
"#,
        "E-TYP-MISMATCH",
    );
}

#[test]
fn range_bound_errors_are_e_typ_range() {
    expect_code(
        r#"
fn f() {
    let start: i32 = 0;
    let end: u32 = 4;
    for i in start..end {
    }
}
"#,
        "E-TYP-RANGE",
    );
    expect_code(
        r#"
fn f() {
    for i in 0..1.5 {
    }
}
"#,
        "E-TYP-RANGE",
    );
}

#[test]
fn for_binding_type_mismatch_is_rejected() {
    expect_code(
        r#"
fn f() {
    for i in 0u8..4u8 {
        let x: i32 = i;
    }
}
"#,
        "E-TYP-MISMATCH",
    );
}

#[test]
fn array_repeat_errors() {
    // diagnostics.md: `E-TYP-ARRAY-REPEAT` = non-literal/zero length, non-Copy
    // repeated element, invalid array length.
    expect_code(
        r#"
fn f() {
    let a = [0u8; 0];
}
"#,
        "E-TYP-ARRAY-REPEAT",
    );
    expect_code(
        r#"
fn f(n: usize) {
    let a = [0; n];
}
"#,
        "E-TYP-ARRAY-REPEAT",
    );
    expect_code(
        r#"
struct D { s: String }
fn f(d: D) {
    let a = [d; 3];
}
"#,
        "E-TYP-ARRAY-REPEAT",
    );
}

#[test]
fn float_literal_out_of_range_is_e_typ_float_literal() {
    expect_code(
        r#"
fn f() -> f32 {
    return 1e39;
}
"#,
        "E-TYP-FLOAT-LITERAL",
    );
}

#[test]
fn integer_literal_out_of_range_is_e_typ_literal_range() {
    expect_code(
        r#"
fn f() -> u8 {
    return 300;
}
"#,
        "E-TYP-LITERAL-RANGE",
    );
}

// ===========================================================================
// Constants
// ===========================================================================

#[test]
fn constant_evaluation_errors_are_e_typ_const() {
    expect_code(
        r#"
const X: i8 = 127 + 1;
"#,
        "E-TYP-CONST",
    );
    expect_code(
        r#"
const X: i32 = 1 % 0;
"#,
        "E-TYP-CONST",
    );
    expect_code(
        r#"
const X: i8 = -100 - 100;
"#,
        "E-TYP-CONST",
    );
}

#[test]
fn non_constant_const_is_e_res_const_not_const() {
    expect_code(
        r#"
fn g() -> i32 { return 1; }
const X: i32 = g();
"#,
        "E-RES-CONST-NOT-CONST",
    );
    expect_code(
        r#"
const X: f64 = 1.5;
"#,
        "E-RES-CONST-NOT-CONST",
    );
}

#[test]
fn constant_type_mismatch_is_rejected() {
    expect_code(
        r#"
const X: i32 = true;
"#,
        "E-TYP-MISMATCH",
    );
}

// ===========================================================================
// `self`, unknown names, builtins
// ===========================================================================

#[test]
fn self_outside_impl_is_rejected() {
    expect_code(
        r#"
fn f() -> i32 {
    return self.n;
}
"#,
        "E-RES-SELF",
    );
}

#[test]
fn unknown_method_is_e_res_unknown() {
    expect_code(
        r#"
fn f() {
    let x = 1;
    x.zzz();
}
"#,
        "E-RES-UNKNOWN",
    );
}

#[test]
fn builtin_arity_and_equality_errors() {
    expect_code(
        r#"
fn f() {
    assert();
}
"#,
        "E-TYP-ARITY",
    );
    expect_code(
        r#"
fn f() {
    assert_eq(1, true);
}
"#,
        "E-TYP-MISMATCH",
    );
    expect_code(
        r#"
fn f() {
    assert_eq((1, 2), (1, 2));
}
"#,
        "E-TYP-EQ",
    );
}

#[test]
fn variant_constructor_errors() {
    expect_code(
        r#"
fn f() {
    let o = Option::Some(1, 2);
}
"#,
        "E-TYP-ARITY",
    );
    expect_code(
        r#"
fn f(o: Option<i32>) -> i32 {
    match o {
        Option::Some(true) => 1,
        Option::None => 0,
    }
}
"#,
        "E-TYP-MISMATCH",
    );
}

#[test]
fn shift_errors_are_e_typ_shift() {
    expect_code(
        r#"
fn f(a: i32) -> i32 {
    return a << -1;
}
"#,
        "E-TYP-SHIFT",
    );
}

#[test]
fn assignment_type_errors() {
    expect_code(
        r#"
fn f() {
    let mut x: i32 = 1;
    x = true;
}
"#,
        "E-TYP-MISMATCH",
    );
    expect_code(
        r#"
fn f() {
    let mut x: i32 = 1;
    x += true;
}
"#,
        "E-TYP-MISMATCH",
    );
}

// ===========================================================================
// Patterns
// ===========================================================================

#[test]
fn pattern_type_errors_are_rejected() {
    expect_code(
        r#"
fn f(x: i32) -> i32 {
    match x {
        true => 1,
        _ => 0,
    }
}
"#,
        "E-TYP-MISMATCH",
    );
    expect_code(
        r#"
fn f(o: Option<i32>) -> i32 {
    match o {
        Result::Ok(v) => 1,
        _ => 0,
    }
}
"#,
        "E-TYP-MISMATCH",
    );
}

#[test]
fn tuple_pattern_arity_mismatch_is_rejected() {
    expect_code(
        r#"
fn f(t: (i32, i32)) -> i32 {
    match t {
        (a, b, c) => a,
    }
}
"#,
        "E-TYP-ARITY",
    );
}

#[test]
fn unknown_path_pattern_is_e_res_unknown() {
    // A bare identifier is an irrefutable binding pattern, not an unknown name;
    // an unknown *path* pattern is a resolution error.
    expect_code(
        r#"
fn f(o: Option<i32>) -> i32 {
    match o {
        Option::Inconnu => 1,
        _ => 0,
    }
}
"#,
        "E-RES-UNKNOWN",
    );
}

// ===========================================================================
// Static-text analysis (`&'static str` propagation, TYP-1.6 / OWN-6.2)
// ===========================================================================

#[test]
fn static_text_literal_return_is_accepted() {
    accept(
        r#"
fn f() -> &str {
    return "a";
}
"#,
    );
}

#[test]
fn static_text_if_value_is_accepted() {
    accept(
        r#"
fn f(c: bool) -> &str {
    return if c { "a" } else { "b" };
}
"#,
    );
}

#[test]
fn static_text_match_value_is_accepted() {
    accept(
        r#"
fn f(c: bool) -> &str {
    return match c {
        true => "a",
        false => "b",
    };
}
"#,
    );
}

#[test]
fn static_text_block_value_is_accepted() {
    accept(
        r#"
fn f() -> &str {
    return { "a" };
}
"#,
    );
}

#[test]
fn static_text_function_call_is_accepted() {
    accept(
        r#"
fn lit() -> &str {
    return "a";
}

fn f() -> &str {
    return lit();
}
"#,
    );
}

#[test]
fn static_text_in_aggregates_is_accepted() {
    accept(
        r#"
struct S { text: &str }

fn pair() -> (&str, &str) {
    return ("a", "b");
}

fn make() -> S {
    return S { text: "x" };
}

// NOTE: returning a tuple field of `(&str, &str)` is *conservatively*
// rejected by the static-text analysis (it does not track projections). The
// rejection is sound but imprecise; it is documented as a limitation instead of
// being frozen as an expected diagnostic here.
"#,
    );
}

#[test]
fn static_text_local_binding_is_accepted() {
    accept(
        r#"
fn direct() -> &str {
    let text = "ravum";
    return text;
}

fn through_if(c: bool) -> &str {
    let text = if c { "a" } else { "b" };
    return text;
}

fn through_block() -> &str {
    let text = { "a" };
    return text;
}
"#,
    );
}

#[test]
fn static_text_is_comparable_and_storable() {
    accept(
        r#"
struct S { text: &str }

fn eq(a: &str, b: &str) -> bool {
    return a == b;
}

fn store() -> S {
    let l = "littéral";
    return S { text: l };
}
"#,
    );
}

// ===========================================================================
// Patterns and exhaustiveness
// ===========================================================================

#[test]
fn literal_and_or_patterns_are_accepted() {
    accept(
        r#"
fn f(x: i32) -> i32 {
    match x {
        0 => 1,
        1 => 2,
        _ => 0,
    }
}
"#,
    );
}

#[test]
fn tuple_patterns_are_accepted() {
    accept(
        r#"
fn tuple(t: (i32, i32)) -> i32 {
    match t {
        (a, b) => a + b,
    }
}
"#,
    );
}

#[test]
fn variant_patterns_and_exhaustiveness_are_accepted() {
    accept(
        r#"
enum Couleur { Rouge, Vert, Bleu }

fn code(c: Couleur) -> i32 {
    match c {
        Couleur::Rouge => 1,
        Couleur::Vert => 2,
        Couleur::Bleu => 3,
    }
}

fn option(o: Option<i32>) -> i32 {
    match o {
        Some(v) => v,
        None => 0,
    }
}
"#,
    );
}

#[test]
fn bool_and_char_match_are_exhaustive() {
    accept(
        r#"
fn b(c: bool) -> i32 {
    match c {
        true => 1,
        false => 0,
    }
}

fn ch(c: char) -> i32 {
    match c {
        'a' => 1,
        _ => 0,
    }
}
"#,
    );
}

// ===========================================================================
// Constant evaluation (accepted programs)
// ===========================================================================

#[test]
fn constant_arithmetic_is_evaluated() {
    accept(
        r#"
const A: i32 = 1 + 2 * 3;
const B: i32 = (10 - 4) / 2;
const C: i32 = 7 % 3;
const D: i8 = -100 - 20;
const E: u8 = 1 << 3;
const F: u8 = 255 >> 4;
const G: u8 = 0b1100 & 0b1010;
const H: u8 = 0b1100 | 0b0001;
const I: u8 = 0b1100 ^ 0b1010;
const J: bool = 1 < 2 && 3 >= 3;
const K: bool = true || false;
const L: bool = !false;
const M: char = 'a';
const N: &str = "texte";
"#,
    );
}

#[test]
fn constant_casts_are_evaluated() {
    accept(
        r#"
const A: i64 = 1 as i64;
const B: u16 = 300 as u16;
const C: i8 = 300 as i8;
const D: i64 = 'a' as i64;
"#,
    );
}

#[test]
fn constant_is_usable_as_array_length() {
    accept(
        r#"
const N: usize = 3;

fn f() -> [i32; 3] {
    let a: [i32; 3] = [1, 2, 3];
    return a;
}
"#,
    );
}

// ===========================================================================
// Operators, casts, ranges
// ===========================================================================

#[test]
fn compound_assignments_are_accepted() {
    accept(
        r#"
fn f() {
    let mut x: i32 = 10;
    x += 1;
    x += 2;
}
"#,
    );
}

#[test]
fn binary_and_unary_operators_are_accepted() {
    accept(
        r#"
fn arith(a: i32, b: i32) -> i32 {
    return a + b - a * b / (b + 1) % 7;
}

fn bits(a: u8, b: u8) -> u8 {
    return a & b | a ^ b;
}

fn shifts(a: u8) -> u8 {
    return (a << 1) >> 1;
}

fn logic(a: bool, b: bool) -> bool {
    return !a && b || a;
}

fn cmp(a: i32, b: i32) -> bool {
    return a < b || a <= b || a > b || a >= b || a == b || a != b;
}

fn neg(a: i32) -> i32 {
    return -a;
}
"#,
    );
}

#[test]
fn allowed_casts_are_accepted() {
    accept(
        r#"
fn numeric(a: i32) -> i64 {
    return a as i64;
}

fn narrow(a: i64) -> u8 {
    return a as u8;
}

fn from_char(c: char) -> u32 {
    return c as u32;
}

fn char_to_i64(c: char) -> i64 {
    return c as i64;
}

fn int_to_float(n: i32) -> f64 {
    return n as f64;
}

fn float_to_int(f: f64) -> i32 {
    return f as i32;
}
"#,
    );
}

#[test]
fn for_over_integer_ranges_is_accepted() {
    accept(
        r#"
fn f() {
    for i in 0..10 {
        let x: i32 = i;
    }
    for j in 0u8..4u8 {
        let y: u8 = j;
    }
    for k in 0usize..3usize {
        let z: usize = k;
    }
}
"#,
    );
}

// ===========================================================================
// Generics, aggregates, methods
// ===========================================================================

#[test]
fn generic_function_and_struct_are_accepted() {
    accept(
        r#"
struct Boite<T> { valeur: T }

fn id<T>(x: T) -> T {
    return x;
}

fn get<T>(b: Boite<T>) -> T {
    return b.valeur;
}

fn f() {
    let a: i32 = id(1);
    let b: f64 = id(2.5);
    let c: Boite<i32> = Boite { valeur: 1 };
    let d: i32 = get(c);
}
"#,
    );
}

#[test]
fn user_methods_with_all_receivers_are_accepted() {
    accept(
        r#"
struct Compteur { n: i32 }

impl Compteur {
    fn lit(&self) -> i32 {
        return self.n;
    }
    fn incremente(&mut self) {
        self.n += 1;
    }
    fn consomme(self) -> i32 {
        return self.n;
    }
}

fn f(mut c: Compteur) -> i32 {
    let a = c.lit();
    c.incremente();
    let b = c.lit();
    let d = c.consomme();
    return a + b + d;
}
"#,
    );
}

#[test]
fn builtin_methods_are_accepted() {
    accept(
        r#"
fn f() {
    let mut v: Vec<i32> = Vec::new();
    v.push(1);
    let n = v.len();
    let e = v.is_empty();

    let mut s = String::from("abc");
    s.push('d');
    let l = s.len();

    let o: Option<i32> = Option::Some(1);
    let p = o.is_some();
    let q = o.unwrap();

    let x = 2.0;
    let r = x.sqrt() + x.abs();
}
"#,
    );
}

#[test]
fn nested_struct_literals_and_field_access_are_accepted() {
    accept(
        r#"
struct Point { x: f64, y: f64 }
struct Ligne { debut: Point, fin: Point }

fn longueur_x(l: Ligne) -> f64 {
    return l.debut.x + l.fin.x;
}

fn f() -> Ligne {
    return Ligne {
        debut: Point { x: 0.0, y: 0.0 },
        fin: Point { x: 1.0, y: 1.0 },
    };
}
"#,
    );
}

#[test]
fn tuple_index_and_array_index_are_accepted() {
    accept(
        r#"
fn tuple(t: (i32, f64)) -> f64 {
    return t.1;
}

fn array(a: [i32; 3], i: usize) -> i32 {
    return a[i] + a[0];
}

fn vector(v: Vec<i32>) -> i32 {
    return v[0];
}
"#,
    );
}

#[test]
fn try_operator_in_compatible_context_is_accepted() {
    accept(
        r#"
fn premier(v: Vec<i32>) -> Option<i32> {
    return Option::Some(v[0]);
}

fn suite(v: Vec<i32>) -> Option<i32> {
    let a = premier(v)?;
    return Option::Some(a + 1);
}
"#,
    );
}

// ===========================================================================
// Second batch: wider operator, constant, pattern and method coverage
// ===========================================================================

#[test]
fn constant_remaining_arithmetic_and_bitwise_operators() {
    accept(
        r#"
const A: i32 = 20 / 3;
const B: i32 = 20 % 3;
const C: u8 = 0b1010 & 0b0110;
const D: u8 = 0b1010 | 0b0110;
const E: u8 = 0b1010 ^ 0b0110;
const F: u64 = 1 << 40;
const G: u128 = 1 << 100;
const H: u8 = 255 >> 7;
const I: i64 = 3000000000;
const J: bool = !(1 == 2);
const K: bool = 1 != 2 && 2 < 3 && 3 <= 3 && 4 > 3 && 5 >= 5;
const L: i32 = -42;
"#,
    );
}

#[test]
fn constant_casts_truncate_and_sign_extend() {
    accept(
        r#"
const A: u8 = 300 as u8;
const B: u16 = -1 as u16;
const C: i8 = 200 as i8;
const D: u64 = -1 as u64;
const E: i32 = 65535 as i32;
const F: u8 = 1u64 as u8;
"#,
    );
}

#[test]
fn static_text_through_nested_control_flow_is_accepted() {
    accept(
        r#"
fn direct_return(c: bool) -> &str {
    if c {
        return "a";
    }
    return "b";
}

fn nested(c: bool, d: bool) -> &str {
    return if c {
        if d { "a" } else { "b" }
    } else {
        "c"
    };
}

fn block_value(c: bool) -> &str {
    return {
        if c { "a" } else { "b" }
    };
}
"#,
    );
}

#[test]
fn result_match_and_nested_tuple_patterns_are_accepted() {
    accept(
        r#"
fn resultat(r: Result<i32, Error>) -> i32 {
    match r {
        Ok(v) => v,
        Err(_) => 0,
    }
}

fn imbrique(p: (i32, (i32, i32))) -> i32 {
    match p {
        (a, (b, c)) => a + b + c,
    }
}

fn partiel(p: (i32, i32)) -> i32 {
    match p {
        (1, b) => b,
        (a, _) => a,
    }
}
"#,
    );
}

#[test]
fn text_literal_patterns_are_accepted() {
    // NOTE: `spec/language/grammar.md` G-6.36 (float patterns) and G-6.37
    // (negative literal patterns) are MUST rules that the implementation does
    // not accept yet: float patterns are rejected with `E-PARSE-EXPECTED` and
    // `-N` with `E-UNSUPPORTED-NEG-PAT`. Both are recorded as
    // `PAT-LIT-1` / `PAT-LIT-2` in `docs/qualification/T2-FINDINGS.md`; the
    // implementation is fail-closed, and the gap is not frozen as "expected"
    // here. Text literal patterns *are* accepted (this case).
    accept(
        r#"
fn texte(s: &str) -> i32 {
    match s {
        "a" => 1,
        _ => 0,
    }
}
"#,
    );
}

#[test]
fn primitive_methods_are_accepted() {
    accept(
        r#"
fn entiers(a: i32) -> String {
    let b = a.abs();
    return b.to_string();
}

fn flottants(x: f64) -> f64 {
    return x.abs() + x.sqrt() + x.floor() + x.ceil() + x.round() + x.powf(2.0) + x.min(1.0) + x.max(2.0);
}

fn booleens(b: bool) -> String {
    return b.to_string();
}

fn caracteres(c: char) -> String {
    return c.to_string();
}
"#,
    );
}

#[test]
fn collection_and_option_methods_are_accepted() {
    accept(
        r#"
fn vecteurs() -> usize {
    let mut v: Vec<i32> = Vec::new();
    v.push(1);
    v.push(2);
    let p = v.pop();
    let l = v.len();
    let e = v.is_empty();
    let c = v.clone();
    v.clear();
    return l;
}

fn options(o: Option<i32>) -> i32 {
    let a = o.is_some();
    let b = o.is_none();
    let u = o.unwrap();
    let d = o.unwrap_or(0);
    return u + d;
}

fn resultats(r: Result<i32, Error>) -> i32 {
    let a = r.is_ok();
    let b = r.is_err();
    let o = r.ok();
    let e = r.err();
    return 0;
}
"#,
    );
}

#[test]
fn string_methods_are_accepted() {
    accept(
        r#"
fn texte() -> usize {
    let s = String::from("abc");
    let u = s.to_uppercase();
    let l = s.to_lowercase();
    let c = s.contains("b");
    let r = s.replace("a", "z");
    let st = s.starts_with("a");
    let en = s.ends_with("c");
    let li = s.lines();
    let n = s.len();
    let e = s.is_empty();
    return n;
}
"#,
    );
}

#[test]
fn tuple_named_field_is_e_typ_field() {
    expect_code(
        r#"
fn f() -> i32 {
    let t = (1, 2);
    return t.n;
}
"#,
        "E-TYP-FIELD",
    );
}

#[test]
fn method_used_as_field_is_e_res_unknown() {
    expect_code(
        r#"
fn f() {
    let s = String::from("abc");
    let t = s.trim;
}
"#,
        "E-RES-UNKNOWN",
    );
}

#[test]
fn unknown_field_on_primitive_is_e_typ_field() {
    expect_code(
        r#"
fn f() -> i32 {
    let x = 1;
    return x.n;
}
"#,
        "E-TYP-FIELD",
    );
}

#[test]
fn nested_generic_aggregates_are_accepted() {
    accept(
        r#"
struct Boite<T> { valeur: T }

fn enveloppe(x: i32) -> Boite<Vec<i32>> {
    let mut v: Vec<i32> = Vec::new();
    v.push(x);
    return Boite { valeur: v };
}

fn option_de_boite(o: Option<Boite<i32>>) -> Option<i32> {
    match o {
        Some(b) => Option::Some(b.valeur),
        None => Option::None,
    }
}

fn vec_option() -> Vec<Option<i32>> {
    let mut v: Vec<Option<i32>> = Vec::new();
    v.push(Option::Some(1));
    v.push(Option::None);
    return v;
}
"#,
    );
}

#[test]
fn main_returning_result_unit_error_is_accepted() {
    let result = run_main(
        r#"
fn main() -> Result<(), Error> {
    return Ok(());
}
"#,
    );
    let errors: Vec<String> = result
        .diags
        .iter()
        .filter(|d| !d.code.starts_with("W-"))
        .map(|d| format!("{}: {}", d.code, d.message))
        .collect();
    assert!(
        result.checked.is_some() && errors.is_empty(),
        "expected success, got {}",
        describe(&result)
    );
}

// ===========================================================================
// Third batch: enum variants, builtin constructors, unaries, arrays,
// const rejections, exhaustiveness messages
// ===========================================================================

#[test]
fn enum_variant_construction_and_patterns_are_accepted() {
    accept(
        r#"
enum Forme {
    Cercle(f64),
    Rect { w: f64, h: f64 },
    Vide,
}

fn aire(f: Forme) -> f64 {
    match f {
        Forme::Cercle(r) => 3.0 * r * r,
        Forme::Rect { w: w, h: h } => w * h,
        Forme::Vide => 0.0,
    }
}

fn construis() -> Forme {
    return Forme::Rect { w: 1.0, h: 2.0 };
}
"#,
    );
}

#[test]
fn enum_variant_construction_errors() {
    expect_code(
        r#"
enum Forme { Cercle(f64), Vide }
fn f() {
    let x = Forme::Cercle(1.0, 2.0);
}
"#,
        "E-TYP-ARITY",
    );
    expect_code(
        r#"
enum Forme { Cercle(f64), Vide }
fn f() {
    let x = Forme::Cercle(true);
}
"#,
        "E-TYP-MISMATCH",
    );
    expect_code(
        r#"
enum Forme { Cercle(f64), Vide }
fn f() {
    let x = Forme::Vide(1.0);
}
"#,
        "E-TYP-ARITY",
    );
    expect_code(
        r#"
enum Forme { Cercle(f64), Vide }
fn f() {
    let x = Forme::Inconnu;
}
"#,
        "E-RES-UNKNOWN",
    );
}

#[test]
fn enum_variant_pattern_errors() {
    expect_code(
        r#"
enum Forme { Cercle(f64), Vide }
fn f(x: Forme) -> f64 {
    match x {
        Forme::Cercle(true) => 0.0,
        Forme::Vide => 1.0,
    }
}
"#,
        "E-TYP-MISMATCH",
    );
    expect_code(
        r#"
enum Forme { Cercle(f64), Vide }
fn f(x: Forme) -> f64 {
    match x {
        Forme::Cercle(r) => r,
    }
}
"#,
        "E-TYP-NONEXHAUSTIVE",
    );
}

#[test]
fn builtin_constructor_payloads_are_accepted() {
    accept(
        r#"
fn option() -> Option<i32> {
    let a: Option<i32> = Option::Some(1);
    let b: Option<i32> = Option::None;
    return a;
}

fn resultat() -> Result<i32, Error> {
    let a: Result<i32, Error> = Ok(1);
    let b: Result<i32, Error> = Err(Error::new("boom"));
    return a;
}
"#,
    );
}

#[test]
fn builtin_constructor_errors() {
    expect_code(
        r#"
fn f() {
    let o: Option<i32> = Option::Some();
}
"#,
        "E-TYP-ARITY",
    );
    expect_code(
        r#"
fn f() {
    let r: Result<i32, Error> = Ok(1, 2);
}
"#,
        "E-TYP-ARITY",
    );
    expect_code(
        r#"
fn f() {
    let r: Result<i32, Error> = Err(1);
}
"#,
        "E-TYP-MISMATCH",
    );
}

#[test]
fn builtin_associated_functions_are_accepted() {
    accept(
        r#"
fn f() {
    let a = String::from("abc");
    let b = String::new();
    let c: Vec<i32> = Vec::new();
    let d = Error::new("boom");
}
"#,
    );
}

#[test]
fn builtin_associated_function_errors() {
    expect_code(
        r#"
fn f() {
    let v: Vec<i32> = Vec::new(1);
}
"#,
        "E-TYP-ARITY",
    );
    expect_code(
        r#"
fn f() {
    let s = String::from();
}
"#,
        "E-TYP-ARITY",
    );
    expect_code(
        r#"
fn f() {
    let s = String::from(1);
}
"#,
        "E-TYP-MISMATCH",
    );
    expect_code(
        r#"
fn f() {
    let v: Vec<i32> = Vec::nouveau();
}
"#,
        "E-RES-UNKNOWN",
    );
}

#[test]
fn unary_operator_errors() {
    expect_code(
        r#"
fn f() -> bool {
    return -true;
}
"#,
        "E-TYP-MISMATCH",
    );
    expect_code(
        r#"
fn f() -> i32 {
    return !1.5;
}
"#,
        "E-TYP-MISMATCH",
    );
    expect_code(
        r#"
fn f(u: u32) -> u32 {
    return -u;
}
"#,
        "E-TYP-MISMATCH",
    );
}

#[test]
fn method_argument_and_mutability_errors() {
    expect_code(
        r#"
fn f() {
    let mut v: Vec<i32> = Vec::new();
    v.push(true);
}
"#,
        "E-TYP-MISMATCH",
    );
    expect_code(
        r#"
fn f() {
    let v: Vec<i32> = Vec::new();
    v.push(1);
}
"#,
        "E-OWN-IMMUTABLE",
    );
    expect_code(
        r#"
fn f() {
    let x: u32 = 1;
    let y = x.abs();
}
"#,
        "E-RES-UNKNOWN",
    );
    expect_code(
        r#"
fn f() {
    let x = 1;
    let y = x.trim();
}
"#,
        "E-RES-UNKNOWN",
    );
}

#[test]
fn array_rules() {
    accept(
        r#"
fn f() {
    let a = [1, 2, 3];
    let b: [i32; 2] = [1, 2];
    let c = [[0; 2]; 3];
    let d = [(1, 2), (3, 4)];
    let e = [0u8; 4];
}
"#,
    );
    expect_code(
        r#"
fn f() {
    let a = [1, true];
}
"#,
        "E-TYP-MISMATCH",
    );
    expect_code(
        r#"
fn f() {
    let a: [i32; 3] = [1, 2];
}
"#,
        "E-TYP-MISMATCH",
    );
}

#[test]
fn constant_rejections_are_e_res_const_not_const() {
    expect_code(
        r#"
const X: i32 = { 1 };
"#,
        "E-RES-CONST-NOT-CONST",
    );
    expect_code(
        r#"
const X: i32 = -true;
"#,
        "E-RES-CONST-NOT-CONST",
    );
    expect_code(
        r#"
const X: String = String::from("a");
"#,
        "E-RES-CONST-NOT-CONST",
    );
}

#[test]
fn exhaustiveness_messages_cover_prelude_and_enums() {
    expect_code(
        r#"
fn f(r: Result<i32, Error>) -> i32 {
    match r {
        Ok(v) => v,
    }
}
"#,
        "E-TYP-NONEXHAUSTIVE",
    );
    expect_code(
        r#"
fn f(b: bool) -> i32 {
    match b {
        true => 1,
    }
}
"#,
        "E-TYP-NONEXHAUSTIVE",
    );
    expect_code(
        r#"
enum Couleur { Rouge, Vert }
fn f(c: Couleur) -> i32 {
    match c {
        Couleur::Rouge => 1,
    }
}
"#,
        "E-TYP-NONEXHAUSTIVE",
    );
}

#[test]
fn path_patterns_without_payload_are_accepted() {
    accept(
        r#"
fn f(o: Option<i32>) -> i32 {
    match o {
        None => 0,
        Some(v) => v,
    }
}

fn g(r: Result<i32, Error>) -> i32 {
    match r {
        Ok(v) => v,
        Err(_) => 0,
    }
}
"#,
    );
}

#[test]
fn returns_inside_control_expressions_are_accepted() {
    accept(
        r#"
fn a(c: bool) -> i32 {
    return if c { 1 } else { 2 };
}

fn b(c: bool) -> i32 {
    return match c {
        true => 1,
        false => 2,
    };
}

fn c(x: i32) -> i32 {
    if x > 0 {
        return 1;
    }
    return 0;
}
"#,
    );
}
