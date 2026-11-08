//! Fixture négative — export manquant.
//!
//! Aucun `ravum_compute` n'est exporté. L'hôte doit refuser avec
//! `MissingExport` et ne rien activer.

use std::os::raw::c_char;

const MANIFEST: &[u8] = b"{\"schema_version\":1,\"patch_id\":\"no-compute\",\"export\":\"ravum_compute\",\"signature\":\"f64->f64\"}\0";

#[no_mangle]
pub extern "C" fn ravum_abi_version() -> u32 {
    1
}

#[no_mangle]
pub extern "C" fn ravum_manifest_v1() -> *const c_char {
    MANIFEST.as_ptr() as *const c_char
}

#[no_mangle]
pub extern "C" fn something_else(x: f64) -> f64 {
    x + 7.0
}
