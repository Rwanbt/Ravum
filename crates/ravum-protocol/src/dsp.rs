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

pub const DSP_OPAQUE_ABI_VERSION: u32 = 3;
pub const DSP_OPAQUE_SIGNATURE: &str = "block:f32->f32+state:opaque:v3";
pub const DSP_STATE_LAYOUT_EXPORT: &str = "ravum_state_layout";
pub const DSP_STATE_INIT_EXPORT: &str = "ravum_state_init";
pub const DSP_STATE_MIGRATE_EXPORT: &str = "ravum_state_migrate";
pub const DSP_STATE_MIGRATE_FROM_EXPORT: &str = "ravum_state_migrate_from";
pub const DSP_STATE_MIGRATE_STATUS_EXPORT: &str = "ravum_state_migrate_status";
pub const DSP_STATE_MAX_SIZE: u32 = 64 * 1024;
pub const DSP_STATE_MAX_ALIGN: u32 = 64;

/// C return type of `ravum_state_layout` in DSP ABI 3.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DspStateLayout {
    pub size: u32,
    pub align: u32,
}

impl DspStateLayout {
    /// A C struct's size includes its tail padding. An empty state still
    /// requires a nonzero power-of-two alignment.
    pub fn is_valid(self) -> bool {
        self.size <= DSP_STATE_MAX_SIZE
            && self.align <= DSP_STATE_MAX_ALIGN
            && self.align.is_power_of_two()
            && self.size.is_multiple_of(self.align)
    }
}

/// Exported symbol giving the module's state schema digest (RFC-E11-DSP-BLOCK
/// DSP-1.4): a NUL-terminated lowercase hexadecimal SHA-256.
pub const DSP_STATE_SCHEMA_EXPORT: &str = "ravum_state_schema";

/// Canonical text of [`DspState`] in the state schema format shared with hot
/// reload (`spec/language/reload.md` HR-1.1). A Ravum block whose state has
/// another schema is refused at compile time and, if forged, by the host.
pub const DSP_STATE_SCHEMA_V1_TEXT: &str = "#0=struct{z1:f32;z2:f32;}\n";

/// SHA-256 (lowercase hex) of [`DSP_STATE_SCHEMA_V1_TEXT`].
pub fn dsp_state_schema_v1_digest() -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(DSP_STATE_SCHEMA_V1_TEXT.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

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
    /// Deallocations performed on the audio thread (must be zero).
    ///
    /// Additive field (schema_version stays 1): an older report without it still
    /// deserializes, and its absence means "not measured", never zero.
    #[serde(default)]
    pub deallocations_on_audio_thread: u64,
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
    fn state_schema_digest_is_the_sha256_of_the_v1_text() {
        let digest = dsp_state_schema_v1_digest();
        assert_eq!(digest.len(), 64);
        assert!(digest
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn dsp_abi_is_distinct_from_the_scalar_one() {
        assert_ne!(DSP_ABI_VERSION, crate::PATCH_ABI_VERSION);
        assert_ne!(DSP_OPAQUE_ABI_VERSION, crate::PATCH_ABI_VERSION);
        assert_ne!(DSP_OPAQUE_ABI_VERSION, DSP_ABI_VERSION);
    }

    #[test]
    fn opaque_state_layout_enforces_bounds_and_c_tail_padding() {
        for layout in [
            DspStateLayout { size: 0, align: 1 },
            DspStateLayout { size: 0, align: 64 },
            DspStateLayout {
                size: DSP_STATE_MAX_SIZE,
                align: DSP_STATE_MAX_ALIGN,
            },
        ] {
            assert!(layout.is_valid(), "{layout:?}");
        }
        for layout in [
            DspStateLayout { size: 0, align: 0 },
            DspStateLayout { size: 12, align: 3 },
            DspStateLayout { size: 12, align: 8 },
            DspStateLayout {
                size: 128,
                align: 128,
            },
            DspStateLayout {
                size: DSP_STATE_MAX_SIZE + 1,
                align: 1,
            },
        ] {
            assert!(!layout.is_valid(), "{layout:?}");
        }
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
            deallocations_on_audio_thread: 0,
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
