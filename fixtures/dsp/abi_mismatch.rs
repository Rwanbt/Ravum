//! Fixture DSP negative — declare l'ABI scalaire (1) tout en exportant un symbole de bloc.
//!
//! L'hote DSP attend l'ABI 2 : il doit refuser avant d'appeler quoi que ce soit.

use std::os::raw::c_char;

const MANIFEST: &[u8] = b"{\"schema_version\":1,\"patch_id\":\"dsp-wrong-abi\",\"export\":\"ravum_process_block\",\"signature\":\"block:f32->f32+state:v1\"}\0";

#[no_mangle]
pub extern "C" fn ravum_abi_version() -> u32 {
    1
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
        output[i] = input[i] * 100.0;
    }
}

#[repr(C)]
pub struct DspState {
    pub z1: f32,
    pub z2: f32,
}
