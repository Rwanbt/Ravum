//! Machine-readable record of one accepted change (COLLAB-2 / L2 boundary).
//!
//! This is the stable hand-off between the episodic coordinator and a future
//! multi-session daemon (L2 / COLLAB-4). It deliberately contains **only**
//! identities and verdicts that the coordinator actually observed:
//!
//! * the exact snapshot before and after the integration (content-addressed);
//! * the paths and symbols the agent declared, plus the symbols the heuristic
//!   analysis attributed the changed lines to;
//! * the verification record that gated the acceptance, with a deterministic
//!   evidence id per check and per record;
//! * the journal sequence number, so the record can be reconciled with the
//!   append-only decision journal.
//!
//! Nothing here is a claim of semantic correctness: `verification.status` says
//! what was actually run, and `MergeCheckStatus::Skipped` explicitly records
//! that no semantic verification existed (COLLAB-1 compatibility mode).

use ravum_protocol::{sha256_hex, DecisionKind, SnapshotId};
use serde::{Deserialize, Serialize};

pub const COLLAB_RECORD_SCHEMA_VERSION: u32 = 1;

/// Kind of a semantic check performed on the candidate merged snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeCheckKind {
    /// `rustc --emit=metadata` on the candidate snapshot (compiles or not).
    MetadataCheck,
    /// The test harness compiles (`rustc --test`), without executing it.
    TestCompile,
    /// The compiled test harness was executed and its verdict recorded.
    TestRun,
}

impl MergeCheckKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MergeCheckKind::MetadataCheck => "metadata_check",
            MergeCheckKind::TestCompile => "test_compile",
            MergeCheckKind::TestRun => "test_run",
        }
    }
}

/// Outcome vocabulary of a check. `Unavailable` is a first-class value: a
/// required check that could not run is never silently promoted to success.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeCheckStatus {
    Passed,
    Failed,
    Unavailable,
    Skipped,
}

impl MergeCheckStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            MergeCheckStatus::Passed => "passed",
            MergeCheckStatus::Failed => "failed",
            MergeCheckStatus::Unavailable => "unavailable",
            MergeCheckStatus::Skipped => "skipped",
        }
    }
}

/// One compiler diagnostic retained as evidence (never the whole rendered blob:
/// the record must stay small and stable).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeDiagnostic {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub level: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

/// One executed check, bound to the exact candidate snapshot it ran on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeCheck {
    pub kind: MergeCheckKind,
    pub status: MergeCheckStatus,
    pub tool: String,
    pub tool_version: String,
    pub target: String,
    pub snapshot_id: SnapshotId,
    pub summary: String,
    pub diagnostics: Vec<MergeDiagnostic>,
    /// Deterministic digest of `(kind, snapshot, status, tool, target, summary,
    /// diagnostics)`: re-running the same check on the same content yields the
    /// same evidence id, so two independent observers can compare proofs.
    pub evidence_id: String,
}

impl MergeCheck {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        kind: MergeCheckKind,
        status: MergeCheckStatus,
        tool: impl Into<String>,
        tool_version: impl Into<String>,
        target: impl Into<String>,
        snapshot_id: SnapshotId,
        summary: impl Into<String>,
        diagnostics: Vec<MergeDiagnostic>,
    ) -> Self {
        let tool = tool.into();
        let tool_version = tool_version.into();
        let target = target.into();
        let summary = summary.into();
        let evidence_id = merge_check_evidence_id(
            kind,
            &snapshot_id,
            status,
            &tool,
            &tool_version,
            &target,
            &summary,
            &diagnostics,
        );
        Self {
            kind,
            status,
            tool,
            tool_version,
            target,
            snapshot_id,
            summary,
            diagnostics,
            evidence_id,
        }
    }

    pub fn passed(
        kind: MergeCheckKind,
        tool: impl Into<String>,
        tool_version: impl Into<String>,
        target: impl Into<String>,
        snapshot_id: SnapshotId,
        summary: impl Into<String>,
    ) -> Self {
        Self::new(
            kind,
            MergeCheckStatus::Passed,
            tool,
            tool_version,
            target,
            snapshot_id,
            summary,
            Vec::new(),
        )
    }

    pub fn failed(
        kind: MergeCheckKind,
        tool: impl Into<String>,
        tool_version: impl Into<String>,
        target: impl Into<String>,
        snapshot_id: SnapshotId,
        summary: impl Into<String>,
        diagnostics: Vec<MergeDiagnostic>,
    ) -> Self {
        Self::new(
            kind,
            MergeCheckStatus::Failed,
            tool,
            tool_version,
            target,
            snapshot_id,
            summary,
            diagnostics,
        )
    }
}

/// Deterministic identity of one check.
#[allow(clippy::too_many_arguments)]
pub fn merge_check_evidence_id(
    kind: MergeCheckKind,
    snapshot_id: &SnapshotId,
    status: MergeCheckStatus,
    tool: &str,
    tool_version: &str,
    target: &str,
    summary: &str,
    diagnostics: &[MergeDiagnostic],
) -> String {
    let mut buf = format!(
        "ravum-merge-check-v1\0{}\0{}\0{}\0{}\0{}\0{}\0{}",
        kind.as_str(),
        snapshot_id.as_str(),
        status.as_str(),
        tool,
        tool_version,
        target,
        summary
    );
    for diagnostic in diagnostics {
        buf.push('\0');
        buf.push_str(diagnostic.level.as_str());
        buf.push('\0');
        buf.push_str(diagnostic.code.as_deref().unwrap_or(""));
        buf.push('\0');
        buf.push_str(&diagnostic.message);
        buf.push('\0');
        buf.push_str(&diagnostic.line.map(|l| l.to_string()).unwrap_or_default());
    }
    sha256_hex(buf.as_bytes())
}

/// Aggregate verification record attached to an accepted change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationRecord {
    pub schema_version: u32,
    /// Checks the coordinator required before accepting the merge.
    pub required_checks: Vec<MergeCheckKind>,
    pub status: MergeCheckStatus,
    pub checks: Vec<MergeCheck>,
    /// Digest over the required checks and every per-check evidence id.
    pub evidence_id: String,
}

impl VerificationRecord {
    /// COLLAB-1 compatibility: no semantic verification was configured, so the
    /// record says so instead of pretending the merge was compiled.
    pub fn not_required() -> Self {
        let required = Vec::new();
        let checks = Vec::new();
        let status = MergeCheckStatus::Skipped;
        let evidence_id = verification_evidence_id(&required, status, &checks);
        Self {
            schema_version: COLLAB_RECORD_SCHEMA_VERSION,
            required_checks: required,
            status,
            checks,
            evidence_id,
        }
    }

    pub fn from_checks(required_checks: Vec<MergeCheckKind>, checks: Vec<MergeCheck>) -> Self {
        let status = if checks
            .iter()
            .all(|check| check.status == MergeCheckStatus::Passed)
        {
            MergeCheckStatus::Passed
        } else {
            MergeCheckStatus::Failed
        };
        let evidence_id = verification_evidence_id(&required_checks, status, &checks);
        Self {
            schema_version: COLLAB_RECORD_SCHEMA_VERSION,
            required_checks,
            status,
            checks,
            evidence_id,
        }
    }

    pub fn evidence_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .checks
            .iter()
            .map(|check| check.evidence_id.clone())
            .collect();
        ids.push(self.evidence_id.clone());
        ids
    }

    /// One-line human summary used in journaled decision reasons.
    pub fn summary_line(&self) -> String {
        if self.checks.is_empty() {
            return format!(
                "semantic verification: {} (no check executed, evidence {})",
                self.status.as_str(),
                self.evidence_id
            );
        }
        let checks: Vec<String> = self
            .checks
            .iter()
            .map(|check| format!("{}={}", check.kind.as_str(), check.status.as_str()))
            .collect();
        format!(
            "semantic verification: {} ({}, evidence {})",
            self.status.as_str(),
            checks.join(", "),
            self.evidence_id
        )
    }
}

/// Deterministic identity of one aggregate verification record.
pub fn verification_evidence_id(
    required: &[MergeCheckKind],
    status: MergeCheckStatus,
    checks: &[MergeCheck],
) -> String {
    let mut buf = format!("ravum-verification-v1\0{}\0", status.as_str());
    for kind in required {
        buf.push_str(kind.as_str());
        buf.push('\0');
    }
    for check in checks {
        buf.push_str(&check.evidence_id);
        buf.push('\0');
    }
    sha256_hex(buf.as_bytes())
}

/// One accepted change: the record an L2 daemon (COLLAB-4) can consume without
/// re-running the coordinator and without trusting prose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptedChange {
    pub schema_version: u32,
    pub proposal_id: String,
    pub agent_id: String,
    pub task_id: String,
    /// Always `AUTO_SAFE` today; kept typed so a future partial acceptance can
    /// be represented without changing the record shape.
    pub decision: DecisionKind,
    /// Sequence number of the journaled decision that produced this record.
    pub journal_seq: u64,
    pub snapshot_before: SnapshotId,
    pub snapshot_after: SnapshotId,
    pub paths: Vec<String>,
    pub declared_symbols: Vec<String>,
    /// Symbols the heuristic analysis attributed the changed lines to.
    pub modified_symbols: Vec<String>,
    pub verification_required: bool,
    pub verification: VerificationRecord,
    pub evidence_ids: Vec<String>,
    pub recorded_at_unix: u64,
}

impl AcceptedChange {
    pub fn verification_summary(&self) -> String {
        self.verification.summary_line()
    }
}

/// Deterministic identity of an accepted-change record. Used by downstream
/// consumers that need a stable key for an activation log entry.
pub fn accepted_change_evidence_id(change: &AcceptedChange) -> String {
    let mut buf = format!(
        "ravum-accepted-change-v1\0{}\0{}\0{}\0",
        change.proposal_id,
        change.snapshot_before.as_str(),
        change.snapshot_after.as_str()
    );
    for evidence in &change.evidence_ids {
        buf.push_str(evidence);
        buf.push('\0');
    }
    sha256_hex(buf.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ravum_protocol::SnapshotId;

    fn check(status: MergeCheckStatus, summary: &str) -> MergeCheck {
        MergeCheck::new(
            MergeCheckKind::MetadataCheck,
            status,
            "rustc",
            "1.95.0",
            "x86_64-pc-windows-msvc",
            SnapshotId::new("snapshot-a"),
            summary,
            vec![MergeDiagnostic {
                code: Some("E0428".to_owned()),
                level: "error".to_owned(),
                message: "the name `helper2` is defined multiple times".to_owned(),
                line: Some(3),
            }],
        )
    }

    #[test]
    fn evidence_ids_are_deterministic_and_status_sensitive() {
        let passed_a = check(MergeCheckStatus::Passed, "compiles");
        let passed_b = check(MergeCheckStatus::Passed, "compiles");
        assert_eq!(passed_a.evidence_id, passed_b.evidence_id);
        let failed = check(MergeCheckStatus::Failed, "does not compile");
        assert_ne!(passed_a.evidence_id, failed.evidence_id);
        assert_eq!(passed_a.evidence_id.len(), 64);
    }

    #[test]
    fn a_skipped_record_never_claims_a_passed_check() {
        let record = VerificationRecord::not_required();
        assert_eq!(record.status, MergeCheckStatus::Skipped);
        assert!(record.checks.is_empty());
        assert!(record.summary_line().contains("no check executed"));
    }

    #[test]
    fn aggregate_record_status_requires_every_check_to_pass() {
        let passed = VerificationRecord::from_checks(
            vec![MergeCheckKind::MetadataCheck],
            vec![check(MergeCheckStatus::Passed, "compiles")],
        );
        assert_eq!(passed.status, MergeCheckStatus::Passed);
        let failed = VerificationRecord::from_checks(
            vec![MergeCheckKind::MetadataCheck],
            vec![check(MergeCheckStatus::Failed, "does not compile")],
        );
        assert_eq!(failed.status, MergeCheckStatus::Failed);
        assert_ne!(passed.evidence_id, failed.evidence_id);
    }
}
