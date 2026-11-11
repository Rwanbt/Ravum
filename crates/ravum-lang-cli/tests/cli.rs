//! End-to-end tests for the `ravum-lang` binary.
//!
//! Every test exercises the real binary; watch tests read the child output
//! through a bounded channel (timeouts, no fixed-sleep-only assertions) and
//! WASM tests skip explicitly when the `wasm32-wasip1` rustc target or Node.js
//! is unavailable.

use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
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
            "ravum-cli-test-{tag}-{}-{}-{nanos}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn write(&self, relative: &str, contents: &str) -> PathBuf {
        let path = self.path.join(relative);
        fs::create_dir_all(path.parent().unwrap()).expect("create parent");
        fs::write(&path, contents).expect("write test file");
        path
    }

    fn exists(&self, relative: &str) -> bool {
        self.path.join(relative).exists()
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

fn run(args: &[&str]) -> Output {
    Command::new(binary())
        .args(args)
        .output()
        .expect("run ravum-lang")
}

fn run_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(binary())
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run ravum-lang")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn json(output: &Output) -> Value {
    let text = stdout(output);
    serde_json::from_str(&text).unwrap_or_else(|error| {
        panic!(
            "stdout is not valid JSON ({error}):\n--- stdout ---\n{text}\n--- stderr ---\n{}",
            stderr(output)
        )
    })
}

fn manifest(name: &str, dependencies: &str) -> String {
    format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n\n{dependencies}"
    )
}

fn assert_json_envelope(value: &Value, command: &str) {
    assert_eq!(value["schema_version"], 1, "schema_version");
    assert_eq!(value["tool"], "ravum-lang", "tool");
    assert!(value["tool_version"].is_string(), "tool_version");
    assert!(
        value["spec_version"]
            .as_str()
            .map(|version| version.starts_with("core-1.0"))
            .unwrap_or(false),
        "spec_version"
    );
    assert_eq!(value["command"], command, "command");
    let status = value["status"].as_str().unwrap();
    assert!(["success", "failure", "unsupported"].contains(&status));
    assert!(value["exit_code"].is_i64(), "exit_code");
    assert!(value["success"].is_boolean(), "success must be a boolean");
    assert!(value["target"]["kind"].is_string(), "target.kind");
    assert!(value["target"]["path"].is_string(), "target.path");
    assert!(
        value["diagnostics"].is_array(),
        "diagnostics must be an array"
    );
    assert!(value["summary"].is_object(), "summary must be an object");
    for diagnostic in value["diagnostics"].as_array().unwrap() {
        for key in [
            "code",
            "severity",
            "phase",
            "message",
            "file",
            "line",
            "column",
            "end_line",
            "end_column",
            "primary",
            "labels",
            "notes",
        ] {
            assert!(
                diagnostic.get(key).is_some(),
                "diagnostic is missing `{key}`: {diagnostic}"
            );
        }
        assert!(diagnostic["notes"].is_array());
        assert!(diagnostic["labels"].is_array());
        assert!(diagnostic["primary"].is_object());
    }
}

fn diagnostic_codes(value: &Value) -> Vec<String> {
    value["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|diagnostic| diagnostic["code"].as_str().unwrap_or("").to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// Version / help / usage
// ---------------------------------------------------------------------------

#[test]
fn version_and_help_are_stable() {
    let output = run(&["--version"]);
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains("ravum-lang"));
    assert!(text.contains("core-1.0"));

    let output = run(&["--help"]);
    assert!(output.status.success());
    let text = stdout(&output);
    for command in [
        "init", "check", "build", "run", "watch", "test", "fmt", "lint", "doc", "cache", "doctor",
    ] {
        assert!(text.contains(command), "help must mention `{command}`");
    }
    assert!(text.contains("NOT IMPLEMENTED"));
    assert!(text.contains("lsp"));
    assert!(text.contains("language server"));

    let output = run(&["help", "check"]);
    assert!(output.status.success());
    assert!(stdout(&output).contains("USAGE: ravum-lang check"));
}

#[test]
fn unknown_command_exits_2() {
    let output = run(&["nope"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("unknown command"));
}

#[test]
fn invalid_arguments_exit_2() {
    let output = run(&["init"]);
    assert_eq!(output.status.code(), Some(2));

    let output = run(&["check", "--profile", "fast"]);
    assert_eq!(output.status.code(), Some(2));

    let output = run(&["check", "--nope"]);
    assert_eq!(output.status.code(), Some(2));

    let output = run(&["cache"]);
    assert_eq!(output.status.code(), Some(2));
}

// ---------------------------------------------------------------------------
// init
// ---------------------------------------------------------------------------

#[test]
fn init_creates_a_package_and_refuses_to_overwrite() {
    let temp = TempDir::new("init");
    let package = temp.path().join("hello");
    let output = run(&["init", package.to_str().unwrap()]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(package.join("ravum.toml").is_file());
    assert!(package.join("src/main.rv").is_file());
    assert!(package.join(".gitignore").is_file());
    let manifest = fs::read_to_string(package.join("ravum.toml")).unwrap();
    assert!(manifest.contains("name = \"hello\""));
    assert!(manifest.contains("edition = \"core-1.0\""));
    let main = fs::read_to_string(package.join("src/main.rv")).unwrap();
    assert!(main.contains("use std::io;"));
    assert!(main.contains("io::println"));
    assert!(fs::read_to_string(package.join(".gitignore"))
        .unwrap()
        .contains(".ravum/"));

    let output = run(&["init", package.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("not empty"));

    fs::write(package.join("keep.txt"), "keep").unwrap();
    let output = run(&["init", package.to_str().unwrap(), "--force"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(
        package.join("keep.txt").is_file(),
        "--force must not delete files"
    );
}

#[test]
fn init_lib_creates_a_library_root() {
    let temp = TempDir::new("init-lib");
    let package = temp.path().join("mylib");
    let output = run(&["init", package.to_str().unwrap(), "--lib"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(package.join("src/lib.rv").is_file());
    assert!(!package.join("src/main.rv").exists());
    assert!(fs::read_to_string(package.join("src/lib.rv"))
        .unwrap()
        .contains("pub fn answer"));
}

// ---------------------------------------------------------------------------
// check
// ---------------------------------------------------------------------------

#[test]
fn check_invalid_program_reports_json_and_exits_1() {
    let temp = TempDir::new("check-invalid");
    let package = temp.path().join("app");
    assert!(run(&["init", package.to_str().unwrap()]).status.success());
    fs::write(package.join("src/main.rv"), "fn main( {\n").unwrap();

    let output = run(&["check", package.to_str().unwrap(), "--json"]);
    assert_eq!(output.status.code(), Some(1));
    let value = json(&output);
    assert_json_envelope(&value, "check");
    assert_eq!(value["success"], false);
    assert!(!value["diagnostics"].as_array().unwrap().is_empty());
    let summary = &value["summary"];
    for key in [
        "package",
        "crate",
        "profile",
        "files",
        "items",
        "functions",
        "tests",
        "errors",
        "warnings",
        "effects",
        "dependencies",
        "dependencies_digest",
        "lockfile",
    ] {
        assert!(summary.get(key).is_some(), "summary missing `{key}`");
    }
    assert!(summary["errors"].as_u64().unwrap() >= 1);
}

#[test]
fn check_missing_path_reports_json_and_exits_1() {
    let temp = TempDir::new("check-missing");
    let missing = temp.path().join("does-not-exist");
    let output = run(&["check", missing.to_str().unwrap(), "--json"]);
    assert_eq!(output.status.code(), Some(1));
    let value = json(&output);
    assert_json_envelope(&value, "check");
    assert_eq!(value["success"], false);
    assert!(!value["diagnostics"].as_array().unwrap().is_empty());
}

#[test]
fn check_single_file_reports_crate_file() {
    let temp = TempDir::new("check-file");
    let file = temp.write("hello.rv", "fn main( {\n");
    let output = run(&["check", file.to_str().unwrap(), "--json"]);
    assert_eq!(output.status.code(), Some(1));
    let value = json(&output);
    assert_json_envelope(&value, "check");
    assert_eq!(value["summary"]["crate"], "file");
}

#[test]
fn single_file_mode_check_build_run() {
    let temp = TempDir::new("single-file");
    let file = temp.write(
        "solo.rv",
        "use std::io;\nfn main() {\n    io::println(\"solo\");\n}\n",
    );
    let output = run(&["check", file.to_str().unwrap(), "--json"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}\nstdout: {}",
        stderr(&output),
        stdout(&output)
    );
    let value = json(&output);
    assert_json_envelope(&value, "check");
    assert_eq!(value["success"], true);
    assert_eq!(value["summary"]["crate"], "file");
    assert_eq!(value["target"]["kind"], "file");
    assert!(
        !temp.exists("ravum.lock"),
        "single-file mode has no lockfile"
    );

    let output = run(&["build", file.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "stderr: {}\nstdout: {}",
        stderr(&output),
        stdout(&output)
    );
    let binary = if cfg!(windows) {
        temp.path().join(".ravum/bin/solo.exe")
    } else {
        temp.path().join(".ravum/bin/solo")
    };
    assert!(binary.is_file(), "missing {}", binary.display());

    let output = run(&["run", file.to_str().unwrap()]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(stdout(&output).contains("solo"));
}

#[test]
fn both_crate_roots_are_refused() {
    let temp = TempDir::new("both-roots");
    let package = temp.path().join("app");
    assert!(run(&["init", package.to_str().unwrap()]).status.success());
    fs::write(
        package.join("src/lib.rv"),
        "pub fn helper() -> i32 { return 1; }\n",
    )
    .unwrap();
    let output = run(&["check", package.to_str().unwrap(), "--json"]);
    assert_eq!(output.status.code(), Some(1));
    let value = json(&output);
    assert!(diagnostic_codes(&value).contains(&"E-MOD-BOTH-ROOTS".to_string()));
}

#[test]
fn reserved_windows_module_name_is_refused() {
    let temp = TempDir::new("reserved-module");
    let package = temp.path().join("app");
    assert!(run(&["init", package.to_str().unwrap()]).status.success());
    fs::write(package.join("src/main.rv"), "mod con;\nfn main() { }\n").unwrap();
    let output = run(&["check", package.to_str().unwrap(), "--json"]);
    assert_eq!(output.status.code(), Some(1));
    let value = json(&output);
    assert!(diagnostic_codes(&value).contains(&"E-MOD-RESERVED-NAME".to_string()));
}

#[test]
fn check_valid_hello_exits_0() {
    let temp = TempDir::new("check-hello");
    let package = temp.path().join("hello");
    assert!(run(&["init", package.to_str().unwrap()]).status.success());
    let output = run(&["check", package.to_str().unwrap(), "--json"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}\nstdout: {}",
        stderr(&output),
        stdout(&output)
    );
    let value = json(&output);
    assert_eq!(value["success"], true);
    assert_eq!(value["summary"]["errors"], 0);
    assert!(value["summary"]["functions"].as_u64().unwrap() >= 1);
}

#[test]
fn check_warnings_do_not_fail() {
    let temp = TempDir::new("check-warning");
    let package = temp.path().join("warn");
    assert!(run(&["init", package.to_str().unwrap()]).status.success());
    fs::write(
        package.join("src/main.rv"),
        "use std::io;\nfn main() {\n}\n",
    )
    .unwrap();
    let output = run(&["check", package.to_str().unwrap(), "--json"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "warnings must not fail check:\nstdout: {}\nstderr: {}",
        stdout(&output),
        stderr(&output)
    );
    let value = json(&output);
    assert_json_envelope(&value, "check");
    assert_eq!(value["success"], true);
    assert_eq!(value["summary"]["warnings"], 1);
    assert_eq!(value["summary"]["errors"], 0);
    assert_eq!(value["diagnostics"][0]["code"], "W-RES-UNUSED-IMPORT");
    assert_eq!(value["diagnostics"][0]["severity"], "warning");
}

// ---------------------------------------------------------------------------
// dependencies, lockfile
// ---------------------------------------------------------------------------

fn dependency_project(temp: &TempDir) -> PathBuf {
    temp.write("util/ravum.toml", &manifest("util", ""));
    temp.write("util/src/lib.rv", "pub fn helper() -> i32 { return 1; }\n");
    let app = temp.path().join("app");
    temp.write(
        "app/ravum.toml",
        &manifest("app", "\n[dependencies]\nutil = { path = \"../util\" }\n"),
    );
    temp.write("app/src/main.rv", "fn main() { }\n");
    app
}

#[test]
fn lockfile_is_deterministic_and_tracks_dependency_content() {
    let temp = TempDir::new("lock");
    let app = dependency_project(&temp);

    let first = run(&["check", app.to_str().unwrap(), "--json"]);
    let value = json(&first);
    assert_json_envelope(&value, "check");
    let digest_one = value["summary"]["dependencies_digest"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(digest_one.starts_with("sha256:"));
    let dependencies = value["summary"]["dependencies"].as_array().unwrap();
    assert_eq!(dependencies.len(), 1);
    assert_eq!(dependencies[0]["name"], "util");
    assert!(app.join("ravum.lock").is_file());

    let lock_one = fs::read(app.join("ravum.lock")).unwrap();
    let second = run(&["check", app.to_str().unwrap(), "--json"]);
    assert_json_envelope(&json(&second), "check");
    let lock_two = fs::read(app.join("ravum.lock")).unwrap();
    assert_eq!(
        lock_one, lock_two,
        "lockfile must be byte-identical across runs"
    );

    // Changing a dependency source must change the digest and the lockfile.
    temp.write("util/src/lib.rv", "pub fn helper() -> i32 { return 2; }\n");
    let third = run(&["check", app.to_str().unwrap(), "--json"]);
    let value = json(&third);
    assert_json_envelope(&value, "check");
    let digest_two = value["summary"]["dependencies_digest"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(digest_one, digest_two);
    let lock_three = fs::read(app.join("ravum.lock")).unwrap();
    assert_ne!(lock_two, lock_three);
}

#[test]
fn locked_mode_reports_missing_lockfile() {
    let temp = TempDir::new("locked-missing");
    let app = dependency_project(&temp);
    let _ = fs::remove_file(app.join("ravum.lock"));

    let output = run(&["check", app.to_str().unwrap(), "--locked", "--json"]);
    assert_eq!(output.status.code(), Some(1));
    let value = json(&output);
    assert_json_envelope(&value, "check");
    assert!(
        diagnostic_codes(&value).contains(&"E-MOD-MISSING".to_string()),
        "codes: {:?}",
        diagnostic_codes(&value)
    );

    // Without --locked the file is created; a locked run then no longer reports
    // a lockfile problem (other stages may still fail).
    let _ = run(&["check", app.to_str().unwrap(), "--json"]);
    assert!(app.join("ravum.lock").is_file());
    let output = run(&["check", app.to_str().unwrap(), "--locked", "--json"]);
    let value = json(&output);
    let codes = diagnostic_codes(&value);
    assert!(
        !codes.iter().any(|code| code.starts_with("E-MOD-LOCK")),
        "unexpected lock diagnostics: {codes:?}"
    );
}

#[test]
fn absolute_dependency_path_is_rejected() {
    let temp = TempDir::new("dep-absolute");
    let target = temp.path().join("util");
    let app = temp.path().join("app");
    temp.write("util/ravum.toml", &manifest("util", ""));
    temp.write("util/src/lib.rv", "pub fn helper() -> i32 { return 1; }\n");
    let absolute = target.to_string_lossy().replace('\\', "/");
    temp.write(
        "app/ravum.toml",
        &manifest(
            "app",
            &format!("\n[dependencies]\nutil = {{ path = \"{absolute}\" }}\n"),
        ),
    );
    temp.write("app/src/main.rv", "fn main() { }\n");

    let output = run(&["check", app.to_str().unwrap(), "--json"]);
    assert_eq!(output.status.code(), Some(1));
    let value = json(&output);
    let codes = diagnostic_codes(&value);
    assert!(
        codes
            .iter()
            .any(|code| code == "E-MOD-PATH-ESCAPE" || code == "E-MOD-DEPENDENCY"),
        "codes: {codes:?}"
    );
}

#[test]
fn nonexistent_dependency_is_rejected() {
    let temp = TempDir::new("dep-nonexistent");
    let app = temp.path().join("app");
    temp.write(
        "app/ravum.toml",
        &manifest(
            "app",
            "\n[dependencies]\nutil = { path = \"../missing\" }\n",
        ),
    );
    temp.write("app/src/main.rv", "fn main() { }\n");
    let output = run(&["check", app.to_str().unwrap(), "--json"]);
    assert_eq!(output.status.code(), Some(1));
    let value = json(&output);
    assert!(diagnostic_codes(&value).contains(&"E-MOD-DEPENDENCY".to_string()));
}

#[test]
fn dependency_name_mismatch_is_rejected() {
    let temp = TempDir::new("dep-name");
    temp.write("util/ravum.toml", &manifest("other", ""));
    temp.write("util/src/lib.rv", "pub fn helper() -> i32 { return 1; }\n");
    let app = temp.path().join("app");
    temp.write(
        "app/ravum.toml",
        &manifest("app", "\n[dependencies]\nutil = { path = \"../util\" }\n"),
    );
    temp.write("app/src/main.rv", "fn main() { }\n");
    let output = run(&["check", app.to_str().unwrap(), "--json"]);
    assert_eq!(output.status.code(), Some(1));
    let value = json(&output);
    assert!(diagnostic_codes(&value).contains(&"E-MOD-DEPENDENCY".to_string()));
}

#[test]
fn local_dependency_check_build_run() {
    let temp = TempDir::new("dep-e2e");
    temp.write("util/ravum.toml", &manifest("util", ""));
    temp.write(
        "util/src/lib.rv",
        "pub fn helper() -> i32 {\n    return 7;\n}\n",
    );
    let app = temp.path().join("app");
    temp.write(
        "app/ravum.toml",
        &manifest("app", "\n[dependencies]\nutil = { path = \"../util\" }\n"),
    );
    temp.write(
        "app/src/main.rv",
        "use std::io;\nuse util::helper;\nfn main() {\n    io::println(\"dep\");\n    let n = helper();\n}\n",
    );

    let output = run(&["check", app.to_str().unwrap(), "--json"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}\nstdout: {}",
        stderr(&output),
        stdout(&output)
    );
    let value = json(&output);
    assert_json_envelope(&value, "check");
    assert_eq!(value["success"], true);
    assert_eq!(value["summary"]["dependencies"][0]["name"], "util");
    assert!(value["summary"]["dependencies_digest"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));

    let output = run(&["build", app.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "stderr: {}\nstdout: {}",
        stderr(&output),
        stdout(&output)
    );
    let output = run(&["run", app.to_str().unwrap()]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(stdout(&output).contains("dep"));
}

// ---------------------------------------------------------------------------
// build / run / test failures that do not depend on the checker
// ---------------------------------------------------------------------------

#[test]
fn build_invalid_program_fails_honestly() {
    let temp = TempDir::new("build-invalid");
    let package = temp.path().join("app");
    assert!(run(&["init", package.to_str().unwrap()]).status.success());
    fs::write(package.join("src/main.rv"), "fn main( {\n").unwrap();
    let output = run(&["build", package.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(!package.join(".ravum/bin").exists());
    assert!(stderr(&output).contains("error"));
}

#[test]
fn run_invalid_program_fails_honestly() {
    let temp = TempDir::new("run-invalid");
    let package = temp.path().join("app");
    assert!(run(&["init", package.to_str().unwrap()]).status.success());
    fs::write(package.join("src/main.rv"), "fn main( {\n").unwrap();
    let output = run(&["run", package.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("error"));
}

#[test]
fn test_invalid_program_reports_json() {
    let temp = TempDir::new("test-invalid");
    let package = temp.path().join("app");
    assert!(run(&["init", package.to_str().unwrap()]).status.success());
    fs::write(package.join("src/main.rv"), "fn main( {\n").unwrap();
    let output = run(&["test", package.to_str().unwrap(), "--json"]);
    assert_eq!(output.status.code(), Some(1));
    let value = json(&output);
    assert_json_envelope(&value, "test");
    assert_eq!(value["success"], false);
    assert_eq!(value["summary"]["total"], 0);
}

#[test]
fn build_and_run_hello() {
    if Command::new("rustc").arg("--version").output().is_err() {
        eprintln!("SKIP: rustc is not installed");
        return;
    }
    let temp = TempDir::new("build-hello");
    let package = temp.path().join("hello");
    assert!(run(&["init", package.to_str().unwrap()]).status.success());
    let output = run(&["build", package.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "stderr: {}\nstdout: {}",
        stderr(&output),
        stdout(&output)
    );
    let binary = if cfg!(windows) {
        package.join(".ravum/bin/hello.exe")
    } else {
        package.join(".ravum/bin/hello")
    };
    assert!(binary.is_file(), "missing {}", binary.display());

    let output = run(&["run", package.to_str().unwrap(), "--", "alpha", "beta"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(stdout(&output).contains("hello, ravum"));
}

#[test]
fn test_runner_executes_tests_and_reports_failures() {
    let temp = TempDir::new("test-runner");
    let package = temp.path().join("app");
    assert!(run(&["init", package.to_str().unwrap()]).status.success());

    fs::write(
        package.join("src/main.rv"),
        "fn main() {\n}\n\ntest fn simple() {\n    assert_eq(1 + 1, 2);\n}\n",
    )
    .unwrap();
    let output = run(&["test", package.to_str().unwrap(), "--json"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}\nstdout: {}",
        stderr(&output),
        stdout(&output)
    );
    let value = json(&output);
    assert_json_envelope(&value, "test");
    assert_eq!(value["summary"]["passed"], 1);
    assert_eq!(value["summary"]["failed"], 0);
    assert_eq!(value["summary"]["tests"][0]["name"], "simple");
    assert_eq!(value["summary"]["tests"][0]["status"], "ok");

    fs::write(
        package.join("src/main.rv"),
        "fn main() {\n}\n\ntest fn broken() {\n    assert_eq(1, 2);\n}\n",
    )
    .unwrap();
    let output = run(&["test", package.to_str().unwrap(), "--json"]);
    assert_eq!(output.status.code(), Some(1));
    let value = json(&output);
    assert_json_envelope(&value, "test");
    assert_eq!(value["success"], false);
    assert_eq!(value["summary"]["failed"], 1);
    assert_eq!(value["summary"]["tests"][0]["status"], "FAILED");
}

#[test]
fn build_emit_rust_and_emit_ir() {
    let temp = TempDir::new("emit");
    let package = temp.path().join("hello");
    assert!(run(&["init", package.to_str().unwrap()]).status.success());
    let emitted = temp.path().join("out/generated.rs");
    let output = run(&[
        "build",
        package.to_str().unwrap(),
        "--emit-rust",
        emitted.to_str().unwrap(),
        "--emit-ir",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}\nstdout: {}",
        stderr(&output),
        stdout(&output)
    );
    let text = stdout(&output);
    assert!(text.contains("functions:"), "IR dump missing: {text}");
    assert!(text.contains("crate::main"), "IR dump missing main: {text}");
    assert!(text.to_lowercase().contains("not a stable ir"));
    assert!(emitted.is_file(), "generated Rust was not written");
    let generated = fs::read_to_string(&emitted).unwrap();
    assert!(generated.contains("fn main"), "generated Rust: {generated}");
}

// ---------------------------------------------------------------------------
// fmt / lint / doc
// ---------------------------------------------------------------------------

#[test]
fn fmt_check_is_idempotent() {
    let temp = TempDir::new("fmt");
    let package = temp.path().join("app");
    assert!(run(&["init", package.to_str().unwrap()]).status.success());
    // The generated template is already canonical.
    let output = run(&["fmt", package.to_str().unwrap(), "--check"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "init output is not formatted:\nstdout: {}\nstderr: {}",
        stdout(&output),
        stderr(&output)
    );
    let output = run(&["fmt", package.to_str().unwrap()]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let output = run(&["fmt", package.to_str().unwrap(), "--check"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}\nstdout: {}",
        stderr(&output),
        stdout(&output)
    );
}

#[test]
fn fmt_missing_path_fails() {
    let temp = TempDir::new("fmt-missing");
    let missing = temp.path().join("nope");
    let output = run(&["fmt", missing.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("error"));
}

#[test]
fn doc_generates_markdown_for_public_items() {
    let temp = TempDir::new("doc");
    let package = temp.path().join("lib");
    assert!(run(&["init", package.to_str().unwrap(), "--lib"])
        .status
        .success());
    let out = temp.path().join("docs");
    let output = run(&[
        "doc",
        package.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "stderr: {}\nstdout: {}",
        stderr(&output),
        stdout(&output)
    );
    let markdown = fs::read_to_string(out.join("index.md")).expect("index.md");
    assert!(markdown.contains("pub fn answer() -> i32"));
    assert!(markdown.contains("Ravum API documentation"));
}

#[test]
fn doc_fails_on_invalid_program() {
    let temp = TempDir::new("doc-invalid");
    let package = temp.path().join("app");
    assert!(run(&["init", package.to_str().unwrap()]).status.success());
    fs::write(package.join("src/main.rv"), "fn main( {\n").unwrap();
    let out = temp.path().join("docs");
    let output = run(&[
        "doc",
        package.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert!(!out.join("index.md").exists());
}

#[test]
fn lint_deny_turns_warning_into_error() {
    let temp = TempDir::new("lint");
    let package = temp.path().join("warn");
    assert!(run(&["init", package.to_str().unwrap()]).status.success());
    fs::write(
        package.join("src/main.rv"),
        "use std::io;\nfn main() {\n}\n",
    )
    .unwrap();

    let output = run(&["lint", package.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}\nstdout: {}",
        stderr(&output),
        stdout(&output)
    );
    assert!(stdout(&output).contains("1 warning(s)"));

    let output = run(&[
        "lint",
        package.to_str().unwrap(),
        "--deny",
        "W-RES-UNUSED-IMPORT",
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).contains("1 denied warning(s)"));

    let output = run(&["lint", package.to_str().unwrap(), "--deny", "all"]);
    assert_eq!(output.status.code(), Some(1));
}

// ---------------------------------------------------------------------------
// cache / doctor / lsp
// ---------------------------------------------------------------------------

#[test]
fn cache_status_and_clean_are_safe() {
    let temp = TempDir::new("cache");
    let package = temp.path().join("app");
    assert!(run(&["init", package.to_str().unwrap()]).status.success());
    fs::create_dir_all(package.join(".ravum/gen")).unwrap();
    fs::write(package.join(".ravum/gen/main.rs"), "fn main() {}\n").unwrap();

    let output = run(&["cache", "status", package.to_str().unwrap()]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("state: present"));
    assert!(text.contains(".ravum"));
    assert!(text.contains("gc:"));

    let output = run(&["cache", "clean", package.to_str().unwrap()]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(!package.join(".ravum").exists());
    assert!(package.join("src/main.rv").is_file());
    assert!(package.join("ravum.toml").is_file());

    let output = run(&["cache", "clean", package.to_str().unwrap()]);
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains("nothing to clean"));
}

#[test]
fn cache_status_reports_absent_cache() {
    let temp = TempDir::new("cache-absent");
    let package = temp.path().join("app");
    assert!(run(&["init", package.to_str().unwrap()]).status.success());
    let output = run(&["cache", "status", package.to_str().unwrap()]);
    assert!(output.status.success());
    assert!(stdout(&output).contains("state: absent"));
}

#[test]
fn doctor_json_is_valid_and_honest() {
    let temp = TempDir::new("doctor");
    let output = run_in(temp.path(), &["doctor", "--json"]);
    let value = json(&output);
    assert_json_envelope(&value, "doctor");
    let environment = &value["summary"]["environment"];
    assert!(environment["rustc"]["present"].is_boolean());
    assert!(environment["cache_dir"].is_string());
    for stage in ["parser", "checker", "codegen"] {
        let status = value["summary"]["frontend"][stage].as_str().unwrap();
        assert!(
            ["ready", "pending", "error"].contains(&status),
            "unexpected {stage} status: {status}"
        );
    }
    let verdict = value["summary"]["verdict"].as_str().unwrap();
    assert!(["ready", "partial", "error", "blocked"].contains(&verdict));
    let expected = if environment["rustc"]["present"] == true {
        0
    } else {
        3
    };
    assert_eq!(output.status.code(), Some(expected));
}

#[test]
fn lsp_rejects_arguments_instead_of_pretending() {
    // The protocol arrives on stdin, so any argument is a usage error (exit 2).
    // The served protocol itself is covered end-to-end by `tests/lsp_client.rs`.
    let output = run(&["lsp", "--json"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("lsp"));
}

// ---------------------------------------------------------------------------
// watch / dev (polling, restart, failure recovery, no leaked child)
// ---------------------------------------------------------------------------

/// Kills the watched process if a test panics or times out.
struct WatchChild {
    child: Option<Child>,
}

impl WatchChild {
    fn wait_exit(&mut self, timeout: Duration) -> Option<i32> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(child) = self.child.as_mut() {
                match child.try_wait().expect("try_wait on watch") {
                    Some(status) => {
                        self.child.take();
                        return status.code();
                    }
                    None => {}
                }
            } else {
                return None;
            }
            if Instant::now() >= deadline {
                return None;
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for WatchChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn spawn_watch(dir: &Path, args: &[&str]) -> (WatchChild, Receiver<String>) {
    let mut child = Command::new(binary())
        .args(args)
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn ravum-lang watch");
    let stdout = child.stdout.take().expect("watch stdout");
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    (WatchChild { child: Some(child) }, receiver)
}

fn wait_watch_line(
    receiver: &Receiver<String>,
    what: &str,
    timeout: Duration,
    predicate: impl Fn(&str) -> bool,
) -> String {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            panic!("timeout waiting for watch output: {what}");
        }
        match receiver.recv_timeout(remaining) {
            Ok(line) => {
                if predicate(&line) {
                    return line;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                panic!("timeout waiting for watch output: {what}")
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                panic!("watch output closed before: {what}")
            }
        }
    }
}

fn watch_timeout() -> Duration {
    Duration::from_secs(20)
}

#[test]
fn watch_rebuilds_on_source_change_and_restarts_child() {
    let temp = TempDir::new("watch-rebuild");
    let package = temp.path().join("app");
    assert!(run(&["init", package.to_str().unwrap()]).status.success());
    fs::write(
        package.join("src/main.rv"),
        "use std::io;\nfn main() {\n    io::println(\"v1\");\n}\n",
    )
    .unwrap();

    let (mut watch, receiver) =
        spawn_watch(&package, &["watch", ".", "--run", "--max-rebuilds", "1"]);
    wait_watch_line(&receiver, "watching", watch_timeout(), |line| {
        line.contains("[ravum] watching")
    });
    wait_watch_line(&receiver, "initial build ok", watch_timeout(), |line| {
        line == "[ravum] build ok"
    });
    wait_watch_line(&receiver, "v1 output", watch_timeout(), |line| line == "v1");

    fs::write(
        package.join("src/main.rv"),
        "use std::io;\nfn main() {\n    io::println(\"v2\");\n}\n",
    )
    .unwrap();
    wait_watch_line(&receiver, "change detected", watch_timeout(), |line| {
        line.contains("[ravum] change detected")
    });
    wait_watch_line(&receiver, "rebuild ok", watch_timeout(), |line| {
        line == "[ravum] build ok"
    });
    wait_watch_line(&receiver, "v2 output", watch_timeout(), |line| line == "v2");

    assert_eq!(
        watch.wait_exit(watch_timeout()),
        Some(0),
        "watch must exit 0 after --max-rebuilds"
    );
}

#[test]
fn watch_continues_after_compile_failure() {
    let temp = TempDir::new("watch-failure");
    let package = temp.path().join("app");
    assert!(run(&["init", package.to_str().unwrap()]).status.success());
    fs::write(
        package.join("src/main.rv"),
        "use std::io;\nfn main() {\n    io::println(\"ok\");\n}\n",
    )
    .unwrap();

    let (mut watch, receiver) = spawn_watch(&package, &["dev", ".", "--max-rebuilds", "2"]);
    wait_watch_line(&receiver, "watching", watch_timeout(), |line| {
        line.contains("[ravum] watching")
    });
    wait_watch_line(&receiver, "initial build ok", watch_timeout(), |line| {
        line == "[ravum] build ok"
    });

    fs::write(package.join("src/main.rv"), "fn main( {\n").unwrap();
    wait_watch_line(&receiver, "change detected", watch_timeout(), |line| {
        line.contains("[ravum] change detected")
    });
    wait_watch_line(&receiver, "build failed", watch_timeout(), |line| {
        line.contains("[ravum] build failed (exit 1)")
    });

    fs::write(
        package.join("src/main.rv"),
        "use std::io;\nfn main() {\n    io::println(\"fixed\");\n}\n",
    )
    .unwrap();
    wait_watch_line(&receiver, "second change", watch_timeout(), |line| {
        line.contains("[ravum] change detected")
    });
    wait_watch_line(&receiver, "recovery build ok", watch_timeout(), |line| {
        line == "[ravum] build ok"
    });

    assert_eq!(watch.wait_exit(watch_timeout()), Some(0));
}

#[test]
fn watch_reaps_the_child_process() {
    let temp = TempDir::new("watch-reap");
    let package = temp.path().join("loopapp");
    assert!(run(&["init", package.to_str().unwrap()]).status.success());
    let busy = "fn main() {\n    loop {\n    }\n}\n";
    fs::write(package.join("src/main.rv"), busy).unwrap();

    let (mut watch, receiver) =
        spawn_watch(&package, &["watch", ".", "--run", "--max-rebuilds", "1"]);
    wait_watch_line(&receiver, "watching", watch_timeout(), |line| {
        line.contains("[ravum] watching")
    });
    wait_watch_line(&receiver, "initial build ok", watch_timeout(), |line| {
        line == "[ravum] build ok"
    });
    wait_watch_line(&receiver, "child spawn", watch_timeout(), |line| {
        line.contains("[ravum] running")
    });

    fs::write(
        package.join("src/main.rv"),
        "fn main() {\n    loop {\n    }\n}\n// touched\n",
    )
    .unwrap();
    wait_watch_line(&receiver, "change detected", watch_timeout(), |line| {
        line.contains("[ravum] change detected")
    });
    wait_watch_line(&receiver, "rebuild ok", watch_timeout(), |line| {
        line == "[ravum] build ok"
    });
    assert_eq!(watch.wait_exit(watch_timeout()), Some(0));

    // On Windows a still-running child keeps the executable locked, so a
    // successful removal proves the process tree was reaped.
    let bin_dir = package.join(".ravum/bin");
    let executable = fs::read_dir(&bin_dir)
        .expect("cache bin dir")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().and_then(|ext| ext.to_str()) != Some("cjs"))
        .expect("built executable");
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match fs::remove_file(&executable) {
            Ok(()) => break,
            Err(error) => {
                if Instant::now() >= deadline {
                    panic!(
                        "executable is still locked after watch exit (leaked child?): \
                         {} ({error})",
                        executable.display()
                    );
                }
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// WASM target (wasm32-wasip1 via Node.js WASI preview1)
// ---------------------------------------------------------------------------

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

fn node_ready() -> bool {
    let node = std::env::var("RAVUM_NODE").unwrap_or_else(|_| "node".to_string());
    Command::new(node)
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[test]
fn wasm_build_and_run_hello() {
    if !rustc_wasm_ready() {
        eprintln!("SKIP: rustc target wasm32-wasip1 is not installed");
        return;
    }
    if !node_ready() {
        eprintln!("SKIP: Node.js is not available");
        return;
    }
    let temp = TempDir::new("wasm-hello");
    let package = temp.path().join("hello");
    assert!(run(&["init", package.to_str().unwrap()]).status.success());

    let output = run(&[
        "build",
        package.to_str().unwrap(),
        "--target",
        "wasm32-wasip1",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}\nstdout: {}",
        stderr(&output),
        stdout(&output)
    );
    let wasm = package.join(".ravum/bin/hello.wasm");
    assert!(wasm.is_file(), "missing {}", wasm.display());
    let bytes = fs::read(&wasm).expect("read wasm");
    assert_eq!(&bytes[..4], b"\0asm", "not a WebAssembly module");

    let output = run(&[
        "run",
        package.to_str().unwrap(),
        "--target",
        "wasm32-wasip1",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}\nstdout: {}",
        stderr(&output),
        stdout(&output)
    );
    assert!(stdout(&output).contains("hello, ravum"));
}

#[test]
fn wasm_run_fib_example() {
    if !rustc_wasm_ready() || !node_ready() {
        eprintln!("SKIP: wasm32-wasip1 rustc target or Node.js is unavailable");
        return;
    }
    let fib = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/fib");
    if !fib.join("ravum.toml").is_file() {
        eprintln!("SKIP: examples/fib is not present");
        return;
    }
    let output = run(&["run", fib.to_str().unwrap(), "--target", "wasm32-wasip1"]);
    assert!(
        output.status.success(),
        "stderr: {}\nstdout: {}",
        stderr(&output),
        stdout(&output)
    );
    assert!(stdout(&output).contains("55"));
}

#[test]
fn wasm_forwards_arguments() {
    if !rustc_wasm_ready() || !node_ready() {
        eprintln!("SKIP: wasm32-wasip1 rustc target or Node.js is unavailable");
        return;
    }
    let calculator = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/calculator");
    if !calculator.join("ravum.toml").is_file() {
        eprintln!("SKIP: examples/calculator is not present");
        return;
    }
    let output = run(&[
        "run",
        calculator.to_str().unwrap(),
        "--target",
        "wasm32-wasip1",
        "--",
        "5",
        "+",
        "3",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}\nstdout: {}",
        stderr(&output),
        stdout(&output)
    );
    assert!(stdout(&output).contains('8'));
}

#[test]
fn wasm_forwards_exit_code_and_stderr() {
    if !rustc_wasm_ready() || !node_ready() {
        eprintln!("SKIP: wasm32-wasip1 rustc target or Node.js is unavailable");
        return;
    }
    let temp = TempDir::new("wasm-exit");
    let package = temp.path().join("failing");
    assert!(run(&["init", package.to_str().unwrap()]).status.success());
    fs::write(
        package.join("src/main.rv"),
        "fn main() -> Result<(), Error> {\n    return Err(Error::new(\"boom\"));\n}\n",
    )
    .unwrap();
    let output = run(&[
        "run",
        package.to_str().unwrap(),
        "--target",
        "wasm32-wasip1",
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("error: boom"));
}

#[test]
fn wasm_unknown_target_is_usage_error() {
    let temp = TempDir::new("wasm-target");
    let package = temp.path().join("hello");
    assert!(run(&["init", package.to_str().unwrap()]).status.success());
    let output = run(&[
        "build",
        package.to_str().unwrap(),
        "--target",
        "x86_64-unknown-none",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("unknown target"));

    let output = run(&["run", package.to_str().unwrap(), "--target", "wasm32-wasi"]);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn wasm_missing_node_reports_runtime_error() {
    let temp = TempDir::new("wasm-node");
    let package = temp.path().join("hello");
    assert!(run(&["init", package.to_str().unwrap()]).status.success());
    let output = Command::new(binary())
        .args([
            "run",
            package.to_str().unwrap(),
            "--target",
            "wasm32-wasip1",
        ])
        .env("RAVUM_NODE", "ravum-node-that-does-not-exist-xyz")
        .output()
        .expect("run ravum-lang");
    assert_eq!(output.status.code(), Some(3));
    assert!(
        stderr(&output).contains("E-CLI-WASM-RUNTIME"),
        "stderr: {}",
        stderr(&output)
    );
}

/// `E-LIMIT-FILE-SIZE` (P0-H robustness bound, `MAX_FILE_SIZE` = 8 MiB): a real
/// source file above the limit is refused before parsing with exactly that code
/// (`diagnostics.md` §3.1). The fixture is generated at run time: an 8 MiB file
/// committed to the repository would be roughly fifty times the whole
/// conformance corpus.
#[test]
fn oversized_source_file_is_refused_with_e_limit_file_size() {
    let temp = TempDir::new("oversized");
    temp.write("ravum.toml", &manifest("oversized", ""));
    let mut source = String::from("fn main() { }\n");
    // 8 MiB of padding guarantees `len() > MAX_FILE_SIZE` in the loader.
    source.push_str(&" ".repeat(8 * 1024 * 1024));
    temp.write("src/main.rv", &source);

    let output = run(&["check", "--json", temp.path().to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "an oversized file must fail the check; stderr: {}",
        stderr(&output)
    );
    let value = json(&output);
    assert_json_envelope(&value, "check");
    let codes: Vec<String> = value["diagnostics"]
        .as_array()
        .expect("diagnostics array")
        .iter()
        .filter_map(|diagnostic| diagnostic["code"].as_str().map(str::to_string))
        .collect();
    assert!(
        codes.iter().any(|code| code == "E-LIMIT-FILE-SIZE"),
        "expected E-LIMIT-FILE-SIZE, got {codes:?}"
    );
}
