//! Shared helpers for the COLLAB-2/3 integration tests.
//!
//! Every test builds a throw-away Git repository under `target/ravum-tests/`,
//! runs the real `ravum-collab` binary (so the actual CLI contract, including the
//! opt-in semantic verification, is exercised), then re-reads the repository with
//! Git plumbing. No test trusts the binary's prose: decisions come from the
//! `--json` report and repository state is re-read with `git`.

#![allow(dead_code)]

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const COLLAB_BIN: &str = env!("CARGO_BIN_EXE_ravum-collab");
pub const LAB_BIN: &str = env!("CARGO_BIN_EXE_collab2-lab");

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

pub fn fixtures() -> PathBuf {
    workspace_root().join("fixtures/collab2")
}

/// Read a fixture file (patch, source, manifest) as text.
pub fn fixture(relative: &str) -> String {
    let path = fixtures().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

pub fn tests_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ravum-tests")
}

pub fn fresh_dir(name: &str) -> PathBuf {
    let dir = tests_root().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create test directory");
    dir
}

pub fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed:\nstderr: {}\nstdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

pub fn write_file(dir: &Path, relative: &str, content: &str) {
    let path = dir.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent directory");
    }
    std::fs::write(&path, content).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
}

pub fn read_file(dir: &Path, relative: &str) -> String {
    let path = dir.join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Create a Git repository with the given files committed as the base snapshot.
pub fn init_repo(name: &str, files: &[(&str, String)]) -> PathBuf {
    let dir = fresh_dir(name);
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.name", "collab-test"]);
    git(&dir, &["config", "user.email", "collab-test@ravum.invalid"]);
    git(&dir, &["config", "core.autocrlf", "false"]);
    git(&dir, &["config", "commit.gpgsign", "false"]);
    for (relative, content) in files {
        write_file(&dir, relative, content);
    }
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "base"]);
    dir
}

/// `ravum-collab base --workspace <dir>` as parsed JSON.
pub fn run_base(workspace: &Path) -> Value {
    let output = Command::new(COLLAB_BIN)
        .arg("base")
        .arg("--workspace")
        .arg(workspace)
        .output()
        .expect("run ravum-collab base");
    assert!(
        output.status.success(),
        "ravum-collab base failed:\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("base output is not JSON: {e}\nstdout: {stdout}"))
}

/// Build a proposal against a base snapshot returned by [`run_base`].
pub fn proposal_json(
    base: &Value,
    agent: &str,
    task: &str,
    path: &str,
    symbols: &[&str],
    patch: &str,
) -> Value {
    let hash = base["files"][path]
        .as_str()
        .unwrap_or_else(|| panic!("base snapshot has no hash for {path}"))
        .to_owned();
    json!({
        "schema_version": 1,
        "agent_id": agent,
        "task_id": task,
        "base_snapshot_id": base["snapshot_id"],
        "target_paths": [path],
        "target_symbols": symbols,
        "expected_hashes": { path: hash },
        "declared_dependencies": [],
        "impacted_contracts": [],
        "unified_patch": patch,
        "note": Value::Null
    })
}

/// Write proposals as `NN-name.json` into a fresh directory outside the repo.
pub fn write_proposals(base_dir: &Path, proposals: &[(&str, Value)]) -> PathBuf {
    let dir = base_dir.with_extension("proposals");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create proposals directory");
    for (name, proposal) in proposals {
        let text = serde_json::to_string_pretty(proposal).expect("serialize proposal");
        std::fs::write(dir.join(name), text).expect("write proposal");
    }
    dir
}

pub struct Run {
    pub code: i32,
    pub report: Value,
    pub stdout: String,
    pub stderr: String,
}

impl Run {
    pub fn decisions(&self) -> &Vec<Value> {
        self.report["decisions"]
            .as_array()
            .unwrap_or_else(|| panic!("report has no decisions: {}", self.report))
    }

    pub fn decision_kinds(&self) -> Vec<String> {
        self.decisions()
            .iter()
            .map(|decision| {
                decision["decision"]
                    .as_str()
                    .unwrap_or("<missing>")
                    .to_owned()
            })
            .collect()
    }

    pub fn reasons(&self, index: usize) -> String {
        self.decisions()[index]["reasons"]
            .as_array()
            .map(|reasons| {
                reasons
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" | ")
            })
            .unwrap_or_default()
    }

    pub fn accepted(&self) -> &Vec<Value> {
        self.report["accepted_changes"]
            .as_array()
            .unwrap_or_else(|| panic!("report has no accepted_changes: {}", self.report))
    }

    pub fn summary(&self, key: &str) -> i64 {
        self.report["summary"][key].as_i64().unwrap_or(-1)
    }
}

/// Run `ravum-collab integrate --json` with the given extra flags.
pub fn run_integrate(workspace: &Path, proposals: &Path, extra: &[&str], scratch: &Path) -> Run {
    let _ = std::fs::remove_dir_all(scratch);
    std::fs::create_dir_all(scratch).expect("create scratch dir");
    let mut command = Command::new(COLLAB_BIN);
    command
        .arg("integrate")
        .arg("--workspace")
        .arg(workspace)
        .arg("--proposals")
        .arg(proposals)
        .arg("--journal")
        .arg(scratch.join("journal.jsonl"))
        .arg("--publication-journal")
        .arg(scratch.join("publications.jsonl"))
        .arg("--work")
        .arg(scratch.join("verify"))
        .arg("--json");
    command.args(extra);
    let output = command.output().expect("run ravum-collab integrate");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let report = serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!("integrate --json output is not JSON: {e}\nstdout: {stdout}\nstderr: {stderr}")
    });
    Run {
        code: output.status.code().unwrap_or(-1),
        report,
        stdout,
        stderr,
    }
}

/// Parse the durable decision journal as JSON lines.
pub fn journal_lines(scratch: &Path) -> Vec<Value> {
    let path = scratch.join("journal.jsonl");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("journal line is JSON"))
        .collect()
}

pub fn assert_no_unexpected_state_change(workspace: &Path, before: &str) {
    assert_eq!(
        git(workspace, &["rev-parse", "HEAD"]).trim(),
        before,
        "a refusal must not publish anything"
    );
    assert_eq!(
        git(workspace, &["status", "--porcelain"]).trim(),
        "",
        "a refusal must not dirty the working tree"
    );
}
