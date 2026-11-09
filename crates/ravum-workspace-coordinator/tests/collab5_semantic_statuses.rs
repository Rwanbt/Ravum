//! MVP-0c L3 — versioned semantic statuses and the injectable oracle.
//!
//! This suite drives the strict v2 coordinator over the negative corpus
//! `fixtures/collab5` (the ten cases of the mandate) and proves the central
//! invariant on the real code path:
//!
//! **a textually applicable patch is never `AUTO_SAFE` without a declared
//! semantic verification.**
//!
//! The compile-only lanes use a scripted verifier (the mission explicitly asks
//! for a fake oracle in tests; the real L1/rustc branch is wired later by the
//! Lead and is already exercised end-to-end by `collab3_semantic.rs`). The
//! textual lanes use the real default oracle and the real `evaluate` path, and
//! the lockfile/stale/conflict cases are decided by real coordinator rules.

use ravum_protocol::{
    ChangeProposal, DecisionKind, JournalEntry, ProposalDecision, COLLAB_SCHEMA_VERSION,
};
use ravum_snapshot::SourceSnapshot;
use ravum_workspace_coordinator::{
    read_journal_records, Coordinator, EvaluationLevel, EvaluationProtocol, MergeCheck,
    MergeCheckKind, MergeCheckStatus, MergeDiagnostic, MergeVerifier, OracleFinding,
    OracleFindingKind, OracleRequest, OracleSeverity, OracleVerdict, SemanticOracle,
    VerificationOutcome, VerificationRequest, COORDINATOR_SCHEMA_VERSION,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Corpus model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
struct Scenario {
    id: String,
    description: String,
    #[serde(default)]
    lane: String,
    base_files: Vec<String>,
    proposals: Vec<ProposalSpec>,
    expected_decisions: Vec<DecisionKind>,
    expected_levels: Vec<EvaluationLevel>,
    #[serde(default)]
    oracle_findings: Vec<FindingSpec>,
    #[serde(default)]
    oracle_mode: Option<String>,
    #[serde(default)]
    verifier: Option<VerifierSpec>,
    #[serde(default)]
    expect_reason_contains: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
struct ProposalSpec {
    agent_id: String,
    task_id: String,
    target: String,
    #[serde(default)]
    symbols: Vec<String>,
    patch: String,
    #[serde(default)]
    base_snapshot_id: Option<String>,
    #[serde(default = "default_true")]
    expected_hashes: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
struct FindingSpec {
    kind: OracleFindingKind,
    severity: OracleSeverity,
    message: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
enum VerifierSpec {
    PassAll,
    PassMetadataOnly,
    Fail {
        kind: MergeCheckKind,
        #[serde(default)]
        code: String,
        #[serde(default)]
        message: String,
    },
    Unavailable {
        reason: String,
    },
}

// ---------------------------------------------------------------------------
// Scripted components (the mission asks for a fake oracle in tests)
// ---------------------------------------------------------------------------

struct ScriptedOracle {
    findings: Vec<OracleFinding>,
    unavailable: Option<String>,
}

impl SemanticOracle for ScriptedOracle {
    fn name(&self) -> &'static str {
        "scripted-l1-oracle"
    }

    fn evaluate(&self, _request: &OracleRequest) -> OracleVerdict {
        if let Some(reason) = &self.unavailable {
            return OracleVerdict::Unavailable {
                reason: reason.clone(),
            };
        }
        if self.findings.is_empty() {
            OracleVerdict::Clear {
                bounded_proof: None,
            }
        } else {
            OracleVerdict::Findings(self.findings.clone())
        }
    }
}

struct ScriptedVerifier {
    spec: VerifierSpec,
}

impl MergeVerifier for ScriptedVerifier {
    fn name(&self) -> &'static str {
        "scripted-verifier"
    }

    fn verify(&self, request: &VerificationRequest) -> VerificationOutcome {
        match &self.spec {
            VerifierSpec::PassAll => VerificationOutcome::Passed {
                checks: request
                    .required_checks
                    .iter()
                    .map(|kind| {
                        MergeCheck::passed(
                            *kind,
                            "rustc",
                            "1.95.0",
                            "host",
                            request.snapshot_after.id().clone(),
                            format!("simulated {} passed", kind.as_str()),
                        )
                    })
                    .collect(),
            },
            VerifierSpec::PassMetadataOnly => VerificationOutcome::Passed {
                checks: vec![MergeCheck::passed(
                    MergeCheckKind::MetadataCheck,
                    "rustc",
                    "1.95.0",
                    "host",
                    request.snapshot_after.id().clone(),
                    "simulated metadata check passed",
                )],
            },
            VerifierSpec::Fail {
                kind,
                code,
                message,
            } => {
                let diagnostics = if code.is_empty() {
                    Vec::new()
                } else {
                    vec![MergeDiagnostic {
                        code: Some(code.clone()),
                        level: "error".to_owned(),
                        message: message.clone(),
                        line: None,
                    }]
                };
                let check = MergeCheck::failed(
                    *kind,
                    "rustc",
                    "1.95.0",
                    "host",
                    request.snapshot_after.id().clone(),
                    format!("simulated {} failure", kind.as_str()),
                    diagnostics,
                );
                let reason = if *kind == MergeCheckKind::TestRun {
                    format!("the test harness of the merged snapshot failed: {message}")
                } else {
                    format!("merged snapshot does not compile: {message}")
                };
                VerificationOutcome::Failed { check, reason }
            }
            VerifierSpec::Unavailable { reason } => VerificationOutcome::Unavailable {
                reason: reason.clone(),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/collab5/cases")
}

fn case_dir(id: &str) -> PathBuf {
    corpus_dir().join(id)
}

fn read_case_file(id: &str, relative: &str) -> String {
    let path = case_dir(id).join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

fn load_scenarios() -> Vec<Scenario> {
    let mut scenarios = Vec::new();
    let entries = std::fs::read_dir(corpus_dir()).expect("fixtures/collab5/cases exists");
    for entry in entries.filter_map(Result::ok) {
        if !entry.path().is_dir() {
            continue;
        }
        let manifest = entry.path().join("scenario.json");
        if !manifest.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&manifest)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", manifest.display()));
        let scenario: Scenario = serde_json::from_str(&text)
            .unwrap_or_else(|error| panic!("invalid {}: {error}", manifest.display()));
        scenarios.push(scenario);
    }
    scenarios.sort_by(|left, right| left.id.cmp(&right.id));
    scenarios
}

fn scratch_dir(id: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/ravum-tests/collab5")
        .join(id);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn base_snapshot(scenario: &Scenario) -> SourceSnapshot {
    SourceSnapshot::new(
        scenario
            .base_files
            .iter()
            .map(|relative| (relative.clone(), read_case_file(&scenario.id, relative))),
    )
}

fn build_coordinator(scenario: &Scenario, journal: &Path) -> Coordinator {
    let mut coordinator = Coordinator::v2(base_snapshot(scenario));
    if let Some(spec) = &scenario.verifier {
        coordinator =
            coordinator.with_merge_verifier(Arc::new(ScriptedVerifier { spec: spec.clone() }));
    }
    if !scenario.oracle_findings.is_empty() || scenario.oracle_mode.is_some() {
        let findings = scenario
            .oracle_findings
            .iter()
            .map(|finding| {
                OracleFinding::new(finding.kind, finding.severity, finding.message.clone())
            })
            .collect();
        coordinator = coordinator.with_semantic_oracle(Arc::new(ScriptedOracle {
            findings,
            unavailable: scenario
                .oracle_mode
                .as_ref()
                .filter(|mode| mode.as_str() == "unavailable")
                .map(|_| "L1 analysis is not available in this environment".to_owned()),
        }));
    }
    coordinator
        .with_journal(journal)
        .expect("decision journal is writable")
}

fn proposal_for(
    scenario: &Scenario,
    coordinator: &Coordinator,
    spec: &ProposalSpec,
    base: &SourceSnapshot,
) -> ChangeProposal {
    let patch = read_case_file(&scenario.id, &spec.patch);
    let expected_hashes = if spec.expected_hashes {
        let hash = base
            .file_hash(&spec.target)
            .unwrap_or_else(|| panic!("{} has no hash for {}", scenario.id, spec.target))
            .to_owned();
        BTreeMap::from([(spec.target.clone(), hash)])
    } else {
        BTreeMap::new()
    };
    ChangeProposal {
        schema_version: COLLAB_SCHEMA_VERSION,
        agent_id: spec.agent_id.clone(),
        task_id: spec.task_id.clone(),
        base_snapshot_id: spec
            .base_snapshot_id
            .clone()
            .map(ravum_protocol::SnapshotId::new)
            .unwrap_or_else(|| coordinator.current_id()),
        target_paths: vec![spec.target.clone()],
        target_symbols: spec.symbols.clone(),
        expected_hashes,
        declared_dependencies: Vec::new(),
        impacted_contracts: Vec::new(),
        unified_patch: patch,
        note: None,
    }
}

fn joined_reasons(coordinator: &Coordinator, proposal_id: &str) -> String {
    coordinator
        .evaluation_for(proposal_id)
        .map(|evaluation| evaluation.reasons.join(" | "))
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn the_corpus_contains_the_ten_mandate_cases() {
    let scenarios = load_scenarios();
    assert_eq!(
        scenarios.len(),
        10,
        "the negative corpus must cover the ten mandate cases: {:?}",
        scenarios
            .iter()
            .map(|scenario| scenario.id.as_str())
            .collect::<Vec<_>>()
    );
    for scenario in &scenarios {
        assert_eq!(scenario.proposals.len(), scenario.expected_decisions.len());
        assert_eq!(scenario.proposals.len(), scenario.expected_levels.len());
        assert!(
            !scenario.description.trim().is_empty(),
            "{} must document itself",
            scenario.id
        );
        // The corpus itself encodes the central invariant: a case with no declared
        // verifier may never expect AUTO_SAFE.
        if scenario.verifier.is_none() {
            assert!(
                !scenario
                    .expected_decisions
                    .iter()
                    .any(|decision| *decision == DecisionKind::AutoSafe),
                "{}: AUTO_SAFE without a declared verifier is forbidden",
                scenario.id
            );
        }
    }
}

#[test]
fn the_ten_cases_produce_their_expected_statuses_and_reasons() {
    println!(
        "{:<38} {:<18} {:<24} {}",
        "case", "decision", "level", "lane"
    );
    for scenario in load_scenarios() {
        let scratch = scratch_dir(&scenario.id);
        let journal = scratch.join("journal.jsonl");
        let base = base_snapshot(&scenario);
        let mut coordinator = build_coordinator(&scenario, &journal);
        let mut observed = Vec::new();
        for (index, spec) in scenario.proposals.iter().enumerate() {
            let proposal = proposal_for(&scenario, &coordinator, spec, &base);
            let proposal_id = proposal.proposal_id();
            let decision = coordinator.submit(&proposal);
            let evaluation = coordinator
                .evaluation_for(&proposal_id)
                .expect("evaluation recorded");
            observed.push((decision.decision, evaluation.level));

            assert_eq!(
                decision.decision,
                scenario.expected_decisions[index],
                "{} / {}: unexpected decision (reasons: {})",
                scenario.id,
                proposal_id,
                decision.reasons.join(" | ")
            );
            assert_eq!(
                evaluation.level, scenario.expected_levels[index],
                "{} / {}: unexpected evaluation level",
                scenario.id, proposal_id
            );
            for expected in scenario
                .expect_reason_contains
                .get(index)
                .into_iter()
                .flatten()
            {
                let reasons = joined_reasons(&coordinator, &proposal_id);
                assert!(
                    reasons.contains(expected),
                    "{} / {}: journaled reasons must contain {:?}: {reasons}",
                    scenario.id,
                    proposal_id,
                    expected
                );
            }
            // A v2 acceptance can never rest on textual applicability alone.
            if decision.decision == DecisionKind::AutoSafe {
                assert!(
                    scenario.verifier.is_some(),
                    "{}: AUTO_SAFE without a declared verifier",
                    scenario.id
                );
                assert!(
                    evaluation.level.is_integration_grade(),
                    "{}: AUTO_SAFE with a non integration-grade level {:?}",
                    scenario.id,
                    evaluation.level
                );
                assert_eq!(
                    evaluation.verification.as_ref().map(|record| record.status),
                    Some(MergeCheckStatus::Passed)
                );
            }
            println!(
                "{:<38} {:<18} {:<24} {}",
                if index == 0 { scenario.id.as_str() } else { "" },
                decision.decision.as_str(),
                evaluation.level.as_str(),
                if index == 0 {
                    scenario.lane.as_str()
                } else {
                    ""
                }
            );
        }
        assert_eq!(observed.len(), scenario.expected_decisions.len());
    }
}

#[test]
fn a_textually_applicable_patch_without_declared_verification_is_never_auto_safe() {
    // Real textual path: the patch applies exactly, nothing overlaps, the default
    // textual oracle reports no finding — and yet v2 refuses it, because textual
    // applicability is not proof.
    let base = SourceSnapshot::new([(
        "lib.rs".to_owned(),
        "fn alpha() -> u32 {\n    1\n}\n\nfn beta() -> u32 {\n    2\n}\n".to_owned(),
    )]);
    let hash = base.file_hash("lib.rs").unwrap().to_owned();
    let patch = "+++ b/lib.rs\n@@ -1,3 +1,3 @@\n fn alpha() -> u32 {\n-    1\n+    10\n }\n";

    let mut coordinator = Coordinator::v2(base);
    let proposal = ChangeProposal {
        schema_version: COLLAB_SCHEMA_VERSION,
        agent_id: "agent-a".to_owned(),
        task_id: "alpha".to_owned(),
        base_snapshot_id: coordinator.current_id(),
        target_paths: vec!["lib.rs".to_owned()],
        target_symbols: vec!["alpha".to_owned()],
        expected_hashes: BTreeMap::from([("lib.rs".to_owned(), hash)]),
        declared_dependencies: Vec::new(),
        impacted_contracts: Vec::new(),
        unified_patch: patch.to_owned(),
        note: None,
    };
    let before = coordinator.current_id();
    let decision = coordinator.submit(&proposal);

    assert_eq!(decision.decision, DecisionKind::ManualRequired);
    assert_eq!(coordinator.current_id(), before, "nothing integrated");
    assert!(coordinator.accepted_changes().is_empty());
    let evaluation = coordinator.evaluations().last().expect("evaluation");
    assert_eq!(evaluation.protocol, EvaluationProtocol::V2SemanticStatuses);
    assert_eq!(evaluation.level, EvaluationLevel::TextuallyApplicable);
    assert_eq!(evaluation.decision, DecisionKind::ManualRequired);
    assert!(evaluation.verification.is_none());
    assert!(
        decision
            .reasons
            .iter()
            .any(|reason| reason.contains("textual applicability alone")),
        "the reason must state why AUTO_SAFE was not emitted: {:?}",
        decision.reasons
    );

    // The same patch under the v1 compatibility profile is AUTO_SAFE, but the
    // evaluation explicitly marks it as legacy textual applicability.
    let mut legacy = Coordinator::new(SourceSnapshot::new([(
        "lib.rs".to_owned(),
        "fn alpha() -> u32 {\n    1\n}\n\nfn beta() -> u32 {\n    2\n}\n".to_owned(),
    )]));
    let legacy_hash = legacy.base().file_hash("lib.rs").unwrap().to_owned();
    let mut legacy_proposal = proposal.clone();
    legacy_proposal.base_snapshot_id = legacy.current_id();
    legacy_proposal.expected_hashes = BTreeMap::from([("lib.rs".to_owned(), legacy_hash)]);
    let legacy_decision = legacy.submit(&legacy_proposal);
    assert_eq!(legacy_decision.decision, DecisionKind::AutoSafe);
    assert!(legacy
        .evaluations()
        .last()
        .unwrap()
        .is_legacy_textual_auto_safe());
}

#[test]
fn a_declared_verifier_is_what_promotes_a_patch_to_semantically_checked() {
    let scenario = load_scenarios()
        .into_iter()
        .find(|scenario| scenario.id == "01_same_function_twice")
        .expect("case 01");
    let scratch = scratch_dir("declared-verifier-lane");
    let journal = scratch.join("journal.jsonl");
    let base = base_snapshot(&scenario);
    let mut coordinator = build_coordinator(&scenario, &journal);
    let first = proposal_for(&scenario, &coordinator, &scenario.proposals[0], &base);
    let decision = coordinator.submit(&first);
    assert_eq!(decision.decision, DecisionKind::AutoSafe);
    let record = coordinator
        .accepted_changes()
        .last()
        .expect("accepted change");
    assert!(record.verification_required);
    assert_eq!(
        record
            .evaluation
            .as_ref()
            .map(|evaluation| evaluation.level),
        Some(EvaluationLevel::SemanticallyChecked)
    );
    assert_eq!(
        record
            .evaluation
            .as_ref()
            .map(|evaluation| evaluation.protocol),
        Some(EvaluationProtocol::V2SemanticStatuses)
    );
    assert_eq!(record.schema_version, COORDINATOR_SCHEMA_VERSION);
    assert!(record
        .evaluation
        .as_ref()
        .map(|evaluation| !evaluation.evidence_ids.is_empty())
        .unwrap_or(false));
}

#[test]
fn a_v1_journal_file_is_read_without_data_loss() {
    // A real v1 journal line: the bare `JournalEntry` serialisation produced by
    // COLLAB-1 (no top-level schema_version, no evaluation).
    let proposal = ChangeProposal {
        schema_version: COLLAB_SCHEMA_VERSION,
        agent_id: "agent-a".to_owned(),
        task_id: "legacy".to_owned(),
        base_snapshot_id: ravum_protocol::SnapshotId::new("legacy-base"),
        target_paths: vec!["lib.rs".to_owned()],
        target_symbols: Vec::new(),
        expected_hashes: BTreeMap::new(),
        declared_dependencies: Vec::new(),
        impacted_contracts: Vec::new(),
        unified_patch: String::new(),
        note: None,
    };
    let mut decision = ProposalDecision::new(&proposal, DecisionKind::AutoSafe);
    decision
        .reasons
        .push("legacy textual acceptance".to_owned());
    let entry = JournalEntry {
        seq: 4,
        decision,
        snapshot_before: ravum_protocol::SnapshotId::new("before-v1"),
        snapshot_after: ravum_protocol::SnapshotId::new("after-v1"),
    };
    let v1_line = serde_json::to_string(&entry).expect("serialize v1 entry");
    let top_level: serde_json::Value = serde_json::from_str(&v1_line).expect("valid json");
    assert!(top_level.get("schema_version").is_none());

    let scratch = scratch_dir("v1-journal-compat");
    let journal = scratch.join("journal.jsonl");
    std::fs::write(&journal, format!("{v1_line}\n")).expect("write v1 journal");

    let records = read_journal_records(&journal).expect("v1 journal is readable");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].schema_version, 1);
    assert!(records[0].evaluation.is_none());
    assert_eq!(records[0].entry, entry, "no field of the v1 entry is lost");
    assert_eq!(
        records[0].entry.decision.reasons,
        vec!["legacy textual acceptance".to_owned()]
    );
}
