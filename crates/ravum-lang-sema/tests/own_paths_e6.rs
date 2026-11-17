//! E6 — ownership paths that no test reached before the product-coverage
//! campaign. Every test drives the real parser and checker and asserts the
//! diagnostics the user sees, so a regression in the path changes a verdict.

use ravum_lang_sema::facts::CheckOptions;
use ravum_lang_sema::syntax::{parser, source::SourceMap};

/// Error codes reported for `source`, in emission order.
fn errors(source: &str) -> Vec<String> {
    let mut sources = SourceMap::new();
    let file = sources.add("main.rv", source);
    let parsed = parser::parse_file(file, source);
    assert!(parsed.diags.is_empty(), "the fixture must parse");
    ravum_lang_sema::check(&parsed.program, &sources, &CheckOptions::default())
        .diags
        .iter()
        .filter(|diag| diag.code.starts_with("E-"))
        .map(|diag| diag.code.clone())
        .collect()
}

/// Messages of the errors reported for `source`.
fn messages(source: &str) -> Vec<String> {
    let mut sources = SourceMap::new();
    let file = sources.add("main.rv", source);
    let parsed = parser::parse_file(file, source);
    assert!(parsed.diags.is_empty(), "the fixture must parse");
    ravum_lang_sema::check(&parsed.program, &sources, &CheckOptions::default())
        .diags
        .iter()
        .filter(|diag| diag.code.starts_with("E-"))
        .map(|diag| diag.message.clone())
        .collect()
}

#[test]
fn a_field_cannot_be_moved_out_of_a_borrowed_temporary() {
    let codes = errors(
        "struct Pair { name: String, n: i32 }\n\
         fn main() { let p = Pair { name: String::from(\"a\"), n: 1 }; let s = (&p).name; }",
    );
    assert_eq!(codes, ["E-OWN-MOVE-OUT-OF-BORROW"]);
}

#[test]
fn a_copy_field_of_a_borrowed_temporary_is_read_freely() {
    let codes = errors(
        "struct Pair { name: String, n: i32 }\n\
         fn main() { let p = Pair { name: String::from(\"a\"), n: 1 }; let n = (&p).n; let q = p; }",
    );
    assert_eq!(codes, [] as [&str; 0]);
}

#[test]
fn an_element_cannot_be_moved_out_of_a_borrowed_vector_temporary() {
    let codes = errors("fn main() { let v: Vec<String> = Vec::new(); let s = (&v)[0]; }");
    assert_eq!(codes, ["E-OWN-MOVE-OUT-OF-BORROW"]);
}

#[test]
fn an_element_cannot_be_moved_out_of_a_vector_temporary() {
    let codes = errors(
        "fn mk() -> Vec<String> { Vec::new() }\n\
         fn main() { let s = mk()[0]; }",
    );
    assert_eq!(codes, ["E-OWN-PARTIAL-MOVE"]);
}

#[test]
fn a_copy_element_of_a_vector_temporary_is_read_freely() {
    let codes = errors(
        "fn mk() -> Vec<i32> { Vec::new() }\n\
         fn main() { let s = mk()[0]; let t = (&mk())[0]; }",
    );
    assert_eq!(codes, [] as [&str; 0]);
}

#[test]
fn a_movable_value_cannot_be_moved_through_a_dereferenced_temporary() {
    let codes = errors("fn main() { let s = String::from(\"a\"); let t = *(&s); }");
    assert_eq!(codes, ["E-OWN-MOVE-OUT-OF-BORROW"]);
}

#[test]
fn an_element_cannot_be_moved_out_of_a_slice() {
    let codes = errors("fn f(s: &[String]) { let x = s[0]; }\nfn main() { }");
    assert_eq!(codes, ["E-OWN-MOVE-OUT-OF-BORROW"]);
}

#[test]
fn a_loop_in_unreachable_code_is_still_analysed_without_a_diagnostic() {
    let codes = errors("fn f() { return; for i in 0..3 { } }\nfn main() { f(); }");
    assert_eq!(codes, [] as [&str; 0]);
}

#[test]
fn a_reference_cannot_be_stored_in_a_variant_constructor() {
    let codes = errors("fn main() { let x = 1; let o = Some(&x); }");
    assert_eq!(codes, ["E-OWN-REF-ESCAPE"]);
}

#[test]
fn unwrap_or_keeps_a_static_text_only_when_its_default_is_static() {
    let accepted = errors(
        "fn main() { let o: Option<&str> = Some(\"a\"); let s: &str = o.unwrap_or(\"x\"); }",
    );
    assert_eq!(accepted, [] as [&str; 0]);
    let refused = errors(
        "fn main() { let d = String::from(\"q\"); let o: Option<&str> = Some(\"a\"); \
         let s: &str = o.unwrap_or(d.as_str()); }",
    );
    assert_eq!(refused, ["E-OWN-REF-ESCAPE"]);
}

/// `check` accepted these programs and `rustc` refused the generated Rust
/// (E0506, E0503): a block, branch or `match` nested in a later argument ended
/// the loan taken by an earlier argument of the same call.
#[test]
fn a_loan_taken_by_an_argument_holds_inside_a_later_nested_block() {
    let write = messages(
        "fn h(a: &i32, b: i32) -> i32 { *a + b }\n\
         fn main() { let mut x = 1; let r = h(&x, { x = 2; 1 }); }",
    );
    assert_eq!(write, ["écriture de `x` pendant qu'il est emprunté"]);

    let field_write = messages(
        "struct P { a: i32 }\n\
         fn h(a: &P, b: i32) -> i32 { b }\n\
         fn main() { let mut p = P { a: 1 }; let r = h(&p, { p.a = 3; 1 }); }",
    );
    assert_eq!(
        field_write,
        ["écriture de `p.a` pendant qu'il est emprunté"]
    );

    let read_under_exclusive = errors(
        "fn g(a: &mut i32, b: i32) { }\n\
         fn main() { let mut x = 1; g(&mut x, { let y = x; y }); }",
    );
    assert_eq!(read_under_exclusive, ["E-OWN-BORROW-CONFLICT"]);

    let in_a_branch = errors(
        "fn h(a: &i32, b: i32) -> i32 { *a + b }\n\
         fn main() { let mut x = 1; let c = true; let r = h(&x, if c { x = 2; 1 } else { 3 }); }",
    );
    assert_eq!(in_a_branch, ["E-OWN-BORROW-CONFLICT"]);

    let in_a_match = errors(
        "fn h(a: &i32, b: i32) -> i32 { *a + b }\n\
         fn main() { let mut x = 1; let k = 0; let r = h(&x, match k { 0 => { x = 2; 1 } _ => 3 }); }",
    );
    assert_eq!(in_a_match, ["E-OWN-BORROW-CONFLICT"]);
}

/// The loan ends with the call: the same writes are accepted once the call
/// has returned, and a nested block that touches another place is unaffected.
#[test]
fn a_loan_taken_by_an_argument_ends_with_its_call() {
    let codes = errors(
        "fn h(a: &i32, b: i32) -> i32 { *a + b }\n\
         fn main() { let mut x = 1; let mut y = 5; let r = h(&x, { y = 2; y }); x = r; \
         let s = h(&y, 1); y = s; }",
    );
    assert_eq!(codes, [] as [&str; 0]);
}

#[test]
fn a_second_move_of_a_tuple_field_names_the_field() {
    let found = messages(
        "fn main() { let t = (String::from(\"a\"), String::from(\"b\")); let a = t.0; let b = t.0; }",
    );
    assert_eq!(found.len(), 1);
    assert!(found[0].contains("`t.0`"), "{found:?}");
}

#[test]
fn an_array_inside_a_tuple_is_restored_element_by_element() {
    let codes = errors(
        "fn main() { let mut t = ([String::from(\"a\"), String::from(\"b\")], 1); \
         let a = t.0[0]; t.0[1] = String::from(\"c\"); t.0[0] = String::from(\"d\"); let u = t; }",
    );
    assert_eq!(codes, [] as [&str; 0]);
}

#[test]
fn a_reference_cannot_be_returned_inside_an_option_or_stored_in_a_vector_field() {
    let codes = errors(
        "fn f(x: &i32) -> Option<&i32> { Some(x) }\n\
         struct S { v: Vec<&i32> }\n\
         fn main() { }",
    );
    assert_eq!(codes, ["E-OWN-REF-ESCAPE", "E-OWN-REF-ESCAPE"]);
}
