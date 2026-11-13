//! Contract for the additive `--timings` JSON field (phase 3.1).
//!
//! The JSON envelope is a public contract (`schema_version: 1`): adding
//! `timings` must be additive, must not change `schema_version`, must be absent
//! when the flag is not passed, and must expose every phase as a finite,
//! non-negative number of milliseconds. `total_ms` must be the sum of the
//! analysis phases (`check` has no codegen/rustc phases).

use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ravum-lang"))
}

fn example(name: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.parent()
        .and_then(|crates| crates.parent())
        .expect("crates/ravum-lang-cli must have a repository root")
        .join("examples")
        .join(name)
}

fn run(args: &[&str]) -> (String, String, i32) {
    let output = Command::new(binary())
        .args(args)
        .output()
        .expect("cannot run ravum-lang");
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    )
}

fn timings_object(text: &str) -> Value {
    let envelope: Value =
        serde_json::from_str(text.trim()).expect("check --json must emit one JSON object");
    envelope
        .get("timings")
        .cloned()
        .expect("--timings must add a top-level `timings` object")
}

#[test]
fn check_timings_are_additive_and_complete() {
    let hello = example("hello");
    let path = hello.to_str().expect("example path is UTF-8");

    let (text, stderr, code) = run(&["check", path, "--json", "--timings"]);
    assert_eq!(code, 0, "check failed: {stderr}");
    let envelope: Value = serde_json::from_str(text.trim()).expect("valid JSON");
    assert_eq!(
        envelope["schema_version"], 1,
        "`timings` is additive: schema_version must stay 1"
    );
    assert_eq!(envelope["command"], "check");

    let timings = timings_object(&text);
    let keys = [
        "load_ms",
        "parse_ms",
        "resolve_ms",
        "typeck_ms",
        "effects_ms",
        "own_ms",
        "total_ms",
    ];
    let mut sum = 0.0;
    for key in keys {
        let value = timings[key]
            .as_f64()
            .unwrap_or_else(|| panic!("`timings.{key}` must be a number, got {:?}", timings[key]));
        assert!(
            value.is_finite() && value >= 0.0,
            "`{key}` must be >= 0, got {value}"
        );
        if key != "total_ms" {
            sum += value;
        }
    }
    let total = timings["total_ms"].as_f64().expect("total_ms is a number");
    assert!(
        (total - sum).abs() < 0.05,
        "total_ms ({total}) must equal the sum of the phases ({sum})"
    );
}

#[test]
fn check_without_timings_has_no_timings_field() {
    let hello = example("hello");
    let path = hello.to_str().expect("example path is UTF-8");
    let (text, stderr, code) = run(&["check", path, "--json"]);
    assert_eq!(code, 0, "check failed: {stderr}");
    let envelope: Value = serde_json::from_str(text.trim()).expect("valid JSON");
    assert!(
        envelope.get("timings").is_none(),
        "without --timings the field must be absent: {envelope}"
    );
}

#[test]
fn build_timings_cover_codegen_and_rustc() {
    let hello = example("hello");
    let path = hello.to_str().expect("example path is UTF-8");
    let (text, stderr, code) = run(&["build", path, "--timings"]);
    assert_eq!(code, 0, "build failed: {stderr}\n{text}");
    let line = text
        .lines()
        .find(|line| line.starts_with("timings:"))
        .expect("build --timings must print a `timings:` line");
    for phase in [
        "load", "parse", "resolve", "typeck", "effects", "own", "codegen", "rustc", "total",
    ] {
        assert!(
            line.contains(&format!("{phase}=")),
            "the build timings line must contain `{phase}=`: {line}"
        );
    }
    assert!(
        line.contains("rustc="),
        "the build timing must measure the rustc invocation: {line}"
    );
}
