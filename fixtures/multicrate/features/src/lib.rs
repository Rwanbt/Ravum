//! Optional-feature unit: its activation set comes from the resolved
//! `cargo metadata`, so a resolved run compiles it with the exact `--cfg
//! feature="…"` set and only an unknown activation falls back to Cargo.

#[cfg(feature = "extra")]
pub fn extra_value() -> u32 {
    42
}

#[cfg(not(feature = "extra"))]
pub fn extra_value() -> u32 {
    0
}
