//! Fixture de preuve - resultat non fini.
//!
//! Renvoie NaN en `x == 0.0`. Toute obligation `Finite` doit echouer.

use std::os::raw::c_char;

const MANIFEST: &[u8] = b"{\"schema_version\":1,\"patch_id\":\"proof-nan\",\"export\":\"ravum_compute\",\"signature\":\"f64->f64\"}\0";

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
    if x == 0.0 {
        return f64::NAN;
    }
    x * x
}
