//! End-to-end: proposition → real L1 oracle → coordinator v2 decision.
//!
//! This is the integration the mission asks for: the production `Coordinator::v2`
//! consumes the real `UnitGraphOracle`, and a patch that breaks a used interface
//! is **never** `AUTO_SAFE`. The positive control shows that the same pipeline
//! accepts a compatible change when a declared verification (here a declared test
//! double for the compiler, since this crate owns no compiler) passed.

mod support;

use ravum_collab_oracle::UnitGraphOracle;
use ravum_protocol::{ChangeProposal, DecisionKind, COLLAB_SCHEMA_VERSION};
use ravum_snapshot::SourceSnapshot;
use ravum_workspace_coordinator::{
    Coordinator, EvaluationLevel, EvaluationProtocol, OracleFindingKind, OracleVerdict,
    SemanticOracle,
};
use std::collections::BTreeMap;
use std::sync::Arc;
use support::*;

fn proposal_for(
    before: &SourceSnapshot,
    path: &str,
    symbols: &[&str],
    declared_dependencies: &[&str],
    patch_text: &str,
) -> ChangeProposal {
    let hash = before
        .file_hash(path)
        .unwrap_or_else(|| panic!("snapshot has no hash for {path}"))
        .to_owned();
    ChangeProposal {
        schema_version: COLLAB_SCHEMA_VERSION,
        agent_id: "agent-a".to_owned(),
        task_id: "l3-oracle".to_owned(),
        base_snapshot_id: before.id().clone(),
        target_paths: vec![path.to_owned()],
        target_symbols: symbols.iter().map(|symbol| (*symbol).to_owned()).collect(),
        expected_hashes: BTreeMap::from([(path.to_owned(), hash)]),
        declared_dependencies: declared_dependencies
            .iter()
            .map(|dependency| (*dependency).to_owned())
            .collect(),
        impacted_contracts: Vec::new(),
        unified_patch: patch_text.to_owned(),
        note: None,
    }
}

#[test]
fn a_patch_that_breaks_a_used_interface_is_never_auto_safe() {
    let before = fixture_snapshot();
    let oracle = Arc::new(UnitGraphOracle::new(state_dir("e2e-interface-break")));
    let mut coordinator = Coordinator::v2(before.clone()).with_semantic_oracle(oracle.clone());
    assert_eq!(
        coordinator.protocol(),
        EvaluationProtocol::V2SemanticStatuses
    );

    let proposal = proposal_for(
        &before,
        CORE_PATH,
        &["scale"],
        &["api", "consumer", "application"],
        CORE_INTERFACE_BREAK,
    );
    let decision = coordinator.submit(&proposal);

    assert_ne!(
        decision.decision,
        DecisionKind::AutoSafe,
        "a patch breaking a used interface must never be AUTO_SAFE: {:?}",
        decision.reasons
    );
    assert_eq!(
        decision.decision,
        DecisionKind::RebaseRevalidate,
        "per the v2 rule, a pure interface break is at least revalidated: {:?}",
        decision.reasons
    );
    assert!(coordinator.accepted_changes().is_empty());
    assert_eq!(
        coordinator.current_id(),
        before.id().clone(),
        "nothing may be integrated"
    );

    let evaluation = coordinator.evaluations().last().expect("evaluation record");
    assert_eq!(evaluation.protocol, EvaluationProtocol::V2SemanticStatuses);
    assert_eq!(evaluation.level, EvaluationLevel::TextuallyApplicable);
    assert!(!evaluation.level.is_integration_grade());
    let oracle_report = evaluation.oracle.as_ref().expect("oracle report recorded");
    assert!(
        oracle_report.has_blocking(),
        "the oracle report must contain the blocking interface break: {oracle_report:?}"
    );
    assert!(
        oracle_report
            .findings
            .iter()
            .any(|finding| finding.kind == OracleFindingKind::InterfaceBreak),
        "an INTERFACE_BREAK finding must be journaled: {oracle_report:?}"
    );
    let reasons = decision.reasons.join(" | ");
    assert!(
        reasons.contains("INTERFACE_BREAK") && reasons.contains("api"),
        "the journaled reason must name the break and an affected unit: {reasons}"
    );

    // The adapter really ran the L1 campaign (two metadata calls, real rustc).
    let cost = oracle.last_cost().expect("oracle cost");
    assert_eq!(cost.cargo_metadata_invocations, 2);
    assert!(cost.rustc_invocations > 0);
    assert!(cost.affected_units.iter().any(|unit| unit == "api"));
}

#[test]
fn a_compatible_change_is_auto_safe_only_with_a_declared_verification() {
    let before = fixture_snapshot();
    let oracle = Arc::new(UnitGraphOracle::new(state_dir("e2e-compatible")));

    // Without any declared verification, textual applicability alone stays
    // MANUAL_REQUIRED even though the L1 oracle says Clear.
    let mut manual = Coordinator::v2(before.clone()).with_semantic_oracle(oracle.clone());
    let proposal = proposal_for(
        &before,
        CORE_PATH,
        &["internal_factor"],
        &["api", "consumer", "application"],
        CORE_BODY_ONLY,
    );
    let decision = manual.submit(&proposal);
    assert_eq!(decision.decision, DecisionKind::ManualRequired);
    let evaluation = manual.evaluations().last().expect("evaluation");
    assert_eq!(evaluation.level, EvaluationLevel::TextuallyApplicable);
    let oracle_report = evaluation.oracle.as_ref().expect("oracle report");
    assert!(
        !oracle_report.has_blocking(),
        "the compatible change must not produce blocking findings: {oracle_report:?}"
    );

    // With a declared verifier that passes the required checks, the same change is
    // accepted at SEMANTICALLY_CHECKED (never higher: no test run, no proof).
    let mut verified = Coordinator::v2(before.clone())
        .with_merge_verifier(Arc::new(PassVerifier))
        .with_semantic_oracle(oracle);
    let decision = verified.submit(&proposal);
    assert_eq!(
        decision.decision,
        DecisionKind::AutoSafe,
        "reasons: {:?}",
        decision.reasons
    );
    let evaluation = verified.evaluations().last().expect("evaluation");
    assert_eq!(evaluation.level, EvaluationLevel::SemanticallyChecked);
    assert_eq!(
        evaluation.verification.as_ref().map(|record| record.status),
        Some(ravum_workspace_coordinator::MergeCheckStatus::Passed)
    );
    assert_eq!(verified.accepted_changes().len(), 1);
}

#[test]
fn the_oracle_is_used_only_through_the_injected_trait() {
    // Compile-time sanity: the adapter is usable as a boxed injected oracle and
    // produces a verdict on the real fixture — the coordinator cannot depend on the
    // analyser, only on this trait object.
    let oracle: Arc<dyn SemanticOracle> =
        Arc::new(UnitGraphOracle::new(state_dir("e2e-trait-object")));
    let verdict = oracle.evaluate(&body_only_request(CORE_DEPENDENTS));
    assert!(
        !matches!(verdict, OracleVerdict::Unavailable { .. }),
        "the injected oracle must analyse the fixture: {verdict:?}"
    );
}
