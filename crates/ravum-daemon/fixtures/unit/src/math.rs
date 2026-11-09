//! Arithmetic helpers used by the pipeline module.

/// Global scale factor applied by [`scale`].
pub const SCALE: f64 = 2.5;

/// Number of buckets produced by [`histogram`].
pub const BUCKETS: usize = 8;

/// Multiplies `x` by [`SCALE`].
pub fn scale(x: f64) -> f64 {
    x * SCALE
}

/// Sums a slice with a simple loop so the front-end has real work to type-check.
pub fn accumulate(values: &[f64]) -> f64 {
    let mut total = 0.0;
    for value in values {
        total += *value;
    }
    total
}

/// Clamps `x` into the inclusive range `[0, 1]`.
pub fn clamp01(x: f64) -> f64 {
    if x < 0.0 {
        0.0
    } else if x > 1.0 {
        1.0
    } else {
        x
    }
}

/// Applies `op` to every element of `values`, returning a new vector.
pub fn map_all<F>(values: &[f64], mut op: F) -> Vec<f64>
where
    F: FnMut(f64) -> f64,
{
    values.iter().copied().map(|value| op(value)).collect()
}

/// Buckets `values` into [`BUCKETS`] equal ranges of `[0, 1]`.
pub fn histogram(values: &[f64]) -> [usize; BUCKETS] {
    let mut buckets = [0usize; BUCKETS];
    for value in values {
        let clamped = clamp01(*value);
        let mut index = (clamped * BUCKETS as f64) as usize;
        if index >= BUCKETS {
            index = BUCKETS - 1;
        }
        buckets[index] += 1;
    }
    buckets
}
