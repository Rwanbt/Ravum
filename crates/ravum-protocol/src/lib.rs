//! Ravum machine protocol — the shared, versioned contract between the CLI, the
//! Rust adapter, the patch host and the workspace coordinator.
//!
//! Constraints enforced by construction:
//! * no dependency on `rustc_private` or any private compiler API (INV-11);
//! * the five identities are distinct types (M0 / RAV-005);
//! * every verification verdict carries a status that cannot be upgraded by
//!   convenience (INV-01).
//!
//! Status: experimental MVP-0a contract. `PATCH_SCHEMA_VERSION == 1` is not a
//! published ABI and may change without notice until M1 is qualified.

pub mod collab;
pub mod diagnostics;
pub mod dsp;
pub mod evidence;
pub mod ids;
pub mod patch;

pub use collab::{
    ChangeProposal, DecisionKind, JournalEntry, ProposalDecision, COLLAB_SCHEMA_VERSION,
};
pub use diagnostics::{
    Diagnostic, DiagnosticSet, DiagnosticSpan, Severity, DIAGNOSTICS_SCHEMA_VERSION,
};
pub use dsp::{
    DspBlockTimings, DspSessionReport, DspState, DSP_ABI_VERSION, DSP_EXPORT, DSP_SIGNATURE,
};
pub use evidence::{EvidenceRecord, VerificationStatus, EVIDENCE_SCHEMA_VERSION};
pub use ids::{ActiveVersionId, ArtifactId, BuildContextId, SnapshotId, VerificationKey};
pub use patch::{
    ActivationReport, ActivationStatus, BuildOutput, HostStatus, PatchManifest, RejectReason,
    StageTimings, ABI_VERSION_EXPORT, COMPUTE_EXPORT, COMPUTE_SIGNATURE, MANIFEST_EXPORT,
    PATCH_ABI_VERSION, PATCH_SCHEMA_VERSION,
};

/// SHA-256 of `bytes`, lowercase hex. Used for every content identity.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Unix time in seconds; recorded in every decision and evidence entry.
pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
