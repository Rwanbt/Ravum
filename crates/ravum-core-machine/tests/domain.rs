//! The domain of the machine (core-machine.md §2, §6).
//!
//! * MACH-2.2: `&str` is admitted for literals and text comparison only; any
//!   other reference is outside the domain and faults instead of being given
//!   a value.
//! * MACH-6.1: `let x: T;` without an initialiser is outside the domain.
//!
//! MACH-6.4 (an expression statement is `()`) is an assumption the checker
//! guarantees: `cases/parser/block-statement-value-fail` (`E-TYP-STATEMENT-VALUE`).

use ravum_core_machine::{run, DEFAULT_BUDGET, SPEC_VERSION};

fn out(src: &str) -> String {
    run(src, "check", SPEC_VERSION, DEFAULT_BUDGET).render()
}

#[test]
fn text_literals_and_comparisons_are_in_the_domain() {
    assert_eq!(
        out("fn check() -> bool effects() {\n    return \"a\" == \"a\";\n}\n"),
        "value bool true"
    );
}

#[test]
fn other_references_are_outside_the_domain() {
    let got = out(
        "fn check() -> i64 effects() {\n    let x: i64 = 1;\n    let r: &i64 = &x;\n    return x;\n}\n",
    );
    assert!(got.starts_with("fault: outside the pure core"), "{got}");
    assert!(got.contains("references"), "{got}");
}

#[test]
fn a_let_without_initialiser_is_outside_the_domain() {
    let got = out("fn check() -> i64 effects() {\n    let x: i64;\n    x = 3;\n    return x;\n}\n");
    assert!(got.starts_with("fault: outside the pure core"), "{got}");
}
