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

/// `check` accepted these programs and `rustc` refused the generated Rust
/// (E0382): past 256 levels the ownership analysis gave up without a
/// diagnostic, and a 300-link chain builds a tree that deep. Chains within
/// the nesting limit are analysed to the bottom; longer ones do not parse.
///
/// The checker recurses on the tree, so the test runs on a stack sized like
/// the toolchain's own (`ravum-lang` runs every command on a 64 MiB thread).
#[test]
fn a_double_move_under_a_long_chain_is_still_reported() {
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(double_moves_under_long_chains)
        .expect("the thread starts")
        .join()
        .expect("the checks pass");
}

fn double_moves_under_long_chains() {
    let sum = format!(
        "fn m(a: String) -> i32 {{ 1 }}\n\
         fn main() {{ let s = String::from(\"a\"); let r = m(s) + m(s){}; }}",
        " + 0".repeat(200)
    );
    assert_eq!(errors(&sum), ["E-OWN-USE-AFTER-MOVE"]);

    let branches = format!(
        "fn m(a: String) {{ }}\n\
         fn main() {{ let c = true; let s = String::from(\"a\"); \
         if c {{ }}{} else {{ m(s); m(s); }} }}",
        " else if c { }".repeat(200)
    );
    assert_eq!(errors(&branches), ["E-OWN-USE-AFTER-MOVE"]);

    let blocks = format!(
        "fn m(a: String) {{ }}\n\
         fn main() {{ let s = String::from(\"a\"); {} m(s); m(s); {} }}",
        "{".repeat(80),
        "}".repeat(80)
    );
    assert_eq!(errors(&blocks), ["E-OWN-USE-AFTER-MOVE"]);
}

#[test]
fn a_field_of_an_uninitialised_binding_cannot_be_written() {
    let codes = errors(
        "struct P { a: i32, b: i32 }\n\
         fn main() { let mut p: P; p.a = 1; }",
    );
    assert_eq!(codes, ["E-OWN-UNINIT"]);
}

/// OWN-5.3: writes into a partially moved aggregate. A sibling field, the
/// moved field itself and an ancestor of the moved field are all writable, and
/// each write restores exactly what it covers.
#[test]
fn a_partially_moved_struct_accepts_sibling_exact_and_ancestor_writes() {
    let codes = errors(
        "struct Q { x: String, y: String }\n\
         struct P { q: Q, n: String }\n\
         fn main() { \
         let mut p = P { q: Q { x: String::from(\"a\"), y: String::from(\"b\") }, n: String::from(\"c\") }; \
         let a = p.q.x; p.n = String::from(\"d\"); p.q.x = String::from(\"e\"); \
         let b = p.q.y; p.q = Q { x: String::from(\"a\"), y: String::from(\"b\") }; \
         let c = p.q.x; let d = p.n; p.q.y = String::from(\"z\"); }",
    );
    assert_eq!(codes, [] as [&str; 0]);
}

#[test]
fn a_compound_assignment_reads_then_writes_under_an_exclusive_loan() {
    let found = messages(
        "fn g(a: &mut i32, b: i32) { }\n\
         fn main() { let mut x = 1; g(&mut x, { x += 1; 1 }); }",
    );
    assert_eq!(
        found,
        [
            "lecture de `x` pendant un emprunt exclusif",
            "écriture de `x` pendant qu'il est emprunté"
        ]
    );
}

/// OWN-5.11: an element is restored one by one only while no other part of the
/// same array is moved.
#[test]
fn an_element_is_not_restored_while_another_part_of_the_array_is_moved() {
    let codes = errors(
        "fn main() { let mut a = [[String::from(\"a\"), String::from(\"b\")], \
         [String::from(\"c\"), String::from(\"d\")]]; \
         let x = a[0][0]; let y = a[1][1]; a[0][0] = String::from(\"e\"); }",
    );
    assert_eq!(codes, ["E-OWN-PARTIAL-MOVE"]);
}

#[test]
fn an_array_of_tuples_is_restored_by_element_and_by_field() {
    let codes = errors(
        "fn main() { let mut a = [(String::from(\"a\"), String::from(\"b\")), \
         (String::from(\"c\"), String::from(\"d\"))]; \
         let x = a[0].0; a[0] = (String::from(\"e\"), String::from(\"f\")); \
         let y = a[1].1; a[1].1 = String::from(\"g\"); let z = a; }",
    );
    assert_eq!(codes, [] as [&str; 0]);
}

#[test]
fn a_loop_cannot_bind_references_taken_out_of_a_vector() {
    let codes = errors("fn f(v: Vec<&i32>) { for r in v { } }\nfn main() { }");
    assert_eq!(codes, ["E-OWN-REF-ESCAPE"]);
}

/// OWN-6.2: a reference place handed to a generic parameter or a variant
/// payload would be stored; only a `&'static str` literal or a fresh `&` borrow
/// of the call's duration is accepted.
#[test]
fn a_reference_place_passed_by_value_to_a_generic_or_a_variant_is_refused() {
    for source in [
        "enum W<T> { A(T), B }\nfn f(s: &String) { drop(W::A(s)); }\nfn main() { }",
        "fn id<T>(x: T) { }\nfn f(s: &String) { id(s); }\nfn main() { }",
    ] {
        assert_eq!(errors(source), ["E-OWN-REF-ESCAPE"], "{source}");
    }
    for source in [
        "fn id<T>(x: T) { }\nfn main() { id(\"lit\"); }",
        "fn id<T>(x: T) { }\nfn f(s: &String) { id(&s); }\nfn main() { }",
    ] {
        assert!(errors(source).is_empty(), "{source}");
    }
}

/// Writes after a partial move: to a moved field (re-initialisation), to a
/// sibling field, to an enclosing tuple field, and to a sibling array element
/// followed by a use of the whole array.
#[test]
fn writes_after_a_partial_move_reinitialise_or_keep_the_partial_state() {
    for source in [
        "struct P { n: String, m: String }\n\
         fn main() { let mut p = P { n: String::from(\"a\"), m: String::from(\"b\") }; \
         let a = p.n; p.m = String::from(\"c\"); drop(a); drop(p.m); }",
        "struct P { n: String, m: String }\n\
         fn main() { let mut p = P { n: String::from(\"a\"), m: String::from(\"b\") }; \
         let a = p.n; p.n = String::from(\"c\"); drop(a); drop(p); }",
        "struct Q { t: (String, String) }\n\
         fn main() { let mut q = Q { t: (String::from(\"a\"), String::from(\"b\")) }; \
         let x = q.t.0; q.t = (String::from(\"c\"), String::from(\"d\")); drop(x); drop(q); }",
    ] {
        assert!(errors(source).is_empty(), "{source}");
    }
    let codes = errors(
        "fn main() { let mut a = [String::from(\"a\"), String::from(\"b\")]; \
         let x = a[0]; a[1] = String::from(\"c\"); drop(x); drop(a); }",
    );
    assert_eq!(codes, ["E-OWN-PARTIAL-MOVE"]);
    let codes = errors("fn f(s: &[String]) { let x = s[0]; drop(x); }\nfn main() { }");
    assert_eq!(codes, ["E-OWN-MOVE-OUT-OF-BORROW"]);
}

/// A reference argument stored by a builtin `push`, either a reference-typed
/// place or a reference rvalue, is an escape.
#[test]
fn a_reference_pushed_into_a_vec_escapes() {
    for source in [
        "fn f(r: &String) { let mut v: Vec<&String> = Vec::new(); v.push(r); }\nfn main() { }",
        "fn f(s: &String) { let mut v: Vec<&str> = Vec::new(); v.push(s.as_str()); }\nfn main() { }",
    ] {
        assert!(
            errors(source).contains(&"E-OWN-REF-ESCAPE".to_string()),
            "{source}: {:?}",
            errors(source)
        );
    }
}

/// A `for` whose iterator diverges after a `return` keeps a `break` of its dead
/// body out of the exit state: the function checks.
#[test]
fn a_for_over_a_diverging_iterator_with_a_break_checks() {
    assert_eq!(
        errors("fn f(v: Vec<i32>) -> i32 { for i in { return 1; v } { break; } 2 }\nfn main() { }"),
        Vec::<String>::new()
    );
}

/// After a field is moved out, writing a disjoint field and then the moved one
/// is accepted (OWN-5.4).
#[test]
fn writes_after_a_partial_move_restore_the_aggregate() {
    assert_eq!(
        errors(
            "struct P { name: String, n: i32 }\n\
             fn main() { let mut p = P { name: String::from(\"a\"), n: 1 }; let s = p.name; \
             p.n = 3; p.name = String::from(\"x\"); let t = p; }"
        ),
        Vec::<String>::new()
    );
}

/// Moving an element out of a borrowed slice moves out of borrowed memory.
#[test]
fn moving_out_of_a_slice_element_is_refused() {
    let codes = errors("fn f(s: &[String]) { let x = s[0]; }\nfn main() { }");
    assert!(
        codes.contains(&"E-OWN-MOVE-OUT-OF-BORROW".to_string()),
        "{codes:?}"
    );
}

/// Writing below a moved field (`p.a.b = ..` after `let x = p.a;`) writes into
/// moved memory: E-OWN-USE-AFTER-MOVE, as rustc E0382.
#[test]
fn writing_below_a_moved_field_is_a_use_after_move() {
    assert_eq!(
        errors(
            "struct Q { b: String }\nstruct P { a: Q }\n\
             fn main() { let mut p = P { a: Q { b: String::from(\"a\") } }; let x = p.a; \
             p.a.b = String::from(\"y\"); }"
        ),
        ["E-OWN-USE-AFTER-MOVE"]
    );
}

/// An array repeat of a value that is not `Copy`, as a temporary or as a
/// place, is refused once (G-5.5, E-TYP-ARRAY-REPEAT from the ownership phase).
#[test]
fn an_array_repeat_of_a_non_copy_value_is_refused() {
    for source in [
        "fn main() { let a = [String::from(\"a\"); 3]; }",
        "fn main() { let s = String::from(\"a\"); let a = [s; 3]; }",
    ] {
        assert_eq!(errors(source), ["E-TYP-ARRAY-REPEAT"], "{source}");
    }
}

/// `self` passed on as a reference argument is a reborrow of a parameter,
/// not a `'static` reference: the program checks.
#[test]
fn self_passed_on_as_a_reference_argument_checks() {
    assert_eq!(
        errors(
            "struct S { n: i32 }\nfn take(s: &S) -> i32 { s.n }\n\
             impl S { fn m(&self) -> i32 { take(self) } }\nfn main() { }"
        ),
        Vec::<String>::new()
    );
}
