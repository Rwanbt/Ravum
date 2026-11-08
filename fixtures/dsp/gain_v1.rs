//! Fixture DSP — gain 0.5, sans etat.
//!
//! ABI de bloc : `ravum_process_block`. L'etat est possede par l'hote et n'est pas
//! utilise par ce module, mais sa disposition reste declaree par le contrat.

use std::os::raw::c_char;

const MANIFEST: &[u8] = b"{\"schema_version\":1,\"patch_id\":\"dsp-gain-v1\",\"export\":\"ravum_process_block\",\"signature\":\"block:f32->f32+state:v1\",\"author\":\"fixture\"}\0";

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
        output[i] = input[i] * 0.5;
    }
}

/// Copie littérale du contrat `state:v1` (le module ne l'utilise pas).
#[repr(C)]
pub struct DspState {
    pub z1: f32,
    pub z2: f32,
}
