//! Adversarial, independent audit of the Ravum borrow checker (`own.rs`).
//!
//! This file is **not** the implementation's own test suite: it was written by
//! a reviewer who did not write `own.rs`, and it tries to *break* the checker.
//! Each test states the rule it probes (`spec/language/ownership.md`) and the
//! ownership diagnostic that a conservative, sound checker must produce (or
//! must not produce, for legitimate programs).
//!
//! Helpers mirror the ones in `own.rs`'s private `#[cfg(test)] mod tests`
//! (`run`, `own_codes`, `accepted`, `rejected`) because an integration test
//! cannot reach private test helpers. The real parser/checker are used: a parse
//! or type error is reported as-is instead of being masked.
//!
//! Run with:
//! `cargo test -p ravum-lang-sema --test own_audit -- --test-threads=1`

use ravum_lang_sema::syntax::{parser, source::SourceMap, LANGUAGE_EDITION};
use ravum_lang_sema::{check, CheckOptions, CheckResult, Profile};

/// Parse and check a one-file library crate. Parse diagnostics are a hard
/// failure: an audit case must be syntactically valid.
fn run(src: &str) -> CheckResult {
    let mut sources = SourceMap::new();
    let file = sources.add("src/lib.rv", src);
    let parsed = parser::parse_file(file, src);
    let parse_codes: Vec<String> = parsed.diags.iter().map(|d| d.code.clone()).collect();
    assert!(
        parsed.diags.is_empty(),
        "audit case is not syntactically valid: {parse_codes:?}"
    );
    let options = CheckOptions {
        profile: Profile::Core,
        edition: LANGUAGE_EDITION.to_string(),
    };
    check(&parsed.program, &sources, &options)
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

fn own_codes(result: &CheckResult) -> Vec<String> {
    result
        .diags
        .iter()
        .filter(|d| d.code.starts_with("E-OWN-"))
        .map(|d| d.code.clone())
        .collect()
}

/// A legitimate program must be accepted, with zero ownership diagnostic.
/// A rejection caused by an *earlier* phase is reported separately: it is not a
/// borrow-checker false positive but it blocks the audit case.
fn accepted(src: &str) {
    let result = run(src);
    let codes = own_codes(&result);
    assert!(
        codes.is_empty(),
        "FALSE POSITIVE (borrow checker rejected a legitimate program): {codes:?}\n{}",
        describe(&result)
    );
    assert!(
        result.checked.is_some(),
        "audit case blocked by a non-ownership diagnostic: {}",
        describe(&result)
    );
}

/// An unsound program must be rejected with the exact expected ownership code.
fn rejected(src: &str, expected: &str) {
    let result = run(src);
    let codes = own_codes(&result);
    assert!(
        result.checked.is_none(),
        "UNSOUND ACCEPT: expected {expected}, but the program was accepted"
    );
    assert!(
        codes.iter().any(|code| code == expected),
        "WRONG CODE: expected {expected}, got {codes:?}\n{}",
        describe(&result)
    );
}

// ===========================================================================
// Initialisation / define-before-use
// ===========================================================================

#[test]
fn audit_uninit_read_on_all_paths() {
    // OWN-4.3: no path initialises `x`.
    rejected(
        r#"
fn f(c: bool) -> i32 {
    let x: i32;
    if c { x = 1; }
    return x;
}
"#,
        "E-OWN-UNINIT",
    );
}

#[test]
fn audit_uninit_after_zero_iteration_loop() {
    // OWN-4.3 + loop semantics: `while` may execute zero times.
    rejected(
        r#"
fn f(c: bool) -> i32 {
    let x: i32;
    while c {
        x = 1;
    }
    return x;
}
"#,
        "E-OWN-UNINIT",
    );
}

#[test]
fn audit_double_deferred_init_is_allowed() {
    // Assigning an already initialised local replaces it (OWN-4.4).
    accepted(
        r#"
fn f() -> i32 {
    let mut x: i32;
    x = 1;
    x = 2;
    return x;
}
"#,
    );
}

#[test]
fn audit_init_in_all_match_arms_is_allowed() {
    // Every arm initialises `x`; the join must see INIT on all paths.
    accepted(
        r#"
fn f(c: bool) -> i32 {
    let mut x: i32;
    match c {
        true => x = 1,
        false => x = 2,
    }
    return x;
}
"#,
    );
}

#[test]
fn audit_init_in_one_match_arm_is_rejected() {
    rejected(
        r#"
fn f(c: bool) -> i32 {
    let mut x: i32;
    match c {
        true => x = 1,
        false => {}
    }
    return x;
}
"#,
        "E-OWN-UNINIT",
    );
}

// ===========================================================================
// Moves
// ===========================================================================

#[test]
fn audit_use_after_move_anchor() {
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
fn audit_double_move_anchor() {
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
fn audit_move_in_call_then_use_anchor() {
    rejected(
        r#"
fn take(s: String) {}
fn f(s: String) {
    take(s);
    take(s);
}
"#,
        "E-OWN-USE-AFTER-MOVE",
    );
}

#[test]
fn audit_move_then_reassign_on_all_paths_is_allowed() {
    // Both branches re-initialise the moved place: the join is INIT.
    accepted(
        r#"
fn f(c: bool) -> usize {
    let mut s = String::from("a");
    let t = s;
    if c {
        s = String::from("b");
    } else {
        s = String::from("c");
    }
    return s.len();
}
"#,
    );
}

#[test]
fn audit_move_then_reassign_on_one_path_is_rejected() {
    // Conservative join: `s` may still be moved after the `if`.
    rejected(
        r#"
fn f(c: bool) -> usize {
    let mut s = String::from("a");
    let t = s;
    if c {
        s = String::from("b");
    }
    return s.len();
}
"#,
        "E-OWN-USE-AFTER-MOVE",
    );
}

#[test]
fn audit_shadowing_after_move_is_allowed() {
    // A new binding is a new place; the old moved place is unreachable by name.
    accepted(
        r#"
fn f() -> usize {
    let s = String::from("a");
    let t = s;
    let s = String::from("b");
    return s.len();
}
"#,
    );
}

#[test]
fn audit_move_out_of_shared_reference_is_rejected() {
    rejected(
        r#"
fn f(s: &String) -> usize {
    let t = *s;
    return t.len();
}
"#,
        "E-OWN-MOVE-BORROWED",
    );
}

#[test]
fn audit_move_out_of_mut_reference_field_is_rejected() {
    rejected(
        r#"
struct D { titre: String }
fn f(d: &mut D) -> String {
    return d.titre;
}
"#,
        "E-OWN-MOVE-BORROWED",
    );
}

#[test]
fn audit_move_out_of_shared_reference_field_is_rejected() {
    rejected(
        r#"
struct D { titre: String }
fn f(d: &D) -> String {
    return d.titre;
}
"#,
        "E-OWN-MOVE-BORROWED",
    );
}

#[test]
fn audit_move_of_vec_element_is_rejected() {
    rejected(
        r#"
fn f(v: Vec<String>) -> String {
    return v[0];
}
"#,
        "E-OWN-MOVE-INDEXED",
    );
}

// ===========================================================================
// Partial moves and projections
// ===========================================================================

#[test]
fn audit_move_field_then_whole_struct_is_rejected() {
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
fn audit_move_field_twice_is_rejected() {
    rejected(
        r#"
struct D { titre: String, n: i32 }
fn f(d: D) -> String {
    let t = d.titre;
    let u = d.titre;
    return u;
}
"#,
        "E-OWN-DOUBLE-MOVE",
    );
}

#[test]
fn audit_read_moved_field_is_rejected() {
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
fn audit_borrow_moved_field_is_rejected() {
    rejected(
        r#"
struct D { titre: String }
fn h(x: &mut String) {}
fn f(mut d: D) {
    let t = d.titre;
    h(&mut d.titre);
}
"#,
        "E-OWN-USE-AFTER-MOVE",
    );
}

#[test]
fn audit_reassign_moved_field_restores_whole_struct() {
    // Writing the exact moved projection re-initialises it (OWN-4.4/5.4).
    accepted(
        r#"
struct D { titre: String, n: i32 }
fn f() -> D {
    let mut d = D { titre: String::from("a"), n: 0 };
    let t = d.titre;
    d.titre = String::from("x");
    return d;
}
"#,
    );
}

#[test]
fn audit_move_nested_field_then_use_sibling() {
    accepted(
        r#"
struct Inner { x: String, y: String }
struct Outer { inner: Inner }
fn f(o: Outer) -> usize {
    let x = o.inner.x;
    return o.inner.y.len() + x.len();
}
"#,
    );
}

#[test]
fn audit_move_parent_projection_then_child_is_rejected() {
    rejected(
        r#"
struct Inner { x: String }
struct Outer { inner: Inner }
fn f(o: Outer) -> usize {
    let i = o.inner;
    return o.inner.x.len();
}
"#,
        "E-OWN-USE-AFTER-MOVE",
    );
}

#[test]
fn audit_move_tuple_field_then_other_field() {
    accepted(
        r#"
fn f(t: (String, i32)) -> usize {
    let s = t.0;
    return t.1 as usize + s.len();
}
"#,
    );
}

// ===========================================================================
// Borrows: exclusivity, overlaps, receivers
// ===========================================================================

#[test]
fn audit_two_exclusive_args_conflict() {
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
fn audit_shared_then_exclusive_conflicts() {
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
fn audit_exclusive_then_move_conflicts() {
    rejected(
        r#"
fn h(a: &mut String, b: String) {}
fn f() {
    let mut s = String::from("a");
    h(&mut s, s);
}
"#,
        "E-OWN-BORROW-CONFLICT",
    );
}

#[test]
fn audit_disjoint_struct_fields_are_allowed() {
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
fn audit_nested_disjoint_fields_are_allowed() {
    accepted(
        r#"
struct Inner { x: i32, y: i32 }
struct Outer { inner: Inner }
fn h(a: &mut i32, b: &mut i32) {}
fn f() {
    let mut o = Outer { inner: Inner { x: 1, y: 2 } };
    h(&mut o.inner.x, &mut o.inner.y);
}
"#,
    );
}

#[test]
fn audit_parent_and_child_mut_borrows_conflict() {
    rejected(
        r#"
struct Inner { x: i32, y: i32 }
struct Outer { inner: Inner }
fn h(a: &mut Inner, b: &mut i32) {}
fn f() {
    let mut o = Outer { inner: Inner { x: 1, y: 2 } };
    h(&mut o.inner, &mut o.inner.x);
}
"#,
        "E-OWN-BORROW-CONFLICT",
    );
}

#[test]
fn audit_literal_array_indices_are_disjoint() {
    accepted(
        r#"
fn h(a: &mut i32, b: &mut i32) {}
fn f() {
    let mut a = [1, 2, 3];
    h(&mut a[0], &mut a[1]);
}
"#,
    );
}

#[test]
fn audit_opaque_array_indices_are_conservatively_conflicting() {
    // Q-OWN-5: an index computed from a non-literal expression cannot be
    // proven distinct from another index.
    rejected(
        r#"
fn h(a: &mut i32, b: &mut i32) {}
fn f(mut a: [i32; 3], i: usize, j: usize) {
    h(&mut a[i], &mut a[j]);
}
"#,
        "E-OWN-BORROW-CONFLICT",
    );
}

#[test]
fn audit_receiver_loan_conflicts_with_argument_read() {
    // OWN-3.6: Core 1.0 has no two-phase borrows, so `v.push(v.len())` must be
    // rejected (Rust accepts it thanks to two-phase borrows; Ravum does not).
    rejected(
        r#"
fn f() {
    let mut v: Vec<usize> = Vec::new();
    v.push(1);
    v.push(v.len());
}
"#,
        "E-OWN-BORROW-CONFLICT",
    );
}

#[test]
fn audit_shared_receiver_and_shared_argument_are_allowed() {
    accepted(
        r#"
fn f(v: Vec<i32>) -> usize {
    return v.len() + v.len();
}
"#,
    );
}

#[test]
fn audit_sequential_mutating_methods_are_allowed() {
    accepted(
        r#"
fn f() {
    let mut v: Vec<i32> = Vec::new();
    v.push(1);
    v.push(2);
    let x = v.pop();
    v.push(3);
}
"#,
    );
}

// ===========================================================================
// Non-escaping references / static text
// ===========================================================================

#[test]
fn audit_store_reference_in_let_is_rejected() {
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
fn audit_store_as_str_is_rejected() {
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
fn audit_return_reference_parameter_is_rejected() {
    rejected(
        r#"
fn f(s: &str) -> &str {
    return s;
}
"#,
        "E-OWN-REF-ESCAPE",
    );
}

#[test]
fn audit_reference_in_struct_literal_is_rejected() {
    rejected(
        r#"
struct S { text: &str }
fn f(s: String) {
    let value = S { text: s.as_str() };
}
"#,
        "E-OWN-REF-ESCAPE",
    );
}

#[test]
fn audit_reference_in_array_is_rejected() {
    rejected(
        r#"
fn f() {
    let x = 1;
    let refs = [&x, &x];
}
"#,
        "E-OWN-REF-ESCAPE",
    );
}

#[test]
fn audit_static_text_literal_local_is_returnable() {
    accepted(
        r#"
fn f() -> &str {
    let text = "ravum";
    return text;
}
"#,
    );
}

#[test]
fn audit_static_text_through_if_is_storable() {
    // Both branches are `&'static str`, so the binding is static and legal.
    // This is a likely false-positive probe: provenance is not syntactic
    // through `if`.
    accepted(
        r#"
fn f(c: bool) -> &str {
    let text: &str = if c { "a" } else { "b" };
    return text;
}
"#,
    );
}

#[test]
fn audit_static_text_through_match_is_storable() {
    // Same probe through `match`.
    accepted(
        r#"
fn f(c: bool) -> &str {
    let text: &str = match c {
        true => "a",
        false => "b",
    };
    return text;
}
"#,
    );
}

#[test]
fn audit_static_text_from_function_is_storable() {
    // A `&str`-returning function can only return a static reference in
    // Core 1.0, so storing its result is legitimate.
    accepted(
        r#"
fn lit() -> &str {
    return "a";
}
fn f() -> &str {
    let text = lit();
    return text;
}
"#,
    );
}

#[test]
fn audit_static_text_through_block_is_storable() {
    accepted(
        r#"
fn f() -> &str {
    let text = { "a" };
    return text;
}
"#,
    );
}

// ===========================================================================
// Coercions at call boundaries
// ===========================================================================

#[test]
fn audit_string_to_str_argument_is_allowed() {
    accepted(
        r#"
fn g(s: &str) {}
fn f(s: String) {
    g(&s);
}
"#,
    );
}

#[test]
fn audit_array_to_slice_argument_is_allowed() {
    accepted(
        r#"
fn g(a: &[i32]) {}
fn f(a: [i32; 3]) {
    g(&a);
}
"#,
    );
}

#[test]
fn audit_mut_reborrow_parameter_is_allowed() {
    accepted(
        r#"
fn h(v: &mut Vec<i32>) {}
fn f(v: &mut Vec<i32>) {
    h(&mut *v);
}
"#,
    );
}

// ===========================================================================
// Control flow: divergence, match, short-circuit, `?`, loops
// ===========================================================================

#[test]
fn audit_early_return_after_move_on_other_path_is_allowed() {
    // The only path reaching the use did not move `s`.
    accepted(
        r#"
fn f(c: bool, s: String) -> usize {
    if c {
        drop(s);
        return 0;
    }
    return s.len();
}
"#,
    );
}

#[test]
fn audit_move_in_if_without_else_then_use_is_rejected() {
    rejected(
        r#"
fn f(c: bool, s: String) -> usize {
    if c {
        drop(s);
    }
    return s.len();
}
"#,
        "E-OWN-USE-AFTER-MOVE",
    );
}

#[test]
fn audit_move_in_match_arm_then_use_after_is_rejected() {
    rejected(
        r#"
fn f(o: Option<String>, s: String) -> usize {
    match o {
        Some(v) => drop(s),
        None => {}
    }
    return s.len();
}
"#,
        "E-OWN-USE-AFTER-MOVE",
    );
}

#[test]
fn audit_match_moves_scrutinee_then_use_is_rejected() {
    // The matched value is consumed by the scrutinee (OWN-5.9).
    rejected(
        r#"
fn f(o: Option<String>) -> usize {
    match o {
        Some(v) => drop(v),
        None => {}
    }
    return o.is_some() as usize;
}
"#,
        "E-OWN-USE-AFTER-MOVE",
    );
}

#[test]
fn audit_short_circuit_move_then_use_is_rejected() {
    // The right operand may not be evaluated; conservative join is "moved".
    rejected(
        r#"
fn f(c: bool, s: String) -> usize {
    let b = c && { drop(s); true };
    return s.len();
}
"#,
        "E-OWN-USE-AFTER-MOVE",
    );
}

#[test]
fn audit_try_moves_result_twice_is_rejected() {
    // No use occurs between the two moves of `r`: OWN-5.2 classifies this as a
    // double move, not a use-after-move.
    rejected(
        r#"
fn f(r: Result<String, Error>) -> Result<usize, Error> {
    let a = r?;
    let b = r?;
    return Ok(a.len() + b.len());
}
"#,
        "E-OWN-DOUBLE-MOVE",
    );
}

#[test]
fn audit_loop_move_then_break_then_use_is_rejected() {
    rejected(
        r#"
fn f(s: String) -> usize {
    loop {
        drop(s);
        break;
    }
    return s.len();
}
"#,
        "E-OWN-USE-AFTER-MOVE",
    );
}

#[test]
fn audit_loop_break_before_move_then_use_is_rejected() {
    // On a later iteration `s` is already moved when `break` executes, so the
    // conservative rejection is correct.
    rejected(
        r#"
fn f(c: bool, s: String) -> usize {
    loop {
        if c {
            break;
        }
        drop(s);
    }
    return s.len();
}
"#,
        "E-OWN-USE-AFTER-MOVE",
    );
}

// ===========================================================================
// `self` receivers
// ===========================================================================

#[test]
fn audit_by_value_self_can_move_a_field() {
    accepted(
        r#"
struct D { titre: String, n: i32 }
impl D {
    fn into_titre(self) -> String {
        return self.titre;
    }
}
"#,
    );
}

#[test]
fn audit_shared_self_cannot_move_a_field() {
    rejected(
        r#"
struct D { titre: String, n: i32 }
impl D {
    fn bad(&self) -> String {
        return self.titre;
    }
}
"#,
        "E-OWN-MOVE-BORROWED",
    );
}

#[test]
fn audit_mut_self_cannot_move_a_field() {
    rejected(
        r#"
struct D { titre: String, n: i32 }
impl D {
    fn bad(&mut self) -> String {
        return self.titre;
    }
}
"#,
        "E-OWN-MOVE-BORROWED",
    );
}

// ===========================================================================
// Drop
// ===========================================================================

#[test]
fn audit_drop_then_use_is_rejected() {
    rejected(
        r#"
fn f(s: String) -> usize {
    drop(s);
    return s.len();
}
"#,
        "E-OWN-USE-AFTER-MOVE",
    );
}

#[test]
fn audit_drop_copy_is_a_noop() {
    accepted(
        r#"
fn f() -> i32 {
    let n = 1;
    drop(n);
    return n + 1;
}
"#,
    );
}

#[test]
fn audit_drop_one_field_keeps_sibling_usable() {
    accepted(
        r#"
struct D { titre: String, n: i32 }
fn f(d: D) -> i32 {
    drop(d.titre);
    return d.n;
}
"#,
    );
}

// ===========================================================================
// Additional projection / alias probes
// ===========================================================================

#[test]
fn audit_whole_mut_borrow_conflicts_with_field_read() {
    rejected(
        r#"
struct P { x: i32 }
fn h(a: &mut P, b: i32) {}
fn f() {
    let mut p = P { x: 1 };
    h(&mut p, p.x);
}
"#,
        "E-OWN-BORROW-CONFLICT",
    );
}

#[test]
fn audit_field_shared_borrow_conflicts_with_whole_mut() {
    rejected(
        r#"
struct P { x: i32 }
fn h(a: &i32, b: &mut P) {}
fn f() {
    let mut p = P { x: 1 };
    h(&p.x, &mut p);
}
"#,
        "E-OWN-BORROW-CONFLICT",
    );
}

#[test]
fn audit_two_reborrows_of_same_mut_parameter_conflict() {
    rejected(
        r#"
fn h(a: &mut i32, b: &mut i32) {}
fn f(x: &mut i32) {
    h(&mut *x, &mut *x);
}
"#,
        "E-OWN-BORROW-CONFLICT",
    );
}

#[test]
fn audit_move_field_during_its_own_borrow_conflicts() {
    rejected(
        r#"
struct D { titre: String }
fn h(a: &mut String, b: String) {}
fn f(d: D) {
    h(&mut d.titre, d.titre);
}
"#,
        "E-OWN-BORROW-CONFLICT",
    );
}

#[test]
fn audit_by_value_receiver_move_is_tracked() {
    rejected(
        r#"
struct D { titre: String }
impl D {
    fn into_titre(self) -> String {
        return self.titre;
    }
}
fn f(d: D) -> usize {
    let a = d.into_titre();
    let b = d.into_titre();
    return a.len() + b.len();
}
"#,
        "E-OWN-USE-AFTER-MOVE",
    );
}

#[test]
fn audit_whole_mut_borrow_after_partial_move_is_rejected() {
    rejected(
        r#"
struct D { titre: String, n: i32 }
fn h(x: &mut D) {}
fn f(d: D) {
    let t = d.titre;
    h(&mut d);
}
"#,
        "E-OWN-PARTIAL-MOVE",
    );
}

#[test]
fn audit_write_ancestor_restores_nested_field() {
    accepted(
        r#"
struct Inner { x: String }
struct Outer { inner: Inner }
fn f() -> usize {
    let mut o = Outer { inner: Inner { x: String::from("a") } };
    let s = o.inner.x;
    o.inner = Inner { x: String::from("b") };
    return o.inner.x.len();
}
"#,
    );
}

#[test]
fn audit_static_text_in_struct_field_is_storable() {
    accepted(
        r#"
struct S { text: &str }
fn f() -> S {
    let s = S { text: "a" };
    return s;
}
"#,
    );
}

#[test]
fn audit_static_text_in_tuple_is_storable() {
    accepted(
        r#"
fn f() -> (&str, &str) {
    let t = ("a", "b");
    return t;
}
"#,
    );
}

// ===========================================================================
// Scope, pattern and condition probes (second pass)
// ===========================================================================

#[test]
fn audit_move_in_inner_block_persists_after_block() {
    // Moving an outer place inside an inner block leaves it moved afterwards.
    rejected(
        r#"
fn f() -> usize {
    let s = String::from("a");
    {
        let t = s;
    }
    return s.len();
}
"#,
        "E-OWN-USE-AFTER-MOVE",
    );
}

#[test]
fn audit_inner_block_shadow_does_not_move_outer() {
    accepted(
        r#"
fn f() -> usize {
    let s = String::from("a");
    {
        let s = String::from("b");
    }
    return s.len();
}
"#,
    );
}

#[test]
fn audit_match_tuple_bindings_are_independent() {
    accepted(
        r#"
fn f(t: (String, String)) -> usize {
    match t {
        (a, b) => {
            let c = a;
            return b.len() + c.len();
        }
        _ => 0usize,
    }
}
"#,
    );
}

#[test]
fn audit_match_binding_moved_then_used_is_rejected() {
    rejected(
        r#"
fn f(t: (String, String)) -> usize {
    match t {
        (a, b) => {
            let c = a;
            return a.len() + b.len() + c.len();
        }
        _ => 0usize,
    }
}
"#,
        "E-OWN-USE-AFTER-MOVE",
    );
}

#[test]
fn audit_move_in_while_condition_then_use_after_is_rejected() {
    // The condition always runs at least once and moves `s` on the exit path.
    rejected(
        r#"
fn f(s: String, c: bool) -> usize {
    while { let t = s; c } {
    }
    return s.len();
}
"#,
        "E-OWN-USE-AFTER-MOVE",
    );
}

#[test]
fn audit_nonstatic_reference_through_if_is_rejected() {
    // The conservative static-reference analysis must not accept a branch that
    // derives a `&str` from a local `String`.
    rejected(
        r#"
fn f(c: bool, s: String) -> &str {
    let t = if c { s.as_str() } else { "a" };
    return t;
}
"#,
        "E-OWN-REF-ESCAPE",
    );
}

#[test]
fn audit_assign_as_str_to_reference_field_is_rejected() {
    rejected(
        r#"
struct S { text: &str }
fn f(t: String) {
    let mut s = S { text: "a" };
    s.text = t.as_str();
}
"#,
        "E-OWN-REF-ESCAPE",
    );
}

#[test]
fn audit_return_local_as_str_is_rejected() {
    rejected(
        r#"
fn f() -> &str {
    let s = String::from("a");
    return s.as_str();
}
"#,
        "E-OWN-REF-ESCAPE",
    );
}

#[test]
fn audit_move_field_then_write_sibling_then_whole_is_rejected() {
    rejected(
        r#"
struct D { a: String, b: String }
fn f() -> D {
    let mut d = D { a: String::from("a"), b: String::from("b") };
    let x = d.a;
    d.b = String::from("c");
    return d;
}
"#,
        "E-OWN-PARTIAL-MOVE",
    );
}

#[test]
fn audit_write_whole_after_partial_move_restores_field() {
    accepted(
        r#"
struct D { a: String, b: String }
fn f() -> usize {
    let mut d = D { a: String::from("a"), b: String::from("b") };
    let x = d.a;
    d = D { a: String::from("c"), b: String::from("d") };
    return d.a.len();
}
"#,
    );
}

#[test]
fn audit_receiver_loan_conflicts_with_index_argument() {
    rejected(
        r#"
fn f() {
    let mut v: Vec<i32> = Vec::new();
    v.push(1);
    v.push(v[0]);
}
"#,
        "E-OWN-BORROW-CONFLICT",
    );
}

#[test]
fn audit_setter_receiver_conflicts_with_field_argument() {
    rejected(
        r#"
struct P { x: i32 }
impl P {
    fn set(&mut self, v: i32) {
        self.x = v;
    }
}
fn f() {
    let mut p = P { x: 1 };
    p.set(p.x);
}
"#,
        "E-OWN-BORROW-CONFLICT",
    );
}

// ===========================================================================
// Nested references to a static-text local (soundness probe)
// ===========================================================================

#[test]
fn audit_reference_to_static_local_is_rejected() {
    // `&text` borrows the *stack slot* `text`; its type is `&&str`, not a
    // `&'static str`, so it must not be storable/returnable.
    rejected(
        r#"
fn f() {
    let text = "a";
    let r = &text;
}
"#,
        "E-OWN-REF-ESCAPE",
    );
}

#[test]
fn audit_reference_to_string_literal_is_rejected() {
    // `&"a"` borrows a temporary of type `&str`: the value has type `&&str`
    // and its referent is not a `&'static str` value usable as static data.
    rejected(
        r#"
fn f() {
    let r = &"a";
}
"#,
        "E-OWN-REF-ESCAPE",
    );
}

#[test]
fn audit_use_of_reference_to_temporary_is_rejected() {
    // `r` borrows a temporary `&str` slot whose lifetime is the `let`
    // instruction (OWN-3.6: no temporary lifetime extension). Reading it in a
    // later instruction is a use-after-free and must be rejected.
    rejected(
        r#"
fn g(s: &str) {}
fn f() {
    let r = &"a";
    g(*r);
}
"#,
        "E-OWN-REF-ESCAPE",
    );
}
