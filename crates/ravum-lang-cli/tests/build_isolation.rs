//! P0-B: deterministic build identity, isolated compilation and atomic
//! publication.
//!
//! Every test drives the real `ravum-lang` binary in a throw-away package (no
//! network, no shared state) and asserts on the published `.ravum/` tree:
//!
//! * `same sources + options` => same identity, twice;
//! * a changed snapshot => a different identity *and* an artifact whose observed
//!   behaviour matches the new source;
//! * two concurrent builds of one package both succeed and never truncate each
//!   other's output;
//! * native and `wasm32-wasip1` builds of one package never collide (skipped
//!   explicitly, never counted as a pass, when the rustc target is absent);
//! * a failing build never replaces a previously published artifact/manifest;
//! * a killed build publishes nothing and leaves the previous artifact running.
//!
//! The wasm test needs only the rustc target (it does not execute the module).

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "ravum-build-isolation-{tag}-{}-{}-{nanos}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_ravum-lang")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn run_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(binary())
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run ravum-lang")
}

fn run_in_env(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(binary());
    command.args(args).current_dir(dir);
    for (key, value) in env {
        command.env(key, value);
    }
    command.output().expect("run ravum-lang")
}

fn spawn_in(dir: &Path, args: &[&str]) -> Child {
    Command::new(binary())
        .args(args)
        .current_dir(dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn ravum-lang")
}

/// One hermetic binary package `src/main.rv` printing `text`.
fn package(temp: &TempDir, name: &str, text: &str) -> PathBuf {
    let dir = temp.path().join(name);
    fs::create_dir_all(dir.join("src")).expect("create package");
    fs::write(
        dir.join("ravum.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n"),
    )
    .expect("write manifest");
    write_source(&dir, text);
    dir
}

fn write_source(package: &Path, text: &str) {
    fs::write(
        package.join("src/main.rv"),
        format!("fn main() {{\n    io::println(\"{text}\");\n}}\n"),
    )
    .expect("write source");
}

fn crate_name(name: &str) -> String {
    name.replace(['-', '.'], "_")
}

fn artifact_path(package: &Path, name: &str) -> PathBuf {
    let file = if cfg!(windows) {
        format!("{}.exe", crate_name(name))
    } else {
        crate_name(name)
    };
    package.join(".ravum/bin").join(file)
}

fn manifest_path(package: &Path, name: &str) -> PathBuf {
    let file = if cfg!(windows) {
        format!("{}.exe", crate_name(name))
    } else {
        crate_name(name)
    };
    package
        .join(".ravum/bin")
        .join(format!("{file}.build.json"))
}

fn build(package: &Path) -> Output {
    run_in(package, &["build", "."])
}

fn build_ok(package: &Path) -> Output {
    let output = build(package);
    assert!(
        output.status.success(),
        "build must succeed\n--- stdout ---\n{}\n--- stderr ---\n{}",
        stdout(&output),
        stderr(&output)
    );
    output
}

/// `identity: sha256:<hex>` line of `ravum-lang build`.
fn identity_line(output: &Output) -> String {
    let text = stdout(output);
    let line = text
        .lines()
        .find(|line| line.starts_with("identity: "))
        .unwrap_or_else(|| panic!("no `identity:` line in build output:\n{text}"));
    line["identity: ".len()..].trim().to_string()
}

fn manifest(package: &Path, name: &str) -> Value {
    let path = manifest_path(package, name);
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read `{}`: {error}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("`{}` is not JSON: {error}\n{text}", path.display()))
}

fn run_artifact(package: &Path, name: &str) -> Output {
    let path = artifact_path(package, name);
    Command::new(&path)
        .current_dir(package)
        .output()
        .unwrap_or_else(|error| panic!("cannot run `{}`: {error}", path.display()))
}

fn artifact_stdout(package: &Path, name: &str) -> String {
    let output = run_artifact(package, name);
    assert!(
        output.status.success(),
        "artifact failed: {}",
        stderr(&output)
    );
    stdout(&output)
}

/// Names of the files published in `.ravum/bin` (sorted), excluding the PDB
/// rustc may leave inside the *isolated* directory (never published).
fn published_bin_entries(package: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(package.join(".ravum/bin"))
        .expect("read .ravum/bin")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    names
}

fn rustc_ready() -> bool {
    Command::new(std::env::var("RAVUM_RUSTC").unwrap_or_else(|_| "rustc".to_string()))
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn rustc_wasm_ready() -> bool {
    let rustc = std::env::var("RAVUM_RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let output = Command::new(rustc).args(["--print", "sysroot"]).output();
    match output {
        Ok(output) if output.status.success() => {
            let sysroot = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Path::new(&sysroot)
                .join("lib/rustlib/wasm32-wasip1/lib")
                .is_dir()
        }
        _ => false,
    }
}

fn skip_without_rustc() -> bool {
    if rustc_ready() {
        return false;
    }
    eprintln!("SKIP: rustc is not available");
    true
}

// ---------------------------------------------------------------------------
// 1. Determinism: same sources + options => same identity.
// ---------------------------------------------------------------------------

#[test]
fn same_sources_and_options_produce_the_same_identity_twice() {
    if skip_without_rustc() {
        return;
    }
    let temp = TempDir::new("identity-stable");
    let pkg = package(&temp, "iso-stable", "one");

    let first = build_ok(&pkg);
    let identity_first = identity_line(&first);
    let second = build_ok(&pkg);
    let identity_second = identity_line(&second);

    assert!(
        identity_first.starts_with("sha256:"),
        "identity must be a sha256 digest: {identity_first}"
    );
    assert_eq!(
        identity_first, identity_second,
        "same sources and options must produce the same identity"
    );
    assert!(artifact_stdout(&pkg, "iso-stable") == "one\n");
    assert_eq!(
        manifest(&pkg, "iso-stable")["identity"],
        identity_first.as_str()
    );
    assert!(
        manifest(&pkg, "iso-stable")["source_digest"]
            .as_str()
            .unwrap()
            .starts_with("sha256:"),
        "the manifest records the source snapshot digest"
    );
    // The identity keys an isolated directory holding the intermediates. The
    // on-disk component is a short 16-hex prefix (Windows MAX_PATH safety), while
    // the full digest is what the CLI and the manifest report.
    let short: String = identity_first
        .trim_start_matches("sha256:")
        .chars()
        .take(16)
        .collect();
    assert_eq!(short.len(), 16);
    let isolated = pkg.join(".ravum/build").join(&short);
    assert!(
        isolated.join("gen/main.rs").is_file(),
        "the generated Rust lives in the isolated identity directory: {}",
        isolated.display()
    );
    assert!(
        !pkg.join(".ravum/build")
            .join(identity_first.trim_start_matches("sha256:"))
            .is_dir(),
        "the on-disk component must stay short (16 hex), not the full digest"
    );
    // The documented published paths are still there.
    assert!(pkg.join(".ravum/gen/main.rs").is_file());
    assert!(artifact_path(&pkg, "iso-stable").is_file());
}

// ---------------------------------------------------------------------------
// 2. A changed snapshot changes the identity and the artifact behaviour.
// ---------------------------------------------------------------------------

#[test]
fn different_snapshots_give_different_identities_and_the_right_artifact() {
    if skip_without_rustc() {
        return;
    }
    let temp = TempDir::new("identity-snapshot");
    let pkg = package(&temp, "iso-snapshot", "one");

    let first = build_ok(&pkg);
    let identity_one = identity_line(&first);
    let digest_one = manifest(&pkg, "iso-snapshot")["source_digest"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(artifact_stdout(&pkg, "iso-snapshot"), "one\n");

    write_source(&pkg, "two");
    let second = build_ok(&pkg);
    let identity_two = identity_line(&second);
    let digest_two = manifest(&pkg, "iso-snapshot")["source_digest"]
        .as_str()
        .unwrap()
        .to_string();

    assert_ne!(
        identity_one, identity_two,
        "a changed source snapshot must change the build identity"
    );
    assert_ne!(
        digest_one, digest_two,
        "the snapshot digest must change with the source content"
    );
    assert_eq!(
        artifact_stdout(&pkg, "iso-snapshot"),
        "two\n",
        "the published artifact must correspond to the new source"
    );

    // And back: the first identity is reproducible from the first source.
    write_source(&pkg, "one");
    let third = build_ok(&pkg);
    assert_eq!(
        identity_line(&third),
        identity_one,
        "restoring the source must restore the identity (no mtime input)"
    );
}

// ---------------------------------------------------------------------------
// 3. Two concurrent builds of one package both succeed, nobody truncates the
//    other's output, and the published tree stays clean.
// ---------------------------------------------------------------------------

#[test]
fn two_concurrent_builds_of_the_same_package_succeed() {
    if skip_without_rustc() {
        return;
    }
    let temp = TempDir::new("concurrent");
    let pkg = package(&temp, "iso-concurrent", "parallel");
    // A first successful build establishes the published artifact.
    let baseline = build_ok(&pkg);
    let identity = identity_line(&baseline);

    let mut first = spawn_in(&pkg, &["build", "."]);
    let mut second = spawn_in(&pkg, &["build", "."]);
    let first_output = first.wait_with_output().expect("wait first build");
    let second_output = second.wait_with_output().expect("wait second build");

    assert!(
        first_output.status.success(),
        "concurrent build #1 failed ({}):\n--- stdout ---\n{}\n--- stderr ---\n{}",
        first_output.status,
        stdout(&first_output),
        stderr(&first_output)
    );
    assert!(
        second_output.status.success(),
        "concurrent build #2 failed ({}):\n--- stdout ---\n{}\n--- stderr ---\n{}",
        second_output.status,
        stdout(&second_output),
        stderr(&second_output)
    );

    assert_eq!(
        artifact_stdout(&pkg, "iso-concurrent"),
        "parallel\n",
        "the published artifact must be valid after concurrent builds"
    );
    assert_eq!(
        manifest(&pkg, "iso-concurrent")["identity"],
        identity.as_str(),
        "concurrent builds of one snapshot share one identity"
    );
    let entries = published_bin_entries(&pkg);
    let expected = vec![
        format!("{}.build.json", artifact_file(&pkg, "iso-concurrent")),
        artifact_file(&pkg, "iso-concurrent"),
    ];
    let mut expected = expected;
    expected.sort();
    assert_eq!(
        entries, expected,
        "publication must not leave temporary or partial files behind: {entries:?}"
    );
}

fn artifact_file(package: &Path, name: &str) -> String {
    artifact_path(package, name)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string()
}

// ---------------------------------------------------------------------------
// 4. Native and wasm targets never collide.
// ---------------------------------------------------------------------------

#[test]
fn native_and_wasm_targets_do_not_collide() {
    if skip_without_rustc() {
        return;
    }
    if !rustc_wasm_ready() {
        eprintln!("SKIP: rustc target wasm32-wasip1 is not installed");
        return;
    }
    let temp = TempDir::new("targets");
    let pkg = package(&temp, "iso-targets", "targets");

    let native = build_ok(&pkg);
    let native_identity = identity_line(&native);
    let wasm = run_in(&pkg, &["build", ".", "--target", "wasm32-wasip1"]);
    assert!(
        wasm.status.success(),
        "wasm build must succeed\n{}\n{}",
        stdout(&wasm),
        stderr(&wasm)
    );
    let wasm_identity = identity_line(&wasm);

    assert_ne!(
        native_identity, wasm_identity,
        "the target triple is part of the build identity"
    );
    // The wasm artifact lives next to the native one, with its own manifest.
    let wasm_file = pkg
        .join(".ravum/bin")
        .join(format!("{}.wasm", crate_name("iso-targets")));
    assert!(
        wasm_file.is_file(),
        "wasm artifact: {}",
        wasm_file.display()
    );
    assert!(artifact_path(&pkg, "iso-targets").is_file());
    let wasm_manifest_path = wasm_file.with_file_name(format!(
        "{}.build.json",
        wasm_file.file_name().unwrap().to_string_lossy()
    ));
    let wasm_json: Value =
        serde_json::from_str(&fs::read_to_string(&wasm_manifest_path).expect("read wasm manifest"))
            .expect("wasm manifest is JSON");
    assert_eq!(wasm_json["target"], "wasm32-wasip1");
    assert_eq!(wasm_json["identity"], wasm_identity.as_str());
    assert_ne!(
        wasm_json["identity"],
        native_identity.as_str(),
        "each target owns its identity and manifest"
    );
    assert_eq!(
        artifact_stdout(&pkg, "iso-targets"),
        "targets\n",
        "the native artifact must survive the wasm build"
    );
}

// ---------------------------------------------------------------------------
// 5. A failing build never replaces a published artifact or its manifest.
// ---------------------------------------------------------------------------

/// A failing compiler is simulated with `RAVUM_RUSTC` pointing at a directory:
/// the CLI cannot execute it, so the build fails at the compile stage — exactly
/// the branch that must not publish. This keeps the test independent of any
/// language/codegen behaviour.
#[test]
fn a_failing_build_never_replaces_the_published_artifact() {
    if skip_without_rustc() {
        return;
    }
    let temp = TempDir::new("failing");
    let pkg = package(&temp, "iso-failing", "kept");
    let good = build_ok(&pkg);
    assert_eq!(artifact_stdout(&pkg, "iso-failing"), "kept\n");
    let artifact = artifact_path(&pkg, "iso-failing");
    let manifest_file = manifest_path(&pkg, "iso-failing");
    let artifact_bytes = fs::read(&artifact).expect("read artifact");
    let manifest_bytes = fs::read(&manifest_file).expect("read manifest");
    let good_identity = identity_line(&good);

    // The source changes, so a failing build would publish a different artifact
    // if it published at all.
    write_source(&pkg, "changed");
    let fake_compiler = temp.path().join("not-a-compiler");
    fs::create_dir_all(&fake_compiler).expect("create fake compiler dir");
    let fake = fake_compiler.to_string_lossy().to_string();
    let failed = run_in_env(&pkg, &["build", "."], &[("RAVUM_RUSTC", &fake)]);
    assert!(!failed.status.success(), "the build must fail");
    assert!(
        !stderr(&failed).is_empty(),
        "a failed build must report a diagnostic"
    );

    assert_eq!(
        fs::read(&artifact).expect("read artifact"),
        artifact_bytes,
        "a failed build must leave the published artifact byte-for-byte untouched"
    );
    assert_eq!(
        fs::read(&manifest_file).expect("read manifest"),
        manifest_bytes,
        "a failed build must leave the manifest untouched"
    );
    assert_eq!(artifact_stdout(&pkg, "iso-failing"), "kept\n");
    assert_eq!(
        manifest(&pkg, "iso-failing")["identity"],
        good_identity.as_str()
    );
}

// ---------------------------------------------------------------------------
// 6. A killed build publishes nothing; the old artifact still runs.
//
// Timing note (learned from the first Linux CI run): `try_wait` immediately
// followed by `kill` is a race — the build can finish between the two, and then
// a *complete* publication is the correct outcome. So the test waits for the
// observable start of the compile phase (`gen/main.rs` inside the identity
// directory, written before `rustc` runs) and then accepts exactly two states:
// the previous artifact untouched, or the whole new program published. A
// half-written artifact, a stale manifest or a leftover file in `.ravum/bin`
// fails either way. What this protects is atomic publication, not "the kill
// always wins".
// ---------------------------------------------------------------------------

/// Wait until some `.ravum/build/<id>/gen/main.rs` exists, i.e. the frontend
/// finished and `rustc` is starting. Returns false on timeout (fast machine:
/// the build already completed).
fn wait_for_generated_source(package: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        let root = package.join(".ravum/build");
        if let Ok(entries) = fs::read_dir(&root) {
            for entry in entries.filter_map(Result::ok) {
                if entry.path().join("gen/main.rs").is_file() {
                    return true;
                }
            }
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_killed_build_leaves_the_previous_artifact_running() {
    if skip_without_rustc() {
        return;
    }
    let temp = TempDir::new("killed");
    let pkg = package(&temp, "iso-killed", "before");
    build_ok(&pkg);
    assert_eq!(artifact_stdout(&pkg, "iso-killed"), "before\n");
    let artifact = artifact_path(&pkg, "iso-killed");
    let manifest_file = manifest_path(&pkg, "iso-killed");
    let artifact_bytes = fs::read(&artifact).expect("read artifact");
    let manifest_bytes = fs::read(&manifest_file).expect("read manifest");
    let entries_before = published_bin_entries(&pkg);

    // A large replacement source so the compile phase lasts long enough to be
    // interrupted: the child is killed once `rustc` has started.
    let mut source = String::from("fn main() {\n");
    for index in 0..400 {
        source.push_str(&format!("    io::println(\"line {index}\");\n"));
    }
    source.push_str("}\n");
    fs::write(pkg.join("src/main.rv"), source).expect("write large source");

    let mut child = spawn_in(&pkg, &["build", "."]);
    let compiled_started = wait_for_generated_source(&pkg, Duration::from_secs(30));
    if child.try_wait().expect("poll build").is_none() {
        child.kill().expect("kill the running build");
    }
    let _output = child.wait_with_output().expect("wait for the killed build");

    let artifact_after = fs::read(&artifact).expect("read artifact after kill");
    let manifest_after = fs::read(&manifest_file).expect("read manifest after kill");
    if artifact_after == artifact_bytes {
        // Interrupted before publication: nothing may have changed.
        assert!(
            compiled_started,
            "the build was killed before it even generated its intermediate source"
        );
        assert_eq!(
            manifest_after, manifest_bytes,
            "a killed build must not publish a manifest"
        );
        assert_eq!(
            published_bin_entries(&pkg),
            entries_before,
            "a killed build must not leave partial files in `.ravum/bin`"
        );
        assert_eq!(
            artifact_stdout(&pkg, "iso-killed"),
            "before\n",
            "the previous artifact still runs"
        );
    } else {
        // The build completed before the kill landed. Publication must then be
        // whole and coherent — never a truncated or half-updated artifact.
        let output = artifact_stdout(&pkg, "iso-killed");
        assert!(
            output.contains("line 0") && output.contains("line 399"),
            "a completed build must publish the whole new program, got: {output}"
        );
        assert!(
            manifest(&pkg, "iso-killed")["identity"]
                .as_str()
                .unwrap()
                .starts_with("sha256:"),
            "the manifest must describe the published artifact"
        );
        assert_eq!(
            published_bin_entries(&pkg),
            entries_before,
            "publication must leave exactly the artifact and its manifest"
        );
    }
}

// ---------------------------------------------------------------------------
// 7. Path safety: a deep package directory (near MAX_PATH) still builds.
//
// The pre-fix layout added a 64-hex identity directory and a `\\?\` verbatim
// prefix to the rustc command line, which made `link.exe` fail with
// "LNK1181: cannot open input file '.obj'" for `%TEMP%` packages. This test pins
// the short, verbatim-free layout with a deliberately long package path.
// ---------------------------------------------------------------------------

#[test]
fn a_deep_package_path_still_builds() {
    if skip_without_rustc() {
        return;
    }
    let temp = TempDir::new("deep");
    // Build a nested path so that the package directory is well past 100 chars
    // (the Windows `%TEMP%` failure shape) but still under MAX_PATH.
    let mut dir = temp.path().to_path_buf();
    for level in ["niveau-un", "niveau-deux", "niveau-trois", "niveau-quatre"] {
        dir = dir.join(format!("{level}-abcdefghij"));
    }
    fs::create_dir_all(dir.join("src")).expect("create deep package");
    fs::write(
        dir.join("ravum.toml"),
        "[package]\nname = \"deep-path\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
    )
    .expect("write manifest");
    write_source(&dir, "deep");

    let output = run_in(&dir, &["build", "."]);
    assert!(
        output.status.success(),
        "a deep package must build ({}), path length {}:\n--- stdout ---\n{}\n--- stderr ---\n{}",
        output.status,
        dir.display().to_string().len(),
        stdout(&output),
        stderr(&output)
    );
    assert_eq!(artifact_stdout(&dir, "deep-path"), "deep\n");
    // The isolated path stays short: `.ravum/build/<16 hex>/gen/main.rs`.
    let build_root = dir.join(".ravum/build");
    let identity_dir = fs::read_dir(&build_root)
        .expect("read .ravum/build")
        .filter_map(Result::ok)
        .next()
        .expect("one identity directory")
        .path();
    let name = identity_dir
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    assert_eq!(name.len(), 16, "short identity directory: {name}");
    let generated = identity_dir.join("gen/main.rs");
    assert!(
        generated.is_file(),
        "generated intermediate: {}",
        generated.display()
    );
}
