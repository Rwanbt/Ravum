//! Consumer layer: depends transitively on `core` through `api`.

pub fn run(x: f64) -> f64 {
    api::scaled(x) + api::shifted(x)
}

pub fn point(x: f64) -> api::Point {
    api::midpoint(x, x + 1.0)
}
