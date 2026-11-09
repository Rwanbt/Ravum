//! Semantic verification injection point (COLLAB-2 / COLLAB-3).
//!
//! The coordinator **never** imports a compiler or an adapter: it owns the
//! `MergeVerifier` trait, and the production binary injects an implementation
//! based on the public `rustc` CLI (see `src/bin/common/mod.rs`). This is what
//! keeps the coordinator usable in tests and in a future daemon that may run the
//! verification elsewhere.
//!
//! Contract of the gate:
//! * the coordinator computes the checks the merge actually requires
//!   (`required_checks`) from the patch semantics, not from the agent's claims;
//! * a verifier that cannot run a required check returns `Unavailable`, and the
//!   decision becomes `MANUAL_REQUIRED` — a textually clean merge is never
//!   accepted by default when the required semantic check could not run;
//! * a verifier that claims `Passed` while omitting a required check is treated
//!   as an incomplete proof and refused.

use crate::records::{MergeCheck, MergeCheckKind, MergeCheckStatus};
use ravum_snapshot::SourceSnapshot;

/// Everything a verifier needs, and nothing that would let it mutate coordinator
/// state. Both snapshots are provided so a verifier can diff them itself; the
/// coordinator only guarantees `snapshot_after` is the candidate merge.
#[derive(Debug, Clone)]
pub struct VerificationRequest {
    pub proposal_id: String,
    pub agent_id: String,
    pub snapshot_before: SourceSnapshot,
    pub snapshot_after: SourceSnapshot,
    pub changed_paths: Vec<String>,
    pub declared_symbols: Vec<String>,
    /// Check kinds the coordinator requires before accepting this merge.
    pub required_checks: Vec<MergeCheckKind>,
}

impl VerificationRequest {
    pub fn requires(&self, kind: MergeCheckKind) -> bool {
        self.required_checks.contains(&kind)
    }
}

/// Verdict of one verification attempt.
///
/// `Failed` carries the concrete check (with diagnostics and evidence id) that
/// failed; `Unavailable` means the verifier could not run a required check — it
/// is **not** a pass and never becomes one.
#[derive(Debug, Clone)]
pub enum VerificationOutcome {
    Passed { checks: Vec<MergeCheck> },
    Failed { check: MergeCheck, reason: String },
    Unavailable { reason: String },
}

/// Injected semantic verifier.
///
/// Implementations run out-of-process tooling (`rustc` CLI is the reference
/// implementation) and must be side-effect free with respect to the workspace:
/// they may only write under their own scratch directory.
pub trait MergeVerifier: Send + Sync {
    fn name(&self) -> &'static str {
        "unnamed-verifier"
    }

    fn verify(&self, request: &VerificationRequest) -> VerificationOutcome;
}

/// Required checks that are absent from `checks` or did not pass.
pub fn missing_required_checks(
    required: &[MergeCheckKind],
    checks: &[MergeCheck],
) -> Vec<MergeCheckKind> {
    required
        .iter()
        .copied()
        .filter(|kind| {
            !checks
                .iter()
                .any(|check| check.kind == *kind && check.status == MergeCheckStatus::Passed)
        })
        .collect()
}

/// Checks that were returned but that the coordinator did not require. They are
/// kept in the record (more evidence is fine) but never substitute for a
/// required check.
pub fn extra_checks(required: &[MergeCheckKind], checks: &[MergeCheck]) -> Vec<MergeCheck> {
    checks
        .iter()
        .filter(|check| !required.contains(&check.kind))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::{MergeCheck, MergeDiagnostic};
    use ravum_protocol::SnapshotId;

    fn check(kind: MergeCheckKind, status: MergeCheckStatus) -> MergeCheck {
        MergeCheck::new(
            kind,
            status,
            "rustc",
            "1.95.0",
            "host",
            SnapshotId::new("s"),
            "summary",
            Vec::<MergeDiagnostic>::new(),
        )
    }

    #[test]
    fn a_failed_required_check_is_reported_missing() {
        let required = vec![MergeCheckKind::MetadataCheck];
        assert_eq!(
            missing_required_checks(
                &required,
                &[check(
                    MergeCheckKind::MetadataCheck,
                    MergeCheckStatus::Failed
                )]
            ),
            required
        );
        assert!(missing_required_checks(
            &required,
            &[check(
                MergeCheckKind::MetadataCheck,
                MergeCheckStatus::Passed
            )]
        )
        .is_empty());
    }

    #[test]
    fn a_passing_unrelated_check_does_not_cover_a_required_one() {
        let required = vec![MergeCheckKind::MetadataCheck, MergeCheckKind::TestRun];
        let checks = vec![check(MergeCheckKind::TestCompile, MergeCheckStatus::Passed)];
        assert_eq!(
            missing_required_checks(&required, &checks),
            vec![MergeCheckKind::MetadataCheck, MergeCheckKind::TestRun]
        );
    }

    #[test]
    fn extra_checks_do_not_count_as_required() {
        let required = vec![MergeCheckKind::MetadataCheck];
        let checks = vec![
            check(MergeCheckKind::MetadataCheck, MergeCheckStatus::Passed),
            check(MergeCheckKind::TestRun, MergeCheckStatus::Passed),
        ];
        assert_eq!(extra_checks(&required, &checks).len(), 1);
    }
}
