//! Block-processing host (M3): real-time entry point kept lock-free and allocation-free.

use crate::host::{evaluate_gates, validate_loaded, ExpectedAbi};
use crate::reclaimer::{live_modules, unloaded_modules, DeferredLibrary};
use crate::slot::ArcSlot;
use ravum_protocol::{
    ActiveVersionId, ActivationReport, ActivationStatus, ArtifactId, BuildOutput, DspState,
    EvidenceRecord, HostStatus, PatchManifest, RejectReason, SnapshotId, StageTimings,
    VerificationStatus, DSP_EXPORT, PATCH_SCHEMA_VERSION,
};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

/// Signature of the block entry point.
///
/// # Safety
/// The module must not retain `input`/`output`/`state` beyond the call, must write
/// exactly `frames` samples, and must not allocate or block.
pub type BlockFn =
    unsafe extern "C" fn(input: *const f32, output: *mut f32, frames: u32, state: *mut DspState);

/// One loaded block module. Alive while at least one `Arc` reference exists.
pub struct BlockModule {
    process: BlockFn,
    pub manifest: PatchManifest,
    pub artifact_id: ArtifactId,
    _lib: DeferredLibrary,
}

impl BlockModule {
    /// Process one block in place. Returns the number of frames processed.
    ///
    /// This is the only function called from the audio thread: no lock, no
    /// allocation, no formatting, no `Drop` of native code.
    pub fn process_block(&self, input: &[f32], output: &mut [f32], state: &mut DspState) -> usize {
        let frames = input.len().min(output.len());
        if frames == 0 {
            return 0;
        }
        // SAFETY: slices are valid for `frames`, state is a valid `&mut DspState`,
        // and the module is kept loaded by `Arc<Self>` for the whole call.
        unsafe {
            (self.process)(
                input.as_ptr(),
                output.as_mut_ptr(),
                frames as u32,
                state as *mut DspState,
            )
        };
        frames
    }

    pub fn path(&self) -> &Path {
        self._lib.path()
    }
}

/// A validated block module, loaded but not yet published.
pub struct PreparedBlock {
    pub module: Arc<BlockModule>,
    pub load_ms: u64,
    pub abi_version: u32,
}

impl std::fmt::Debug for PreparedBlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedBlock")
            .field("artifact_id", &self.module.artifact_id)
            .field("patch_id", &self.module.manifest.patch_id)
            .field("load_ms", &self.load_ms)
            .finish()
    }
}

/// Load and validate a block module. Never activates.
pub fn prepare_block(path: &Path, expected: &ExpectedAbi) -> Result<PreparedBlock, RejectReason> {
    let started = Instant::now();
    let lib = DeferredLibrary::open(path).map_err(|detail| RejectReason::LoadFailed { detail })?;
    let validated = validate_loaded(lib, path, expected)?;
    let process: BlockFn = unsafe { validated.lib.symbol(b"ravum_process_block\0") }.map_err(|_| {
        RejectReason::MissingExport {
            symbol: DSP_EXPORT.to_owned(),
        }
    })?;
    Ok(PreparedBlock {
        module: Arc::new(BlockModule {
            process,
            manifest: validated.manifest,
            artifact_id: validated.artifact_id,
            _lib: validated.lib,
        }),
        load_ms: started.elapsed().as_millis() as u64,
        abi_version: validated.abi_version,
    })
}

/// Publishes block modules without ever locking the audio path.
pub struct BlockHost {
    expected: ExpectedAbi,
    slot: ArcSlot<BlockModule>,
    writer: Mutex<()>,
    verification: RwLock<VerificationStatus>,
    generation: AtomicU64,
    activations: AtomicU64,
    rejections: AtomicU64,
}

impl BlockHost {
    /// First activation of a fresh process: ABI validated, no verification evidence
    /// yet, so the status is `Assumed`.
    pub fn bootstrap(expected: ExpectedAbi, path: &Path) -> Result<Self, RejectReason> {
        let prepared = prepare_block(path, &expected)?;
        Ok(Self {
            expected,
            slot: ArcSlot::new(prepared.module),
            writer: Mutex::new(()),
            verification: RwLock::new(VerificationStatus::Assumed),
            generation: AtomicU64::new(1),
            activations: AtomicU64::new(0),
            rejections: AtomicU64::new(0),
        })
    }

    pub fn expected_abi(&self) -> &ExpectedAbi {
        &self.expected
    }

    /// Publish a candidate if every gate passes. The audio path is never blocked:
    /// the swap is one atomic pointer exchange.
    pub fn apply(
        &self,
        prepared: PreparedBlock,
        build: &BuildOutput,
        evidence: Option<&EvidenceRecord>,
        current_snapshot: &SnapshotId,
    ) -> ActivationReport {
        let mut timings = StageTimings {
            build_ms: build.build_ms,
            load_ms: prepared.load_ms,
            ..StageTimings::default()
        };

        let verdict = match evaluate_gates(
            build,
            &prepared.module.artifact_id,
            evidence,
            current_snapshot,
        ) {
            Ok(status) => status,
            Err(reason) => return self.reject(reason, timings),
        };

        let activate_started = Instant::now();
        {
            let _writer = self.writer.lock().unwrap_or_else(|e| e.into_inner());
            let active = self.slot.load();
            if active.artifact_id == prepared.module.artifact_id {
                drop(active);
                drop(_writer);
                return self.reject(
                    RejectReason::AlreadyActive {
                        artifact_id: prepared.module.artifact_id.clone(),
                    },
                    timings,
                );
            }
            // Publish, then release the retired module. Blocks already in flight keep
            // it alive through their own `Arc`; no code is unmapped mid-block.
            let retired = self.slot.store(Arc::clone(&prepared.module));
            drop(retired);
            drop(active);
        }
        timings.activate_ms = activate_started.elapsed().as_millis() as u64;
        timings.total_ms = timings.build_ms.saturating_add(timings.load_ms).saturating_add(timings.activate_ms);

        let new_version = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.activations.fetch_add(1, Ordering::SeqCst);
        *self.verification.write().unwrap_or_else(|e| e.into_inner()) = verdict;

        ActivationReport {
            schema_version: PATCH_SCHEMA_VERSION,
            status: ActivationStatus::Activated,
            reject_reason: None,
            active_version: Some(ActiveVersionId(new_version)),
            previous_version: Some(ActiveVersionId(new_version - 1)),
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

    /// Audio-thread entry point: lock-free, allocation-free.
    pub fn process_rt(&self, input: &[f32], output: &mut [f32], state: &mut DspState) -> usize {
        let module = self.slot.load();
        module.process_block(input, output, state)
    }

    /// Enter a block explicitly and keep the module alive for its duration.
    /// Used by tests and by callers that need to measure the block themselves.
    pub fn enter(&self) -> Arc<BlockModule> {
        self.slot.load()
    }

    pub fn active_version(&self) -> ActiveVersionId {
        ActiveVersionId(self.generation.load(Ordering::SeqCst))
    }

    pub fn active_artifact(&self) -> ArtifactId {
        self.slot.load().artifact_id.clone()
    }

    pub fn active_patch_id(&self) -> String {
        self.slot.load().manifest.patch_id.clone()
    }

    pub fn active_verification(&self) -> VerificationStatus {
        *self.verification.read().unwrap_or_else(|e| e.into_inner())
    }

    pub fn status(&self) -> HostStatus {
        let module = self.slot.load();
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
