//! Fixture de latence - v2.
//!
//! Meme ABI et meme sentinelle que v1, comportement different.

use std::os::raw::c_char;

const MANIFEST: &[u8] = b"{\"schema_version\":1,\"patch_id\":\"slow-v2\",\"export\":\"ravum_compute\",\"signature\":\"f64->f64\"}\0";

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
        std::thread::sleep(std::time::Duration::from_millis(3000));
    }
    x * 2.0 + 1000.0
}
