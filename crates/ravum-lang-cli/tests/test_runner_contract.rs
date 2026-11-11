//! P0-E — `ravum-lang test` must not report a partial or unverified run as
//! `test result: ok`.
//!
//! The unit tests in `src/commands/test.rs` cover the pure validation matrix
//! (truncation, count mismatch, filter mismatch, exit-code disagreement). This
//! file proves the same contract end to end through the real binary with a
//! `tests-demo`-style package.
//!
//! Every invocation is bounded by `Command::output()` (the runner is a short
//! program; no polling) and temp dirs are removed on drop.

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

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
            "ravum-cli-testcontract-{tag}-{}-{}-{nanos}",
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

fn cli(args: &[&str]) -> Output {
    Command::new(binary())
        .args(args)
        .output()
        .expect("run ravum-lang")
}

/// `ravum-lang test`, retrying **once** when the failure is a toolchain/link
/// infrastructure error: on a machine where several agents compile at the same
/// time, `link.exe` can transiently fail (`LNK1181`/`LNK1104`) without any
/// contract being violated. A Ravum diagnostic, a test failure or a validation
/// error is never retried — those are exactly what these tests check.
fn test_cli(args: &[&str]) -> Output {
    let output = cli(args);
    if output.status.success() {
        return output;
    }
    let err = stderr(&output);
    let toolchain_failure = err.contains("rustc failed")
        && !err.contains("error[")
        && (err.contains("LNK") || err.contains("cannot open input file"));
    if toolchain_failure {
        eprintln!("NOTE: transient toolchain failure, retrying once:\n{err}");
        return cli(args);
    }
    output
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

fn package_with_source(temp: &TempDir, name: &str, source: &str) -> PathBuf {
    let root = temp.path().join(name);
    let output = cli(&["init", root.to_str().unwrap()]);
    assert!(output.status.success(), "init failed: {}", stderr(&output));
    fs::write(root.join("src/main.rv"), source).expect("write main.rv");
    root
}

/// Copy the repository's `examples/tests-demo` package into a temp dir so the
/// test never writes `.ravum/` into the working tree.
fn copy_tests_demo(temp: &TempDir) -> PathBuf {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/tests-demo");
    assert!(
        source.join("ravum.toml").is_file(),
        "missing example package at {}",
        source.display()
    );
    let root = temp.path().join("tests-demo");
    fs::create_dir_all(root.join("src")).expect("create src");
    fs::copy(source.join("ravum.toml"), root.join("ravum.toml")).expect("copy manifest");
    fs::copy(source.join("src/main.rv"), root.join("src/main.rv")).expect("copy main.rv");
    root
}

fn rustc_available() -> bool {
    Command::new("rustc")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn skip_without_rustc(test: &str) -> bool {
    if rustc_available() {
        return false;
    }
    eprintln!("SKIP {test}: rustc is not installed (the test runner cannot be built)");
    true
}

#[test]
fn tests_demo_package_reports_every_declared_test() {
    if skip_without_rustc("tests_demo_package_reports_every_declared_test") {
        return;
    }
    let temp = TempDir::new("tests-demo");
    let root = copy_tests_demo(&temp);

    let output = test_cli(&["test", root.to_str().unwrap(), "--json"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}\nstdout: {}",
        stderr(&output),
        stdout(&output)
    );
    let value = json(&output);
    assert_eq!(value["command"], "test");
    assert_eq!(value["success"], true);
    let summary = &value["summary"];
    assert_eq!(summary["declared"], 3, "summary: {summary}");
    assert_eq!(summary["total"], 3, "summary: {summary}");
    assert_eq!(summary["passed"], 3, "summary: {summary}");
    assert_eq!(summary["failed"], 0, "summary: {summary}");
    assert_eq!(summary["ignored"], 0, "summary: {summary}");
    assert_eq!(summary["filtered"], 0, "summary: {summary}");
    assert_eq!(
        summary["validation_errors"]
            .as_array()
            .map(Vec::len)
            .unwrap_or(0),
        0,
        "summary: {summary}"
    );
    let names = summary["tests"]
        .as_array()
        .expect("tests array")
        .iter()
        .map(|entry| entry["name"].as_str().unwrap_or("").to_string())
        .collect::<Vec<_>>();
    for expected in ["addition_works", "strings_work", "result_propagation_works"] {
        assert!(
            names.iter().any(|name| name == expected),
            "missing {expected}: {names:?}"
        );
    }
}

#[test]
fn text_output_reports_declared_and_executed_counts() {
    if skip_without_rustc("text_output_reports_declared_and_executed_counts") {
        return;
    }
    let temp = TempDir::new("tests-demo-text");
    let root = copy_tests_demo(&temp);

    let output = test_cli(&["test", root.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let text = stdout(&output);
    assert!(
        text.contains("test addition_works ... ok"),
        "runner output must be shown: {text}"
    );
    let result_lines = text
        .lines()
        .filter(|line| line.trim_start().starts_with("test result:"))
        .collect::<Vec<_>>();
    assert_eq!(
        result_lines.len(),
        1,
        "exactly one `test result:` line (no duplicate CLI summary):\n{text}"
    );
    assert_eq!(
        result_lines[0].trim(),
        "test result: ok; 3 passed; 0 failed; 0 ignored; 0 filtered out",
        "the runner's measured summary is the primary output:\n{text}"
    );
    assert!(
        text.contains("test cases: 3 declared; 0 filtered out; 3 executed"),
        "the CLI accounting line is separate from `test result:`:\n{text}"
    );
}

#[test]
fn a_failing_test_is_a_consistent_failure_not_a_validation_error() {
    if skip_without_rustc("a_failing_test_is_a_consistent_failure_not_a_validation_error") {
        return;
    }
    let temp = TempDir::new("failing");
    let root = package_with_source(
        &temp,
        "mixed",
        "fn main() {\n}\n\n\
         test fn passes() {\n    assert_eq(1 + 1, 2);\n}\n\n\
         test fn fails() {\n    assert_eq(1, 2);\n}\n",
    );

    let output = test_cli(&["test", root.to_str().unwrap(), "--json"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "a failing test must exit 1; stderr: {}",
        stderr(&output)
    );
    let value = json(&output);
    let summary = &value["summary"];
    assert_eq!(summary["declared"], 2, "summary: {summary}");
    assert_eq!(summary["total"], 2, "summary: {summary}");
    assert_eq!(summary["passed"], 1, "summary: {summary}");
    assert_eq!(summary["failed"], 1, "summary: {summary}");
    assert_eq!(
        summary["validation_errors"]
            .as_array()
            .map(Vec::len)
            .unwrap_or(0),
        0,
        "the run is complete — only the test verdict differs: {summary}"
    );
    assert_eq!(value["success"], false);
}

#[test]
fn filter_is_applied_and_accounted_for() {
    if skip_without_rustc("filter_is_applied_and_accounted_for") {
        return;
    }
    let temp = TempDir::new("filter");
    let root = copy_tests_demo(&temp);

    let output = test_cli(&[
        "test",
        root.to_str().unwrap(),
        "--filter",
        "addition_works",
        "--json",
    ]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "the emitted runner applies --filter; stderr: {}",
        stderr(&output)
    );
    let value = json(&output);
    let summary = &value["summary"];
    assert_eq!(summary["declared"], 3, "summary: {summary}");
    assert_eq!(summary["total"], 1, "only the matching test ran: {summary}");
    assert_eq!(summary["passed"], 1, "summary: {summary}");
    assert_eq!(summary["filtered"], 2, "summary: {summary}");
    assert_eq!(
        summary["validation_errors"]
            .as_array()
            .map(Vec::len)
            .unwrap_or(0),
        0,
        "summary: {summary}"
    );
    for entry in summary["tests"].as_array().expect("tests array") {
        let name = entry["name"].as_str().unwrap_or("");
        assert!(
            name.contains("addition_works"),
            "an executed test does not match the filter: {name}"
        );
    }
    assert_eq!(value["success"], true);

    // A filter that matches nothing is an honest empty run.
    let output = test_cli(&[
        "test",
        root.to_str().unwrap(),
        "--filter",
        "no-such-test-9f13",
        "--json",
    ]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let value = json(&output);
    assert_eq!(value["summary"]["total"], 0);
    assert_eq!(value["summary"]["filtered"], 3);
}

#[test]
fn a_package_without_tests_is_an_honest_empty_run() {
    if skip_without_rustc("a_package_without_tests_is_an_honest_empty_run") {
        return;
    }
    let temp = TempDir::new("empty");
    let root = package_with_source(&temp, "notests", "fn main() {\n}\n");

    let output = test_cli(&["test", root.to_str().unwrap(), "--json"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "an empty package is a legitimate empty run; stderr: {}",
        stderr(&output)
    );
    let value = json(&output);
    assert_eq!(value["success"], true);
    assert_eq!(value["summary"]["declared"], 0);
    assert_eq!(value["summary"]["total"], 0);
    assert_eq!(value["summary"]["passed"], 0);
}
