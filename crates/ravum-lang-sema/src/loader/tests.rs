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
