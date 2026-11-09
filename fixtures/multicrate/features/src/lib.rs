//! Optional-feature unit: its cfg surface is not enumerable from metadata, so the
//! graph must fall back to Cargo for this package.

#[cfg(feature = "extra")]
pub fn extra_value() -> u32 {
    42
}

#[cfg(not(feature = "extra"))]
pub fn extra_value() -> u32 {
    0
}
