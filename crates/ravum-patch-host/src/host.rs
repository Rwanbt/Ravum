//! Versioned patch host: validate, stage, atomically publish, never unload in-flight code.

use crate::reclaimer::{live_modules, unloaded_modules, DeferredLibrary};
use ravum_protocol::{
    sha256_hex, ActiveVersionId, ActivationReport, ActivationStatus, ArtifactId, BuildOutput,
    EvidenceRecord, HostStatus, PatchManifest, RejectReason, SnapshotId, StageTimings,
    VerificationStatus, ABI_VERSION_EXPORT, COMPUTE_EXPORT, COMPUTE_SIGNATURE, MANIFEST_EXPORT,
    PATCH_ABI_VERSION, PATCH_SCHEMA_VERSION,
};
use std::ffi::CStr;
use std::os::raw::c_char;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;

pub type ComputeFn = unsafe extern "C" fn(f64) -> f64;

/// The ABI contract a host is willing to activate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedAbi {
    pub abi_version: u32,
    pub export: String,
    pub signature: String,
}

impl Default for ExpectedAbi {
    fn default() -> Self {
        Self {
            abi_version: PATCH_ABI_VERSION,
            export: COMPUTE_EXPORT.to_owned(),
            signature: COMPUTE_SIGNATURE.to_owned(),
        }
    }
}

/// Facts read out of a native artifact, before it is trusted for anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactFacts {
    pub artifact_id: ArtifactId,
    pub abi_version: u32,
    pub manifest: PatchManifest,
}

/// One loaded module, alive exactly as long as at least one `Arc` reference exists.
pub struct LoadedModule {
    // SAFETY: `compute` points into `lib`. `lib` is released only in `Drop`,
    // after which no reference to this module exists, so the pointer is never
    // called after unload.
    compute: ComputeFn,
    pub manifest: PatchManifest,
    pub artifact_id: ArtifactId,
    _lib: DeferredLibrary,
}

impl LoadedModule {
    pub fn call(&self, x: f64) -> f64 {
        // SAFETY: signature validated by the host against `ExpectedAbi`.
        unsafe { (self.compute)(x) }
    }

    pub fn path(&self) -> &Path {
        self._lib.path()
    }
}

/// A validated module that has been loaded but not yet published.
pub struct PreparedPatch {
    pub module: Arc<LoadedModule>,
    pub load_ms: u64,
    /// ABI version the artifact itself declared (equal to the expected one).
    pub abi_version: u32,
}

impl std::fmt::Debug for PreparedPatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedPatch")
            .field("artifact_id", &self.module.artifact_id)
            .field("patch_id", &self.module.manifest.patch_id)
            .field("load_ms", &self.load_ms)
            .finish()
    }
}

/// Read the artifact's own manifest and ABI tag, without publishing anything.
pub fn inspect_artifact(
    path: &Path,
    expected: &ExpectedAbi,
) -> Result<ArtifactFacts, RejectReason> {
    let prepared = prepare(path, expected)?;
    Ok(ArtifactFacts {
        artifact_id: prepared.module.artifact_id.clone(),
        abi_version: prepared.abi_version,
        manifest: prepared.module.manifest.clone(),
    })
}

/// Load and fully validate a candidate artifact. Never activates.
///
/// Rejection drops the `DeferredLibrary`, so the OS handle is released by the
/// reclaimer thread and no partial state is left behind.
pub fn prepare(path: &Path, expected: &ExpectedAbi) -> Result<PreparedPatch, RejectReason> {
    let started = Instant::now();
    let lib = DeferredLibrary::open(path).map_err(|detail| RejectReason::LoadFailed { detail })?;

    // 1) Cheapest gate first: the module's declared ABI version.
    let abi_fn: unsafe extern "C" fn() -> u32 = unsafe { lib.symbol(b"ravum_abi_version\0") }
        .map_err(|_| RejectReason::MissingExport {
            symbol: ABI_VERSION_EXPORT.to_owned(),
        })?;
    let abi_version = unsafe { abi_fn() };
    if abi_version != expected.abi_version {
        return Err(RejectReason::AbiMismatch {
            expected: expected.abi_version,
            found: abi_version,
        });
    }

    // 2) The module describes itself; the host decides whether that description is acceptable.
    let manifest_fn: unsafe extern "C" fn() -> *const c_char =
        unsafe { lib.symbol(b"ravum_manifest_v1\0") }.map_err(|_| RejectReason::MissingExport {
            symbol: MANIFEST_EXPORT.to_owned(),
        })?;
    let raw = unsafe { manifest_fn() };
    if raw.is_null() {
        return Err(RejectReason::ManifestInvalid {
            detail: "ravum_manifest_v1 returned a null pointer".to_owned(),
        });
    }
    let text = unsafe { CStr::from_ptr(raw) }
        .to_str()
        .map_err(|e| RejectReason::ManifestInvalid {
            detail: format!("manifest is not UTF-8: {e}"),
        })?;
    let manifest: PatchManifest =
        serde_json::from_str(text).map_err(|e| RejectReason::ManifestInvalid {
            detail: format!("manifest is not valid JSON: {e}"),
        })?;
    if manifest.schema_version != PATCH_SCHEMA_VERSION {
        return Err(RejectReason::ManifestInvalid {
            detail: format!(
                "manifest schema_version {} != {PATCH_SCHEMA_VERSION}",
                manifest.schema_version
            ),
        });
    }
    if manifest.export != expected.export {
        return Err(RejectReason::ManifestInvalid {
            detail: format!(
                "manifest declares export {:?}, host requires {:?}",
                manifest.export, expected.export
            ),
        });
    }
    if manifest.signature != expected.signature {
        return Err(RejectReason::SignatureMismatch {
            expected: expected.signature.clone(),
            found: manifest.signature.clone(),
        });
    }

    // 3) Only now resolve the callable symbol.
    let compute: ComputeFn = unsafe { lib.symbol(b"ravum_compute\0") }.map_err(|_| {
        RejectReason::MissingExport {
            symbol: COMPUTE_EXPORT.to_owned(),
        }
    })?;

    let bytes = std::fs::read(path).map_err(|e| RejectReason::LoadFailed {
        detail: format!("cannot hash artifact: {e}"),
    })?;
    let artifact_id = ArtifactId::new(sha256_hex(&bytes));

    Ok(PreparedPatch {
        module: Arc::new(LoadedModule {
            compute,
            manifest,
            artifact_id,
            _lib: lib,
        }),
        load_ms: started.elapsed().as_millis() as u64,
        abi_version,
    })
}

/// Holds the currently published module and swaps it atomically.
pub struct PatchHost {
    expected: ExpectedAbi,
    slot: RwLock<Arc<LoadedModule>>,
    verification: RwLock<VerificationStatus>,
    generation: AtomicU64,
    activations: AtomicU64,
    rejections: AtomicU64,
}

impl PatchHost {
    /// First activation of a fresh process.
    ///
    /// ABI and manifest are validated exactly like any candidate, but no
    /// verification evidence exists yet, so the status is recorded as
    /// `Assumed` — never as `Tested` or `Proved`.
    pub fn bootstrap(expected: ExpectedAbi, path: &Path) -> Result<Self, RejectReason> {
        let prepared = prepare(path, &expected)?;
        let host = Self {
            expected,
            slot: RwLock::new(prepared.module),
            verification: RwLock::new(VerificationStatus::Assumed),
            generation: AtomicU64::new(1),
            activations: AtomicU64::new(0),
            rejections: AtomicU64::new(0),
        };
        Ok(host)
    }

    pub fn expected_abi(&self) -> &ExpectedAbi {
        &self.expected
    }

    /// Publish a candidate if, and only if, every gate passes.
    ///
    /// Gates, in order: snapshot freshness, verification evidence, redundancy.
    /// Any failure returns `Rejected` and leaves the active version untouched.
    pub fn apply(
        &self,
        prepared: PreparedPatch,
        build: &BuildOutput,
        evidence: Option<&EvidenceRecord>,
        current_snapshot: &SnapshotId,
    ) -> ActivationReport {
        let mut timings = StageTimings {
            build_ms: build.build_ms,
            load_ms: prepared.load_ms,
            ..StageTimings::default()
        };

        if !build.is_current_for(current_snapshot) {
            return self.reject(
                RejectReason::StaleBuild {
                    built_from: build.source_snapshot_id.clone(),
                    expected: current_snapshot.clone(),
                },
                timings,
            );
        }

        let verdict = evidence
            .filter(|e| {
                e.applies_to(
                    &build.source_snapshot_id,
                    &build.build_context_id,
                    &prepared.module.artifact_id,
                )
            })
            .map(|e| (e.status, e.authorizes_activation()))
            .unwrap_or((VerificationStatus::Unknown, false));
        if !verdict.1 {
            return self.reject(
                RejectReason::NotVerified {
                    verification_status: verdict.0.as_str().to_owned(),
                },
                timings,
            );
        }

        let activate_started = Instant::now();
        {
            let mut slot = self.slot.write().unwrap_or_else(|e| e.into_inner());
            if slot.artifact_id == prepared.module.artifact_id {
                drop(slot);
                return self.reject(
                    RejectReason::AlreadyActive {
                        artifact_id: prepared.module.artifact_id.clone(),
                    },
                    timings,
                );
            }
            let previous = Arc::clone(&slot);
            *slot = Arc::clone(&prepared.module);
            drop(slot);
            // `previous` (and the caller's captured clones) keep the old module
            // loaded until the last in-flight call returns.
            drop(previous);
        }
        timings.activate_ms = activate_started.elapsed().as_millis() as u64;

        let new_version = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let previous_version = ActiveVersionId(new_version - 1);
        self.activations.fetch_add(1, Ordering::SeqCst);
        *self.verification.write().unwrap_or_else(|e| e.into_inner()) = verdict.0;
        timings.total_ms = timings
            .check_ms
            .saturating_add(timings.build_ms)
            .saturating_add(timings.load_ms)
            .saturating_add(timings.activate_ms)
            .saturating_add(timings.verify_ms);

        ActivationReport {
            schema_version: PATCH_SCHEMA_VERSION,
            status: ActivationStatus::Activated,
            reject_reason: None,
            active_version: Some(ActiveVersionId(new_version)),
            previous_version: Some(previous_version),
            artifact_id: Some(prepared.module.artifact_id.clone()),
            patch_id: Some(prepared.module.manifest.patch_id.clone()),
            timings,
        }
    }

    fn reject(&self, reason: RejectReason, timings: StageTimings) -> ActivationReport {
        self.rejections.fetch_add(1, Ordering::SeqCst);
        let mut report = ActivationReport::rejected(reason);
        report.timings = timings;
        report.active_version = Some(self.active_version());
        report
    }

    /// Clone the current module and release the lock *before* calling it, so a
    /// long-running call never blocks an activation.
    pub fn call(&self, x: f64) -> f64 {
        let module = {
            let slot = self.slot.read().unwrap_or_else(|e| e.into_inner());
            Arc::clone(&slot)
        };
        module.call(x)
    }

    /// Enter a call and return the guard; the module stays loaded while it lives.
    pub fn enter(&self) -> Arc<LoadedModule> {
        let slot = self.slot.read().unwrap_or_else(|e| e.into_inner());
        Arc::clone(&slot)
    }

    pub fn active_version(&self) -> ActiveVersionId {
        ActiveVersionId(self.generation.load(Ordering::SeqCst))
    }

    pub fn active_artifact(&self) -> ArtifactId {
        self.slot
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .artifact_id
            .clone()
    }

    pub fn active_verification(&self) -> VerificationStatus {
        *self.verification.read().unwrap_or_else(|e| e.into_inner())
    }

    pub fn status(&self) -> HostStatus {
        let module = self.slot.read().unwrap_or_else(|e| e.into_inner());
        HostStatus {
            schema_version: PATCH_SCHEMA_VERSION,
            pid: std::process::id(),
            active_version: self.active_version(),
            artifact_id: module.artifact_id.clone(),
            patch_id: module.manifest.patch_id.clone(),
            activations: self.activations.load(Ordering::SeqCst),
            unloaded_modules: unloaded_modules(),
            live_modules: live_modules(),
        }
    }
}
