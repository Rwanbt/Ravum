//! Contract tests for the Ravum name resolver
//! (`crates/ravum-lang-sema/src/resolve.rs`).
//!
//! Every case is a small program plus the outcome that `spec/language/`
//! requires for it: the diagnostic code named by `diagnostics.md` (§3.1-3.4)
//! and the rule cited in the comment above the test (`modules.md` MOD-*,
//! `types.md` TYP-*, `grammar.md` G-*, `ownership.md` OWN-*).
//!
//! A case that the checker rejects for a *different* reason than the expected
//! code is a failure: the tests report every diagnostic, so a drift in another
//! phase cannot masquerade as a pass. Nothing here is allowed to assert
//! "some diagnostic": each expected code is the one the spec names.
//!
//! Run with:
//! `cargo test -p ravum-lang-sema --test resolve_contracts`

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

fn struct_info<'a>(program: &'a CheckedProgram, name: &str) -> &'a ravum_lang_sema::StructInfo {
    program
        .facts
        .structs
        .values()
        .find(|info| info.name == name)
        .unwrap_or_else(|| panic!("no checked struct named `{name}`"))
}

fn enum_info<'a>(program: &'a CheckedProgram, name: &str) -> &'a ravum_lang_sema::EnumInfo {
    program
        .facts
        .enums
        .values()
        .find(|info| info.name == name)
        .unwrap_or_else(|| panic!("no checked enum named `{name}`"))
}

/// Runs the public `resolve::resolve` entry point (without the later phases)
/// and returns whether it succeeded, the emitted codes and the facts.
fn resolve_direct(src: &str) -> (bool, Vec<String>, Facts) {
    let mut sources = SourceMap::new();
    let file = sources.add("src/lib.rv", src);
    let parsed = parser::parse_file(file, src);
    let mut facts = Facts::default();
    let mut diags = DiagnosticBag::new();
    let ok = ravum_lang_sema::resolve::resolve(&parsed.program, &sources, &mut facts, &mut diags);
    let codes = diags.as_slice().iter().map(|d| d.code.clone()).collect();
    (ok, codes, facts)
}

// ===========================================================================
// Public entry point and structural diagnostics
// ===========================================================================

#[test]
fn resolve_entry_point_reports_success_and_fills_tables() {
    // MOD-1.1: a module tree is built from the root; `resolve` is the public
    // resolution entry point and succeeds exactly when no error is reported.
    let (ok, codes, facts) = resolve_direct("pub fn f(x: i32) -> i32 { return x; }");
    assert!(ok, "expected resolution to succeed, got {codes:?}");
    assert!(codes.is_empty(), "unexpected diagnostics: {codes:?}");
    assert_eq!(facts.fn_sigs.len(), 1, "expected one recorded signature");
    let sig = facts.fn_sigs.values().next().expect("one signature");
    assert_eq!(sig.name, "f");
    assert_eq!(sig.rust_path, "crate::f");
    assert_eq!(
        facts.item_paths.get(&sig.item),
        Some(&"crate::f".to_string())
    );
}

#[test]
fn resolve_entry_point_reports_failure_on_unknown_type() {
    // `diagnostics.md` E-RES-UNKNOWN: unknown name. `resolve` must report it
    // and return `false` so `check` never runs the later phases.
    let (ok, codes, _facts) = resolve_direct("pub fn f(x: Nope) { }");
    assert!(!ok, "expected resolution to fail");
    assert!(
        codes.iter().any(|code| code == "E-RES-UNKNOWN"),
        "expected E-RES-UNKNOWN, got {codes:?}"
    );
}

#[test]
fn resolve_entry_point_rejects_pub_use_reexport() {
    // MOD-3.3: `pub use` (re-export) is refused. The parser refuses it too,
    // but `resolve` is a public entry point and must not silently accept a
    // re-export when it is handed the program.
    let (ok, codes, _facts) = resolve_direct("mod geo { pub fn f() { } }\npub use crate::geo;\n");
    assert!(!ok, "expected resolution to fail");
    assert!(
        codes.iter().any(|code| code == "E-UNSUPPORTED-REEXPORT"),
        "expected E-UNSUPPORTED-REEXPORT, got {codes:?}"
    );
}

#[test]
fn mod_decl_is_e_res_moddecl() {
    // MOD-1.3 / `diagnostics.md` E-RES-MODDECL: `mod nom;` must be expanded by
    // the loader; the checker never reads the disk.
    expect_code("mod detail;\n", "E-RES-MODDECL");
}

#[test]
fn duplicate_type_name_is_e_res_duplicate() {
    // MOD-1.5 + `diagnostics.md` E-RES-DUPLICATE: two types of the same name in
    // one module.
    expect_code(
        "struct A { x: i32 }\nstruct A { y: i32 }\n",
        "E-RES-DUPLICATE",
    );
}

#[test]
fn duplicate_value_name_is_e_res_duplicate() {
    // MOD-1.5: two values (functions) of the same name in one module.
    expect_code("fn f() { }\nfn f() { }\n", "E-RES-DUPLICATE");
}

#[test]
fn duplicate_module_name_is_e_res_duplicate() {
    // MOD-1.5: the module namespace is checked independently (a module named
    // twice is a duplicate even when its contents differ).
    expect_code("mod m { }\nmod m { }\n", "E-RES-DUPLICATE");
}

#[test]
fn prelude_type_name_is_e_res_conflict() {
    // `diagnostics.md` E-RES-CONFLICT: prelude names are reserved; redefining
    // the type `String` collides with the prelude.
    expect_code("struct String { x: i32 }\n", "E-RES-CONFLICT");
}

#[test]
fn prelude_value_name_is_e_res_conflict() {
    // E-RES-CONFLICT in the value namespace: `panic` is a prelude value.
    expect_code("fn panic() { }\n", "E-RES-CONFLICT");
}

#[test]
fn prelude_module_name_is_e_res_conflict() {
    // E-RES-CONFLICT in the module namespace: `io` is a prelude module
    // (PRELUDE_MODULES).
    expect_code("mod io { }\n", "E-RES-CONFLICT");
}

// ===========================================================================
// `use`, imports and visibility
// ===========================================================================

#[test]
fn import_of_public_items_is_accepted() {
    // MOD-3.2: `use crate::a::B;` imports one public name; MOD-4.2: a `pub`
    // item of a `pub` module is visible from anywhere in the crate.
    let program = accept(
        r#"
mod geo {
    pub struct Point { pub x: i32 }
    pub fn make(x: i32) -> Point { return Point { x: x }; }
}
mod site {
    use crate::geo::Point;
    use crate::geo::make;
    pub fn via_import(x: i32) -> Point { return make(x); }
}
"#,
    );
    // Both imports are consumed, so no `W-RES-UNUSED-IMPORT`.
    assert!(
        program.facts.structs.values().any(|s| s.name == "Point"),
        "Point must be registered"
    );
}

#[test]
fn import_of_private_item_is_e_res_private() {
    // MOD-3.9: `use` cannot import a private item of another module.
    expect_code(
        r#"
mod geo { fn cache() -> i32 { return 0; } }
mod site { use crate::geo::cache; }
"#,
        "E-RES-PRIVATE",
    );
}

#[test]
fn import_of_private_module_is_e_res_private() {
    // MOD-3.9 / MOD-4.3: a private module of a sibling module is not importable.
    expect_code(
        r#"
mod geo { mod hidden { pub fn f() { } } }
mod site { use crate::geo::hidden; }
"#,
        "E-RES-PRIVATE",
    );
}

#[test]
fn import_of_module_is_accepted() {
    // MOD-3.1: `use crate::geo;` imports the module name; MOD-6.7: the
    // qualified form `geo::f()` is the normative call syntax. MOD-4.2: the
    // module must be `pub` to cross a module boundary.
    accept(
        r#"
pub mod geo { pub fn f() -> i32 { return 1; } }
mod site {
    use crate::geo;
    pub fn g() -> i32 { return geo::f(); }
}
"#,
    );
}

#[test]
fn import_of_builtin_module_is_accepted() {
    // MOD-3.2: `use std::io;` is a admitted form (stdlib module path).
    accept(
        r#"
use std::io;

fn announce() { io::println("ok"); }
"#,
    );
}

#[test]
fn import_of_builtin_function_is_accepted() {
    // MOD-3.1: `use std::io::println;` imports the prelude function name.
    accept(
        r#"
use std::io::println;

fn announce() { println("ok"); }
"#,
    );
}

#[test]
fn import_of_unknown_path_is_e_res_unknown() {
    // `diagnostics.md` E-RES-UNKNOWN: unknown import path.
    expect_code("use crate::nowhere::Thing;\n", "E-RES-UNKNOWN");
}

#[test]
fn import_of_user_enum_variant_is_e_res_unknown() {
    // MOD-3.5: user enum variants are never individually importable.
    expect_code(
        "enum Shape { Circle }\nuse Shape::Circle;\n",
        "E-RES-UNKNOWN",
    );
}

#[test]
fn import_conflicting_with_declared_type_is_e_res_duplicate() {
    // MOD-3.7: an imported name may not collide with an item declared in the
    // same module.
    expect_code(
        "struct Point { pub x: i32 }\nuse Point;\n",
        "E-RES-DUPLICATE",
    );
}

#[test]
fn import_conflicting_with_declared_value_is_e_res_duplicate() {
    // MOD-3.7, value namespace: the local `fn f` occupies the name.
    expect_code("fn f() { }\nuse f;\n", "E-RES-DUPLICATE");
}

#[test]
fn import_conflicting_with_declared_module_is_e_res_duplicate() {
    // MOD-3.7, module namespace: the local inline module occupies the name
    // (it is `pub` so the privacy check of MOD-4.2 cannot mask the duplicate).
    expect_code("pub mod geo { }\nuse geo;\n", "E-RES-DUPLICATE");
}

#[test]
fn duplicate_import_of_same_symbol_is_e_res_duplicate() {
    // MOD-3.7: two `use` importing the same name are E-RES-DUPLICATE.
    expect_code(
        r#"
mod a { pub struct Thing { pub x: i32 } }
mod b { pub struct Thing { pub y: i32 } }
use crate::a::Thing;
use crate::b::Thing;
"#,
        "E-RES-DUPLICATE",
    );
}

#[test]
fn reimport_of_builtin_module_is_e_res_duplicate() {
    // MOD-3.7: `use io;` after `use std::io;` imports the same name twice.
    expect_code("use std::io;\nuse io;\n", "E-RES-DUPLICATE");
}

#[test]
fn reimport_of_builtin_function_is_e_res_duplicate() {
    // MOD-3.7: same rule in the value namespace.
    expect_code("use std::io::println;\nuse println;\n", "E-RES-DUPLICATE");
}

#[test]
fn unused_import_is_w_res_unused_import() {
    // MOD-3.4: an unused import is a warning (`W-RES-UNUSED-IMPORT`), never an
    // error.
    let result = run("use std::math;\nfn f() { }\n");
    assert!(
        result.checked.is_some(),
        "unused import must not fail: {}",
        describe(&result)
    );
    assert!(
        codes(&result)
            .iter()
            .any(|code| code == "W-RES-UNUSED-IMPORT"),
        "expected W-RES-UNUSED-IMPORT, got {:?}",
        codes(&result)
    );
}

// ===========================================================================
// Generic scopes and function headers
// ===========================================================================

#[test]
fn duplicate_generic_parameter_is_e_res_duplicate() {
    // TYP-11.4 + `diagnostics.md` E-RES-DUPLICATE: two generic parameters of
    // the same name.
    expect_code("fn f<T, T>(x: T) { }\n", "E-RES-DUPLICATE");
}

#[test]
fn impl_generic_scope_is_used_for_method_headers() {
    // TYP-11.5: a method of `impl<T> Boite<T>` uses the `impl` parameters; the
    // recorded signature must carry `["T"]`.
    let program = accept(
        r#"
struct Boite<T> { pub valeur: T }

impl<T> Boite<T> {
    pub fn emballer(v: T) -> Boite<T> { return Boite { valeur: v }; }
}
"#,
    );
    let sig = program
        .facts
        .fn_sigs
        .values()
        .find(|sig| sig.name == "emballer")
        .expect("method signature");
    assert_eq!(sig.generics, vec!["T".to_string()]);
    assert_eq!(sig.params.len(), 1);
}

#[test]
fn method_with_own_generic_parameter_is_e_typ_arity() {
    // TYP-11.5: a method cannot add generic parameters not declared by the
    // `impl` (`E-TYP-ARITY`).
    expect_code(
        r#"
struct P { pub x: i32 }
impl P { pub fn m<U>(&self) { } }
"#,
        "E-TYP-ARITY",
    );
}

#[test]
fn self_parameter_not_first_is_e_res_self() {
    // G-3.7: `self` is the first parameter of an `impl` method; a misplaced
    // receiver is `E-RES-SELF`.
    expect_code(
        r#"
struct P { pub x: i32 }
impl P { pub fn m(a: i32, self) { } }
"#,
        "E-RES-SELF",
    );
}

#[test]
fn duplicate_self_parameter_is_e_res_duplicate() {
    // G-3.7: a method carries a single receiver (`E-RES-DUPLICATE`).
    expect_code(
        r#"
struct P { pub x: i32 }
impl P { pub fn m(&self, &mut self) { } }
"#,
        "E-RES-DUPLICATE",
    );
}

#[test]
fn never_parameter_type_is_e_typ_mismatch() {
    // G-5.11 / TYP-1.6: `!` is not a valid parameter type.
    expect_code("fn f(x: !) { }\n", "E-TYP-MISMATCH");
}

#[test]
fn never_compound_return_type_is_e_typ_mismatch() {
    // G-5.11: `!` is only legal as the whole return type of a divergent
    // function; `Vec<!>` is not inhabitable.
    expect_code("fn f() -> Vec<!> { panic(\"boom\"); }\n", "E-TYP-MISMATCH");
}

// ===========================================================================
// `extern "C"` declarations
// ===========================================================================

#[test]
fn extern_duplicate_parameter_is_e_res_duplicate() {
    // G-3.11 + `diagnostics.md` E-RES-DUPLICATE: duplicate `extern` parameter.
    expect_code(
        "extern \"C\" { fn f(x: i32, x: i32); }\n",
        "E-RES-DUPLICATE",
    );
}

#[test]
fn extern_self_parameter_is_e_res_self() {
    // G-3.11: an `extern` declaration has no receiver (`E-RES-SELF`).
    expect_code("extern \"C\" { fn f(&self); }\n", "E-RES-SELF");
}

#[test]
fn extern_return_outside_ffi_safe_is_e_ffi_type() {
    // G-3.10 / `diagnostics.md` E-FFI-TYPE: `extern` returns must be FFI-safe.
    expect_code("extern \"C\" { fn f() -> String; }\n", "E-FFI-TYPE");
}

#[test]
fn extern_signature_is_recorded() {
    // G-3.10: `extern "C" { fn abs(...); }` is accepted and the resolver
    // records the C symbol and the FFI signature for the backend.
    let program = accept(
        r#"
extern "C" {
    fn abs(x: i32) -> i32;
}

fn call_abs(x: i32) -> i32 effects(unsafe, ffi) {
    unsafe { abs(x) }
}
"#,
    );
    assert!(
        program
            .facts
            .extern_symbols
            .values()
            .any(|symbol| symbol == "abs"),
        "the C symbol `abs` must be recorded"
    );
    assert!(
        program.facts.fn_sigs.values().any(|sig| sig.is_extern),
        "the extern signature must be recorded"
    );
}

// ===========================================================================
// Structs, enums and structural `Copy`
// ===========================================================================

#[test]
fn struct_field_never_type_is_e_typ_mismatch() {
    // G-5.11: `!` is not a valid field type.
    expect_code("struct S { x: ! }\n", "E-TYP-MISMATCH");
}

#[test]
fn enum_tuple_variant_never_type_is_e_typ_mismatch() {
    // G-5.11: `!` is not a valid tuple-variant field type.
    expect_code("enum E { V(!) }\n", "E-TYP-MISMATCH");
}

#[test]
fn enum_struct_variant_never_type_is_e_typ_mismatch() {
    // G-5.11: `!` is not a valid struct-variant field type.
    expect_code("enum E { V { x: ! } }\n", "E-TYP-MISMATCH");
}

#[test]
fn recursive_struct_is_e_typ_recursive() {
    // `diagnostics.md` E-TYP-RECURSIVE: an infinite type without indirection.
    expect_code("struct Node { next: Node }\n", "E-TYP-RECURSIVE");
}

#[test]
fn recursive_enum_is_e_typ_recursive() {
    // E-TYP-RECURSIVE through an enum variant (no `Box` in Core 1.0).
    expect_code("enum Loop2 { Next(Loop2) }\n", "E-TYP-RECURSIVE");
}

#[test]
fn structural_copy_is_computed_from_fields() {
    // TYP-14.1: aggregates whose fields are all `Copy` are `Copy`;
    // TYP-14.2: `Error` and aggregates holding a mobile field are mobile.
    let program = accept(
        r#"
struct Copyish {
    arr: [i32; 2],
    tup: (i32, bool),
    opt: Option<i32>,
    res: Result<i32, char>,
}

struct Holder {
    arr: [i32; 2],
    tup: (i32, bool),
    opt: Option<i32>,
    res: Result<i32, char>,
    err: Error,
}
"#,
    );
    assert!(
        struct_info(&program, "Copyish").is_copy,
        "Copyish holds only Copy fields and must be Copy"
    );
    assert!(
        !struct_info(&program, "Holder").is_copy,
        "Holder contains `Error`, which TYP-14.2 declares mobile"
    );
}

#[test]
fn generic_enum_signature_is_recorded() {
    // G-3.5 / TYP-11.4: generic parameters of an enum are recorded in order
    // and its arguments are resolved positionally.
    let program = accept(
        r#"
enum Outcome<T, E> { Yes(T), No(E) }

fn pass(o: Outcome<i32, bool>) -> Outcome<i32, bool> { return o; }
"#,
    );
    let info = enum_info(&program, "Outcome");
    assert_eq!(info.generics, vec!["T".to_string(), "E".to_string()]);
    assert_eq!(info.variants.len(), 2);
}

// ===========================================================================
// `impl` blocks
// ===========================================================================

#[test]
fn impl_of_foreign_type_is_e_res_impl_self_type() {
    // G-4.5: `impl i32` targets a foreign type (`E-RES-IMPL-SELF-TYPE`).
    expect_code(
        "impl i32 { fn double(&self) -> i32 { return *self; } }\n",
        "E-RES-IMPL-SELF-TYPE",
    );
}

#[test]
fn impl_generic_arguments_mismatch_is_e_res_impl_target() {
    // G-4.6 / TYP-11.5: the `impl` parameters must match the target type
    // arguments in order (`E-RES-IMPL-TARGET`).
    expect_code(
        r#"
struct Boite<T> { pub v: T }
impl<T> Boite<i32> { pub fn get(&self) -> i32 { return 1; } }
"#,
        "E-RES-IMPL-TARGET",
    );
}

#[test]
fn non_function_impl_member_is_e_res_impl_unknown() {
    // G-4.7: only `fn` items are allowed in an `impl`
    // (`E-RES-IMPL-UNKNOWN`).
    expect_code(
        r#"
struct P { pub x: i32 }
impl P { const N: i32 = 1; }
"#,
        "E-RES-IMPL-UNKNOWN",
    );
}

#[test]
fn duplicate_method_name_is_e_res_impl_duplicate() {
    // `diagnostics.md` E-RES-IMPL-DUPLICATE: two methods of the same name for
    // one type.
    expect_code(
        r#"
struct P { pub x: i32 }
impl P {
    pub fn m(&self) -> i32 { return self.x; }
    pub fn m(&self) -> i32 { return self.x; }
}
"#,
        "E-RES-IMPL-DUPLICATE",
    );
}

#[test]
fn associated_function_resolution_is_accepted() {
    // G-4.6 + TYP-3: `P::new(..)` resolves to the associated function and
    // `p.get()` to the receiver method.
    accept(
        r#"
struct P { pub x: i32 }

impl P {
    pub fn new(x: i32) -> P { return P { x: x }; }
    pub fn get(&self) -> i32 { return self.x; }
}

fn use_both() -> i32 {
    let p = P::new(3);
    return p.get();
}
"#,
    );
}

#[test]
fn unknown_associated_function_on_primitive_is_e_res_unknown() {
    // `diagnostics.md` E-RES-UNKNOWN: a method/associated name that is not in
    // the builtin table and not declared by an `impl` is unknown.
    expect_code("fn f() { let x = i32::max(1, 2); }\n", "E-RES-UNKNOWN");
}

// ===========================================================================
// Type and value path resolution across modules
// ===========================================================================

#[test]
fn cross_module_type_paths_are_accepted() {
    // MOD-3.6: `crate` is the crate root; MOD-2.x: module paths traverse the
    // module tree; a struct literal names a type through the same path.
    accept(
        r#"
mod geo {
    pub struct Point { pub x: i32 }
    pub mod inner { pub struct Deep { pub y: i32 } }
    pub fn make(x: i32) -> Point { return Point { x: x }; }
}

mod site {
    use crate::geo::Point;

    pub fn via_crate() -> crate::geo::Point { return crate::geo::make(1); }
    pub fn via_nested() -> crate::geo::inner::Deep {
        return crate::geo::inner::Deep { y: 2 };
    }
    pub fn via_import() -> Point { return Point { x: 3 }; }
}
"#,
    );
}

#[test]
fn self_and_super_type_paths_are_accepted() {
    // MOD-3.6: `self` is the current module, `super` the parent module; both
    // are valid type-path roots.
    accept(
        r#"
mod outer {
    pub struct B { pub y: i32 }
    pub mod inner {
        pub struct A { pub x: i32 }
        pub fn via_self(a: self::A) -> self::A { return a; }
        pub fn via_super(b: super::B) -> super::B { return b; }
    }
}
"#,
    );
}

#[test]
fn crate_super_self_value_paths_are_accepted() {
    // G-6.45: `crate::`, `super::` and `self::` are valid expression path
    // roots.
    accept(
        r#"
mod outer {
    pub fn top() -> i32 { return 1; }
    pub mod inner {
        pub fn sibling() -> i32 { return 2; }
        pub fn g() -> i32 { return super::top() + self::sibling(); }
    }
}

fn h() -> i32 { return crate::outer::inner::sibling(); }
"#,
    );
}

#[test]
fn super_in_value_path_at_crate_root_is_e_res_unknown() {
    // MOD-3.6: `super` at the crate root is `E-RES-UNKNOWN`.
    expect_code("fn f() -> i32 { return super::g(); }\n", "E-RES-UNKNOWN");
}

#[test]
fn super_in_type_path_at_crate_root_is_e_res_unknown() {
    // MOD-3.6: same rule for a type path.
    expect_code("fn f(x: super::Thing) { }\n", "E-RES-UNKNOWN");
}

#[test]
fn type_used_as_module_prefix_is_e_res_unknown() {
    // MOD-1.x: only modules are path prefixes; a type prefix is
    // `E-RES-UNKNOWN`.
    expect_code(
        "struct P { pub x: i32 }\nfn f(v: P::Inner) { }\n",
        "E-RES-UNKNOWN",
    );
}

#[test]
fn unknown_root_in_type_path_is_e_res_unknown() {
    // E-RES-UNKNOWN: unknown first segment of a type path.
    expect_code("fn f(v: Nope::Thing) { }\n", "E-RES-UNKNOWN");
}

#[test]
fn unknown_module_in_type_path_is_e_res_unknown() {
    // E-RES-UNKNOWN: unknown intermediate module.
    expect_code(
        "mod m { pub mod inner { pub struct A { pub z: i32 } } }\nfn f(v: crate::m::Nope::A) { }\n",
        "E-RES-UNKNOWN",
    );
}

#[test]
fn unknown_type_in_type_path_is_e_res_unknown() {
    // E-RES-UNKNOWN: unknown last segment of a type path.
    expect_code("mod m { }\nfn f(v: crate::m::Nope) { }\n", "E-RES-UNKNOWN");
}

#[test]
fn module_in_type_position_is_e_res_unknown() {
    // MOD-1.x: a path that ends on a module is not a type.
    expect_code(
        "mod m { pub mod inner { } }\nfn f(v: crate::m::inner) { }\n",
        "E-RES-UNKNOWN",
    );
}

#[test]
fn module_name_as_type_is_e_res_unknown() {
    // The single-segment form of the same rule: a module name is not a type.
    expect_code("mod m { }\nfn f(v: m) { }\n", "E-RES-UNKNOWN");
}

#[test]
fn builtin_module_in_type_path_is_e_res_unknown() {
    // stdlib.md: `std::io` is a module, never a type; `std::io::Foo` is
    // E-RES-UNKNOWN.
    expect_code("fn f(v: std::io::Foo) { }\n", "E-RES-UNKNOWN");
}

#[test]
fn private_type_in_type_path_is_e_res_private() {
    // MOD-4.8: a private type is not nameable outside its defining subtree.
    expect_code(
        r#"
mod a { struct Hidden { pub x: i32 } }
mod b { pub fn f(v: crate::a::Hidden) { } }
"#,
        "E-RES-PRIVATE",
    );
}

#[test]
fn private_module_in_type_path_is_e_res_private() {
    // MOD-4.3: a parent module sees but a sibling does not see a private
    // module; traversing it is `E-RES-PRIVATE`.
    expect_code(
        r#"
mod a { mod secret { pub struct Deep { pub y: i32 } } }
mod b { pub fn f(v: crate::a::secret::Deep) { } }
"#,
        "E-RES-PRIVATE",
    );
}

#[test]
fn private_type_in_value_path_is_e_res_private() {
    // MOD-4.2: a private type used in an expression path from outside its
    // subtree is `E-RES-PRIVATE`.
    expect_code(
        r#"
mod a { struct Hidden { pub x: i32 } }
fn f() { let h = crate::a::Hidden { x: 1 }; }
"#,
        "E-RES-PRIVATE",
    );
}

#[test]
fn private_module_in_value_path_is_e_res_private() {
    // MOD-4.3: same rule for a module traversed in an expression path.
    expect_code(
        r#"
mod a { mod hidden { pub fn f() -> i32 { return 1; } } }
fn g() -> i32 { return crate::a::hidden::f(); }
"#,
        "E-RES-PRIVATE",
    );
}

#[test]
fn private_function_in_value_path_is_e_res_private() {
    // MOD-4.3: a private function is not callable from another module.
    expect_code(
        r#"
mod a { fn hidden() -> i32 { return 1; } }
fn g() -> i32 { return crate::a::hidden(); }
"#,
        "E-RES-PRIVATE",
    );
}

#[test]
fn generic_parameter_with_type_arguments_is_e_typ_arity() {
    // TYP-11.4: a generic parameter takes no arguments.
    expect_code("fn f<T>(x: T<i32>) { }\n", "E-TYP-ARITY");
}

#[test]
fn str_alone_in_type_position_is_e_res_unknown() {
    // TYP-6.5: `str` alone is not a type (`E-RES-UNKNOWN`).
    expect_code("fn f(s: str) { }\n", "E-RES-UNKNOWN");
}

#[test]
fn mut_str_reference_is_e_res_unknown() {
    // TYP-6.5: `&mut str` does not exist in Core 1.0.
    expect_code("fn f(s: &mut str) { }\n", "E-RES-UNKNOWN");
}

#[test]
fn underscore_in_type_position_is_e_typ_ambiguous() {
    // G-5.x + `diagnostics.md` E-TYP-AMBIGUOUS: `_` is not allowed in a type
    // position.
    expect_code("fn f(x: _) { }\n", "E-TYP-AMBIGUOUS");
}

#[test]
fn self_type_outside_impl_is_e_res_self_outside_impl() {
    // G-5.10 / `diagnostics.md` E-RES-SELF-OUTSIDE-IMPL: `Self` outside an
    // `impl`.
    expect_code(
        "fn f() -> Self { panic(\"x\"); }\n",
        "E-RES-SELF-OUTSIDE-IMPL",
    );
}

#[test]
fn self_type_in_impl_is_accepted() {
    // G-5.10: `Self` is an alias of the `impl` target type.
    accept(
        r#"
struct P { pub x: i32 }
impl P {
    pub fn make(x: i32) -> Self { return P { x: x }; }
}
"#,
    );
}

#[test]
fn array_length_zero_is_e_typ_array_repeat() {
    // G-5.5: `[T; 0]` is forbidden (`E-TYP-ARRAY-REPEAT`).
    expect_code("fn f(x: [i32; 0]) { }\n", "E-TYP-ARRAY-REPEAT");
}

#[test]
fn array_length_overflow_is_e_typ_array_repeat() {
    // G-5.5: the length must fit in `usize` (`E-TYP-ARRAY-REPEAT`).
    expect_code(
        "fn f(x: [i32; 18446744073709551616]) { }\n",
        "E-TYP-ARRAY-REPEAT",
    );
}

#[test]
fn array_length_non_literal_is_e_typ_array_repeat() {
    // G-5.5: the length must be a decimal integer literal (`E-TYP-ARRAY-REPEAT`).
    expect_code("fn f(x: [i32; N]) { }\n", "E-TYP-ARRAY-REPEAT");
}

#[test]
fn prelude_vec_arity_is_e_typ_arity() {
    // TYP-9.1 / TYP-11.4: `Vec` takes exactly one argument (`E-TYP-ARITY`).
    expect_code("fn f(v: Vec) { }\n", "E-TYP-ARITY");
}

#[test]
fn prelude_option_arity_is_e_typ_arity() {
    // TYP-10.3: `Option` takes exactly one argument (`E-TYP-ARITY`).
    expect_code("fn f(v: Option<i32, i32>) { }\n", "E-TYP-ARITY");
}

#[test]
fn primitive_with_type_arguments_is_e_typ_arity() {
    // G-5.x: primitive types take no generic arguments (`E-TYP-ARITY`).
    expect_code("fn f(v: i32<i64>) { }\n", "E-TYP-ARITY");
}

#[test]
fn nominal_type_arity_mismatch_is_e_typ_arity() {
    // TYP-11.4: a user generic type requires exactly its declared arity.
    expect_code(
        r#"
struct Boite<T> { pub v: T }
fn f(b: Boite) { }
"#,
        "E-TYP-ARITY",
    );
}

#[test]
fn unqualified_user_enum_variant_is_e_res_unknown() {
    // TYP-10.2: user enum variants must stay qualified; an unqualified one is
    // E-RES-UNKNOWN.
    expect_code(
        r#"
enum Couleur { Rouge }
fn f() { let c = Rouge; }
"#,
        "E-RES-UNKNOWN",
    );
}

#[test]
fn unknown_value_name_is_e_res_unknown() {
    // G-6.46: there is no implicit import; an undeclared name is unknown.
    expect_code("fn f() { let x = Nope; }\n", "E-RES-UNKNOWN");
}

#[test]
fn generic_parameter_as_value_is_e_res_unknown() {
    // TYP-11.1: a type parameter is not a value (E-RES-UNKNOWN).
    expect_code("fn f<T>(x: T) { let y = T; }\n", "E-RES-UNKNOWN");
}

#[test]
fn explicit_generic_arguments_in_expression_is_e_parse_unexpected() {
    // TYP-11.4: `f::<i32>(1)` is refused (the `types.md` example names
    // E-PARSE-UNEXPECTED).
    expect_code(
        r#"
fn id<T>(x: T) -> T { return x; }
fn g() { let y = id::<i32>(1); }
"#,
        "E-PARSE-UNEXPECTED",
    );
}

#[test]
fn unknown_member_of_module_in_value_path_is_e_res_unknown() {
    // MOD-6.7: only names actually declared by the module resolve;
    // `crate::a::nope` is E-RES-UNKNOWN.
    expect_code(
        "mod a { pub fn f() { } }\nfn g() { crate::a::nope(); }\n",
        "E-RES-UNKNOWN",
    );
}

#[test]
fn type_member_must_be_associated_item_is_e_res_unknown() {
    // TYP-14.4 / `diagnostics.md` E-RES-UNKNOWN: a type is not traversable as a
    // module (`P::new::extra`).
    expect_code(
        r#"
struct P { pub x: i32 }
impl P { pub fn new() -> P { return P { x: 1 }; } }
fn g() { let q = P::new::extra; }
"#,
        "E-RES-UNKNOWN",
    );
}

#[test]
fn module_used_as_constructor_is_e_typ_call() {
    // `diagnostics.md` E-TYP-CALL: a module is not a function value.
    expect_code(
        "mod geo { pub fn f() { } }\nfn g() { crate::geo(); }\n",
        "E-TYP-CALL",
    );
}

#[test]
fn builtin_module_used_as_constructor_is_e_typ_call() {
    // E-TYP-CALL: same rule for a stdlib module (`std::io`).
    expect_code("fn g() { std::io(); }\n", "E-TYP-CALL");
}

#[test]
fn nested_module_used_as_value_is_e_typ_call() {
    // E-TYP-CALL: a nested module name is not a value.
    expect_code(
        "mod a { pub mod b { pub fn f() { } } }\nfn g() { crate::a::b(); }\n",
        "E-TYP-CALL",
    );
}

#[test]
fn builtin_unknown_function_is_e_res_unknown() {
    // stdlib.md: the builtin table is closed; `std::io::nope` is unknown.
    expect_code("fn g() { std::io::nope(); }\n", "E-RES-UNKNOWN");
}

#[test]
fn builtin_unknown_child_module_is_e_res_unknown() {
    // stdlib.md: the builtin module table is closed; `std::io::nope::println`
    // is unknown.
    expect_code("fn g() { std::io::nope::println(); }\n", "E-RES-UNKNOWN");
}

#[test]
fn builtin_path_call_is_accepted() {
    // stdlib.md: `std::io::println` is the canonical builtin function path.
    accept("fn announce() { std::io::println(\"ok\"); }\n");
}

#[test]
fn result_err_constructor_is_accepted() {
    // TYP-10.2: `Result::Err` is a builtin constructor of the prelude.
    accept("fn fail() -> Result<i32, i32> { return Result::Err(1); }\n");
}

// ===========================================================================
// Residual reachable branches (name mangling, impl targets, path cursors)
// ===========================================================================

#[test]
fn impl_of_prelude_error_type_is_e_res_impl_self_type() {
    // G-4.5: `impl` target must be a nominal type *defined in this package*;
    // the prelude `Error` is foreign (`E-RES-IMPL-SELF-TYPE`).
    expect_code(
        "impl Error { pub fn m(&self) { } }\n",
        "E-RES-IMPL-SELF-TYPE",
    );
}

#[test]
fn rust_keyword_identifier_is_mangled() {
    // G-5.8: `dyn` is a valid Ravum identifier; the Rust path generated by the
    // resolver must be a valid Rust identifier (`r#dyn`).
    let program = accept(
        r#"
fn dyn() -> i32 { return 1; }
struct S { pub dyn: i32 }
"#,
    );
    let sig = program
        .facts
        .fn_sigs
        .values()
        .find(|sig| sig.name == "dyn")
        .expect("signature of `dyn`");
    assert_eq!(sig.rust_path, "crate::r#dyn");
    let field = struct_info(&program, "S")
        .fields
        .iter()
        .find(|field| field.name == "dyn")
        .expect("field `dyn`");
    assert_eq!(field.rust_name, "r#dyn");
}

#[test]
fn private_type_as_path_prefix_is_e_res_private() {
    // MOD-4.3: a private type is not nameable outside its defining subtree,
    // even when only used as the prefix of an associated function.
    expect_code(
        r#"
mod a {
    struct Hidden { pub x: i32 }
    impl Hidden { pub fn new() -> Hidden { return Hidden { x: 1 }; } }
}
fn f() { let h = crate::a::Hidden::new(); }
"#,
        "E-RES-PRIVATE",
    );
}

#[test]
fn unknown_member_in_middle_of_value_path_is_e_res_unknown() {
    // E-RES-UNKNOWN: unknown module segment in the middle of a value path.
    expect_code(
        "mod a { pub fn f() { } }\nfn g() { crate::a::nope::f(); }\n",
        "E-RES-UNKNOWN",
    );
}

#[test]
fn root_path_to_imported_builtin_function_is_accepted() {
    // MOD-3.6: `crate` is the root module; a name imported there (MOD-3.1) is
    // reachable through the root path.
    accept(
        r#"
use std::io::println;

fn announce() { crate::println("ok"); }
"#,
    );
}

#[test]
fn root_path_to_imported_builtin_module_is_e_typ_call() {
    // E-TYP-CALL: a module is not a function value; the root path also names
    // modules imported at the root.
    expect_code("use std::io;\nfn f() { crate::io(); }\n", "E-TYP-CALL");
}

#[test]
fn enum_associated_function_is_accepted() {
    // G-4.6: an `impl` on an enum declares associated functions; a variant
    // lookup that misses falls back to the associated-function table.
    accept(
        r#"
enum E { A }

impl E { pub fn make() -> E { return E::A; } }

fn f() -> E { return E::make(); }
"#,
    );
}

#[test]
fn use_self_qualified_import_is_accepted() {
    // MOD-3.2 / MOD-3.6: `use self::...` is relative to the current module.
    accept(
        r#"
mod a {
    pub mod sub { pub struct Item { pub x: i32 } }
    use self::sub::Item;
    pub fn make() -> Item { return Item { x: 1 }; }
}
"#,
    );
}

#[test]
fn use_super_qualified_import_is_accepted() {
    // MOD-3.2 / MOD-3.6: `use super::helper;` imports a name of the parent.
    accept(
        r#"
mod a {
    pub fn helper() -> i32 { return 1; }
    pub mod b {
        use super::helper;
        pub fn g() -> i32 { return helper(); }
    }
}
"#,
    );
}

#[test]
fn use_super_at_crate_root_is_e_res_unknown() {
    // MOD-3.6: `super` at the crate root is E-RES-UNKNOWN.
    expect_code("use super::x;\n", "E-RES-UNKNOWN");
}

#[test]
fn use_relative_module_path_is_accepted() {
    // MOD-3.2: a naked path (`use geo::f;`) resolves relative to the current
    // module.
    accept(
        r#"
mod a {
    pub mod geo { pub fn f() -> i32 { return 1; } }
    use geo::f;
    pub fn g() -> i32 { return f(); }
}
"#,
    );
}

#[test]
fn use_unknown_member_of_module_is_e_res_unknown() {
    // E-RES-UNKNOWN: the last segment of a `use` path names nothing.
    expect_code(
        "mod geo { pub fn f() { } }\nuse geo::nope;\n",
        "E-RES-UNKNOWN",
    );
}

#[test]
fn use_unknown_builtin_child_is_e_res_unknown() {
    // stdlib.md: the builtin module table is closed (`std::io::nope` is not a
    // module).
    expect_code("use std::io::nope::println;\n", "E-RES-UNKNOWN");
}

#[test]
fn use_self_qualified_reimport_of_builtin_module_is_e_res_duplicate() {
    // MOD-3.7: `use self::io;` names the same imported module a second time.
    expect_code("use std::io;\nuse self::io;\n", "E-RES-DUPLICATE");
}

#[test]
fn shared_recursion_is_reported_once() {
    // E-TYP-RECURSIVE: the same in-progress type reached through two fields
    // must not duplicate the diagnostic, and the program is still rejected.
    expect_code(
        r#"
struct A { x: B, y: C }
struct B { a: A }
struct C { a: A }
"#,
        "E-TYP-RECURSIVE",
    );
}

#[test]
fn unit_struct_is_copy() {
    // TYP-14.1: a struct whose fields are all `Copy` is `Copy`; with no field
    // the condition holds vacuously.
    let program = accept("struct Unit { }\n");
    assert!(
        struct_info(&program, "Unit").is_copy,
        "a fieldless struct is structurally Copy"
    );
}

#[test]
fn naked_module_type_path_is_accepted() {
    // G-5.9: a type path may traverse modules from the current module without
    // the `crate::` prefix.
    accept(
        r#"
mod geo { pub struct Point { pub x: i32 } }

fn take(p: geo::Point) { }
"#,
    );
}

#[test]
fn const_through_module_path_is_accepted() {
    // MOD-3.1: a module path may name a constant in its value namespace.
    accept(
        r#"
mod a { pub const C: i32 = 1; }

fn f() -> i32 { return crate::a::C; }
"#,
    );
}

#[test]
fn private_module_as_last_value_path_is_e_res_private() {
    // MOD-4.3: a private module is not nameable from a non-descendant module,
    // including when it is the last segment of an expression path.
    expect_code(
        r#"
mod a { mod secret { pub fn f() -> i32 { return 1; } } }
fn g() { crate::a::secret(); }
"#,
        "E-RES-PRIVATE",
    );
}

#[test]
fn imported_builtin_module_in_type_path_is_e_res_unknown() {
    // stdlib.md: `io` imported at the root is a module; `io::Foo` used in a
    // type position is E-RES-UNKNOWN.
    expect_code("use std::io;\nfn f(x: io::Foo) { }\n", "E-RES-UNKNOWN");
}
