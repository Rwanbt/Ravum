//! Real integration tests for `ravum unit-check` (MVP-0c L5.1).
//!
//! The tests run the actual `ravum` binary against `fixtures/multicrate`:
//! successful campaign, scoped campaign (`--changed`), a real broken interface
//! (rustc diagnostics), invalid arguments, and the guarantee that the user
//! workspace is never modified (the analysis runs on a copy).
//!
//! Rustc-heavy tests are meant to run with `--test-threads=1`.

use ravum_unit_graph::sha256_hex;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const SKIPPED_DIRS: &[&str] = &[".git", "target", "node_modules", ".cache"];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn fixture_dir() -> PathBuf {
    repo_root().join("fixtures/multicrate")
}

fn tests_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/ravum-tests/unit-check")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create test dir");
    dir
}

fn run_ravum(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ravum"))
        .args(args)
        .output()
        .expect("run ravum")
}

fn parse_json(output: &Output) -> serde_json::Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout).unwrap_or_else(|error| {
        panic!(
            "unit-check --json output is not JSON: {error}\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn walk_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&directory)
            .expect("read dir")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();
        entries.sort();
        for path in entries {
            let metadata = std::fs::symlink_metadata(&path).expect("metadata");
            if metadata.is_dir() {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("");
                if !SKIPPED_DIRS.contains(&name) {
                    stack.push(path);
                }
            } else if metadata.is_file() {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// Content fingerprint of a source tree (same idea as the CLI: never modified).
fn fingerprint(root: &Path) -> String {
    let mut text = String::new();
    for path in walk_files(root) {
        let relative = path
            .strip_prefix(root)
            .expect("inside root")
            .to_string_lossy()
            .replace('\\', "/");
        let bytes = std::fs::read(&path).expect("read file");
        text.push_str(&relative);
        text.push('\0');
        text.push_str(&sha256_hex(&bytes));
        text.push('\0');
    }
    sha256_hex(text.as_bytes())
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("create destination");
    for path in walk_files(from) {
        let relative = path.strip_prefix(from).expect("inside source");
        let target = to.join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        std::fs::copy(&path, &target).expect("copy file");
    }
}

#[test]
fn unit_check_succeeds_on_multicrate_without_touching_the_workspace() {
    let state = tests_dir("readonly-state");
    let before = fingerprint(&fixture_dir());
    assert!(
        !fixture_dir().join("Cargo.lock").exists(),
        "the fixture must be lock-free before the test"
    );

    let output = run_ravum(&[
        "unit-check",
        fixture_dir().to_str().unwrap(),
        "--state",
        state.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "unit-check must succeed on the fixture: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = parse_json(&output);
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["command"], "ravum unit-check");
    assert_eq!(report["valid"], true);
    assert_eq!(report["errors"], 0);
    assert_eq!(report["stale"], 0);
    assert_eq!(report["exit_code"], 0);
    assert_eq!(report["workspace_unchanged"], true);
    assert_eq!(report["units"].as_array().expect("units").len(), 6);
    assert!(
        report["rustc_invocations"].as_u64().unwrap_or(0) > 0,
        "a real campaign must invoke rustc: {report}"
    );
    assert!(
        report["cargo_fallback_invocations"].as_u64().unwrap_or(0) > 0,
        "the fixture has fallback units: {report}"
    );

    // No user file was created or modified by the analysis.
    assert_eq!(
        fingerprint(&fixture_dir()),
        before,
        "fixture must be untouched"
    );
    assert!(!fixture_dir().join("Cargo.lock").exists());
    assert!(!fixture_dir().join("target").exists());
}

#[test]
fn unit_check_restricts_the_scope_with_changed_paths() {
    let state = tests_dir("changed-scope");
    let output = run_ravum(&[
        "unit-check",
        fixture_dir().to_str().unwrap(),
        "--changed",
        "core/src/lib.rs",
        "--state",
        state.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = parse_json(&output);
    assert_eq!(report["changed_units"], serde_json::json!(["core"]));
    let affected: Vec<&str> = report["affected_units"]
        .as_array()
        .expect("affected_units")
        .iter()
        .filter_map(|value| value.as_str())
        .collect();
    for expected in ["core", "api", "consumer", "application"] {
        assert!(
            affected.contains(&expected),
            "affected must contain {expected}: {affected:?}"
        );
    }
    for unrelated in ["features", "buildscript"] {
        assert!(
            !affected.contains(&unrelated),
            "unrelated unit {unrelated} must not be in scope: {affected:?}"
        );
    }
}

#[test]
fn unit_check_broken_interface_exits_one_with_real_diagnostics() {
    let workspace = tests_dir("broken-workspace");
    copy_tree(&fixture_dir(), &workspace);
    let core = workspace.join("core/src/lib.rs");
    let original = std::fs::read_to_string(&core).expect("read core");
    let patched = original
        .replace(
            "pub fn scale(x: f64) -> f64 {\n    x * 2.0\n}",
            "pub fn scale(x: f32) -> f32 {\n    x * 2.0_f32\n}",
        )
        .replace(
            "    scale(x) * internal_factor() / internal_factor()",
            "    scale(x as f32) as f64 * internal_factor() / internal_factor()",
        );
    assert_ne!(patched, original, "the interface patch must apply");
    std::fs::write(&core, patched).expect("write broken core");

    let state = tests_dir("broken-state");
    let output = run_ravum(&[
        "unit-check",
        workspace.to_str().unwrap(),
        "--state",
        state.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "a broken interface must exit 1: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = parse_json(&output);
    assert_eq!(report["exit_code"], 1);
    assert_eq!(report["workspace_unchanged"], true);
    let errors = report["errors"].as_u64().expect("errors");
    assert!(errors > 0, "the campaign must report errors: {report}");
    let api = report["units"]
        .as_array()
        .expect("units")
        .iter()
        .find(|unit| unit["unit"] == "api")
        .expect("api unit");
    assert_eq!(api["status"], "error");
    let diagnostics = api["diagnostics"].as_array().expect("diagnostics");
    assert!(
        diagnostics
            .iter()
            .any(|line| line.as_str().unwrap_or("").contains("E0308")),
        "rustc must produce the mismatched-types diagnostic: {diagnostics:?}"
    );
}

#[test]
fn unit_check_invalid_arguments_exit_two() {
    let fixture = fixture_dir();
    let fixture_argument = fixture.to_str().unwrap();
    let cases: Vec<Vec<&str>> = vec![
        vec!["unit-check"],
        vec!["unit-check", "--bogus"],
        vec!["unit-check", "definitely-not-a-directory"],
        vec![
            "unit-check",
            fixture_argument,
            "--changed",
            "not/a/unit/lib.rs",
        ],
        vec!["unit-check", fixture_argument, "extra"],
    ];
    for arguments in cases {
        let output = run_ravum(&arguments);
        assert_eq!(
            output.status.code(),
            Some(2),
            "arguments {arguments:?} must be a usage error, got {:?}: stderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn unit_check_out_writes_the_reported_payload() {
    let state = tests_dir("out-state");
    let out = tests_dir("out-report").join("report.json");
    let output = run_ravum(&[
        "unit-check",
        fixture_dir().to_str().unwrap(),
        "--state",
        state.to_str().unwrap(),
        "--changed",
        "core/src/lib.rs",
        "--json",
        "--out",
        out.to_str().unwrap(),
    ]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let written = std::fs::read_to_string(&out).expect("out report");
    let report: serde_json::Value = serde_json::from_str(&written).expect("out report JSON");
    assert_eq!(report["command"], "ravum unit-check");
    assert_eq!(report["changed_units"], serde_json::json!(["core"]));
}
