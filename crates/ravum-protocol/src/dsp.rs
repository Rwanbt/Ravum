//! Block-processing (DSP) contract: M3.
//!
//! Design choice, stated explicitly: the **host owns the state**, the module is a
//! pure function of `(input block, state)`. That is what makes a compatible swap
//! keep its state for free — no migration code, no reinterpretation of memory
//! (ADR-022 stays open for *incompatible* states).
//!
//! A block module also carries the generic manifest (`ravum_abi_version`,
//! `ravum_manifest_v1`), so the same validation gates apply as for the M1 scalar ABI.

use serde::{Deserialize, Serialize};

/// Version of the block ABI. Distinct from the scalar `PATCH_ABI_VERSION` so a
/// scalar module can never be loaded as a DSP module by accident.
pub const DSP_ABI_VERSION: u32 = 2;

/// Exported symbol processing one interleaved-mono block.
pub const DSP_EXPORT: &str = "ravum_process_block";

/// Logical signature: `f32` block in, `f32` block out, host-owned state.
/// Changing the state layout requires a new signature, not a silent reinterpretation.
pub const DSP_SIGNATURE: &str = "block:f32->f32+state:v1";

/// Host-owned processing state.
///
/// `#[repr(C)]` and fixed-size on purpose: the layout **is** the contract. Fixtures
/// redeclare it literally (they are standalone files, not crate consumers), so a
/// mismatch is a compile-time or link-time error rather than a silent corruption.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct DspState {
    pub z1: f32,
    pub z2: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DspBlockTimings {
    pub count: u64,
    pub p50_us: u64,
    pub p95_us: u64,
    pub p99_us: u64,
    pub max_us: u64,
}

/// Observable result of one DSP session. Every field is measured, none estimated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DspSessionReport {
    pub schema_version: u32,
    pub blocks: u64,
    pub frames: u64,
    pub period_us: u64,
    pub block_timings: DspBlockTimings,
    /// Blocks whose processing time exceeded the simulated period.
    pub overruns: u64,
    /// Allocations performed on the audio thread (must be zero).
    pub allocations_on_audio_thread: u64,
    /// Largest |x[n] - x[n-1]| observed on the output.
    pub max_sample_delta: f32,
    /// Same metric measured on a pure sine of the same amplitude, as a baseline.
    pub baseline_sample_delta: f32,
    pub swaps: u64,
    pub active_patch_id: String,
    pub unloaded_modules: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_layout_is_two_f32_and_stable() {
        assert_eq!(std::mem::size_of::<DspState>(), 8);
        assert_eq!(std::mem::align_of::<DspState>(), 4);
    }

    #[test]
    fn dsp_abi_is_distinct_from_the_scalar_one() {
        assert_ne!(DSP_ABI_VERSION, crate::PATCH_ABI_VERSION);
    }

    #[test]
    fn report_round_trips() {
        let report = DspSessionReport {
            schema_version: 1,
            blocks: 3,
            frames: 768,
            period_us: 5333,
            block_timings: DspBlockTimings {
                count: 3,
                p50_us: 40,
                p95_us: 60,
                p99_us: 61,
                max_us: 61,
            },
            overruns: 0,
            allocations_on_audio_thread: 0,
            max_sample_delta: 0.12,
            baseline_sample_delta: 0.11,
            swaps: 1,
            active_patch_id: "gain-v2".to_owned(),
            unloaded_modules: 1,
        };
        let json = serde_json::to_string(&report).unwrap();
        let back: DspSessionReport = serde_json::from_str(&json).unwrap();
        assert_eq!(report, back);
    }
}
