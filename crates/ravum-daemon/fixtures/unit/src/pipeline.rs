//! Pipeline that turns raw samples into a bounded score, using both helpers.

use crate::math::{accumulate, clamp01, histogram, map_all, scale, SCALE};
use crate::shapes::{Point, Shape};

/// Turns a slice of raw measurements into a bounded score.
pub fn score(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let scaled = map_all(values, |value| scale(clamp01(value)));
    let total = accumulate(&scaled);
    total / values.len() as f64
}

/// Score of the shape's centroid, used by the public entry point.
pub fn shape_score(shape: &Shape) -> f64 {
    let Point { x, y } = shape.centroid();
    score(&[x, y])
}

/// Human readable description of a shape.
pub fn describe(shape: &Shape) -> String {
    format!("{:?} with area {:.3}", shape, shape.area())
}

/// Compact multi-line summary of a shape and a sample slice. Exercises
/// formatting, iteration and the histogram helper.
pub fn summary(shape: &Shape, values: &[f64]) -> String {
    let buckets = histogram(values);
    let (min, max) = shape.bounds();
    let mut text = String::new();
    text.push_str(&describe(shape));
    text.push('\n');
    text.push_str(&format!(
        "centroid=({:.2}, {:.2}) bounds=({:.2}, {:.2})..({:.2}, {:.2}) scale={SCALE}",
        shape.centroid().x,
        shape.centroid().y,
        min.x,
        min.y,
        max.x,
        max.y,
    ));
    text.push('\n');
    text.push_str("buckets=");
    for (index, count) in buckets.iter().enumerate() {
        if index > 0 {
            text.push(',');
        }
        text.push_str(&count.to_string());
    }
    text
}
