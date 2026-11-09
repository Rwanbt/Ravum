//! Root package of the workspace (the one that also declares `[workspace]`).
//! Its sources must never contain the nested member's sources.

pub fn app(x: f64) -> f64 {
    core::double(x) + 1.0
}

pub fn version() -> &'static str {
    "root-v1"
}
