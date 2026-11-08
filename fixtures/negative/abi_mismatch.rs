//! Fixture négative — ABI native incompatible.
//!
//! Le module compile et exporte tous les symboles, mais déclare ABI 999.
//! L'hôte doit le refuser avant même de résoudre `ravum_compute`.

use std::os::raw::c_char;

const MANIFEST: &[u8] = b"{\"schema_version\":1,\"patch_id\":\"abi-999\",\"export\":\"ravum_compute\",\"signature\":\"f64->f64\"}\0";

#[no_mangle]
pub extern "C" fn ravum_abi_version() -> u32 {
    999
}

#[no_mangle]
pub extern "C" fn ravum_manifest_v1() -> *const c_char {
    MANIFEST.as_ptr() as *const c_char
}

#[no_mangle]
pub extern "C" fn ravum_compute(x: f64) -> f64 {
    x * 3.0
}
