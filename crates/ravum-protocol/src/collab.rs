//! Multi-agent change proposals (COLLAB-1).
//!
//! A proposal is *always* expressed against a `base_snapshot_id` and carries the
//! hashes the author observed. The coordinator — not the agent — decides whether
//! it can be integrated. Nothing here is a CRDT: convergence is not correctness.

use crate::ids::SnapshotId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const COLLAB_SCHEMA_VERSION: u32 = 1;

/// A patch proposal produced by one agent for one task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeProposal {
    pub schema_version: u32,
    pub agent_id: String,
    pub task_id: String,
    /// Snapshot the agent based its work on. Stale bases are refused, not rebased silently.
    pub base_snapshot_id: SnapshotId,
    pub target_paths: Vec<String>,
    pub target_symbols: Vec<String>,
    /// Path -> sha256 the agent observed. Compare-and-swap precondition.
    pub expected_hashes: BTreeMap<String, String>,
    /// Paths whose behaviour this change depends on being unchanged.
    pub declared_dependencies: Vec<String>,
    pub impacted_contracts: Vec<String>,
    /// Unified diff, applied by the coordinator in an in-memory overlay.
    pub unified_patch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl ChangeProposal {
    pub fn proposal_id(&self) -> String {
        format!("{}/{}", self.agent_id, self.task_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DecisionKind {
    /// Independent, preconditions satisfied, patch applies cleanly.
    AutoSafe,
    /// Could be replayed on a newer snapshot but requires fresh validation.
    RebaseRevalidate,
    /// Incompatible with an already integrated change on the same unit.
    Conflict,
    /// Dangerous, invalid, stale or unauthorized; no partial write happened.
    Reject,
    /// Requires a human or semantic arbitration the tool cannot demonstrate.
    ManualRequired,
}

impl DecisionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DecisionKind::AutoSafe => "AUTO_SAFE",
            DecisionKind::RebaseRevalidate => "REBASE_REVALIDATE",
            DecisionKind::Conflict => "CONFLICT",
            DecisionKind::Reject => "REJECT",
            DecisionKind::ManualRequired => "MANUAL_REQUIRED",
        }
    }

    pub fn is_integrated(self) -> bool {
        matches!(self, DecisionKind::AutoSafe)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalDecision {
    pub schema_version: u32,
    pub proposal_id: String,
    pub agent_id: String,
    pub task_id: String,
    pub base_snapshot_id: SnapshotId,
    pub decision: DecisionKind,
    /// Every reason is kept; none is overwritten by a later decision.
    pub reasons: Vec<String>,
    pub integrated_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resulting_snapshot_id: Option<SnapshotId>,
    pub decided_at_unix: u64,
}

impl ProposalDecision {
    pub fn new(proposal: &ChangeProposal, decision: DecisionKind) -> Self {
        Self {
            schema_version: COLLAB_SCHEMA_VERSION,
            proposal_id: proposal.proposal_id(),
            agent_id: proposal.agent_id.clone(),
            task_id: proposal.task_id.clone(),
            base_snapshot_id: proposal.base_snapshot_id.clone(),
            decision,
            reasons: Vec::new(),
            integrated_paths: Vec::new(),
            resulting_snapshot_id: None,
            decided_at_unix: 0,
        }
    }

    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reasons.push(reason.into());
        self
    }
}

/// One append-only journal line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalEntry {
    pub seq: u64,
    pub decision: ProposalDecision,
    /// Snapshot that was current before this decision.
    pub snapshot_before: SnapshotId,
    /// Snapshot that is current after this decision (unchanged if refused).
    pub snapshot_after: SnapshotId,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proposal_id_is_agent_scoped() {
        let p = ChangeProposal {
            schema_version: COLLAB_SCHEMA_VERSION,
            agent_id: "worker-b".into(),
            task_id: "t1".into(),
            base_snapshot_id: SnapshotId::new("s"),
            target_paths: vec!["a.rs".into()],
            target_symbols: vec!["f".into()],
            expected_hashes: BTreeMap::new(),
            declared_dependencies: vec![],
            impacted_contracts: vec![],
            unified_patch: String::new(),
            note: None,
        };
        assert_eq!(p.proposal_id(), "worker-b/t1");
    }

    #[test]
    fn decision_tagging_is_explicit() {
        assert_eq!(DecisionKind::Conflict.as_str(), "CONFLICT");
        assert!(DecisionKind::AutoSafe.is_integrated());
        assert!(!DecisionKind::Reject.is_integrated());
    }
}
