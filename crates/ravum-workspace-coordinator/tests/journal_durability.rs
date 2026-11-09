//! The journal must be durable per decision, not flushed at the end.

use ravum_protocol::{ChangeProposal, DecisionKind, JournalEntry, SnapshotId, COLLAB_SCHEMA_VERSION};
use ravum_snapshot::SourceSnapshot;
use ravum_workspace_coordinator::Coordinator;
use std::path::{Path, PathBuf};

const FILE: &str = "fn alpha() -> u32 {\n    1\n}\n\nfn beta() -> u32 {\n    2\n}\n";
const PATCH_ALPHA: &str =
    "+++ b/lib.rs\n@@ -1,3 +1,3 @@\n fn alpha() -> u32 {\n-    1\n+    10\n }\n";

fn dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/ravum-tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn base() -> SourceSnapshot {
    SourceSnapshot::new([("lib.rs".to_string(), FILE.to_string())])
}

fn proposal(coordinator: &Coordinator, agent: &str, patch: &str) -> ChangeProposal {
    ChangeProposal {
        schema_version: COLLAB_SCHEMA_VERSION,
        agent_id: agent.to_owned(),
        task_id: "t".to_owned(),
        base_snapshot_id: coordinator.current_id(),
        target_paths: vec!["lib.rs".to_owned()],
        target_symbols: vec!["alpha".to_owned()],
        expected_hashes: [(
            "lib.rs".to_owned(),
            coordinator.base().file_hash("lib.rs").unwrap().to_owned(),
        )]
        .into_iter()
        .collect(),
        declared_dependencies: vec![],
        impacted_contracts: vec![],
        unified_patch: patch.to_owned(),
        note: None,
    }
}

fn lines(path: &Path) -> Vec<JournalEntry> {
    std::fs::read_to_string(path)
        .expect("journal readable")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<JournalEntry>(l).expect("journal line parses"))
        .collect()
}

#[test]
fn every_decision_is_on_disk_before_the_next_one() {
    let dir = dir("journal-durable");
    let path = dir.join("journal.jsonl");
    let mut coordinator = Coordinator::new(base()).with_journal(&path).expect("journal");

    coordinator.submit(&proposal(&coordinator, "agent-a", PATCH_ALPHA));
    assert_eq!(lines(&path).len(), 1, "the first decision must already be durable");

    coordinator.submit(&proposal(&coordinator, "agent-b", "not a diff"));
    let written = lines(&path);
    assert_eq!(written.len(), 2);
    assert_eq!(written[0].seq, 1);
    assert_eq!(written[1].seq, 2);
    assert_eq!(written[1].decision.decision, DecisionKind::Reject);
    assert_eq!(
        written[1].snapshot_before, written[1].snapshot_after,
        "a refusal must not advance the snapshot"
    );

    // Simulate a crash: no flush call, just drop.
    drop(coordinator);
    assert_eq!(lines(&path).len(), 2, "history survives a crash");
}

#[test]
fn a_journal_that_cannot_be_written_is_reported_not_ignored() {
    let dir = dir("journal-broken");
    let path = dir.join("journal.jsonl");
    let mut coordinator = Coordinator::new(base()).with_journal(&path).expect("journal");
    // Replace the file with a directory: appending must now fail.
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();

    coordinator.submit(&proposal(&coordinator, "agent-a", PATCH_ALPHA));
    assert!(
        coordinator.journal_error().is_some(),
        "a persistence failure must be observable"
    );
}

#[test]
fn a_coordinator_without_a_journal_still_works_in_memory() {
    let mut coordinator = Coordinator::new(base());
    coordinator.submit(&proposal(&coordinator, "agent-a", PATCH_ALPHA));
    assert_eq!(coordinator.journal().len(), 1);
    assert!(coordinator.journal_error().is_none());
    assert_ne!(coordinator.current_id(), SnapshotId::new("anything"));
}
