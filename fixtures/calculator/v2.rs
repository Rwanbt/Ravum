//! Fixture de patch — calculator v2.
//!
//! Même ABI, même signature, comportement observable différent : c'est le cas
//! « patch compatible » que la boucle M1 doit activer sans redémarrer le processus.

use std::os::raw::c_char;

const MANIFEST: &[u8] = b"{\"schema_version\":1,\"patch_id\":\"calculator-v2\",\"export\":\"ravum_compute\",\"signature\":\"f64->f64\",\"author\":\"fixture\"}\0";

#[no_mangle]
pub extern "C" fn ravum_abi_version() -> u32 {
    1
}

#[no_mangle]
pub extern "C" fn ravum_manifest_v1() -> *const c_char {
    MANIFEST.as_ptr() as *const c_char
}

/// v2 : double l'entrée puis ajoute 1.
#[no_mangle]
pub extern "C" fn ravum_compute(x: f64) -> f64 {
    x * 2.0 + 1.0
}
