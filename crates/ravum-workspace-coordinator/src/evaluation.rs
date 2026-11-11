//! Versioned evaluation protocol (MVP-0c L3).
//!
//! COLLAB-1/2/3 spoke one language: a `DecisionKind` plus reasons. That is not
//! enough to answer the question a multi-agent consumer actually asks — *how far
//! was this merge verified?* — and it made a textually applicable patch look like
//! an accepted semantic merge.
//!
//! This module adds a second, additive axis: an [`EvaluationLevel`] attached to
//! every journaled decision and every accepted-change record, under a versioned
//! schema ([`COORDINATOR_SCHEMA_VERSION`]).
//!
//! Levels, from weakest to strongest:
//! * `TEXTUALLY_APPLICABLE` — the patch applies exactly on the current content and
//!   passes textual preconditions. **It is not an acceptance**: without a declared
//!   semantic verification the decision becomes `MANUAL_REQUIRED` (or
//!   `REBASE_REVALIDATE` when revalidation is the honest next step);
//! * `SEMANTICALLY_CHECKED` — a declared verifier ran on the exact candidate
//!   snapshot and every required check passed;
//! * `TESTED` — the merged snapshot's test harness compiled **and executed**
//!   successfully;
//! * `PROVED` — a [`crate::oracle::BoundedProof`] with complete explicit
//!   provenance applies to the exact candidate snapshot, on top of the required
//!   checks. `PROVED` is always bounded: it names a property, a scope and
//!   assumptions. There is no implicit `PROVED`;
//! * `REJECTED` / `MANUAL_REQUIRED` — terminal non-integration levels.
//!
//! `AUTO_SAFE` is kept unchanged for compatibility, but its **meaning is now
//! constrained**: under the v2 protocol `AUTO_SAFE` is only emitted with a level
//! at least `SEMANTICALLY_CHECKED` (or `TESTED`/`PROVED`). The v1 protocol is
//! still available as the explicit COLLAB-1 compatibility profile, where
//! `AUTO_SAFE` historically meant textual applicability only; such records are
//! marked `protocol = V1_COLLAB_COMPAT` so no consumer can mistake them for a
//! semantic acceptance.
//!
//! Journals written before this module (v1) are plain `JournalEntry` lines. They
//! remain readable without loss: the reader tolerates both shapes and reports
//! `schema_version = 1` for legacy lines.

use crate::oracle::{BoundedProof, OracleReport};
use crate::records::{MergeCheckStatus, VerificationRecord};
use ravum_protocol::{DecisionKind, JournalEntry, SnapshotId};
use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;

/// Version of the coordinator record/journal protocol.
///
/// * 1 — legacy COLLAB-1 journals: one `JournalEntry` per line, no evaluation.
/// * 2 — each line is a `JournalEntry` **superset**: the same flattened fields as
///   v1 plus `schema_version` and an `evaluation` record. v1 readers ignore the
///   extra fields; the v2 reader accepts both shapes.
pub const COORDINATOR_SCHEMA_VERSION: u32 = 2;

/// Which protocol a coordinator instance speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EvaluationProtocol {
    /// COLLAB-1 compatibility: `AUTO_SAFE` means "textually applicable", nothing
    /// more. Kept only so existing callers keep their exact behaviour.
    V1CollabCompat,
    /// Strict protocol: `AUTO_SAFE` requires a declared semantic verification and
    /// a level at least `SEMANTICALLY_CHECKED`.
    V2SemanticStatuses,
}

impl EvaluationProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            EvaluationProtocol::V1CollabCompat => "V1_COLLAB_COMPAT",
            EvaluationProtocol::V2SemanticStatuses => "V2_SEMANTIC_STATUSES",
        }
    }

    pub fn schema_version(self) -> u32 {
        match self {
            EvaluationProtocol::V1CollabCompat => 1,
            EvaluationProtocol::V2SemanticStatuses => COORDINATOR_SCHEMA_VERSION,
        }
    }

    pub fn is_v2(self) -> bool {
        matches!(self, EvaluationProtocol::V2SemanticStatuses)
    }
}

impl Default for EvaluationProtocol {
    fn default() -> Self {
        EvaluationProtocol::V1CollabCompat
    }
}

/// Explicit evaluation level attached to every decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EvaluationLevel {
    TextuallyApplicable,
    SemanticallyChecked,
    Tested,
    Proved,
    Rejected,
    ManualRequired,
}

impl EvaluationLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            EvaluationLevel::TextuallyApplicable => "TEXTUALLY_APPLICABLE",
            EvaluationLevel::SemanticallyChecked => "SEMANTICALLY_CHECKED",
            EvaluationLevel::Tested => "TESTED",
            EvaluationLevel::Proved => "PROVED",
            EvaluationLevel::Rejected => "REJECTED",
            EvaluationLevel::ManualRequired => "MANUAL_REQUIRED",
        }
    }

    /// Rank on the verification axis (higher is stronger). Terminal refusal
    /// levels rank at 0: they are not verification progress.
    pub fn rank(self) -> u8 {
        match self {
            EvaluationLevel::Rejected | EvaluationLevel::ManualRequired => 0,
            EvaluationLevel::TextuallyApplicable => 1,
            EvaluationLevel::SemanticallyChecked => 2,
            EvaluationLevel::Tested => 3,
            EvaluationLevel::Proved => 4,
        }
    }

    /// Levels that justify an integration under the v2 protocol.
    pub fn is_integration_grade(self) -> bool {
        matches!(
            self,
            EvaluationLevel::SemanticallyChecked
                | EvaluationLevel::Tested
                | EvaluationLevel::Proved
        )
    }

    pub fn is_terminal_refusal(self) -> bool {
        matches!(
            self,
            EvaluationLevel::Rejected | EvaluationLevel::ManualRequired
        )
    }
}

/// Compute the evaluation level implied by a decision and its evidence.
///
/// This is the single place that decides whether `AUTO_SAFE` is justified:
/// * `AUTO_SAFE` with a passed verification record is at least
///   `SEMANTICALLY_CHECKED`, `TESTED` if the test harness ran, `PROVED` only when
///   an explicit bounded proof applies;
/// * `AUTO_SAFE` without a passed verification record is only
///   `TEXTUALLY_APPLICABLE` (legacy v1 profile);
/// * `REJECT`/`CONFLICT` are `REJECTED`, `MANUAL_REQUIRED` is terminal, and
///   `REBASE_REVALIDATE` means textually applicable but not integrated.
pub fn level_for_decision(
    decision: DecisionKind,
    verification: Option<&VerificationRecord>,
    bounded_proof: Option<&BoundedProof>,
) -> EvaluationLevel {
    match decision {
        DecisionKind::AutoSafe => {
            let Some(record) = verification.filter(|record| {
                record.status == MergeCheckStatus::Passed
                    && record
                        .checks
                        .iter()
                        .any(|check| check.status == MergeCheckStatus::Passed)
            }) else {
                return EvaluationLevel::TextuallyApplicable;
            };
            if bounded_proof
                .map(BoundedProof::is_explicit)
                .unwrap_or(false)
            {
                return EvaluationLevel::Proved;
            }
            if record.checks.iter().any(|check| {
                check.kind == crate::records::MergeCheckKind::TestRun
                    && check.status == MergeCheckStatus::Passed
            }) {
                EvaluationLevel::Tested
            } else {
                EvaluationLevel::SemanticallyChecked
            }
        }
        DecisionKind::Reject | DecisionKind::Conflict => EvaluationLevel::Rejected,
        DecisionKind::ManualRequired => EvaluationLevel::ManualRequired,
        DecisionKind::RebaseRevalidate => EvaluationLevel::TextuallyApplicable,
    }
}

/// One evaluation record: the versioned, self-contained answer to "what was this
/// decision based on?".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvaluationRecord {
    pub schema_version: u32,
    pub protocol: EvaluationProtocol,
    pub level: EvaluationLevel,
    pub decision: DecisionKind,
    pub proposal_id: String,
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oracle: Option<OracleReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification: Option<VerificationRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounded_proof: Option<BoundedProof>,
    /// A copy of the decision reasons: the evaluation record is readable on its
    /// own, without joining it to the journal entry.
    pub reasons: Vec<String>,
    pub evidence_ids: Vec<String>,
    pub snapshot_before: SnapshotId,
    pub snapshot_after: SnapshotId,
    pub journal_seq: u64,
    pub recorded_at_unix: u64,
}

impl EvaluationRecord {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        protocol: EvaluationProtocol,
        level: EvaluationLevel,
        decision: DecisionKind,
        proposal_id: impl Into<String>,
        agent_id: impl Into<String>,
        oracle: Option<OracleReport>,
        verification: Option<VerificationRecord>,
        bounded_proof: Option<BoundedProof>,
        reasons: Vec<String>,
        evidence_ids: Vec<String>,
        snapshot_before: SnapshotId,
        snapshot_after: SnapshotId,
        journal_seq: u64,
    ) -> Self {
        Self {
            schema_version: COORDINATOR_SCHEMA_VERSION,
            protocol,
            level,
            decision,
            proposal_id: proposal_id.into(),
            agent_id: agent_id.into(),
            oracle,
            verification,
            bounded_proof,
            reasons,
            evidence_ids,
            snapshot_before,
            snapshot_after,
            journal_seq,
            recorded_at_unix: ravum_protocol::now_unix(),
        }
    }

    /// True for a legacy v1 `AUTO_SAFE`: textually applicable only. Consumers must
    /// not read this as a semantic acceptance.
    pub fn is_legacy_textual_auto_safe(&self) -> bool {
        self.protocol == EvaluationProtocol::V1CollabCompat
            && self.decision == DecisionKind::AutoSafe
            && self.level == EvaluationLevel::TextuallyApplicable
    }

    pub fn summary_line(&self) -> String {
        format!(
            "evaluation: {} (decision {}, protocol {})",
            self.level.as_str(),
            self.decision.as_str(),
            self.protocol.as_str()
        )
    }
}

/// One line of the coordinator decision journal (schema v2).
///
/// The `JournalEntry` fields are flattened at the top level so a v1 reader still
/// parses the line; `schema_version` and `evaluation` are additive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoordinatorJournalRecord {
    #[serde(flatten)]
    pub entry: JournalEntry,
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluation: Option<EvaluationRecord>,
}

impl CoordinatorJournalRecord {
    pub fn new(entry: JournalEntry, evaluation: Option<EvaluationRecord>) -> Self {
        Self {
            entry,
            schema_version: COORDINATOR_SCHEMA_VERSION,
            evaluation,
        }
    }

    /// The evaluation attached to this line, if it was written by a v2 producer.
    pub fn evaluation(&self) -> Option<&EvaluationRecord> {
        self.evaluation.as_ref()
    }
}

impl From<JournalEntry> for CoordinatorJournalRecord {
    fn from(entry: JournalEntry) -> Self {
        Self {
            entry,
            schema_version: 1,
            evaluation: None,
        }
    }
}

/// Errors of the tolerant journal reader.
#[derive(Debug)]
pub enum JournalReadError {
    Io(io::Error),
    Malformed { line: usize, message: String },
}

impl std::fmt::Display for JournalReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JournalReadError::Io(error) => write!(f, "journal io error: {error}"),
            JournalReadError::Malformed { line, message } => {
                write!(f, "malformed journal line {line}: {message}")
            }
        }
    }
}

impl std::error::Error for JournalReadError {}

impl From<io::Error> for JournalReadError {
    fn from(error: io::Error) -> Self {
        JournalReadError::Io(error)
    }
}

/// Parse one journal line, accepting both the v1 shape (a bare `JournalEntry`) and
/// the v2 superset. No field of a v1 entry is lost.
pub fn parse_journal_line(line: &str) -> Result<CoordinatorJournalRecord, String> {
    let value: serde_json::Value =
        serde_json::from_str(line).map_err(|error| format!("invalid JSON: {error}"))?;
    parse_journal_value(value)
}

fn parse_journal_value(value: serde_json::Value) -> Result<CoordinatorJournalRecord, String> {
    let declared_schema = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64);
    let entry: JournalEntry = serde_json::from_value(value.clone())
        .map_err(|error| format!("invalid decision journal entry: {error}"))?;
    match declared_schema {
        Some(schema) if schema >= COORDINATOR_SCHEMA_VERSION as u64 => {
            let record: CoordinatorJournalRecord = serde_json::from_value(value)
                .map_err(|error| format!("invalid v2 coordinator journal record: {error}"))?;
            if record.evaluation.is_none() {
                return Err("v2 journal record has no evaluation".to_owned());
            }
            Ok(record)
        }
        Some(schema) => Ok(CoordinatorJournalRecord {
            entry,
            schema_version: schema as u32,
            evaluation: None,
        }),
        None => Ok(CoordinatorJournalRecord {
            entry,
            schema_version: 1,
            evaluation: None,
        }),
    }
}

/// Read a whole journal file, tolerating v1 and v2 lines.
pub fn read_journal_records(
    path: &Path,
) -> Result<Vec<CoordinatorJournalRecord>, JournalReadError> {
    use std::io::BufRead;
    let file = std::fs::File::open(path).map_err(JournalReadError::Io)?;
    let reader = io::BufReader::new(file);
    let mut records = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line = line.map_err(JournalReadError::Io)?;
        if line.trim().is_empty() {
            continue;
        }
        let record = parse_journal_line(&line).map_err(|message| JournalReadError::Malformed {
            line: index + 1,
            message,
        })?;
        records.push(record);
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::{MergeCheck, MergeCheckKind, MergeDiagnostic};
    use ravum_protocol::{ProposalDecision, COLLAB_SCHEMA_VERSION};

    fn entry(seq: u64, decision: DecisionKind) -> JournalEntry {
        let proposal = ravum_protocol::ChangeProposal {
            schema_version: COLLAB_SCHEMA_VERSION,
            agent_id: "agent-a".to_owned(),
            task_id: "t".to_owned(),
            base_snapshot_id: SnapshotId::new("base"),
            target_paths: vec!["lib.rs".to_owned()],
            target_symbols: Vec::new(),
            expected_hashes: Default::default(),
            declared_dependencies: Vec::new(),
            impacted_contracts: Vec::new(),
            unified_patch: String::new(),
            note: None,
        };
        let mut proposal_decision = ProposalDecision::new(&proposal, decision);
        proposal_decision.reasons.push("because".to_owned());
        JournalEntry {
            seq,
            decision: proposal_decision,
            snapshot_before: SnapshotId::new("before"),
            snapshot_after: SnapshotId::new("after"),
        }
    }

    fn passed_metadata_record() -> VerificationRecord {
        VerificationRecord::from_checks(
            vec![MergeCheckKind::MetadataCheck],
            vec![MergeCheck::passed(
                MergeCheckKind::MetadataCheck,
                "rustc",
                "1.95.0",
                "host",
                SnapshotId::new("candidate"),
                "compiles",
            )],
        )
    }

    #[test]
    fn a_v1_journal_line_is_read_without_loss() {
        // A real v1 line is the bare serialisation of the protocol JournalEntry.
        let v1_entry = entry(7, DecisionKind::AutoSafe);
        let line = serde_json::to_string(&v1_entry).expect("serialize v1");
        let top_level: serde_json::Value = serde_json::from_str(&line).expect("valid JSON");
        assert!(
            top_level.get("schema_version").is_none(),
            "a v1 line has no top-level schema_version (the nested proposal decision has its own)"
        );

        let record = parse_journal_line(&line).expect("v1 line parses");
        assert_eq!(record.schema_version, 1);
        assert!(record.evaluation.is_none());
        assert_eq!(record.entry, v1_entry);
    }

    #[test]
    fn a_v2_journal_line_round_trips_with_its_evaluation() {
        let v2_entry = entry(8, DecisionKind::AutoSafe);
        let evaluation = EvaluationRecord::new(
            EvaluationProtocol::V2SemanticStatuses,
            EvaluationLevel::SemanticallyChecked,
            DecisionKind::AutoSafe,
            "agent-a/t",
            "agent-a",
            None,
            Some(passed_metadata_record()),
            None,
            vec!["semantic verification: passed".to_owned()],
            vec!["evidence-a".to_owned()],
            SnapshotId::new("before"),
            SnapshotId::new("after"),
            8,
        );
        let record = CoordinatorJournalRecord::new(v2_entry.clone(), Some(evaluation.clone()));
        let line = serde_json::to_string(&record).expect("serialize v2");

        // A v1 reader still sees a valid JournalEntry (extra fields are ignored).
        let legacy: JournalEntry = serde_json::from_str(&line).expect("v1 reader accepts v2 line");
        assert_eq!(legacy, v2_entry);

        let parsed = parse_journal_line(&line).expect("v2 line parses");
        assert_eq!(parsed.schema_version, COORDINATOR_SCHEMA_VERSION);
        assert_eq!(parsed.entry, v2_entry);
        assert_eq!(parsed.evaluation, Some(evaluation));
    }

    #[test]
    fn a_v2_line_without_evaluation_is_refused() {
        let mut line = serde_json::to_value(entry(9, DecisionKind::Reject)).expect("value");
        line["schema_version"] = serde_json::json!(COORDINATOR_SCHEMA_VERSION);
        assert!(parse_journal_line(&line.to_string()).is_err());
    }

    #[test]
    fn level_mapping_never_promotes_textual_applicability_to_semantics() {
        assert_eq!(
            level_for_decision(DecisionKind::AutoSafe, None, None),
            EvaluationLevel::TextuallyApplicable
        );
        let record = passed_metadata_record();
        assert_eq!(
            level_for_decision(DecisionKind::AutoSafe, Some(&record), None),
            EvaluationLevel::SemanticallyChecked
        );
        let tested = VerificationRecord::from_checks(
            vec![MergeCheckKind::MetadataCheck, MergeCheckKind::TestRun],
            vec![
                MergeCheck::passed(
                    MergeCheckKind::MetadataCheck,
                    "rustc",
                    "1.95.0",
                    "host",
                    SnapshotId::new("candidate"),
                    "compiles",
                ),
                MergeCheck::passed(
                    MergeCheckKind::TestRun,
                    "rustc --test",
                    "1.95.0",
                    "host",
                    SnapshotId::new("candidate"),
                    "tests pass",
                ),
            ],
        );
        assert_eq!(
            level_for_decision(DecisionKind::AutoSafe, Some(&tested), None),
            EvaluationLevel::Tested
        );
        assert_eq!(
            level_for_decision(DecisionKind::Conflict, None, None),
            EvaluationLevel::Rejected
        );
        assert_eq!(
            level_for_decision(DecisionKind::RebaseRevalidate, None, None),
            EvaluationLevel::TextuallyApplicable
        );
        assert_eq!(
            level_for_decision(DecisionKind::ManualRequired, None, None),
            EvaluationLevel::ManualRequired
        );
    }

    #[test]
    fn proved_requires_an_explicit_bounded_proof() {
        let target = SnapshotId::new("candidate");
        let record = passed_metadata_record();
        let explicit = BoundedProof::new(
            "buffer access is race free",
            ["src/dsp.rs"],
            ["single writer"],
            "kani",
            "0.40.0",
            target.clone(),
        );
        assert_eq!(
            level_for_decision(DecisionKind::AutoSafe, Some(&record), Some(&explicit)),
            EvaluationLevel::Proved
        );
        let implicit = BoundedProof::new(
            "buffer access is race free",
            Vec::<String>::new(),
            Vec::<String>::new(),
            "",
            "",
            target,
        );
        assert_eq!(
            level_for_decision(DecisionKind::AutoSafe, Some(&record), Some(&implicit)),
            EvaluationLevel::SemanticallyChecked
        );
    }

    #[test]
    fn integration_grade_levels_are_only_the_verified_ones() {
        assert!(!EvaluationLevel::TextuallyApplicable.is_integration_grade());
        assert!(EvaluationLevel::SemanticallyChecked.is_integration_grade());
        assert!(EvaluationLevel::Tested.is_integration_grade());
        assert!(EvaluationLevel::Proved.is_integration_grade());
        assert!(!EvaluationLevel::Rejected.is_integration_grade());
        assert!(!EvaluationLevel::ManualRequired.is_integration_grade());
        assert!(EvaluationLevel::Proved.rank() > EvaluationLevel::Tested.rank());
    }

    #[test]
    fn a_legacy_record_is_marked_as_such() {
        let evaluation = EvaluationRecord::new(
            EvaluationProtocol::V1CollabCompat,
            EvaluationLevel::TextuallyApplicable,
            DecisionKind::AutoSafe,
            "agent-a/t",
            "agent-a",
            None,
            Some(VerificationRecord::not_required()),
            None,
            Vec::new(),
            Vec::new(),
            SnapshotId::new("before"),
            SnapshotId::new("after"),
            1,
        );
        assert!(evaluation.is_legacy_textual_auto_safe());
        assert_eq!(
            evaluation.summary_line(),
            "evaluation: TEXTUALLY_APPLICABLE (decision AUTO_SAFE, protocol V1_COLLAB_COMPAT)"
        );
    }

    #[test]
    fn diagnostic_import_is_used() {
        // Keep the MergeDiagnostic import meaningful for readers of this test module:
        // failed checks carry diagnostics, and the level mapping must not depend on them.
        let check = MergeCheck::failed(
            MergeCheckKind::MetadataCheck,
            "rustc",
            "1.95.0",
            "host",
            SnapshotId::new("candidate"),
            "does not compile",
            vec![MergeDiagnostic {
                code: Some("E0428".to_owned()),
                level: "error".to_owned(),
                message: "duplicate".to_owned(),
                line: Some(3),
            }],
        );
        assert_eq!(check.status, MergeCheckStatus::Failed);
    }
}
