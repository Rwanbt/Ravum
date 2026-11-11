#![allow(dead_code)]

fn checked_divide(value: u32, divisor: u32) -> u32 {
    assert!(divisor != 0, "divisor must not be zero");
    value / divisor
}
