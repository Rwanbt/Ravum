//! Honest verification statuses (INV-01) and the evidence record that binds a
//! verdict to the exact snapshot / build context / artifact it applies to.

use crate::ids::{ArtifactId, BuildContextId, SnapshotId, VerificationKey};
use serde::{Deserialize, Serialize};

pub const EVIDENCE_SCHEMA_VERSION: u32 = 1;

/// Status vocabulary. M1 code paths only ever produce `Tested`, `Assumed`,
/// `Pending`, `Stale`, `Timeout` or `Unknown`. `Proved` exists in the schema for
/// M2+ but has no producer in this MVP: claiming it here would be dishonest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Proved,
    Tested,
    Assumed,
    Pending,
    Stale,
    Timeout,
    Unknown,
}

impl VerificationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            VerificationStatus::Proved => "proved",
            VerificationStatus::Tested => "tested",
            VerificationStatus::Assumed => "assumed",
            VerificationStatus::Pending => "pending",
            VerificationStatus::Stale => "stale",
            VerificationStatus::Timeout => "timeout",
            VerificationStatus::Unknown => "unknown",
        }
    }

    /// Only these may gate an activation in the M1 profile.
    pub fn allows_activation(self) -> bool {
        matches!(self, VerificationStatus::Proved | VerificationStatus::Tested)
    }
}

/// One verification verdict, never detached from its context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceRecord {
    pub schema_version: u32,
    pub verification_key: VerificationKey,
    pub status: VerificationStatus,
    /// Human-readable statement of what was actually checked.
    pub property: String,
    /// Assumptions and limits of the check (empty is *not* "no assumptions").
    pub assumptions: Vec<String>,
    pub tool: String,
    pub tool_version: String,
    pub source_snapshot_id: SnapshotId,
    pub build_context_id: BuildContextId,
    pub artifact_id: ArtifactId,
    pub passed: bool,
    pub recorded_at_unix: u64,
}

impl EvidenceRecord {
    /// Evidence is only authoritative for the exact triple it was recorded on.
    /// Anything else must be re-run and is reported `Stale`, never reused.
    pub fn applies_to(
        &self,
        snapshot: &SnapshotId,
        build: &BuildContextId,
        artifact: &ArtifactId,
    ) -> bool {
        &self.source_snapshot_id == snapshot
            && &self.build_context_id == build
            && &self.artifact_id == artifact
    }

    /// The only condition under which M1 accepts activation.
    pub fn authorizes_activation(&self) -> bool {
        self.passed && self.status.allows_activation()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> EvidenceRecord {
        EvidenceRecord {
            schema_version: EVIDENCE_SCHEMA_VERSION,
            verification_key: VerificationKey::new("vk"),
            status: VerificationStatus::Tested,
            property: "smoke: finite + deterministic".into(),
            assumptions: vec!["single-threaded call".into()],
            tool: "ravum-cli".into(),
            tool_version: "0.1.0".into(),
            source_snapshot_id: SnapshotId::new("s"),
            build_context_id: BuildContextId::new("b"),
            artifact_id: ArtifactId::new("a"),
            passed: true,
            recorded_at_unix: 0,
        }
    }

    #[test]
    fn unknown_never_authorizes_activation() {
        let mut r = record();
        r.status = VerificationStatus::Unknown;
        assert!(!r.authorizes_activation());
        r.status = VerificationStatus::Timeout;
        assert!(!r.authorizes_activation());
        r.status = VerificationStatus::Tested;
        assert!(r.authorizes_activation());
    }

    #[test]
    fn evidence_is_bound_to_its_context() {
        let r = record();
        assert!(r.applies_to(&SnapshotId::new("s"), &BuildContextId::new("b"), &ArtifactId::new("a")));
        assert!(!r.applies_to(&SnapshotId::new("s2"), &BuildContextId::new("b"), &ArtifactId::new("a")));
    }
}
