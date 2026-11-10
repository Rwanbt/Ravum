//! Second batch of contract tests for the Ravum type checker (`typeck.rs`).
//!
//! Every case is a small program whose expected diagnostics come from
//! `spec/language/` (rule ids are quoted in the comment above each case):
//! `types.md`, `expressions.md`, `grammar.md`, `modules.md`, `ownership.md`,
//! `stdlib.md` and `diagnostics.md`.  The tests are written against the
//! specification, not against the current implementation: a program the spec
//! requires to be accepted must be accepted (`accept` allows the documented
//! `W-*` warnings), and a program the spec rejects must be rejected with the
//! exact canonical code (`expect_code`).
//!
//! Run with:
//! `cargo test -p ravum-lang-sema --test typeck_contracts2`

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
/// allowed (the checker documents `W-OWN-UNUSED-VARIABLE` and
/// `W-RES-UNUSED-IMPORT`), but they are still shown in a failure message.
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
// Calls: callee shapes and value paths (EXPR-7.1, EXPR-7.8, EXPR-7.9)
// ===========================================================================

#[test]
fn non_path_callee_is_e_typ_call() {
    // EXPR-7.9: calling a non-function value (`x(1)`) is `E-TYP-CALL`.
    expect_code(
        r#"
fn f() {
    let x = (1 + 2)(3);
}
"#,
        "E-TYP-CALL",
    );
    // A parenthesized `if` is also a non-function callee.
    expect_code(
        r#"
fn f(c: bool) {
    let x = (if c { 1 } else { 2 })();
}
"#,
        "E-TYP-CALL",
    );
    // The result of a call is not callable either.
    expect_code(
        r#"
fn g() -> i32 { return 1; }
fn f() {
    let x = g()();
}
"#,
        "E-TYP-CALL",
    );
}

#[test]
fn local_callee_checks_arguments_then_e_typ_call() {
    // EXPR-7.9: a local binding is not callable; the arguments are still
    // type-checked (`E-TYP-MISMATCH` for the extra one) but the canonical
    // call diagnostic is reported.
    expect_code(
        r#"
fn f() {
    let x = 1;
    x(true);
}
"#,
        "E-TYP-CALL",
    );
}

#[test]
fn constant_callee_is_e_typ_call() {
    // EXPR-7.9: only functions and constructors are callable; a `const` is a
    // value, not a function.
    expect_code(
        r#"
const C: i32 = 1;
fn f() {
    C(1);
}
"#,
        "E-TYP-CALL",
    );
}

#[test]
fn unit_variant_call_is_e_typ_arity() {
    // G-6.29: `None` is a value, not a function. Calling it is an arity
    // problem (`E-TYP-ARITY`), not "not callable".
    expect_code(
        r#"
fn f() {
    let x = Option::None(1);
}
"#,
        "E-TYP-ARITY",
    );
}

#[test]
fn builtin_module_cannot_be_value_or_callee() {
    // `std::io` is a module: it is neither a value nor callable.
    expect_code(
        r#"
fn f() {
    let x = std::io;
}
"#,
        "E-TYP-CALL",
    );
    expect_code(
        r#"
fn f() {
    std::io();
}
"#,
        "E-TYP-CALL",
    );
}

#[test]
fn type_cannot_be_value_or_callee() {
    // A type name is not a value; `i32(...)` is not a constructor in Core 1.0.
    expect_code(
        r#"
fn f() {
    let x = i32;
}
"#,
        "E-TYP-CALL",
    );
    expect_code(
        r#"
fn f() {
    let x = i32(1);
}
"#,
        "E-TYP-CALL",
    );
}

#[test]
fn function_and_constructor_values_require_calls() {
    // EXPR-7.9 / G-6.29: a function item, an associated function, a payload
    // variant constructor and `Some` are not values.
    expect_code(
        r#"
fn g() -> i32 { return 1; }
fn f() {
    let x = g;
}
"#,
        "E-TYP-CALL",
    );
    expect_code(
        r#"
struct S { n: i32 }
impl S {
    fn make() -> S { return S { n: 1 }; }
}
fn f() {
    let x = S::make;
}
"#,
        "E-TYP-CALL",
    );
    expect_code(
        r#"
enum E { V(i32), W }
fn f() {
    let x = E::V;
}
"#,
        "E-TYP-CALL",
    );
    expect_code(
        r#"
fn f() {
    let x = Option::Some;
}
"#,
        "E-TYP-CALL",
    );
}

#[test]
fn ufcs_is_e_res_unknown() {
    // EXPR-7.8: `Type::method(receiver, ...)` (UFCS) does not exist in Core
    // 1.0; the canonical diagnostic is a resolution failure, not a type error.
    expect_code(
        r#"
struct S { n: i32 }
impl S {
    fn m(&self) -> i32 { return self.n; }
}
fn f(s: S) -> i32 {
    return S::m(s);
}
"#,
        "E-RES-UNKNOWN",
    );
}

#[test]
fn assoc_fn_generic_impl_binds_expected_type() {
    // TYP-11.4: the `impl<T> Boite<T>` generics are bound by the expected
    // `Boite<i32>` type, so the associated function is callable without
    // turbofish syntax (none exists in Core 1.0).
    accept(
        r#"
struct Boite<T> { valeur: T }

impl<T> Boite<T> {
    fn enveloppe(v: T) -> Boite<T> {
        return Boite { valeur: v };
    }
}

fn f() -> Boite<i32> {
    return Boite::enveloppe(1);
}
"#,
    );
}

// ===========================================================================
// Struct literals (G-6.25, G-6.26, TYP-11)
// ===========================================================================

#[test]
fn struct_literal_must_target_a_struct() {
    // G-6.25: only a nominal struct (or a struct variant) can be built with
    // `{ ... }`; an enum type is not a struct.
    expect_code(
        r#"
enum E { V }
fn f() {
    let x = E { };
}
"#,
        "E-RES-UNKNOWN",
    );
    // A primitive type has no fields.
    expect_code(
        r#"
fn f() {
    let x = i32 { };
}
"#,
        "E-RES-UNKNOWN",
    );
}

#[test]
fn struct_literal_field_rules() {
    // G-6.26 / EXPR-7.5: unknown field, duplicate field and a private field
    // accessed outside its module each have a canonical diagnostic.
    expect_code(
        r#"
struct P { x: i32 }
fn f() -> P {
    return P { z: 1 };
}
"#,
        "E-TYP-FIELD",
    );
    expect_code(
        r#"
struct P { x: i32 }
fn f() -> P {
    return P { x: 1, x: 2 };
}
"#,
        "E-TYP-FIELD",
    );
    expect_code(
        r#"
mod m {
    pub struct S { n: i32 }
}
fn f() -> m::S {
    return m::S { n: 1 };
}
"#,
        "E-RES-PRIVATE",
    );
}

#[test]
fn generic_struct_literal_inference() {
    // TYP-11.4 / TYP-13.4: the expected type supplies the generic argument,
    // and each field is checked against the substituted field type.
    accept(
        r#"
struct B<T> { v: T }
fn f() -> B<i32> {
    return B { v: 1 };
}
"#,
    );
    expect_code(
        r#"
struct B<T> { v: T }
fn f() -> B<i32> {
    return B { v: true };
}
"#,
        "E-TYP-MISMATCH",
    );
    // TYP-1.6: `!` must not determine a generic field type.
    expect_code(
        r#"
struct W<T> { v: T }
fn f() {
    let w = W { v: panic("x") };
}
"#,
        "E-TYP-AMBIGUOUS",
    );
    // Without any constraint the generic argument stays free.
    expect_code(
        r#"
struct B<T> { v: T }
fn f() {
    let x = B { };
}
"#,
        "E-TYP-AMBIGUOUS",
    );
}

#[test]
fn struct_variant_literal_rules() {
    // G-6.25: a struct literal cannot target a unit or tuple variant.
    expect_code(
        r#"
enum E { V(i32), W }
fn f() -> E {
    return E::V { x: 1 };
}
"#,
        "E-TYP-FIELD",
    );
    expect_code(
        r#"
enum E { V, W }
fn f() -> E {
    return E::V { x: 1 };
}
"#,
        "E-TYP-FIELD",
    );
    // A struct variant literal must provide every field.
    expect_code(
        r#"
enum E { V { x: i32, y: i32 } }
fn f() -> E {
    return E::V { x: 1 };
}
"#,
        "E-TYP-MISSING-FIELD",
    );
    // The expected enum type supplies the generic argument of the variant.
    accept(
        r#"
enum E<T> { V { x: T }, W }
fn f() -> E<i32> {
    return E::V { x: 1 };
}
"#,
    );
    // With no field and no expected type the enum's generics are uninferable.
    expect_code(
        r#"
enum E<T> { V { x: T } }
fn f() {
    let e = E::V { };
}
"#,
        "E-TYP-AMBIGUOUS",
    );
}

#[test]
fn builtin_constant_struct_literal_is_checked() {
    // `Option::None` is a value, so a `{ ... }` literal on it resolves to a
    // unit constructor and leaves the type argument uninferable (TYP-1.6).
    expect_code(
        r#"
fn f() {
    let x = Option::None { };
}
"#,
        "E-TYP-AMBIGUOUS",
    );
}

// ===========================================================================
// Variant constructors and variant patterns (G-6.29, TYP-10)
// ===========================================================================

#[test]
fn variant_call_generic_inference() {
    // G-6.29 / TYP-11.4: the expected nominal type binds the enum generics.
    accept(
        r#"
enum E<T> { V(T), W }
fn f() -> E<i32> {
    return E::V(1);
}
"#,
    );
    // A unit variant leaves the generic free without an expected type.
    expect_code(
        r#"
enum E<T> { V(T), W }
fn f() {
    let e = E::W;
}
"#,
        "E-TYP-AMBIGUOUS",
    );
    // TYP-1.6: a diverging payload must not determine the generic argument.
    expect_code(
        r#"
enum E<T> { V(T), W }
fn f() {
    let e = E::V(panic("x"));
}
"#,
        "E-TYP-AMBIGUOUS",
    );
    // ... but an explicit expected type makes the divergence harmless.
    accept(
        r#"
enum E<T> { V(T), W }
fn f() -> E<i32> {
    return E::V(panic("x"));
}
"#,
    );
}

#[test]
fn variant_call_kind_errors() {
    // G-6.29: a struct variant is built with `{ field: value }`, never with
    // parentheses.
    expect_code(
        r#"
enum E { V { x: i32 } }
fn f() {
    let e = E::V(1);
}
"#,
        "E-TYP-CALL",
    );
    // A variant constructor result must match its expected type.
    expect_code(
        r#"
enum E { V(i32) }
fn f() -> Option<i32> {
    return E::V(1);
}
"#,
        "E-TYP-MISMATCH",
    );
}

#[test]
fn enum_variant_payload_errors() {
    // G-6.29: tuple-variant arity and payload types are checked.
    expect_code(
        r#"
enum E { V(i32, i32), W }
fn f(e: E) -> i32 {
    match e {
        E::V(a) => a,
        E::W => 0,
    }
}
"#,
        "E-TYP-ARITY",
    );
    // A struct-variant pattern must name existing fields and every field.
    expect_code(
        r#"
enum E { V { x: i32, y: i32 } }
fn f(e: E) -> i32 {
    match e {
        E::V { x: a } => a,
    }
}
"#,
        "E-TYP-MISSING-FIELD",
    );
    expect_code(
        r#"
enum E { V { x: i32 } }
fn f(e: E) -> i32 {
    match e {
        E::V { z: a } => a,
    }
}
"#,
        "E-TYP-FIELD",
    );
    // The field-list form must match the variant's kind.
    expect_code(
        r#"
enum E { V(i32) }
fn f(e: E) -> i32 {
    match e {
        E::V { x: a } => a,
        _ => 0,
    }
}
"#,
        "E-TYP-ARITY",
    );
    expect_code(
        r#"
enum E { V { x: i32 } }
fn f(e: E) -> i32 {
    match e {
        E::V(a) => a,
        _ => 0,
    }
}
"#,
        "E-TYP-ARITY",
    );
}

#[test]
fn variant_pattern_resolution_and_scrutinee() {
    // TYP-10.2: a user variant must be qualified and must belong to the
    // scrutinee's enum.
    expect_code(
        r#"
enum E { V(i32) }
fn f(e: E) -> i32 {
    match e {
        E::Inconnu(v) => v,
        _ => 0,
    }
}
"#,
        "E-RES-UNKNOWN",
    );
    expect_code(
        r#"
enum E { V(i32) }
fn f(x: i32) -> i32 {
    match x {
        E::V(v) => v,
        _ => 0,
    }
}
"#,
        "E-TYP-MISMATCH",
    );
    // A path that names a function is not a variant pattern.
    expect_code(
        r#"
fn g() -> i32 { return 1; }
fn f(x: i32) -> i32 {
    match x {
        crate::g(v) => v,
        _ => 0,
    }
}
"#,
        "E-RES-UNKNOWN",
    );
}

#[test]
fn builtin_variant_pattern_errors() {
    // G-6.29 / TYP-10.2: prelude constructors are checked like enum variants.
    expect_code(
        r#"
fn f(o: Option<i32>) -> i32 {
    match o {
        Some(a, b) => a,
        None => 0,
    }
}
"#,
        "E-TYP-ARITY",
    );
    expect_code(
        r#"
fn f(o: Option<i32>) -> i32 {
    match o {
        Some { v: a } => a,
        None => 0,
    }
}
"#,
        "E-TYP-ARITY",
    );
    expect_code(
        r#"
fn f(r: Result<i32, Error>) -> i32 {
    match r {
        Ok { v: a } => a,
        Err(_) => 0,
    }
}
"#,
        "E-TYP-ARITY",
    );
    expect_code(
        r#"
fn f(x: i32) -> i32 {
    match x {
        Some(v) => v,
        _ => 0,
    }
}
"#,
        "E-TYP-MISMATCH",
    );
    expect_code(
        r#"
fn f(x: i32) -> i32 {
    match x {
        Ok(v) => v,
        _ => 0,
    }
}
"#,
        "E-TYP-MISMATCH",
    );
    expect_code(
        r#"
fn f(x: i32) -> i32 {
    match x {
        None(v) => 1,
        _ => 0,
    }
}
"#,
        "E-TYP-MISMATCH",
    );
}

#[test]
fn path_patterns_reference_constants() {
    // G-6.38: a bare identifier is a binding; a `const` pattern must be a
    // qualified path. Its type must match the scrutinee.
    accept(
        r#"
const N: i32 = 3;
fn f(x: i32) -> i32 {
    match x {
        crate::N => 1,
        _ => 0,
    }
}
"#,
    );
    expect_code(
        r#"
const N: i32 = 3;
fn f(x: u8) -> i32 {
    match x {
        crate::N => 1,
        _ => 0,
    }
}
"#,
        "E-TYP-MISMATCH",
    );
    // A function item is not a constant.
    expect_code(
        r#"
fn g() -> i32 { return 1; }
fn f(x: i32) -> i32 {
    match x {
        crate::g => 1,
        _ => 0,
    }
}
"#,
        "E-RES-UNKNOWN",
    );
    // `Result::Ok` without a payload is not a valid path pattern (`Ok` needs a
    // tuple field, G-6.29); the diagnostic is a resolution/field error.
    expect_code(
        r#"
fn f(r: Result<i32, Error>) -> i32 {
    match r {
        Result::Ok => 1,
        _ => 0,
    }
}
"#,
        "E-RES-UNKNOWN",
    );
    // An unknown qualified path is a resolution failure.
    expect_code(
        r#"
fn f(x: i32) -> i32 {
    match x {
        crate::Nope => 1,
        _ => 0,
    }
}
"#,
        "E-RES-UNKNOWN",
    );
}

#[test]
fn path_patterns_reference_unit_variants() {
    // TYP-10.2 / G-6.29: a unit variant used as a path pattern is exhaustive
    // for its own enum and must match the scrutinee's type.
    accept(
        r#"
enum E { V, W }
fn f(e: E) -> i32 {
    match e {
        E::V => 1,
        E::W => 2,
    }
}
"#,
    );
    expect_code(
        r#"
enum E { V, W }
fn f(x: i32) -> i32 {
    match x {
        E::V => 1,
        _ => 2,
    }
}
"#,
        "E-TYP-MISMATCH",
    );
    // `Option::None` as a path pattern must match `Option<_>`.
    expect_code(
        r#"
fn f(x: i32) -> i32 {
    match x {
        Option::None => 1,
        _ => 0,
    }
}
"#,
        "E-TYP-MISMATCH",
    );
    // A diverging scrutinee (`!`) is compatible with every pattern.
    accept(
        r#"
enum E { V, W }
fn f() -> i32 {
    match panic("x") {
        E::V => 1,
        _ => 0,
    }
}
"#,
    );
}

// ===========================================================================
// Literals and literal patterns (TYP-13.2, G-6.36, G-6.37)
// ===========================================================================

#[test]
fn literal_patterns_type_check_against_scrutinee() {
    // TYP-7.3 / G-6.36: a literal pattern must match the scrutinee's type.
    expect_code(
        r#"
fn f(b: bool) -> i32 {
    match b {
        1 => 1,
        _ => 0,
    }
}
"#,
        "E-TYP-MISMATCH",
    );
    expect_code(
        r#"
fn f(b: bool) -> i32 {
    match b {
        -1 => 1,
        _ => 0,
    }
}
"#,
        "E-TYP-MISMATCH",
    );
    expect_code(
        r#"
fn f(x: i32) -> i32 {
    match x {
        'a' => 1,
        _ => 0,
    }
}
"#,
        "E-TYP-MISMATCH",
    );
    expect_code(
        r#"
fn f(x: i32) -> i32 {
    match x {
        "a" => 1,
        _ => 0,
    }
}
"#,
        "E-TYP-MISMATCH",
    );
    expect_code(
        r#"
fn f(x: bool) -> i32 {
    match x {
        'a' => 1,
        _ => 0,
    }
}
"#,
        "E-TYP-MISMATCH",
    );
}

#[test]
fn literal_pattern_integer_range() {
    // TYP-13.2 / G-6.37: the sign decides the bound. `-128` fits `i8`; `-129`
    // does not, and an unsigned scrutinee accepts no negative pattern.
    accept(
        r#"
fn f(x: i8) -> i32 {
    match x {
        -128 => 1,
        _ => 0,
    }
}
"#,
    );
    expect_code(
        r#"
fn f(x: i8) -> i32 {
    match x {
        -129 => 1,
        _ => 0,
    }
}
"#,
        "E-TYP-LITERAL-RANGE",
    );
    expect_code(
        r#"
fn f(x: u32) -> i32 {
    match x {
        -1 => 1,
        _ => 0,
    }
}
"#,
        "E-TYP-LITERAL-RANGE",
    );
}

#[test]
fn literal_suffixes_set_the_type() {
    // TYP-13.2 / LEX: each integer suffix fixes the literal's type, and
    // float suffixes select `f32`/`f64`. These must all be accepted.
    accept(
        r#"
fn f() {
    let a = 1i8;
    let b = 1i16;
    let c = 1i32;
    let d = 1i64;
    let e = 1i128;
    let g = 1u8;
    let h = 1u16;
    let i = 1u32;
    let j = 1u64;
    let k = 1u128;
    let l = 1isize;
    let m = 1usize;
    let n = 1.5f32;
    let o = 1.5f64;
}

fn p(x: i64) -> i32 {
    match x {
        1i64 => 1,
        _ => 0,
    }
}

fn q(x: f32) -> i32 {
    match x {
        1.5f32 => 1,
        -2.5 => 2,
        _ => 0,
    }
}
"#,
    );
}

// ===========================================================================
// Unary and binary operators (EXPR-8, TYP-13)
// ===========================================================================

#[test]
fn unary_operator_rules() {
    // TYP-13.2: `-N` where N is a signed literal keeps the suffix type.
    accept(
        r#"
fn f() -> i64 {
    return -5i64;
}
"#,
    );
    // Negating an unsigned value is a type mismatch.
    expect_code(
        r#"
fn f() -> u32 {
    return -1;
}
"#,
        "E-TYP-MISMATCH",
    );
    // A literal that does not fit the negated type is out of range.
    expect_code(
        r#"
fn f() -> i8 {
    let x: i8 = -129;
    return x;
}
"#,
        "E-TYP-LITERAL-RANGE",
    );
    // `!` is defined on bool and integers only.
    accept(
        r#"
fn f(x: u8) -> u8 {
    return !x;
}
"#,
    );
    expect_code(
        r#"
fn f(x: f64) -> f64 {
    return !x;
}
"#,
        "E-TYP-MISMATCH",
    );
    // Dereference requires a reference.
    expect_code(
        r#"
fn f(x: i32) -> i32 {
    return *x;
}
"#,
        "E-TYP-MISMATCH",
    );
    // `!` coerces: a diverging operand is accepted by every unary operator.
    accept(
        r#"
fn f() -> i32 {
    return -panic("x");
}
fn g() -> bool {
    return !panic("x");
}
fn h() -> i32 {
    return *panic("x");
}
"#,
    );
}

#[test]
fn binary_operator_rules() {
    // TYP-13.2 / EXPR-4.8: no implicit numeric conversion.
    expect_code(
        r#"
fn f(a: i32, b: i64) -> i32 {
    return a + b;
}
"#,
        "E-TYP-MISMATCH",
    );
    expect_code(
        r#"
fn f(a: i32, b: f64) {
    let c = a + b;
}
"#,
        "E-TYP-MISMATCH",
    );
    // EXPR-8: shifts require integer operands.
    expect_code(
        r#"
fn f(a: f64) -> i64 {
    return a << 1;
}
"#,
        "E-TYP-MISMATCH",
    );
    // `&`/`|`/`^` accept two bools or two integers of the same type.
    accept(
        r#"
fn f(a: bool, b: bool) -> bool {
    return a & b;
}
"#,
    );
    expect_code(
        r#"
fn f(a: i32, b: bool) -> i32 {
    return a & b;
}
"#,
        "E-TYP-MISMATCH",
    );
    // `&&`/`||` require bools.
    expect_code(
        r#"
fn f(a: i32, b: i32) -> bool {
    return a && b;
}
"#,
        "E-TYP-MISMATCH",
    );
    // Ordering is defined on integers, floats and chars, never on bool.
    expect_code(
        r#"
fn f(a: bool, b: bool) -> bool {
    return a < b;
}
"#,
        "E-TYP-CMP",
    );
    // A shift amount may be a variable (only literals are range-checked).
    accept(
        r#"
fn f(a: i32, b: i32) -> i32 {
    return a << b;
}
"#,
    );
}

// ===========================================================================
// Assignment and places (EXPR-9, TYP-9, OWN-7)
// ===========================================================================

#[test]
fn assignment_place_mutability() {
    // A field, an index and a dereference of a `&mut` are mutable places.
    accept(
        r#"
struct S { n: i32 }
fn f(mut s: S, mut a: [i32; 2], p: &mut i32) {
    s.n = 1;
    a[0] = 1;
    *p = 1;
}
"#,
    );
    // A field of an immutable parameter is not a mutable place.
    expect_code(
        r#"
struct S { n: i32 }
fn f(s: S) {
    s.n = 1;
}
"#,
        "E-OWN-IMMUTABLE",
    );
    expect_code(
        r#"
fn f(a: [i32; 2]) {
    a[0] = 1;
}
"#,
        "E-OWN-IMMUTABLE",
    );
    // A shared dereference is immutable.
    expect_code(
        r#"
fn f(r: &i32) {
    *r = 1;
}
"#,
        "E-OWN-IMMUTABLE",
    );
    // A deferred `let x: T;` binding may be initialised once without `mut`.
    accept(
        r#"
fn f() -> i32 {
    let x: i32;
    x = 1;
    return x;
}
"#,
    );
}

#[test]
fn compound_assignment_type_rules() {
    // EXPR-9.4: `place = place op e` requires operands of the same numeric
    // type; `+=` on floats is admitted.
    accept(
        r#"
fn f() {
    let mut x = 1.5;
    x += 1.0;
}
"#,
    );
    expect_code(
        r#"
fn f() {
    let mut x = 1;
    x += 1.0;
}
"#,
        "E-TYP-MISMATCH",
    );
}

#[test]
fn builtin_exclusive_receiver_needs_a_mutable_place() {
    // EXPR-7.3 / OWN-7: `Vec::push` takes `&mut self`.
    accept(
        r#"
struct S { v: Vec<i32> }
fn f(mut s: S, mut t: (Vec<i32>,), mut a: [Vec<i32>; 1], p: &mut Vec<i32>) {
    s.v.push(1);
    t.0.push(1);
    a[0].push(1);
    p.push(1);
}
"#,
    );
    // A temporary is not a mutable place.
    expect_code(
        r#"
fn f() {
    Vec::new().push(1);
}
"#,
        "E-OWN-IMMUTABLE",
    );
    // A shared reference is not a mutable place either.
    expect_code(
        r#"
fn f(r: &Vec<i32>) {
    (*r).push(1);
}
"#,
        "E-OWN-IMMUTABLE",
    );
}

// ===========================================================================
// Control flow and divergence (G-6.19, EXPR-5, EXPR-6)
// ===========================================================================

#[test]
fn control_expressions_are_statements_not_values() {
    // EXPR-5.11/6.7, G-6.19: `while`/`loop`/`for` have type `()` and are not
    // values; they cannot be bound, passed, or used as an element.
    expect_code(
        r#"
fn f(c: bool) {
    let x = while c { };
}
"#,
        "E-TYP-STATEMENT-VALUE",
    );
    expect_code(
        r#"
fn f() {
    let x = loop { break; };
}
"#,
        "E-TYP-STATEMENT-VALUE",
    );
    expect_code(
        r#"
fn f() {
    let x = for i in 0..1 { };
}
"#,
        "E-TYP-STATEMENT-VALUE",
    );
    expect_code(
        r#"
fn g(x: i32) { }
fn f(c: bool) {
    g(loop { });
}
"#,
        "E-TYP-STATEMENT-VALUE",
    );
    expect_code(
        r#"
fn f(c: bool) -> i32 {
    match c {
        true => while c { },
        false => 1,
    }
}
"#,
        "E-TYP-STATEMENT-VALUE",
    );
}

#[test]
fn for_iteration_forms() {
    // G-6.24 / EXPR-6.4: the range form is exclusive and needs both bounds.
    expect_code(
        r#"
fn f() {
    for i in 0..=4 { }
}
"#,
        "E-TYP-RANGE",
    );
    expect_code(
        r#"
fn f() {
    for i in ..4 { }
}
"#,
        "E-TYP-RANGE",
    );
    // The collection form consumes an owned `Vec<T>`.
    accept(
        r#"
fn f(v: Vec<i32>) -> i32 {
    let mut s = 0;
    for x in v {
        s += x;
    }
    return s;
}
"#,
    );
    // Other types are not iterable.
    expect_code(
        r#"
fn f(s: String) {
    for c in s { }
}
"#,
        "E-TYP-MISMATCH",
    );
    expect_code(
        r#"
fn f(s: String) {
    for c in &s { }
}
"#,
        "E-TYP-MISMATCH",
    );
    // TYP-1.6: `!` cannot be consumed by `for`.
    expect_code(
        r#"
fn f() {
    for c in panic("x") { }
}
"#,
        "E-TYP-MISMATCH",
    );
}

#[test]
fn if_branch_rules() {
    // G-6.20 / TYP-13.3: both branches must have the same type.
    expect_code(
        r#"
fn f(c: bool) -> i32 {
    let x = if c { 1 } else { true };
    return 0;
}
"#,
        "E-TYP-MISMATCH",
    );
    // A diverging `then` branch does not constrain the result type.
    accept(
        r#"
fn f(c: bool) -> i32 {
    let x: i32 = if c { return 1; } else { 2 };
    return x;
}
fn g(c: bool) -> i32 {
    return if c { panic("x") } else { 1 };
}
"#,
    );
}

#[test]
fn match_arm_and_expected_types() {
    // G-6.21 / TYP-13.3: all arms share one type.
    expect_code(
        r#"
fn f(c: bool) -> i32 {
    match c {
        true => 1,
        false => true,
    }
}
"#,
        "E-TYP-MISMATCH",
    );
    // The `match` result must match its expected type.
    expect_code(
        r#"
fn f(c: bool) -> i32 {
    let x: bool = match c {
        true => 1,
        false => 2,
    };
    return 0;
}
"#,
        "E-TYP-MISMATCH",
    );
    // A diverging arm is compatible with any other arm.
    accept(
        r#"
fn f(c: bool) -> i32 {
    match c {
        true => panic("x"),
        false => 2,
    }
}
"#,
    );
    // G-6.22: a non-enumerable scrutinee is covered by `_`.
    accept(
        r#"
struct S { n: i32 }
fn f(s: S) -> i32 {
    match s {
        _ => 0,
    }
}
"#,
    );
}

#[test]
fn return_and_never_rules() {
    // G-5.11 / EXPR-3: `return;` is only valid in a `()` function.
    expect_code(
        r#"
fn f() -> i32 {
    return;
}
"#,
        "E-TYP-RETURN",
    );
    // A `-> !` function cannot return a value, and `return;` is not divergent.
    expect_code(
        r#"
fn f() -> ! {
    return 1;
}
"#,
        "E-TYP-RETURN",
    );
    expect_code(
        r#"
fn f() -> ! {
    return;
}
"#,
        "E-TYP-RETURN",
    );
    // G-5.11: `loop` has type `()` (EXPR-6.2), so it is not a divergent body.
    expect_code(
        r#"
fn f() -> ! {
    loop {
        panic("x");
    }
}
"#,
        "E-TYP-RETURN",
    );
    // `panic` is divergent and therefore a valid `-> !` body.
    accept(
        r#"
fn f() -> ! {
    panic("x")
}
"#,
    );
}

#[test]
fn try_operator_error_contexts() {
    // TYP-10: `?` on `Result<_, E>` requires the function to return a
    // `Result<_, E>` (same error type).
    expect_code(
        r#"
fn f() -> Result<i32, Error> {
    let r: Result<i32, bool> = Ok(1);
    let x = r?;
    return Ok(x);
}
"#,
        "E-TYP-TRY",
    );
    // `?` on `Option<_>` requires an `Option<_>` return type.
    expect_code(
        r#"
fn f() -> Result<i32, Error> {
    let o: Option<i32> = None;
    let x = o?;
    return Ok(x);
}
"#,
        "E-TYP-TRY",
    );
    // ... and is accepted in a compatible context.
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
// Aggregates: arrays and tuples (TYP-4, TYP-5)
// ===========================================================================

#[test]
fn array_inference_rules() {
    // TYP-5.7: an empty array has no element type.
    expect_code(
        r#"
fn f() {
    let a = [];
}
"#,
        "E-TYP-AMBIGUOUS",
    );
    // TYP-1.6: a diverging element leaves the element type uninferable ...
    expect_code(
        r#"
fn f() {
    let a = [panic("x"), 1];
}
"#,
        "E-TYP-AMBIGUOUS",
    );
    expect_code(
        r#"
fn f() {
    let a = [panic("x"); 3];
}
"#,
        "E-TYP-AMBIGUOUS",
    );
    // ... unless an annotation supplies it.
    accept(
        r#"
fn f() -> [i32; 2] {
    let a: [i32; 2] = [panic("x"), 1];
    return a;
}
fn g() -> [u8; 3] {
    let b: [u8; 3] = [0; 3];
    return b;
}
"#,
    );
    // EXPR-5.11: control expressions are not array elements.
    expect_code(
        r#"
fn f(c: bool) {
    let a = [1, while c { }];
}
"#,
        "E-TYP-STATEMENT-VALUE",
    );
}

#[test]
fn tuple_inference_rules() {
    // TYP-4: tuples are heterogeneous and each element is checked separately.
    accept(
        r#"
fn f() -> (i32, bool) {
    return (1, true);
}
"#,
    );
    // TYP-1.6: a diverging element is uninferable without an annotation.
    expect_code(
        r#"
fn f() {
    let t = (panic("x"),);
}
"#,
        "E-TYP-AMBIGUOUS",
    );
    // ... and coerces when the annotation supplies the element type.
    accept(
        r#"
fn f() {
    let t: (i32, i32) = (panic("x"), 1);
}
"#,
    );
    // EXPR-5.11: control expressions are not tuple elements.
    expect_code(
        r#"
fn f(c: bool) {
    let t = (1, while c { });
}
"#,
        "E-TYP-STATEMENT-VALUE",
    );
}

#[test]
fn tuple_and_index_access_rules() {
    // TYP-4.5: out-of-bounds tuple index and `.n` on a non-tuple.
    expect_code(
        r#"
fn f(t: (i32, i32)) -> i32 {
    return t.2;
}
"#,
        "E-TYP-FIELD",
    );
    expect_code(
        r#"
fn f(x: i32) -> i32 {
    return x.0;
}
"#,
        "E-TYP-FIELD",
    );
    // EXPR-7.6: `&[T]` is indexable; a non-sequence type is not.
    accept(
        r#"
fn f(s: &[i32], i: usize) -> i32 {
    return s[i];
}
"#,
    );
    expect_code(
        r#"
fn f() {
    let x = 1;
    let y = x[0];
}
"#,
        "E-TYP-INDEX",
    );
}

// ===========================================================================
// Constant evaluation (TYP-13, DIAG E-TYP-CONST)
// ===========================================================================

#[test]
fn constant_folding_operators() {
    // TYP-13: every operator on integer/bool constants is folded.
    accept(
        r#"
const A: i32 = 1 + 2 * 3 - (10 - 4) / 2 + 7 % 3;
const B: u8 = 0b1100 & 0b1010 | 0b0001 ^ 0b0011;
const C: u64 = 1 << 40;
const D: u128 = 1 << 100;
const E: u8 = 255 >> 4;
const F: bool = 1 < 2 && 3 >= 3;
const G: bool = true || false;
const H: bool = !false;
const I: i32 = !0;
const J: i8 = -100 - 20;
const K: char = 'a';
const L: &str = "texte";
"#,
    );
}

#[test]
fn constant_evaluation_errors() {
    // TYP-13 / DIAG: division and remainder by zero, shift overflow and
    // arithmetic overflow are `E-TYP-CONST`.
    expect_code(
        r#"
const X: i32 = 1 / 0;
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
const X: i8 = 127 + 1;
"#,
        "E-TYP-CONST",
    );
    expect_code(
        r#"
const X: i32 = 1 << -1;
"#,
        "E-TYP-CONST",
    );
    expect_code(
        r#"
const X: u8 = 1 << 200;
"#,
        "E-TYP-CONST",
    );
    expect_code(
        r#"
const X: u8 = 128 << 1;
"#,
        "E-TYP-CONST",
    );
    // A non-integer operand is a type mismatch, not a constant error.
    expect_code(
        r#"
const X: i32 = true + 1;
"#,
        "E-TYP-MISMATCH",
    );
}

#[test]
fn constant_casts_are_folded() {
    // TYP-13.5/13.6: admitted `as` pairs fold to a `ConstValue`.
    accept(
        r#"
const A: i64 = 1 as i64;
const B: u8 = 300 as u8;
const C: i128 = 300 as i128;
const D: i32 = true as i32;
const E: i32 = 'a' as i32;
const F: f64 = 1 as f64;
const G: bool = true as bool;
const H: char = 'a' as char;
"#,
    );
    // TYP-13.8: no `as` conversion for text.
    expect_code(
        r#"
const X: &str = "a" as &str;
"#,
        "E-TYP-CAST",
    );
    // A float constant is not representable in the facts (DIAG).
    expect_code(
        r#"
const X: f64 = 1.5;
"#,
        "E-RES-CONST-NOT-CONST",
    );
}

#[test]
fn constant_expression_restrictions() {
    // DIAG: only the constant-expression grammar is allowed.
    expect_code(
        r#"
fn g() -> i32 { return 1; }
const X: i32 = g();
"#,
        "E-RES-CONST-NOT-CONST",
    );
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
    // G-5.11: `!` is not a valid constant type.
    expect_code(
        r#"
const X: ! = panic("x");
"#,
        "E-TYP-MISMATCH",
    );
}

#[test]
fn constant_int_value_limits() {
    // TYP-5.7: an array length must be a constant integer.
    accept(
        r#"
mod m {
    pub const N: usize = 3;
}
fn f() -> [i32; 3] {
    let a: [i32; 3] = [0; crate::m::N];
    return a;
}
"#,
    );
    expect_code(
        r#"
const N: bool = true;
fn f() {
    let a = [0; N];
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
}

// ===========================================================================
// Entry points (G-3.6, diagnostics)
// ===========================================================================

#[test]
fn main_signature_rules() {
    // DIAG E-TYP-MAIN: `main` returns `()` or `Result<(), Error>`.
    expect_code_main(
        r#"
fn main() -> i32 {
    return 0;
}
"#,
        "E-TYP-MAIN",
    );
    expect_code_main(
        r#"
fn main(x: i32) {
}
"#,
        "E-TYP-MAIN",
    );
    // The documented valid forms.
    let result = run_main(
        r#"
fn main() -> Result<(), Error> {
    return Ok(());
}
"#,
    );
    assert!(
        result.checked.is_some(),
        "valid `main` rejected: {}",
        describe(&result)
    );
}

#[test]
fn test_fn_signature_rules() {
    // F-SYN-004 / G-3.6 / MOD-4.10 / diagnostics.md: a `test fn` has no
    // parameter, and an invalid signature is a *typing* error named
    // `E-TYP-MAIN` by the registry ("`test fn` invalide"), not a parse error.
    // This contract previously encoded `E-PARSE-EXPECTED`, which contradicted
    // all three rules; `T2-FINDINGS.md` §19.2 item 4 tracked the defect.
    expect_code(
        r#"
test fn t(x: i32) {
}
"#,
        "E-TYP-MAIN",
    );
    // The return type must be `()` or `Result<(), Error>` (G-3.6).
    expect_code(
        r#"
test fn t() -> i32 {
    return 1;
}
"#,
        "E-TYP-MAIN",
    );
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

// ===========================================================================
// Static-text analysis (`&'static str` propagation, TYP-6.4)
// ===========================================================================

#[test]
fn static_text_analysis_traverses_every_expression_form() {
    // TYP-6.4 / OWN-6.2: a function returning `&str` is `'static` only when
    // every explicit return yields static text. The analysis must walk every
    // expression shape without losing that property: this body contains one
    // of each form and a final static return.
    accept(
        r#"
struct S { n: i32 }

fn lit() -> &str {
    return "a";
}

fn giant(c: bool, mut b: bool, arr: [i32; 3], s: S) -> &str {
    let a = -1;
    let n = 1 + 2;
    let mut m = 3;
    m += 1;
    b = true;
    let f = s.n;
    let t = (1, 2).0;
    let e = arr[0];
    let call = lit();
    let mc = lit().replace("a", "b");
    let cast = 1 as i64;
    let blk = { 1 };
    let tup = (1, 2);
    let ar = [1, 2];
    let rep = [0; 2];
    let st = S { n: 1 };
    if c { }
    while c { }
    loop { }
    for i in 0..1 { }
    unsafe { };
    return "z";
}
"#,
    );
}

#[test]
fn static_text_candidates_include_impl_methods() {
    // TYP-6.4: an `impl` method returning `&str` is part of the fixpoint.
    accept(
        r#"
struct S { n: i32 }

impl S {
    fn lit(&self) -> &str {
        return "a";
    }
}
"#,
    );
}

#[test]
fn static_text_unsafe_block_is_static() {
    // G-6.40: `unsafe { ... }` is a block expression whose type is the
    // block's type; a static text inside remains static.
    accept(
        r#"
fn f() -> &str {
    return unsafe { "a" };
}
"#,
    );
}

#[test]
fn static_text_empty_return_is_invalid() {
    // TYP-6.4: `return;` cannot produce the `&str` the function promises.
    expect_code(
        r#"
fn f(c: bool) -> &str {
    if c {
        return;
    }
    return "a";
}
"#,
        "E-TYP-RETURN",
    );
    // `?` is not defined on `&text`.
    expect_code(
        r#"
fn lit() -> &str { return "a"; }
fn f() -> &str {
    let x = lit()?;
    return "a";
}
"#,
        "E-TYP-TRY",
    );
}

// ===========================================================================
// Inference and generics (TYP-11, TYP-13.4)
// ===========================================================================

#[test]
fn generic_call_instantiations_accepted() {
    // TYP-11.3: every generic shape in a signature is instantiated and
    // unified structurally.
    accept(
        r#"
struct Boite<T> { valeur: T }

fn arr<T>(x: [T; 2]) -> T {
    return x[0];
}
fn tup<T>(x: (T, T)) -> T {
    return x.0;
}
fn vec<T>(x: Vec<T>) -> usize {
    return x.len();
}
fn opt<T>(x: Option<T>) -> bool {
    return x.is_some();
}
fn res<T, E>(x: Result<T, E>) -> bool {
    return x.is_ok();
}
fn nom<T>(x: Boite<T>) -> i32 {
    return 0;
}

fn g() -> i32 {
    let x: [i32; 2] = [1, 2];
    let a = arr(x);
    let b = tup((1, 2));
    let c: Vec<i32> = Vec::new();
    let d = vec(c);
    let o: Option<i32> = Option::None;
    let r: Result<i32, Error> = Ok(1);
    let b2: Boite<i32> = Boite { valeur: 1 };
    let e = nom(b2);
    return a + b + e;
}
"#,
    );
    // TYP-11.1: a generic parameter is opaque; no method is resolvable.
    expect_code(
        r#"
fn f<T>(x: T) {
    x.m();
}
"#,
        "E-RES-UNKNOWN",
    );
}

#[test]
fn inference_failures_are_e_typ_mismatch() {
    // TYP-13.4: no retroactive unification. A free inference variable that
    // cannot be bound by the expected type is a mismatch.
    expect_code(
        r#"
fn f() {
    let x: i32 = Vec::new();
}
"#,
        "E-TYP-MISMATCH",
    );
    expect_code(
        r#"
fn f() {
    let x: i32 = Option::Some(Vec::new());
}
"#,
        "E-TYP-MISMATCH",
    );
    expect_code(
        r#"
fn f() {
    let x: i32 = &Vec::new();
}
"#,
        "E-TYP-MISMATCH",
    );
    expect_code(
        r#"
fn f() {
    let x: i32 = (Vec::new(),);
}
"#,
        "E-TYP-MISMATCH",
    );
    expect_code(
        r#"
fn f() {
    let x: i32 = Ok(Vec::new());
}
"#,
        "E-TYP-MISMATCH",
    );
    expect_code(
        r#"
struct Boite<T> { valeur: T }
fn f() {
    let x: i32 = Boite { valeur: Vec::new() };
}
"#,
        "E-TYP-MISMATCH",
    );
}

#[test]
fn occurs_check_rejects_an_infinite_type() {
    // TYP-11: `v.push(v)` on an unannotated `Vec` would require
    // `Vec<T> = T`, an infinite type; the argument is a type mismatch (and
    // the variable stays ambiguous).
    expect_code(
        r#"
fn f() {
    let mut v = Vec::new();
    v.push(v);
}
"#,
        "E-TYP-MISMATCH",
    );
}

#[test]
fn never_type_is_not_an_inferable_aggregate_argument() {
    // TYP-1.6: `!` coerces towards an expected type but must never determine
    // a type argument.
    expect_code(
        r#"
fn f() {
    let o = Option::Some(panic("x"));
}
"#,
        "E-TYP-AMBIGUOUS",
    );
    // With an expected type the divergence is harmless.
    accept(
        r#"
fn f() -> Option<i32> {
    return Option::Some(panic("x"));
}
"#,
    );
}

// ===========================================================================
// Methods (EXPR-7.4, EXPR-7.5, TYP-11.1, TYP-14.4)
// ===========================================================================

#[test]
fn method_resolution_errors() {
    // TYP-14.4: `Vec<T>::clone` requires a `Copy` element.
    expect_code(
        r#"
fn f(v: Vec<String>) {
    let c = v.clone();
}
"#,
        "E-RES-UNKNOWN",
    );
    // An unknown method on a nominal type.
    expect_code(
        r#"
struct S { n: i32 }
fn f(s: S) {
    s.nope(1);
}
"#,
        "E-RES-UNKNOWN",
    );
    // An unknown builtin method on a primitive.
    expect_code(
        r#"
fn f() {
    let x = 1;
    x.nope(1);
}
"#,
        "E-RES-UNKNOWN",
    );
}

#[test]
fn method_argument_errors() {
    // EXPR-7.1: method arity is exact.
    expect_code(
        r#"
struct S { n: i32 }
impl S {
    fn m(&self) -> i32 { return self.n; }
}
fn f(s: S) {
    s.m(1);
}
"#,
        "E-TYP-ARITY",
    );
    // The argument type is checked against the parameter.
    expect_code(
        r#"
struct S { n: i32 }
impl S {
    fn m(&self, a: i32) -> i32 { return a; }
}
fn f(s: S) {
    s.m(true);
}
"#,
        "E-TYP-MISMATCH",
    );
}

#[test]
fn private_members_are_e_res_private() {
    // EXPR-7.5 / RES: a private field or method is only visible in its
    // module subtree.
    expect_code(
        r#"
mod m {
    pub struct S { n: i32 }
}
fn f(s: m::S) -> i32 {
    return s.n;
}
"#,
        "E-RES-PRIVATE",
    );
    expect_code(
        r#"
mod m {
    pub struct S { pub n: i32 }
    impl S {
        fn secret(&self) -> i32 { return self.n; }
    }
}
fn f(s: m::S) -> i32 {
    return s.secret();
}
"#,
        "E-RES-PRIVATE",
    );
    // A public field is visible everywhere.
    accept(
        r#"
mod m {
    pub struct S { pub n: i32 }
}
fn f(s: m::S) -> i32 {
    return s.n;
}
"#,
    );
}

// ===========================================================================
// Builtin calls and coercion (EXPR-7, TYP-13.1, STD-8.2)
// ===========================================================================

#[test]
fn builtin_call_arity_and_argument_types() {
    // EXPR-7.1: extra arguments are an arity error.
    expect_code(
        r#"
fn f() {
    assert(true, false);
}
"#,
        "E-TYP-ARITY",
    );
    // Argument types are unified with the builtin signature.
    expect_code(
        r#"
fn f() {
    let x = std::math::abs(true);
}
"#,
        "E-TYP-MISMATCH",
    );
}

#[test]
fn assert_eq_contract() {
    // STD-8.2: `assert_eq` takes exactly two arguments.
    expect_code(
        r#"
fn f() {
    assert_eq();
}
"#,
        "E-TYP-ARITY",
    );
    expect_code(
        r#"
fn f() {
    assert_eq(1);
}
"#,
        "E-TYP-ARITY",
    );
    // STD-8.2: text, bool, char and integers are comparable.
    accept(
        r#"
fn f(a: &str, b: &str, c: char, d: bool, e: i64) {
    assert_eq(a, b);
    assert_eq(c, c);
    assert_eq(d, d);
    assert_eq(e, e);
    assert_eq(1, e);
}
"#,
    );
    // No equality on `Option`/`Result`/aggregates in Core 1.0.
    expect_code(
        r#"
fn f(o: Option<i32>) {
    assert_eq(o, o);
}
"#,
        "E-TYP-EQ",
    );
}

#[test]
fn implicit_coercions_are_limited_to_the_spec() {
    // TYP-13.1: `&[T; N] -> &[T]` is an implicit adjustment in argument
    // position; `&String -> &str` is the other one. EXPR-7.10: the borrow
    // created by the `&s` argument expires at the end of the call.
    accept(
        r#"
fn takes_str(s: &str) { }
fn takes_slice(s: &[i32]) { }
fn f() {
    let s = String::from("a");
    takes_str(&s);
    let a = [1, 2];
    takes_slice(&a);
}
"#,
    );
    // TYP-13.1: there is no `&Vec<T> -> &[T]` coercion.
    expect_code(
        r#"
fn takes_slice(s: &[i32]) { }
fn f(v: Vec<i32>) {
    takes_slice(&v);
}
"#,
        "E-TYP-MISMATCH",
    );
}

// ===========================================================================
// Exhaustiveness messages (G-6.22, G-6.44)
// ===========================================================================

#[test]
fn exhaustiveness_messages_cover_every_enumerable_shape() {
    // G-6.22: `match` must be exhaustive; the diagnostic names what is
    // missing for bool, Option, Result and user enums.
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
fn f(b: bool) -> i32 {
    match b {
        false => 1,
    }
}
"#,
        "E-TYP-NONEXHAUSTIVE",
    );
    expect_code(
        r#"
fn f(o: Option<i32>) -> i32 {
    match o {
        Option::None => 0,
    }
}
"#,
        "E-TYP-NONEXHAUSTIVE",
    );
    expect_code(
        r#"
fn f(o: Option<i32>) -> i32 {
    match o {
        Some(v) => v,
    }
}
"#,
        "E-TYP-NONEXHAUSTIVE",
    );
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
fn f(r: Result<i32, Error>) -> i32 {
    match r {
        Err(_) => 0,
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
fn coverage_engine_tracks_pattern_shapes() {
    // G-6.36: a float sub-pattern is lowered to a guard and covers nothing,
    // so `Some(1.5)` does not cover `Some`: a qualified `Option::None` arm is
    // still non-exhaustive. (A bare `None` would be a *binding* arm under
    // G-6.38 and would hide the gap, so the qualified constructor is used.)
    expect_code(
        r#"
fn f(o: Option<f64>) -> i32 {
    match o {
        Some(1.5) => 1,
        Option::None => 0,
    }
}
"#,
        "E-TYP-NONEXHAUSTIVE",
    );
    // The same rule applies to a float nested in a variant payload.
    expect_code(
        r#"
fn f(o: Option<(f64, i32)>) -> i32 {
    match o {
        Some((1.5, _)) => 1,
        Option::None => 0,
    }
}
"#,
        "E-TYP-NONEXHAUSTIVE",
    );
    // ... and to a struct-variant field.
    expect_code(
        r#"
enum E { V { x: f64 } }
fn f(e: E) -> i32 {
    match e {
        E::V { x: 1.5 } => 1,
    }
}
"#,
        "E-TYP-NONEXHAUSTIVE",
    );
    // A variant pattern from another enum does not cover the scrutinee, but
    // it is first a type mismatch.
    expect_code(
        r#"
enum A { X }
enum B { Y }
fn f(a: A) -> i32 {
    match a {
        B::Y => 1,
        _ => 0,
    }
}
"#,
        "E-TYP-MISMATCH",
    );
    // A bool match with a non-bool literal also exercises the "other
    // pattern" path of the coverage engine (and is a mismatch).
    expect_code(
        r#"
fn f(b: bool) -> i32 {
    match b {
        1 => 1,
        _ => 0,
    }
}
"#,
        "E-TYP-MISMATCH",
    );
}

// ===========================================================================
// Misc: field visibility in subtrees, float literal forms
// ===========================================================================

#[test]
fn private_field_is_visible_in_child_modules() {
    // RES visibility: a private item is visible in its module and descendants.
    accept(
        r#"
mod m {
    pub struct S { n: i32 }
    pub mod inner {
        pub fn g(s: super::S) -> i32 {
            return s.n;
        }
    }
}
"#,
    );
}

#[test]
fn float_literal_forms_are_typed() {
    // TYP-13.2: float suffix and contextual typing, including `f32`.
    accept(
        r#"
fn f() -> f32 {
    return 1.5f32;
}
fn g() -> f64 {
    return 1.5f64;
}
fn h(x: f32) -> String {
    return x.to_string();
}
"#,
    );
    // TYP-13.5: float widen/narrow casts are admitted explicitly.
    accept(
        r#"
fn f(a: f32) -> f64 {
    return a as f64;
}
fn g(b: f64) -> f32 {
    return b as f32;
}
"#,
    );
}
// ===========================================================================
// Never type and pattern shapes (TYP-1.6, F-SYN-030)
// ===========================================================================

#[test]
fn never_type_is_not_constructible_or_annotatable() {
    // TYP-1.6: `!` is not a valid local type; the diverging initializer still
    // coerces, but the annotation itself is rejected.
    expect_code(
        r#"
fn f() {
    let x: ! = panic("x");
}
"#,
        "E-TYP-MISMATCH",
    );
    // TYP-1.6: `!` is not a valid cast target ...
    expect_code(
        r#"
fn f() -> i32 {
    return 1 as !;
}
"#,
        "E-TYP-CAST",
    );
    // ... but a diverging source coerces to any cast target.
    accept(
        r#"
fn f() -> i32 {
    return panic("x") as i32;
}
"#,
    );
}

#[test]
fn tuple_pattern_requires_a_tuple_scrutinee() {
    // F-SYN-030 / G-6.38: a tuple pattern has the exact arity of a tuple type;
    // against a non-tuple it is a type mismatch (the match also needs a `_`).
    expect_code(
        r#"
fn f(x: i32) -> i32 {
    match x {
        (a, b) => a,
        _ => 0,
    }
}
"#,
        "E-TYP-MISMATCH",
    );
}
