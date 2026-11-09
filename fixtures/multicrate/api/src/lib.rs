//! API layer on top of `core`. It consumes `core::scale`, so a `core` interface
//! change must force a real revalidation of this unit.

pub use core::Point;

pub fn scaled(x: f64) -> f64 {
    core::scale(x)
}

pub fn shifted(x: f64) -> f64 {
    core::add(core::scale(x), 1.0)
}

pub fn midpoint(a: f64, b: f64) -> Point {
    Point {
        x: (a + b) / 2.0,
        y: (a - b) / 2.0,
    }
}
