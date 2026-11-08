//! Fixture DSP — filtre passe-bas a un pole (v1), etat possede par l'hote.
//!
//! `y[n] = a * x[n] + (1 - a) * z1`, puis `z1 = y[n]`.
//! L'etat vit chez l'hote : un remplacement compatible (meme disposition) conserve
//! donc le passe-bas sans aucune migration de memoire.

use std::os::raw::c_char;

const MANIFEST: &[u8] = b"{\"schema_version\":1,\"patch_id\":\"dsp-iir-v1\",\"export\":\"ravum_process_block\",\"signature\":\"block:f32->f32+state:v1\",\"author\":\"fixture\"}\0";

const ALPHA: f32 = 0.1;

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
