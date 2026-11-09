//! MVP-0b / L4-B — crossfaded block swap.
//!
//! MVP-0a measured two real clicks: `max_sample_delta` 0.108 for an IIR coefficient
//! change and 0.657 for a gain doubling (baseline sinusoid: 0.029). This test measures
//! the same swap with the host crossfade armed and asserts that the discontinuity drops,
//! without adding an allocation or a deallocation to the callback.
//!
//! Honest scope: this is a block simulator. There is no audio device, no real-time
//! priority, and no psychoacoustic threshold claim: the metric is sample-to-sample
//! discontinuity of the rendered signal, compared before/after on the same signal.

use ravum_dsp_harness::{
    allocations_on_this_thread, block_evidence, deallocations_on_this_thread, measure_from,
    reset_thread_counters, SessionConfig, SessionOutcome,
};
use ravum_patch_host::{
    prepare_block, wait_until_idle, BlockHost, ExpectedAbi, DEFAULT_CROSSFADE_FRAMES,
};
use ravum_protocol::{ActivationStatus, BuildOutput, DspState, EvidenceRecord};
use ravum_rust_adapter::RustAdapter;
use ravum_snapshot::SourceSnapshot;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Builds and module counters are process-wide; the crossfade tests each build their own
/// fixtures, so they run sequentially to avoid measuring machine contention instead of
/// the code (and to keep exact `live_modules()` assertions meaningful).
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn work(name: &str) -> PathBuf {
    let dir = root().join("target/ravum-tests").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("work dir");
    dir
}

struct Built {
    build: BuildOutput,
    evidence: EvidenceRecord,
}

fn build(dir: &Path, fixture: &str) -> Built {
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
    Built {
        evidence: block_evidence(&outcome.build).expect("evidence"),
        build: outcome.build,
    }
}

fn swap(host: &BlockHost, candidate: &Built) {
    let prepared = prepare_block(
        Path::new(&candidate.build.artifact_path),
        &ExpectedAbi::dsp(),
    )
    .expect("prepare");
    let report = host.apply(
        prepared,
        &candidate.build,
        Some(&candidate.evidence),
        &candidate.build.source_snapshot_id,
    );
    assert_eq!(
        report.status,
        ActivationStatus::Activated,
        "swap refused: {:?}",
        report.reject_reason
    );
}

fn config(blocks: u64) -> SessionConfig {
    SessionConfig {
        blocks,
        frames: 128,
        ..SessionConfig::default()
    }
}

/// Render a session, swap, then render the continuation from the same state and signal
/// so `max_sample_delta` really covers the swap boundary.
fn measure_swap_boundary(host: &BlockHost, candidate: &Built, blocks: u64) -> (SessionOutcome, SessionOutcome) {
    let before = measure_from(config(blocks), DspState::default(), 0.0, |input, output, state| {
        host.process_rt(input, output, state);
    });
    swap(host, candidate);
    let continued = SessionConfig {
        start_sample: config(blocks).blocks * config(blocks).frames as u64,
        ..config(blocks)
    };
    let after = measure_from(continued, before.final_state, before.last_sample, |input, output, state| {
        host.process_rt(input, output, state);
    });
    (before, after)
}

/// Only used to make the baseline value explicit in the assertions.
fn baseline_delta(blocks: u64) -> f32 {
    let config = config(blocks);
    let omega = 2.0 * std::f32::consts::PI * config.frequency_hz / config.sample_rate as f32;
    2.0 * config.amplitude * (omega / 2.0).sin().abs()
}

#[test]
fn a_crossfaded_gain_swap_removes_the_click() {
    let _serial = serial();
    let dir = work("crossfade-gain");
    let v1 = build(&dir, "gain_v1.rs");
    let v2 = build(&dir, "gain_v2.rs");

    // Sample several signal phases: the click magnitude depends on where the block
    // boundary falls in the sine, so a single phase would make the test flaky.
    let configs = [37u64, 80, 400];
    let mut immediate_worst = 0f32;
    let mut faded_worst = 0f32;
    for blocks in configs {
        let immediate = BlockHost::bootstrap(ExpectedAbi::dsp(), Path::new(&v1.build.artifact_path))
            .expect("bootstrap");
        let faded = BlockHost::bootstrap_with_crossfade(
            ExpectedAbi::dsp(),
            Path::new(&v1.build.artifact_path),
            DEFAULT_CROSSFADE_FRAMES,
        )
        .expect("bootstrap");
        assert_eq!(immediate.crossfade_frames(), 0);
        assert_eq!(faded.crossfade_frames(), DEFAULT_CROSSFADE_FRAMES);
        let (_, immediate_after) = measure_swap_boundary(&immediate, &v2, blocks);
        let (_, faded_after) = measure_swap_boundary(&faded, &v2, blocks);
        immediate_worst = immediate_worst.max(immediate_after.measurement.max_sample_delta);
        faded_worst = faded_worst.max(faded_after.measurement.max_sample_delta);
        assert_eq!(immediate_after.measurement.allocations, 0);
        assert_eq!(faded_after.measurement.deallocations, 0);
    }
    let baseline = baseline_delta(400);

    eprintln!(
        "[l4b] gain 0.5->2.0: immediate_worst={immediate_worst:.4} crossfaded_worst={faded_worst:.4} baseline={baseline:.4}"
    );

    // The MVP-0a measurement was 0.657: the click must be visible in this harness,
    // otherwise the comparison below would be meaningless.
    assert!(
        immediate_worst > 0.3,
        "the immediate swap should still show its click, worst was {immediate_worst}"
    );
    assert!(
        faded_worst < immediate_worst / 5.0,
        "crossfade did not reduce the gain click: immediate={immediate_worst} crossfaded={faded_worst}"
    );
    // A linear gain ramp adds at most ~|input| * dg/dn on top of the sine baseline.
    assert!(
        faded_worst < baseline * 4.0,
        "crossfaded discontinuity {faded_worst} above {} (4x baseline)",
        baseline * 4.0
    );
}

#[test]
fn a_crossfaded_iir_swap_removes_the_click() {
    let _serial = serial();
    let dir = work("crossfade-iir");
    let v1 = build(&dir, "iir_v1.rs");
    let v2 = build(&dir, "iir_v2.rs");

    let configs = [37u64, 80, 400];
    let mut immediate_worst = 0f32;
    let mut faded_worst = 0f32;
    for blocks in configs {
        let immediate = BlockHost::bootstrap(ExpectedAbi::dsp(), Path::new(&v1.build.artifact_path))
            .expect("bootstrap");
        let faded = BlockHost::bootstrap_with_crossfade(
            ExpectedAbi::dsp(),
            Path::new(&v1.build.artifact_path),
            DEFAULT_CROSSFADE_FRAMES,
        )
        .expect("bootstrap");
        let (_, immediate_after) = measure_swap_boundary(&immediate, &v2, blocks);
        let (_, faded_after) = measure_swap_boundary(&faded, &v2, blocks);
        immediate_worst = immediate_worst.max(immediate_after.measurement.max_sample_delta);
        faded_worst = faded_worst.max(faded_after.measurement.max_sample_delta);
        assert_eq!(faded_after.measurement.allocations, 0);
        assert_eq!(faded_after.measurement.deallocations, 0);
    }
    let baseline = baseline_delta(400);

    eprintln!(
        "[l4b] iir coefficient change: immediate_worst={immediate_worst:.4} crossfaded_worst={faded_worst:.4} baseline={baseline:.4}"
    );

    assert!(
        immediate_worst > 0.05,
        "the immediate swap should still show its click, worst was {immediate_worst}"
    );
    assert!(
        faded_worst < immediate_worst,
        "crossfade did not reduce the IIR coefficient click: immediate={immediate_worst} crossfaded={faded_worst}"
    );
    assert!(
        faded_worst < baseline * 4.0,
        "crossfaded IIR discontinuity {faded_worst} above {} (4x baseline)",
        baseline * 4.0
    );
}

#[test]
fn the_crossfade_path_does_not_allocate_or_free_on_the_audio_thread() {
    let _serial = serial();
    let dir = work("crossfade-alloc");
    let v1 = build(&dir, "gain_v1.rs");
    let v2 = build(&dir, "gain_v2.rs");
    let host = BlockHost::bootstrap_with_crossfade(
        ExpectedAbi::dsp(),
        Path::new(&v1.build.artifact_path),
        DEFAULT_CROSSFADE_FRAMES,
    )
    .expect("bootstrap");

    let stop = AtomicBool::new(false);
    let swaps = std::sync::atomic::AtomicU64::new(0);

    let (allocations, deallocations) = std::thread::scope(|scope| {
        let audio = scope.spawn(|| {
            let mut state = DspState::default();
            let mut input = vec![1.0f32; 256];
            let mut output = vec![0.0f32; 256];
            host.process_rt(&input, &mut output, &mut state);
            reset_thread_counters();
            while !stop.load(Ordering::SeqCst) {
                host.process_rt(&input, &mut output, &mut state);
                std::hint::black_box(output[0]);
            }
            input.clear();
            (allocations_on_this_thread(), deallocations_on_this_thread())
        });

        // Swap back and forth so several crossfades are rendered on the audio thread.
        for round in 0..8 {
            let candidate = if round % 2 == 0 { &v2 } else { &v1 };
            swap(&host, candidate);
            swaps.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(2));
        }
        stop.store(true, Ordering::SeqCst);
        audio.join().expect("audio thread")
    });

    assert_eq!(swaps.load(Ordering::SeqCst), 8);
    assert_eq!(allocations, 0, "the crossfade allocated on the audio thread");
    assert_eq!(deallocations, 0, "the crossfade freed on the audio thread");

    drop(host);
    assert!(wait_until_idle(Duration::from_secs(20)));
}

#[test]
fn a_second_swap_during_a_fade_keeps_the_host_sound_and_settles() {
    let _serial = serial();
    let dir = work("crossfade-double-swap");
    let v1 = build(&dir, "gain_v1.rs");
    let v2 = build(&dir, "gain_v2.rs");
    let host = BlockHost::bootstrap_with_crossfade(
        ExpectedAbi::dsp(),
        Path::new(&v1.build.artifact_path),
        DEFAULT_CROSSFADE_FRAMES,
    )
    .expect("bootstrap");

    let mut state = DspState::default();
    let input = vec![0.25f32; 64];
    let mut output = vec![0.0f32; 64];

    swap(&host, &v2);
    assert_eq!(host.active_fade_remaining(), DEFAULT_CROSSFADE_FRAMES);
    for _ in 0..2 {
        host.process_rt(&input, &mut output, &mut state);
    }
    // The fade is still in progress, but a new publication must not corrupt anything.
    swap(&host, &v1);
    assert_eq!(host.active_fade_remaining(), DEFAULT_CROSSFADE_FRAMES);

    for _ in 0..8 {
        host.process_rt(&input, &mut output, &mut state);
        assert!(output.iter().all(|s| s.is_finite()), "non-finite output");
    }
    assert_eq!(
        host.active_fade_remaining(),
        0,
        "the fade must reach its end after enough frames"
    );
    assert_eq!(host.active_patch_id(), "dsp-gain-v1");
    assert_eq!(host.status().activations, 2);
    drop(host);
    assert!(wait_until_idle(Duration::from_secs(20)));
}

#[test]
fn no_crossfade_is_the_default_and_keeps_the_immediate_switch() {
    let _serial = serial();
    let dir = work("crossfade-default");
    let v1 = build(&dir, "gain_v1.rs");
    let v2 = build(&dir, "gain_v2.rs");
    let host = BlockHost::bootstrap(ExpectedAbi::dsp(), Path::new(&v1.build.artifact_path))
        .expect("bootstrap");
    swap(&host, &v2);
    assert_eq!(host.active_fade_remaining(), 0, "default must not fade");
    let mut state = DspState::default();
    let mut output = vec![0.0f32; 64];
    host.process_rt(&vec![1.0f32; 64], &mut output, &mut state);
    assert!(
        output.iter().all(|s| (*s - 2.0).abs() < 1e-6),
        "the immediate switch semantics are preserved by default"
    );
    drop(host);
    assert!(wait_until_idle(Duration::from_secs(20)));
}
