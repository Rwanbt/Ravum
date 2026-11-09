//! Nested workspace member consumed by the root package.

pub fn double(x: f64) -> f64 {
    x * 2.0
}

fn internal_offset() -> f64 {
    0.0
}

pub fn shifted(x: f64) -> f64 {
    x + internal_offset()
}
