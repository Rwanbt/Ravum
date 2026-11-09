//! Fixture de preuve - propriete vraie sur tout le domaine declare.
//!
//! `x * x` est fini, deterministe, positif ou nul, et non decroissant sur les
//! entrees positives. Sert de cas positif pour le broker de preuve.

use std::os::raw::c_char;

const MANIFEST: &[u8] = b"{\"schema_version\":1,\"patch_id\":\"proof-positive\",\"export\":\"ravum_compute\",\"signature\":\"f64->f64\"}\0";

#[no_mangle]
pub extern "C" fn ravum_abi_version() -> u32 {
    1
}

#[no_mangle]
pub extern "C" fn ravum_manifest_v1() -> *const c_char {
    MANIFEST.as_ptr() as *const c_char
}

#[no_mangle]
pub extern "C" fn ravum_compute(x: f64) -> f64 {
    x * x
}
