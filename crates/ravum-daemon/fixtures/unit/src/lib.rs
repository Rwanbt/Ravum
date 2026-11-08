//! Daemon fixture unit: a small but real Rust library compiled through the
//! public `rustc` CLI by the MVP-0b incremental analysis measurements.
//!
//! The unit spans four modules so the campaign can distinguish a local body
//! edit (no public API change) from a signature change that must invalidate
//! callers in another module. It has no dependency beyond `core`/`alloc`:
//! measurements must isolate compiler work, not dependency graph work.

#![forbid(unsafe_code)]

mod math;
mod pipeline;
mod shapes;

pub use math::{accumulate, clamp01, histogram, map_all, scale, SCALE};
pub use pipeline::{describe, score, shape_score, summary};
pub use shapes::{Point, Shape};

/// Public entry point: score raw samples and combine them with a shape's
/// centroid score. Kept small on purpose; it is the stable API of the unit.
pub fn evaluate(values: &[f64], shape: &Shape) -> f64 {
    score(values) + shape_score(shape)
}
