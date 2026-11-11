//! Contract tests for the Ravum name resolver, second batch
//! (`crates/ravum-lang-sema/src/resolve.rs`).
//!
//! These cases lock the four defects recorded in
//! `docs/qualification/T2-FINDINGS.md` §19.2, items 2, 3, 6 and 7:
//!
//! * item 2 — MOD-4.3: a private `mod geo` is visible from its declaring
//!   module **and its descendants**, so importing it from a descendant must
//!   be accepted (the resolver used to pass the target module id as the
//!   defining module and reported `E-RES-PRIVATE`);
//! * item 3 — MOD-4.2/MOD-6.2: every module on the path of a `use` must be
//!   visible, not only the final segment;
//! * item 6 — MOD-3.7 assigns `E-RES-DUPLICATE` to import/local and
//!   import/import collisions; `E-RES-CONFLICT` is only emitted for a prelude
//!   name collision, a case no rule describes;
//! * item 7 — `E-RES-SELF` (`self` parameter outside a valid context) and
//!   `E-RES-SELF-OUTSIDE-IMPL` (`Self` outside an `impl`) are two registry
//!   entries; both codes must be observed, and the sites the parser cannot
//!   reach are recorded as such instead of being claimed as covered.
//!
//! It also locks MOD-3.2 (MUST): a bare relative `use` path is interpreted
//! relative to the current module first, then to the crate root, and a
//! double resolution to different entities is `E-RES-CONFLICT`.
//!
//! Each case is a small program plus the outcome `spec/language/` requires:
//! the code named by `diagnostics.md` §3 and the rule cited above the test.
//! A case that the checker rejects for a *different* reason than the expected
//! code is a failure: `expect_error_codes` compares the **exact** set of error
//! codes, so no test can accept "some diagnostic". Warnings (`W-*`) are outside
//! the contract measured here (`diagnostics.md` DIAG-1.6 keeps them separate
//! from errors) and are filtered out, but `accept` still keeps them visible.
//!
//! Run with:
//! `cargo test -p ravum-lang-sema --test resolve_contracts2`

use ravum_lang_sema::facts::Facts;
use ravum_lang_sema::syntax::diag::DiagnosticBag;
use ravum_lang_sema::syntax::{parser, source::SourceMap, LANGUAGE_EDITION};
use ravum_lang_sema::{check, CheckOptions, CheckResult, CheckedProgram, Profile};

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

/// Error codes only: `diagnostics.md` DIAG-1.6 keeps warnings in a separate,
/// closed list, and an expected `E-*` never justifies a `W-*`.
fn error_codes(result: &CheckResult) -> Vec<String> {
    let mut codes: Vec<String> = result
        .diags
        .iter()
        .map(|d| d.code.clone())
        .filter(|code| code.starts_with("E-"))
        .collect();
    codes.sort();
    codes
}

/// The program must be rejected with **exactly** these error codes. Extra
/// errors are a failure: they would mean the case is not exercising the rule
/// it claims to pin.
fn expect_error_codes(src: &str, expected: &[&str]) {
    let result = run(src);
    assert!(
        result.checked.is_none(),
        "expected {expected:?} but the program was accepted\n--- source ---\n{src}"
    );
    let mut want: Vec<String> = expected.iter().map(|code| code.to_string()).collect();
    want.sort();
    let got = error_codes(&result);
    assert_eq!(
        got,
        want,
        "expected exactly {want:?}, got {got:?} ({})\n--- source ---\n{src}",
        describe(&result)
    );
}

/// The program must be accepted with no *error* diagnostic; warnings are
/// allowed but stay visible in the failure message. Returns the checked
/// program so a test can assert the facts the resolver filled.
fn accept(src: &str) -> CheckedProgram {
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
    result.checked.expect("checked program")
}

/// Parse only: used for the sites the parser makes unreachable, so a test can
/// state *why* a resolver code cannot be observed instead of pretending it is
/// covered by the public pipeline.
fn parse_codes(src: &str) -> Vec<String> {
    let mut sources = SourceMap::new();
    let file = sources.add("src/lib.rv", src);
    let parsed = parser::parse_file(file, src);
    parsed.diags.iter().map(|d| d.code.clone()).collect()
}

// ===========================================================================
// MOD-4.3 — visibility of a private module in a `use`
// (T2-FINDINGS §19.2 item 2)
// ===========================================================================

#[test]
fn private_module_imported_by_descendant_is_accepted() {
    // MOD-4.3 (MUST): a private module is visible in its defining module
    // **and in all of its descendants**. `geo` is declared at the crate root,
    // so the root's descendant `site` may import it; MOD-3.1/MOD-3.2 make
    // `use crate::geo;` the normative form. Before the fix this program was
    // rejected with `E-RES-PRIVATE` because the resolver used the target
    // module as the defining module.
    let program = accept(
        r#"
mod geo { pub fn f() -> i32 { return 1; } }
mod site {
    use crate::geo;
    pub fn g() -> i32 { return geo::f(); }
}
"#,
    );
    assert!(
        program.facts.fn_sigs.values().any(|sig| sig.name == "g"),
        "the importing module must be fully checked"
    );
}

#[test]
fn private_module_imported_by_sibling_is_e_res_private() {
    // MOD-4.3 (MUST): a module parent does not see the private items of its
    // children. `geo` is declared inside `a`; the sibling `b` is not a
    // descendant of `a`, so the final segment is `E-RES-PRIVATE`
    // (`diagnostics.md` §3.2: private item accessed outside its subtree).
    expect_error_codes(
        r#"
mod a { mod geo { pub fn f() -> i32 { return 1; } } }
mod b { use crate::a::geo; }
"#,
        &["E-RES-PRIVATE"],
    );
}

#[test]
fn private_module_imported_into_declaring_module_is_e_res_duplicate() {
    // MOD-3.7 (MUST): an imported name may not collide with an item declared
    // in the same module. `use crate::geo;` names the local `mod geo`, so the
    // collision is `E-RES-DUPLICATE` — not a privacy error. This is the exact
    // program of T2-FINDINGS §19.2 item 2: measured `E-RES-PRIVATE` before the
    // fix, `E-RES-DUPLICATE` after, which is what MOD-3.7 requires.
    expect_error_codes(
        r#"
mod geo { pub fn f() -> i32 { return 1; } }
use crate::geo;
"#,
        &["E-RES-DUPLICATE"],
    );
}

#[test]
fn private_module_imported_via_self_in_declaring_module_is_e_res_duplicate() {
    // MOD-4.3 + MOD-3.7: the defining module may name its own private module
    // (`use crate::site::geo;` inside `site` is visible), but the name
    // `geo` is already declared there, so the import is a duplicate. Before
    // the fix the visibility error masked the real conflict.
    expect_error_codes(
        r#"
mod site {
    mod geo { pub fn f() { } }
    use crate::site::geo;
}
"#,
        &["E-RES-DUPLICATE"],
    );
}

// ===========================================================================
// MOD-4.2 / MOD-6.2 — every module on a `use` path must be visible
// (T2-FINDINGS §19.2 item 3)
// ===========================================================================

#[test]
fn use_through_private_intermediate_module_is_e_res_private() {
    // MOD-4.2/MOD-6.2/MOD-3.9 (MUST): only `pub` modules may appear on the
    // path; a private intermediate module denies access to everything below
    // it. `secret` is declared in `a` and `b` is not a descendant of `a`, so
    // `use crate::a::secret::X;` is `E-RES-PRIVATE` even though the final
    // item `X` is `pub`. Before the fix only the final segment was checked
    // and this import was **accepted** (a confidentiality hole).
    let result = run(r#"
mod a { mod secret { pub struct X { pub x: i32 } } }
mod b { use crate::a::secret::X; }
"#);
    assert!(result.checked.is_none(), "expected the use to be rejected");
    assert_eq!(
        error_codes(&result),
        vec!["E-RES-PRIVATE".to_string()],
        "expected exactly E-RES-PRIVATE, got {}",
        describe(&result)
    );
    assert!(
        describe(&result).contains("`secret`"),
        "the message must cite the offending segment: {}",
        describe(&result)
    );
}

#[test]
fn use_through_private_intermediate_from_descendant_is_accepted() {
    // MOD-4.3 (MUST): `secret` is private but `inner` is a descendant of its
    // defining module `a`, so the traversal is allowed and `X` is `pub`
    // (MOD-4.2). The privacy rule must not reject the legitimate case.
    accept(
        r#"
mod a {
    mod secret { pub struct X { pub x: i32 } }
    mod inner {
        use crate::a::secret::X;
        pub fn g(v: X) -> i32 { return v.x; }
    }
}
"#,
    );
}

#[test]
fn use_through_first_private_intermediate_names_that_segment() {
    // MOD-4.2/MOD-6.2: the first non-public module on the path is the one the
    // diagnostic cites. Here `a` is declared at the root and visible from
    // `z`, but `b` is private in `a` and `z` is not a descendant of `a`.
    let result = run(r#"
mod a { mod b { mod c { pub struct X { pub x: i32 } } } }
mod z { use crate::a::b::c::X; }
"#);
    assert!(result.checked.is_none(), "expected the use to be rejected");
    assert_eq!(
        error_codes(&result),
        vec!["E-RES-PRIVATE".to_string()],
        "expected exactly E-RES-PRIVATE, got {}",
        describe(&result)
    );
    assert!(
        describe(&result).contains("`b`"),
        "the diagnostic must cite `b`, the first private segment: {}",
        describe(&result)
    );
}

#[test]
fn use_through_public_modules_is_accepted() {
    // MOD-4.2 (MUST): a `pub` item is visible from anywhere in the crate when
    // every module of the path is itself `pub`. This is the positive
    // counterpart of the intermediate-visibility check.
    accept(
        r#"
mod a { pub mod b { pub struct X { pub x: i32 } } }
use crate::a::b::X;
"#,
    );
}

#[test]
fn use_through_builtin_modules_is_accepted() {
    // MOD-6.2/MOD-7.3 (MUST): the implicit `std` package exposes only public
    // modules, so a multi-segment builtin path has nothing private to reject.
    accept(
        r#"
use std::io::println;

fn announce() { println("ok"); }
"#,
    );
}

// ===========================================================================
// MOD-3.2 — bare relative `use`: current module first, then the crate root
// ===========================================================================

#[test]
fn bare_relative_use_falls_back_to_crate_root() {
    // MOD-3.2 (MUST): "un chemin relatif nu (`use foo::Bar;`) est interprété
    // relativement au module courant d'abord, puis à la racine du crate".
    // `site` declares no `util`, so the second interpretation applies. Before
    // the fix the fallback was missing and the valid program was refused with
    // E-RES-UNKNOWN.
    accept(
        r#"
mod util { pub fn helper() -> i32 { return 1; } }
mod site {
    use util::helper;
    pub fn g() -> i32 { return helper(); }
}
"#,
    );
}

#[test]
fn bare_relative_use_prefers_local_over_root() {
    // MOD-3.2: the current module is tried **first**. The crate root declares
    // no `util`, so only `site::util` resolves and the import is accepted;
    // there is no conflict because the second interpretation does not exist.
    accept(
        r#"
mod site {
    mod util { pub fn helper() -> i32 { return 1; } }
    use util::helper;
    pub fn g() -> i32 { return helper(); }
}
"#,
    );
}

#[test]
fn bare_relative_use_double_resolution_is_e_res_conflict() {
    // MOD-3.2 (MUST) + `diagnostics.md` §3.2 E-RES-CONFLICT: both the current
    // module and the crate root declare `util`, and both paths resolve to a
    // **different** `helper`, so the bare path is ambiguous and is refused.
    expect_error_codes(
        r#"
mod util { pub fn helper() -> i32 { return 1; } }
mod site {
    mod util { pub fn helper() -> i32 { return 2; } }
    use util::helper;
}
"#,
        &["E-RES-CONFLICT"],
    );
}

#[test]
fn bare_relative_use_unknown_in_both_is_e_res_unknown() {
    // MOD-3.2: both interpretations fail, so the fallback is exhausted and
    // `diagnostics.md` E-RES-UNKNOWN is reported (behaviour unchanged).
    expect_error_codes("mod site { use nowhere::Thing; }\n", &["E-RES-UNKNOWN"]);
}

#[test]
fn bare_relative_use_falls_back_when_local_path_does_not_resolve() {
    // MOD-3.2 "d'abord ... puis ...": the local `util` exists but has no
    // `helper`, so the path does not resolve locally and the crate-root
    // interpretation is used. Reading the rule as "fallback only when the
    // first segment is absent" would refuse this valid program; the wording
    // and the double-resolution clause make the full-path fallback the
    // faithful reading.
    accept(
        r#"
mod util { pub fn helper() -> i32 { return 1; } }
mod site {
    mod util { pub fn other() { } }
    use util::helper;
    pub fn g() -> i32 { return helper(); }
}
"#,
    );
}

#[test]
fn bare_single_segment_use_falls_back_to_crate_root() {
    // MOD-3.2 applies to a bare path with one segment too: the name is
    // unknown in `site`, then found at the crate root. Before the fix this
    // valid program was refused with E-RES-UNKNOWN.
    accept(
        r#"
fn helper() -> i32 { return 1; }
mod site {
    use helper;
    pub fn g() -> i32 { return helper(); }
}
"#,
    );
}

#[test]
fn bare_single_segment_use_unknown_is_e_res_unknown() {
    // MOD-3.2: local then root, both unknown -> `diagnostics.md`
    // E-RES-UNKNOWN (no diagnostic is invented for the failed fallback).
    expect_error_codes("mod site { use nowhere; }\n", &["E-RES-UNKNOWN"]);
}

#[test]
fn bare_relative_use_reaching_the_same_entity_is_not_a_conflict() {
    // MOD-3.2's conflict requires two **different** entities. `site` imports
    // the root's `util`; the bare path then resolves to that same module
    // through both routes, so it is accepted (no spurious E-RES-CONFLICT).
    accept(
        r#"
mod util { pub fn helper() -> i32 { return 1; } }
mod site {
    use crate::util;
    use util::helper;
    pub fn g() -> i32 { return helper(); }
}
"#,
    );
}

#[test]
fn crate_prefixed_use_is_not_ambiguity_checked() {
    // MOD-3.2 only reinterprets **bare** paths. `use crate::util::helper;`
    // is absolute even though `site` also declares a `util` module; the
    // prefix must pin the crate-root interpretation (no E-RES-CONFLICT).
    accept(
        r#"
mod util { pub fn helper() -> i32 { return 1; } }
mod site {
    mod util { pub fn helper() -> i32 { return 2; } }
    use crate::util::helper;
    pub fn g() -> i32 { return helper(); }
}
"#,
    );
}

// ===========================================================================
// MOD-3.7 vs E-RES-CONFLICT — which code names which collision
// (T2-FINDINGS §19.2 item 6)
// ===========================================================================

#[test]
fn import_local_collision_is_e_res_duplicate_not_conflict() {
    // MOD-3.7 (MUST): a name imported into a module collides with an item
    // declared in the same module -> `E-RES-DUPLICATE` (`diagnostics.md` §3.2).
    // `diagnostics.md` also assigns this case to `E-RES-CONFLICT`; the
    // normative module rule names `E-RES-DUPLICATE` and the corpus locks it
    // (`resolve/import-collision`), so the duplicate code is the one asserted
    // here and `E-RES-CONFLICT` must **not** be emitted.
    let result = run(r#"
mod a { pub fn f() { } }
use crate::a::f;
fn f() { }
"#);
    assert!(
        result.checked.is_none(),
        "expected the program to be rejected"
    );
    assert_eq!(
        error_codes(&result),
        vec!["E-RES-DUPLICATE".to_string()],
        "expected exactly E-RES-DUPLICATE, got {}",
        describe(&result)
    );
}

#[test]
fn import_import_collision_is_e_res_duplicate_not_conflict() {
    // MOD-3.7 (MUST): two `use` importing the same name are
    // `E-RES-DUPLICATE`, even when they come from different modules.
    let result = run(r#"
mod a { pub struct Thing { pub x: i32 } }
mod b { pub struct Thing { pub y: i32 } }
use crate::a::Thing;
use crate::b::Thing;
"#);
    assert!(
        result.checked.is_none(),
        "expected the program to be rejected"
    );
    assert_eq!(
        error_codes(&result),
        vec!["E-RES-DUPLICATE".to_string()],
        "expected exactly E-RES-DUPLICATE, got {}",
        describe(&result)
    );
}

#[test]
fn prelude_collision_is_e_res_conflict_not_duplicate() {
    // `diagnostics.md` §3.2 E-RES-CONFLICT: prelude names are reserved. This
    // is the only collision the implementation actually reports with that
    // code (T2-FINDINGS §19.2 item 6: no language rule describes it, but the
    // prelude reservation is implemented); the local name is *not* a
    // duplicate (MOD-3.7 applies to imported names).
    let result = run("fn panic() { }\n");
    assert!(
        result.checked.is_none(),
        "expected the program to be rejected"
    );
    assert_eq!(
        error_codes(&result),
        vec!["E-RES-CONFLICT".to_string()],
        "expected exactly E-RES-CONFLICT, got {}",
        describe(&result)
    );
}

// ===========================================================================
// E-RES-SELF vs E-RES-SELF-OUTSIDE-IMPL
// (T2-FINDINGS §19.2 item 7)
// ===========================================================================

#[test]
fn self_parameter_outside_impl_is_e_res_self() {
    // `diagnostics.md` §3.2 E-RES-SELF: "`self` parameter hors `impl`".
    // grammar.md G-3.7: `self` is the receiver of an `impl` method. This is
    // the site in `resolve_fn_header` (impl_ctx == None).
    expect_error_codes("fn f(self) { }\n", &["E-RES-SELF"]);
}

#[test]
fn self_parameter_not_first_in_impl_is_e_res_self() {
    // grammar.md G-3.7: the receiver is the **first** parameter of an `impl`
    // method; a misplaced receiver is `E-RES-SELF` (`diagnostics.md` §3.2).
    // This is the other reachable site in `resolve_fn_header`
    // (index != 0), reached here with impl_ctx == Some so only one code is
    // emitted.
    expect_error_codes(
        r#"
struct P { pub x: i32 }
impl P { pub fn m(a: i32, &self) { } }
"#,
        &["E-RES-SELF"],
    );
}

#[test]
fn self_parameter_outside_impl_and_not_first_reports_both_sites() {
    // grammar.md G-3.7 + `diagnostics.md` §3.2: a free function with a
    // mis-placed receiver violates both conditions, and the resolver reports
    // each one. This pins that the two sites are distinct diagnostics, not a
    // single one duplicated by accident.
    let result = run("fn f(a: i32, &self) { }\n");
    assert!(
        result.checked.is_none(),
        "expected the program to be rejected"
    );
    assert_eq!(
        error_codes(&result),
        vec!["E-RES-SELF".to_string(), "E-RES-SELF".to_string()],
        "expected two E-RES-SELF diagnostics, got {}",
        describe(&result)
    );
    let text = describe(&result);
    assert!(
        text.contains("must be the first parameter"),
        "missing the position diagnostic: {text}"
    );
    assert!(
        text.contains("only allowed in an `impl` method"),
        "missing the context diagnostic: {text}"
    );
}

#[test]
fn self_parameter_in_extern_is_e_res_self() {
    // grammar.md G-3.11: an `extern "C"` declaration has no receiver;
    // `diagnostics.md` §3.2 E-RES-SELF covers "`self` hors contexte valide".
    expect_error_codes("extern \"C\" { fn f(&self); }\n", &["E-RES-SELF"]);
}

#[test]
fn bare_self_expression_outside_a_method_is_e_res_self() {
    // `diagnostics.md` §3.2 E-RES-SELF: `self` value outside a method.
    // This fifth emitter lives in `typeck::check_self_value`, not in
    // `resolve.rs`; it is observed here because T2-FINDINGS §19.2 item 7 only
    // counts diagnostics, not the file that emits them. The warning
    // `W-OWN-UNUSED-VARIABLE` is filtered by `expect_error_codes`.
    expect_error_codes("fn f() { let x = self; }\n", &["E-RES-SELF"]);
}

#[test]
fn self_type_outside_impl_is_e_res_self_outside_impl() {
    // `diagnostics.md` §3.1 (added code) E-RES-SELF-OUTSIDE-IMPL: `Self` used
    // outside an `impl`. Locked by the corpus case
    // `conformance/cases/resolve/self-type-outside-impl`.
    //
    // Divergence recorded, not resolved here: grammar.md G-5.10 (MUST) names
    // `E-RES-SELF` for the same situation, and diagnostics.md §3.2 lists
    // "`Self` hors impl" under E-RES-SELF as well. The two registry lines
    // describe one case; choosing between them is a spec decision
    // (T2-FINDINGS §19.2 item 7). The test pins the behaviour the corpus
    // already locks rather than silently changing the diagnostic name.
    expect_error_codes(
        "fn f() -> Self { panic(\"x\"); }\n",
        &["E-RES-SELF-OUTSIDE-IMPL"],
    );
}

#[test]
fn self_type_in_impl_is_accepted() {
    // grammar.md G-5.10 (MUST): `Self` is an alias of the `impl` target type
    // inside the `impl`; the negative code must not fire there.
    accept(
        r#"
struct P { pub x: i32 }
impl P {
    pub fn make() -> Self { return P { x: 0 }; }
}
"#,
    );
}

#[test]
fn self_value_path_is_a_parse_error_so_its_resolver_site_is_unreachable() {
    // grammar.md §5 (expr_path = "crate" | "super" | "self" | IDENT { ... }):
    // `Self` is not a value-path root, so the parser rejects `Self::new()`
    // with `E-PARSE-UNEXPECTED` before resolution (`diagnostics.md` DIAG-1.4:
    // an early phase prevents the later phases on the same node).
    //
    // Consequence measured for T2-FINDINGS §19.2 item 7: the `E-RES-SELF`
    // site `PathError::self_outside_impl` in `resolve_value_path` (the
    // `"Self"` arm) cannot be reached from source, because no parsed path can
    // start with a segment named `Self`. This test records the observable
    // behaviour instead of claiming the site is covered.
    let codes = parse_codes("fn f() { let p = Self::new(); }\n");
    assert!(
        codes.iter().any(|code| code == "E-PARSE-UNEXPECTED"),
        "expected the parser to refuse `Self::new()`, got {codes:?}"
    );
}

#[test]
fn self_assoc_type_is_a_parse_error_so_its_resolver_site_is_unreachable() {
    // grammar.md type_segment admits `Self`, but the parser lowers a bare
    // `Self` to `TypeKind::SelfType` and `Self::Assoc` to
    // `E-UNSUPPORTED-ASSOC-TYPE` (`diagnostics.md` §3.1) without building a
    // type path. Therefore the `E-RES-SELF-OUTSIDE-IMPL` branch of
    // `resolve_type_path` (the `Path` starting with `Self`) is unreachable
    // from parsed source; only the `TypeKind::SelfType` branch can fire (see
    // `self_type_outside_impl_is_e_res_self_outside_impl`).
    let codes = parse_codes("fn f() -> Self::Assoc { panic(\"x\"); }\n");
    assert!(
        codes.iter().any(|code| code == "E-UNSUPPORTED-ASSOC-TYPE"),
        "expected the parser to refuse `Self::Assoc`, got {codes:?}"
    );
}

#[test]
fn resolve_entry_point_still_reports_unknown_names() {
    // `diagnostics.md` E-RES-UNKNOWN: this guard keeps the second batch from
    // silently weakening the "unknown name" contract when the visibility
    // logic changed. `resolve` must fail and name the missing item.
    let mut sources = SourceMap::new();
    let file = sources.add("src/lib.rv", "fn f() -> Nope { panic(\"x\"); }\n");
    let src = "fn f() -> Nope { panic(\"x\"); }\n";
    let parsed = parser::parse_file(file, src);
    let mut facts = Facts::default();
    let mut diags = DiagnosticBag::new();
    let ok = ravum_lang_sema::resolve::resolve(&parsed.program, &sources, &mut facts, &mut diags);
    let codes: Vec<String> = diags.as_slice().iter().map(|d| d.code.clone()).collect();
    assert!(!ok, "expected resolution to fail");
    assert!(
        codes.iter().any(|code| code == "E-RES-UNKNOWN"),
        "expected E-RES-UNKNOWN, got {codes:?}"
    );
}
