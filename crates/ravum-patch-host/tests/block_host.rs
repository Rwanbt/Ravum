//! M3 block-host tests: real DSP modules, real swaps, measured allocation behaviour.

use ravum_dsp_harness::{allocations_on_this_thread, block_evidence, reset_thread_counter};
use ravum_patch_host::{prepare_block, BlockHost, ExpectedAbi};
use ravum_protocol::{BuildOutput, DspState, RejectReason, VerificationStatus};
use ravum_rust_adapter::RustAdapter;
use ravum_snapshot::SourceSnapshot;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn work(name: &str) -> PathBuf {
    let dir = root().join("target/ravum-tests").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("work dir");
    dir
}

fn build(dir: &Path, fixture: &str) -> BuildOutput {
    let source = root().join("fixtures/dsp").join(fixture);
    let snapshot = SourceSnapshot::from_file(&source).expect("fixture");
    let adapter = RustAdapter::new("rustc", dir.join("work"));
    let outcome = adapter
        .build_cdylib(&snapshot, &dir.join("artifacts"))
        .expect("adapter");
    assert!(
        outcome.success,
        "{fixture} must compile: {:?}",
        outcome.build.diagnostics
    );
    outcome.build
}

fn host(dir: &Path, fixture: &str) -> (BlockHost, BuildOutput) {
    let build = build(dir, fixture);
    let host = BlockHost::bootstrap(ExpectedAbi::dsp(), Path::new(&build.artifact_path))
        .expect("bootstrap");
    (host, build)
}

fn constant_block(value: f32, frames: usize) -> Vec<f32> {
    vec![value; frames]
}

#[test]
fn a_gain_patch_replaces_the_processing_at_a_block_boundary() {
    let dir = work("dsp-gain");
    let (host, v1) = host(&dir, "gain_v1.rs");
    let mut state = DspState::default();
    let mut out = vec![0f32; 128];

    host.process_rt(&constant_block(1.0, 128), &mut out, &mut state);
    assert!(out.iter().all(|s| (*s - 0.5).abs() < 1e-6), "v1 gain is 0.5");
    assert_eq!(host.active_verification(), VerificationStatus::Assumed);

    let v2 = build(&dir, "gain_v2.rs");
    let evidence = block_evidence(&v2).expect("evidence");
    assert_eq!(evidence.status, VerificationStatus::Tested);
    let prepared = prepare_block(Path::new(&v2.artifact_path), &ExpectedAbi::dsp()).expect("prepare");
    let report = host.apply(prepared, &v2, Some(&evidence), &v2.source_snapshot_id);
    assert_eq!(report.status, ravum_protocol::ActivationStatus::Activated, "{:?}", report.reject_reason);

    // The whole block uses exactly one version: no half-swapped buffer.
    host.process_rt(&constant_block(1.0, 128), &mut out, &mut state);
    assert!(
        out.iter().all(|s| (*s - 2.0).abs() < 1e-6),
        "v2 gain is 2.0 and must apply to the entire block"
    );
    assert_eq!(host.active_patch_id(), "dsp-gain-v2");
    assert_eq!(host.status().activations, 1);
}

#[test]
fn an_incompatible_dsp_abi_is_refused_before_any_call() {
    let dir = work("dsp-abi");
    let (host, _) = host(&dir, "gain_v1.rs");
    let bad = build(&dir, "abi_mismatch.rs");

    let reason = prepare_block(Path::new(&bad.artifact_path), &ExpectedAbi::dsp()).unwrap_err();
    assert!(
        matches!(reason, RejectReason::AbiMismatch { expected: 2, found: 1 }),
        "unexpected: {reason:?}"
    );

    let mut out = vec![0f32; 64];
    host.process_rt(&constant_block(1.0, 64), &mut out, &mut DspState::default());
    assert!(out.iter().all(|s| (*s - 0.5).abs() < 1e-6), "v1 still active");
}

#[test]
fn a_scalar_module_cannot_be_loaded_as_a_dsp_module() {
    let dir = work("dsp-scalar");
    let source = root().join("fixtures/calculator/v1.rs");
    let snapshot = SourceSnapshot::from_file(&source).expect("fixture");
    let adapter = RustAdapter::new("rustc", dir.join("work"));
    let build = adapter
        .build_cdylib(&snapshot, &dir.join("artifacts"))
        .expect("adapter")
        .build;

    let reason = prepare_block(Path::new(&build.artifact_path), &ExpectedAbi::dsp()).unwrap_err();
    assert!(
        matches!(reason, RejectReason::AbiMismatch { expected: 2, found: 1 }),
        "unexpected: {reason:?}"
    );
}

#[test]
fn compatible_iir_swap_keeps_the_host_owned_state_and_the_signal_continuous() {
    let dir = work("dsp-iir");
    let (host, v1) = host(&dir, "iir_v1.rs");
    let mut state = DspState::default();
    let mut out = vec![0f32; 64];

    // Settle the one-pole filter on a constant input.
    let mut last = 0f32;
    for _ in 0..40 {
        host.process_rt(&constant_block(1.0, 64), &mut out, &mut state);
        last = out[63];
    }
    let state_before = state;
    assert!(last > 0.98, "filter should be settled, got {last}");

    let v2 = build(&dir, "iir_v2.rs");
    let evidence = block_evidence(&v2).expect("evidence");
    let prepared = prepare_block(Path::new(&v2.artifact_path), &ExpectedAbi::dsp()).expect("prepare");
    let report = host.apply(prepared, &v2, Some(&evidence), &v2.source_snapshot_id);
    assert_eq!(report.status, ravum_protocol::ActivationStatus::Activated, "{:?}", report.reject_reason);

    // The host owns the state, so the swap cannot reset it.
    assert_eq!(state, state_before);

    host.process_rt(&constant_block(1.0, 64), &mut out, &mut state);
    let first_after = out[0];
    let jump = (first_after - last).abs();
    assert!(
        jump < 0.05,
        "a compatible swap must not click: last={last}, first_after={first_after}"
    );
    // And the new coefficient is really in use: 0.5*1 + 0.5*z1, not 0.5*1.
    let expected = 0.5 * 1.0 + 0.5 * state_before.z1;
    assert!((first_after - expected).abs() < 1e-5, "got {first_after}, expected {expected}");
}

#[test]
fn the_block_path_does_not_allocate() {
    let dir = work("dsp-alloc");
    let (host, _) = host(&dir, "iir_v1.rs");
    let host = Arc::new(host);
    let audio = Arc::clone(&host);

    let allocations = std::thread::spawn(move || {
        let mut state = DspState::default();
        let input = constant_block(0.25, 256);
        let mut output = vec![0f32; 256];
        // Warm up (first iteration may touch TLS).
        audio.process_rt(&input, &mut output, &mut state);
        reset_thread_counter();
        for _ in 0..100 {
            audio.process_rt(&input, &mut output, &mut state);
        }
        allocations_on_this_thread()
    })
    .join()
    .expect("audio thread");

    assert_eq!(allocations, 0, "the audio path must not allocate");
}

#[test]
fn an_in_flight_block_keeps_the_retired_module_alive() {
    let dir = work("dsp-inflight");
    let (host, v1) = host(&dir, "gain_v1.rs");
    // This is the "in-flight" reference: the audio thread holds it for one block.
    let held = host.enter();
    let weak = Arc::downgrade(&held);
    assert_eq!(
        held.manifest.patch_id, "dsp-gain-v1",
        "the held reference is the first version"
    );

    let v2 = build(&dir, "gain_v2.rs");
    let evidence = block_evidence(&v2).expect("evidence");
    let prepared = prepare_block(Path::new(&v2.artifact_path), &ExpectedAbi::dsp()).expect("prepare");
    host.apply(prepared, &v2, Some(&evidence), &v2.source_snapshot_id);

    let mut out = vec![0f32; 32];
    let mut state = DspState::default();
    held.process_block(&constant_block(1.0, 32), &mut out, &mut state);
    assert!(
        out.iter().all(|s| (*s - 0.5).abs() < 1e-6),
        "the in-flight block still runs v1"
    );
    drop(held);

    let deadline = Instant::now() + Duration::from_secs(5);
    while weak.upgrade().is_some() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        weak.upgrade().is_none(),
        "the retired module must be freed after the in-flight block ends"
    );
}

#[test]
fn republishing_the_active_module_is_refused() {
    let dir = work("dsp-redundant");
    let (host, build) = host(&dir, "gain_v1.rs");
    let evidence = block_evidence(&build).expect("evidence");
    let prepared = prepare_block(Path::new(&build.artifact_path), &ExpectedAbi::dsp()).expect("prepare");
    let report = host.apply(prepared, &build, Some(&evidence), &build.source_snapshot_id);
    assert!(matches!(
        report.reject_reason,
        Some(RejectReason::AlreadyActive { .. })
    ));
    assert_eq!(host.status().activations, 0);
}

/// Evidence for a DSP module must never claim more than a single fixed block.
#[test]
fn dsp_evidence_is_explicitly_bounded() {
    let dir = work("dsp-evidence");
    let build = build(&dir, "gain_v1.rs");
    let evidence = block_evidence(&build).expect("evidence");
    assert_eq!(evidence.status, VerificationStatus::Tested);
    assert!(evidence.passed);
    assert!(evidence
        .assumptions
        .iter()
        .any(|a| a.contains("single fixed 64-frame block")));
    assert!(evidence.authorizes_activation());
}
