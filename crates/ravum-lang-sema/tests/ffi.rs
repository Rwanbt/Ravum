//! FFI boundary tests: `extern "C"` declarations, `unsafe` contexts and the
//! profile interaction. These tests exercise the real loader + checker on
//! temporary packages, so they also prove that the emitted `Facts` carry the
//! FFI signature the code generator consumes.

use ravum_lang_sema::facts::{CheckOptions, Profile};
use ravum_lang_syntax::diag::Severity;
use std::path::PathBuf;

struct TempPackage {
    dir: PathBuf,
}

impl TempPackage {
    fn new(name: &str, source: &str) -> TempPackage {
        let dir = std::env::temp_dir().join(format!(
            "ravum-ffi-sema-{}-{}-{}",
            std::process::id(),
            name,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(dir.join("src")).expect("cannot create temp package");
        std::fs::write(
            dir.join("ravum.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n"),
        )
        .expect("cannot write manifest");
        std::fs::write(dir.join("src/main.rv"), source).expect("cannot write source");
        TempPackage { dir }
    }

    fn check_result(&self, profile: Profile) -> ravum_lang_sema::CheckResult {
        let loaded = ravum_lang_sema::load_package(&self.dir);
        assert!(
            loaded.diags.is_empty(),
            "loader diagnostics:\n{}",
            loaded.diags.render(&loaded.sources)
        );
        let program = loaded.program.expect("loader produced no program");
        let options = CheckOptions {
            profile,
            ..CheckOptions::default()
        };
        ravum_lang_sema::check(&program, &loaded.sources, &options)
    }

    fn check(&self, profile: Profile) -> Vec<String> {
        self.check_result(profile)
            .diags
            .as_slice()
            .iter()
            .map(|diag| diag.code.clone())
            .collect()
    }
}

impl Drop for TempPackage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn extern_call_outside_unsafe_is_rejected() {
    let package = TempPackage::new(
        "ffi-outside",
        r#"extern "C" {
    fn abs(x: i32) -> i32;
}

fn main() {
    let value = abs(-1);
}
"#,
    );
    let codes = package.check(Profile::Core);
    assert!(
        codes.iter().any(|code| code == "E-UNSAFE-CONTEXT"),
        "expected E-UNSAFE-CONTEXT, got {codes:?}"
    );
}

#[test]
fn extern_call_inside_unsafe_is_accepted() {
    let package = TempPackage::new(
        "ffi-inside",
        r#"extern "C" {
    fn abs(x: i32) -> i32;
}

fn main() {
    let value = unsafe { abs(-1) };
}
"#,
    );
    let codes = package.check(Profile::Core);
    // The binding is deliberately unused, so `W-OWN-UNUSED-VARIABLE` is expected;
    // what this test asserts is that no *error* is produced.
    assert!(
        codes.iter().all(|code| !code.starts_with("E-")),
        "expected no error, got {codes:?}"
    );
}

#[test]
fn unsafe_is_forbidden_by_the_pure_profile() {
    let package = TempPackage::new(
        "ffi-pure",
        r#"extern "C" {
    fn abs(x: i32) -> i32;
}

fn main() {
    let value = unsafe { abs(-1) };
}
"#,
    );
    let codes = package.check(Profile::Pure);
    assert!(
        codes.iter().any(|code| code == "E-EFF-PROFILE"),
        "expected E-EFF-PROFILE, got {codes:?}"
    );
}

#[test]
fn declared_unsafe_and_ffi_effects_are_accepted() {
    let package = TempPackage::new(
        "ffi-declared",
        r#"extern "C" {
    fn abs(x: i32) -> i32;
}

fn call_abs(x: i32) -> i32 effects(unsafe, ffi) {
    unsafe { abs(x) }
}

fn main() {
}
"#,
    );
    let codes = package.check(Profile::Core);
    assert!(
        !codes.iter().any(|code| code == "E-UNSUPPORTED-EFFECT"),
        "declared `unsafe`/`ffi` must be supported, got {codes:?}"
    );
    assert!(codes.is_empty(), "expected no diagnostics, got {codes:?}");
}

#[test]
fn non_ffi_safe_parameter_is_rejected() {
    let package = TempPackage::new(
        "ffi-type",
        r#"extern "C" {
    fn puts(s: String) -> i32;
}

fn main() {
}
"#,
    );
    let codes = package.check(Profile::Core);
    assert!(
        codes.iter().any(|code| code == "E-FFI-TYPE"),
        "expected E-FFI-TYPE, got {codes:?}"
    );
}

/// Error codes only, in deterministic order. Warnings (for example
/// `W-OWN-UNUSED-VARIABLE`) are not part of the FFI contract under test.
fn error_codes(result: &ravum_lang_sema::CheckResult) -> Vec<String> {
    result
        .diags
        .as_slice()
        .iter()
        .filter(|diag| diag.severity == Severity::Error)
        .map(|diag| diag.code.clone())
        .collect()
}

#[test]
fn i128_parameter_is_rejected_at_the_ffi_boundary() {
    let package = TempPackage::new(
        "ffi-i128-param",
        r#"extern "C" {
    fn take_128(x: i128) -> i32;
}

fn main() {
}
"#,
    );
    let result = package.check_result(Profile::Core);
    assert_eq!(
        error_codes(&result),
        vec!["E-FFI-TYPE"],
        "i128 is not FFI-safe as an extern parameter (S4, 2026-11-14)"
    );
    assert!(
        result.checked.is_none(),
        "a refused extern declaration must not produce a CheckedProgram"
    );
}

#[test]
fn u128_return_is_rejected_at_the_ffi_boundary() {
    let package = TempPackage::new(
        "ffi-u128-return",
        r#"extern "C" {
    fn give_128() -> u128;
}

fn main() {
}
"#,
    );
    let result = package.check_result(Profile::Core);
    assert_eq!(
        error_codes(&result),
        vec!["E-FFI-TYPE"],
        "u128 is not FFI-safe as an extern return (S4, 2026-11-14)"
    );
    assert!(
        result.checked.is_none(),
        "a refused extern declaration must not produce a CheckedProgram"
    );
}

/// C-FFI-1 on the verifier side (S4, APPROVED by the owner, 2026-11-14).
///
/// `i128` and `u128` are out of the admitted FFI types. The twin backend list
/// (`crates/ravum-lang-codegen/src/emit.rs`, `ast_ffi_safe`) still admits them
/// on this branch and is aligned by agent E (wave 2). This test proves the
/// front-end gate runs **before** the backend: `check` emits `E-FFI-TYPE` and
/// returns **no** `CheckedProgram`, so codegen can never receive an
/// `i128`/`u128` extern signature even while its own guard is still permissive.
#[test]
fn c_ffi_1_verifier_refuses_i128_and_u128_before_the_backend() {
    let package = TempPackage::new(
        "ffi-128-before-backend",
        r#"extern "C" {
    fn take_128(x: i128) -> u128;
}

fn main() {
}
"#,
    );
    let result = package.check_result(Profile::Core);
    assert_eq!(
        error_codes(&result),
        vec!["E-FFI-TYPE", "E-FFI-TYPE"],
        "expected one E-FFI-TYPE for the i128 parameter and one for the u128 return"
    );
    assert!(
        result.checked.is_none(),
        "the verifier must refuse before the backend: no CheckedProgram may be produced"
    );
}

#[test]
fn i64_declaration_remains_ffi_safe() {
    let package = TempPackage::new(
        "ffi-i64-admitted",
        r#"extern "C" {
    fn take_64(x: i64) -> i64;
}

fn main() {
}
"#,
    );
    let result = package.check_result(Profile::Core);
    assert_eq!(
        error_codes(&result),
        Vec::<String>::new(),
        "i64 stays in the admitted set"
    );
    assert!(
        result.checked.is_some(),
        "an admitted extern signature still produces a CheckedProgram"
    );
}
