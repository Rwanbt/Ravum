//! Manifest-contract coverage for the package loader
//! (`crates/ravum-lang-sema/src/loader.rs`).
//!
//! `loader_contracts.rs` pins the *package graph* behaviours (dependencies,
//! cycles, lockfile, cache, symlinks). This file pins the **manifest format**
//! and the small public enumerations the CLI prints: every strict rule of
//! `spec/language/modules.md` §5/§8 that the loader enforces, exercised through
//! `load_package` on a real temporary package, with the exact diagnostic code
//! and the discriminating part of its message.
//!
//! Every case asserts a *refusal* (or the documented identifier), so a loader
//! that silently accepted a malformed manifest fails a test here.
//!
//! Run with:
//! `cargo test -p ravum-lang-sema --test loader_manifest_contracts`

use ravum_lang_sema::loader::{load_package, CrateKind, LoadedPackage, LockfileState};
use ravum_lang_sema::syntax::ast::ItemKind;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "ravum-loader-manifest-{tag}-{}-{}-{nanos}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("cannot create the temporary directory");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn file(&self, rel: &str, content: &str) -> PathBuf {
        let path = self.path.join(rel);
        fs::create_dir_all(path.parent().expect("a file needs a parent directory"))
            .expect("cannot create the parent directory");
        fs::write(&path, content).expect("cannot write the fixture file");
        path
    }

    fn dir(&self, rel: &str) -> PathBuf {
        let path = self.path.join(rel);
        fs::create_dir_all(&path).expect("cannot create the fixture directory");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn describe(loaded: &LoadedPackage) -> String {
    if loaded.diags.is_empty() {
        return "<no diagnostic>".to_string();
    }
    loaded
        .diags
        .iter()
        .map(|diag| format!("{}: {}", diag.code, diag.message))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn message_of(loaded: &LoadedPackage, code: &str) -> String {
    loaded
        .diags
        .iter()
        .find(|diag| diag.code == code)
        .unwrap_or_else(|| panic!("expected diagnostic {code}; got [{}]", describe(loaded)))
        .message
        .clone()
}

/// Load a package whose manifest is exactly `manifest_text`; the crate root is
/// a minimal `src/main.rv` so a *manifest* error is the only reason to fail.
fn load_manifest(tag: &str, manifest_text: &str) -> LoadedPackage {
    let temp = TempDir::new(tag);
    temp.file("ravum.toml", manifest_text);
    temp.file("src/main.rv", "fn main() {}\n");
    load_package(temp.path())
}

/// The load must report `code` with a message containing `needle`, and must not
/// hand a program to the checker.
fn assert_manifest_refusal(manifest_text: &str, code: &str, needle: &str) {
    let loaded = load_manifest("refuse", manifest_text);
    let message = message_of(&loaded, code);
    assert!(
        message.contains(needle),
        "diagnostic {code} must contain {needle:?}; got {message:?} (all: [{}])",
        describe(&loaded)
    );
    assert!(
        loaded.program.is_none(),
        "a refused manifest must not yield a program; got one for:\n{manifest_text}"
    );
}

// ---------------------------------------------------------------------------
// Public identifiers printed by the CLI
// ---------------------------------------------------------------------------

#[test]
fn crate_kind_names_are_the_documented_identifiers() {
    assert_eq!(CrateKind::Bin.name(), "bin");
    assert_eq!(CrateKind::Lib.name(), "lib");
    assert_eq!(CrateKind::File.name(), "file");

    // The same identifiers are observable through a real load.
    let temp = TempDir::new("kind-bin");
    temp.file(
        "ravum.toml",
        "[package]\nname = \"kind-bin\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
    );
    temp.file("src/main.rv", "fn main() {}\n");
    let bin = load_package(temp.path());
    assert_eq!(bin.crate_kind, Some(CrateKind::Bin));
    assert_eq!(bin.crate_kind.expect("a crate kind").name(), "bin");

    let temp = TempDir::new("kind-lib");
    temp.file(
        "ravum.toml",
        "[package]\nname = \"kind-lib\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
    );
    temp.file("src/lib.rv", "fn helper() -> i32 { return 1; }\n");
    let lib = load_package(temp.path());
    assert_eq!(lib.crate_kind, Some(CrateKind::Lib));
    assert_eq!(lib.crate_kind.expect("a crate kind").name(), "lib");
}

#[test]
fn lockfile_state_names_are_the_documented_identifiers() {
    assert_eq!(LockfileState::Created.name(), "created");
    assert_eq!(LockfileState::Current.name(), "current");
    assert_eq!(LockfileState::Stale.name(), "stale");
    assert_eq!(LockfileState::Missing.name(), "missing");
    assert_eq!(LockfileState::Invalid.name(), "invalid");
    assert_eq!(LockfileState::NotApplicable.name(), "not_applicable");

    // A fresh package writes its lockfile: the state is `Created`, and the name
    // the CLI prints matches the enum.
    let temp = TempDir::new("lock-created");
    temp.file(
        "ravum.toml",
        "[package]\nname = \"lock-created\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
    );
    temp.file("src/main.rv", "fn main() {}\n");
    let loaded = load_package(temp.path());
    assert!(
        loaded.diags.is_empty(),
        "expected a clean load: [{}]",
        describe(&loaded)
    );
    assert_eq!(loaded.lockfile_state, LockfileState::Created);
    assert_eq!(loaded.lockfile_state.name(), "created");
}

// ---------------------------------------------------------------------------
// Sections and keys
// ---------------------------------------------------------------------------

#[test]
fn unknown_and_unsupported_sections_are_refused() {
    assert_manifest_refusal(
        "[package]\nname = \"pkg\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n\n[proto]\nthing = \"x\"\n",
        "E-MOD-MANIFEST",
        "unknown section `[proto]`",
    );
    assert_manifest_refusal(
        "[package]\nname = \"pkg\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n\n[build]\ncommand = \"make\"\n",
        "E-MOD-BUILD-SCRIPT",
        "section `[build]` is not supported",
    );
    assert_manifest_refusal(
        "[package]\nname = \"pkg\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n\n[scripts]\ntest = \"run\"\n",
        "E-MOD-BUILD-SCRIPT",
        "section `[scripts]` is not supported",
    );
    assert_manifest_refusal(
        "[package]\nname = \"pkg\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n\n[dependencies.dep]\npath = \"dep\"\n",
        "E-MOD-DEPENDENCY",
        "section `[dependencies.dep]` is not supported",
    );
}

#[test]
fn package_required_keys_and_value_kinds_are_enforced() {
    // Missing `[package]` entirely.
    assert_manifest_refusal(
        "[dependencies]\n",
        "E-MOD-MANIFEST",
        "missing required `[package]` section",
    );
    // Required keys, one at a time.
    assert_manifest_refusal(
        "[package]\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
        "E-MOD-MANIFEST",
        "missing required `package.name`",
    );
    assert_manifest_refusal(
        "[package]\nname = \"pkg\"\nedition = \"core-1.0\"\n",
        "E-MOD-MANIFEST",
        "missing required `package.version`",
    );
    assert_manifest_refusal(
        "[package]\nname = \"pkg\"\nversion = \"0.1.0\"\n",
        "E-MOD-MANIFEST",
        "missing required `package.edition`",
    );
    // A bare integer is refused by the TOML reader before the manifest schema
    // is consulted (`parse_manifest` only ever sees strings and inline tables).
    assert_manifest_refusal(
        "[package]\nname = 5\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
        "E-MOD-MANIFEST",
        "unsupported TOML value `5`",
    );
    // The manifest is strict about unknown keys.
    assert_manifest_refusal(
        "[package]\nname = \"pkg\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\nlicence = \"MIT\"\n",
        "E-MOD-MANIFEST",
        "unknown key `package.licence`",
    );
    // Name and version syntax.
    assert_manifest_refusal(
        "[package]\nname = \"Pkg\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
        "E-MOD-MANIFEST",
        "invalid package name `Pkg`",
    );
    assert_manifest_refusal(
        "[package]\nname = \"pkg\"\nversion = \"1.0\"\nedition = \"core-1.0\"\n",
        "E-MOD-MANIFEST",
        "invalid version `1.0`",
    );
    assert_manifest_refusal(
        "[package]\nname = \"pkg\"\nversion = \"0.1.0\"\nedition = \"core-2.0\"\n",
        "E-UNSUPPORTED-EDITION",
        "unsupported edition `core-2.0`",
    );
}

#[test]
fn dependency_entries_are_strict() {
    let base = "[package]\nname = \"pkg\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n";
    // A dependency name must be a valid package name.
    assert_manifest_refusal(
        &format!("{base}\n[dependencies]\nDep = {{ path = \"dep\" }}\n"),
        "E-MOD-DEPENDENCY",
        "invalid dependency name `Dep`",
    );
    // A dependency must be an inline table, not a bare string.
    assert_manifest_refusal(
        &format!("{base}\n[dependencies]\ndep = \"dep\"\n"),
        "E-MOD-DEPENDENCY",
        "must be `dep = { path = \"...\" }`",
    );
    // Exactly one `path` key, and it must be named `path`.
    assert_manifest_refusal(
        &format!("{base}\n[dependencies]\ndep = {{ version = \"1.0.0\" }}\n"),
        "E-MOD-DEPENDENCY",
        "must contain exactly one `path` key",
    );
    // ... and a non-string value is refused by the TOML reader, which only
    // accepts quoted values inside an inline table.
    assert_manifest_refusal(
        &format!("{base}\n[dependencies]\ndep = {{ path = 5 }}\n"),
        "E-MOD-MANIFEST",
        "must be `key = \"value\"`",
    );
}

// ---------------------------------------------------------------------------
// TOML lexical rules
// ---------------------------------------------------------------------------

#[test]
fn toml_lexical_errors_are_reported_with_the_manifest_code() {
    // Bare (non-string) value.
    assert_manifest_refusal(
        "[package]\nname = pkg\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
        "E-MOD-MANIFEST",
        "unsupported TOML value `pkg`",
    );
    // Unterminated basic string.
    assert_manifest_refusal(
        "[package]\nname = \"pkg\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
        "E-MOD-MANIFEST",
        "unterminated string",
    );
    // Unsupported escape sequence.
    assert_manifest_refusal(
        "[package]\nname = \"pkg\\q\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
        "E-MOD-MANIFEST",
        "unsupported escape sequence `\\q`",
    );
    // A control character cannot appear in a basic string.
    assert_manifest_refusal(
        "[package]\nname = \"p\tkg\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
        "E-MOD-MANIFEST",
        "control character in string",
    );
    // Inline tables must close on the same line.
    assert_manifest_refusal(
        "[package]\nname = \"pkg\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n\n[dependencies]\ndep = { path = \"dep\"\n",
        "E-MOD-MANIFEST",
        "inline table must be closed with `}`",
    );
    // An inline entry without `key = value`.
    assert_manifest_refusal(
        "[package]\nname = \"pkg\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n\n[dependencies]\ndep = { path }\n",
        "E-MOD-MANIFEST",
        "invalid inline table entry `path`",
    );
    // A duplicate inline key is refused, not silently last-wins.
    assert_manifest_refusal(
        "[package]\nname = \"pkg\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n\n[dependencies]\ndep = { path = \"a\", path = \"b\" }\n",
        "E-MOD-MANIFEST",
        "duplicate inline table key `path`",
    );
}

#[test]
fn escaped_values_and_comments_are_accepted_by_the_manifest_parser() {
    // Escapes decode to the same string the plain form produces, and a trailing
    // comment is stripped only outside strings.
    let manifest = concat!(
        "[package]\n",
        "name = \"escaped\" # a comment with a \"quote\" and a \\ backslash\n",
        "version = \"0.1.0\"\n",
        "edition = \"core-1.0\"\n",
    );
    let loaded = load_manifest("escaped", manifest);
    assert!(
        loaded.diags.is_empty(),
        "a valid manifest with a commented line must load: [{}]",
        describe(&loaded)
    );
    assert_eq!(loaded.package_name.as_deref(), Some("escaped"));
    assert!(loaded.program.is_some(), "the crate root must be loaded");
}

#[test]
fn a_package_with_both_crate_roots_is_refused() {
    let temp = TempDir::new("both");
    temp.file(
        "ravum.toml",
        "[package]\nname = \"both\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
    );
    temp.file("src/main.rv", "fn main() {}\n");
    temp.file("src/lib.rv", "fn helper() -> i32 { return 1; }\n");
    let loaded = load_package(temp.path());
    let message = message_of(&loaded, "E-MOD-BOTH-ROOTS");
    assert!(
        message.contains("main.rv") && message.contains("lib.rv"),
        "the diagnostic must name both roots; got {message:?}"
    );
    assert!(loaded.program.is_none());
    assert!(loaded.lib_program.is_none());
}

#[test]
fn a_package_without_any_crate_root_is_refused() {
    let temp = TempDir::new("no-root");
    temp.file(
        "ravum.toml",
        "[package]\nname = \"no-root\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
    );
    temp.dir("src");
    let loaded = load_package(temp.path());
    assert!(
        loaded.diags.has_errors(),
        "an empty src/ must be a load error, got [{}]",
        describe(&loaded)
    );
    assert!(loaded.program.is_none());
}

#[test]
fn module_declarations_are_inlined_into_module_items() {
    // `mod inner;` is resolved by the loader and rewritten into an inline
    // `ItemKind::Mod`: no `ModDecl` survives into the checked program, which is
    // why the backend's `ModDecl` refusal has no accepted program behind it.
    let temp = TempDir::new("mod-inline");
    temp.file(
        "ravum.toml",
        "[package]\nname = \"mod-inline\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
    );
    temp.file(
        "src/main.rv",
        "mod inner;\n\nfn main() {\n    let value = inner::helper();\n}\n",
    );
    temp.file(
        "src/inner.rv",
        "pub fn helper() -> i32 {\n    return 7;\n}\n",
    );
    let loaded = load_package(temp.path());
    assert!(
        !loaded.diags.has_errors(),
        "a resolvable `mod inner;` must load cleanly: [{}]",
        describe(&loaded)
    );
    let program = loaded.program.expect("the package must load");
    let mut declarations = 0usize;
    let mut inline_modules = 0usize;
    for item in &program.root.items {
        match &item.kind {
            ItemKind::ModDecl(_) => declarations += 1,
            ItemKind::Mod(_) => inline_modules += 1,
            _ => {}
        }
    }
    assert_eq!(
        declarations, 0,
        "the loader must inline every `mod` declaration (found {declarations} ModDecl)"
    );
    assert_eq!(
        inline_modules, 1,
        "`mod inner;` must become one inline module (found {inline_modules})"
    );
    assert!(
        loaded
            .loaded_files
            .iter()
            .any(|path| path.ends_with("inner.rv")),
        "the module file must be registered in the load: {:?}",
        loaded.loaded_files
    );
}
