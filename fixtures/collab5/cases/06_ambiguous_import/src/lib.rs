#![allow(dead_code)]

mod a { pub fn f() -> u32 { 1 } }
use a::*;
mod b {}
fn r() -> u32 { f() }
