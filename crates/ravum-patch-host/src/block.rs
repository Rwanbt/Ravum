//! Block-processing host (M3), hardened and crossfaded for MVP-0b.
//!
//! Two independent improvements over MVP-0a:
//!
//! 1. **Real-time lifecycle** — the audio path enters a non-owning [`ReadGuard`] on the
//!    published program instead of taking an owning `Arc`; the callback can therefore
//!    never be the last holder of a retired module (see `slot.rs` and
//!    `docs/mvp/MVP-0b-rt-lifecycle.md`).
//! 2. **Optional swap crossfade** — with a non-zero crossfade length, a publication
//!    keeps the previous module alive for a bounded number of frames and the host mixes
//!    `old*(1-w) + new*w`, `w` ramping from 0 to 1. The measured clicks of MVP-0a
//!    (0.108 for an IIR coefficient change, 0.657 for a gain doubling) are reduced
//!    without adding any allocation to the callback: the fade buffer is a fixed-size
//!    stack array and the fade position is one atomic.
//!
//! Crossfade limits, stated honestly: during the fade a block intentionally contains
//! both versions (bounded mixing, not an atomic switch); the state handed to the old
//! module is a stack copy of the host state taken at the start of the block, so an
//! incompatible state *layout* is still refused (the ABI gate is unchanged) and the old
//! module's state trajectory is discarded at the end of the fade. The fade counter is
//! shared by the host, so concurrent callback threads shorten the fade; there is no
//! memory-safety consequence and a real host has one callback thread.

use crate::host::{evaluate_gates, validate_loaded, ExpectedAbi};
use crate::reclaimer::{init_reclaimer, live_modules, unloaded_modules, DeferredLibrary};
use crate::slot::ArcSlot;
use ravum_protocol::{
    ActiveVersionId, ActivationReport, ActivationStatus, ArtifactId, BuildOutput, DspState,
    EvidenceRecord, HostStatus, PatchManifest, RejectReason, SnapshotId, StageTimings,
    VerificationStatus, DSP_EXPORT, PATCH_SCHEMA_VERSION,
};
use std::cell::Cell;
use std::path::Path;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

/// Largest fade chunk processed in one callback call. The fade itself may be longer (it
/// is spread over several blocks); this bounds the stack buffer, so the audio path never
/// allocates for the crossfade.
pub const MAX_FADE_FRAMES: usize = 512;

/// Default crossfade length in frames: 5.3 ms at 48 kHz.
pub const DEFAULT_CROSSFADE_FRAMES: u32 = 256;

/// Per audio-thread crossfade state.
///
/// The state of the module being faded out has to stay continuous across blocks,
/// otherwise feeding it the *new* module's state each block makes its own output jump
/// (measured: 0.112 instead of the expected smooth ramp). Keeping that state per thread
/// is both correct for a real host (one callback thread) and sound for a host shared by
/// several readers: each thread renders its own fade, nothing is shared mutably, and the
/// type is `Copy` so the thread-local cell needs no allocation and no destructor.
#[derive(Clone, Copy)]
struct FadeThread {
    epoch: u64,
    remaining: u32,
    state: DspState,
}

thread_local! {
    /// `None` before the first fade; const-initialized, no allocation, no `Drop`.
    static FADE_STATE: Cell<Option<FadeThread>> = const { Cell::new(None) };
}

/// Process-wide publication sequence. Per-thread fade state is keyed on it, so a state
/// left by a previous host or a recycled generation can never be mistaken for the
/// current fade (observed in test: two hosts both published generation 2 and a thread
/// reused a finished fade, silently skipping the ramp).
static PUBLICATION_SEQ: AtomicU64 = AtomicU64::new(0);

fn next_fade_epoch() -> u64 {
    PUBLICATION_SEQ.fetch_add(1, Ordering::SeqCst) + 1
}

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

/// One published program: the active module and, while a crossfade is in progress, the
/// module it is replacing. The previous module is kept in this payload so it cannot be
/// unloaded during the fade; it is released with the payload on a control thread.
pub struct BlockProgram {
    pub current: Arc<BlockModule>,
    pub previous: Option<Arc<BlockModule>>,
    /// Fade length in frames (`0` = immediate switch, the MVP-0a behaviour).
    pub fade_total: u32,
    /// Identity of this publication, used by each audio thread to reset its own fade.
    pub fade_epoch: u64,
    fade_rendered: AtomicU32,
}

impl BlockProgram {
    fn new(
        current: Arc<BlockModule>,
        previous: Option<Arc<BlockModule>>,
        fade_frames: u32,
        fade_epoch: u64,
    ) -> Self {
        Self {
            current,
            previous,
            fade_total: fade_frames,
            fade_epoch,
            fade_rendered: AtomicU32::new(0),
        }
    }

    /// Frames of fade already rendered (observability; the per-thread progress is what
    /// the audio path actually uses).
    pub fn fade_rendered(&self) -> u32 {
        self.fade_rendered.load(Ordering::SeqCst)
    }

    fn record_fade_progress(&self, frames: usize) {
        self.fade_rendered.fetch_add(frames as u32, Ordering::SeqCst);
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

/// Publishes block programs without ever locking the audio path.
pub struct BlockHost {
    expected: ExpectedAbi,
    slot: ArcSlot<BlockProgram>,
    writer: Mutex<()>,
    verification: RwLock<VerificationStatus>,
    crossfade_frames: u32,
    generation: AtomicU64,
    activations: AtomicU64,
    rejections: AtomicU64,
}

impl BlockHost {
    /// First activation of a fresh process: ABI validated, no verification evidence
    /// yet, so the status is `Assumed`. No crossfade is armed for the first module.
    pub fn bootstrap(expected: ExpectedAbi, path: &Path) -> Result<Self, RejectReason> {
        Self::bootstrap_with_crossfade(expected, path, 0)
    }

    /// Same as [`BlockHost::bootstrap`] but arms a crossfade of `frames` for later
    /// publications (`0` = immediate switch).
    pub fn bootstrap_with_crossfade(
        expected: ExpectedAbi,
        path: &Path,
        frames: u32,
    ) -> Result<Self, RejectReason> {
        // Start the reclaimer before any library can be dropped, so no `Drop` path can
        // ever spawn a thread (that would be an unbounded operation on whichever thread
        // drops the first library).
        init_reclaimer();
        let prepared = prepare_block(path, &expected)?;
        Ok(Self {
            expected,
            slot: ArcSlot::new(Arc::new(BlockProgram::new(prepared.module, None, 0, 1))),
            writer: Mutex::new(()),
            verification: RwLock::new(VerificationStatus::Assumed),
            crossfade_frames: frames,
            generation: AtomicU64::new(1),
            activations: AtomicU64::new(0),
            rejections: AtomicU64::new(0),
        })
    }

    pub fn expected_abi(&self) -> &ExpectedAbi {
        &self.expected
    }

    /// Fade length armed for the next publications, in frames.
    pub fn crossfade_frames(&self) -> u32 {
        self.crossfade_frames
    }

    /// Publish a candidate if every gate passes. The audio path is never blocked:
    /// the swap is one atomic pointer exchange, and the crossfade (if enabled) is
    /// rendered from the published payload without any allocation.
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
        let new_version;
        {
            let _writer = self.writer.lock().unwrap_or_else(|e| e.into_inner());
            let active = self.slot.load_owning();
            if active.current.artifact_id == prepared.module.artifact_id {
                drop(active);
                drop(_writer);
                return self.reject(
                    RejectReason::AlreadyActive {
                        artifact_id: prepared.module.artifact_id.clone(),
                    },
                    timings,
                );
            }
            // The previous module is kept in the published payload for the fade; when no
            // fade is armed, it is simply not retained (MVP-0a semantics for free).
            let previous = if self.crossfade_frames == 0 {
                None
            } else {
                Some(Arc::clone(&active.current))
            };
            new_version = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
            let program = Arc::new(BlockProgram::new(
                Arc::clone(&prepared.module),
                previous,
                self.crossfade_frames,
                next_fade_epoch(),
            ));
            // Publish, then release the retired payload. Blocks already in flight keep it
            // alive through the reader guard; no code is unmapped mid-block.
            let retired = self.slot.store(program);
            drop(retired);
            drop(active);
        }
        timings.activate_ms = activate_started.elapsed().as_millis() as u64;
        timings.total_ms = timings
            .build_ms
            .saturating_add(timings.load_ms)
            .saturating_add(timings.activate_ms);

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

    /// Audio-thread entry point: lock-free, allocation-free, free-free, and
    /// crossfade-aware.
    ///
    /// The reader guard is non-owning: it cannot release the last `Arc` reference, so
    /// the retired module's `Drop` (and therefore the deferred library release) can
    /// never run on this thread. The fade uses a fixed-size stack buffer only.
    pub fn process_rt(&self, input: &[f32], output: &mut [f32], state: &mut DspState) -> usize {
        let program = self.slot.enter();
        let frames = input.len().min(output.len());
        if frames == 0 {
            return 0;
        }

        if let Some(previous) = program.previous.as_ref() {
            // Per-thread fade: the module being replaced keeps its own continuous state,
            // which is what makes its output ramp instead of jumping. A thread that has
            // never seen this publication starts from the caller's current state.
            let mut fade = FADE_STATE
                .try_with(|cell| cell.get())
                .ok()
                .flatten()
                .filter(|f| f.epoch == program.fade_epoch)
                .unwrap_or(FadeThread {
                    epoch: program.fade_epoch,
                    remaining: program.fade_total,
                    state: *state,
                });
            if fade.remaining > 0 {
                let chunk = frames.min(fade.remaining as usize).min(MAX_FADE_FRAMES);
                let mut old_output = [0f32; MAX_FADE_FRAMES];
                previous.process_block(&input[..chunk], &mut old_output[..chunk], &mut fade.state);
                program
                    .current
                    .process_block(&input[..frames], &mut output[..frames], state);

                let total = program.fade_total.max(1) as f32;
                let done = program.fade_total.saturating_sub(fade.remaining) as f32;
                for (i, sample) in output.iter_mut().take(chunk).enumerate() {
                    let w = ((done + i as f32) / total).clamp(0.0, 1.0);
                    *sample = old_output[i] * (1.0 - w) + *sample * w;
                }
                fade.remaining = fade.remaining.saturating_sub(chunk as u32);
                let _ = FADE_STATE.try_with(|cell| cell.set(Some(fade)));
                program.record_fade_progress(chunk);
                return frames;
            }
        }

        program
            .current
            .process_block(&input[..frames], &mut output[..frames], state);
        frames
    }

    /// Enter a block with a non-owning guard on the published program and keep it alive
    /// for its duration. This is the real-time safe equivalent of [`BlockHost::enter`].
    /// Used by tests and by callers that need to measure the block themselves.
    pub fn enter_rt(&self) -> crate::slot::ReadGuard<'_, BlockProgram> {
        self.slot.enter()
    }

    /// Enter a block with an **owning** `Arc<BlockModule>` of the active module.
    /// Control-thread only: if the returned `Arc` is the last reference to a retired
    /// module, its `Drop` would run on the caller's thread. An audio callback must call
    /// [`BlockHost::enter_rt`] instead.
    pub fn enter(&self) -> Arc<BlockModule> {
        Arc::clone(&self.slot.load_owning().current)
    }

    pub fn active_version(&self) -> ActiveVersionId {
        ActiveVersionId(self.generation.load(Ordering::SeqCst))
    }

    pub fn active_artifact(&self) -> ArtifactId {
        self.slot.load_owning().current.artifact_id.clone()
    }

    pub fn active_patch_id(&self) -> String {
        self.slot.load_owning().current.manifest.patch_id.clone()
    }

    /// Frames of crossfade still to render on the active program (`0` = steady state).
    ///
    /// This is an observation for control threads; each audio thread keeps its own
    /// progress, so a host with several callback threads can report a value that differs
    /// from a given thread's remaining fade.
    pub fn active_fade_remaining(&self) -> u32 {
        let program = self.slot.load_owning();
        program.fade_total.saturating_sub(program.fade_rendered())
    }

    pub fn active_verification(&self) -> VerificationStatus {
        *self.verification.read().unwrap_or_else(|e| e.into_inner())
    }

    pub fn status(&self) -> HostStatus {
        let program = self.slot.load_owning();
        HostStatus {
            schema_version: PATCH_SCHEMA_VERSION,
            pid: std::process::id(),
            active_version: self.active_version(),
            artifact_id: program.current.artifact_id.clone(),
            patch_id: program.current.manifest.patch_id.clone(),
            activations: self.activations.load(Ordering::SeqCst),
            unloaded_modules: unloaded_modules(),
            live_modules: live_modules(),
        }
    }
}
