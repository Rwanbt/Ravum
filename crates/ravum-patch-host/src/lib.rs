//! Patch host: versioned native modules behind a checked C ABI.
//!
//! Scope and honest limits of this MVP:
//! * one patchable entry point (`f64 -> f64`), because a wider ABI needs its own
//!   layout/compat study (RAV-020);
//! * the module describes itself (`ravum_abi_version`, `ravum_manifest_v1`) but
//!   the *host* decides; a self-declaration is never trusted on its own;
//! * in-flight code is kept alive by `Arc` reference counting and unloaded by a
//!   dedicated thread, never by the caller;
//! * state migration across versions is **not** implemented (ADR-022 remains open).
//!
//! Not provided: arbitrary struct passing, panics across the FFI boundary,
//! thread-affine state, or any claim of hard real-time behaviour.

mod block;
mod host;
mod reclaimer;
mod slot;

pub use block::{
    prepare_block, BlockFn, BlockHost, BlockModule, BlockProgram, PreparedBlock,
    DEFAULT_CROSSFADE_FRAMES, MAX_FADE_FRAMES,
};
pub use host::{
    inspect_artifact, prepare, ArtifactFacts, ComputeFn, ExpectedAbi, LoadedModule, PatchHost,
    PreparedPatch,
};
pub use reclaimer::{
    init_reclaimer, live_modules, pending_reclamations, unloaded_modules, wait_until_idle,
    DeferredLibrary,
};
pub use slot::{ArcSlot, ReadGuard};
