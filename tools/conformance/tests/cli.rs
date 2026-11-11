//! End-to-end tests of the `ravum-conformance` binary.
//!
//! The real `ravum-lang` toolchain may be absent or unfinished (the language is
//! under construction). These tests therefore use the harness binary itself as
//! a *stand-in* toolchain: `<exe> --version` succeeds, and any other command
//! exits 2. That is enough to prove the harness reports PASS, FAIL and SKIP
//! honestly, never invents a success, and sanitizes the JSON report.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn harness() -> &'static str {
    env!("CARGO_BIN_EXE_ravum-conformance")
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn repo_root() -> PathBuf {
    let manifest = manifest_dir();
    manifest
        .parent()
        .and_then(Path::parent)
        .expect("crate lives under <repo>/tools/conformance")
        .to_path_buf()
}

fn scratch(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "ravum-conformance-it-{}-{tag}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn run_in(cwd: &Path, args: &[&str]) -> Output {
    Command::new(harness())
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("spawn ravum-conformance")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn write_case(root: &Path, taxonomy: &str, name: &str, case_json: &str, files: &[(&str, &str)]) {
    let dir = root.join(taxonomy).join(name);
    fs::create_dir_all(&dir).expect("create case dir");
    fs::write(dir.join("case.json"), case_json).expect("write case.json");
    for (rel, content) in files {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(path, content).expect("write fixture");
    }
}

fn package_files() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "ravum.toml",
            "[package]\nname = \"it\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
        ),
        ("src/main.rv", "fn main() {\n}\n"),
    ]
}

fn status_of(text: &str, id: &str) -> String {
    text.lines()
        .find(|line| {
            line.contains(id) && line.starts_with("PASS")
                || line.contains(id) && line.starts_with("FAIL")
                || line.contains(id) && line.starts_with("SKIP")
                || line.contains(id) && line.starts_with("NOT_RUN")
        })
        .and_then(|line| line.split_whitespace().next())
        .unwrap_or("<missing>")
        .to_string()
}

#[test]
fn version_and_help_work() {
    let dir = scratch("version");
    let output = run_in(&dir, &["--version"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).contains("ravum-conformance"));
    assert!(stdout(&output).contains("schema 1"));

    let output = run_in(&dir, &["--help"]);
    assert_eq!(output.status.code(), Some(0));
    let text = stdout(&output);
    assert!(text.contains("USAGE:"));
    assert!(text.contains("--toolchain"));
    assert!(text.contains("--verify-build"));
    assert!(text.contains("build_verified"));

    let output = run_in(&dir, &["bogus-command"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("unknown command"));
}

#[test]
fn real_corpus_lists_without_invalid_cases() {
    let output = run_in(&manifest_dir(), &["list"]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let text = stdout(&output);
    assert!(
        text.contains("invalid: 0"),
        "corpus has invalid cases:\n{text}"
    );
    // The corpus must stay substantial; the mission requires at least 40 cases.
    let total_line = text
        .lines()
        .find(|line| line.starts_with("total:"))
        .expect("total line");
    let total: usize = total_line
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .expect("numeric total");
    assert!(total >= 40, "expected at least 40 cases, got {total}");
}

#[test]
fn missing_toolchain_is_not_run_and_report_has_no_user_paths() {
    let dir = scratch("missing-toolchain");
    let json_path = dir.join("report.json");
    let output = run_in(
        &dir,
        &[
            "run",
            "--cases",
            repo_root().join("conformance/cases").to_str().unwrap(),
            "--filter",
            "cli/version-flag",
            "--toolchain",
            "definitely-not-a-toolchain",
            "--json",
            json_path.to_str().unwrap(),
        ],
    );
    assert_eq!(output.status.code(), Some(3), "stderr: {}", stderr(&output));
    assert!(stdout(&output).contains("NOT_RUN"));
    assert!(stdout(&output).contains("summary: total=1 pass=0 fail=0 skip=0 not_run=1"));
    let report = fs::read_to_string(&json_path).expect("report written");
    assert!(report.contains("\"schema_version\": 1"));
    assert!(report.contains("\"not_run\": 1"));
    assert!(report.contains("\"status\": \"not_run\""));
    // A report produced without the flag must never claim build correctness.
    assert!(report.contains("\"verify_build\": false"));
    assert!(report.contains("\"build_verified\": false"));
    assert!(report.contains("\"build\": \"not_requested\""));
    let repo = repo_root().to_string_lossy().into_owned();
    assert!(
        !report.contains(&repo),
        "report contains an absolute repo path:\n{report}"
    );
}

#[test]
fn verify_build_reports_not_run_and_never_claims_build_verified() {
    let dir = scratch("verify-build-no-toolchain");
    let json_path = dir.join("report.json");
    let output = run_in(
        &dir,
        &[
            "run",
            "--cases",
            repo_root().join("conformance/cases").to_str().unwrap(),
            "--filter",
            "cli/version-flag",
            "--toolchain",
            "definitely-not-a-toolchain",
            "--verify-build",
            "--json",
            json_path.to_str().unwrap(),
        ],
    );
    assert_eq!(output.status.code(), Some(3), "stderr: {}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("NOT_RUN"));
    assert!(text.contains("build_verified: false"), "{text}");
    let report = fs::read_to_string(&json_path).expect("report written");
    assert!(report.contains("\"verify_build\": true"));
    assert!(report.contains("\"build_verified\": false"));
    assert!(report.contains("\"build\": \"not_attempted\""));
    assert!(report.contains("\"build_summary\""));
}

#[test]
fn verify_build_with_a_check_failure_records_not_attempted() {
    let dir = scratch("verify-build-check-fails");
    let cases = dir.join("cases");
    // The harness used as a stand-in toolchain exits 2 for `check`, so the
    // check fails before any build could be attempted.
    write_case(
        &cases,
        "lexer",
        "check-not-implemented",
        r#"{
  "id": "lexer/check-not-implemented",
  "mode": "compile-pass"
}"#,
        &package_files(),
    );
    let json_path = dir.join("report.json");
    let output = run_in(
        &dir,
        &[
            "run",
            "--cases",
            cases.to_str().unwrap(),
            "--toolchain",
            harness(),
            "--verify-build",
            "--json",
            json_path.to_str().unwrap(),
        ],
    );
    assert_eq!(output.status.code(), Some(1), "stderr: {}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("FAIL"));
    assert!(text.contains("build=not_attempted"), "{text}");
    assert!(text.contains("build_verified: false"), "{text}");
    let report = fs::read_to_string(&json_path).expect("report written");
    assert!(report.contains("\"verify_build\": true"));
    assert!(report.contains("\"build_verified\": false"));
    assert!(report.contains("\"build\": \"not_attempted\""));
    assert!(report.contains("\"failed\": 0"));
    assert!(report.contains("\"not_measured\": 1"));
}

#[test]
fn stand_in_toolchain_reports_pass_fail_and_skip_honestly() {
    let dir = scratch("stand-in");
    let cases = dir.join("cases");

    write_case(
        &cases,
        "cli",
        "version-self",
        r#"{
  "id": "cli/version-self",
  "mode": "cli",
  "args": ["--version"],
  "stdout_contains": "ravum-conformance"
}"#,
        &package_files(),
    );
    write_case(
        &cases,
        "lexer",
        "check-not-implemented",
        r#"{
  "id": "lexer/check-not-implemented",
  "mode": "compile-pass"
}"#,
        &package_files(),
    );
    write_case(
        &cases,
        "run",
        "skipped-everywhere",
        r#"{
  "id": "run/skipped-everywhere",
  "mode": "run-pass",
  "skip_on": ["windows", "linux", "macos"]
}"#,
        &package_files(),
    );

    let json_path = dir.join("report.json");
    let output = run_in(
        &dir,
        &[
            "run",
            "--cases",
            cases.to_str().unwrap(),
            "--toolchain",
            harness(),
            "--json",
            json_path.to_str().unwrap(),
        ],
    );
    let text = stdout(&output);
    // `--toolchain` points at the harness itself: `--version` passes, `check`
    // exits 2 because the harness has no `check` command. No success may be
    // invented for the compile-pass case.
    assert_eq!(
        output.status.code(),
        Some(1),
        "expected exit 1 (a FAIL), got {:?}\nstdout:\n{text}\nstderr:\n{}",
        output.status.code(),
        stderr(&output)
    );
    assert!(
        text.contains("summary: total=3 pass=1 fail=1 skip=1 not_run=0"),
        "unexpected summary:\n{text}"
    );
    assert_eq!(status_of(&text, "cli/version-self"), "PASS");
    assert_eq!(status_of(&text, "lexer/check-not-implemented"), "FAIL");
    assert_eq!(status_of(&text, "run/skipped-everywhere"), "SKIP");
    assert!(text.contains("exit code mismatch: expected 0, got 2"));

    let report = fs::read_to_string(&json_path).expect("report written");
    assert!(report.contains("\"pass\": 1"));
    assert!(report.contains("\"fail\": 1"));
    assert!(report.contains("\"skip\": 1"));
    let repo = repo_root().to_string_lossy().into_owned();
    let corpus = cases.to_string_lossy().into_owned();
    assert!(
        !report.contains(&repo),
        "report leaks the repo path:\n{report}"
    );
    assert!(
        !report.contains(&corpus),
        "report leaks the corpus path:\n{report}"
    );
}

#[test]
fn invalid_case_is_a_failure_even_without_a_toolchain() {
    let dir = scratch("invalid-case");
    let cases = dir.join("cases");
    write_case(
        &cases,
        "lexer",
        "bad-field",
        r#"{
  "id": "lexer/bad-field",
  "mode": "compile-pass",
  "stdout_contain": "typo"
}"#,
        &package_files(),
    );
    let json_path = dir.join("report.json");
    let output = run_in(
        &dir,
        &[
            "run",
            "--cases",
            cases.to_str().unwrap(),
            "--toolchain",
            "definitely-not-a-toolchain",
            "--json",
            json_path.to_str().unwrap(),
        ],
    );
    assert_eq!(output.status.code(), Some(1), "invalid cases must FAIL");
    let text = stdout(&output);
    assert!(text.contains("summary: total=1 pass=0 fail=1 skip=0 not_run=0"));
    assert!(text.contains("unknown field `stdout_contain`"));

    // `list` must expose the invalid case instead of silently dropping it.
    let listing = run_in(&dir, &["list", "--cases", cases.to_str().unwrap()]);
    assert_eq!(listing.status.code(), Some(0));
    let listing_text = stdout(&listing);
    assert!(listing_text.contains("invalid: 1"));
    assert!(listing_text.contains("INVALID CASES"));
}

#[test]
fn missing_cases_directory_is_a_configuration_error() {
    let dir = scratch("missing-cases");
    let output = run_in(
        &dir,
        &["run", "--cases", "does-not-exist", "--toolchain", harness()],
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("cases directory not found"));
}

#[test]
fn filter_that_matches_nothing_is_not_a_failure() {
    let dir = scratch("filter-none");
    let output = run_in(
        &dir,
        &[
            "run",
            "--cases",
            repo_root().join("conformance/cases").to_str().unwrap(),
            "--filter",
            "zzz-no-such-case-zzz",
            "--toolchain",
            harness(),
        ],
    );
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).contains("summary: total=0 pass=0 fail=0 skip=0 not_run=0"));
}
