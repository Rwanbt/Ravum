//! Fixture de preuve - contre-exemple isole.
//!
//! La fonction est correcte partout sauf en `x == 2.0` ou elle renvoie -1.
//! Le verificateur doit produire un contre-exemple precis, pas un echec vague.

use std::os::raw::c_char;

const MANIFEST: &[u8] = b"{\"schema_version\":1,\"patch_id\":\"proof-negative\",\"export\":\"ravum_compute\",\"signature\":\"f64->f64\"}\0";

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
    if x == 2.0 {
        return -1.0;
    }
    x * x
}
