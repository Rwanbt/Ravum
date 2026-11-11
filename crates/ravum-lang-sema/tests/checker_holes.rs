//! Contract tests for checker-rooted `check ⇒ build` divergences found by the
//! P0-F adversarial probe campaign (task-4).
//!
//! Every case is a small program plus the diagnostic code that
//! `spec/language/` mandates. Each section names the rule it pins; the
//! "positive" cases are programs the spec accepts and that must keep being
//! accepted (the conformance corpus builds them).
//!
//! Run with:
//! `cargo test -p ravum-lang-sema --test checker_holes`

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

fn run(src: &str) -> CheckResult {
    run_at(src, "src/lib.rv")
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

/// The program must be rejected with `expected` (and must not be accepted).
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

/// The program must be accepted with no error diagnostic.
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
// D. `Option`/`Result` projections consume the receiver
//     `spec/language/stdlib.md` STD-11.3 ("Tous consomment le receveur"),
//     STD-11.4 (closed method list: no `expect` in Core 1.0), and
//     `spec/language/ownership.md` OWN-5.8 / OWN-5.1 / OWN-5.2.
// ===========================================================================

#[test]
fn unwrap_then_read_is_use_after_move() {
    // OWN-5.1: reading a MOVED place is `E-OWN-USE-AFTER-MOVE`. `unwrap()`
    // takes `self` by value (STD-11.3), so `o` is moved.
    expect_code(
        r#"
fn main() {
    let o = Option::Some(String::from("x"));
    let s = o.unwrap();
    let b = o.is_some();
}
"#,
        "E-OWN-USE-AFTER-MOVE",
    );
}

#[test]
fn unwrap_then_move_again_is_double_move() {
    // OWN-5.2 (MUST): two moves with no use between them are
    // `E-OWN-DOUBLE-MOVE`, *not* `E-OWN-USE-AFTER-MOVE`. This is the exact
    // shape reported by the Lead; the code is the more specific one.
    expect_code(
        r#"
fn main() {
    let o = Option::Some(String::from("x"));
    let s = o.unwrap();
    let t = o;
}
"#,
        "E-OWN-DOUBLE-MOVE",
    );
}

#[test]
fn unwrap_or_then_read_is_use_after_move() {
    // STD-11.3: `unwrap_or(self, default)` also consumes the receiver.
    expect_code(
        r#"
fn main() {
    let o = Option::Some(String::from("x"));
    let s = o.unwrap_or(String::from("y"));
    let b = o.is_some();
}
"#,
        "E-OWN-USE-AFTER-MOVE",
    );
}

#[test]
fn result_unwrap_then_read_is_use_after_move() {
    // STD-11.3: `Result::unwrap(self)` and `Result::ok(self)` consume `r`;
    // `Result<i32, Error>` is mobile (TYP-14.2, `Error` contains a `String`).
    expect_code(
        r#"
fn main() {
    let r: Result<i32, Error> = Result::Ok(1);
    let v = r.unwrap();
    let b = r.is_ok();
}
"#,
        "E-OWN-USE-AFTER-MOVE",
    );
}

#[test]
fn result_ok_then_err_is_use_after_move() {
    // STD-11.3: both projections consume; the second use sees a moved value.
    expect_code(
        r#"
fn main() {
    let r: Result<i32, Error> = Result::Ok(1);
    let o = r.ok();
    let e = r.err();
}
"#,
        "E-OWN-USE-AFTER-MOVE",
    );
}

#[test]
fn unwrap_family_on_copy_option_is_accepted() {
    // OWN-2.3 / TYP-10.3: `Option<i32>` is `Copy`; consuming projections copy
    // the receiver, which stays usable. This is the positive half of the fix.
    accept(
        r#"
fn f(o: Option<i32>) -> i32 {
    let a = o.unwrap();
    let b = o.unwrap_or(0);
    let c = o.is_some();
    return a + b + c as i32;
}
"#,
    );
}

#[test]
fn result_projection_on_distinct_values_is_accepted() {
    accept(
        r#"
fn f(r: Result<i32, Error>, s: Result<i32, Error>) -> i32 {
    let a = r.is_ok();
    let b = r.is_err();
    let o = r.ok();
    let e = s.err();
    return 0;
}
"#,
    );
}

// ===========================================================================
// E. Annotated array literals verify every element
//     `spec/language/types.md` TYP-5.1 (homogeneous elements) and TYP-5.7
//     (literal length / repeat rules); `expressions.md` EXPR-5.11.
// ===========================================================================

#[test]
fn annotated_array_rejects_heterogeneous_element() {
    // TYP-5.1: all elements have the same type. The generated Rust failed with
    // E0308; the checker must report `E-TYP-MISMATCH` on the offending element.
    expect_code(
        r#"
fn main() {
    let a: [i32; 2] = [1, true];
}
"#,
        "E-TYP-MISMATCH",
    );
}

#[test]
fn annotated_array_rejects_text_element_against_integer() {
    expect_code(
        r#"
fn main() {
    let a: [i32; 3] = [1, 2, "x"];
}
"#,
        "E-TYP-MISMATCH",
    );
}

#[test]
fn annotated_array_accepts_homogeneous_elements() {
    accept(
        r#"
fn f() -> i32 {
    let a: [i32; 3] = [1, 2, 3];
    let b: [u8; 3] = [0; 3];
    return a[0] + a[2] + b[1] as i32;
}
"#,
    );
}

#[test]
fn annotated_array_element_coerced_text_is_rejected() {
    // TYP-13.1: the only implicit coercions are in argument/receiver position;
    // an array element is not one of them.
    expect_code(
        r#"
fn main() {
    let s = String::from("x");
    let a: [&str; 1] = [s];
}
"#,
        "E-TYP-MISMATCH",
    );
}

// ===========================================================================
// F. Nested match exhaustiveness
//     `spec/language/grammar.md` G-6.22 and `types.md` TYP-10.4 (MUST).
// ===========================================================================

#[test]
fn nested_option_payload_gap_is_nonexhaustive() {
    // `Some(None)` is not covered; rustc rejected the generated match (E0004).
    expect_code(
        r#"
fn f(x: Option<Option<i32>>) -> i32 {
    match x {
        Option::Some(Option::Some(n)) => n,
        Option::None => 0,
    }
}
"#,
        "E-TYP-NONEXHAUSTIVE",
    );
}

#[test]
fn tuple_inside_option_payload_gap_is_nonexhaustive() {
    // The second tuple component `false` is not covered.
    expect_code(
        r#"
fn f(x: Option<(i32, bool)>) -> i32 {
    match x {
        Option::Some((n, true)) => n,
        Option::None => 0,
    }
}
"#,
        "E-TYP-NONEXHAUSTIVE",
    );
}

#[test]
fn nested_option_inside_enum_payload_gap_is_nonexhaustive() {
    expect_code(
        r#"
enum E { A(Option<i32>), B }
fn f(e: E) -> i32 {
    match e {
        E::A(Option::Some(n)) => n,
        E::B => 0,
    }
}
"#,
        "E-TYP-NONEXHAUSTIVE",
    );
}

#[test]
fn nested_result_payload_gap_is_nonexhaustive() {
    expect_code(
        r#"
fn f(r: Result<Option<i32>, Error>) -> i32 {
    match r {
        Result::Ok(Option::Some(n)) => n,
        Result::Err(e) => 0,
    }
}
"#,
        "E-TYP-NONEXHAUSTIVE",
    );
}

#[test]
fn complete_nested_option_match_is_accepted() {
    accept(
        r#"
fn f(x: Option<Option<i32>>) -> i32 {
    match x {
        Option::Some(Option::Some(n)) => n,
        Option::Some(Option::None) => 0,
        Option::None => 0,
    }
}
"#,
    );
}

#[test]
fn complete_tuple_of_bools_match_is_accepted() {
    // G-6.22: a complete enumeration of a `(bool, bool)` scrutinee is
    // exhaustive. The previous engine rejected it (`E-TYP-NONEXHAUSTIVE`) while
    // rustc accepted the generated match.
    accept(
        r#"
fn f(t: (bool, bool)) -> i32 {
    match t {
        (true, true) => 1,
        (true, false) => 2,
        (false, true) => 3,
        (false, false) => 4,
    }
}
"#,
    );
}

#[test]
fn complete_nested_enum_match_is_accepted() {
    accept(
        r#"
enum Inner { A(i32), B }
enum Outer { X(Inner), Y }
fn f(o: Outer) -> i32 {
    match o {
        Outer::X(Inner::A(n)) => n,
        Outer::X(Inner::B) => 0,
        Outer::Y => 1,
    }
}
"#,
    );
}

#[test]
fn wildcard_covers_nested_option_payload() {
    accept(
        r#"
fn f(x: Option<Option<i32>>) -> i32 {
    match x {
        Option::Some(_) => 1,
        Option::None => 0,
    }
}
"#,
    );
}

#[test]
fn complete_nested_tuple_match_is_accepted() {
    // A tuple nested inside a tuple is a product column: all 8 combinations
    // must be recognised as exhaustive.
    accept(
        r#"
fn f(t: ((bool, bool), bool)) -> i32 {
    match t {
        ((true, true), true) => 1,
        ((true, true), false) => 2,
        ((true, false), true) => 3,
        ((true, false), false) => 4,
        ((false, true), true) => 5,
        ((false, true), false) => 6,
        ((false, false), true) => 7,
        ((false, false), false) => 8,
    }
}
"#,
    );
}

#[test]
fn nested_tuple_gap_is_nonexhaustive() {
    expect_code(
        r#"
fn f(t: ((bool, bool), bool)) -> i32 {
    match t {
        ((true, true), true) => 1,
        ((true, true), false) => 2,
        ((true, false), true) => 3,
        ((true, false), false) => 4,
        ((false, true), true) => 5,
        ((false, true), false) => 6,
        ((false, false), true) => 7,
    }
}
"#,
        "E-TYP-NONEXHAUSTIVE",
    );
}

#[test]
fn integer_match_still_requires_a_wildcard() {
    // G-6.22: integers are not enumerable; `_` is mandatory.
    expect_code(
        r#"
fn f(n: i32) -> i32 {
    match n {
        1 => 1,
        2 => 2,
    }
}
"#,
        "E-TYP-NONEXHAUSTIVE",
    );
}

// ===========================================================================
// A. References stored in a field or an enum payload
//     `spec/language/ownership.md` OWN-6.1/6.2/6.6 and the negative example
//     of `types.md` §6 (`struct W { r: &i32 }` -> E-OWN-REF-ESCAPE).
// ===========================================================================

#[test]
fn reference_struct_field_is_rejected() {
    expect_code(
        r#"
struct W { r: &i32 }
"#,
        "E-OWN-REF-ESCAPE",
    );
}

#[test]
fn reference_field_in_generic_struct_is_rejected() {
    expect_code(
        r#"
struct S<T> { r: &T }
"#,
        "E-OWN-REF-ESCAPE",
    );
}

#[test]
fn reference_enum_payload_is_rejected() {
    expect_code(
        r#"
enum E { V(&i32) }
"#,
        "E-OWN-REF-ESCAPE",
    );
}

#[test]
fn reference_inside_a_stored_collection_is_rejected() {
    expect_code(
        r#"
struct S { v: Vec<&i32> }
"#,
        "E-OWN-REF-ESCAPE",
    );
}

#[test]
fn reference_inside_a_stored_tuple_is_rejected() {
    expect_code(
        r#"
struct S { t: (&i32, i32) }
"#,
        "E-OWN-REF-ESCAPE",
    );
}

#[test]
fn static_text_field_is_the_documented_exception() {
    // OWN-6.2 / TYP-6.3: `&'static str` is the only storable reference; a
    // `&str` field is legal when every value comes from a literal.
    accept(
        r#"
struct S { s: &str }
enum E { V(&str), W }
fn f() -> S { return S { s: "x" }; }
fn g() -> E { return E::V("y"); }
"#,
    );
}

// ===========================================================================
// B. A prelude constructor stores its argument
//     `ownership.md` OWN-6.1/3.3/6.3: a reference cannot be placed in an
//     `Option`/`Result`; OWN-6.2 keeps `&'static str` legal.
// ===========================================================================

#[test]
fn some_of_a_reference_parameter_is_rejected() {
    expect_code(
        r#"
fn f(n: &i32) {
    let o = Option::Some(n);
}
"#,
        "E-OWN-REF-ESCAPE",
    );
}

#[test]
fn some_of_as_str_is_rejected() {
    // OWN-6.3: `as_str()` is not static and cannot be stored.
    expect_code(
        r#"
fn f(s: String) {
    let o = Option::Some(s.as_str());
}
"#,
        "E-OWN-REF-ESCAPE",
    );
}

#[test]
fn returning_a_reference_inside_option_is_rejected() {
    expect_code(
        r#"
fn f(n: &i32) -> Option<&i32> {
    return Option::Some(n);
}
"#,
        "E-OWN-REF-ESCAPE",
    );
}

#[test]
fn ok_of_a_reference_parameter_is_rejected() {
    expect_code(
        r#"
fn f(n: &i32) -> Result<&i32, Error> {
    return Result::Ok(n);
}
"#,
        "E-OWN-REF-ESCAPE",
    );
}

#[test]
fn static_text_inside_option_is_accepted() {
    // OWN-6.2: a `&'static str` literal may be stored.
    accept(
        r#"
fn f() -> usize {
    let o: Option<&str> = Option::Some("x");
    let b = o.is_some();
    return 0;
}
"#,
    );
}

#[test]
fn owned_value_inside_option_is_accepted() {
    accept(
        r#"
fn f() -> usize {
    let o: Option<String> = Option::Some(String::from("x"));
    let b = o.is_some();
    return 0;
}
"#,
    );
}

// ===========================================================================
// C. A `&mut self` user method requires a mutable receiver place
//     `types.md` TYP-3.2 and `ownership.md` OWN-7.4 (MUST: `E-OWN-IMMUTABLE`).
// ===========================================================================

#[test]
fn mut_self_method_on_immutable_binding_is_rejected() {
    expect_code(
        r#"
struct P { x: i32 }
impl P { fn set(&mut self) { self.x = 1; } }
fn f(p: P) { p.set(); }
"#,
        "E-OWN-IMMUTABLE",
    );
}

#[test]
fn mut_self_method_on_temporary_is_rejected() {
    // TYP-3.2: `Point::new(..).set(..)` on a temporary is `E-OWN-IMMUTABLE`.
    expect_code(
        r#"
struct P { x: i32 }
impl P { fn set(&mut self) { self.x = 1; } }
fn f() { P { x: 0 }.set(); }
"#,
        "E-OWN-IMMUTABLE",
    );
}

#[test]
fn mut_self_method_on_mutable_binding_is_accepted() {
    accept(
        r#"
struct P { x: i32 }
impl P { fn set(&mut self) { self.x = 1; } }
fn f(mut p: P) { p.set(); }
"#,
    );
}

#[test]
fn mut_self_method_through_mut_reference_is_accepted() {
    // OWN-3.14: a `&mut T` parameter is usable for writes and reborrows.
    accept(
        r#"
struct P { x: i32 }
impl P { fn set(&mut self) { self.x = 1; } }
fn f(p: &mut P) { p.set(); }
"#,
    );
}

#[test]
fn shared_self_method_on_temporary_is_accepted() {
    // TYP-3.3 / G-6.30: a `&self` method accepts any receiver, including a
    // temporary; this positive case guards against over-restricting.
    accept(
        r#"
struct P { x: i32 }
impl P { fn get(&self) -> i32 { return self.x; } }
fn f() -> i32 { return P { x: 0 }.get(); }
"#,
    );
}
