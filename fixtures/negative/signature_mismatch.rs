//! Fixture négative — signature logique incompatible.
//!
//! L'ABI native est correcte mais le manifeste annonce `i64->i64`.
//! Activer ce module appellerait une fonction avec la mauvaise convention :
//! le manifeste doit être refusé avant résolution de l'appel.

use std::os::raw::c_char;

const MANIFEST: &[u8] = b"{\"schema_version\":1,\"patch_id\":\"bad-signature\",\"export\":\"ravum_compute\",\"signature\":\"i64->i64\"}\0";

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
    x + 1000.0
}
