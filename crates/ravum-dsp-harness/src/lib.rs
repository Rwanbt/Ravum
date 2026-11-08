//! DSP harness (M3): allocation counting, block measurement, block evidence.
//!
//! This crate installs a counting global allocator for the processes that link it,
//! so "the audio thread does not allocate" becomes a measurement instead of a claim.
//!
//! Honest scope: the cadence is simulated (`frames / sample_rate`). There is no audio
//! driver, no real-time thread priority, no XRUN from a device. Nothing here
//! qualifies hard real-time.

pub mod alloc;
pub mod evidence;
pub mod measure;

pub use alloc::{
    allocator_ops_on_this_thread, allocations_on_this_thread, deallocations_on_this_thread,
    reset_thread_counter, reset_thread_counters, CountingAllocator,
};
pub use evidence::{block_evidence, dsp_evidence_key, EVIDENCE_PROPERTY};
pub use measure::{measure, measure_from, SessionConfig, SessionMeasurement, SessionOutcome};

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;
