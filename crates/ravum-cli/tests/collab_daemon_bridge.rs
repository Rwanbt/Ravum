//! COLLAB-4 / L3.4 integration test: a collaborative change accepted by the
//! coordinator triggers **only** the daemon analyses of the affected units
//! (+ transitive path-dependents), shares content identities, and serves a
//! replay of identical content from the daemon content cache.
//!
//! These tests drive the real `ravum` binary (real coordinator, real daemon,
//! real `rustc` metadata runs) on the `fixtures/collab4/workspace` fixture.
//!
//! Machine load: the reference machine runs other agents in parallel; every
//! test that spawns `rustc` must be executed with `cargo test -p ravum-cli --
//! --test-threads=1` (documented in `docs/mvp/MVP-0b-collab-4.md`).

use ravum_snapshot::SourceSnapshot;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture_workspace() -> PathBuf {
    repo_root().join("fixtures/collab4/workspace")
}

fn system_ravum() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ravum"))
}

/// Fresh directory below `target/ravum-tests` (never inside the fixture).
fn work_dir(name: &str) -> PathBuf {
    let dir = repo_root().join("target/ravum-tests").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("work dir");
    dir
}

fn run_collab_cycle(arguments: &[String]) -> std::process::Output {
    Command::new(system_ravum())
        .arg("collab-cycle")
        .args(arguments)
        .current_dir(repo_root())
        .output()
        .expect("spawn ravum collab-cycle")
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn read_report(path: &Path) -> Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read report {}: {error}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("report {} is not valid JSON: {error}", path.display()))
}

fn analysis_units(report: &Value, pass: u64) -> Vec<String> {
    report["analyses"]
        .as_array()
        .expect("analyses")
        .iter()
        .filter(|analysis| analysis["pass"].as_u64() == Some(pass))
        .map(|analysis| analysis["unit"].as_str().expect("unit").to_owned())
        .collect()
}

#[test]
fn accepted_changes_analyse_only_affected_units_and_replay_hits_the_cache() {
    let work = work_dir("collab4-it-cycle");
    let report_path = work.join("report.json");
    let daemon_work = work.join("daemon");
    let journal = work.join("journal.jsonl");
    let output = run_collab_cycle(&[
        fixture_workspace().display().to_string(),
        "--daemon-work".to_owned(),
        daemon_work.display().to_string(),
        "--journal".to_owned(),
        journal.display().to_string(),
        "--out".to_owned(),
        report_path.display().to_string(),
        "--passes".to_owned(),
        "2".to_owned(),
    ]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "collab-cycle must succeed; stderr: {}",
        stderr(&output)
    );

    let report = read_report(&report_path);
    assert_eq!(report["exit_code"].as_u64(), Some(0));
    assert!(
        report["violations"]
            .as_array()
            .expect("violations")
            .is_empty(),
        "no invariant violation expected: {:?}",
        report["violations"]
    );

    // The graph: crate_a (changed) <- crate_b (path dependent); crate_c is a
    // control unit with no edge and must never be analysed.
    let units = report["unit_graph"]["units"]
        .as_array()
        .expect("units")
        .iter()
        .map(|unit| unit["unit"].as_str().expect("unit").to_owned())
        .collect::<Vec<_>>();
    assert_eq!(units, vec!["crate_a", "crate_b", "crate_c"]);
    let crate_b = report["unit_graph"]["units"]
        .as_array()
        .expect("units")
        .iter()
        .find(|unit| unit["unit"].as_str() == Some("crate_b"))
        .expect("crate_b");
    assert_eq!(
        crate_b["dependencies"],
        serde_json::json!(["crate_a"]),
        "crate_b must depend on crate_a by path"
    );
    let skipped = report["skipped_units"]
        .as_array()
        .expect("skipped")
        .iter()
        .map(|unit| unit["unit"].as_str().expect("unit").to_owned())
        .collect::<Vec<_>>();
    assert_eq!(skipped, vec!["crate_c"], "unaffected unit must be skipped");

    // Two accepted proposals, four analyses (2 changes x 2 affected units),
    // replayed twice => eight analyses.
    let analyses = report["analyses"].as_array().expect("analyses");
    assert_eq!(analyses.len(), 8, "2 changes x 2 units x 2 passes");
    assert_eq!(
        analysis_units(&report, 1),
        vec!["crate_a", "crate_b", "crate_a", "crate_b"]
    );
    assert_eq!(
        analysis_units(&report, 2),
        vec!["crate_a", "crate_b", "crate_a", "crate_b"]
    );
    assert_eq!(report["decisions"].as_array().expect("decisions").len(), 4);
    assert_eq!(
        report["accepted_changes"]
            .as_array()
            .expect("accepted")
            .len(),
        4
    );

    for analysis in analyses {
        assert_eq!(
            analysis["daemon_status"].as_str(),
            Some("fresh"),
            "analysis must be current: {analysis}"
        );
        assert_eq!(analysis["daemon_error_count"].as_u64(), Some(0));
        assert_eq!(analysis["daemon_success"], serde_json::json!(true));
        assert_eq!(
            analysis["content_identity_verified"],
            serde_json::json!(true),
            "coordinator content hashes must match daemon file hashes: {analysis}"
        );
        assert!(
            !analysis["file_content_mapping"]
                .as_array()
                .expect("mapping")
                .is_empty(),
            "content mapping must be explicit"
        );
    }

    // Pass 1: real compiler work. crate_a is recompiled for each new content;
    // crate_b is content-identical between both changes => one memory hit.
    let pass1 = &report["passes"].as_array().expect("passes")[0];
    assert_eq!(pass1["rustc_runs"].as_u64(), Some(3), "{pass1}");
    assert_eq!(pass1["cache_misses"].as_u64(), Some(3), "{pass1}");
    assert_eq!(pass1["cache_hits"].as_u64(), Some(1), "{pass1}");

    // Pass 2 replays exactly the same content: every analysis is a cache hit
    // and no compiler runs — content, not path or clock, carries the identity.
    let pass2 = &report["passes"].as_array().expect("passes")[1];
    assert_eq!(pass2["rustc_runs"].as_u64(), Some(0), "{pass2}");
    assert_eq!(pass2["cache_misses"].as_u64(), Some(0), "{pass2}");
    assert_eq!(pass2["cache_hits"].as_u64(), Some(4), "{pass2}");
    for analysis in analyses {
        if analysis["pass"].as_u64() == Some(2) {
            assert_eq!(
                analysis["cache_hit"],
                serde_json::json!(true),
                "replay must hit the content cache: {analysis}"
            );
        }
    }

    // Coordinator identity chain: after(change 1) == before(change 2), and the
    // daemon snapshot id is stable for identical content across passes.
    let change1 = &report["accepted_changes"][0]["record"];
    let change2 = &report["accepted_changes"][1]["record"];
    assert_eq!(change1["snapshot_after"], change2["snapshot_before"]);
    // Same (proposal, unit) content replayed in pass 2 must have the exact same
    // daemon content identity.
    for pass1 in analyses
        .iter()
        .filter(|analysis| analysis["pass"].as_u64() == Some(1))
    {
        let replay = analyses
            .iter()
            .find(|candidate| {
                candidate["pass"].as_u64() == Some(2)
                    && candidate["change_proposal_id"] == pass1["change_proposal_id"]
                    && candidate["unit"] == pass1["unit"]
            })
            .unwrap_or_else(|| panic!("missing pass-2 replay of {pass1}"));
        assert_eq!(
            pass1["daemon_snapshot_id"], replay["daemon_snapshot_id"],
            "content identity must be stable across passes for {}",
            pass1["unit"]
        );
    }

    // The durable journal holds one line per decision per pass.
    for path in report["journal_paths"].as_array().expect("journals") {
        let path = path.as_str().expect("journal path");
        let lines = std::fs::read_to_string(path)
            .expect("journal")
            .lines()
            .count();
        assert_eq!(lines, 2, "one journal line per decision in {path}");
    }
    assert!(
        journal.exists(),
        "pass 1 journal must be at the requested path"
    );

    // Daemon-level totals: three real compiler runs for eight answers.
    let stats = &report["daemon"]["stats"];
    assert_eq!(stats["rustc_runs"].as_u64(), Some(3), "{stats}");
    assert_eq!(
        stats["cache_hits_memory"].as_u64().expect("memory hits"),
        5,
        "{stats}"
    );
}

#[test]
fn a_second_process_is_served_by_the_persistent_disk_cache() {
    let work = work_dir("collab4-it-disk");
    let daemon_work = work.join("daemon");
    let first_report = work.join("first.json");
    let base_arguments = vec![
        fixture_workspace().display().to_string(),
        "--daemon-work".to_owned(),
        daemon_work.display().to_string(),
        "--journal".to_owned(),
        work.join("journal.jsonl").display().to_string(),
    ];

    let mut first = base_arguments.clone();
    first.push("--out".to_owned());
    first.push(first_report.display().to_string());
    let output = run_collab_cycle(&first);
    assert_eq!(
        output.status.code(),
        Some(0),
        "first process must succeed: {}",
        stderr(&output)
    );
    let first = read_report(&first_report);
    assert_eq!(
        first["daemon"]["stats"]["rustc_runs"].as_u64(),
        Some(3),
        "first process pays the compiler cost once per content"
    );

    // Second process: a new daemon instance re-reads the persistent verdict
    // cache from `--daemon-work`. Same fixture, same proposed changes, same
    // content => disk hits, zero compiler runs.
    let second_report = work.join("second.json");
    let mut second = base_arguments;
    second.push("--out".to_owned());
    second.push(second_report.display().to_string());
    let output = run_collab_cycle(&second);
    assert_eq!(
        output.status.code(),
        Some(0),
        "second process must succeed: {}",
        stderr(&output)
    );
    let second = read_report(&second_report);
    assert_eq!(
        second["daemon"]["stats"]["rustc_runs"].as_u64(),
        Some(0),
        "no compiler run on a content-identical second process: {:?}",
        second["daemon"]["stats"]
    );
    let analyses = second["analyses"].as_array().expect("analyses");
    assert_eq!(analyses.len(), 4);
    for analysis in analyses {
        assert_eq!(
            analysis["cache_hit"],
            serde_json::json!(true),
            "every replay analysis must be a cache hit: {analysis}"
        );
    }
    let disk_hits = second["daemon"]["stats"]["cache_hits_disk"]
        .as_u64()
        .expect("disk hits");
    assert!(
        disk_hits >= 3,
        "at least one disk hit per distinct content, got {disk_hits}"
    );
}

#[test]
fn unreadable_graph_fails_closed_with_a_nonzero_exit_code() {
    let work = work_dir("collab4-it-bad-graph");
    let workspace = work.join("workspace");
    copy_directory(&fixture_workspace(), &workspace);
    // Break the dependency edge: the path dependency target does not exist.
    let manifest = workspace.join("crate_b/Cargo.toml");
    let text = std::fs::read_to_string(&manifest).expect("manifest");
    std::fs::write(
        &manifest,
        text.replace(
            "path = \"../crate_a\"",
            "path = \"../crate_does_not_exist\"",
        ),
    )
    .expect("broken manifest");

    let output = run_collab_cycle(&[
        workspace.display().to_string(),
        "--daemon-work".to_owned(),
        work.join("daemon").display().to_string(),
    ]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "an unreadable dependency graph must exit non-zero without analysing"
    );
    let message = stderr(&output);
    assert!(
        message.contains("introuvable") && message.contains("analyse refusée"),
        "the refusal must be explicit: {message}"
    );
    assert!(
        !work.join("daemon/snapshots").exists(),
        "no daemon snapshot may be created by a refused graph"
    );
}

#[test]
fn a_textually_accepted_change_that_fails_analysis_exits_nonzero_and_reports_it() {
    let work = work_dir("collab4-it-analysis-failure");
    let proposals = work.join("proposals");
    std::fs::create_dir_all(&proposals).expect("proposals dir");

    // Compute the real base identity/hashes of the fixture (single content
    // engine: ravum-snapshot), exactly like a collaborating agent would.
    let snapshot = SourceSnapshot::from_directory(&fixture_workspace(), |path| {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        name == "Cargo.toml" || path.extension().map(|ext| ext == "rs").unwrap_or(false)
    })
    .expect("fixture snapshot");
    let path = "crate_a/src/lib.rs";
    let proposal = serde_json::json!({
        "schema_version": 1,
        "agent_id": "agent-broken",
        "task_id": "introduces-syntax-error",
        "base_snapshot_id": snapshot.id().as_str(),
        "target_paths": [path],
        "target_symbols": ["double"],
        "expected_hashes": { path: snapshot.file_hash(path).expect("hash") },
        "declared_dependencies": [],
        "impacted_contracts": [],
        // Textually valid hunk (the coordinator applies it exactly), but the
        // resulting Rust does not compile: the daemon must catch it.
        "unified_patch": "+++ b/crate_a/src/lib.rs\n@@ -9,3 +9,3 @@\n pub fn double(x: u32) -> u32 {\n-    x * 2\n+    x *\n }\n",
        "note": "integration test: analysis failure must be non-zero"
    });
    std::fs::write(
        proposals.join("01-broken.json"),
        serde_json::to_string_pretty(&proposal).expect("json"),
    )
    .expect("write proposal");

    let report_path = work.join("report.json");
    let output = run_collab_cycle(&[
        fixture_workspace().display().to_string(),
        "--proposals".to_owned(),
        proposals.display().to_string(),
        "--daemon-work".to_owned(),
        work.join("daemon").display().to_string(),
        "--journal".to_owned(),
        work.join("journal.jsonl").display().to_string(),
        "--out".to_owned(),
        report_path.display().to_string(),
    ]);
    assert_eq!(
        output.status.code(),
        Some(3),
        "a failed affected analysis must exit 3; stderr: {}",
        stderr(&output)
    );
    let report = read_report(&report_path);
    assert_eq!(report["exit_code"].as_u64(), Some(3));
    assert_eq!(
        report["decisions"][0]["decision"].as_str(),
        Some("AUTO_SAFE")
    );
    let violations = report["violations"].as_array().expect("violations");
    assert!(
        violations
            .iter()
            .any(|violation| violation["kind"].as_str() == Some("analysis_failed")),
        "the report must name the failed analysis: {violations:?}"
    );
    let analysed = analysis_units(&report, 1);
    assert_eq!(
        analysed,
        vec!["crate_a", "crate_b"],
        "the dependent is still scheduled; the failure is attributable to crate_a"
    );
    let failed = &report["analyses"][0];
    assert_eq!(failed["unit"].as_str(), Some("crate_a"));
    assert!(
        failed["daemon_error_count"].as_u64().unwrap_or(0) > 0,
        "the daemon diagnostics must be recorded: {failed}"
    );
    assert_eq!(
        failed["content_identity_verified"],
        serde_json::json!(true),
        "identity mapping is still verified even when analysis fails"
    );
}

fn copy_directory(source: &Path, destination: &Path) {
    std::fs::create_dir_all(destination).expect("create destination");
    for entry in std::fs::read_dir(source).expect("read dir") {
        let entry = entry.expect("dir entry");
        let target = destination.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy_directory(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("copy file");
        }
    }
}
