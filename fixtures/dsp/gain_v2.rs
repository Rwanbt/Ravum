//! Fixture DSP — gain 2.0, sans etat. Meme ABI, comportement audiblement different.

use std::os::raw::c_char;

const MANIFEST: &[u8] = b"{\"schema_version\":1,\"patch_id\":\"dsp-gain-v2\",\"export\":\"ravum_process_block\",\"signature\":\"block:f32->f32+state:v1\",\"author\":\"fixture\"}\0";

#[no_mangle]
pub extern "C" fn ravum_abi_version() -> u32 {
    2
}

#[no_mangle]
pub extern "C" fn ravum_manifest_v1() -> *const c_char {
    MANIFEST.as_ptr() as *const c_char
}

#[no_mangle]
pub extern "C" fn ravum_process_block(
    input: *const f32,
    output: *mut f32,
    frames: u32,
    _state: *mut DspState,
) {
    let n = frames as usize;
    let input = unsafe { std::slice::from_raw_parts(input, n) };
    let output = unsafe { std::slice::from_raw_parts_mut(output, n) };
    for i in 0..n {
        output[i] = input[i] * 2.0;
    }
}

#[repr(C)]
pub struct DspState {
    pub z1: f32,
    pub z2: f32,
}
