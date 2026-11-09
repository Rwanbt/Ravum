#![allow(dead_code)]

use core::fmt::Debug;

fn debug_it<T: Debug>(value: T) -> String {
    format!("{value:?}")
}

fn other() -> u32 {
    0
}
