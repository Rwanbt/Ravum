//! Tiny arithmetic core used by the L1 cross-crate fixture.

/// Shared point type (used by the "shared type" negative case).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// Scales one value. Its public signature is the central L1 experiment.
pub fn scale(x: f64) -> f64 {
    x * 2.0
}

/// Adds two values (used by the "symbol removal" negative case).
pub fn add(a: f64, b: f64) -> f64 {
    a + b
}

/// Private helper: a body-only change must not invalidate dependents.
fn internal_factor() -> f64 {
    2.0
}

/// Calls the private helper so a body change here has no interface impact.
pub fn scaled(x: f64) -> f64 {
    scale(x) * internal_factor() / internal_factor()
}
