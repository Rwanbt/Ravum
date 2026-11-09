//! Unit with a build script: its compilation depends on executed build-script
//! output, so the conservative path is a Cargo fallback.

pub fn build_marker() -> &'static str {
    option_env!("RAVUM_FIXTURE_BUILD").unwrap_or("missing")
}
