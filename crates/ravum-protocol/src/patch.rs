//! Patch/build/activation protocol for the M1 continuous loop.
//!
//! Wire compatibility is versioned by `PATCH_SCHEMA_VERSION` (JSON documents) and
//! `PATCH_ABI_VERSION` (native module ABI). Both start at 1 and are checked, never
//! assumed.

use crate::diagnostics::DiagnosticSet;
use crate::ids::{ActiveVersionId, ArtifactId, BuildContextId, SnapshotId};
use serde::{Deserialize, Serialize};

/// Version of the JSON documents exchanged between CLI, adapter and demo host.
pub const PATCH_SCHEMA_VERSION: u32 = 1;

/// Native ABI version exported by every patchable module.
///
/// Bumping it invalidates every previously built module on purpose: a host that
/// expects 2 must refuse a module exporting 1.
pub const PATCH_ABI_VERSION: u32 = 1;

/// Symbol the host resolves as the patchable computation.
pub const COMPUTE_EXPORT: &str = "ravum_compute";
/// Fast ABI gate queried before any other symbol is trusted.
pub const ABI_VERSION_EXPORT: &str = "ravum_abi_version";
/// NUL-terminated JSON manifest exported by the artifact itself.
pub const MANIFEST_EXPORT: &str = "ravum_manifest_v1";
/// Logical signature of `COMPUTE_EXPORT`: one f64 argument, one f64 result.
pub const COMPUTE_SIGNATURE: &str = "f64->f64";

/// Manifest embedded in the artifact and validated by the host *after* loading.
///
/// It describes the module, never the source that produced it: staleness is a
/// property of a build result, not of artifact bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatchManifest {
    pub schema_version: u32,
    pub patch_id: String,
    pub export: String,
    pub signature: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
}

impl PatchManifest {
    pub fn new(patch_id: impl Into<String>, author: Option<String>) -> Self {
        Self {
            schema_version: PATCH_SCHEMA_VERSION,
            patch_id: patch_id.into(),
            export: COMPUTE_EXPORT.to_owned(),
            signature: COMPUTE_SIGNATURE.to_owned(),
            author,
        }
    }
}

/// Result of one `rustc` invocation, with the five-identity triple attached.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildOutput {
    pub schema_version: u32,
    pub artifact_path: String,
    pub artifact_id: ArtifactId,
    pub source_snapshot_id: SnapshotId,
    pub build_context_id: BuildContextId,
    pub tool: String,
    pub tool_version: String,
    pub target: String,
    pub flags: Vec<String>,
    pub build_ms: u64,
    /// True when the artifact was reused from the on-disk build cache instead of
    /// invoking `rustc` again. Never implies that analysis was skipped.
    #[serde(default)]
    pub cached_build: bool,
    pub diagnostics: DiagnosticSet,
}

impl BuildOutput {
    pub fn succeeded(&self) -> bool {
        !self.diagnostics.has_errors()
    }

    /// A build result is usable only for the snapshot it was produced from.
    pub fn is_current_for(&self, current: &SnapshotId) -> bool {
        &self.source_snapshot_id == current
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RejectReason {
    /// Artifact declares another native ABI version.
    AbiMismatch { expected: u32, found: u32 },
    /// Required exported symbol is absent.
    MissingExport { symbol: String },
    /// Manifest JSON absent, unparsable or schema-versioned differently.
    ManifestInvalid { detail: String },
    /// Manifest declares another logical signature than the host supports.
    SignatureMismatch { expected: String, found: String },
    /// The build was produced from a snapshot that is no longer current.
    StaleBuild { built_from: SnapshotId, expected: SnapshotId },
    /// The operating system refused to load the module.
    LoadFailed { detail: String },
    /// No version is active yet, so "keep the previous version" is impossible.
    NoActiveVersion,
    /// The module is already the active one.
    AlreadyActive { artifact_id: ArtifactId },
    /// Artifact bytes do not match the `ArtifactId` recorded by the build.
    ArtifactMismatch { expected: ArtifactId, found: ArtifactId },
    /// The required verification did not reach a passing `tested` status.
    NotVerified { verification_status: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationStatus {
    Activated,
    Rejected,
}

/// Timing of one activation attempt, in milliseconds, stages measured separately.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageTimings {
    /// `rustc --emit=metadata` (analysis only).
    pub check_ms: u64,
    /// `rustc --crate-type=cdylib` (codegen + link, not separable through the public CLI).
    pub build_ms: u64,
    /// Dynamic loader call (`LoadLibraryW` / `dlopen`).
    pub load_ms: u64,
    /// Atomic publication of the new entry point.
    pub activate_ms: u64,
    /// Contract smoke test executed on the candidate artifact.
    pub verify_ms: u64,
    /// Obligation checking through the proof broker (may be served from cache).
    #[serde(default)]
    pub proof_ms: u64,
    /// Sum of every stage above.
    pub total_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivationReport {
    pub schema_version: u32,
    pub status: ActivationStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reject_reason: Option<RejectReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_version: Option<ActiveVersionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_version: Option<ActiveVersionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<ArtifactId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch_id: Option<String>,
    pub timings: StageTimings,
}

impl ActivationReport {
    pub fn rejected(reason: RejectReason) -> Self {
        Self {
            schema_version: PATCH_SCHEMA_VERSION,
            status: ActivationStatus::Rejected,
            reject_reason: Some(reason),
            active_version: None,
            previous_version: None,
            artifact_id: None,
            patch_id: None,
            timings: StageTimings::default(),
        }
    }
}

/// Observable state of a running host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostStatus {
    pub schema_version: u32,
    pub pid: u32,
    pub active_version: ActiveVersionId,
    pub artifact_id: ArtifactId,
    pub patch_id: String,
    pub activations: u64,
    /// Number of native modules freed by the reclaimer thread.
    pub unloaded_modules: u64,
    /// Modules currently alive (active + in-flight + awaiting reclamation).
    pub live_modules: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_output_rejects_other_snapshot() {
        let diags = DiagnosticSet::new(SnapshotId::new("s1"), "rustc", "1.95.0");
        let out = BuildOutput {
            schema_version: PATCH_SCHEMA_VERSION,
            artifact_path: "a.dll".into(),
            artifact_id: ArtifactId::new("aid"),
            source_snapshot_id: SnapshotId::new("s1"),
            build_context_id: BuildContextId::new("bc"),
            tool: "rustc".into(),
            tool_version: "1.95.0".into(),
            target: "x86_64-pc-windows-msvc".into(),
            flags: vec![],
            build_ms: 1,
            cached_build: false,
            diagnostics: diags,
        };
        assert!(out.is_current_for(&SnapshotId::new("s1")));
        assert!(!out.is_current_for(&SnapshotId::new("s2")));
        assert!(out.succeeded());
    }

    #[test]
    fn reject_reason_is_serializable_with_tag() {
        let json = serde_json::to_value(RejectReason::AbiMismatch { expected: 1, found: 2 }).unwrap();
        assert_eq!(json["kind"], "abi_mismatch");
        assert_eq!(json["found"], 2);
    }
}
