//! M1 acceptance tests: positive activation and every refusal path.
//!
//! These tests build real native artifacts with the public `rustc` CLI. They are
//! deliberately serialized (`--test-threads=1`) because each `rustc` invocation
//! costs memory and the reference machine is 16 Go with little headroom.

use ravum_coordinator::{LoopOutcome, Pipeline};
use ravum_patch_host::{prepare, ExpectedAbi, PatchHost};
use ravum_protocol::{RejectReason, VerificationStatus};
use ravum_snapshot::SourceSnapshot;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn fixture(rel: &str) -> SourceSnapshot {
    let path = fixtures().join(rel);
    SourceSnapshot::from_file(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

fn work(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/ravum-tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create work dir");
    dir
}

fn pipeline(name: &str) -> Pipeline {
    Pipeline::new("rustc", work(name))
}

/// Build one source and return (artifact path, build output).
fn build(pipeline: &Pipeline, source: &SourceSnapshot) -> (PathBuf, ravum_protocol::BuildOutput) {
    let outcome = pipeline.build(source).expect("adapter error");
    assert!(
        outcome.success,
        "build failed for {}: {:?}",
        source.id().short(),
        outcome.build.diagnostics
    );
    (PathBuf::from(&outcome.build.artifact_path), outcome.build)
}

fn bootstrap(artifact: &Path) -> PatchHost {
    PatchHost::bootstrap(ExpectedAbi::default(), artifact).expect("bootstrap")
}

#[test]
fn compatible_patch_changes_behaviour_without_restart() {
    let pipeline = pipeline("m1-compatible");
    let v1 = fixture("calculator/v1.rs");
    let v2 = fixture("calculator/v2.rs");

    let (artifact_v1, build_v1) = build(&pipeline, &v1);
    let host = bootstrap(&artifact_v1);
    assert_eq!(host.call(2.0), 4.0, "v1 must be active before the patch");
    assert_eq!(host.active_verification(), VerificationStatus::Assumed);

    let report = pipeline.run_once(&host, &v2, &|| v2.id().clone());
    assert!(
        report.activated(),
        "expected activation, got {:?}",
        report.outcome
    );

    assert_eq!(
        host.call(2.0),
        5.0,
        "v2 behaviour must be visible without restart"
    );
    let status = host.status();
    assert_eq!(status.patch_id, "calculator-v2");
    assert_eq!(status.activations, 1);
    assert_ne!(status.artifact_id, build_v1.artifact_id);
    assert_eq!(host.active_verification(), VerificationStatus::Tested);
    assert!(report.timings.total_ms >= report.timings.build_ms);
}

#[test]
fn invalid_source_is_refused_before_any_build() {
    let pipeline = pipeline("m1-broken");
    let v1 = fixture("calculator/v1.rs");
    let broken = fixture("negative/broken_type.rs");

    let (artifact_v1, _) = build(&pipeline, &v1);
    let host = bootstrap(&artifact_v1);

    let report = pipeline.run_once(&host, &broken, &|| broken.id().clone());
    match report.outcome {
        LoopOutcome::RefusedByCheck { errors } => assert!(errors > 0),
        other => panic!("expected RefusedByCheck, got {other:?}"),
    }
    assert!(
        report.build.is_none(),
        "nothing may be built on a failed check"
    );
    assert_eq!(host.call(2.0), 4.0, "the active version must be untouched");
    assert_eq!(host.status().activations, 0);
}

#[test]
fn incompatible_native_abi_is_refused() {
    let pipeline = pipeline("m1-abi");
    let v1 = fixture("calculator/v1.rs");
    let bad = fixture("negative/abi_mismatch.rs");

    let (artifact_v1, _) = build(&pipeline, &v1);
    let host = bootstrap(&artifact_v1);
    let (bad_artifact, _) = build(&pipeline, &bad);

    let reason = prepare(&bad_artifact, &ExpectedAbi::default()).unwrap_err();
    assert!(
        matches!(
            reason,
            RejectReason::AbiMismatch {
                expected: 1,
                found: 999
            }
        ),
        "unexpected reason: {reason:?}"
    );

    let report = pipeline.run_once(&host, &bad, &|| bad.id().clone());
    assert!(matches!(report.outcome, LoopOutcome::RefusedByHost { .. }));
    assert_eq!(host.call(2.0), 4.0);
    assert_eq!(host.status().activations, 0);
}

#[test]
fn incompatible_logical_signature_is_refused() {
    let pipeline = pipeline("m1-signature");
    let v1 = fixture("calculator/v1.rs");
    let bad = fixture("negative/signature_mismatch.rs");

    let (artifact_v1, _) = build(&pipeline, &v1);
    let host = bootstrap(&artifact_v1);
    let (bad_artifact, _) = build(&pipeline, &bad);

    let reason = prepare(&bad_artifact, &ExpectedAbi::default()).unwrap_err();
    match reason {
        RejectReason::SignatureMismatch { expected, found } => {
            assert_eq!(expected, "f64->f64");
            assert_eq!(found, "i64->i64");
        }
        other => panic!("expected SignatureMismatch, got {other:?}"),
    }
    assert_eq!(host.call(2.0), 4.0);
}

#[test]
fn missing_export_is_refused() {
    let pipeline = pipeline("m1-missing");
    let v1 = fixture("calculator/v1.rs");
    let bad = fixture("negative/missing_export.rs");

    let (artifact_v1, _) = build(&pipeline, &v1);
    let host = bootstrap(&artifact_v1);
    let (bad_artifact, _) = build(&pipeline, &bad);

    let reason = prepare(&bad_artifact, &ExpectedAbi::default()).unwrap_err();
    assert!(
        matches!(reason, RejectReason::MissingExport { .. }),
        "unexpected reason: {reason:?}"
    );
    assert_eq!(host.call(2.0), 4.0);
}

#[test]
fn stale_build_is_refused() {
    let pipeline = pipeline("m1-stale");
    let v1 = fixture("calculator/v1.rs");
    let v2 = fixture("calculator/v2.rs");

    let (artifact_v1, _) = build(&pipeline, &v1);
    let host = bootstrap(&artifact_v1);
    let (artifact_v2, build_v2) = build(&pipeline, &v2);
    let prepared = prepare(&artifact_v2, &ExpectedAbi::default()).expect("prepare v2");
    let evidence = ravum_coordinator::smoke_verify(&build_v2).expect("smoke");

    let report = host.apply(prepared, &build_v2, Some(&evidence), v1.id());
    assert_eq!(report.status, ravum_protocol::ActivationStatus::Rejected);
    assert!(matches!(
        report.reject_reason,
        Some(RejectReason::StaleBuild { .. })
    ));
    assert_eq!(host.call(2.0), 4.0, "a stale build must never be activated");
}

#[test]
fn activation_without_evidence_is_refused() {
    let pipeline = pipeline("m1-evidence");
    let v1 = fixture("calculator/v1.rs");
    let v2 = fixture("calculator/v2.rs");

    let (artifact_v1, _) = build(&pipeline, &v1);
    let host = bootstrap(&artifact_v1);
    let (artifact_v2, build_v2) = build(&pipeline, &v2);
    let prepared = prepare(&artifact_v2, &ExpectedAbi::default()).expect("prepare v2");

    let report = host.apply(prepared, &build_v2, None, v2.id());
    assert_eq!(report.status, ravum_protocol::ActivationStatus::Rejected);
    assert!(matches!(
        report.reject_reason,
        Some(RejectReason::NotVerified { .. })
    ));
    assert_eq!(host.call(2.0), 4.0);
}

#[test]
fn building_the_active_version_twice_is_refused_as_redundant() {
    let pipeline = pipeline("m1-redundant");
    let v1 = fixture("calculator/v1.rs");

    let (artifact_v1, build_v1) = build(&pipeline, &v1);
    let host = bootstrap(&artifact_v1);
    let prepared = prepare(&artifact_v1, &ExpectedAbi::default()).expect("prepare v1");
    let evidence = ravum_coordinator::smoke_verify(&build_v1).expect("smoke");

    let report = host.apply(prepared, &build_v1, Some(&evidence), v1.id());
    assert!(matches!(
        report.reject_reason,
        Some(RejectReason::AlreadyActive { .. })
    ));
    assert_eq!(host.status().activations, 0);
}

#[test]
fn result_built_from_a_superseded_snapshot_is_never_activated() {
    let pipeline = pipeline("m1-superseded");
    let v1 = fixture("calculator/v1.rs");
    let v2 = fixture("calculator/v2.rs");

    let (artifact_v1, _) = build(&pipeline, &v1);
    let host = bootstrap(&artifact_v1);

    // While v2 is being analysed/built, someone edits again: `current` moves on.
    let newer = fixture("calculator/v2.rs").with_overlay([(
        "v2.rs".to_string(),
        fixture("calculator/v2.rs")
            .content_of("v2.rs")
            .unwrap()
            .to_string()
            + "\n",
    )]);
    let report = pipeline.run_once(&host, &v2, &|| newer.id().clone());

    assert!(
        matches!(report.outcome, LoopOutcome::RefusedAsStale { .. }),
        "expected RefusedAsStale, got {:?}",
        report.outcome
    );
    assert_eq!(host.call(2.0), 4.0, "the previous version must stay active");
}

#[test]
fn in_flight_call_keeps_the_old_module_loaded() {
    let pipeline = pipeline("m1-inflight");
    let slow_v1 = fixture("latency/slow_v1.rs");
    let slow_v2 = fixture("latency/slow_v2.rs");

    let (artifact_v1, _) = build(&pipeline, &slow_v1);
    let host = bootstrap(&artifact_v1);

    // Per-module lifetime observation: a Weak lets us prove the old module is
    // still alive while a call executes in it, without relying on global counters
    // (which also count the verification loads of the same iteration).
    let module = host.enter();
    let weak = std::sync::Arc::downgrade(&module);
    let caller = std::thread::spawn(move || {
        // Sentinelle: this call blocks for ~3 s inside the native module, which is
        // deliberately longer than a full check+build+verify iteration.
        module.call(99.0)
    });
    std::thread::sleep(Duration::from_millis(100));

    let report = pipeline.run_once(&host, &slow_v2, &|| slow_v2.id().clone());
    assert!(
        report.activated(),
        "activation must not wait for the in-flight call"
    );

    let still_alive = weak
        .upgrade()
        .expect("the old module must not be freed while a call is executing in it");
    // 1 = the in-flight call's Arc, 2 = the temporary created by `upgrade()`.
    // A value of 3 would mean the host still holds the retired module.
    assert_eq!(
        std::sync::Arc::strong_count(&still_alive),
        2,
        "the host released its reference; only the in-flight call keeps it alive"
    );
    drop(still_alive);

    assert_eq!(host.call(2.0), 1004.0, "new calls use the new version");

    let old_value = caller.join().expect("in-flight call panicked");
    assert_eq!(
        old_value, 198.0,
        "the in-flight call finished on the OLD code"
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    while weak.upgrade().is_some() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        weak.upgrade().is_none(),
        "the reclaimer must free the old module once no call is in flight"
    );
    assert!(ravum_patch_host::wait_until_idle(Duration::from_secs(5)));
}
