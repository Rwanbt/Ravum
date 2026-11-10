//! Contract tests, third batch (`typeck.rs`): `test fn` signatures, mixed
//! float comparisons, inference through `Vec::push`, and the compound
//! assignment family.
//!
//! Same contract as `typeck_contracts.rs`/`typeck_contracts2.rs`: each case is
//! a small program and the diagnostic code that `spec/language/` requires.
//! Every contract cites the rule it pins; a case rejected for a different
//! reason than the expected code fails, because the failure message lists
//! every diagnostic.
//!
//! These contracts pin the corrections recorded in
//! `docs/qualification/T2-FINDINGS.md` §19.2 items 4, 5 and 13, plus the
//! compound-assignment family of `grammar.md` G-6.8 / `expressions.md`
//! EXPR-9.4 (decision D-CMP-1).
//!
//! Run with:
//! `cargo test -p ravum-lang-sema --test typeck_contracts3`

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

/// Like [`expect_code`], in a binary crate (`src/main.rv`).
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

/// The expected code must be reported, the forbidden one must not: used when
/// the defect was a *wrong code* for a case that was already rejected.
fn expect_code_not(src: &str, expected: &str, forbidden: &str) {
    let result = run(src);
    assert!(
        result.checked.is_none(),
        "expected {expected} but the program was accepted\n--- source ---\n{src}"
    );
    let reported = codes(&result);
    assert!(
        reported.iter().any(|c| c == expected),
        "expected {expected}, got {reported:?}\n--- source ---\n{src}"
    );
    assert!(
        !reported.iter().any(|c| c == forbidden),
        "{forbidden} must no longer be reported, got {reported:?}\n--- source ---\n{src}"
    );
}

/// The expected code must be reported with a message that names `needle`
/// (used to pin that a diagnostic speaks about the *right* operator/type).
fn expect_code_msg(src: &str, expected: &str, needle: &str) {
    let result = run(src);
    assert!(
        result.checked.is_none(),
        "expected {expected} but the program was accepted\n--- source ---\n{src}"
    );
    let matched = result
        .diags
        .iter()
        .any(|d| d.code == expected && d.message.contains(needle));
    assert!(
        matched,
        "expected {expected} naming {needle:?}, got {:?}\n--- source ---\n{src}",
        describe(&result)
    );
}

/// The program must be accepted with no *error* diagnostic. Warnings are
/// allowed (this file tests typing, not lint suppressions).
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
// 1. `test fn` signature (`T2-FINDINGS.md` §19.2 item 4)
//    G-3.6 (MUST), MOD-4.10 (MUST), diagnostics.md: an invalid `test fn` is
//    `E-TYP-MAIN` ("`test fn` invalide"), not `E-PARSE-EXPECTED`.
// ===========================================================================

#[test]
fn test_fn_parameter_is_e_typ_main() {
    // G-3.6: "`test_item` MAY NOT be `pub` and has NO parameter"; MOD-4.10:
    // "n'a aucun paramètre". diagnostics.md assigns `E-TYP-MAIN` to an invalid
    // `test fn`. Before the correction the checker emitted `E-PARSE-EXPECTED`,
    // a code the grammar reserves for a malformed *form*, not a bad signature.
    expect_code_not(
        r#"
test fn t(x: i32) {
}
"#,
        "E-TYP-MAIN",
        "E-PARSE-EXPECTED",
    );
}

#[test]
fn test_fn_invalid_return_is_e_typ_main() {
    // G-3.6 (MUST): the return type is `()` or `Result<(), Error>`
    // (`E-TYP-MAIN` otherwise). Both wrong arities/kinds are covered.
    expect_code_not(
        r#"
test fn t() -> i32 {
    return 1;
}
"#,
        "E-TYP-MAIN",
        "E-PARSE-EXPECTED",
    );
    expect_code(
        r#"
test fn t() -> Result<i32, Error> {
    return Ok(1);
}
"#,
        "E-TYP-MAIN",
    );
}

#[test]
fn test_fn_valid_signatures_are_accepted() {
    // Positive half of G-3.6/MOD-4.10: exactly the two documented forms.
    accept(
        r#"
test fn t() {
}
test fn u() -> Result<(), Error> {
    return Ok(());
}
"#,
    );
}

#[test]
fn test_fn_invalid_signature_in_binary_crate_is_e_typ_main() {
    // The rule is about the `test fn` itself, not the crate kind
    // (diagnostics.md, G-3.6): a binary crate with a valid `main` still
    // reports `E-TYP-MAIN` on the test signature.
    expect_code_main(
        r#"
fn main() {
}
test fn bad(a: i32) {
}
"#,
        "E-TYP-MAIN",
    );
}

// ===========================================================================
// 2. Mixed numeric comparisons (`T2-FINDINGS.md` §19.2 item 5)
//    EXPR-4.8 (MUST): `f32` and `f64` never mix implicitly — the code is
//    `E-TYP-MISMATCH`. TYP-7.3/TYP-7.4: `E-TYP-EQ`/`E-TYP-CMP` are reserved
//    for an operator not defined on an otherwise well-typed (equal) operand
//    type; TYP-13.2 keeps contextual literal inference.
// ===========================================================================

#[test]
fn mixed_float_equality_is_e_typ_mismatch() {
    // EXPR-4.8 (MUST) names `E-TYP-MISMATCH`; the old `E-TYP-EQ` said the
    // operator was undefined on each type, which is false (`==` is defined on
    // both `f32` and `f64` — TYP-7.3): the operands simply disagree.
    expect_code_not(
        r#"
fn f(a: f32, b: f64) -> bool {
    return a == b;
}
"#,
        "E-TYP-MISMATCH",
        "E-TYP-EQ",
    );
    // Same rule for `!=`.
    expect_code_not(
        r#"
fn f(a: f32, b: f64) -> bool {
    return a != b;
}
"#,
        "E-TYP-MISMATCH",
        "E-TYP-EQ",
    );
}

#[test]
fn mixed_float_ordering_is_e_typ_mismatch() {
    // EXPR-4.8 (MUST) covers every comparison that mixes `f32`/`f64`.
    for operator in ["<", "<=", ">", ">="] {
        let src = format!(
            r#"
fn f(a: f32, b: f64) -> bool {{
    return a {operator} b;
}}
"#
        );
        expect_code_not(&src, "E-TYP-MISMATCH", "E-TYP-CMP");
    }
}

#[test]
fn mixed_integer_or_float_vs_integer_comparison_is_e_typ_mismatch() {
    // TYP-13.2 / EXPR-10.5: no implicit conversion repairs an operand type.
    // Two numeric types that differ therefore disagree (`E-TYP-MISMATCH`),
    // they are not "non comparable" (`E-TYP-EQ` would claim the operator is
    // undefined on each of them, which TYP-7.3 contradicts).
    expect_code_not(
        r#"
fn f(a: i32, b: i64) -> bool {
    return a == b;
}
"#,
        "E-TYP-MISMATCH",
        "E-TYP-EQ",
    );
    expect_code_not(
        r#"
fn f(a: f32, b: i32) -> bool {
    return a < b;
}
"#,
        "E-TYP-MISMATCH",
        "E-TYP-CMP",
    );
}

#[test]
fn contextual_float_literal_comparison_stays_accepted() {
    // TYP-13.2 (MUST): a float literal takes the context type. `a == 1.5` with
    // `a: f32` must stay accepted by the correction of EXPR-4.8.
    accept(
        r#"
fn f(a: f32) -> bool {
    return (a == 1.5) && (a < 2.5);
}
fn g(b: f64) -> bool {
    return (b == 1.5) && (b >= 0.0);
}
"#,
    );
}

#[test]
fn non_comparable_equal_types_keep_e_typ_eq() {
    // TYP-7.3: `==` is not defined on tuples, `Option`, `Vec`; the operands
    // have the *same* type, so the canonical code stays `E-TYP-EQ`.
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
    let result = run(r#"
fn f(a: Vec<i32>, b: Vec<i32>) -> bool {
    return a == b;
}
"#);
    assert!(
        codes(&result).iter().any(|c| c == "E-TYP-EQ"),
        "expected E-TYP-EQ, got {:?}",
        codes(&result)
    );
}

#[test]
fn ordering_on_text_keeps_e_typ_cmp() {
    // TYP-7.4 (MUST): `< <= > >=` are not defined on `&str`/`String`.
    expect_code(
        r#"
fn f(a: &str, b: &str) -> bool {
    return a < b;
}
"#,
        "E-TYP-CMP",
    );
}

#[test]
fn text_equality_mixes_are_still_accepted() {
    // TYP-7.3 (MUST): `==` compares text by content and admits every mixture
    // of `String`/`&str`. The `E-TYP-MISMATCH` correction must not catch it.
    accept(
        r#"
fn f(a: &str, s: String) -> bool {
    return (a == s) && (a == "x");
}
"#,
    );
}

// ===========================================================================
// 3. Inference through `Vec::push` (`T2-FINDINGS.md` §19.2 item 13)
//    stdlib.md STD-10.3: `push(x)` appends an element of type `T`;
//    TYP-13.2: the element type is the one the constraints determine. The
//    element variable used to stay unsubstituted in the types returned to
//    comparisons (`v[i]`), which produced a spurious `E-TYP-MISMATCH` on
//    `s += v[i]`.
// ===========================================================================

#[test]
fn vec_push_infers_element_type_for_later_uses() {
    // `Vec::new()` has no annotation: `push(1)` must bind `T = i32` and the
    // later `+=`, `+` and `=` uses must see `i32` (not `$0`).
    accept(
        r#"
fn sum() -> i32 {
    let mut v = Vec::new();
    v.push(1);
    let mut s = 0;
    let mut i: usize = 0;
    while i < v.len() {
        s += v[i];
        i += 1;
    }
    return s;
}
"#,
    );
    accept(
        r#"
fn plus() -> i32 {
    let mut v = Vec::new();
    v.push(1);
    return v[0] + 1;
}
"#,
    );
    accept(
        r#"
fn assign() {
    let mut v = Vec::new();
    v.push(1);
    let mut y = 0;
    y = v[0];
}
"#,
    );
}

#[test]
fn vec_push_infers_float_and_char_elements() {
    // The same propagation must work for every element type inferable from a
    // literal (TYP-13.2): float literal defaults to `f64`, `char` is `char`.
    accept(
        r#"
fn f() -> f64 {
    let mut v = Vec::new();
    v.push(1.5);
    return v[0] * 2.0;
}
"#,
    );
    accept(
        r#"
fn f() -> bool {
    let mut v = Vec::new();
    v.push('a');
    let c = v[0];
    return c == 'a';
}
"#,
    );
}

#[test]
fn unconstrained_vec_new_stays_e_typ_ambiguous() {
    // TYP-13.2: `Vec::new()` with nothing to infer `T` from is ambiguous. The
    // correction must not invent a default element type.
    expect_code(
        r#"
fn f() {
    let v = Vec::new();
}
"#,
        "E-TYP-AMBIGUOUS",
    );
}

#[test]
fn vec_push_of_diverging_value_stays_e_typ_ambiguous() {
    // TYP-1.6 / F-TYP-004: `!` never determines a type argument, so
    // `v.push(panic("x"))` must stay `E-TYP-AMBIGUOUS` (corpus case
    // `types/never-infer-vec-fail`).
    expect_code(
        r#"
fn f() {
    let mut v = Vec::new();
    v.push(panic("x"));
}
"#,
        "E-TYP-AMBIGUOUS",
    );
}

#[test]
fn vec_push_conflicting_element_is_e_typ_mismatch() {
    // Once `T = i32` is inferred, a second `push` of another type cannot
    // change it (TYP-13.2: no retroactive unification).
    expect_code(
        r#"
fn f() {
    let mut v = Vec::new();
    v.push(1);
    v.push(1.5);
}
"#,
        "E-TYP-MISMATCH",
    );
    expect_code(
        r#"
fn f() {
    let mut v = Vec::new();
    v.push('a');
    v.push(1);
}
"#,
        "E-TYP-MISMATCH",
    );
}

// ===========================================================================
// 4. Compound assignment family
//    grammar.md G-6.8 (MUST) and expressions.md EXPR-9.4 (MUST) declare
//    `+= -= *= /= %= &= |= ^= <<= >>=` accepted; semantics is
//    `place = place op e` with `place` evaluated once. The semantic rules per
//    operator mirror `check_binary` (TYP-13.2, TYP-7.3/TYP-7.4, EXPR-3.4).
// ===========================================================================

#[test]
fn every_compound_assignment_operator_is_accepted() {
    // All ten operators (G-6.8) on a type where the underlying binary
    // operator is defined. `+=`-only used to be accepted: each of the nine
    // others emitted `E-UNSUPPORTED-COMPOUND-ASSIGN`.
    accept(
        r#"
fn integers(a: i32, b: i32) -> i32 {
    let mut x = a;
    x += b;
    x -= b;
    x *= b;
    x /= b;
    x %= b;
    x &= b;
    x |= b;
    x ^= b;
    x <<= b;
    x >>= b;
    return x;
}
"#,
    );
    // Floating-point arithmetic (`%` is IEEE `fmod`, TYP-7.6).
    accept(
        r#"
fn floats(a: f64, b: f64) -> f64 {
    let mut x = a;
    x += b;
    x -= b;
    x *= b;
    x /= b;
    x %= b;
    return x;
}
"#,
    );
    // Bitwise assignment on `bool` is the second documented case.
    accept(
        r#"
fn booleans(p: bool, q: bool) -> bool {
    let mut x = p;
    x &= q;
    x |= q;
    x ^= q;
    return x;
}
"#,
    );
}

#[test]
fn compound_assignment_shift_allows_different_integer_widths() {
    // EXPR-3.4 / check_binary: the two shift operands are integers, their
    // widths may differ; the result has the left operand's type.
    accept(
        r#"
fn f(a: i32, b: i64) -> i32 {
    let mut x = a;
    x <<= b;
    x >>= b;
    return x;
}
"#,
    );
}

#[test]
fn compound_assignment_uses_contextual_literals() {
    // TYP-13.2: the value literal takes the place's type, for every operator
    // of the family (`x -= 1` on `f32` is `1f32`, not `f64`).
    accept(
        r#"
fn f() {
    let mut x: f32 = 1.0;
    x -= 1.0;
    x *= 2.0;
    let mut n: i64 = 0;
    n += 1;
    n <<= 1;
}
"#,
    );
}

#[test]
fn compound_assignment_float_mismatch_is_e_typ_mismatch() {
    // TYP-13.2 / EXPR-4.8: no implicit `f64` → `f32` conversion.
    expect_code_msg(
        r#"
fn f(mut a: f32, b: f64) {
    a -= b;
}
"#,
        "E-TYP-MISMATCH",
        "-=",
    );
}

#[test]
fn compound_assignment_bool_arithmetic_is_e_typ_mismatch() {
    // G-6.8 only accepts `+=` where `+` exists: `+` is undefined on `bool`
    // (check_binary: arithmetic needs two integers or two floats).
    expect_code_msg(
        r#"
fn f(mut b: bool) {
    b += true;
}
"#,
        "E-TYP-MISMATCH",
        "+=",
    );
}

#[test]
fn compound_assignment_text_is_e_typ_mismatch() {
    // No text concatenation operator exists in Core 1.0 (TYP-7.3 lists text
    // only for `==`/`!=`), so `+=` on a `String` is a type mismatch.
    expect_code_msg(
        r#"
fn f(mut s: String) {
    s += "x";
}
"#,
        "E-TYP-MISMATCH",
        "+=",
    );
}

#[test]
fn compound_assignment_negative_shift_is_e_typ_shift() {
    // EXPR-3.4: a negative shift amount is refused statically, with the code
    // that `check_binary` already uses for `<<`/`>>`.
    expect_code(
        r#"
fn f(mut x: i32) {
    x <<= -1;
}
"#,
        "E-TYP-SHIFT",
    );
}

#[test]
fn compound_assignment_immutable_place_is_e_own_immutable() {
    // EXPR-9.1/OWN: the place must be mutable, whatever the operator.
    for src in [
        r#"
fn f(x: i32) {
    x += 1;
}
"#,
        r#"
fn f(x: i32) {
    x -= 1;
}
"#,
        r#"
fn f(x: i32) {
    x <<= 1;
}
"#,
        r#"
fn f(b: bool) {
    b &= true;
}
"#,
    ] {
        expect_code(src, "E-OWN-IMMUTABLE");
    }
}

#[test]
fn compound_assignment_reports_only_the_immutability_error() {
    // D-CMP-1: the family is accepted, so for an immutable place the only
    // diagnostic is `E-OWN-IMMUTABLE`. Before the correction every operator
    // except `+=` was refused with the retired compound-assignment code; an
    // exact code-set assertion proves that no other diagnostic is emitted
    // (the retired code is not even named here, so the registry audit stays
    // honest).
    for operator in ["-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>="] {
        let src = format!(
            r#"
fn f(x: i32) {{
    x {operator} 1;
}}
"#
        );
        let result = run(&src);
        assert!(
            result.checked.is_none(),
            "expected the immutable place to be rejected\n--- source ---\n{src}"
        );
        assert_eq!(
            codes(&result),
            vec!["E-OWN-IMMUTABLE".to_string()],
            "expected only E-OWN-IMMUTABLE for `{operator}`, got {:?}\n--- source ---\n{src}",
            codes(&result)
        );
    }
}
