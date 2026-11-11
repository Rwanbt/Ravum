//! Fixture négative — erreur de type : la compilation doit échouer.
//!
//! Le pipeline doit produire un diagnostic structuré et NE DOIT PAS construire
//! ni activer d'artefact. La version active précédente reste en place.

use std::os::raw::c_char;

const MANIFEST: &[u8] = b"{\"schema_version\":1,\"patch_id\":\"calculator-broken\",\"export\":\"ravum_compute\",\"signature\":\"f64->f64\"}\0";

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
    x * "not a number"
}
