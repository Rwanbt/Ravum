//! Fixture de preuve - domaine trop lent pour le budget.
//!
//! Dort 600 ms pour l'entree sentinelle 99.0, afin de produire un `Timeout`
//! verifiable sans faire durer la suite de tests.

use std::os::raw::c_char;

const MANIFEST: &[u8] = b"{\"schema_version\":1,\"patch_id\":\"proof-slow\",\"export\":\"ravum_compute\",\"signature\":\"f64->f64\"}\0";

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
    if x == 99.0 {
        std::thread::sleep(std::time::Duration::from_millis(600));
    }
    x * x
}
