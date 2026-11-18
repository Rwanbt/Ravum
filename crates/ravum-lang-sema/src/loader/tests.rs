use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "ravum-loader-{tag}-{}-{}-{nanos}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn file(&self, rel: &str, content: &str) -> PathBuf {
        let path = self.path.join(rel);
        fs::create_dir_all(path.parent().unwrap()).expect("create parent");
        fs::write(&path, content).expect("write file");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn manifest(name: &str, deps: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n\n{deps}")
}

fn codes(diags: &DiagnosticBag) -> Vec<String> {
    diags.iter().map(|d| d.code.clone()).collect()
}

// --- SHA-256 ---------------------------------------------------------

#[test]
fn sha256_known_vectors() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        sha256_hex(b"The quick brown fox jumps over the lazy dog"),
        "d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592"
    );
    assert_eq!(
        sha256_hex(&vec![b'a'; 1_000_000]),
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
    );
    // Block/padding boundaries.
    let mut expected = String::new();
    for len in [55usize, 56, 57, 63, 64, 65, 127, 128, 129] {
        let digest = sha256_hex(&vec![b'x'; len]);
        assert_eq!(digest.len(), 64);
        if len == 64 {
            expected = digest;
        }
    }
    assert!(!expected.is_empty());
}

// --- manifest --------------------------------------------------------

#[test]
fn manifest_accepts_minimal_package() {
    let src = "[package]\nname = \"hello\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n";
    let (manifest, diags) = parse_manifest(src, 0);
    assert!(!diags.has_errors(), "{:?}", codes(&diags));
    let manifest = manifest.expect("manifest");
    assert_eq!(manifest.name, "hello");
    assert_eq!(manifest.version, "0.1.0");
    assert_eq!(manifest.edition, "core-1.0");
    assert!(manifest.dependencies.is_empty());
}

#[test]
fn manifest_accepts_path_dependency() {
    let src = manifest("app", "\n[dependencies]\nutil = { path = \"../util\" }\n");
    let (manifest, diags) = parse_manifest(&src, 0);
    assert!(!diags.has_errors(), "{:?}", codes(&diags));
    let manifest = manifest.expect("manifest");
    assert_eq!(manifest.dependencies.len(), 1);
    assert_eq!(manifest.dependencies[0].name, "util");
    assert_eq!(manifest.dependencies[0].path, "../util");
}

#[test]
fn manifest_rejects_unknown_keys_and_sections() {
    let src = manifest(
        "app",
        "\n[dependencies]\nutil = { path = \"../util\", version = \"1\" }\n",
    );
    let (_, diags) = parse_manifest(&src, 0);
    assert!(codes(&diags).contains(&"E-MOD-DEPENDENCY".to_string()));

    let src = manifest("app", "\n[features]\ndefault = \"x\"\n");
    let (_, diags) = parse_manifest(&src, 0);
    assert!(codes(&diags).contains(&"E-MOD-MANIFEST".to_string()));

    let src = manifest("app", "\n[build]\nscript = \"build.sh\"\n");
    let (_, diags) = parse_manifest(&src, 0);
    assert!(codes(&diags).contains(&"E-MOD-BUILD-SCRIPT".to_string()));

    let src = "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\nlicense = \"MIT\"\n";
    let (_, diags) = parse_manifest(src, 0);
    assert!(codes(&diags).contains(&"E-MOD-MANIFEST".to_string()));
}

#[test]
fn manifest_rejects_invalid_identity_fields() {
    let src = "[package]\nname = \"App\"\nversion = \"01.0.0\"\nedition = \"core-2.0\"\n";
    let (manifest, diags) = parse_manifest(src, 0);
    assert!(manifest.is_none());
    let codes = codes(&diags);
    assert!(codes.contains(&"E-MOD-MANIFEST".to_string()));
    assert!(codes.contains(&"E-UNSUPPORTED-EDITION".to_string()));
}

#[test]
fn manifest_rejects_version_ranges_and_string_dependencies() {
    let src = manifest("app", "\n[dependencies]\nutil = \"^1.0.0\"\n");
    let (_, diags) = parse_manifest(&src, 0);
    assert!(codes(&diags).contains(&"E-MOD-DEPENDENCY".to_string()));
}

#[test]
fn version_validation() {
    assert!(is_valid_version("0.0.0"));
    assert!(is_valid_version("1.2.3"));
    assert!(is_valid_version("10.20.30"));
    assert!(!is_valid_version("1.2"));
    assert!(!is_valid_version("1.2.3.4"));
    assert!(!is_valid_version("01.2.3"));
    assert!(!is_valid_version("1.2.3-alpha"));
    assert!(!is_valid_version("1.2.x"));
    assert!(!is_valid_version(""));
}

#[test]
fn package_name_validation() {
    assert!(is_valid_package_name("hello"));
    assert!(is_valid_package_name("hello-world_2"));
    assert!(!is_valid_package_name("Hello"));
    assert!(!is_valid_package_name("2hello"));
    assert!(!is_valid_package_name("hello/world"));
    assert!(!is_valid_package_name(""));
}

#[test]
fn reserved_windows_components_are_detected() {
    for name in [
        "CON", "con", "Con.rv", "PRN", "AUX", "NUL", "COM1", "LPT9", "aux.txt",
    ] {
        assert!(
            is_reserved_windows_component(name),
            "`{name}` must be reserved"
        );
    }
    for name in [
        "com0", "com10", "lpt0", "console", "foo.rv", "main", "aux_", "_con",
    ] {
        assert!(
            !is_reserved_windows_component(name),
            "`{name}` must not be reserved"
        );
    }
    assert!(is_reserved_windows_component("foo."));
    assert!(is_reserved_windows_component("foo "));
}

#[test]
fn reserved_module_name_is_refused() {
    let temp = TempDir::new("reserved-module");
    temp.file("ravum.toml", &manifest("app", ""));
    temp.file("src/main.rv", "mod con;\nfn main() { }\n");
    let loaded = load_package(&temp.path);
    assert!(codes(&loaded.diags).contains(&"E-MOD-RESERVED-NAME".to_string()));
}

#[test]
fn reserved_dependency_component_is_refused() {
    let temp = TempDir::new("reserved-dep");
    temp.file(
        "app/ravum.toml",
        &manifest("app", "\n[dependencies]\nutil = { path = \"../aux\" }\n"),
    );
    temp.file("app/src/main.rv", "fn main() { }\n");
    let loaded = load_package(&temp.path.join("app"));
    assert!(codes(&loaded.diags).contains(&"E-MOD-RESERVED-NAME".to_string()));
}

// --- package loading -------------------------------------------------

#[test]
fn package_loads_manifest_and_writes_lockfile() {
    let temp = TempDir::new("load");
    temp.file("ravum.toml", &manifest("app", ""));
    temp.file("src/main.rv", "fn main() { }\n");
    let loaded = load_package(&temp.path);
    assert_eq!(loaded.package_name.as_deref(), Some("app"));
    assert_eq!(loaded.package_version.as_deref(), Some("0.1.0"));
    assert_eq!(loaded.crate_kind, Some(CrateKind::Bin));
    assert_eq!(loaded.lockfile_state, LockfileState::Created);
    let lock = temp.path.join(LOCKFILE_NAME);
    assert!(lock.is_file());
    let text = fs::read_to_string(&lock).unwrap();
    assert!(text.contains("lockfile_version = 1"));
    assert!(text.contains("name = \"app\""));
    assert!(text.contains("digest = \"sha256:"));
}

#[test]
fn lockfile_is_deterministic_across_runs() {
    let temp = TempDir::new("lock");
    temp.file("ravum.toml", &manifest("app", ""));
    temp.file("src/main.rv", "fn main() { }\n");
    let first = load_package(&temp.path);
    assert_eq!(first.lockfile_state, LockfileState::Created);
    let bytes_one = fs::read(temp.path.join(LOCKFILE_NAME)).unwrap();
    let second = load_package(&temp.path);
    assert_eq!(second.lockfile_state, LockfileState::Current);
    let bytes_two = fs::read(temp.path.join(LOCKFILE_NAME)).unwrap();
    assert_eq!(bytes_one, bytes_two);
}

#[test]
fn lockfile_changes_when_dependency_changes() {
    let temp = TempDir::new("lockdep");
    temp.file("util/ravum.toml", &manifest("util", ""));
    temp.file("util/src/lib.rv", "pub fn helper() -> i32 { return 1; }\n");
    temp.file(
        "app/ravum.toml",
        &manifest("app", "\n[dependencies]\nutil = { path = \"../util\" }\n"),
    );
    temp.file("app/src/main.rv", "fn main() { }\n");
    let first = load_package(&temp.path.join("app"));
    assert_eq!(first.dependencies.len(), 1);
    let digest_one = first.dependencies_digest.clone();
    let lock_one = fs::read(temp.path.join("app").join(LOCKFILE_NAME)).unwrap();

    temp.file("util/src/lib.rv", "pub fn helper() -> i32 { return 2; }\n");
    let second = load_package(&temp.path.join("app"));
    assert_ne!(digest_one, second.dependencies_digest);
    let lock_two = fs::read(temp.path.join("app").join(LOCKFILE_NAME)).unwrap();
    assert_ne!(lock_one, lock_two);
}

#[test]
fn locked_mode_refuses_missing_lockfile() {
    let temp = TempDir::new("locked");
    temp.file("ravum.toml", &manifest("app", ""));
    temp.file("src/main.rv", "fn main() { }\n");
    let loaded = load_package_with_options(
        &temp.path,
        &LoadOptions {
            locked: true,
            ..Default::default()
        },
    );
    assert_eq!(loaded.lockfile_state, LockfileState::Missing);
    assert!(codes(&loaded.diags).contains(&"E-MOD-MISSING".to_string()));
}

#[test]
fn locked_mode_accepts_current_lockfile() {
    let temp = TempDir::new("locked-ok");
    temp.file("ravum.toml", &manifest("app", ""));
    temp.file("src/main.rv", "fn main() { }\n");
    let _ = load_package(&temp.path);
    let loaded = load_package_with_options(
        &temp.path,
        &LoadOptions {
            locked: true,
            ..Default::default()
        },
    );
    assert_eq!(loaded.lockfile_state, LockfileState::Current);
    assert!(!codes(&loaded.diags).contains(&"E-MOD-LOCK".to_string()));
}

#[test]
fn override_is_analysed_instead_of_the_file_on_disk() {
    let temp = TempDir::new("override");
    temp.file("ravum.toml", &manifest("app", ""));
    temp.file("src/main.rv", "fn main() { }\n");
    let source = std::fs::canonicalize(temp.path.join("src").join("main.rv"))
        .expect("canonical source path");

    let buffered = "fn main() { let unsaved = 1; }\n";
    let options = LoadOptions {
        locked: false,
        overrides: std::iter::once((source.clone(), buffered.to_string())).collect(),
        ..Default::default()
    };
    let loaded = load_package_with_options(&temp.path, &options);
    assert!(!loaded.diags.has_errors(), "{:?}", codes(&loaded.diags));
    let program = loaded.program.expect("program");
    let text = loaded
        .sources
        .file(program.file)
        .expect("root source")
        .text
        .clone();
    assert_eq!(
        text, buffered,
        "the loader must analyse the in-memory buffer"
    );
    assert_eq!(
        std::fs::read_to_string(&source).expect("read disk"),
        "fn main() { }\n",
        "an override must never write the user's file"
    );
}

#[test]
fn override_does_not_change_the_package_digest_on_disk() {
    let temp = TempDir::new("override-digest");
    temp.file("ravum.toml", &manifest("app", ""));
    temp.file("src/main.rv", "fn main() { }\n");
    let source = std::fs::canonicalize(temp.path.join("src").join("main.rv"))
        .expect("canonical source path");
    let clean = load_package(&temp.path);
    let options = LoadOptions {
        locked: false,
        overrides: std::iter::once((source, "fn main() { let x = 2; }\n".to_string())).collect(),
        ..Default::default()
    };
    let overridden = load_package_with_options(&temp.path, &options);
    // A package with no dependency has the empty digest either way; the
    // point is that the override path adds no diagnostic and no extra file.
    assert_eq!(clean.dependencies_digest, overridden.dependencies_digest);
    assert_eq!(clean.loaded_files, overridden.loaded_files);
    assert!(
        !overridden.diags.has_errors(),
        "{:?}",
        codes(&overridden.diags)
    );
}

#[test]
fn override_above_the_file_size_limit_is_refused() {
    let temp = TempDir::new("override-limit");
    temp.file("ravum.toml", &manifest("app", ""));
    temp.file("src/main.rv", "fn main() { }\n");
    let source = std::fs::canonicalize(temp.path.join("src").join("main.rv"))
        .expect("canonical source path");
    let huge = "x".repeat(MAX_FILE_SIZE as usize + 1);
    let options = LoadOptions {
        locked: false,
        overrides: std::iter::once((source, huge)).collect(),
        ..Default::default()
    };
    let loaded = load_package_with_options(&temp.path, &options);
    assert!(codes(&loaded.diags).contains(&"E-LIMIT-FILE-SIZE".to_string()));
}

#[test]
fn locked_mode_refuses_stale_lockfile() {
    let temp = TempDir::new("locked-stale");
    temp.file("ravum.toml", &manifest("app", ""));
    temp.file("src/main.rv", "fn main() { }\n");
    let _ = load_package(&temp.path);
    temp.file("src/main.rv", "fn main() { let x = 1; }\n");
    let loaded = load_package_with_options(
        &temp.path,
        &LoadOptions {
            locked: true,
            ..Default::default()
        },
    );
    assert_eq!(loaded.lockfile_state, LockfileState::Stale);
    assert!(codes(&loaded.diags).contains(&"E-MOD-LOCK".to_string()));
}

#[test]
fn lockfile_unknown_version_is_rejected() {
    let temp = TempDir::new("lock-version");
    temp.file("ravum.toml", &manifest("app", ""));
    temp.file("src/main.rv", "fn main() { }\n");
    temp.file(
            LOCKFILE_NAME,
            "lockfile_version = 99\n\n[[package]]\nname = \"app\"\nversion = \"0.1.0\"\ndigest = \"sha256:00\"\n",
        );
    let loaded = load_package(&temp.path);
    assert_eq!(loaded.lockfile_state, LockfileState::Invalid);
    assert!(codes(&loaded.diags).contains(&"E-MOD-LOCK".to_string()));
}

#[test]
fn missing_manifest_directory_is_rejected() {
    let temp = TempDir::new("no-manifest");
    temp.file("src/main.rv", "fn main() { }\n");
    let loaded = load_package(&temp.path);
    assert!(loaded.program.is_none());
    assert!(codes(&loaded.diags).contains(&"E-MOD-MANIFEST".to_string()));
}

// --- dependency graph security --------------------------------------

fn app_with_dependency(dep_path: &str, dep_name: &str) -> TempDir {
    let temp = TempDir::new("depsec");
    temp.file(
        "app/ravum.toml",
        &manifest(
            "app",
            &format!("\n[dependencies]\n{dep_name} = {{ path = \"{dep_path}\" }}\n"),
        ),
    );
    temp.file("app/src/main.rv", "fn main() { }\n");
    temp
}

#[test]
fn absolute_dependency_path_is_refused() {
    let temp = app_with_dependency("/tmp/util", "util");
    let loaded = load_package(&temp.path.join("app"));
    let codes = codes(&loaded.diags);
    assert!(
        codes.contains(&"E-MOD-PATH-ESCAPE".to_string())
            || codes.contains(&"E-MOD-DEPENDENCY".to_string()),
        "{codes:?}"
    );
}

#[test]
fn dependency_path_above_package_parent_is_path_escape() {
    let temp = TempDir::new("dep-escape");
    temp.file("outside/ravum.toml", &manifest("outside", ""));
    temp.file("outside/src/lib.rv", "pub fn o() -> i32 { return 1; }\n");
    temp.file(
        "nested/app/ravum.toml",
        &manifest(
            "app",
            "\n[dependencies]\noutside = { path = \"../../outside\" }\n",
        ),
    );
    temp.file("nested/app/src/main.rv", "fn main() { }\n");
    let loaded = load_package(&temp.path().join("nested/app"));
    assert!(
        codes(&loaded.diags).contains(&"E-MOD-PATH-ESCAPE".to_string()),
        "codes: {:?}",
        codes(&loaded.diags)
    );
}

#[test]
fn nonexistent_escape_is_path_escape_not_missing() {
    let temp = TempDir::new("dep-escape-missing");
    temp.file(
        "nested/app/ravum.toml",
        &manifest(
            "app",
            "\n[dependencies]\nutil = { path = \"../../outside\" }\n",
        ),
    );
    temp.file("nested/app/src/main.rv", "fn main() { }\n");
    // `../../outside` does not exist; the path policy must win over the
    // existence check.
    let loaded = load_package(&temp.path().join("nested/app"));
    let codes = codes(&loaded.diags);
    assert!(
        codes.contains(&"E-MOD-PATH-ESCAPE".to_string()),
        "codes: {codes:?}"
    );
    assert!(
        !codes.contains(&"E-MOD-DEPENDENCY".to_string()),
        "existence must not be reported before the path policy: {codes:?}"
    );
}

#[test]
fn dependency_path_component_depth_is_bounded() {
    let temp = app_with_dependency("a/a/a/a/a/a/a/a/a", "util");
    let loaded = load_package(&temp.path().join("app"));
    assert!(
        codes(&loaded.diags).contains(&"E-MOD-PATH-ESCAPE".to_string()),
        "codes: {:?}",
        codes(&loaded.diags)
    );
}

#[test]
fn windows_prefixed_dependency_path_is_refused() {
    let temp = app_with_dependency("C:\\\\tmp\\\\util", "util");
    let loaded = load_package(&temp.path.join("app"));
    let codes = codes(&loaded.diags);
    assert!(
        codes.contains(&"E-MOD-PATH-ESCAPE".to_string())
            || codes.contains(&"E-MOD-DEPENDENCY".to_string()),
        "{codes:?}"
    );
}

#[test]
fn nonexistent_dependency_path_is_refused() {
    let temp = app_with_dependency("../nope", "util");
    let loaded = load_package(&temp.path.join("app"));
    assert!(codes(&loaded.diags).contains(&"E-MOD-DEPENDENCY".to_string()));
}

#[test]
fn dependency_name_must_match_target_package() {
    let temp = TempDir::new("depname");
    temp.file("util/ravum.toml", &manifest("other", ""));
    temp.file("util/src/lib.rv", "pub fn helper() -> i32 { return 1; }\n");
    temp.file(
        "app/ravum.toml",
        &manifest("app", "\n[dependencies]\nutil = { path = \"../util\" }\n"),
    );
    temp.file("app/src/main.rv", "fn main() { }\n");
    let loaded = load_package(&temp.path.join("app"));
    assert!(codes(&loaded.diags).contains(&"E-MOD-DEPENDENCY".to_string()));
}

#[test]
fn dependency_without_lib_is_refused() {
    let temp = TempDir::new("deplib");
    temp.file("util/ravum.toml", &manifest("util", ""));
    temp.file("util/src/main.rv", "fn main() { }\n");
    temp.file(
        "app/ravum.toml",
        &manifest("app", "\n[dependencies]\nutil = { path = \"../util\" }\n"),
    );
    temp.file("app/src/main.rv", "fn main() { }\n");
    let loaded = load_package(&temp.path.join("app"));
    assert!(codes(&loaded.diags).contains(&"E-MOD-DEPENDENCY".to_string()));
}

#[test]
fn dependency_cycle_is_refused() {
    let temp = TempDir::new("depcycle");
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
    let loaded = load_package(&temp.path.join("a"));
    assert!(codes(&loaded.diags).contains(&"E-MOD-CYCLE".to_string()));
}

#[test]
fn two_packages_with_same_name_conflict() {
    let temp = TempDir::new("depconflict");
    // `a` and `b` both depend on different packages named `util`.
    temp.file(
        "a/ravum.toml",
        &manifest("a", "\n[dependencies]\nutil = { path = \"../util-a\" }\n"),
    );
    temp.file("a/src/lib.rv", "pub fn a() -> i32 { return 1; }\n");
    temp.file(
        "b/ravum.toml",
        &manifest("b", "\n[dependencies]\nutil = { path = \"../util-b\" }\n"),
    );
    temp.file("b/src/lib.rv", "pub fn b() -> i32 { return 2; }\n");
    temp.file("util-a/ravum.toml", &manifest("util", ""));
    temp.file("util-a/src/lib.rv", "pub fn ua() -> i32 { return 1; }\n");
    temp.file("util-b/ravum.toml", &manifest("util", ""));
    temp.file("util-b/src/lib.rv", "pub fn ub() -> i32 { return 2; }\n");
    temp.file(
        "app/ravum.toml",
        &manifest(
            "app",
            "\n[dependencies]\na = { path = \"../a\" }\nb = { path = \"../b\" }\n",
        ),
    );
    temp.file("app/src/main.rv", "fn main() { }\n");
    let loaded = load_package(&temp.path.join("app"));
    assert!(codes(&loaded.diags).contains(&"E-MOD-DUPLICATE".to_string()));
}

#[cfg(unix)]
#[test]
fn symlinked_dependency_is_refused() {
    let temp = TempDir::new("depsymlink");
    fs::create_dir_all(temp.path.join("real")).unwrap();
    temp.file("real/ravum.toml", &manifest("util", ""));
    temp.file("real/src/lib.rv", "pub fn u() -> i32 { return 1; }\n");
    std::os::unix::fs::symlink(temp.path.join("real"), temp.path.join("link"))
        .expect("create symlink");
    temp.file(
        "app/ravum.toml",
        &manifest("app", "\n[dependencies]\nutil = { path = \"../link\" }\n"),
    );
    temp.file("app/src/main.rv", "fn main() { }\n");
    let loaded = load_package(&temp.path.join("app"));
    assert!(codes(&loaded.diags).contains(&"E-MOD-PATH".to_string()));
}

#[test]
fn directory_with_one_rv_file_uses_single_file_mode() {
    let temp = TempDir::new("lone-rv");
    temp.file("solo.rv", "fn main() { }\n");
    let loaded = load_package(temp.path());
    assert_eq!(loaded.crate_kind, Some(CrateKind::File));
    assert_eq!(loaded.package_name.as_deref(), Some("solo"));
    assert_eq!(loaded.lockfile_state, LockfileState::NotApplicable);
}

#[test]
fn directory_with_several_rv_files_without_manifest_is_ambiguous() {
    let temp = TempDir::new("two-rv");
    temp.file("a.rv", "fn main() { }\n");
    temp.file("b.rv", "fn main() { }\n");
    let loaded = load_package(temp.path());
    assert!(codes(&loaded.diags).contains(&"E-MOD-MANIFEST".to_string()));
}

#[test]
fn single_file_mode_loads_without_manifest() {
    let temp = TempDir::new("single");
    let file = temp.file("hello.rv", "fn main() { }\n");
    let loaded = load_package(&file);
    assert_eq!(loaded.crate_kind, Some(CrateKind::File));
    assert_eq!(loaded.package_name.as_deref(), Some("hello"));
    assert_eq!(loaded.lockfile_state, LockfileState::NotApplicable);
    assert_eq!(loaded.dependencies_digest, empty_dependencies_digest());
}

// --- module resolution (real parser) ---------------------------------

fn has_mod_decl(module: &Module) -> bool {
    module.items.iter().any(|item| match &item.kind {
        ItemKind::Mod(inner) => has_mod_decl(inner),
        ItemKind::ModDecl(_) => true,
        _ => false,
    })
}

fn module_named<'a>(module: &'a Module, name: &str) -> Option<&'a Module> {
    module.items.iter().find_map(|item| match &item.kind {
        ItemKind::Mod(inner) if inner.name.as_ref().map(|n| n.name.as_str()) == Some(name) => {
            Some(inner)
        }
        _ => None,
    })
}

#[test]
fn module_files_are_expanded_into_inline_modules() {
    let temp = TempDir::new("modules");
    temp.file("ravum.toml", &manifest("app", ""));
    temp.file(
        "src/main.rv",
        "mod util;\nmod geo {\n    mod point;\n}\nfn main() { }\n",
    );
    temp.file("src/util.rv", "pub fn helper() -> i32 { return 1; }\n");
    temp.file("src/geo/point.rv", "pub struct Point { pub x: i32 }\n");

    let loaded = load_package(&temp.path);
    assert!(
        !loaded.diags.has_errors(),
        "loader errors: {:?}",
        codes(&loaded.diags)
    );
    let program = loaded.program.expect("parsed program");
    assert!(
        !has_mod_decl(&program.root),
        "ModDecl leaked to the checker"
    );
    let util = module_named(&program.root, "util").expect("util module");
    assert!(matches!(
        util.items.first().map(|item| &item.kind),
        Some(ItemKind::Fn(_))
    ));
    let geo = module_named(&program.root, "geo").expect("inline geo module");
    assert!(module_named(geo, "point").is_some(), "nested file module");
    assert!(loaded
        .loaded_files
        .iter()
        .any(|path| path == "src/geo/point.rv"));
}

#[test]
fn both_crate_roots_are_refused() {
    let temp = TempDir::new("both-roots");
    temp.file("ravum.toml", &manifest("app", ""));
    temp.file("src/main.rv", "fn main() { }\n");
    temp.file("src/lib.rv", "pub fn helper() -> i32 { return 1; }\n");
    let loaded = load_package(&temp.path);
    assert!(codes(&loaded.diags).contains(&"E-MOD-BOTH-ROOTS".to_string()));
    assert!(loaded.program.is_none());
    assert!(loaded.lib_program.is_none());
}

#[test]
fn ambiguous_module_file_is_refused() {
    let temp = TempDir::new("ambiguous");
    temp.file("ravum.toml", &manifest("app", ""));
    temp.file("src/main.rv", "mod util;\nfn main() { }\n");
    temp.file("src/util.rv", "pub fn a() -> i32 { return 1; }\n");
    temp.file("src/util/mod.rv", "pub fn b() -> i32 { return 2; }\n");
    let loaded = load_package(&temp.path);
    assert!(codes(&loaded.diags).contains(&"E-MOD-AMBIGUOUS".to_string()));
    assert!(loaded.program.is_none());
}

#[test]
fn missing_module_file_is_refused() {
    let temp = TempDir::new("missing-module");
    temp.file("ravum.toml", &manifest("app", ""));
    temp.file("src/main.rv", "mod absent;\nfn main() { }\n");
    let loaded = load_package(&temp.path);
    assert!(codes(&loaded.diags).contains(&"E-MOD-MISSING".to_string()));
}

#[test]
fn duplicate_module_file_is_refused() {
    let temp = TempDir::new("duplicate-module");
    temp.file("ravum.toml", &manifest("app", ""));
    temp.file("src/main.rv", "mod a;\nmod b;\nfn main() { }\n");
    temp.file("src/a.rv", "mod shared;\npub fn a() -> i32 { return 1; }\n");
    temp.file("src/b.rv", "mod shared;\npub fn b() -> i32 { return 2; }\n");
    temp.file("src/shared.rv", "pub fn s() -> i32 { return 3; }\n");
    let loaded = load_package(&temp.path);
    assert!(codes(&loaded.diags).contains(&"E-MOD-DUPLICATE".to_string()));
}

#[test]
fn module_cycle_surfaces_as_duplicate_file() {
    let temp = TempDir::new("module-cycle");
    temp.file("ravum.toml", &manifest("app", ""));
    temp.file("src/main.rv", "mod a;\nfn main() { }\n");
    temp.file("src/a.rv", "mod b;\npub fn a() -> i32 { return 1; }\n");
    temp.file("src/b.rv", "mod a;\npub fn b() -> i32 { return 2; }\n");
    let loaded = load_package(&temp.path);
    assert!(codes(&loaded.diags).contains(&"E-MOD-DUPLICATE".to_string()));
}

#[test]
fn dependency_root_is_merged_as_public_module() {
    let temp = TempDir::new("dep-merge");
    temp.file("util/ravum.toml", &manifest("util", ""));
    temp.file("util/src/lib.rv", "pub fn helper() -> i32 { return 1; }\n");
    temp.file(
        "app/ravum.toml",
        &manifest("app", "\n[dependencies]\nutil = { path = \"../util\" }\n"),
    );
    temp.file("app/src/main.rv", "use util::helper;\nfn main() { }\n");
    let loaded = load_package(&temp.path.join("app"));
    assert!(
        !loaded.diags.has_errors(),
        "loader errors: {:?}",
        codes(&loaded.diags)
    );
    let program = loaded.program.expect("parsed program");
    let util = module_named(&program.root, "util").expect("dependency module");
    assert!(util
        .items
        .iter()
        .any(|item| matches!(&item.kind, ItemKind::Fn(_))));
    assert_eq!(loaded.dependencies.len(), 1);
    assert!(loaded.dependencies_digest.starts_with("sha256:"));
}

#[test]
fn digest_encoding_is_stable_for_crlf() {
    let temp = TempDir::new("crlf");
    temp.file("ravum.toml", &manifest("app", ""));
    temp.file("src/main.rv", "fn main() { }\n");
    let lf = package_digest(&temp.path, &mut DiagnosticBag::new()).unwrap();
    temp.file("src/main.rv", "fn main() { }\r\n");
    let crlf = package_digest(&temp.path, &mut DiagnosticBag::new()).unwrap();
    assert_eq!(lf, crlf);
}

#[test]
fn digest_hashes_nested_ravum_files_in_relative_path_order() {
    let temp = TempDir::new("digest-order");
    temp.file("ravum.toml", "manifest\n");
    temp.file("src/z.rv", "z\n");
    temp.file("src/module/a.rv", "a\n");
    temp.file("src/module/notes.txt", "ignored");
    let mut diags = DiagnosticBag::new();
    let digest = package_digest(temp.path(), &mut diags).expect("package digest");
    let encoded = concat!(
        "ravum.toml\0",
        "9\0manifest\n",
        "src/module/a.rv\0",
        "2\0a\n",
        "src/z.rv\0",
        "2\0z\n",
    );
    let expected = sha256_hex(encoded.as_bytes());
    assert_eq!(digest, format!("sha256:{expected}"));
    assert!(!diags.has_errors());
}

#[test]
fn digest_reports_missing_manifest_and_non_directory_source() {
    let temp = TempDir::new("digest-io");
    let mut diags = DiagnosticBag::new();
    assert_eq!(package_digest(temp.path(), &mut diags), None);
    assert_eq!(codes(&diags), ["E-IO"]);

    temp.file("ravum.toml", &manifest("app", ""));
    temp.file("src", "not a directory");
    let mut diags = DiagnosticBag::new();
    assert_eq!(package_digest(temp.path(), &mut diags), None);
    assert_eq!(codes(&diags), ["E-IO"]);
}

#[test]
fn digest_refuses_oversized_manifest_and_source() {
    let temp = TempDir::new("digest-file-size");
    let manifest = temp.file("ravum.toml", &manifest("app", ""));
    fs::OpenOptions::new()
        .write(true)
        .open(&manifest)
        .unwrap()
        .set_len(MAX_FILE_SIZE + 1)
        .unwrap();

    let mut diags = DiagnosticBag::new();
    assert_eq!(package_digest(temp.path(), &mut diags), None);
    assert_eq!(codes(&diags), ["E-LIMIT-FILE-SIZE"]);
    fs::write(manifest, "[package]\n").unwrap();

    fs::File::create(temp.file("src/main.rv", ""))
        .unwrap()
        .set_len(MAX_FILE_SIZE + 1)
        .unwrap();
    let mut diags = DiagnosticBag::new();
    assert_eq!(package_digest(temp.path(), &mut diags), None);
    assert_eq!(codes(&diags), ["E-LIMIT-FILE-SIZE"]);
}

#[test]
fn digest_refuses_a_package_above_its_ravum_file_limit() {
    let temp = TempDir::new("digest-file-count");
    temp.file("ravum.toml", &manifest("app", ""));
    let source = temp.path().join("src");
    fs::create_dir(&source).unwrap();
    for index in 0..=MAX_PACKAGE_FILES {
        fs::write(source.join(format!("{index}.rv")), b"").unwrap();
    }

    let mut diags = DiagnosticBag::new();
    assert_eq!(package_digest(temp.path(), &mut diags), None);
    assert_eq!(codes(&diags), ["E-LIMIT-FILES"]);
}

#[cfg(unix)]
#[test]
fn digest_refuses_symlinks_at_each_package_boundary() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new("digest-symlinks");
    let other = TempDir::new("digest-symlink-targets");
    let manifest_target = other.file("alternate.toml", &manifest("app", ""));
    let source_target = other.file("payload/real.rv", "fn main() {}");
    let manifest_link = temp.path().join("ravum.toml");
    let source_link = temp.path().join("src");
    symlink(&manifest_target, &manifest_link).unwrap();
    let mut diags = DiagnosticBag::new();
    assert_eq!(package_digest(temp.path(), &mut diags), None);
    assert_eq!(codes(&diags), ["E-MOD-PATH"]);

    fs::remove_file(manifest_link).unwrap();
    temp.file("ravum.toml", &manifest("app", ""));
    symlink(source_target.parent().unwrap(), &source_link).unwrap();
    let mut diags = DiagnosticBag::new();
    assert_eq!(package_digest(temp.path(), &mut diags), None);
    assert_eq!(codes(&diags), ["E-MOD-PATH"]);

    fs::remove_file(&source_link).unwrap();
    fs::create_dir(&source_link).unwrap();
    symlink(source_target, source_link.join("entry.rv")).unwrap();
    let mut diags = DiagnosticBag::new();
    assert_eq!(package_digest(temp.path(), &mut diags), None);
    assert_eq!(codes(&diags), ["E-MOD-PATH"]);
}

#[cfg(unix)]
#[test]
fn digest_refuses_reserved_windows_names_inside_nested_sources() {
    let temp = TempDir::new("digest-portability");
    temp.file("ravum.toml", &manifest("app", ""));
    temp.file("src/nested/CON.rv", "");
    let mut diags = DiagnosticBag::new();
    assert_eq!(package_digest(temp.path(), &mut diags), None);
    assert_eq!(codes(&diags), ["E-MOD-RESERVED-NAME"]);
}

#[test]
fn digest_fails_closed_when_a_source_cannot_be_read_or_inspected() {
    use std::io::{Error, ErrorKind};

    let temp = TempDir::new("digest-source-io");
    temp.file("ravum.toml", &manifest("app", ""));
    let source = temp.file("src/main.rv", "fn main() {}");

    let mut diags = DiagnosticBag::new();
    let read_failure = package_digest_with(
        temp.path(),
        &mut diags,
        |path| {
            if path == source {
                Err(Error::from(ErrorKind::PermissionDenied))
            } else {
                fs::read(path)
            }
        },
        |path| fs::symlink_metadata(path),
    );
    assert_eq!(read_failure, None);
    assert_eq!(codes(&diags), ["E-IO"]);
    assert!(diags
        .iter()
        .any(|diag| diag.message.contains("cannot read")));

    let mut diags = DiagnosticBag::new();
    let inspect_failure = package_digest_with(
        temp.path(),
        &mut diags,
        |path| fs::read(path),
        |path| {
            assert_eq!(path, source);
            Err(Error::from(ErrorKind::NotFound))
        },
    );
    assert_eq!(inspect_failure, None);
    assert_eq!(codes(&diags), ["E-IO"]);
    assert!(diags
        .iter()
        .any(|diag| diag.message.contains("cannot inspect")));
}

/// T4: the source-file ceiling is exercised without creating 65 536 files.
/// The limit is lowered thread-locally, so no other test is affected.
#[test]
fn the_source_file_limit_is_refused_with_the_existing_diagnostic() {
    let temp = TempDir::new("file-limit");
    temp.file("ravum.toml", &manifest("file-limit", ""));
    temp.file("src/main.rv", "mod a;\nmod b;\nfn main() { }\n");
    temp.file("src/a.rv", "pub fn a() -> i32 { return 1; }\n");
    temp.file("src/b.rv", "pub fn b() -> i32 { return 2; }\n");
    TEST_SOURCE_FILE_LIMIT.with(|limit| limit.set(Some(3)));
    let loaded = load_package_with_options(
        &temp.path,
        &LoadOptions {
            locked: false,
            overrides: std::collections::BTreeMap::new(),
            ..Default::default()
        },
    );
    TEST_SOURCE_FILE_LIMIT.with(|limit| limit.set(None));
    let found = codes(&loaded.diags);
    assert!(
        found.iter().any(|code| code == "E-LIMIT-FILES"),
        "expected E-LIMIT-FILES, got {found:?}"
    );
    let message = loaded
        .diags
        .iter()
        .find(|diag| diag.code == "E-LIMIT-FILES")
        .map(|diag| diag.message.clone())
        .unwrap_or_default();
    assert!(
        message.contains("more than 3 source files"),
        "the diagnostic must name the effective limit: {message}"
    );
}

// --- T5: session file ids -------------------------------------------

#[test]
fn session_file_ids_survive_a_file_added_before_the_existing_ones() {
    let temp = TempDir::new("session-ids");
    temp.file("ravum.toml", &manifest("app", ""));
    temp.file(
        "src/main.rv",
        "mod util;\nfn main() { let x: i32 = util::value(); }\n",
    );
    temp.file("src/util.rv", "pub fn value() -> i32 { return 1; }\n");
    let main_path = fs::canonicalize(temp.path.join("src/main.rv")).expect("main");
    let util_path = fs::canonicalize(temp.path.join("src/util.rv")).expect("util");
    let manifest_path = fs::canonicalize(temp.path.join("ravum.toml")).expect("manifest");

    let mut cache = ParseCache::new();
    let first = load_package_with_parse_cache(
        &temp.path,
        &LoadOptions {
            file_ids: Some(BTreeMap::new()),
            ..Default::default()
        },
        &mut cache,
    );
    assert!(
        !first.diags.has_errors(),
        "loader errors: {:?}",
        codes(&first.diags)
    );
    assert!(
        first.file_ids.contains_key(&manifest_path),
        "the manifest is a source file of the package"
    );
    let table = first.file_ids.clone();
    let (manifest_id, main_id, util_id) =
        (table[&manifest_path], table[&main_path], table[&util_path]);
    let max_id = *table.values().max().expect("non-empty table");
    assert_eq!(cache.misses(), 2, "main and util are parsed once");
    assert_eq!(first.program.expect("program").file, main_id);

    // `aa` is declared before `util`: a dense allocation would give it
    // util's id and renumber every file after it.
    temp.file("src/aa.rv", "pub fn extra() -> i32 { return 2; }\n");
    temp.file(
        "src/main.rv",
        "mod aa;\nmod util;\nfn main() { let x: i32 = aa::extra() + util::value(); }\n",
    );
    let second = load_package_with_parse_cache(
        &temp.path,
        &LoadOptions {
            file_ids: Some(table.clone()),
            ..Default::default()
        },
        &mut cache,
    );
    assert!(
        !second.diags.has_errors(),
        "loader errors: {:?}",
        codes(&second.diags)
    );
    let aa_path = fs::canonicalize(temp.path.join("src/aa.rv")).expect("aa");
    assert_eq!(second.file_ids[&manifest_path], manifest_id);
    assert_eq!(second.file_ids[&main_path], main_id);
    assert_eq!(second.file_ids[&util_path], util_id);
    assert_eq!(
        second.file_ids[&aa_path],
        max_id + 1,
        "a new file takes the next id, it does not reuse or shift an old one"
    );
    assert_eq!(cache.hits(), 1, "util.rv is served from the cache");
    assert_eq!(cache.misses(), 4, "main and aa are parsed");
    assert_eq!(second.program.expect("program").file, main_id);
    assert_eq!(
        second.sources.file(util_id).expect("util").path,
        "src/util.rv"
    );
}

#[test]
fn a_session_table_that_maps_one_id_to_two_files_fails_closed() {
    let temp = TempDir::new("session-conflict");
    temp.file("ravum.toml", &manifest("app", ""));
    temp.file(
        "src/main.rv",
        "mod util;\nfn main() { let x: i32 = util::value(); }\n",
    );
    temp.file("src/util.rv", "pub fn value() -> i32 { return 1; }\n");
    let main_path = fs::canonicalize(temp.path.join("src/main.rv")).expect("main");
    let util_path = fs::canonicalize(temp.path.join("src/util.rv")).expect("util");
    // Both files claim id 1: every span parsed for id 1 would point into the
    // wrong file, so the loader refuses instead of reusing the id.
    let mut table = BTreeMap::new();
    table.insert(main_path, 1);
    table.insert(util_path, 1);
    let loaded = load_package_with_options(
        &temp.path,
        &LoadOptions {
            file_ids: Some(table),
            ..Default::default()
        },
    );
    let found = codes(&loaded.diags);
    assert!(
        found.iter().any(|code| code == "E-IO"),
        "expected the fail-closed E-IO diagnostic, got {found:?}"
    );
    assert!(
        loaded.program.is_none(),
        "a rejected load hands no program to the checker"
    );
}

#[test]
fn bounded_text_read_rejects_missing_directories_and_invalid_utf8() {
    let temp = TempDir::new("bounded-read");
    let invalid = temp.path().join("invalid.rv");
    fs::write(&invalid, [0xff]).unwrap();

    for (path, diagnostic, message) in [
        (temp.path().join("missing.rv"), "E-IO", "does not exist"),
        (temp.path().to_path_buf(), "E-IO", "not a regular file"),
        (invalid, "E-UNSUPPORTED-TOKEN", "not valid UTF-8"),
    ] {
        let mut diags = DiagnosticBag::new();
        assert_eq!(read_text_limited(&path, 32, "E-IO", &mut diags), None);
        assert_eq!(codes(&diags), [diagnostic]);
        assert!(diags.iter().any(|diag| diag.message.contains(message)));
    }
}

#[test]
fn atomic_write_cleans_its_temporary_file_when_rename_fails() {
    let temp = TempDir::new("atomic-rename");
    let destination = temp.path().join("ravum.lock");
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("sentinel"), b"preserved").unwrap();

    assert!(atomic_write(&destination, b"replacement").is_err());
    assert_eq!(
        fs::read(destination.join("sentinel")).unwrap(),
        b"preserved"
    );
    let entries = fs::read_dir(temp.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(entries, [destination]);
}

#[cfg(unix)]
#[test]
fn path_guards_report_inspection_errors_beneath_regular_files() {
    let temp = TempDir::new("path-guard-io");
    let file = temp.file("regular.rv", "fn main() {}");
    let target = file.join("child.rv");
    let mut diags = DiagnosticBag::new();
    assert_eq!(read_text_limited(&target, 32, "E-IO", &mut diags), None);
    assert_eq!(codes(&diags), ["E-IO"]);
    assert!(diags
        .iter()
        .any(|diag| diag.message.contains("cannot inspect")));

    let mut diags = DiagnosticBag::new();
    assert!(!check_lexical_no_symlinks(temp.path(), &target, &mut diags));
    assert_eq!(codes(&diags), ["E-IO"]);

    let mut diags = DiagnosticBag::new();
    assert_eq!(
        check_path_no_symlinks(temp.path(), &target, &mut diags),
        None
    );
    assert_eq!(codes(&diags), ["E-IO"]);
}

#[cfg(unix)]
#[test]
fn path_guards_refuse_symlink_components_even_inside_the_base() {
    let temp = TempDir::new("path-guard-link");
    let source = temp.file("actual/main.rv", "fn main() {}");
    let link = temp.path().join("alias");
    std::os::unix::fs::symlink(source.parent().unwrap(), &link).unwrap();
    let target = link.join("main.rv");

    let mut diags = DiagnosticBag::new();
    assert!(!check_lexical_no_symlinks(temp.path(), &target, &mut diags));
    assert_eq!(codes(&diags), ["E-MOD-PATH"]);

    let mut diags = DiagnosticBag::new();
    assert_eq!(
        check_path_no_symlinks(temp.path(), &target, &mut diags),
        None
    );
    assert_eq!(codes(&diags), ["E-MOD-PATH"]);
}

#[test]
fn lockfile_rejects_directories_and_oversized_files() {
    let temp = TempDir::new("lockfile-shape");
    temp.file("ravum.toml", &manifest("app", ""));
    temp.file("src/main.rv", "fn main() {}");
    let path = temp.path().join(LOCKFILE_NAME);
    fs::create_dir(&path).unwrap();

    let loaded = load_package(temp.path());
    assert_eq!(loaded.lockfile_state, LockfileState::Invalid);
    assert!(codes(&loaded.diags).contains(&"E-MOD-LOCK".to_string()));
    assert!(path.is_dir());

    fs::remove_dir(&path).unwrap();
    fs::File::create(&path)
        .unwrap()
        .set_len(MAX_LOCKFILE_SIZE + 1)
        .unwrap();
    let loaded = load_package(temp.path());
    assert_eq!(loaded.lockfile_state, LockfileState::Invalid);
    assert!(codes(&loaded.diags).contains(&"E-LIMIT-FILE-SIZE".to_string()));
    assert_eq!(fs::metadata(path).unwrap().len(), MAX_LOCKFILE_SIZE + 1);
}

#[test]
fn lockfile_reports_create_and_update_failures_without_replacing_existing_data() {
    let temp = TempDir::new("lockfile-write");
    temp.file("ravum.toml", &manifest("app", ""));
    temp.file("src/main.rv", "fn main() {}");
    let blocked = temp
        .path()
        .join(format!(".{LOCKFILE_NAME}.tmp-{}", std::process::id()));
    fs::create_dir(&blocked).unwrap();

    let loaded = load_package(temp.path());
    assert_eq!(loaded.lockfile_state, LockfileState::Invalid);
    assert!(loaded
        .diags
        .iter()
        .any(|d| d.code == "E-MOD-LOCK" && d.message.contains("cannot write")));
    let lockfile = temp.path().join(LOCKFILE_NAME);
    assert!(!lockfile.exists());

    let stale = "lockfile_version = 1\n";
    fs::write(&lockfile, stale).unwrap();
    let loaded = load_package(temp.path());
    assert_eq!(loaded.lockfile_state, LockfileState::Invalid);
    assert!(loaded
        .diags
        .iter()
        .any(|d| d.code == "E-MOD-LOCK" && d.message.contains("cannot update")));
    assert_eq!(fs::read_to_string(lockfile).unwrap(), stale);
}

#[cfg(unix)]
#[test]
fn lockfile_symlink_is_refused_without_touching_its_target() {
    let temp = TempDir::new("lockfile-link");
    temp.file("ravum.toml", &manifest("app", ""));
    temp.file("src/main.rv", "fn main() {}");
    let original = "must not be rewritten";
    let target = temp.file("other.lock", original);
    let link = temp.path().join(LOCKFILE_NAME);
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let loaded = load_package(temp.path());
    assert_eq!(loaded.lockfile_state, LockfileState::Invalid);
    assert!(codes(&loaded.diags).contains(&"E-MOD-PATH".to_string()));
    assert_eq!(fs::read_link(link).unwrap(), target);
    assert_eq!(fs::read_to_string(target).unwrap(), original);
}

#[test]
fn package_refuses_a_directory_as_a_crate_root() {
    let temp = TempDir::new("directory-crate-root");
    temp.file("ravum.toml", &manifest("app", ""));
    fs::create_dir_all(temp.path().join("src/main.rv")).unwrap();

    let loaded = load_package(temp.path());
    assert!(loaded.program.is_none());
    assert!(loaded
        .diags
        .iter()
        .any(|diag| { diag.code == "E-IO" && diag.message.contains("is not a regular file") }));
}

#[test]
fn package_refuses_reserved_inline_module_directory_components() {
    let temp = TempDir::new("reserved-inline-module");
    temp.file("ravum.toml", &manifest("app", ""));
    temp.file("src/main.rv", "mod CON { mod child; } fn main() {}");

    let loaded = load_package(temp.path());
    assert!(loaded.diags.iter().any(|diag| {
        diag.code == "E-MOD-RESERVED-NAME" && diag.message.contains("inline module path component")
    }));
}

#[cfg(unix)]
#[test]
fn crate_root_probe_refuses_links_and_reports_inspection_errors() {
    let temp = TempDir::new("crate-root-probe");
    let source = temp.file("real.rv", "fn main() {}");
    let link = temp.path().join("linked.rv");
    std::os::unix::fs::symlink(&source, &link).unwrap();

    let mut diags = DiagnosticBag::new();
    assert_eq!(regular_file(&link, &mut diags), Err(()));
    assert_eq!(codes(&diags), ["E-MOD-PATH"]);

    let mut diags = DiagnosticBag::new();
    assert_eq!(regular_file(&source.join("child.rv"), &mut diags), Err(()));
    assert_eq!(codes(&diags), ["E-IO"]);
}

#[test]
fn dependency_graph_rejects_chains_beyond_the_package_depth_limit() {
    let temp = TempDir::new("dependency-graph-depth");
    for depth in 0..=MAX_PACKAGE_DEPTH + 1 {
        let name = format!("pkg{depth}");
        let deps = if depth <= MAX_PACKAGE_DEPTH {
            let next = format!("pkg{}", depth + 1);
            format!("[dependencies]\n{next} = {{ path = \"../{next}\" }}\n")
        } else {
            String::new()
        };
        temp.file(&format!("{name}/ravum.toml"), &manifest(&name, &deps));
        temp.file(&format!("{name}/src/lib.rv"), "pub fn value() -> i32 { 1 }");
    }

    let loaded = load_package(&temp.path().join("pkg0"));
    assert!(loaded.program.is_none());
    assert!(loaded.diags.iter().any(|diag| {
        diag.code == "E-MOD-PATH-ESCAPE" && diag.message.contains("dependency graph deeper")
    }));
}

#[test]
fn dependency_graph_rejects_an_alias_of_an_already_loaded_package() {
    let temp = TempDir::new("dependency-shared-alias");
    temp.file(
        "app/ravum.toml",
        &manifest(
            "app",
            "[dependencies]\na = { path = \"../a\" }\nz = { path = \"../a\" }\n",
        ),
    );
    temp.file("app/src/main.rv", "fn main() {}");
    temp.file("a/ravum.toml", &manifest("a", ""));
    temp.file("a/src/lib.rv", "pub fn value() -> i32 { 1 }");

    let loaded = load_package(&temp.path().join("app"));
    assert!(loaded.program.is_none());
    assert!(loaded.diags.iter().any(|diag| {
        diag.code == "E-MOD-DEPENDENCY"
            && diag
                .message
                .contains("dependency key `z` resolves to package `a`")
    }));
}

#[test]
fn dependency_graph_refuses_a_directory_as_a_library_source() {
    let temp = TempDir::new("dependency-library-shape");
    temp.file(
        "app/ravum.toml",
        &manifest("app", "[dependencies]\na = { path = \"../a\" }\n"),
    );
    temp.file("app/src/main.rv", "fn main() {}");
    temp.file("a/ravum.toml", &manifest("a", ""));
    fs::create_dir_all(temp.path().join("a/src/lib.rv")).unwrap();

    let loaded = load_package(&temp.path().join("app"));
    assert!(loaded.program.is_none());
    assert!(loaded.diags.iter().any(|diag| {
        diag.code == "E-MOD-DEPENDENCY" && diag.message.contains("is not a regular file")
    }));
}

#[test]
fn package_supports_module_directory_without_a_sibling_file() {
    let temp = TempDir::new("module-directory");
    temp.file("ravum.toml", &manifest("app", ""));
    temp.file("src/main.rv", "mod util; fn main() {}");
    temp.file("src/util/mod.rv", "pub fn helper() -> i32 { 1 }");

    let loaded = load_package(temp.path());
    assert!(!loaded.diags.has_errors(), "{:?}", loaded.diags);
    let program = loaded.program.unwrap();
    assert!(program.root.items.iter().any(|item| {
        matches!(&item.kind, ItemKind::Mod(module)
            if module.name.as_ref().is_some_and(|name| name.name == "util"))
    }));
    assert!(loaded
        .loaded_files
        .iter()
        .any(|path| path == "src/util/mod.rv"));
}

#[test]
fn package_refuses_a_directory_as_a_library_root() {
    let temp = TempDir::new("directory-library-root");
    temp.file("ravum.toml", &manifest("app", ""));
    fs::create_dir_all(temp.path().join("src/lib.rv")).unwrap();

    let loaded = load_package(temp.path());
    assert!(loaded.program.is_none());
    assert!(loaded
        .diags
        .iter()
        .any(|diag| { diag.code == "E-IO" && diag.message.contains("is not a regular file") }));
}

#[cfg(unix)]
#[test]
fn module_discovery_reports_inspection_errors_for_both_source_layouts() {
    for (source, obstruction, expected_path) in [
        (
            "mod outer { mod leaf; } fn main() {}",
            "src/outer",
            "outer/leaf.rv",
        ),
        ("mod leaf; fn main() {}", "src/leaf", "leaf/mod.rv"),
    ] {
        let temp = TempDir::new("module-inspection");
        temp.file("ravum.toml", &manifest("app", ""));
        temp.file("src/main.rv", source);
        temp.file(obstruction, "regular file blocking a directory path");

        let loaded = load_package(temp.path());
        assert!(loaded.program.is_none());
        assert!(loaded.diags.iter().any(|diag| {
            diag.code == "E-IO"
                && diag.message.contains("cannot inspect")
                && diag.message.contains(expected_path)
        }));
    }
}

#[test]
fn source_loaders_report_disappearing_inputs_at_canonicalization() {
    let temp = TempDir::new("disappeared-input");
    let missing = temp.path().join("missing.rv");
    let options = LoadOptions::default();
    let mut cache = ParseCache::default();
    for loaded in [
        load_single_file(&missing, &options, &mut cache),
        load_package_dir(&missing, &options, &mut cache),
    ] {
        assert!(loaded.program.is_none());
        assert_eq!(codes(&loaded.diags), ["E-IO"]);
        assert!(loaded
            .diags
            .iter()
            .any(|diag| diag.message.contains("cannot canonicalize")));
    }
}

#[test]
fn registering_a_source_twice_preserves_its_original_identity_and_text() {
    let temp = TempDir::new("source-registration");
    let path = temp.path().join("test.rv");
    let mut sources = SourceMap::new();
    let mut ids = BTreeMap::new();
    let mut next = 0;
    let mut diags = DiagnosticBag::new();
    let mut register = |display: &str, text: &str| {
        register_source(
            &mut sources,
            &mut ids,
            &mut next,
            &path,
            display.to_string(),
            text.to_string(),
            &mut diags,
        )
    };

    let id = register("test.rv", "original").unwrap();
    assert_eq!(register("test.rv", "replacement"), Some(id));
    assert_eq!(register("different.rv", "unrelated"), None);
    assert_eq!(codes(&diags), ["E-IO"]);
    assert_eq!(sources.file(id).unwrap().text, "original");
}

/// Defensive refusals of the module loader that a full load never reaches,
/// because the package digest refuses every symlink under `src/` first. They
/// guard a file swapped between the digest walk and the module load; the test
/// drives the loader functions directly on such a state.
#[cfg(unix)]
mod loader_after_digest {
    use super::*;
    use crate::loader::crates::{expand_module, load_crate_root, resolve_module_file, CrateCtx};

    fn with_ctx(src_root: &Path, run: impl FnOnce(&mut CrateCtx<'_>)) -> Vec<String> {
        let mut sources = SourceMap::new();
        let mut diags = DiagnosticBag::new();
        let mut file_ids = BTreeMap::new();
        let mut next_file_id = 0;
        let mut synthetic = 0u64;
        let mut parse_time = Duration::ZERO;
        let mut parse_cache = ParseCache::new();
        let overrides = BTreeMap::new();
        let mut ctx = CrateCtx {
            src_root: src_root.to_path_buf(),
            display_base: src_root.to_path_buf(),
            display_prefix: String::new(),
            sources: &mut sources,
            diags: &mut diags,
            file_ids: &mut file_ids,
            next_file_id: &mut next_file_id,
            loaded: BTreeSet::new(),
            synthetic: &mut synthetic,
            parse_time: &mut parse_time,
            parse_cache: &mut parse_cache,
            overrides: &overrides,
        };
        run(&mut ctx);
        diags
            .iter()
            .map(|diag| format!("{}: {}", diag.code, diag.message))
            .collect()
    }

    #[test]
    fn an_inline_module_without_a_name_keeps_its_parent_prefix() {
        // Only the root module of a file is unnamed; an inline module built
        // without a name is expanded under its parent's prefix.
        use ravum_lang_syntax::ast::ItemKind;
        let temp = TempDir::new("unnamed-inline");
        let src = temp.path().join("src");
        fs::create_dir_all(&src).expect("src");
        let src = fs::canonicalize(&src).expect("canonical src");
        let mut parsed = ravum_lang_syntax::parser::parse_file(0, "mod a { pub fn f() { } }\n");
        let ItemKind::Mod(inner) = &mut parsed.program.root.items[0].kind else {
            panic!("expected the inline module");
        };
        inner.name = None;
        let mut module = parsed.program.root;
        let diags = with_ctx(&src, |ctx| {
            expand_module(ctx, &mut module, &src, Path::new(""));
        });
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(module.items.len(), 1);
    }

    fn decl(name: &str) -> ravum_lang_syntax::ast::ModDecl {
        ravum_lang_syntax::ast::ModDecl {
            name: ravum_lang_syntax::ast::Ident::new(name, Span::dummy()),
        }
    }

    #[test]
    fn a_symlinked_crate_root_is_refused() {
        let temp = TempDir::new("after-digest-root");
        let real = temp.file("real.rv", "fn main() { }\n");
        let src = temp.path().join("src");
        fs::create_dir_all(&src).expect("src");
        std::os::unix::fs::symlink(&real, src.join("main.rv")).expect("symlink");
        let src = fs::canonicalize(&src).expect("canonical src");
        let diags = with_ctx(&src, |ctx| {
            assert!(load_crate_root(ctx, &src.join("main.rv")).is_none());
        });
        assert!(
            diags
                .iter()
                .any(|d| d.starts_with("E-MOD-PATH:") && d.contains("symbolic link")),
            "{diags:?}"
        );
    }

    #[test]
    fn a_module_file_outside_the_source_root_is_refused() {
        let temp = TempDir::new("after-digest-escape");
        temp.file("src/main.rv", "mod m;\nfn main() { }\n");
        temp.file("elsewhere/m.rv", "pub fn f() { }\n");
        let src = fs::canonicalize(temp.path().join("src")).expect("src");
        let elsewhere = fs::canonicalize(temp.path().join("elsewhere")).expect("elsewhere");
        let diags = with_ctx(&src, |ctx| {
            assert!(resolve_module_file(ctx, &decl("m"), &elsewhere, Path::new("")).is_none());
        });
        assert!(
            diags
                .iter()
                .any(|d| d.starts_with("E-MOD-PATH-ESCAPE:") && d.contains("module `m`")),
            "{diags:?}"
        );
    }
}

/// The pure path helpers of the loader, on the shapes a load rarely builds.
mod path_helpers {
    use super::*;
    use crate::loader::fsutil::{
        check_path_no_symlinks, display_relative, next_synthetic, path_to_slash,
    };

    #[test]
    fn a_path_outside_its_base_is_displayed_whole_with_every_component() {
        assert_eq!(
            display_relative(Path::new("/a/./b/../c"), Path::new("/elsewhere")),
            "/a/b/../c"
        );
        assert_eq!(
            display_relative(Path::new("/base/x/y"), Path::new("/base")),
            "x/y"
        );
        assert_eq!(path_to_slash(Path::new("./x")), "x");
    }

    #[test]
    fn a_target_outside_the_base_has_no_link_to_check() {
        let mut diags = DiagnosticBag::new();
        assert_eq!(
            check_path_no_symlinks(Path::new("/base"), Path::new("/other/file"), &mut diags),
            Some(())
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn the_synthetic_counter_stays_in_the_synthetic_file_range() {
        let start = u64::from(SYNTHETIC_FILE) << 32;
        let mut counter = start | 7;
        assert_eq!(next_synthetic(&mut counter), start | 7);
        assert_eq!(counter, start | 8);
        // A counter outside the range (fresh or overflowed) is reset to the
        // first synthetic id.
        let mut counter = 0;
        assert_eq!(next_synthetic(&mut counter), 0);
        assert_eq!(counter, start | 1);
    }
}

mod filesystem_contracts;

// -- E6 coverage: helpers and I/O refusals ------------------------------------

#[test]
fn a_module_identifier_starts_with_a_letter_or_an_underscore() {
    assert!(validate::is_module_identifier("util"));
    assert!(validate::is_module_identifier("_x1"));
    assert!(!validate::is_module_identifier(""));
    assert!(!validate::is_module_identifier("1a"));
    assert!(!validate::is_module_identifier("-a"));
}

#[test]
fn an_empty_inline_table_is_an_empty_table() {
    let mut diags = DiagnosticBag::new();
    match toml::parse_toml_value("{ }", 0, 0, &mut diags) {
        Some(toml::TomlValue::Table(entries)) => assert!(entries.is_empty()),
        other => panic!("expected an empty table, got {other:?}"),
    }
    assert!(!diags.has_errors());
}

#[test]
fn a_string_ending_in_a_lone_backslash_is_unterminated() {
    let mut diags = DiagnosticBag::new();
    assert!(toml::parse_basic_string("\"abc\\", 0, 0, &mut diags).is_none());
    assert!(diags.has_errors());
}

#[cfg(unix)]
#[test]
fn a_lockfile_under_a_path_that_is_a_file_cannot_be_inspected() {
    // `<file>/ravum.lock`: on Unix the metadata call fails with "not a
    // directory" (ENOTDIR), which is neither "not found" nor a file; Windows
    // answers "not found" for the same path.
    let dir = TempDir::new("lock-enotdir");
    let not_a_dir = dir.file("plain", "x");
    let manifest = PackageManifest {
        name: "p".to_string(),
        version: "0.1.0".to_string(),
        edition: "core-1.0".to_string(),
        dependencies: Vec::new(),
    };
    let graph = graph::DepGraph::new(&not_a_dir, manifest);
    let mut diags = DiagnosticBag::new();
    let state = lockfile::update_lockfile(&not_a_dir, &graph, false, &mut diags);
    assert!(matches!(state, LockfileState::Invalid));
    assert!(diags
        .iter()
        .any(|d| d.code == "E-MOD-LOCK" && d.message.contains("cannot inspect")));
}

#[cfg(target_os = "linux")]
#[test]
fn a_regular_file_whose_read_fails_is_reported() {
    // `/proc/self/mem` is a regular, zero-length file whose `read` fails.
    let mut diags = DiagnosticBag::new();
    let text = fsutil::read_text_limited(
        Path::new("/proc/self/mem"),
        1024,
        "E-MOD-MANIFEST",
        &mut diags,
    );
    assert!(text.is_none());
    assert!(diags
        .iter()
        .any(|d| d.code == "E-MOD-MANIFEST" && d.message.contains("cannot read")));
}

#[test]
fn slash_paths_keep_normal_parts_parents_and_the_root() {
    assert_eq!(fsutil::path_to_slash(Path::new("a/./b/../c")), "a/b/../c");
    assert_eq!(fsutil::path_to_slash(Path::new("/x/y")), "/x/y");
}

// -- E6 family E: races between a check and `canonicalize` ---------------------

/// Run `test` with `canonicalize` replaced by `fake`, restoring it after.
fn with_fake_canonicalize(fake: fsutil::FakeCanonicalize, test: impl FnOnce()) {
    fsutil::TEST_CANONICALIZE.with(|cell| cell.set(Some(fake)));
    test();
    fsutil::TEST_CANONICALIZE.with(|cell| cell.set(None));
}

fn app_with_util(temp: &TempDir) {
    temp.file(
        "app/ravum.toml",
        &manifest("app", "\n[dependencies]\nutil = { path = \"../util\" }\n"),
    );
    temp.file("app/src/main.rv", "fn main() { }\n");
    temp.file("util/ravum.toml", &manifest("util", ""));
    temp.file("util/src/lib.rv", "pub fn one() -> i32 { return 1; }\n");
}

#[test]
fn a_dependency_directory_that_vanishes_before_canonicalize_is_refused() {
    let temp = TempDir::new("dep-canonicalize-race");
    app_with_util(&temp);
    with_fake_canonicalize(
        |path| {
            if path.ends_with("util") {
                Err(std::io::Error::new(std::io::ErrorKind::NotFound, "removed"))
            } else {
                fs::canonicalize(path)
            }
        },
        || {
            let loaded = load_package(&temp.path.join("app"));
            assert!(codes(&loaded.diags).iter().any(|c| c == "E-MOD-DEPENDENCY"));
            assert!(loaded
                .diags
                .iter()
                .any(|d| d.message.contains("cannot be resolved")));
        },
    );
}

#[test]
fn a_dependency_directory_replaced_by_an_outside_one_is_refused() {
    let temp = TempDir::new("dep-canonical-escape");
    app_with_util(&temp);
    with_fake_canonicalize(
        |path| {
            if path.ends_with("util") {
                Ok(PathBuf::from("/elsewhere/util"))
            } else {
                fs::canonicalize(path)
            }
        },
        || {
            let loaded = load_package(&temp.path.join("app"));
            assert!(codes(&loaded.diags)
                .iter()
                .any(|c| c == "E-MOD-PATH-ESCAPE"));
        },
    );
}

#[test]
fn a_module_file_that_vanishes_before_canonicalize_is_refused() {
    let temp = TempDir::new("module-canonicalize-race");
    temp.file("app/ravum.toml", &manifest("app", ""));
    temp.file("app/src/main.rv", "mod util;\nfn main() { }\n");
    temp.file("app/src/util.rv", "pub fn one() -> i32 { return 1; }\n");
    with_fake_canonicalize(
        |path| {
            if path.ends_with("util.rv") {
                Err(std::io::Error::new(std::io::ErrorKind::NotFound, "removed"))
            } else {
                fs::canonicalize(path)
            }
        },
        || {
            let loaded = load_package(&temp.path.join("app"));
            assert!(codes(&loaded.diags).iter().any(|c| c == "E-MOD-MISSING"));
        },
    );
}

#[test]
fn a_dependency_crate_root_that_vanishes_after_the_graph_is_refused() {
    // The graph accepted `util/src/lib.rv`; it disappears before the crate is
    // loaded: the dependency contributes no module and the load reports it.
    let temp = TempDir::new("dep-root-race");
    app_with_util(&temp);
    with_fake_canonicalize(
        |path| {
            if path.ends_with("lib.rv") {
                Err(std::io::Error::new(std::io::ErrorKind::NotFound, "removed"))
            } else {
                fs::canonicalize(path)
            }
        },
        || {
            let loaded = load_package(&temp.path.join("app"));
            assert!(codes(&loaded.diags).iter().any(|c| c == "E-IO"));
        },
    );
}

#[test]
fn a_dependency_manifest_whose_session_id_is_taken_is_refused() {
    // A session table that gives the dependency manifest the id of the
    // package manifest (an inconsistent table) makes its registration fail.
    let temp = TempDir::new("dep-manifest-id");
    app_with_util(&temp);
    let app_manifest = fs::canonicalize(temp.path.join("app/ravum.toml")).expect("app manifest");
    let util_manifest = fs::canonicalize(temp.path.join("util/ravum.toml")).expect("util manifest");
    let mut file_ids = BTreeMap::new();
    file_ids.insert(app_manifest, 7);
    file_ids.insert(util_manifest, 7);
    let options = LoadOptions {
        file_ids: Some(file_ids),
        ..LoadOptions::default()
    };
    let loaded = load_package_with_options(&temp.path.join("app"), &options);
    assert!(
        codes(&loaded.diags).iter().any(|c| c == "E-IO"),
        "{:?}",
        codes(&loaded.diags)
    );
}

/// The manifest is the first source of a load's fresh table; a table that
/// already holds a file under the next id is a caller bug, not a diagnostic.
#[test]
#[should_panic(expected = "invariant: the manifest is the first source registered by a load")]
fn registering_the_manifest_into_a_used_table_is_an_invariant_failure() {
    let mut sources = SourceMap::new();
    sources
        .add_at(0, "other.rv", "")
        .expect("a fresh table accepts id 0");
    let mut file_ids = BTreeMap::new();
    let mut next_file_id: FileId = 0;
    let mut diags = DiagnosticBag::new();
    let _ = register_manifest(
        &mut sources,
        &mut file_ids,
        &mut next_file_id,
        Path::new("/pkg/ravum.toml"),
        String::new(),
        &mut diags,
    );
}

#[test]
fn an_entry_matches_only_with_its_exact_stored_name() {
    // MOD-2.6: on a case-insensitive file system `foo.rv` would answer for
    // `Foo.rv`; the listing comparison is byte for byte.
    let temp = TempDir::new("exact-entry");
    fs::write(temp.path().join("foo.rv"), "").unwrap();
    assert!(has_exact_entry(temp.path(), "foo.rv"));
    assert!(!has_exact_entry(temp.path(), "Foo.rv"));
    assert!(!has_exact_entry(&temp.path().join("absent"), "foo.rv"));
}
