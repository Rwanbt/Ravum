//! Fixture de patch — calculator v1.
//!
//! Unité de patch autonome compilée par `rustc --crate-type cdylib`.
//! Le module se décrit lui-même ; l'hôte décide ensuite de l'accepter.

use std::os::raw::c_char;

const MANIFEST: &[u8] = b"{\"schema_version\":1,\"patch_id\":\"calculator-v1\",\"export\":\"ravum_compute\",\"signature\":\"f64->f64\",\"author\":\"fixture\"}\0";

#[no_mangle]
pub extern "C" fn ravum_abi_version() -> u32 {
    1
}

#[no_mangle]
pub extern "C" fn ravum_manifest_v1() -> *const c_char {
    MANIFEST.as_ptr() as *const c_char
}

/// v1 : double l'entrée.
#[no_mangle]
pub extern "C" fn ravum_compute(x: f64) -> f64 {
    x * 2.0
}
