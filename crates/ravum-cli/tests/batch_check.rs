//! Real integration tests for `ravum batch-check` (MVP-0c L2.4 → produit).
//!
//! The tests run the actual `ravum` binary against a temporary Cargo crate and
//! exercise the real collector + real `cargo test --offline` analyzer:
//! compatible proposals are grouped into one analysis, an incompatible proposal
//! is rejected through the conservative fallback, file-granularity pre-flight
//! isolates same-file edits, the user workspace is never modified, and invalid
//! input fails closed.
//!
//! Cargo/rustc-heavy tests are meant to run with `--test-threads=1`.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn tests_dir(name: &str) -> PathBuf {
    let dir = repo_root()
        .join("target/ravum-tests/batch-check")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create test dir");
    dir
}

const LIB_SOURCE: &str = "pub fn alpha() -> i32 {\n    0\n}\n\npub fn beta() -> i32 {\n    0\n}\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n\n    #[test]\n    fn alpha_is_non_negative() {\n        assert!(alpha() >= 0);\n    }\n\n    #[test]\n    fn beta_is_non_negative() {\n        assert!(beta() >= 0);\n    }\n}\n";

fn make_workspace(root: &Path) {
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"batchfix\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
    )
    .expect("write manifest");
    std::fs::write(root.join("src/lib.rs"), LIB_SOURCE).expect("write lib");
}

fn edit_alpha() -> Value {
    serde_json::json!({
        "path": "src/lib.rs",
        "old": "pub fn alpha() -> i32 {\n    0\n}",
        "new": "pub fn alpha() -> i32 {\n    1\n}"
    })
}

fn edit_beta() -> Value {
    serde_json::json!({
        "path": "src/lib.rs",
        "old": "pub fn beta() -> i32 {\n    0\n}",
        "new": "pub fn beta() -> i32 {\n    2\n}"
    })
}

fn edit_missing() -> Value {
    serde_json::json!({
        "path": "src/lib.rs",
        "old": "pub fn gamma() -> i32 {\n    0\n}",
        "new": "pub fn gamma() -> i32 {\n    3\n}"
    })
}

fn write_proposals(root: &Path, proposals: Value) -> PathBuf {
    let file = root.join("proposals.json");
    let payload = serde_json::json!({
        "base_id": "test-base",
        "proposals": proposals
    });
    std::fs::write(
        &file,
        serde_json::to_string_pretty(&payload).expect("serialize"),
    )
    .expect("write proposals");
    file
}

fn run_ravum(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ravum"))
        .args(args)
        .output()
        .expect("run ravum")
}

fn parse_json(output: &Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout).unwrap_or_else(|error| {
        panic!(
            "batch-check --json output is not JSON: {error}\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[test]
fn compatible_proposals_are_grouped_and_workspace_is_preserved() {
    let root = tests_dir("compatible");
    let workspace = root.join("workspace");
    make_workspace(&workspace);
    let proposals = serde_json::json!([
        {"id": "agent-1/t1", "symbols": ["alpha"], "edits": [edit_alpha()]},
        {"id": "agent-2/t2", "symbols": ["beta"], "edits": [edit_beta()]}
    ]);
    let proposals_file = write_proposals(&root, proposals);
    let state = root.join("state");

    let output = run_ravum(&[
        "batch-check",
        workspace.to_str().expect("workspace path"),
        "--proposals",
        proposals_file.to_str().expect("proposals path"),
        "--state",
        state.to_str().expect("state path"),
        "--json",
    ]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = parse_json(&output);
    assert_eq!(report["summary"]["proposed"], 2);
    assert_eq!(report["summary"]["accepted"], 2);
    assert_eq!(report["summary"]["rejected"], 0);
    assert_eq!(report["summary"]["batches"], 1);
    assert_eq!(report["summary"]["analyses_used"], 1);
    assert_eq!(report["summary"]["analyses_saved_net"], 1);
    assert_eq!(report["telemetry"]["batches_accepted_all"], 1);
    assert_eq!(report["validation"]["no_loss"], true);
    assert_eq!(report["validation"]["workspace_unchanged"], true);

    // The user workspace is untouched: same source, no Cargo build output.
    assert_eq!(
        std::fs::read_to_string(workspace.join("src/lib.rs")).expect("source"),
        LIB_SOURCE
    );
    assert!(!workspace.join("target").exists());
}

#[test]
fn incompatible_proposal_falls_back_and_is_rejected() {
    let root = tests_dir("fallback");
    let workspace = root.join("workspace");
    make_workspace(&workspace);
    let proposals = serde_json::json!([
        {"id": "agent-1/t1", "symbols": ["alpha"], "edits": [edit_alpha()]},
        {"id": "agent-2/t2", "symbols": ["beta"], "edits": [edit_beta()]},
        {"id": "agent-3/t3", "symbols": ["gamma"], "edits": [edit_missing()]}
    ]);
    let proposals_file = write_proposals(&root, proposals);
    let state = root.join("state");

    let output = run_ravum(&[
        "batch-check",
        workspace.to_str().expect("workspace path"),
        "--proposals",
        proposals_file.to_str().expect("proposals path"),
        "--state",
        state.to_str().expect("state path"),
        "--json",
    ]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "expected rejection exit code"
    );
    let report = parse_json(&output);
    assert_eq!(report["summary"]["accepted"], 2);
    assert_eq!(report["summary"]["rejected"], 1);
    assert_eq!(
        report["summary"]["rejected_ids"],
        serde_json::json!(["agent-3/t3"])
    );
    assert_eq!(report["summary"]["batches"], 1);
    assert_eq!(report["summary"]["fell_back"], 1);
    // 1 refused batch + 3 individual fallback analyses.
    assert_eq!(report["summary"]["analyses_used"], 4);
    assert_eq!(report["telemetry"]["individual_analyses"], 3);
    assert_eq!(report["validation"]["no_loss"], true);
    assert_eq!(report["validation"]["workspace_unchanged"], true);
}

#[test]
fn file_granularity_isolates_same_file_edits() {
    let root = tests_dir("files");
    let workspace = root.join("workspace");
    make_workspace(&workspace);
    let proposals = serde_json::json!([
        {"id": "agent-1/t1", "files": ["src/lib.rs"], "symbols": ["alpha"], "edits": [edit_alpha()]},
        {"id": "agent-2/t2", "files": ["src/lib.rs"], "symbols": ["beta"], "edits": [edit_beta()]}
    ]);
    let proposals_file = write_proposals(&root, proposals);
    let state = root.join("state");

    let output = run_ravum(&[
        "batch-check",
        workspace.to_str().expect("workspace path"),
        "--proposals",
        proposals_file.to_str().expect("proposals path"),
        "--state",
        state.to_str().expect("state path"),
        "--granularity",
        "files",
        "--json",
    ]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = parse_json(&output);
    assert_eq!(report["summary"]["accepted"], 2);
    assert_eq!(report["summary"]["rejected"], 0);
    assert_eq!(report["summary"]["batches"], 2);
    assert_eq!(report["summary"]["analyses_used"], 2);
    let isolations = report["telemetry"]["preflight_isolations"]
        .as_u64()
        .unwrap_or(0);
    assert!(
        isolations >= 1,
        "same-file edits must be isolated by the file-granularity pre-flight, got {isolations}"
    );
    assert_eq!(report["config"]["granularity"], "files");
}

#[test]
fn invalid_proposals_file_fails_closed() {
    let root = tests_dir("invalid");
    let workspace = root.join("workspace");
    make_workspace(&workspace);
    let proposals_file = root.join("bad.json");
    std::fs::write(&proposals_file, "{ not json").expect("write invalid json");
    let state = root.join("state");

    let output = run_ravum(&[
        "batch-check",
        workspace.to_str().expect("workspace path"),
        "--proposals",
        proposals_file.to_str().expect("proposals path"),
        "--state",
        state.to_str().expect("state path"),
    ]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not a valid proposals file"),
        "unexpected stderr: {stderr}"
    );
}
