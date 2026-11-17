//! E6 — dependency-graph paths of the package loader that no test reached.
//! Same fixtures and same assertion style as `loader_contracts.rs`: every test
//! names the diagnostic the user reads and the fail-closed outcome.

use ravum_lang_sema::loader::{load_package, LoadedPackage, MAX_FILE_SIZE};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

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
            "ravum-loader-paths-{tag}-{}-{}-{nanos}",
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

#[cfg(unix)]
fn symlink_file(target: &Path, link: &Path) -> bool {
    std::os::unix::fs::symlink(target, link).is_ok()
}

#[cfg(windows)]
fn symlink_file(target: &Path, link: &Path) -> bool {
    std::os::windows::fs::symlink_file(target, link).is_ok()
}

/// A diamond: `app` depends on `left` and `right`, both depend on `base`. The
/// second edge to `base` reuses the package already loaded under that name.
#[test]
fn a_package_reached_twice_under_its_own_name_is_loaded_once() {
    let temp = TempDir::new("diamond");
    temp.file(
        "app/ravum.toml",
        &manifest(
            "app",
            "\n[dependencies]\nleft = { path = \"../left\" }\nright = { path = \"../right\" }\n",
        ),
    );
    temp.file("app/src/main.rv", "fn main() { }\n");
    for side in ["left", "right"] {
        temp.file(
            &format!("{side}/ravum.toml"),
            &manifest(side, "\n[dependencies]\nbase = { path = \"../base\" }\n"),
        );
        temp.file(
            &format!("{side}/src/lib.rv"),
            "pub fn side() -> i32 { return 1; }\n",
        );
    }
    library_package(&temp, "base", "base");
    let loaded = load_package(&temp.path().join("app"));
    assert_accepted(&loaded);
    let names: Vec<&str> = loaded
        .dependencies
        .iter()
        .map(|dep| dep.name.as_str())
        .collect();
    assert_eq!(names, ["base", "left", "right"]);
}

/// A dependency manifest above the size limit, or one that is not UTF-8,
/// cannot be read: the whole load fails, with the limit or the encoding named.
#[test]
fn an_unreadable_dependency_manifest_is_refused() {
    let oversized = TempDir::new("manifest-size");
    let app = app_with_dependency(&oversized, "util", "../util");
    let mut text = manifest("util", "");
    text.push_str(&"# padding\n".repeat(MAX_FILE_SIZE as usize / 10 + 1));
    oversized.file("util/ravum.toml", &text);
    oversized.file("util/src/lib.rv", "pub fn helper() -> i32 { return 1; }\n");
    let loaded = load_package(&app);
    assert_fail_closed(&loaded, "E-LIMIT-FILE-SIZE", "ravum.toml");

    let binary = TempDir::new("manifest-utf8");
    let app = app_with_dependency(&binary, "util", "../util");
    binary.bytes("util/ravum.toml", &[0xff, 0xfe, 0x00]);
    binary.file("util/src/lib.rv", "pub fn helper() -> i32 { return 1; }\n");
    let loaded = load_package(&app);
    assert_fail_closed(&loaded, "E-UNSUPPORTED-TOKEN", "is not valid UTF-8");
}

/// `src/lib.rv` of a dependency must be a regular file, not a link to one.
#[test]
fn a_dependency_whose_crate_root_is_a_symlink_is_refused() {
    let temp = TempDir::new("lib-symlink");
    let app = app_with_dependency(&temp, "util", "../util");
    temp.file("util/ravum.toml", &manifest("util", ""));
    let real = temp.file("elsewhere/lib.rv", "pub fn helper() -> i32 { return 1; }\n");
    temp.dir("util/src");
    if !symlink_file(&real, &temp.path().join("util/src/lib.rv")) {
        return; // symlinks unavailable (unprivileged Windows): nothing to assert
    }
    let loaded = load_package(&app);
    assert_fail_closed(
        &loaded,
        "E-MOD-PATH",
        "dependency `util` has a symlinked `src/lib.rv`",
    );
}

/// `.` components are neutral in a dependency path.
#[test]
fn current_directory_components_are_neutral_in_a_dependency_path() {
    let temp = TempDir::new("curdir");
    let app = app_with_dependency(&temp, "util", "./../util/.");
    library_package(&temp, "util", "util");
    assert_accepted(&load_package(&app));
}

/// An absolute dependency path is an escape, whatever it points at.
#[test]
fn an_absolute_dependency_path_is_refused() {
    let temp = TempDir::new("absolute");
    library_package(&temp, "util", "util");
    let absolute = temp.path().join("util");
    let app = app_with_dependency(
        &temp,
        "util",
        &absolute.to_string_lossy().replace('\\', "/"),
    );
    let loaded = load_package(&app);
    assert_fail_closed(&loaded, "E-MOD-PATH-ESCAPE", "is absolute");
}

/// A package opened through a linked directory is the package the link
/// points at: its siblings are those of the real directory.
#[cfg(unix)]
#[test]
fn a_package_opened_through_a_linked_directory_resolves_real_siblings() {
    let temp = TempDir::new("linked-root");
    app_with_dependency(&temp, "util", "../util");
    library_package(&temp, "util", "util");
    temp.dir("view");
    std::os::unix::fs::symlink(temp.path().join("app"), temp.path().join("view/app"))
        .expect("the link is created");
    let loaded = load_package(&temp.path().join("view/app"));
    assert_accepted(&loaded);
    assert_eq!(loaded.dependencies.len(), 1);
}

/// Removes every permission from a directory for the life of the guard.
#[cfg(unix)]
struct Locked(PathBuf);

#[cfg(unix)]
impl Locked {
    /// `None` when permissions do not bind the current user (root): the
    /// filesystem then answers every lookup and there is nothing to observe.
    fn new(dir: PathBuf) -> Option<Self> {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o000)).expect("chmod works");
        let locked = Self(dir);
        if fs::read_dir(&locked.0).is_ok() {
            eprintln!("SKIP: permissions do not apply to this user");
            return None;
        }
        Some(locked)
    }
}

#[cfg(unix)]
impl Drop for Locked {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
    }
}

/// A dependency path whose directory cannot be inspected is an I/O error, not
/// a missing dependency.
#[cfg(unix)]
#[test]
fn an_uninspectable_dependency_path_is_an_io_error() {
    let temp = TempDir::new("locked-path");
    let app = app_with_dependency(&temp, "util", "../locked/util");
    library_package(&temp, "locked/util", "util");
    let Some(_locked) = Locked::new(temp.path().join("locked")) else {
        return;
    };
    let loaded = load_package(&app);
    assert_fail_closed(&loaded, "E-IO", "dependency `util`: cannot inspect");
}

/// A dependency whose crate root cannot be inspected is an I/O error.
#[cfg(unix)]
#[test]
fn an_uninspectable_dependency_crate_root_is_an_io_error() {
    let temp = TempDir::new("locked-src");
    let app = app_with_dependency(&temp, "util", "../util");
    library_package(&temp, "util", "util");
    let Some(_locked) = Locked::new(temp.path().join("util/src")) else {
        return;
    };
    let loaded = load_package(&app);
    assert_fail_closed(&loaded, "E-IO", "cannot inspect dependency `util`");
}

fn manifest_messages(tag: &str, text: &str) -> Vec<String> {
    let temp = TempDir::new(tag);
    temp.file("ravum.toml", text);
    temp.file("src/main.rv", "fn main() { }\n");
    let loaded = load_package(temp.path());
    loaded
        .diags
        .iter()
        .map(|diag| format!("{}: {}", diag.code, diag.message))
        .collect()
}

const PACKAGE: &str = "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n";

/// Basic-string escapes are decoded (`\"`, `\\`, `\n`, `\t`, `\r`) before the
/// value is judged; the strict schema then names the key, not the escape.
#[test]
fn manifest_string_escapes_are_decoded_before_the_value_is_judged() {
    let found = manifest_messages(
        "escapes",
        &format!("{PACKAGE}description = \"say \\\"hi\\\" \\\\ \\n \\t \\r\"\n"),
    );
    assert_eq!(
        found,
        ["E-MOD-MANIFEST: unknown key `package.description` (Core 1.0 manifest is strict)"]
    );
    let found = manifest_messages(
        "escaped-quote",
        "[package]\nname = \"a\\\"pp\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
    );
    assert_eq!(
        found,
        ["E-MOD-MANIFEST: invalid package name `a\"pp`: expected `[a-z][a-z0-9_-]*`"]
    );
}

/// A quoted key is not a bare key, even when it hides an `=` behind an escaped
/// quote: the key/value split skips the string.
#[test]
fn a_quoted_key_with_an_escaped_quote_is_refused_as_a_key() {
    let found = manifest_messages(
        "quoted-key",
        &format!("{PACKAGE}\"we\\\"ird=key\" = \"x\"\n"),
    );
    assert_eq!(
        found,
        ["E-MOD-MANIFEST: invalid key `\"we\\\"ird=key\"` on line 5"]
    );
}

/// Empty entries of an inline table (a leading or trailing comma) are ignored,
/// and an escape inside a dependency path is decoded.
#[test]
fn empty_inline_table_entries_are_ignored() {
    let temp = TempDir::new("inline-commas");
    library_package(&temp, "util", "util");
    temp.file(
        "app/ravum.toml",
        &manifest(
            "app",
            "\n[dependencies]\nutil = { , path = \"../util\", }\n",
        ),
    );
    temp.file("app/src/main.rv", "fn main() { }\n");
    assert_accepted(&load_package(&temp.path().join("app")));

    let temp = TempDir::new("escaped-path");
    let app = app_with_dependency(&temp, "util", "../u\\\\tl");
    let loaded = load_package(&app);
    assert_fail_closed(
        &loaded,
        "E-MOD-DEPENDENCY",
        "path `../u\\tl` does not exist",
    );
}

/// The same I/O refusals without permissions (they run as root too): a regular
/// file used as a directory makes `lstat` fail with `ENOTDIR`, which is not
/// "not found".
#[cfg(unix)]
#[test]
fn a_file_used_as_a_directory_in_a_dependency_is_an_io_error() {
    let temp = TempDir::new("notdir-path");
    temp.file("afile", "x\n");
    let app = app_with_dependency(&temp, "util", "../afile/util");
    let loaded = load_package(&app);
    assert_fail_closed(&loaded, "E-IO", "dependency `util`: cannot inspect");

    let temp = TempDir::new("notdir-src");
    let app = app_with_dependency(&temp, "util", "../util");
    temp.file("util/ravum.toml", &manifest("util", ""));
    temp.file("util/src", "not a directory\n");
    let loaded = load_package(&app);
    assert_fail_closed(&loaded, "E-IO", "cannot inspect dependency `util`");
}
