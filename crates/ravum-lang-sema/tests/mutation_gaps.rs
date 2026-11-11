//! Contract tests closing the mutation-testing gaps of the 2026-11-10
//! campaign (`target/mut5/`, pass 5).
//!
//! Each case targets a **named surviving mutant** of `cargo-mutants` on
//! `crates/ravum-lang-sema/src/{resolve,typeck}.rs` and cites the normative
//! rule of `spec/language/` that the mutant would break. The observable is
//! always a *behaviour*: an accepted/rejected program, the exact diagnostic, or
//! a value recorded in `facts` (the contract the code generator consumes, see
//! `lib.rs`: "A successful result is a facts::CheckedProgram that the code
//! generator may consume").
//!
//! Deaths that are *not* test gaps are documented in
//! `docs/qualification/MUTATION-EQUIVALENCE.md` §8 and are deliberately not
//! asserted here: a test that cannot fail on the mutated code would be a false
//! PASS.
//!
//! Run with:
//! `cargo test -p ravum-lang-sema --test mutation_gaps`

use ravum_lang_sema::syntax::diag::Severity;
use ravum_lang_sema::syntax::{parser, source::SourceMap, LANGUAGE_EDITION};
use ravum_lang_sema::{
    check, facts::ConstValue, CheckOptions, CheckResult, CheckedProgram, Profile, Resolution, Type,
};

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

fn codes(result: &CheckResult) -> Vec<String> {
    result.diags.iter().map(|d| d.code.clone()).collect()
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

/// The program must be accepted (warnings are allowed: these contracts pin
/// typing, not lint noise), and the checked program returned so the test can
/// inspect the facts the code generator reads.
fn accept(src: &str) -> CheckedProgram {
    let result = run(src);
    let errors: Vec<String> = result
        .diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.code.clone())
        .collect();
    assert!(
        errors.is_empty(),
        "expected the program to be accepted, got {errors:?}\n--- source ---\n{src}"
    );
    match result.checked {
        Some(checked) => checked,
        None => panic!("accepted program carries no CheckedProgram\n--- source ---\n{src}"),
    }
}

/// The program must be rejected with `expected` and a message naming `needle`.
fn expect_code_msg(src: &str, expected: &str, needle: &str) {
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
    let described = describe(&result);
    assert!(
        described.contains(needle),
        "expected a diagnostic naming `{needle}`, got: {described}\n--- source ---\n{src}"
    );
}

/// The message must name `needle`, whatever the code: used only where two
/// normative documents assign different codes to the same refusal (see the
/// comment of the calling test).
fn expect_message(src: &str, needle: &str) -> String {
    let result = run(src);
    assert!(
        result.checked.is_none(),
        "expected a diagnostic naming `{needle}` but the program was accepted\n--- source ---\n{src}"
    );
    let described = describe(&result);
    assert!(
        described.contains(needle),
        "expected a diagnostic naming `{needle}`, got: {described}\n--- source ---\n{src}"
    );
    described
}

/// The value recorded for the constant called `name`, i.e. what the emitter
/// would write for it.
fn const_value(checked: &CheckedProgram, name: &str) -> Option<ConstValue> {
    checked
        .facts
        .consts
        .values()
        .find(|c| c.name == name)
        .map(|c| c.value.clone())
}

// ===========================================================================
// `typeck.rs::check_struct_lit` / `check_variant_call` — generic arguments of
// an aggregate literal inferred from the expected type (TYP-11, TYP-13.2)
// ===========================================================================
//
// A literal such as `Boite { v: 3 }` has no explicit generic arguments, so its
// `T` comes from the expected type. These four contracts separate the two
// sources of inference: the *expected type* and the *field value*.

/// Mutant: `typeck.rs:3984:33 replace == with != in check_struct_lit`
/// (`*exp_def == def` when seeding `subst` from the expected type).
///
/// Rule: TYP-13.2 — an integer literal takes the type imposed by its *context*,
/// and here the context is the declared `Boite<usize>`; TYP-11 forbids leaving
/// the literal at its `i32` default and then failing the annotation.
#[test]
fn struct_literal_takes_its_generic_argument_from_the_expected_type() {
    accept(
        r#"
struct Boite<T> { v: T }

fn f() -> Boite<usize> {
    let b: Boite<usize> = Boite { v: 3 };
    b
}
"#,
    );
}

/// Mutant: `typeck.rs:3984:58 replace == with != in check_struct_lit`
/// (`exp_args.len() == info.generics.len()`). Same rule as the preceding
/// contract: with a matching arity the expected type must seed `subst`.
#[test]
fn struct_literal_with_matching_arity_uses_the_expected_arguments() {
    accept(
        r#"
struct Paire<A, B> { a: A, b: B }

fn f() -> Paire<usize, bool> {
    let p: Paire<usize, bool> = Paire { a: 1, b: true };
    p
}
"#,
    );
}

/// Mutant: `typeck.rs:3984:40 replace && with || in check_struct_lit`.
///
/// Rule: TYP-11/TYP-13.2 — only the expected type of the *same* aggregate may
/// seed the generic arguments. `A` and `B` are distinct structs: the literal
/// keeps the type its field gives it (`A<i32>`, the default of `3`), and the
/// diagnostic must say so rather than silently retyping the field to `usize`.
#[test]
fn a_struct_literal_is_not_retyped_by_an_unrelated_expected_type() {
    let result = run(r#"
struct A<T> { x: T }
struct B<T> { y: T }

fn f() -> B<usize> {
    let b: B<usize> = A { x: 3 };
    b
}
"#);
    let described = describe(&result);
    assert!(
        described.contains("E-TYP-MISMATCH") && described.contains("found `A<i32>`"),
        "the literal keeps the type of its field (`A<i32>`), got: {described}"
    );
}

/// Mutant: `typeck.rs:4047:33 replace == with != in check_struct_lit` (the
/// struct-variant literal seeding `subst` from the expected enum).
///
/// Rule: TYP-13.2 + TYP-11 — same inference as a struct literal, for a
/// struct-variant literal: the field `v: 3` must take `usize` from the declared
/// `Boite<usize>`.
#[test]
fn variant_literal_takes_its_generic_argument_from_the_expected_type() {
    accept(
        r#"
enum Boite<T> { Valeur { v: T }, Vide }

fn f() -> Boite<usize> {
    let b: Boite<usize> = Boite::Valeur { v: 3 };
    b
}
"#,
    );
}

/// Mutant: `typeck.rs:4047:46 replace && with || in check_struct_lit`.
///
/// Rule: TYP-11 — an expected type of a *different* enum must not rewrite the
/// generic arguments of a variant literal: `A2::Va { x: 3 }` stays `A2<i32>`
/// and the diagnostic names it.
#[test]
fn a_variant_literal_is_not_retyped_by_an_unrelated_expected_type() {
    let result = run(r#"
enum A2<T> { Va { x: T } }
enum B2<T> { Vb { y: T } }

fn f() -> B2<usize> {
    let b: B2<usize> = A2::Va { x: 3 };
    b
}
"#);
    let described = describe(&result);
    assert!(
        described.contains("E-TYP-MISMATCH") && described.contains("found `A2<i32>`"),
        "the variant literal keeps the type of its field (`A2<i32>`), got: {described}"
    );
}

/// Mutant: `typeck.rs:2248:34 replace && with || in check_variant_call`.
///
/// Rule: TYP-11 / TYP-13.2 — a tuple-variant call gets its generic arguments
/// from the expected type only when the expected type is *that* enum; the
/// argument `3` therefore stays `i32` and the call is `A3<i32>`, which the
/// diagnostic must name (an unrelated `B3<usize>` does not retype the call).
#[test]
fn a_variant_call_is_not_retyped_by_an_unrelated_expected_type() {
    let result = run(r#"
enum A3<T> { Va(T) }
enum B3<T> { Vb(T) }

fn f() -> B3<usize> {
    let b: B3<usize> = A3::Va(3);
    b
}
"#);
    let described = describe(&result);
    assert!(
        described.contains("E-TYP-MISMATCH") && described.contains("found `A3<i32>`"),
        "the call keeps the type of its argument (`A3<i32>`), got: {described}"
    );
}

// ===========================================================================
// `resolve.rs::resolve_type_path` — a module is not a type (MOD-3, MOD-4)
// ===========================================================================

/// Mutant: `resolve.rs:2502:13 replace || with && in resolve_type_path`
/// (`world.lookup(…, Ns::Module, …).is_some() || builtin_root_module(…).is_some()`).
///
/// Rule: G-5.3/diagnostics "E-RES-UNKNOWN — nom, module …": a bare module name
/// in type position is the *module-is-not-a-type* diagnostic, not "unknown
/// type". Both versions report `E-RES-UNKNOWN`, so the discriminating
/// observable is the message.
#[test]
fn a_builtin_module_used_as_a_type_is_named_as_a_module() {
    expect_code_msg(
        "fn f(x: math) {}\n",
        "E-RES-UNKNOWN",
        "`math` is a module, not a type",
    );
}

/// Mutant: `resolve.rs:2581:25 replace || with && in resolve_type_path`
/// (`lookup(Ns::Type, first).is_some() || is_builtin_type_name(first)`).
///
/// Rule: same MOD-3 "module vs type" diagnostic, from the other direction: a
/// user *type* used as the module prefix of a type path (`Point::Champ`) is
/// "`Point` is a type, not a module".
#[test]
fn a_type_used_as_a_module_prefix_is_named_as_a_type() {
    expect_code_msg(
        r#"
struct Point { x: i32 }

fn f(p: Point::Champ) {}
"#,
        "E-RES-UNKNOWN",
        "`Point` is a type, not a module",
    );
}

/// Mutant: `resolve.rs:2630:33 delete match arm Sym::BuiltinModule(p) in
/// resolve_type_path` (an intermediate segment that is a module imported from
/// the std prelude).
///
/// Rule: MOD-3.2 + MOD-8.3 (the `std` modules are importable names); a path
/// that reaches a builtin module must be reported as "`std::io::Foo` is not a
/// type". Without the arm the checker reports "`io` is not a module", which is
/// the wrong explanation: `io` *is* a module.
#[test]
fn an_imported_builtin_module_prefix_is_named_as_a_module() {
    expect_code_msg(
        r#"
mod m {
    use std::io;
}

fn f(x: m::io::Foo) {}
"#,
        "E-RES-UNKNOWN",
        "`std::io::Foo` is not a type",
    );
}

//
// `facts.consts[i].value` is the folded value the emitter writes; a constant
// whose value is missing from the facts cannot be emitted at all, so the
// presence *and* the value are both observable.

/// Mutant: `typeck.rs:4874:61 replace << with >> in check_const_expr`
/// (`magnitude == 1u128 << 127`). The negative i128 lower bound is exactly
/// `1 << 127`; reading it as `1 >> 127 == 0` makes the folded value wrong (or
/// panics on `-(i128::MIN)`).
///
/// Rules: TYP-13.2 (a literal takes the type imposed by the context, and
/// `E-TYP-LITERAL-RANGE` is reported only when it does not fit) and G-5.12
/// (evaluation in verified arithmetic — `-2^127` *is* representable in `i128`).
#[test]
fn const_negated_i128_min_is_accepted_with_its_value() {
    let checked = accept("const MIN: i128 = -170141183460469231731687303715884105728;\n");
    assert_eq!(
        const_value(&checked, "MIN"),
        Some(ConstValue::Int(i128::MIN)),
        "the folded value of the i128 lower bound is i128::MIN"
    );
}

/// Mutant: `typeck.rs:4883:25 delete match arm Some(ConstValue::Int(v)) in
/// check_const_expr` (the non-literal operand of unary `-`).
///
/// Rules: G-5.12 (verified arithmetic, folded by the compiler) and G-5.17:
/// `-(-1)` is a `const_expr` (`const_unary` in G-5.5) whose value is `1`.
#[test]
fn const_negation_of_a_negation_keeps_its_value() {
    let checked = accept("const A: i32 = -(-1);\n");
    assert_eq!(
        const_value(&checked, "A"),
        Some(ConstValue::Int(1)),
        "`-(-1)` folds to 1"
    );
}

/// Mutant: `typeck.rs:4890:46 replace match guard int_i128_fits(neg, &ty) with
/// true in check_const_expr` (the guard that refuses the negation of a
/// minimum value).
///
/// Rules: G-5.12 (a constant overflow is `E-TYP-CONST`, never a silently
/// wrapped value) and TYP-13.2 (`-128` fits `i8`, `+128` does not).
#[test]
fn const_negation_of_i8_min_overflows() {
    expect_code_msg(
        "const X: i8 = -(-128);\n",
        "E-TYP-CONST",
        "constant overflow",
    );
}

/// Mutant: `typeck.rs:4890:46 replace match guard int_i128_fits(neg, &ty) with
/// false in check_const_expr`. Opposite direction: a value that fits must not
/// be refused.
///
/// Rule: G-5.12 (verified arithmetic: no overflow here) and TYP-13.2.
#[test]
fn const_negation_of_a_small_value_is_accepted() {
    let checked = accept("const X: i8 = -(-5);\n");
    assert_eq!(const_value(&checked, "X"), Some(ConstValue::Int(5)));
}

/// Mutant: `typeck.rs:4916:25 delete match arm Some(ConstValue::Bool(v))` and
/// `typeck.rs:4916:76 delete !`, both in `check_const_expr`'s `UnOp::Not`.
///
/// Rule: G-5.12 (the compiler folds `!` on constants — G-5.5 admits
/// `"!" const_unary`); the emitter writes `facts.consts[B].value`, so the
/// folded value is the observable.
#[test]
fn const_boolean_not_is_folded() {
    let checked = accept("const B: bool = !true;\n");
    assert_eq!(
        const_value(&checked, "B"),
        Some(ConstValue::Bool(false)),
        "`!true` folds to false"
    );
}

/// Mutant: `typeck.rs:4917:25 delete match arm Some(ConstValue::Int(v))` and
/// `typeck.rs:4917:74 delete !`, both in `check_const_expr`'s `UnOp::Not`.
///
/// Rule: G-5.12 + G-5.5 (`"!" const_unary` on an integer is bitwise not);
/// `!0` is `-1` in two's complement, which is also what
/// `crates/ravum-lang-sema/tests/typeck_contracts2.rs::constant_folding_operators`
/// pins as accepted.
#[test]
fn const_integer_not_is_folded() {
    let checked = accept("const I: i32 = !0;\n");
    assert_eq!(
        const_value(&checked, "I"),
        Some(ConstValue::Int(-1)),
        "`!0` folds to -1"
    );
}

/// Mutant: `typeck.rs:4830:13 delete match arm ExprKind::Float{text, suffix} in
/// check_const_expr`.
///
/// A float is grammatically a `const_expr` (G-5.5, `FLOAT_LIT`) but Core 1.0
/// facts cannot represent it, which is exactly `E-RES-CONST-NOT-CONST`
/// (`diagnostics.md` §"valeur de constante non représentable dans les faits du
/// checker"). Deleting the arm falls through to the generic "this expression is
/// not allowed in a constant", i.e. the diagnostic stops naming the float.
#[test]
fn const_float_is_refused_by_name() {
    expect_code_msg(
        "const F: f64 = 1.5;\n",
        "E-RES-CONST-NOT-CONST",
        "floating-point constants are not representable",
    );
}

/// Mutant: `typeck.rs:4851:44 replace match guard t.is_int() with true in
/// check_const_expr` (the expected type of a negated integer literal).
///
/// Rule: TYP-13.2 — the literal takes the type *imposed by the context* only
/// when that type is an integer type; `-1` in `f64` position stays `i32` and is
/// `E-TYP-MISMATCH` (no implicit integer→float conversion, TYP-13.1). The
/// mutant instead negates "a value of type f64", a different diagnostic.
#[test]
fn const_negated_literal_in_float_position_stays_an_integer() {
    let result = run("const N: f64 = -1;\n");
    let described = describe(&result);
    assert!(
        described.contains("E-TYP-MISMATCH")
            && described.contains("constant value has type `i32`, declared `f64`"),
        "expected the integer constant to clash with its `f64` declaration, got: {described}"
    );
    assert!(
        !described.contains("cannot negate"),
        "`-1` is a valid integer negation; got: {described}"
    );
}

/// Mutant: `typeck.rs:4951:13 delete match arm ExprKind::Block(_) in
/// check_const_expr`.
///
/// Both versions refuse the block; the mutant loses the diagnostic that names
/// it. G-5.11 (MUST) assigns `E-TYP-CONST` to a block in a `const_expr`, and
/// the implementation now emits exactly that code (the pass-5 review found it
/// emitted `E-RES-CONST-NOT-CONST`, which `diagnostics.md` reserves for a value
/// the facts cannot represent, e.g. a float; the contract pins both the code and
/// the message so a swap cannot pass silently).
#[test]
fn const_block_is_refused_by_name() {
    let described = expect_message(
        "const Z: i32 = { 1 };\n",
        "blocks are not constant expressions",
    );
    assert!(
        described.contains("E-TYP-CONST"),
        "a block in constant position is E-TYP-CONST (G-5.11), got: {described}"
    );
    assert!(
        !described.contains("E-RES-CONST-NOT-CONST"),
        "E-RES-CONST-NOT-CONST is reserved for unrepresentable values, got: {described}"
    );
}

// ===========================================================================
// `typeck.rs::check_lit_pattern` — the type of a float pattern (G-6.36)
// ===========================================================================

/// Mutant: `typeck.rs:4316:25 delete match arm Type::F32 in check_lit_pattern`.
///
/// Rule: G-6.36 — "the literal type is the suffix, else the scrutinee type,
/// else `f64`". `facts.pat_types` is the type the emitter uses to select the
/// Rust literal suffix, so a `f32` scrutinee must give an `f32` pattern (the
/// corpus case `parser/match-float-literal-pass` fails to compile otherwise,
/// cf. `docs/qualification/MUTATION-EQUIVALENCE.md` §7.2).
#[test]
fn float_pattern_on_f32_scrutinee_is_f32() {
    let checked = accept(
        r#"
fn f(x: f32) -> bool {
    match x {
        0.5 => true,
        _ => false,
    }
}
"#,
    );
    let pattern_types: Vec<Type> = checked.facts.pat_types.values().cloned().collect();
    assert_eq!(
        pattern_types.len(),
        2,
        "the float pattern and the wildcard are the only patterns: {pattern_types:?}"
    );
    assert!(
        pattern_types.iter().all(|ty| *ty == Type::F32),
        "every pattern of a `f32` match is `f32`, got {pattern_types:?}"
    );
}

// ===========================================================================
// `typeck.rs::check_for` — the loop counter restored after the body (EXPR-6)
// ===========================================================================

/// Mutants: `typeck.rs:3475:25 replace -= with +=` and `replace -= with /=` in
/// `check_for` (`self.loop_depth -= 1` after the body).
///
/// Rule: EXPR-6.9 — `break`/`continue` outside a loop are `E-TYP-BREAK-OUTSIDE`
/// (the code is declared in `diagnostics.md`); the loop depth must be back to
/// zero once the body has been checked. `/=` leaves it at 1 and `+=` at 2.
#[test]
fn break_after_a_for_loop_is_outside_the_loop() {
    expect_code_msg(
        r#"
fn f() {
    for i in 0..3 {
        let _ = i;
    }
    break;
}
"#,
        "E-TYP-BREAK-OUTSIDE",
        "`break` outside of a loop",
    );
}

/// Same two mutants, for `continue` (EXPR-6.9, `E-TYP-BREAK-OUTSIDE`).
#[test]
fn continue_after_a_for_loop_is_outside_the_loop() {
    expect_code_msg(
        r#"
fn f() {
    for i in 0..3 {
        let _ = i;
    }
    continue;
}
"#,
        "E-TYP-BREAK-OUTSIDE",
        "`continue` outside of a loop",
    );
}

// ===========================================================================
// `resolve.rs::type_is_copy` — structural Copy (TYP-14, OWN-2)
// ===========================================================================

/// Mutant: `resolve.rs:2946:17 replace && with || in type_is_copy`
/// (`Result<ok, err>` was Copy when *either* side was Copy).
///
/// Rules: TYP-14.1 (a struct is `Copy` only when **all** its fields are) and
/// TYP-14.2 (`Result<T, E>` is mobile when `T` or `E` is mobile; `String` is
/// mobile). Observable twice: `facts.structs[S].is_copy` (the fact the emitter
/// and `own.rs` consume) and OWN-2.3/OWN-5.8 (reading a mobile place moves it,
/// so a second read is `E-OWN-USE-AFTER-MOVE`).
#[test]
fn result_with_a_mobile_error_is_not_copy() {
    let source = r#"
struct S { r: Result<i32, String> }

fn f(s: S) {
    let a = s;
    let b = s;
    let _a = a;
    let _b = b;
}
"#;
    let result = run(source);
    assert!(
        result.checked.is_none(),
        "`S` contains a mobile `String`: it must not be Copy"
    );
    assert!(
        codes(&result).iter().any(|c| c == "E-OWN-DOUBLE-MOVE"),
        "the second use of `s` moves it twice, got {:?}\n--- source ---\n{source}",
        codes(&result)
    );
}

/// The same fact, read directly: `S` holds a mobile `Result`, so
/// `facts.structs[S].is_copy` must be false (TYP-14.1/14.2). This is the
/// observable the emitter uses to decide copy vs move.
#[test]
fn struct_holding_a_mobile_result_is_marked_mobile() {
    let checked = accept(
        r#"
struct S { r: Result<i32, String> }
"#,
    );
    let info = checked
        .facts
        .structs
        .values()
        .find(|s| s.name == "S")
        .expect("S is recorded");
    assert!(
        !info.is_copy,
        "`Result<i32, String>` is mobile, so `S` is mobile (TYP-14.2)"
    );
}

// ===========================================================================
// `typeck.rs::check_struct_lit` — the resolution fact of the literal path
// ===========================================================================
//
// `facts.resolutions[path_id]` is the fact the emitter reads to know *which*
// item a literal builds. `docs/qualification/MUTATION-EQUIVALENCE.md` §7.2
// classified the three `delete match arm` mutants below as "observable only
// through the code generator"; they are in fact observable here, because the
// checker's output *is* the facts (`lib.rs`).

/// Mutant: `typeck.rs:3934:13 delete match arm ValuePath::Type(Type::Nominal
/// { def, .. })` in check_struct_lit.
///
/// Rule: the resolution fact is part of the checker's output contract; deleting
/// the arm records `Resolution::Error` for a literal that resolves, so the
/// emitter can no longer name the struct.
#[test]
fn struct_literal_records_an_item_resolution() {
    let checked = accept(
        r#"
struct Point { x: i32, y: i32 }

fn f() -> i32 {
    let p = Point { x: 1, y: 2 };
    p.x
}
"#,
    );
    assert!(
        !checked
            .facts
            .resolutions
            .values()
            .any(|r| *r == Resolution::Error),
        "the literal path resolves, so no resolution may be an error: {:?}",
        checked.facts.resolutions
    );
    assert!(
        checked
            .facts
            .resolutions
            .values()
            .any(|r| matches!(r, Resolution::Item { .. })),
        "the struct literal path must record `Resolution::Item`: {:?}",
        checked.facts.resolutions
    );
}

/// Mutant: `typeck.rs:3939:13 delete match arm ValuePath::Variant { enum_item,
/// variant }` in check_struct_lit.
///
/// Rule: same output contract, for a struct-variant literal (`Forme::Rond { r }`
/// must record `Resolution::Variant`, see G-6.29/G-6.41 on qualified variants).
#[test]
fn struct_variant_literal_records_a_variant_resolution() {
    let checked = accept(
        r#"
enum Forme { Rond { r: i32 }, Carre { c: i32 } }

fn f(x: Forme) -> i32 {
    let y = Forme::Rond { r: 1 };
    let z = y;
    let _z = z;
    match x {
        Forme::Rond { r } => r,
        Forme::Carre { c } => c,
    }
}
"#,
    );
    // The `Resolution::Variant` value alone does not separate the two
    // insertions: the *patterns* below also record variant resolutions. What
    // the deleted arm changes is the fact recorded for the literal path, which
    // becomes `Resolution::Error` — and the emitter cannot name a literal whose
    // resolution is an error.
    assert!(
        !checked
            .facts
            .resolutions
            .values()
            .any(|r| *r == Resolution::Error),
        "the variant literal path resolves, so no resolution may be an error: {:?}",
        checked.facts.resolutions
    );
    assert!(
        checked
            .facts
            .resolutions
            .values()
            .any(|r| matches!(r, Resolution::Variant { .. })),
        "the variant literal path must record `Resolution::Variant`: {:?}",
        checked.facts.resolutions
    );
}

/// Mutant: `typeck.rs:3948:13 delete match arm ValuePath::BuiltinConst(ctor_path)`
/// in check_struct_lit.
///
/// Rule: `None` is a prelude unit variant (`G-6.41`), and a literal-shaped
/// path to it must record `Resolution::BuiltinConst { path: "Option::None" }`
/// for the emitter; deleting the arm records an error resolution instead.
#[test]
fn none_literal_records_a_builtin_const_resolution() {
    let checked = accept(
        r#"
fn f() -> Option<i32> {
    let n: Option<i32> = None {};
    n
}
"#,
    );
    assert!(
        checked
            .facts
            .resolutions
            .values()
            .any(|r| matches!(r, Resolution::BuiltinConst { path } if path == "Option::None")),
        "`None {{}}` must record `Resolution::BuiltinConst`: {:?}",
        checked.facts.resolutions
    );
}
