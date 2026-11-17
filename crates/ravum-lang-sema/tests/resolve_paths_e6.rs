//! E6 — name-resolution paths that no test reached.
//!
//! Every test drives the real parser and checker on one program and pins the
//! errors the user reads: code and message, in emission order.

use ravum_lang_sema::facts::CheckOptions;
use ravum_lang_sema::syntax::{parser, source::SourceMap};

/// `(code, message)` of every error reported for `source`. Parse errors are
/// included: a fixture the parser refuses is pinned as such.
fn errors(source: &str) -> Vec<(String, String)> {
    let mut sources = SourceMap::new();
    let file = sources.add("main.rv", source);
    let parsed = parser::parse_file(file, source);
    let mut found: Vec<(String, String)> = parsed
        .diags
        .iter()
        .map(|diag| (diag.code.clone(), diag.message.clone()))
        .collect();
    if found.is_empty() {
        found = ravum_lang_sema::check(&parsed.program, &sources, &CheckOptions::default())
            .diags
            .iter()
            .filter(|diag| diag.code.starts_with("E-"))
            .map(|diag| (diag.code.clone(), diag.message.clone()))
            .collect();
    }
    found
}

/// The errors of `source` are exactly `expected`, in order.
fn assert_errors(source: &str, expected: &[(&str, &str)]) {
    let found = errors(source);
    let found: Vec<(&str, &str)> = found
        .iter()
        .map(|(code, message)| (code.as_str(), message.as_str()))
        .collect();
    assert_eq!(found, expected, "{source}");
}

/// A module name, user or builtin, is not a value.
#[test]
fn a_module_is_not_a_value() {
    assert_errors(
        "mod m { pub fn f() { } }\n\
         fn main() { let x = m; let y = std::io; let z = io; }",
        &[
            ("E-RES-UNKNOWN", "unknown name `m`"),
            ("E-TYP-CALL", "a module cannot be used as a value"),
            ("E-RES-UNKNOWN", "unknown name `io`"),
        ],
    );
}

#[test]
fn a_private_module_cannot_be_crossed() {
    assert_errors(
        "mod a { mod b { pub fn f() { } } pub fn g() { } }\n\
         fn main() { a::b::f(); }",
        &[("E-RES-PRIVATE", "`b` is private")],
    );
}

#[test]
fn a_path_cannot_continue_through_a_function_or_end_on_a_type() {
    assert_errors(
        "mod m { pub fn f() { } pub struct S { n: i32 } }\n\
         fn main() { m::f::g(); let s = m::S; let t = m::nothing; }",
        &[
            ("E-RES-UNKNOWN", "unknown name `f`"),
            ("E-TYP-CALL", "type `S` cannot be used as a value"),
            ("E-RES-UNKNOWN", "unknown name `nothing`"),
        ],
    );
}

#[test]
fn builtin_modules_and_functions_can_be_imported() {
    assert_errors(
        "use std::io;\n\
         use std::io::println;\n\
         use std::math;\n\
         fn main() { io::println(\"a\"); println(\"b\"); let r = math::abs(-1.0); }",
        &[],
    );
}

#[test]
fn a_module_and_a_function_can_be_imported_from_a_user_module() {
    assert_errors(
        "mod a { pub mod b { pub fn f() { } } pub fn g() { } }\n\
         use a::b;\n\
         use a::g;\n\
         fn main() { b::f(); g(); }",
        &[],
    );
}

#[test]
fn an_import_cannot_continue_through_a_function_or_a_missing_module() {
    assert_errors(
        "mod a { pub fn g() { } }\n\
         use a::g::x;\n\
         use a::missing::x;\n\
         fn main() { }",
        &[
            ("E-RES-UNKNOWN", "unknown import path `a::g::x`"),
            ("E-RES-UNKNOWN", "unknown import path `a::missing::x`"),
        ],
    );
}

#[test]
fn the_same_import_twice_is_a_duplicate() {
    assert_errors(
        "mod a { pub fn g() { } }\n\
         use a::g;\n\
         use a::g;\n\
         fn main() { g(); }",
        &[(
            "E-RES-DUPLICATE",
            "import `g` conflicts with an existing import",
        )],
    );
}

/// MOD-3.2: a bare path is tried from the current module, then from the root.
#[test]
fn a_bare_import_path_resolves_from_the_crate_root_inside_a_module() {
    assert_errors(
        "mod a { pub fn g() { } }\n\
         mod b { use a::g; pub fn h() { g(); } }\n\
         fn main() { b::h(); }",
        &[],
    );
}

#[test]
fn a_bare_import_path_that_resolves_both_ways_to_different_items_is_ambiguous() {
    assert_errors(
        "mod a { pub fn g() { } }\n\
         mod b { pub mod a { pub fn g() { } } use a::g; pub fn h() { g(); } }\n\
         fn main() { b::h(); }",
        &[
            ("E-RES-CONFLICT", "`a::g` is ambiguous: it resolves both in the current module and at the crate root; use a `crate::`/`self::`/`super::` prefix"),
        ],
    );
}

#[test]
fn constructors_and_types_are_not_plain_values() {
    assert_errors(
        "struct P { n: i32 }\n\
         fn main() { let s = Some; let o = Ok; let p = P; let q = P::new(); let v = Option::Nope; }",
        &[
            ("E-TYP-CALL", "variant constructor `Option::Some` requires arguments"),
            ("E-TYP-CALL", "variant constructor `Result::Ok` requires arguments"),
            ("E-TYP-CALL", "type `P` cannot be used as a value"),
            ("E-RES-UNKNOWN", "unknown name `new`"),
            ("E-RES-UNKNOWN", "unknown name `Nope`"),
        ],
    );
}

#[test]
fn a_private_item_cannot_be_imported() {
    assert_errors(
        "mod a { fn private() { } pub fn public() { } }\n\
         use a::private;\n\
         fn main() { a::public(); }",
        &[(
            "E-RES-PRIVATE",
            "`a::private` is private and cannot be imported here",
        )],
    );
}

#[test]
fn a_private_item_cannot_be_named_through_its_module() {
    assert_errors(
        "mod a { fn private() { } pub fn public() { } }\n\
         fn main() { a::private(); }",
        &[("E-RES-PRIVATE", "`private` is private")],
    );
}

#[test]
fn self_type_is_refused_as_a_value_by_the_parser() {
    assert_errors(
        "fn main() { Self::f(); }",
        &[
            ("E-PARSE-UNEXPECTED", "`Self` is a type, not an expression"),
            ("E-PARSE-EXPECTED", "expected `;`, found `::`"),
            ("E-PARSE-UNEXPECTED", "expected an expression, found `::`"),
            ("E-PARSE-EXPECTED", "expected `;`, found identifier `f`"),
        ],
    );
}

#[test]
fn a_builtin_module_is_reached_by_import_or_by_full_path() {
    assert_errors(
        "use std::io;\n\
         fn main() { io::println(\"a\"); std::io::println(\"b\"); let n = std::math::abs(-2.0); }",
        &[],
    );
}

#[test]
fn imports_of_private_unknown_and_missing_names_are_told_apart() {
    assert_errors(
        "mod a { pub mod b { fn hidden() { } pub fn shown() { } } }\n\
         use a::b::hidden;\n\
         use a::b::nothing;\n\
         use a::c::d;\n\
         use nothing_at_all;\n\
         fn main() { }",
        &[
            (
                "E-RES-PRIVATE",
                "`a::b::hidden` is private and cannot be imported here",
            ),
            ("E-RES-UNKNOWN", "unknown import path `a::b::nothing`"),
            ("E-RES-UNKNOWN", "unknown import path `a::c::d`"),
            ("E-RES-UNKNOWN", "unknown import path `nothing_at_all`"),
        ],
    );
}

#[test]
fn a_single_name_import_must_name_something_importable() {
    assert_errors(
        "mod a { pub fn g() { } }\n\
         use a;\n\
         use std;\n\
         use io;\n\
         fn main() { a::g(); }",
        &[
            (
                "E-RES-DUPLICATE",
                "import `a` conflicts with an existing name",
            ),
            ("E-RES-UNKNOWN", "unknown import path `std`"),
            ("E-RES-UNKNOWN", "unknown import path `io`"),
        ],
    );
}
