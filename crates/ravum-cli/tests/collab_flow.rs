//! COLLAB tests that actually couple coordination to the M1 loop:
//! two agents integrate into one file, then the merged source is compiled,
//! verified and hot-activated in a live host.

use ravum_coordinator::Pipeline;
use ravum_patch_host::{ExpectedAbi, PatchHost};
use ravum_protocol::{ChangeProposal, DecisionKind, SnapshotId, COLLAB_SCHEMA_VERSION};
use ravum_snapshot::SourceSnapshot;
use ravum_workspace_coordinator::Coordinator;
use std::path::{Path, PathBuf};

const SOURCE: &str = include_str!("../../../fixtures/collab/two_functions.rs");
const PATH: &str = "two_functions.rs";

/// Agent A: change `helper` only.
const PATCH_A: &str = "+++ b/two_functions.rs\n@@ -24,4 +24,4 @@\n /// Calcule un decalage simple.\n fn helper(x: f64) -> f64 {\n-    x + 1.0\n+    x + 10.0\n }\n";

/// Agent B: change `ravum_compute` only.
const PATCH_B: &str = "+++ b/two_functions.rs\n@@ -29,5 +29,5 @@\n /// Point d'entree patchable.\n #[no_mangle]\n pub extern \"C\" fn ravum_compute(x: f64) -> f64 {\n-    helper(x) * 2.0\n+    helper(x) * 3.0\n }\n";

/// Agent C: change the same line as A.
const PATCH_C: &str = "+++ b/two_functions.rs\n@@ -24,4 +24,4 @@\n /// Calcule un decalage simple.\n fn helper(x: f64) -> f64 {\n-    x + 1.0\n+    x + 99.0\n }\n";

fn base_coordinator() -> Coordinator {
    Coordinator::new(SourceSnapshot::new([(PATH.to_string(), SOURCE.to_string())]))
}

fn proposal(
    base: &Coordinator,
    agent: &str,
    symbols: &[&str],
    patch: &str,
) -> ChangeProposal {
    let hash = base.base().file_hash(PATH).unwrap().to_string();
    ChangeProposal {
        schema_version: COLLAB_SCHEMA_VERSION,
        agent_id: agent.to_owned(),
        task_id: "task".to_owned(),
        base_snapshot_id: base.current_id(),
        target_paths: vec![PATH.to_owned()],
        target_symbols: symbols.iter().map(|s| s.to_string()).collect(),
        expected_hashes: [(PATH.to_string(), hash)].into_iter().collect(),
        declared_dependencies: vec![],
        impacted_contracts: vec![],
        unified_patch: patch.to_owned(),
        note: None,
    }
}

fn work(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/ravum-tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create work dir");
    dir
}

#[test]
fn two_agents_integrate_into_one_file_and_the_result_hot_activates() {
    let mut coordinator = base_coordinator();

    let a = proposal(&coordinator, "agent-a", &["helper"], PATCH_A);
    let decision_a = coordinator.submit(&a);
    assert_eq!(decision_a.decision, DecisionKind::AutoSafe, "{:?}", decision_a.reasons);

    let b = proposal(&coordinator, "agent-b", &["ravum_compute"], PATCH_B);
    let decision_b = coordinator.submit(&b);
    assert_eq!(decision_b.decision, DecisionKind::AutoSafe, "{:?}", decision_b.reasons);

    let merged = coordinator.current().content_of(PATH).unwrap().to_owned();
    assert!(merged.contains("x + 10.0"), "agent A intent must be preserved");
    assert!(merged.contains("helper(x) * 3.0"), "agent B intent must be present");

    // COLLAB-4 (partial): the merged snapshot is compiled and activated for real.
    let pipeline = Pipeline::new("rustc", work("collab-merged"));
    let snapshot = SourceSnapshot::new([(PATH.to_string(), merged)]);
    let built = pipeline.build(&snapshot).expect("adapter");
    assert!(built.success, "merged source must compile: {:?}", built.build.diagnostics);

    let host = PatchHost::bootstrap(ExpectedAbi::default(), Path::new(&built.build.artifact_path))
        .expect("bootstrap merged artifact");
    // (2 + 10) * 3 = 36 with both agents' changes; 6 or 26 if one was lost.
    assert_eq!(host.call(2.0), 36.0);
}

#[test]
fn conflicting_intent_is_refused_and_the_other_agent_work_survives() {
    let mut coordinator = base_coordinator();

    let a = proposal(&coordinator, "agent-a", &["helper"], PATCH_A);
    assert_eq!(coordinator.submit(&a).decision, DecisionKind::AutoSafe);
    let snapshot_after_a = coordinator.current_id();

    let c = proposal(&coordinator, "agent-c", &["helper"], PATCH_C);
    let decision_c = coordinator.submit(&c);
    assert_eq!(decision_c.decision, DecisionKind::Conflict);
    assert!(!decision_c.reasons.is_empty());

    assert_eq!(
        coordinator.current_id(),
        snapshot_after_a,
        "a refused proposal must not perturb the integrated result"
    );
    let content = coordinator.current().content_of(PATH).unwrap();
    assert!(content.contains("x + 10.0"));
    assert!(!content.contains("x + 99.0"));
}

#[test]
fn a_stale_agent_view_is_rejected_not_merged() {
    let mut coordinator = base_coordinator();
    let mut stale = proposal(&coordinator, "agent-a", &["helper"], PATCH_A);
    stale.base_snapshot_id = SnapshotId::new("0000-an-old-base");
    stale.expected_hashes.clear();
    let decision = coordinator.submit(&stale);
    assert_eq!(decision.decision, DecisionKind::RebaseRevalidate);
    assert_eq!(coordinator.journal().len(), 1);
    assert_eq!(coordinator.integrated_proposals().count(), 0);
}
