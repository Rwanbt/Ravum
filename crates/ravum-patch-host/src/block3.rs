//! ABI DSP 3 (RFC-DSP-ABI-2): a block whose state is declared by the module.
//!
//! The host owns the state as an **opaque buffer** of the size and alignment
//! the module reports (`ravum_state_layout`), allocated and initialised
//! (`ravum_state_init`) when the module is loaded — off the audio thread
//! (DSP3-3, DSP3-6). A replacement with the same schema digest keeps the buffer
//! (DSP3-4); a different digest is refused (`StateSchemaChanged`; migration,
//! DSP3-5, is not implemented). ABI 2 and ABI 3 modules never load as each
//! other (`AbiMismatch`, DSP3-7).
//!
//! Honest limits of this first host: [`Block3Host`] is owned by one thread and
//! replaced through `&mut self` (no lock-free publication slot, no crossfade
//! yet, unlike [`crate::BlockHost`] for ABI 2). A real audio host must call
//! [`Block3Host::replace`] at a safe point it controls; the old module is then
//! dropped by the caller, not by the audio callback.

use crate::host::{validate_loaded, ExpectedAbi};
use crate::reclaimer::DeferredLibrary;
use ravum_protocol::{
    ArtifactId, PatchManifest, RejectReason, DSP3_ABI_VERSION, DSP3_MAX_STATE_ALIGN,
    DSP3_MAX_STATE_BYTES, DSP3_SIGNATURE, DSP_EXPORT,
};
use std::alloc::Layout;
use std::path::Path;
use std::ptr::NonNull;
use std::sync::Arc;

/// `{ size, align }` returned by `ravum_state_layout`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateLayout {
    pub size: u32,
    pub align: u32,
}

/// Entry points of an ABI 3 module.
///
/// # Safety
/// `state` is a buffer of the module's layout, initialised by its
/// `ravum_state_init`; `input`/`output` hold `frames` samples; nothing is
/// retained after the call.
pub type Block3Fn =
    unsafe extern "C" fn(input: *const f32, output: *mut f32, frames: u32, state: *mut u8);
type InitFn = unsafe extern "C" fn(state: *mut u8);
type LayoutFn = unsafe extern "C" fn() -> StateLayout;
type SchemaFn = unsafe extern "C" fn() -> *const std::os::raw::c_char;

impl ExpectedAbi {
    /// Contract of an ABI 3 block module.
    pub fn dsp3() -> Self {
        Self {
            abi_version: DSP3_ABI_VERSION,
            export: DSP_EXPORT.to_owned(),
            signature: DSP3_SIGNATURE.to_owned(),
            state_schema: None,
        }
    }
}

/// One loaded ABI 3 module.
pub struct Block3Module {
    process: Block3Fn,
    init: InitFn,
    pub layout: StateLayout,
    pub schema: String,
    pub manifest: PatchManifest,
    pub artifact_id: ArtifactId,
    _lib: DeferredLibrary,
}

/// Load and validate an ABI 3 block module. Never activates.
pub fn prepare_block3(path: &Path) -> Result<Arc<Block3Module>, RejectReason> {
    let lib = DeferredLibrary::open(path).map_err(|detail| RejectReason::LoadFailed { detail })?;
    let validated = validate_loaded(lib, path, &ExpectedAbi::dsp3())?;
    let missing = |symbol: &str| RejectReason::MissingExport {
        symbol: symbol.to_owned(),
    };
    // SAFETY: symbol types follow the ABI 3 contract, whose version and
    // manifest signature were checked by `validate_loaded` just above.
    let schema_fn: SchemaFn = unsafe { validated.lib.symbol(b"ravum_state_schema\0") }
        .map_err(|_| missing("ravum_state_schema"))?;
    let layout_fn: LayoutFn = unsafe { validated.lib.symbol(b"ravum_state_layout\0") }
        .map_err(|_| missing("ravum_state_layout"))?;
    let init: InitFn = unsafe { validated.lib.symbol(b"ravum_state_init\0") }
        .map_err(|_| missing("ravum_state_init"))?;
    let process: Block3Fn = unsafe { validated.lib.symbol(b"ravum_process_block\0") }
        .map_err(|_| missing(DSP_EXPORT))?;
    // SAFETY: both take no argument and return plain data (a C string, copied
    // at once; a `repr(C)` pair of `u32`).
    let raw = unsafe { schema_fn() };
    let schema = if raw.is_null() {
        String::new()
    } else {
        // SAFETY: non-null, NUL-terminated by the module contract.
        unsafe { std::ffi::CStr::from_ptr(raw) }
            .to_string_lossy()
            .into_owned()
    };
    let layout = unsafe { layout_fn() };
    if layout.size > DSP3_MAX_STATE_BYTES
        || layout.align == 0
        || !layout.align.is_power_of_two()
        || layout.align > DSP3_MAX_STATE_ALIGN
    {
        return Err(RejectReason::StateLayout {
            size: layout.size,
            align: layout.align,
        });
    }
    Ok(Arc::new(Block3Module {
        process,
        init,
        layout,
        schema,
        manifest: validated.manifest,
        artifact_id: validated.artifact_id,
        _lib: validated.lib,
    }))
}

/// The host-owned state buffer: allocated once, with the module's layout.
struct StateBuffer {
    ptr: NonNull<u8>,
    layout: Layout,
}

impl StateBuffer {
    fn new(state: StateLayout) -> Result<Self, RejectReason> {
        let bad = || RejectReason::StateLayout {
            size: state.size,
            align: state.align,
        };
        let layout = Layout::from_size_align(state.size as usize, state.align as usize)
            .map_err(|_| bad())?;
        if layout.size() == 0 {
            // A zero-sized state needs no memory: an aligned dangling pointer.
            let ptr = NonNull::new(layout.align() as *mut u8).ok_or_else(bad)?;
            return Ok(Self { ptr, layout });
        }
        // SAFETY: non-zero size, valid alignment (checked by `Layout`).
        let raw = unsafe { std::alloc::alloc_zeroed(layout) };
        let ptr = NonNull::new(raw).ok_or_else(bad)?;
        Ok(Self { ptr, layout })
    }
}

impl Drop for StateBuffer {
    fn drop(&mut self) {
        if self.layout.size() != 0 {
            // SAFETY: allocated by `alloc_zeroed` with this exact layout.
            unsafe { std::alloc::dealloc(self.ptr.as_ptr(), self.layout) };
        }
    }
}

/// A crossfade in progress: the replaced module keeps running on its own copy
/// of the state (taken at `replace_with_fade`, off the audio thread) while the
/// output ramps from it to the new module.
struct Fade3 {
    old: Arc<Block3Module>,
    state: StateBuffer,
    total: u32,
    done: u32,
}

/// One ABI 3 block and its state.
pub struct Block3Host {
    current: Arc<Block3Module>,
    state: StateBuffer,
    fade: Option<Fade3>,
}

impl Block3Host {
    /// Allocate the state with the module's layout and let the module write
    /// its initial value. Off the audio thread.
    pub fn new(module: Arc<Block3Module>) -> Result<Self, RejectReason> {
        let state = StateBuffer::new(module.layout)?;
        // SAFETY: the buffer has the module's size and alignment.
        unsafe { (module.init)(state.ptr.as_ptr()) };
        Ok(Self {
            current: module,
            state,
            fade: None,
        })
    }

    /// Process one block. No allocation, no free, no lock (audio thread):
    /// a finished crossfade is only marked done; its module and state copy
    /// are released by [`Self::collect`] on the caller's thread.
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) -> usize {
        let frames = input.len().min(output.len());
        if frames == 0 {
            return 0;
        }
        if let Some(fade) = self.fade.as_mut().filter(|f| f.done < f.total) {
            let mut start = 0;
            while start < frames {
                let n = (frames - start).min(crate::MAX_FADE_FRAMES);
                let mut old_out = [0.0f32; crate::MAX_FADE_FRAMES];
                let (i, o) = (&input[start..start + n], &mut output[start..start + n]);
                // SAFETY: as below; the fade state is a copy of the current
                // state made with the same layout (same schema digest).
                unsafe {
                    (fade.old.process)(
                        i.as_ptr(),
                        old_out.as_mut_ptr(),
                        n as u32,
                        fade.state.ptr.as_ptr(),
                    );
                    (self.current.process)(
                        i.as_ptr(),
                        o.as_mut_ptr(),
                        n as u32,
                        self.state.ptr.as_ptr(),
                    );
                }
                for (k, y) in o.iter_mut().enumerate() {
                    if fade.done < fade.total {
                        fade.done += 1;
                        let w = fade.done as f32 / fade.total as f32;
                        *y = old_out[k] * (1.0 - w) + *y * w;
                    }
                }
                start += n;
            }
            return frames;
        }
        // SAFETY: valid slices of `frames` samples; the state was initialised
        // by a module of the same schema digest (checked by `replace`).
        unsafe {
            (self.current.process)(
                input.as_ptr(),
                output.as_mut_ptr(),
                frames as u32,
                self.state.ptr.as_ptr(),
            )
        };
        frames
    }

    /// Replace the module, keeping the state (same schema digest), or refuse
    /// and keep the running one. Returns the previous module, to be dropped
    /// off the audio thread by the caller.
    pub fn replace(
        &mut self,
        module: Arc<Block3Module>,
    ) -> Result<Arc<Block3Module>, RejectReason> {
        if module.schema != self.current.schema || module.layout != self.current.layout {
            return Err(RejectReason::StateSchemaChanged {
                expected: self.current.schema.clone(),
                found: module.schema.clone(),
            });
        }
        if module.artifact_id == self.current.artifact_id {
            return Err(RejectReason::AlreadyActive {
                artifact_id: module.artifact_id.clone(),
            });
        }
        Ok(std::mem::replace(&mut self.current, module))
    }

    /// Like [`Self::replace`], with a crossfade of `frames` frames: the state
    /// is copied for the old module here, on the caller's thread. A fade
    /// still in progress is refused (`AlreadyActive` of the fading module):
    /// call [`Self::collect`] once it has finished.
    pub fn replace_with_fade(
        &mut self,
        module: Arc<Block3Module>,
        frames: u32,
    ) -> Result<(), RejectReason> {
        if let Some(fade) = &self.fade {
            if fade.done < fade.total {
                return Err(RejectReason::AlreadyActive {
                    artifact_id: self.current.artifact_id.clone(),
                });
            }
        }
        let copy = StateBuffer::new(self.current.layout)?;
        let old = self.replace(module)?;
        if frames == 0 {
            drop(old);
            return Ok(());
        }
        // SAFETY: both buffers have the same layout; they do not overlap.
        unsafe {
            std::ptr::copy_nonoverlapping(
                self.state.ptr.as_ptr(),
                copy.ptr.as_ptr(),
                self.state.layout.size(),
            )
        };
        self.fade = Some(Fade3 {
            old,
            state: copy,
            total: frames,
            done: 0,
        });
        Ok(())
    }

    /// Frames of crossfade still to render (0 when none).
    pub fn fade_remaining(&self) -> u32 {
        self.fade.as_ref().map(|f| f.total - f.done).unwrap_or(0)
    }

    /// Release a finished crossfade (module and state copy) on the caller's
    /// thread. Returns `true` if something was released.
    pub fn collect(&mut self) -> bool {
        if self.fade.as_ref().is_some_and(|f| f.done >= f.total) {
            self.fade = None;
            return true;
        }
        false
    }

    pub fn active(&self) -> &Arc<Block3Module> {
        &self.current
    }
}
