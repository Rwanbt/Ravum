//! Real integration tests for `ravum collab-cycle --protocol v2 --oracle unit-graph`
//! (MVP-0c L5.1).
//!
//! The workspace is a real two-crate cargo workspace created in `target/`: `core`
//! exposes a public function, `api` consumes it. The proposal changes `core`'s
//! public signature coherently (core still compiles), so the injected L1 oracle
//! must report an `INTERFACE_BREAK` on the unchanged `api` and the coordinator must
//! never emit `AUTO_SAFE`.

use ravum_snapshot::SourceSnapshot;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const CORE_BEFORE: &str = "pub fn scale(x: f64) -> f64 {\n    x * 2.0\n}\n\npub fn scaled(x: f64) -> f64 {\n    scale(x)\n}\n";

/// Coherent core patch: `scale` changes type, `scaled` (in the same crate) is
/// updated, so the changed unit compiles — the unchanged `api` does not.
const CORE_INTERFACE_BREAK_PATCH: &str = "\
+++ b/core/src/lib.rs
@@ -1,3 +1,3 @@
-pub fn scale(x: f64) -> f64 {
-    x * 2.0
+pub fn scale(x: f32) -> f32 {
+    x * 2.0_f32
 }
@@ -5,3 +5,3 @@
 pub fn scaled(x: f64) -> f64 {
-    scale(x)
+    scale(x as f32) as f64
 }
";

fn tests_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/ravum-tests/collab-oracle-cli")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create test dir");
    dir
}

fn write(workspace: &Path, relative: &str, content: &str) {
    let path = workspace.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    std::fs::write(&path, content).unwrap_or_else(|error| panic!("write {relative}: {error}"));
}

/// Real cargo workspace: root package (workspace root) + `core` + `api`.
fn oracle_workspace(name: &str) -> PathBuf {
    let workspace = tests_dir(name);
    write(
        &workspace,
        "Cargo.toml",
        "[package]\nname = \"root_app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n\n[workspace]\nmembers = [\"core\", \"api\"]\n\n[dependencies]\ncore = { path = \"core\" }\n",
    );
    write(
        &workspace,
        "src/lib.rs",
        "pub fn root_value() -> u32 {\n    1\n}\n",
    );
    write(
        &workspace,
        "core/Cargo.toml",
        "[package]\nname = \"core\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n",
    );
    write(&workspace, "core/src/lib.rs", CORE_BEFORE);
    write(
        &workspace,
        "api/Cargo.toml",
        "[package]\nname = \"api\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n\n[dependencies]\ncore = { path = \"../core\" }\n",
    );
    write(
        &workspace,
        "api/src/lib.rs",
        "pub fn read(x: f64) -> f64 {\n    core::scale(x)\n}\n",
    );
    workspace
}

/// Snapshot exactly as `collab-cycle` loads it (`.rs` + `Cargo.toml`).
fn cycle_snapshot(workspace: &Path) -> SourceSnapshot {
    SourceSnapshot::from_directory(workspace, |path| {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        name == "Cargo.toml" || path.extension().map(|ext| ext == "rs").unwrap_or(false)
    })
    .expect("workspace snapshot")
}

fn write_interface_break_proposal(workspace: &Path, proposals: &Path) {
    let snapshot = cycle_snapshot(workspace);
    let hash = snapshot
        .file_hash("core/src/lib.rs")
        .expect("core hash")
        .to_owned();
    let proposal = json!({
        "schema_version": 1,
        "agent_id": "agent-a",
        "task_id": "break-scale",
        "base_snapshot_id": snapshot.id(),
        "target_paths": ["core/src/lib.rs"],
        "target_symbols": ["scale"],
        "expected_hashes": {"core/src/lib.rs": hash},
        "declared_dependencies": ["api"],
        "impacted_contracts": [],
        "unified_patch": CORE_INTERFACE_BREAK_PATCH,
        "note": "breaking public interface used by api"
    });
    std::fs::create_dir_all(proposals).expect("create proposals dir");
    std::fs::write(
        proposals.join("01-break-scale.json"),
        serde_json::to_string_pretty(&proposal).expect("serialize proposal"),
    )
    .expect("write proposal");
}

fn run_collab_cycle(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ravum"))
        .args(arguments)
        .output()
        .expect("run ravum collab-cycle")
}

fn parse_report(output: &Output) -> serde_json::Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout).unwrap_or_else(|error| {
        panic!(
            "collab-cycle --json output is not JSON: {error}\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn fingerprint(root: &Path) -> String {
    let mut text = String::new();
    let mut stack = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = stack.pop() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&directory)
            .expect("read dir")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                stack.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files.sort();
    for path in files {
        let relative = path
            .strip_prefix(root)
            .expect("inside root")
            .to_string_lossy()
            .replace('\\', "/");
        let bytes = std::fs::read(&path).expect("read file");
        text.push_str(&relative);
        text.push('\0');
        text.push_str(&ravum_unit_graph::sha256_hex(&bytes));
        text.push('\0');
    }
    ravum_unit_graph::sha256_hex(text.as_bytes())
}

#[test]
fn v2_with_unit_graph_oracle_is_never_auto_safe_on_an_interface_break() {
    let workspace = oracle_workspace("v2-oracle");
    let proposals = workspace.join("..").join("v2-oracle-proposals");
    let _ = std::fs::remove_dir_all(&proposals);
    write_interface_break_proposal(&workspace, &proposals);
    let state = tests_dir("v2-oracle-state");
    let before = fingerprint(&workspace);

    let output = run_collab_cycle(&[
        "collab-cycle",
        workspace.to_str().unwrap(),
        "--proposals",
        proposals.to_str().unwrap(),
        "--daemon-work",
        tests_dir("v2-oracle-daemon").to_str().unwrap(),
        "--journal",
        tests_dir("v2-oracle-journal")
            .join("journal.jsonl")
            .to_str()
            .unwrap(),
        "--oracle-state",
        state.to_str().unwrap(),
        "--protocol",
        "v2",
        "--oracle",
        "unit-graph",
        "--json",
    ]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "a refused proposal must make collab-cycle fail closed (exit 1): stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = parse_report(&output);
    assert_eq!(report["protocol"], "V2_SEMANTIC_STATUSES");
    assert_eq!(report["oracle"]["kind"], "unit-graph");
    let decisions = report["decisions"].as_array().expect("decisions");
    assert_eq!(decisions.len(), 1);
    let decision = &decisions[0];
    assert_ne!(
        decision["decision"], "AUTO_SAFE",
        "a patch breaking a used interface must never be AUTO_SAFE: {decision}"
    );
    assert!(
        decision["decision"] == "REBASE_REVALIDATE" || decision["decision"] == "MANUAL_REQUIRED",
        "unexpected decision: {decision}"
    );
    assert_eq!(decision["evaluation_level"], "TEXTUALLY_APPLICABLE");
    assert_eq!(decision["oracle_blocking"], true);
    let oracle_report = &decision["evaluation"]["oracle"];
    assert_eq!(
        oracle_report["oracle"], "unit-graph-l1",
        "the injected L1 oracle must be the one that gated the decision: {oracle_report}"
    );
    assert!(
        oracle_report["findings"]
            .as_array()
            .expect("findings")
            .iter()
            .any(
                |finding| finding["kind"] == "INTERFACE_BREAK" && finding["severity"] == "blocking"
            ),
        "a blocking INTERFACE_BREAK must be journaled: {oracle_report}"
    );
    let reasons = decision["reasons"]
        .as_array()
        .expect("reasons")
        .iter()
        .filter_map(|reason| reason.as_str())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        reasons.contains("INTERFACE_BREAK") && reasons.contains("api"),
        "the journaled reason must name the break and the affected unit: {reasons}"
    );
    assert!(
        report["accepted_changes"]
            .as_array()
            .expect("accepted")
            .is_empty(),
        "nothing may be integrated"
    );
    let violations = report["violations"].as_array().expect("violations");
    assert!(
        violations
            .iter()
            .any(|violation| violation["kind"] == "unexpected_decision"),
        "the report must record the non-AUTO_SAFE decision: {violations:?}"
    );
    assert_eq!(
        fingerprint(&workspace),
        before,
        "the workspace must be untouched"
    );
}

#[test]
fn v2_without_the_oracle_keeps_the_textual_refusal() {
    // Same breaking proposal, v2 protocol but no L1 oracle: the default textual
    // oracle still refuses the public surface change. This proves the oracle is a
    // separate opt-in and that v2 alone never accepts the patch.
    let workspace = oracle_workspace("v2-textual");
    let proposals = workspace.join("..").join("v2-textual-proposals");
    let _ = std::fs::remove_dir_all(&proposals);
    write_interface_break_proposal(&workspace, &proposals);
    let output = run_collab_cycle(&[
        "collab-cycle",
        workspace.to_str().unwrap(),
        "--proposals",
        proposals.to_str().unwrap(),
        "--daemon-work",
        tests_dir("v2-textual-daemon").to_str().unwrap(),
        "--journal",
        tests_dir("v2-textual-journal")
            .join("journal.jsonl")
            .to_str()
            .unwrap(),
        "--protocol",
        "v2",
        "--json",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let report = parse_report(&output);
    assert_eq!(report["protocol"], "V2_SEMANTIC_STATUSES");
    assert_eq!(report["oracle"]["kind"], "none");
    let decision = &report["decisions"].as_array().expect("decisions")[0];
    assert_ne!(decision["decision"], "AUTO_SAFE");
    assert_eq!(decision["evaluation_level"], "TEXTUALLY_APPLICABLE");
}

#[test]
fn collab_cycle_oracle_arguments_are_fail_closed() {
    let workspace = oracle_workspace("bad-arguments");
    let workspace_argument = workspace.to_str().unwrap();
    let cases: Vec<Vec<&str>> = vec![
        // oracle requested without the v2 protocol: the oracle would never be
        // consulted, so this is a usage error rather than a silent no-op.
        vec!["collab-cycle", workspace_argument, "--oracle", "unit-graph"],
        vec!["collab-cycle", workspace_argument, "--protocol", "v9"],
        vec![
            "collab-cycle",
            workspace_argument,
            "--protocol",
            "v2",
            "--oracle",
            "unknown-oracle",
        ],
        vec![
            "collab-cycle",
            workspace_argument,
            "--oracle-state",
            "somewhere",
        ],
    ];
    for arguments in cases {
        let output = run_collab_cycle(&arguments);
        assert_eq!(
            output.status.code(),
            Some(2),
            "arguments {arguments:?} must be a usage error: stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
