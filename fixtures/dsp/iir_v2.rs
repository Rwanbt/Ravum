//! Fixture DSP — meme filtre a un pole (v2), coefficient different.
//!
//! Disposition d'etat identique a v1 : le remplacement doit conserver `z1` et
//! produire une sortie continue (pas de saut a la frontiere de bloc).

use std::os::raw::c_char;

const MANIFEST: &[u8] = b"{\"schema_version\":1,\"patch_id\":\"dsp-iir-v2\",\"export\":\"ravum_process_block\",\"signature\":\"block:f32->f32+state:v1\",\"author\":\"fixture\"}\0";

const ALPHA: f32 = 0.5;

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
    state: *mut DspState,
) {
    let n = frames as usize;
    let input = unsafe { std::slice::from_raw_parts(input, n) };
    let output = unsafe { std::slice::from_raw_parts_mut(output, n) };
    let state = unsafe { &mut *state };
    for i in 0..n {
        let y = ALPHA * input[i] + (1.0 - ALPHA) * state.z1;
        state.z1 = y;
        output[i] = y;
    }
}

#[repr(C)]
pub struct DspState {
    pub z1: f32,
    pub z2: f32,
}
