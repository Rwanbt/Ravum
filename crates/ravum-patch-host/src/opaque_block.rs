//! ABI 3 keeps the module's typed state in an aligned host allocation.

use std::alloc::{alloc, dealloc, Layout};
use std::ffi::{c_char, CStr};
use std::path::Path;
use std::ptr::NonNull;

use ravum_protocol::dsp::{
    DspStateLayout, DSP_STATE_INIT_EXPORT, DSP_STATE_LAYOUT_EXPORT, DSP_STATE_MIGRATE_FROM_EXPORT,
    DSP_STATE_MIGRATE_STATUS_EXPORT,
};
use ravum_protocol::{
    ArtifactId, BuildOutput, EvidenceRecord, PatchManifest, RejectReason, SnapshotId, DSP_EXPORT,
    DSP_STATE_SCHEMA_EXPORT,
};

use crate::host::{evaluate_gates, validate_loaded, ExpectedAbi};
use crate::DeferredLibrary;

type Process = unsafe extern "C" fn(*mut u8, *const f32, *mut f32, u32);
type Init = unsafe extern "C" fn(*mut u8);

mod live;
pub use live::{OpaqueEditor, OpaqueRunner, OpaqueUpdate};

pub struct PreparedOpaqueBlock {
    pub manifest: PatchManifest,
    pub artifact_id: ArtifactId,
    schema: String,
    layout: DspStateLayout,
    init: Init,
    process: Process,
    _library: DeferredLibrary,
}

impl PreparedOpaqueBlock {
    pub fn layout(&self) -> DspStateLayout {
        self.layout
    }

    pub fn schema(&self) -> &str {
        &self.schema
    }

    fn authorize(
        &self,
        build: &BuildOutput,
        evidence: Option<&EvidenceRecord>,
        current_snapshot: &SnapshotId,
    ) -> Result<(), RejectReason> {
        if self.artifact_id != build.artifact_id {
            return Err(RejectReason::ArtifactMismatch {
                expected: build.artifact_id.clone(),
                found: self.artifact_id.clone(),
            });
        }
        evaluate_gates(build, &self.artifact_id, evidence, current_snapshot)?;
        Ok(())
    }

    fn migrate_state(
        &self,
        current: &PreparedOpaqueBlock,
        previous: &StateBuffer,
    ) -> Result<StateBuffer, RejectReason> {
        let library = &self._library;
        type Migrate = unsafe extern "C" fn(*const u8, *mut u8);
        // SAFETY: ABI version and manifest were validated before preparing the module.
        let migrate: Migrate =
            unsafe { library.symbol(b"ravum_state_migrate\0") }.map_err(|_| {
                RejectReason::StateSchemaChanged {
                    expected: current.schema.clone(),
                    found: self.schema.clone(),
                }
            })?;
        // SAFETY: the ABI specifies a digest export for migrations.
        let origin = unsafe {
            read_digest(
                library,
                b"ravum_state_migrate_from\0",
                DSP_STATE_MIGRATE_FROM_EXPORT,
            )
        }?;
        if origin != current.schema {
            return Err(RejectReason::StateSchemaChanged {
                expected: current.schema.clone(),
                found: origin,
            });
        }
        // SAFETY: this optional extension has a fixed signature. Absence
        // prevents calling a migration whose outcome cannot be observed.
        let status: unsafe extern "C" fn() -> u32 =
            unsafe { library.symbol(b"ravum_state_migrate_status\0") }
                .map_err(|_| missing(DSP_STATE_MIGRATE_STATUS_EXPORT))?;
        let next = StateBuffer::allocate(self.layout)?;
        // SAFETY: the digest attests the snapshot layout. The status getter
        // is called on this editor thread after the module catches its panic.
        unsafe {
            migrate(previous.ptr.as_ptr(), next.ptr.as_ptr());
            if status() != 1 {
                return Err(RejectReason::StateMigrationFailed);
            }
        }
        Ok(next)
    }

    fn render(&self, state: &mut StateBuffer, input: &[f32], output: &mut [f32]) -> usize {
        let frames = input.len().min(output.len()).min(u32::MAX as usize);
        if frames > 0 {
            // SAFETY: both slices cover `frames`, and the module and its
            // initialized, layout-compatible state remain live for the call.
            unsafe {
                (self.process)(
                    state.ptr.as_ptr(),
                    input.as_ptr(),
                    output.as_mut_ptr(),
                    frames as u32,
                )
            };
        }
        frames
    }
}

/// Load and validate an ABI 3 artifact on the editor thread.
///
/// # Safety
/// The caller must trust the native artifact to honor the ABI. Metadata gates
/// check compatibility, but cannot authenticate executable code or pointers.
pub unsafe fn prepare_opaque_block(path: &Path) -> Result<PreparedOpaqueBlock, RejectReason> {
    let library =
        DeferredLibrary::open(path).map_err(|detail| RejectReason::LoadFailed { detail })?;
    let validated = validate_loaded(library, path, &ExpectedAbi::dsp_opaque())?;
    let library = validated.lib;
    // SAFETY: the caller vouches for native code, and the ABI gate ran first.
    let layout_fn: unsafe extern "C" fn() -> DspStateLayout =
        unsafe { library.symbol(b"ravum_state_layout\0") }
            .map_err(|_| missing(DSP_STATE_LAYOUT_EXPORT))?;
    // SAFETY: the ABI and manifest gates select the ABI 3 metadata signature.
    let layout = unsafe { layout_fn() };
    if !layout.is_valid() {
        return Err(layout_refusal(layout));
    }
    // SAFETY: ABI 3 guarantees the signature of the schema export.
    let schema =
        unsafe { read_digest(&library, b"ravum_state_schema\0", DSP_STATE_SCHEMA_EXPORT) }?;
    // SAFETY: ABI 3 fixes both signatures, and the library is retained.
    let init = unsafe { library.symbol(b"ravum_state_init\0") }
        .map_err(|_| missing(DSP_STATE_INIT_EXPORT))?;
    // SAFETY: the process signature is selected by the ABI 3 gate above.
    let process =
        unsafe { library.symbol(b"ravum_process_block\0") }.map_err(|_| missing(DSP_EXPORT))?;
    Ok(PreparedOpaqueBlock {
        manifest: validated.manifest,
        artifact_id: validated.artifact_id,
        schema,
        layout,
        init,
        process,
        _library: library,
    })
}

// The caller selects an ABI 3 digest export from a trusted native artifact.
unsafe fn read_digest(
    library: &DeferredLibrary,
    symbol: &[u8],
    name: &str,
) -> Result<String, RejectReason> {
    // SAFETY: the caller selects an ABI 3 metadata function returning a C string.
    let getter: unsafe extern "C" fn() -> *const c_char =
        unsafe { library.symbol(symbol) }.map_err(|_| missing(name))?;
    // SAFETY: this signature is guaranteed by the selected metadata export.
    let raw = unsafe { getter() };
    let text = if raw.is_null() {
        None
    } else {
        // SAFETY: the artifact supplies a library-owned NUL-terminated string.
        unsafe { CStr::from_ptr(raw) }.to_str().ok()
    };
    text.filter(|value| {
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
    .map(str::to_owned)
    .ok_or_else(|| RejectReason::ManifestInvalid {
        detail: format!("invalid digest from {name}"),
    })
}

fn missing(symbol: &str) -> RejectReason {
    RejectReason::MissingExport {
        symbol: symbol.into(),
    }
}

fn layout_refusal(layout: DspStateLayout) -> RejectReason {
    RejectReason::StateLayout {
        size: layout.size,
        align: layout.align,
    }
}

struct StateBuffer {
    ptr: NonNull<u8>,
    allocation: Layout,
}

// SAFETY: this allocation is uniquely owned and contains only eligible
// scalar/aggregate DSP fields, with no borrowed or thread-affine values.
unsafe impl Send for StateBuffer {}

impl StateBuffer {
    fn allocate(layout: DspStateLayout) -> Result<Self, RejectReason> {
        let allocation =
            Layout::from_size_align(layout.size.max(1) as usize, layout.align as usize)
                .map_err(|_| layout_refusal(layout))?;
        // SAFETY: the allocation has a nonzero size and a valid layout.
        let ptr =
            NonNull::new(unsafe { alloc(allocation) }).ok_or_else(|| RejectReason::LoadFailed {
                detail: "could not allocate opaque DSP state".into(),
            })?;
        Ok(Self { ptr, allocation })
    }

    fn copy_from(&mut self, source: &Self) {
        assert_eq!(self.allocation, source.allocation);
        // SAFETY: both distinct buffers were allocated for the caller's validated
        // layout. Copying bytes preserves padding without reading it as a value.
        unsafe {
            std::ptr::copy_nonoverlapping(
                source.ptr.as_ptr(),
                self.ptr.as_ptr(),
                self.allocation.size(),
            );
        }
    }
}

impl Drop for StateBuffer {
    fn drop(&mut self) {
        // SAFETY: this is the unique owner and the original allocation layout.
        unsafe { dealloc(self.ptr.as_ptr(), self.allocation) };
    }
}

/// Exclusive state owner. Run lifecycle methods on the editor side at a block
/// boundary, after the preceding `process` call has released its borrow.
pub struct OpaqueBlock {
    module: PreparedOpaqueBlock,
    state: StateBuffer,
}

impl OpaqueBlock {
    pub fn start(module: PreparedOpaqueBlock) -> Result<Self, RejectReason> {
        let state = StateBuffer::allocate(module.layout)?;
        // SAFETY: the fresh buffer meets the validated module layout. The
        // initializer writes a valid State without reading the old bytes.
        unsafe { (module.init)(state.ptr.as_ptr()) };
        Ok(Self { module, state })
    }

    pub fn replace(
        &mut self,
        module: PreparedOpaqueBlock,
        build: &BuildOutput,
        evidence: Option<&EvidenceRecord>,
        current_snapshot: &SnapshotId,
    ) -> Result<(), RejectReason> {
        module.authorize(build, evidence, current_snapshot)?;
        if module.schema == self.module.schema {
            if module.layout != self.module.layout {
                return Err(layout_refusal(module.layout));
            }
        } else {
            let mut snapshot = StateBuffer::allocate(self.module.layout)?;
            snapshot.copy_from(&self.state);
            self.state = module.migrate_state(&self.module, &snapshot)?;
        }
        self.module = module;
        Ok(())
    }

    /// Audio path: exclusive access to the state and no allocation or locks.
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) -> usize {
        self.module.render(&mut self.state, input, output)
    }
}
