#![allow(dead_code)]

#[no_mangle]
pub extern "C" fn abi_entry(value: u32) -> u32 {
    value + 1
}
