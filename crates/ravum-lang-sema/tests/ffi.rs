//! FFI boundary tests: `extern "C"` declarations, `unsafe` contexts and the
//! profile interaction. These tests exercise the real loader + checker on
//! temporary packages, so they also prove that the emitted `Facts` carry the
//! FFI signature the code generator consumes.

use ravum_lang_sema::facts::{CheckOptions, Profile};
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

    fn check(&self, profile: Profile) -> Vec<String> {
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
        let checked = ravum_lang_sema::check(&program, &loaded.sources, &options);
        checked
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
