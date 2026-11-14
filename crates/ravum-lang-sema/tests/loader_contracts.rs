//! Behaviour contracts for the package/module loader
//! (`crates/ravum-lang-sema/src/loader.rs`).
//!
//! Each test builds a realistic temporary package (`ravum.toml`, a `src/`
//! crate root, optional local dependencies) and asserts the *observable*
//! contract of one loader path: the exact diagnostic code, the discriminating
//! part of its message (the offending line, path, component or limit), and the
//! fail-closed outcome (`program == None`, no lockfile written, etc.).
//!
//! The point is not line coverage: a test that only asserted `is_err()` would
//! keep passing after the diagnostic or the fail-closed rule was gutted. Every
//! assertion below names the observable text or state that the loader's
//! documentation promises (`loader.rs` module docs, `spec/language/modules.md`
//! MOD-5.10 / MOD-7.2 / §7, `spec/language/diagnostics.md`).
//!
//! Run with:
//! `cargo test -p ravum-lang-sema --test loader_contracts`

use ravum_lang_sema::loader::{
    load_package, load_package_with_parse_cache, CrateKind, LoadOptions, LoadedPackage,
    LockfileState, ParseCache, LOCKFILE_NAME, MANIFEST_NAME, MAX_FILE_SIZE,
};
use ravum_lang_sema::syntax::ast::ItemKind;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Temporary package fixtures (same shape as the loader unit tests)
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
            "ravum-loader-contracts-{tag}-{}-{}-{nanos}",
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
        self.bytes(rel, content.as_bytes())
    }

    fn bytes(&self, rel: &str, content: &[u8]) -> PathBuf {
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

/// A valid `ravum.toml`; `deps` is appended verbatim (usually a
/// `[dependencies]` section).
fn manifest(name: &str, deps: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n{deps}")
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

/// The message of the first diagnostic carrying `code`.
fn message_of(loaded: &LoadedPackage, code: &str) -> String {
    loaded
        .diags
        .iter()
        .find(|diag| diag.code == code)
        .unwrap_or_else(|| panic!("expected diagnostic {code}; got [{}]", describe(loaded)))
        .message
        .clone()
}

/// The load must report `code` with a message containing `needle`, and must not
/// hand a (partially loaded) program to the checker.
fn assert_fail_closed(loaded: &LoadedPackage, code: &str, needle: &str) {
    let message = message_of(loaded, code);
    assert!(
        message.contains(needle),
        "diagnostic {code} must contain {needle:?}; got {message:?} (all: {})",
        describe(loaded)
    );
    assert!(
        loaded.program.is_none(),
        "a load rejected with {code} must not hand a program to the checker; diags: {}",
        describe(loaded)
    );
}

/// The load must succeed and hand a program to the checker.
fn assert_accepted(loaded: &LoadedPackage) {
    assert!(
        !loaded.diags.has_errors(),
        "unexpected loader errors: {}",
        describe(loaded)
    );
    assert!(
        loaded.program.is_some(),
        "an accepted load must hand a program to the checker"
    );
}

/// Package `app` depending on `dep_name = { path = "dep_path" }`.
fn app_with_dependency(temp: &TempDir, dep_name: &str, dep_path: &str) -> PathBuf {
    temp.file(
        "app/ravum.toml",
        &manifest(
            "app",
            &format!("\n[dependencies]\n{dep_name} = {{ path = \"{dep_path}\" }}\n"),
        ),
    );
    temp.file("app/src/main.rv", "fn main() { }\n");
    temp.path().join("app")
}

/// A local library package `name` in `dir`, with a `src/lib.rv`.
fn library_package(temp: &TempDir, dir: &str, name: &str) {
    temp.file(&format!("{dir}/ravum.toml"), &manifest(name, ""));
    temp.file(
        &format!("{dir}/src/lib.rv"),
        "pub fn helper() -> i32 { return 1; }\n",
    );
}

fn load_with_manifest_text(tag: &str, text: &str) -> LoadedPackage {
    let temp = TempDir::new(tag);
    temp.file(MANIFEST_NAME, text);
    temp.file("src/main.rv", "fn main() { }\n");
    load_package(temp.path())
}

#[cfg(unix)]
fn symlink_file(target: &Path, link: &Path) -> bool {
    std::os::unix::fs::symlink(target, link).is_ok()
}

#[cfg(unix)]
fn symlink_dir(target: &Path, link: &Path) -> bool {
    std::os::unix::fs::symlink(target, link).is_ok()
}

#[cfg(windows)]
fn symlink_file(target: &Path, link: &Path) -> bool {
    std::os::windows::fs::symlink_file(target, link).is_ok()
}

#[cfg(windows)]
fn symlink_dir(target: &Path, link: &Path) -> bool {
    std::os::windows::fs::symlink_dir(target, link).is_ok()
}

// ---------------------------------------------------------------------------
// Manifest: the internal TOML subset is strict and reports the offending line
// ---------------------------------------------------------------------------

/// `parse_toml`: an unterminated section header, a line without `=`, an
/// invalid key, an invalid section name, a duplicate section and a duplicate
/// key all fail closed with `E-MOD-MANIFEST` and a line number.
#[test]
fn manifest_structural_errors_are_reported_with_line_and_text() {
    let cases: &[(&str, &str, &[&str])] = &[
        (
            "header",
            "[package\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
            &["invalid section header on line 1", "[package"],
        ),
        (
            "missing-eq",
            "[package]\nname \"app\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
            &["expected `key = value` on line 2"],
        ),
        (
            "invalid-key",
            "[package]\nbad key = \"x\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
            &["invalid key `bad key` on line 2"],
        ),
        (
            "invalid-section",
            "[bad name]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
            &["invalid section name `bad name` on line 1"],
        ),
        (
            "duplicate-section",
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n[package]\n",
            &["duplicate section `[package]` on line 5"],
        ),
        (
            "duplicate-key",
            "[package]\nname = \"app\"\nname = \"app2\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
            &["duplicate key `name` in section `[package]`"],
        ),
    ];
    for (tag, text, needles) in cases {
        let loaded = load_with_manifest_text(tag, text);
        for needle in *needles {
            assert_fail_closed(&loaded, "E-MOD-MANIFEST", needle);
        }
        assert_eq!(
            loaded.package_name, None,
            "{tag}: an invalid manifest must not publish a package identity"
        );
        assert_eq!(
            loaded.lockfile_state,
            LockfileState::NotApplicable,
            "{tag}: no lockfile may be written for an invalid manifest"
        );
    }
}

/// `parse_toml_value` / `parse_basic_string`: only single-line basic strings and
/// inline tables are accepted; numbers, unterminated strings, unknown escapes
/// and control characters are refused by name.
#[test]
fn manifest_values_must_be_strings_or_inline_tables() {
    let cases: &[(&str, &str, &str)] = &[
        (
            "number",
            "[package]\nname = \"app\"\nversion = 1\nedition = \"core-1.0\"\n",
            "unsupported TOML value `1`; only strings and inline tables are accepted",
        ),
        (
            "unterminated",
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"core-1.0\n",
            "unterminated string",
        ),
        (
            "bad-escape",
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"core-\\q\"\n",
            "unsupported escape sequence `\\q` in string",
        ),
        (
            "control-char",
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"core-1.0\tx\"\n",
            "control character in string",
        ),
    ];
    for (tag, text, needle) in cases {
        let loaded = load_with_manifest_text(tag, text);
        assert_fail_closed(&loaded, "E-MOD-MANIFEST", needle);
    }
}

/// `parse_toml_value`: an inline table must be closed on its line, contain
/// `key = "value"` entries only, and must not repeat a key.
#[test]
fn manifest_inline_tables_are_strict() {
    let header = "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n";
    let cases: &[(&str, String, &str)] = &[
        (
            "unclosed",
            format!("{header}[dependencies]\nutil = {{ path = \"../util\"\n"),
            "inline table must be closed with `}` on the same line",
        ),
        (
            "non-string-entry",
            format!("{header}[dependencies]\nutil = {{ path = 1 }}\n"),
            "must be `key = \"value\"`",
        ),
        (
            "duplicate-key",
            format!("{header}[dependencies]\nutil = {{ path = \"../a\", path = \"../b\" }}\n"),
            "duplicate inline table key `path`",
        ),
    ];
    for (tag, text, needle) in cases {
        let loaded = load_with_manifest_text(tag, text);
        assert_fail_closed(&loaded, "E-MOD-MANIFEST", needle);
    }
}

// ---------------------------------------------------------------------------
// Manifest: I/O limits and encoding
// ---------------------------------------------------------------------------

/// `read_text_limited`: a manifest above [`MAX_FILE_SIZE`] is refused with
/// `E-LIMIT-FILE-SIZE`, the byte count and the limit; no lockfile is written.
#[test]
fn manifest_above_the_size_limit_is_refused() {
    let temp = TempDir::new("manifest-too-large");
    let oversized = vec![b'#'; MAX_FILE_SIZE as usize + 1];
    temp.bytes(MANIFEST_NAME, &oversized);
    temp.file("src/main.rv", "fn main() { }\n");
    let loaded = load_package(temp.path());
    assert_fail_closed(
        &loaded,
        "E-LIMIT-FILE-SIZE",
        "is 8388609 bytes, above the 8388608 byte limit",
    );
    assert!(
        !temp.path().join(LOCKFILE_NAME).exists(),
        "a manifest that was never read must not produce a lockfile"
    );
}

/// `read_text_limited`: a manifest that is not valid UTF-8 is refused with
/// `E-UNSUPPORTED-TOKEN`, not silently lossily decoded.
#[test]
fn manifest_invalid_utf8_is_refused() {
    let temp = TempDir::new("manifest-utf8");
    let mut bytes =
        b"[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n# ".to_vec();
    bytes.push(0xff);
    bytes.push(b'\n');
    temp.bytes(MANIFEST_NAME, &bytes);
    temp.file("src/main.rv", "fn main() { }\n");
    let loaded = load_package(temp.path());
    assert_fail_closed(
        &loaded,
        "E-UNSUPPORTED-TOKEN",
        "is not valid UTF-8; Ravum Core 1.0 sources are UTF-8",
    );
    assert!(
        !temp.path().join(LOCKFILE_NAME).exists(),
        "an unreadable manifest must not produce a lockfile"
    );
}

// ---------------------------------------------------------------------------
// Dependency resolution: paths
// ---------------------------------------------------------------------------

/// `resolve_dependency_dir`: the empty path (`has an empty path`), a regular
/// file (`is not a directory`), a missing target (`does not exist`) and a
/// directory without `ravum.toml` (`does not contain a `ravum.toml``) are all
/// `E-MOD-DEPENDENCY`, and the graph failure suppresses the lockfile.
#[test]
fn dependency_paths_must_resolve_to_existing_directories() {
    // Empty path: `path = ""` (the manifest subset can express it).
    let temp = TempDir::new("dep-empty-path");
    temp.file(
        "app/ravum.toml",
        &manifest("app", "\n[dependencies]\nutil = { path = \"\" }\n"),
    );
    temp.file("app/src/main.rv", "fn main() { }\n");
    let loaded = load_package(&temp.path().join("app"));
    assert_fail_closed(&loaded, "E-MOD-DEPENDENCY", "has an empty path");
    assert_eq!(
        loaded.lockfile_state,
        LockfileState::NotApplicable,
        "an unresolved graph must not produce a lockfile"
    );

    // A regular file is not a package directory.
    let temp = TempDir::new("dep-is-file");
    temp.file("util", "not a directory\n");
    let app = app_with_dependency(&temp, "util", "../util");
    let loaded = load_package(&app);
    assert_fail_closed(&loaded, "E-MOD-DEPENDENCY", "is not a directory");

    // A missing target is reported as missing, with the offending path.
    let temp = TempDir::new("dep-missing");
    let app = app_with_dependency(&temp, "util", "../nope");
    let loaded = load_package(&app);
    assert_fail_closed(&loaded, "E-MOD-DEPENDENCY", "does not exist");
    assert!(
        message_of(&loaded, "E-MOD-DEPENDENCY").contains("nope"),
        "the diagnostic must name the missing component"
    );

    // A directory that is not a package (no manifest).
    let temp = TempDir::new("dep-no-manifest");
    temp.dir("util");
    let app = app_with_dependency(&temp, "util", "../util");
    let loaded = load_package(&app);
    assert_fail_closed(
        &loaded,
        "E-MOD-DEPENDENCY",
        "does not contain a `ravum.toml`",
    );
}

/// `collect_dependencies`: a dependency whose resolved `package.name` is a
/// Windows-reserved component is refused (`E-MOD-RESERVED-NAME`) even though a
/// POSIX filesystem accepts it; the message names the component.
#[test]
fn dependency_name_must_not_be_windows_reserved() {
    let temp = TempDir::new("dep-reserved-name");
    // The dependency key must equal `package.name`, so both are `con`.
    library_package(&temp, "util", "con");
    let app = app_with_dependency(&temp, "con", "../util");
    let loaded = load_package(&app);
    assert_fail_closed(
        &loaded,
        "E-MOD-RESERVED-NAME",
        "is a reserved Windows path component",
    );
}

/// `collect_dependencies`: a package name containing `-` is a valid package
/// name but not a valid module path, so a dependency on it is refused by name.
#[test]
fn dependency_name_must_be_usable_as_a_module() {
    let temp = TempDir::new("dep-module-name");
    library_package(&temp, "util", "my-util");
    let app = app_with_dependency(&temp, "my-util", "../util");
    let loaded = load_package(&app);
    assert_fail_closed(
        &loaded,
        "E-MOD-DEPENDENCY",
        "is not usable as a module path in Core 1.0",
    );
}

/// `collect_dependencies`: `src/lib.rv` must be a regular file; a directory
/// named `lib.rv` is refused instead of being read as a crate root.
#[test]
fn dependency_lib_that_is_not_a_regular_file_is_refused() {
    let temp = TempDir::new("dep-lib-dir");
    temp.file("util/ravum.toml", &manifest("util", ""));
    temp.dir("util/src/lib.rv");
    let app = app_with_dependency(&temp, "util", "../util");
    let loaded = load_package(&app);
    assert_fail_closed(
        &loaded,
        "E-MOD-DEPENDENCY",
        "`src/lib.rv` is not a regular file",
    );
}

/// `collect_dependencies`: a cycle is refused with `E-MOD-CYCLE` naming the
/// whole chain (`a -> b -> a`), and the load hands no program and no
/// dependency list to the CLI.
#[test]
fn dependency_cycle_reports_the_chain_and_fails_closed() {
    let temp = TempDir::new("dep-cycle");
    temp.file(
        "a/ravum.toml",
        &manifest("a", "\n[dependencies]\nb = { path = \"../b\" }\n"),
    );
    temp.file("a/src/lib.rv", "pub fn a() -> i32 { return 1; }\n");
    temp.file(
        "b/ravum.toml",
        &manifest("b", "\n[dependencies]\na = { path = \"../a\" }\n"),
    );
    temp.file("b/src/lib.rv", "pub fn b() -> i32 { return 2; }\n");
    let loaded = load_package(&temp.path().join("a"));
    assert_fail_closed(&loaded, "E-MOD-CYCLE", "dependency cycle: a -> b -> a");
    assert_eq!(
        loaded.dependencies,
        Vec::new(),
        "a cycle resolves no dependency"
    );
    assert_eq!(loaded.lockfile_state, LockfileState::NotApplicable);
}

/// `collect_dependencies`: the graph depth bound (`MAX_PACKAGE_DEPTH = 8`) is
/// enforced with `E-MOD-PATH-ESCAPE` and the documented message, not by a
/// stack overflow.
#[test]
fn dependency_graph_depth_is_bounded() {
    let temp = TempDir::new("dep-depth");
    temp.file(
        "app/ravum.toml",
        &manifest("app", "\n[dependencies]\nd0 = { path = \"../d0\" }\n"),
    );
    temp.file("app/src/main.rv", "fn main() { }\n");
    for index in 0..=8 {
        let next = index + 1;
        library_package(&temp, &format!("d{index}"), &format!("d{index}"));
        temp.file(
            &format!("d{index}/ravum.toml"),
            &manifest(
                &format!("d{index}"),
                &format!("\n[dependencies]\nd{next} = {{ path = \"../d{next}\" }}\n"),
            ),
        );
    }
    // Depth 9 is out of policy; it only needs to resolve as a directory with a
    // manifest, the depth check fires before the manifest is parsed.
    temp.file("d9/ravum.toml", "unused\n");
    let loaded = load_package(&temp.path().join("app"));
    assert_fail_closed(
        &loaded,
        "E-MOD-PATH-ESCAPE",
        "dependency graph deeper than 8 packages",
    );
}

/// `collect_dependencies`: a dependency manifest that itself fails to parse
/// fails the whole load (no program, no lockfile) instead of being skipped.
#[test]
fn dependency_manifest_error_fails_the_whole_load() {
    let temp = TempDir::new("dep-bad-manifest");
    // No `[package]` section: the dependency manifest is invalid.
    temp.file("util/ravum.toml", "[dependencies]\n");
    temp.file("util/src/lib.rv", "pub fn helper() -> i32 { return 1; }\n");
    let app = app_with_dependency(&temp, "util", "../util");
    let loaded = load_package(&app);
    assert_fail_closed(
        &loaded,
        "E-MOD-MANIFEST",
        "missing required `[package]` section",
    );
    assert_eq!(
        loaded.lockfile_state,
        LockfileState::NotApplicable,
        "a dependency that does not parse must not be frozen into ravum.lock"
    );
}

// ---------------------------------------------------------------------------
// Content digest: file-size policy
// ---------------------------------------------------------------------------

/// `package_digest` / `collect_rv_files`: a `.rv` file above [`MAX_FILE_SIZE`]
/// is refused with `E-LIMIT-FILE-SIZE` before it can be hashed, and the failed
/// digest marks the graph as failed (no lockfile, no program).
#[test]
fn dependency_source_above_the_size_limit_is_refused() {
    let temp = TempDir::new("source-too-large");
    temp.file(MANIFEST_NAME, &manifest("app", ""));
    temp.file("src/main.rv", "fn main() { }\n");
    temp.bytes("src/huge.rv", &vec![b'x'; MAX_FILE_SIZE as usize + 1]);
    let loaded = load_package(temp.path());
    assert_fail_closed(
        &loaded,
        "E-LIMIT-FILE-SIZE",
        "is larger than the 8388608 byte limit",
    );
    assert_eq!(loaded.lockfile_state, LockfileState::NotApplicable);
    assert!(
        !temp.path().join(LOCKFILE_NAME).exists(),
        "an unhashable package must not produce a lockfile"
    );
}

// ---------------------------------------------------------------------------
// Symbolic links: every loader path refuses to follow one
// ---------------------------------------------------------------------------

/// Symlinks are refused on all loader paths: a dependency whose path traverses
/// one, a symlinked `ravum.toml`, a symlink under `src/` seen by the package
/// digest, and a module candidate that is a symlink (single-file mode).
#[test]
fn symlinks_are_refused_on_every_loader_path() {
    // (a) A dependency path that traverses a symlink.
    let temp = TempDir::new("symlink-dep");
    let real = temp.dir("real");
    temp.file("real/ravum.toml", &manifest("util", ""));
    temp.file("real/src/lib.rv", "pub fn helper() -> i32 { return 1; }\n");
    let link = temp.path().join("link");
    if !symlink_dir(&real, &link) {
        eprintln!("SKIP symlink contracts: this OS refused to create a symbolic link");
        return;
    }
    let app = app_with_dependency(&temp, "util", "../link");
    let loaded = load_package(&app);
    assert_fail_closed(&loaded, "E-MOD-PATH", "traverses the symbolic link");
    assert!(
        message_of(&loaded, "E-MOD-PATH").contains("symlinks are refused for dependencies"),
        "the dependency diagnostic must state the policy"
    );

    // (b) A `ravum.toml` that is a symbolic link: never read, never followed.
    let temp = TempDir::new("symlink-manifest");
    let actual = temp.file("actual.toml", &manifest("app", ""));
    assert!(
        symlink_file(&actual, &temp.path().join(MANIFEST_NAME)),
        "cannot create the manifest symlink"
    );
    temp.file("src/main.rv", "fn main() { }\n");
    let loaded = load_package(temp.path());
    assert_fail_closed(
        &loaded,
        "E-MOD-PATH",
        "is a symbolic link; refusing to read it",
    );

    // (c) A symlink under `src/`: the package digest refuses to follow it.
    let temp = TempDir::new("symlink-digest");
    temp.file(MANIFEST_NAME, &manifest("app", ""));
    temp.file("src/main.rv", "fn main() { }\n");
    let real_rv = temp.file("src/real.rv", "pub fn helper() -> i32 { return 1; }\n");
    let src_link = temp.path().join("src").join("link.rv");
    assert!(
        symlink_file(&real_rv, &src_link),
        "cannot create the source symlink"
    );
    let loaded = load_package(temp.path());
    assert_fail_closed(&loaded, "E-MOD-PATH", "refused by the package digest");

    // (d) A module candidate that is a symbolic link (single-file mode has no
    // digest phase, so module resolution sees it directly).
    let temp = TempDir::new("symlink-module");
    let main = temp.file("main.rv", "mod util;\nfn main() { }\n");
    let real_rv = temp.file("real.rv", "pub fn helper() -> i32 { return 1; }\n");
    let module_link = temp.path().join("util.rv");
    assert!(
        symlink_file(&real_rv, &module_link),
        "cannot create the module symlink"
    );
    let loaded = load_package(&main);
    assert_fail_closed(
        &loaded,
        "E-MOD-PATH",
        "module `util` resolves through a symbolic link",
    );
}

// ---------------------------------------------------------------------------
// Crate roots and module expansion
// ---------------------------------------------------------------------------

/// `load_crates`: a package with neither `src/main.rv` nor `src/lib.rv` is
/// refused (`E-MOD-MISSING`) and hands no program to the checker.
#[test]
fn package_without_crate_root_is_refused() {
    let temp = TempDir::new("no-crate-root");
    temp.file(MANIFEST_NAME, &manifest("app", ""));
    let loaded = load_package(temp.path());
    assert_fail_closed(
        &loaded,
        "E-MOD-MISSING",
        "has neither `src/main.rv` nor `src/lib.rv`",
    );
    assert_eq!(loaded.crate_kind, None);
}

/// `load_crates`: `src/lib.rv` alone is a valid library crate (`CrateKind::Lib`)
/// and is registered under its own file id.
#[test]
fn library_only_package_loads_as_a_lib_crate() {
    let temp = TempDir::new("lib-only");
    temp.file(MANIFEST_NAME, &manifest("app", ""));
    temp.file("src/lib.rv", "pub fn helper() -> i32 { return 1; }\n");
    let loaded = load_package(temp.path());
    assert_accepted(&loaded);
    assert_eq!(loaded.crate_kind, Some(CrateKind::Lib));
    assert!(loaded.loaded_files.iter().any(|path| path == "src/lib.rv"));
    let program = loaded.program.as_ref().expect("program");
    let root = loaded.sources.file(program.file).expect("root source");
    assert_eq!(root.path, "src/lib.rv");
}

/// `resolve_module_file`: an inline module whose path component is a reserved
/// Windows name is refused before any filesystem lookup, naming the component.
#[test]
fn inline_module_with_reserved_component_is_refused() {
    let temp = TempDir::new("inline-reserved");
    temp.file(MANIFEST_NAME, &manifest("app", ""));
    temp.file(
        "src/main.rv",
        "mod con {\n    mod inner;\n}\nfn main() { }\n",
    );
    let loaded = load_package(temp.path());
    assert_fail_closed(
        &loaded,
        "E-MOD-RESERVED-NAME",
        "inline module path component `con` is a reserved Windows path component",
    );
}

/// `merge_dependencies`: a dependency whose name collides with a local module
/// is refused with `E-MOD-DUPLICATE` instead of shadowing one of the two.
#[test]
fn dependency_colliding_with_local_module_is_refused() {
    let temp = TempDir::new("dep-local-collision");
    library_package(&temp, "util", "util");
    let app = app_with_dependency(&temp, "util", "../util");
    temp.file("app/src/main.rv", "mod util;\nfn main() { }\n");
    temp.file("app/src/util.rv", "pub fn local() -> i32 { return 7; }\n");
    let loaded = load_package(&app);
    assert_fail_closed(
        &loaded,
        "E-MOD-DUPLICATE",
        "collides with a local module of the same name",
    );
}

/// `resolve_module_file`: a declared module that exists nowhere reports both
/// tried candidates (`src/x.rv` and `src/x/mod.rv`) and fails closed.
#[test]
fn module_declaration_without_a_file_names_both_candidates() {
    let temp = TempDir::new("module-absent");
    temp.file(MANIFEST_NAME, &manifest("app", ""));
    temp.file("src/main.rv", "mod absent;\nfn main() { }\n");
    let loaded = load_package(temp.path());
    assert_fail_closed(
        &loaded,
        "E-MOD-MISSING",
        "module `absent` not found: neither",
    );
    let message = message_of(&loaded, "E-MOD-MISSING");
    assert!(
        message.contains("absent.rv") && message.contains("mod.rv"),
        "both candidate files must be named: {message}"
    );
}

// ---------------------------------------------------------------------------
// Lockfile: invalid forms and stale rewrites
// ---------------------------------------------------------------------------

/// `update_lockfile`: a lockfile that is a directory, has no version line, or
/// is not UTF-8 is `LockfileState::Invalid` with the matching diagnostic; no
/// program is handed to the checker.
#[test]
fn lockfile_forms_that_must_be_refused() {
    // A directory named `ravum.lock`.
    let temp = TempDir::new("lock-dir");
    temp.file(MANIFEST_NAME, &manifest("app", ""));
    temp.file("src/main.rv", "fn main() { }\n");
    temp.dir(LOCKFILE_NAME);
    let loaded = load_package(temp.path());
    assert_fail_closed(&loaded, "E-MOD-LOCK", "is not a regular file");
    assert_eq!(loaded.lockfile_state, LockfileState::Invalid);

    // A lockfile without a `lockfile_version` line: never interpreted partially.
    let temp = TempDir::new("lock-no-version");
    temp.file(MANIFEST_NAME, &manifest("app", ""));
    temp.file("src/main.rv", "fn main() { }\n");
    temp.file(LOCKFILE_NAME, "hello\n");
    let loaded = load_package(temp.path());
    assert_fail_closed(
        &loaded,
        "E-MOD-LOCK",
        "has no valid `lockfile_version` line",
    );
    assert_eq!(loaded.lockfile_state, LockfileState::Invalid);

    // A lockfile that is not valid UTF-8.
    let temp = TempDir::new("lock-utf8");
    temp.file(MANIFEST_NAME, &manifest("app", ""));
    temp.file("src/main.rv", "fn main() { }\n");
    temp.bytes(LOCKFILE_NAME, &[0xff, 0xfe, 0x00]);
    let loaded = load_package(temp.path());
    assert_fail_closed(&loaded, "E-UNSUPPORTED-TOKEN", "is not valid UTF-8");
    assert_eq!(loaded.lockfile_state, LockfileState::Invalid);
}

/// `update_lockfile`: a stale lockfile is not an error outside `--locked`; it
/// is rewritten atomically and reported through `LockfileState::Stale`, and the
/// next load sees it as `Current`.
#[test]
fn stale_lockfile_is_rewritten_without_error() {
    let temp = TempDir::new("lock-stale");
    temp.file(MANIFEST_NAME, &manifest("app", ""));
    temp.file("src/main.rv", "fn main() { }\n");
    let first = load_package(temp.path());
    assert_eq!(first.lockfile_state, LockfileState::Created);
    let lock_before = fs::read(temp.path().join(LOCKFILE_NAME)).expect("lockfile");

    // Changing a source changes the package digest, hence the expected lockfile.
    temp.file("src/main.rv", "fn main() { let value: i32 = 1; }\n");
    let second = load_package(temp.path());
    assert_accepted(&second);
    assert_eq!(second.lockfile_state, LockfileState::Stale);
    assert!(
        !second.diags.has_errors(),
        "a stale lockfile is rewritten, not an error: {}",
        describe(&second)
    );
    assert!(
        !second.diags.iter().any(|diag| diag.code == "E-MOD-LOCK"),
        "no E-MOD-LOCK may be emitted for a stale lockfile outside --locked"
    );
    let lock_after = fs::read(temp.path().join(LOCKFILE_NAME)).expect("rewritten lockfile");
    assert_ne!(
        lock_before, lock_after,
        "the stale lockfile must be rewritten"
    );
    let text = String::from_utf8(lock_after).expect("lockfile is UTF-8");
    assert!(text.contains("lockfile_version = 1"));
    assert!(text.contains("name = \"app\""));

    let third = load_package(temp.path());
    assert_accepted(&third);
    assert_eq!(third.lockfile_state, LockfileState::Current);
}

// ---------------------------------------------------------------------------
// Parse cache: reuse, invalidation and diagnostics replay
// ---------------------------------------------------------------------------

/// [`ParseCache`]: a second load of unchanged files is served from the cache
/// (hits/misses), `clear()` resets it, and a cached parse's *diagnostics* are
/// replayed identically rather than dropped.
#[test]
fn parse_cache_reuses_parses_and_clears() {
    let temp = TempDir::new("parse-cache");
    temp.file(MANIFEST_NAME, &manifest("app", ""));
    temp.file(
        "src/main.rv",
        "mod util;\nfn main() { let value: i32 = util::value(); }\n",
    );
    temp.file("src/util.rv", "pub fn value() -> i32 { return 1; }\n");
    let options = LoadOptions::default();
    let mut cache = ParseCache::new();

    let first = load_package_with_parse_cache(temp.path(), &options, &mut cache);
    assert_accepted(&first);
    assert_eq!(cache.misses(), 2, "main.rv and util.rv are parsed once");
    assert_eq!(cache.hits(), 0);
    assert_eq!(cache.len(), 2);
    assert!(!cache.is_empty());

    let second = load_package_with_parse_cache(temp.path(), &options, &mut cache);
    assert_accepted(&second);
    assert_eq!(cache.hits(), 2, "both files are served from the cache");
    assert_eq!(cache.misses(), 2);

    cache.clear();
    assert!(cache.is_empty(), "clear() must drop every entry");
    assert_eq!(cache.len(), 0);

    let third = load_package_with_parse_cache(temp.path(), &options, &mut cache);
    assert_accepted(&third);
    assert_eq!(cache.hits(), 2, "the clear reset the hit counter path");
    assert_eq!(
        cache.misses(),
        4,
        "both files are parsed again after clear()"
    );

    // A cached parse carries its diagnostics: break util.rv, load twice, and
    // expect the exact same diagnostic stream both times.
    temp.file("src/util.rv", "pub fn value( -> i32 { return 1; }\n");
    let broken_miss = load_package_with_parse_cache(temp.path(), &options, &mut cache);
    assert!(
        broken_miss.diags.has_errors(),
        "the broken module must be refused"
    );
    let first_codes: Vec<String> = broken_miss.diags.iter().map(|d| d.code.clone()).collect();
    assert!(
        first_codes.iter().any(|code| code.starts_with("E-PARSE")),
        "expected a parse diagnostic, got {first_codes:?}"
    );
    assert_eq!(cache.hits(), 3, "main.rv is still a hit, util.rv is a miss");
    assert_eq!(cache.misses(), 5);

    let broken_hit = load_package_with_parse_cache(temp.path(), &options, &mut cache);
    let second_codes: Vec<String> = broken_hit.diags.iter().map(|d| d.code.clone()).collect();
    assert_eq!(
        first_codes, second_codes,
        "a cached parse must replay the same diagnostics"
    );
    assert_eq!(cache.hits(), 5, "both files are now cache hits");
    assert_eq!(cache.misses(), 5);
}

// ---------------------------------------------------------------------------
// Dependency merging and digest determinism
// ---------------------------------------------------------------------------

/// `merge_dependencies`: direct *and transitive* dependency roots are merged as
/// public modules named after the package, sorted, with their own modules
/// expanded and display paths prefixed by the package name.
#[test]
fn dependencies_are_merged_publicly_and_sorted() {
    let temp = TempDir::new("dep-merge");
    // `b` is a leaf; `a` depends on it and has its own file module.
    library_package(&temp, "b", "b");
    temp.file(
        "a/ravum.toml",
        &manifest("a", "\n[dependencies]\nb = { path = \"../b\" }\n"),
    );
    temp.file(
        "a/src/lib.rv",
        "mod sub;\npub fn a() -> i32 { return 1; }\n",
    );
    temp.file("a/src/sub.rv", "pub fn helper() -> i32 { return 2; }\n");
    let app = app_with_dependency(&temp, "a", "../a");
    let loaded = load_package(&app);
    assert_accepted(&loaded);

    let names: Vec<&str> = loaded
        .dependencies
        .iter()
        .map(|dependency| dependency.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec!["a", "b"],
        "direct then transitive, sorted by name"
    );
    for dependency in &loaded.dependencies {
        assert_eq!(dependency.version, "0.1.0");
        assert!(
            dependency.digest.starts_with("sha256:") && dependency.digest.len() == 71,
            "dependency digest must be `sha256:<hex>`: {:?}",
            dependency.digest
        );
    }
    assert!(loaded.dependencies_digest.starts_with("sha256:"));
    assert_eq!(loaded.dependencies_digest.len(), 71);

    for expected in ["a/src/lib.rv", "a/src/sub.rv", "b/src/lib.rv"] {
        assert!(
            loaded.loaded_files.iter().any(|path| path == expected),
            "{expected} must be registered with its package prefix; got {:?}",
            loaded.loaded_files
        );
    }

    let program = loaded.program.as_ref().expect("program");
    let modules: Vec<&ravum_lang_sema::syntax::ast::Module> = program
        .root
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Mod(module) => Some(module),
            _ => None,
        })
        .collect();
    let module_names: Vec<&str> = modules
        .iter()
        .map(|module| {
            module
                .name
                .as_ref()
                .expect("dependency modules are named")
                .name
                .as_str()
        })
        .collect();
    assert_eq!(module_names, vec!["a", "b"]);
    for item in &program.root.items {
        if matches!(item.kind, ItemKind::Mod(_)) {
            assert!(item.vis.is_public(), "dependency modules must be public");
        }
    }
}

/// `package_digest`: the digest is content-only and deterministic — two
/// packages with identical contents in different absolute directories hash the
/// same, a non-`.rv` file under `src/` is ignored, and a real source change
/// changes the digest and staleness.
#[test]
fn dependency_digest_is_content_only_and_deterministic() {
    let temp_one = TempDir::new("digest-one");
    library_package(&temp_one, "util", "util");
    let app_one = app_with_dependency(&temp_one, "util", "../util");
    let first = load_package(&app_one);
    assert_accepted(&first);
    let digest_one = first.dependencies[0].digest.clone();

    // Same relative content, different absolute root directory.
    let temp_two = TempDir::new("digest-two");
    library_package(&temp_two, "util", "util");
    let app_two = app_with_dependency(&temp_two, "util", "../util");
    let second = load_package(&app_two);
    assert_accepted(&second);
    assert_eq!(
        digest_one, second.dependencies[0].digest,
        "the digest must not embed the absolute package path"
    );

    // A non-`.rv` file under `src/` is not part of the package digest.
    temp_one.file("util/src/notes.txt", "not a Ravum source\n");
    let with_notes = load_package(&app_one);
    assert_accepted(&with_notes);
    assert_eq!(
        with_notes.dependencies[0].digest, digest_one,
        "only `.rv` files and ravum.toml belong to the digest"
    );
    assert_eq!(
        with_notes.lockfile_state,
        LockfileState::Current,
        "a non-source file must not stale the lockfile"
    );

    // A real source change does change the digest and stales the lockfile.
    temp_one.file("util/src/lib.rv", "pub fn helper() -> i32 { return 42; }\n");
    let changed = load_package(&app_one);
    assert_accepted(&changed);
    assert_ne!(changed.dependencies[0].digest, digest_one);
    assert_eq!(changed.lockfile_state, LockfileState::Stale);
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// `load_package_with_parse_cache`: a path that cannot be accessed is `E-IO`,
/// and a single file that is not `.rv` is refused with the actionable message
/// instead of being parsed as Ravum.
#[test]
fn entry_point_errors_are_io_errors() {
    let temp = TempDir::new("entry-errors");

    let missing = temp.path().join("does-not-exist");
    let loaded = load_package(&missing);
    assert_fail_closed(&loaded, "E-IO", "cannot access");
    assert!(
        message_of(&loaded, "E-IO").contains("does-not-exist"),
        "the diagnostic must name the inaccessible path"
    );
    assert_eq!(loaded.package_name, None);
    assert_eq!(loaded.crate_kind, None);

    let not_ravum = temp.file("hello.txt", "fn main() { }\n");
    let loaded = load_package(&not_ravum);
    assert_fail_closed(
        &loaded,
        "E-IO",
        "is not a `.rv` file; pass a package directory instead",
    );
    assert_eq!(loaded.crate_kind, Some(CrateKind::File));
    assert_eq!(loaded.package_name.as_deref(), Some("hello"));
}
