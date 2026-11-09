//! Artifact cache behaviour, including the two facts that motivated it:
//! a loaded DLL cannot be relinked in place, and MSVC output is not reproducible.

use ravum_rust_adapter::RustAdapter;
use ravum_snapshot::SourceSnapshot;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn work(name: &str) -> PathBuf {
    let dir = root().join("target/ravum-tests").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("work dir");
    dir
}

fn snapshot() -> SourceSnapshot {
    let path = root().join("fixtures/proof/positive.rs");
    SourceSnapshot::from_file(&path).expect("fixture")
}

#[test]
fn the_same_snapshot_is_built_once_and_then_reused() {
    let dir = work("cache-reuse");
    let adapter = RustAdapter::new("rustc", dir.join("work"));
    let snapshot = snapshot();

    let first = adapter
        .build_cdylib(&snapshot, &dir.join("artifacts"))
        .expect("build");
    assert!(first.success);
    assert!(!first.build.cached_build);

    let second = adapter
        .build_cdylib(&snapshot, &dir.join("artifacts"))
        .expect("build");
    assert!(second.success);
    assert!(
        second.build.cached_build,
        "second build must be a cache hit"
    );
    assert_eq!(first.build.artifact_id, second.build.artifact_id);
    assert_eq!(first.build.artifact_path, second.build.artifact_path);
    assert!(
        second.build.build_ms <= first.build.build_ms,
        "a cache hit cannot be slower than a real build ({} vs {} ms)",
        second.build.build_ms,
        first.build.build_ms
    );
}

#[test]
fn a_tampered_cached_artifact_is_rebuilt_not_trusted() {
    let dir = work("cache-tamper");
    let adapter = RustAdapter::new("rustc", dir.join("work"));
    let snapshot = snapshot();
    let first = adapter
        .build_cdylib(&snapshot, &dir.join("artifacts"))
        .expect("build");

    let mut bytes = std::fs::read(&first.build.artifact_path).expect("read artifact");
    bytes.push(0);
    std::fs::write(&first.build.artifact_path, &bytes).expect("tamper");

    let second = adapter
        .build_cdylib(&snapshot, &dir.join("artifacts"))
        .expect("build");
    assert!(
        !second.build.cached_build,
        "a modified artifact must never be served from cache"
    );
    assert_ne!(first.build.artifact_id, second.build.artifact_id);
}

#[test]
fn the_cache_can_be_disabled_for_measurement() {
    let dir = work("cache-off");
    let adapter = RustAdapter::new("rustc", dir.join("work")).with_artifact_cache(false);
    let snapshot = snapshot();

    let first = adapter
        .build_cdylib(&snapshot, &dir.join("artifacts"))
        .expect("build");
    let second = adapter
        .build_cdylib(&snapshot, &dir.join("artifacts"))
        .expect("build");
    assert!(!first.build.cached_build);
    assert!(
        !second.build.cached_build,
        "measurement mode must really rebuild"
    );
    assert!(
        Path::new(&first.build.artifact_path).is_file()
            && Path::new(&second.build.artifact_path).is_file(),
        "cold builds must not overwrite each other"
    );
}

/// Measured fact on the reference machine: two `rustc` invocations producing the
/// same `cdylib` do **not** yield identical bytes. This is why the proof cache is
/// keyed through a stable artifact id produced by the build cache, and why the
/// pipeline never rewrites a loaded artifact.
#[cfg(windows)]
#[test]
fn msvc_output_is_not_byte_reproducible_which_is_why_the_cache_exists() {
    let dir = work("cache-nondeterminism");
    let adapter = RustAdapter::new("rustc", dir.join("work")).with_artifact_cache(false);
    let snapshot = snapshot();

    let first = adapter
        .build_cdylib(&snapshot, &dir.join("artifacts"))
        .expect("build");
    let second = adapter
        .build_cdylib(&snapshot, &dir.join("artifacts"))
        .expect("build");
    assert_ne!(
        first.build.artifact_path, second.build.artifact_path,
        "cold builds use distinct paths"
    );
    assert_ne!(
        first.build.artifact_id, second.build.artifact_id,
        "if this ever becomes reproducible, remove the disclaimer in docs/mvp/M2-evidence.md"
    );
}
