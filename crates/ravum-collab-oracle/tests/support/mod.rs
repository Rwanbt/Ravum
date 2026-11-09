//! Shared helpers for the `ravum-collab-oracle` integration tests.
//!
//! The tests use the committed `fixtures/multicrate` workspace (core → api →
//! consumer → application, plus the features/build-script fallback units) and
//! build the before/after snapshots by applying real unified patches to the
//! fixture contents. No fixture file is modified.

#![allow(dead_code)]

use ravum_snapshot::SourceSnapshot;
use ravum_workspace_coordinator::{
    patch, MergeCheck, MergeCheckKind, MergeVerifier, OracleRequest, OracleVerdict,
    VerificationOutcome, VerificationRequest,
};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Real patches against `fixtures/multicrate`
// ---------------------------------------------------------------------------

/// A compatible, body-only change: `internal_factor` changes, every public
/// interface (and therefore every `.rmeta`) stays identical.
pub const CORE_BODY_ONLY: &str = "\
+++ b/core/src/lib.rs
@@ -20,4 +20,4 @@
 /// Private helper: a body-only change must not invalidate dependents.
 fn internal_factor() -> f64 {
-    2.0
+    3.0
 }
";

/// An interface break used by `api`: `core::scale` changes its parameter/return
/// types to `f32`. `core` is patched coherently (its own `scaled` converts), so
/// the changed unit still compiles — but the unchanged `api` calls `core::scale`
/// with an `f64` and must fail.
pub const CORE_INTERFACE_BREAK: &str = "\
+++ b/core/src/lib.rs
@@ -10,4 +10,4 @@
 /// Scales one value. Its public signature is the central L1 experiment.
-pub fn scale(x: f64) -> f64 {
-    x * 2.0
+pub fn scale(x: f32) -> f32 {
+    x * 2.0_f32
 }
@@ -25,4 +25,4 @@
 /// Calls the private helper so a body change here has no interface impact.
 pub fn scaled(x: f64) -> f64 {
-    scale(x) * internal_factor() / internal_factor()
+    scale(x as f32) as f64 * internal_factor() / internal_factor()
 }
";

/// Removal of a public symbol used by `api` (`core::add`).
pub const CORE_SYMBOL_REMOVAL: &str = "\
+++ b/core/src/lib.rs
@@ -14,5 +14,1 @@
 
-/// Adds two values (used by the \"symbol removal\" negative case).
-pub fn add(a: f64, b: f64) -> f64 {
-    a + b
-}
";

/// A body change inside the feature-bearing unit. Since MVP-0d resolves the
/// activation set, this unit is compiled by the targeted chain, so the change is
/// verifiable; it compiles and changes no interface.
pub const FEATURES_BODY_CHANGE: &str = "\
+++ b/features/src/lib.rs
@@ -9,4 +9,4 @@
 #[cfg(not(feature = \"extra\"))]
 pub fn extra_value() -> u32 {
-    0
+    1
 }
";

pub const CORE_PATH: &str = "core/src/lib.rs";
pub const FEATURES_PATH: &str = "features/src/lib.rs";
pub const BUILDSCRIPT_PATH: &str = "buildscript/src/lib.rs";

/// A change inside a unit that still compiles through a conservative Cargo
/// fallback (`buildscript` runs a build script): its interface identity is not
/// observable, so the oracle must not claim anything (fail-closed).
///
/// `features` used to play this role, but MVP-0d resolves its activation set and
/// verifies it through the targeted chain; the fallback invariant is kept on a
/// unit that genuinely still falls back.
pub const BUILDSCRIPT_FALLBACK_CHANGE: &str = "\
+++ b/buildscript/src/lib.rs
@@ -4,3 +4,3 @@
 pub fn build_marker() -> &'static str {
-    option_env!(\"RAVUM_FIXTURE_BUILD\").unwrap_or(\"missing\")
+    option_env!(\"RAVUM_FIXTURE_BUILD\").unwrap_or(\"present\")
 }
";

/// Dependents of `core` in the fixture (transitive closure).
pub const CORE_DEPENDENTS: &[&str] = &["api", "consumer", "application"];

// ---------------------------------------------------------------------------
// Snapshots, patches, requests
// ---------------------------------------------------------------------------

pub fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/multicrate")
}

pub fn fixture_snapshot() -> SourceSnapshot {
    SourceSnapshot::from_directory(&fixture_dir(), |_| true)
        .unwrap_or_else(|error| panic!("cannot read fixtures/multicrate: {error}"))
}

/// Apply a real unified patch to an in-memory snapshot.
pub fn apply_patch(before: &SourceSnapshot, patch_text: &str) -> SourceSnapshot {
    let files = patch::parse(patch_text).expect("fixture patch parses");
    let mut overlays = Vec::new();
    for file in files {
        let original = before
            .content_of(&file.path)
            .unwrap_or_else(|| panic!("snapshot has no {}", file.path));
        let applied = patch::apply(&file, original)
            .unwrap_or_else(|error| panic!("patch does not apply to {}: {error}", file.path));
        overlays.push((file.path.clone(), applied));
    }
    before.with_overlay(overlays)
}

#[allow(clippy::too_many_arguments)]
pub fn request(
    before: SourceSnapshot,
    after: SourceSnapshot,
    patch_text: &str,
    changed_paths: &[&str],
    declared_dependencies: &[&str],
) -> OracleRequest {
    OracleRequest {
        proposal_id: "agent-a/test".to_owned(),
        agent_id: "agent-a".to_owned(),
        snapshot_before: before,
        snapshot_after: after,
        changed_paths: changed_paths
            .iter()
            .map(|path| (*path).to_owned())
            .collect(),
        declared_symbols: Vec::new(),
        declared_dependencies: declared_dependencies
            .iter()
            .map(|path| (*path).to_owned())
            .collect(),
        impacted_contracts: Vec::new(),
        unified_patch: patch_text.to_owned(),
        required_checks: vec![MergeCheckKind::MetadataCheck],
    }
}

pub fn body_only_request(declared_dependencies: &[&str]) -> OracleRequest {
    let before = fixture_snapshot();
    let after = apply_patch(&before, CORE_BODY_ONLY);
    request(
        before,
        after,
        CORE_BODY_ONLY,
        &[CORE_PATH],
        declared_dependencies,
    )
}

pub fn interface_break_request() -> OracleRequest {
    let before = fixture_snapshot();
    let after = apply_patch(&before, CORE_INTERFACE_BREAK);
    request(
        before,
        after,
        CORE_INTERFACE_BREAK,
        &[CORE_PATH],
        &["api", "consumer", "application"],
    )
}

pub fn state_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/ravum-tests/collab-oracle")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

// ---------------------------------------------------------------------------
// Verdict inspection
// ---------------------------------------------------------------------------

pub fn findings(verdict: &OracleVerdict) -> Vec<&ravum_workspace_coordinator::OracleFinding> {
    match verdict {
        OracleVerdict::Findings(findings) => findings.iter().collect(),
        OracleVerdict::Clear { .. } | OracleVerdict::Unavailable { .. } => Vec::new(),
    }
}

pub fn has_kind(
    verdict: &OracleVerdict,
    kind: ravum_workspace_coordinator::OracleFindingKind,
) -> bool {
    findings(verdict).iter().any(|finding| finding.kind == kind)
}

pub fn blocking(verdict: &OracleVerdict) -> Vec<&ravum_workspace_coordinator::OracleFinding> {
    findings(verdict)
        .into_iter()
        .filter(|finding| finding.is_blocking())
        .collect()
}

pub fn unavailable_reason(verdict: &OracleVerdict) -> Option<&str> {
    match verdict {
        OracleVerdict::Unavailable { reason } => Some(reason.as_str()),
        _ => None,
    }
}

/// A declared verifier for the end-to-end v2 test: it "passes" whatever checks the
/// coordinator requires. It is a test double for the compiler, not a proof.
pub struct PassVerifier;

impl MergeVerifier for PassVerifier {
    fn name(&self) -> &'static str {
        "pass-verifier"
    }

    fn verify(&self, request: &VerificationRequest) -> VerificationOutcome {
        VerificationOutcome::Passed {
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
                        "simulated pass",
                    )
                })
                .collect(),
        }
    }
}
