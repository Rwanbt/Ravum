#![allow(dead_code)]

fn helper(x: u32) -> u32 {
    x + 1
}

fn compose(x: u32) -> u32 {
    helper(x) * 2
}
