//! MVP-0c / L4.3 — DSP stress campaign: mass swaps, several instances, variable
//! buffers, stop/start, destruction during closure, worker stop during a swap, module
//! saturation and publication/render races.
//!
//! Honest scope, stated up front:
//! * these tests run **in memory** on the real block host (`ravum-patch-host`) with the
//!   real `rustc`-built fixtures; there is **no audio device**, no real-time priority,
//!   no XRUN measurement and no hard real-time qualification;
//! * the allocation counters are the instrumented global allocator of
//!   `ravum-dsp-harness`; a zero count proves no allocation happened on the measured
//!   thread during that window, nothing more;
//! * module-load/unload counters are **process-wide**: every scenario that asserts on
//!   them takes a global lock, so tests in this binary run one at a time (deterministic
//!   counters instead of a flaky parallel measurement);
//! * the reference machine is shared with other agents: no test asserts a throughput,
//!   only invariants and generous duration bounds (each scenario < 60 s).
//!
//! Evidence: run with `RAVUM_STRESS_EVIDENCE=<absolute json path>` to execute the full
//! campaign a second time and write durations/counters/RSS, see `docs/mvp/MVP-0c-dsp-stress.md`.

use ravum_dsp_harness::{
    allocations_on_this_thread, block_evidence, deallocations_on_this_thread, measure_from,
    reset_thread_counters, SessionConfig, SessionOutcome,
};
use ravum_patch_host::{
    live_modules, pending_reclamations, prepare_block, unloaded_modules, wait_until_idle,
    BlockHost, ExpectedAbi, PreparedBlock,
};
use ravum_protocol::{ActivationStatus, BuildOutput, DspState, EvidenceRecord};
use ravum_rust_adapter::RustAdapter;
use ravum_snapshot::SourceSnapshot;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Hard bound for one scenario: a scenario that runs longer fails its test.
const MAX_SCENARIO_SECONDS: u64 = 60;

/// Per-scenario time budget. The default (60 s) is the local performance guard.
/// Shared CI runners are 3-10x slower under contention, so the Linux
/// qualification workflow raises the bound explicitly instead of hiding the
/// stress tests. This changes the timing envelope, never the correctness
/// assertions.
fn scenario_budget_seconds() -> u64 {
    std::env::var("RAVUM_STRESS_MAX_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(MAX_SCENARIO_SECONDS)
}

/// Global lock: module counters and the reclamation queue are process-wide, and the
/// scenarios assert exact transitions, so they are serialized. A poisoned lock is still
/// usable (the failing scenario already reported its own panic).
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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
    let snapshot = SourceSnapshot::from_file(&source).expect("fixture source");
    let adapter = RustAdapter::new("rustc", dir.join("work"));
    let outcome = adapter
        .build_cdylib(&snapshot, &dir.join("artifacts"))
        .expect("rustc adapter");
    assert!(
        outcome.success,
        "{fixture} must compile: {:?}",
        outcome.build.diagnostics
    );
    let evidence = block_evidence(&outcome.build).expect("block evidence");
    Built {
        build: outcome.build,
        evidence,
    }
}

/// Build a test-generated Rust source (written under 	arget/), through the same
/// RustAdapter/rustc path as the repository fixtures. No repository file is added.
fn build_source(dir: &Path, name: &str, source: &str) -> Built {
    std::fs::create_dir_all(dir).expect("source dir");
    let source_path = dir.join(format!("{name}.rs"));
    std::fs::write(&source_path, source).expect("write generated source");
    let snapshot = SourceSnapshot::from_file(&source_path).expect("generated source");
    let adapter = RustAdapter::new("rustc", dir.join("work"));
    let outcome = adapter
        .build_cdylib(&snapshot, &dir.join("artifacts"))
        .expect("rustc adapter");
    assert!(
        outcome.success,
        "{name} must compile: {:?}",
        outcome.build.diagnostics
    );
    let evidence = block_evidence(&outcome.build).expect("block evidence");
    Built {
        build: outcome.build,
        evidence,
    }
}

/// A minimal `no_std` gain module: same checked ABI and same lifecycle as the fixtures,
/// but ~9 KiB instead of ~102 KiB. The 10 000-swap loop's dominant cost is the mandatory
/// artifact hash inside prepare_block (31 s of the 32 s measured with the fixtures on
/// the reference machine, debug sha2); using a smaller real module keeps 10 000 real
/// publications inside the CI budget without changing the publication/validation path.
fn mini_source(patch_id: &str, gain: f32) -> String {
    format!(
        r#"#![no_std]
use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {{
    loop {{}}
}}

const MANIFEST: &[u8] = b"{{\"schema_version\":1,\"patch_id\":\"{patch_id}\",\"export\":\"ravum_process_block\",\"signature\":\"block:f32->f32+state:v1\",\"author\":\"test\"}}\0";

#[no_mangle]
pub extern "C" fn ravum_abi_version() -> u32 {{
    2
}}

#[no_mangle]
pub extern "C" fn ravum_manifest_v1() -> *const core::ffi::c_char {{
    MANIFEST.as_ptr() as *const core::ffi::c_char
}}

#[no_mangle]
pub extern "C" fn ravum_process_block(
    input: *const f32,
    output: *mut f32,
    frames: u32,
    _state: *mut core::ffi::c_void,
) {{
    let n = frames as usize;
    for i in 0..n {{
        unsafe {{
            *output.add(i) = *input.add(i) * {gain}f32;
        }}
    }}
}}
"#
    )
}

struct MiniFixtures {
    v1: Built,
    v2: Built,
}

static MINI_FIXTURES: OnceLock<MiniFixtures> = OnceLock::new();

fn mini_fixtures() -> &'static MiniFixtures {
    MINI_FIXTURES.get_or_init(|| {
        let dir = work(&format!("dsp-stress-mini-{}", std::process::id()));
        MiniFixtures {
            v1: build_source(&dir, "mini_v1", &mini_source("dsp-mini-v1", 0.5)),
            v2: build_source(&dir, "mini_v2", &mini_source("dsp-mini-v2", 2.0)),
        }
    })
}

struct Fixtures {
    gain_v1: Built,
    gain_v2: Built,
    iir_v1: Built,
    iir_v2: Built,
}

static FIXTURES: OnceLock<Fixtures> = OnceLock::new();

/// Build the four real fixture modules once per test binary (rustc builds, not mocks).
fn fixtures() -> &'static Fixtures {
    FIXTURES.get_or_init(|| {
        let dir = work(&format!("dsp-stress-mvp0c-{}", std::process::id()));
        Fixtures {
            gain_v1: build(&dir, "gain_v1.rs"),
            gain_v2: build(&dir, "gain_v2.rs"),
            iir_v1: build(&dir, "iir_v1.rs"),
            iir_v2: build(&dir, "iir_v2.rs"),
        }
    })
}

/// Keep the native images mapped for the whole scenario.
///
/// A cold LoadLibrary of a fresh DLL dominates the cost of a publication (Windows
/// loader + security scanner), so a 10 000-swap loop that reloaded the image every time
/// would measure the disk/loader path instead of the swap path. Holding one prepared
/// module per candidate keeps the image mapped (refcount) while each iteration still
/// prepares, publishes and retires a real module: the swap/render path is what is
/// measured. Cold mapping has its own measured scenario (scenario_module_saturation).
fn warm_images(fx: &Fixtures) -> Vec<PreparedBlock> {
    vec![prepare(&fx.gain_v1), prepare(&fx.gain_v2)]
}
fn artifact_path(built: &Built) -> &Path {
    Path::new(&built.build.artifact_path)
}

fn prepare(built: &Built) -> PreparedBlock {
    prepare_block(artifact_path(built), &ExpectedAbi::dsp()).expect("prepare_block")
}

/// Publish a fixture without asserting: concurrent publishers may race and see the
/// candidate already active, which is a documented rejection, not a failure.
fn try_apply(host: &BlockHost, built: &Built) -> ravum_protocol::ActivationReport {
    host.apply(
        prepare(built),
        &built.build,
        Some(&built.evidence),
        &built.build.source_snapshot_id,
    )
}

/// Publish a fixture and assert that the host accepted it (sequential scenarios).
fn apply(host: &BlockHost, built: &Built) -> ravum_protocol::ActivationReport {
    let report = try_apply(host, built);
    assert_eq!(
        report.status,
        ActivationStatus::Activated,
        "swap refused: {:?}",
        report.reject_reason
    );
    report
}

/// Drain the deferred unloader and return the current process-wide live-module count.
fn clean_baseline() -> u64 {
    assert!(
        wait_until_idle(Duration::from_secs(30)),
        "the reclaimer did not drain before the baseline"
    );
    live_modules()
}

fn sine(frames: usize, start: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| {
            let phase = ((start + i) as f32) * 0.028_8;
            phase.sin() * 0.5
        })
        .collect()
}

/// One measured block: reset this thread's counters, process, read them back.
/// The counters are thread-local, so this is safe even when tests run in parallel.
fn process_measured(
    host: &BlockHost,
    input: &[f32],
    output: &mut [f32],
    state: &mut DspState,
) -> (u64, u64) {
    reset_thread_counters();
    let frames = host.process_rt(input, output, state);
    assert_eq!(frames, input.len(), "process_rt must process every frame");
    (allocations_on_this_thread(), deallocations_on_this_thread())
}

fn assert_finite_and_bounded(output: &[f32], bound: f32, context: &str) {
    for sample in output {
        assert!(sample.is_finite(), "{context}: non-finite sample {sample}");
        assert!(
            sample.abs() <= bound,
            "{context}: sample {sample} above bound {bound}"
        );
    }
}

#[cfg(windows)]
mod process_memory {
    //! Working-set measurement without adding a dependency (Windows only, like the rest
    //! of the native pipeline). On other platforms the evidence simply reports `null`.
    use std::ffi::c_void;

    #[repr(C)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn K32GetProcessMemoryInfo(
            process: *mut c_void,
            counters: *mut ProcessMemoryCounters,
            cb: u32,
        ) -> i32;
        fn GetCurrentProcess() -> *mut c_void;
    }

    pub fn working_set_bytes() -> Option<u64> {
        let mut counters: ProcessMemoryCounters = unsafe { std::mem::zeroed() };
        counters.cb = std::mem::size_of::<ProcessMemoryCounters>() as u32;
        // SAFETY: current process pseudo-handle, valid counters buffer.
        let ok =
            unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) };
        if ok != 0 {
            Some(counters.working_set_size as u64)
        } else {
            None
        }
    }
}

#[cfg(not(windows))]
mod process_memory {
    pub fn working_set_bytes() -> Option<u64> {
        None
    }
}

/// Progress watchdog: aborts the test process **only when no observable progress happened
/// for the configured stall**. A slow machine keeps making progress (blocks rendered,
/// modules activated) and is never killed; a genuine deadlock stops the counter and is
/// turned into a fast, visible failure. This replaces a former wall-clock watchdog that
/// killed a slow-but-progressing run on the 4 vCPU CI runner.
struct ProgressWatchdog {
    done: Arc<AtomicBool>,
}

impl ProgressWatchdog {
    fn start(what: &'static str, stall: Duration, progress: Arc<AtomicU64>) -> Self {
        let done = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&done);
        std::thread::spawn(move || {
            let mut last_value = progress.load(Ordering::SeqCst);
            let mut last_change = Instant::now();
            while !flag.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(100));
                let value = progress.load(Ordering::SeqCst);
                if value != last_value {
                    last_value = value;
                    last_change = Instant::now();
                } else if last_change.elapsed() >= stall {
                    eprintln!(
                        "[dsp-stress] WATCHDOG: {what} made no progress for {}s (counter={value}): aborting",
                        stall.as_secs()
                    );
                    std::process::abort();
                }
            }
        });
        Self { done }
    }
}

impl Drop for ProgressWatchdog {
    fn drop(&mut self) {
        self.done.store(true, Ordering::SeqCst);
    }
}

fn progress_counter() -> Arc<AtomicU64> {
    Arc::new(AtomicU64::new(0))
}

/// Ensures the render loop stops even if a publisher thread panics (no infinite join).
struct StopOnDrop<'a>(&'a AtomicBool);

impl Drop for StopOnDrop<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// One executed scenario, suitable for the evidence JSON.
struct Scenario {
    name: &'static str,
    test: &'static str,
    duration_ms: u64,
    facts: Value,
}

impl Scenario {
    fn record(name: &'static str, test: &'static str, started: Instant, facts: Value) -> Scenario {
        let duration_ms = started.elapsed().as_millis() as u64;
        let budget = scenario_budget_seconds();
        assert!(
            duration_ms < budget * 1000,
            "{name} took {duration_ms} ms, above the {budget} s bound"
        );
        eprintln!("[dsp-stress] {test}: {duration_ms} ms");
        Scenario {
            name,
            test,
            duration_ms,
            facts,
        }
    }
}

/// Size knobs shared by the tests and the evidence campaign. `full()` is what the tests
/// execute; `compact()` is the non-empty smoke used when no evidence path is given.
#[derive(Clone, Copy)]
struct Scale {
    swaps: usize,
    iterations: usize,
    cycles: usize,
    modules: usize,
    race_activations: usize,
}

impl Scale {
    const fn full() -> Self {
        Self {
            swaps: 10_000,
            iterations: 500,
            cycles: 50,
            modules: 20,
            race_activations: 2_000,
        }
    }

    const fn compact() -> Self {
        Self {
            swaps: 200,
            iterations: 25,
            cycles: 5,
            modules: 4,
            race_activations: 100,
        }
    }
}

// ---------------------------------------------------------------------------------------
// Scenario 1 — 10 000 swaps during continuous rendering.
// ---------------------------------------------------------------------------------------

fn scenario_ten_thousand_swaps(fx: &Fixtures, scale: Scale) -> Scenario {
    let started = Instant::now();
    let test = "ten_thousand_swaps_keep_the_callback_finite_alloc_free_and_one_version";
    // Test-generated minimal real modules (~9 KiB) for the 10 000-swap loop: the mandatory
    // artifact hash inside prepare_block dominates the cost with the 102 KiB fixtures
    // (measured 31 s / 32 s in debug). The publication/validation path is unchanged.
    let mini = mini_fixtures();
    let host =
        BlockHost::bootstrap(ExpectedAbi::dsp(), artifact_path(&mini.v1)).expect("bootstrap");
    let warm = vec![prepare(&mini.v1), prepare(&mini.v2)];
    let baseline = clean_baseline();
    let mut state = DspState::default();
    let mut input = sine(128, 0);
    let mut output = vec![0f32; 128];
    let mut non_finite = 0u64;
    let mut alloc_violations = 0u64;
    let mut free_violations = 0u64;
    let mut max_abs = 0f32;

    // Warm-up touches TLS and the module's lazy path before measuring.
    host.process_rt(&input, &mut output, &mut state);
    let mut prepare_us: u128 = 0;
    let mut apply_us: u128 = 0;
    for index in 0..scale.swaps {
        let candidate = if index % 2 == 0 { &mini.v2 } else { &mini.v1 };
        let prepare_started = Instant::now();
        let prepared = prepare(candidate);
        prepare_us += prepare_started.elapsed().as_micros();
        let apply_started = Instant::now();
        let report = host.apply(
            prepared,
            &candidate.build,
            Some(&candidate.evidence),
            &candidate.build.source_snapshot_id,
        );
        apply_us += apply_started.elapsed().as_micros();
        assert_eq!(report.status, ActivationStatus::Activated);
        let (allocations, deallocations) = process_measured(&host, &input, &mut output, &mut state);
        alloc_violations += (allocations > 0) as u64;
        free_violations += (deallocations > 0) as u64;
        if output.iter().any(|sample| !sample.is_finite()) {
            non_finite += 1;
        }
        for sample in &output {
            max_abs = max_abs.max(sample.abs());
        }
        // The rendered signal is a sine scaled by 0.5 or 2.0: it must stay bounded.
        assert_finite_and_bounded(&output, 1.01, "10k swaps");
        input = sine(128, index * 128);
    }
    assert_eq!(
        host.status().activations as usize,
        scale.swaps,
        "every prepared module must have been activated exactly once"
    );
    assert_eq!(non_finite, 0, "a block produced non-finite samples");
    assert_eq!(
        alloc_violations, 0,
        "process_rt allocated on the render thread"
    );
    assert_eq!(free_violations, 0, "process_rt freed on the render thread");

    // Publication while a reader guard is alive: ArcSlot::store waits for the reader
    // to leave before releasing the retired payload (documented writer behaviour), so the
    // publisher must run on another thread. The guard keeps the retired module mapped and
    // usable; the publication completes as soon as the guard is dropped.
    // Drain the asynchronous unloader first: otherwise a reclaimer thread could free an
    // older retired module between the snapshot and the assertion and make it flaky.
    assert!(
        wait_until_idle(Duration::from_secs(30)),
        "the reclaimer did not drain before the guard check"
    );
    let live_before_guard = live_modules();
    let progress = progress_counter();
    progress.fetch_add(1, Ordering::SeqCst);
    let watchdog = ProgressWatchdog::start(
        "publication while a reader guard is alive",
        Duration::from_secs(30),
        Arc::clone(&progress),
    );
    std::thread::scope(|scope| {
        let program = host.enter_rt();
        let retired_patch = program.current.manifest.patch_id.clone();
        let publisher = scope.spawn(|| {
            apply(&host, &fx.gain_v2);
            progress.fetch_add(1, Ordering::SeqCst);
        });
        let mut retired_out = vec![0f32; 64];
        program
            .current
            .process_block(&input[..64], &mut retired_out, &mut state);
        assert_finite_and_bounded(&retired_out, 1.01, "retired module through guard");
        assert!(
            live_modules() >= live_before_guard,
            "a module held by a reader guard must stay mapped"
        );
        assert!(!retired_patch.is_empty());
        drop(program);
        publisher.join().expect("publisher after guard release");
    });
    drop(watchdog);
    drop(warm);
    assert!(
        clean_baseline() <= baseline,
        "the retired module was not reclaimed"
    );

    // Crossfaded continuity on the same kind of swap: the fade must bound the click.
    // A fresh pair of hosts per phase: a publication of the same artifact twice would be
    // rejected with AlreadyActive (documented), and each phase is an independent swap.
    let mut immediate_worst = 0f32;
    let mut faded_worst = 0f32;
    for blocks in [37u64, 80, 400] {
        let immediate = BlockHost::bootstrap(ExpectedAbi::dsp(), artifact_path(&fx.gain_v1))
            .expect("bootstrap immediate");
        let faded = BlockHost::bootstrap_with_crossfade(
            ExpectedAbi::dsp(),
            artifact_path(&fx.gain_v1),
            ravum_patch_host::DEFAULT_CROSSFADE_FRAMES,
        )
        .expect("bootstrap faded");
        let config = SessionConfig {
            blocks,
            frames: 128,
            ..SessionConfig::default()
        };
        let render = |host: &BlockHost, swap: bool| -> SessionOutcome {
            let before = measure_from(config, DspState::default(), 0.0, |input, output, state| {
                host.process_rt(input, output, state);
            });
            if swap {
                apply(host, &fx.gain_v2);
            }
            let continued = SessionConfig {
                start_sample: blocks * config.frames as u64,
                ..config
            };
            measure_from(
                continued,
                before.final_state,
                before.last_sample,
                |input, output, state| {
                    host.process_rt(input, output, state);
                },
            )
        };
        immediate_worst =
            immediate_worst.max(render(&immediate, true).measurement.max_sample_delta);
        faded_worst = faded_worst.max(render(&faded, true).measurement.max_sample_delta);
    }
    let baseline_delta = {
        let omega = 2.0 * std::f32::consts::PI * SessionConfig::default().frequency_hz
            / SessionConfig::default().sample_rate as f32;
        2.0 * SessionConfig::default().amplitude * (omega / 2.0).sin().abs()
    };
    assert!(
        immediate_worst > 0.3,
        "the immediate gain swap should show its click, worst {immediate_worst}"
    );
    assert!(
        faded_worst < immediate_worst / 5.0,
        "crossfade did not reduce the click: immediate={immediate_worst} faded={faded_worst}"
    );
    assert!(
        faded_worst < baseline_delta * 4.0,
        "crossfaded discontinuity {faded_worst} above 4x baseline {}",
        baseline_delta * 4.0
    );

    eprintln!(
        "[dsp-stress] 10k split: prepare={prepare_us} us, apply={apply_us} us ({} swaps)",
        scale.swaps
    );
    Scenario::record(
        test,
        test,
        started,
        json!({
            "swaps": scale.swaps,
            "activations": host.status().activations,
            "active_version": host.active_version().0,
            "block_frames": 128,
            "module": "test-generated minimal no_std rustc modules (~9 KiB) for the swap loop; ABI, validation and reclamation path unchanged",
            "non_finite_blocks": non_finite,
            "allocation_violations": alloc_violations,
            "deallocation_violations": free_violations,
            "max_abs_output": max_abs,
            "prepare_us_total": prepare_us as u64,
            "apply_us_total": apply_us as u64,
            "live_modules_baseline": baseline,
            "live_modules_after_drain": clean_baseline(),
            "crossfade": {
                "frames": ravum_patch_host::DEFAULT_CROSSFADE_FRAMES,
                "immediate_worst_delta": immediate_worst,
                "faded_worst_delta": faded_worst,
                "sine_baseline_delta": baseline_delta,
            },
            "retired_module_guard_check": "module stayed mapped while the non-owning guard was alive",
        }),
    )
}

#[test]
fn ten_thousand_swaps_keep_the_callback_finite_alloc_free_and_one_version() {
    let _serial = serial();
    let scenario = scenario_ten_thousand_swaps(fixtures(), Scale::full());
    assert_eq!(scenario.facts["allocation_violations"], json!(0));
}

// ---------------------------------------------------------------------------------------
// Scenario 2 — two hosts in parallel threads, independent state and versions.
// ---------------------------------------------------------------------------------------

fn scenario_two_hosts_in_parallel(fx: &Fixtures, scale: Scale) -> Scenario {
    let started = Instant::now();
    let test = "two_hosts_in_parallel_keep_independent_state";
    let host_a =
        BlockHost::bootstrap(ExpectedAbi::dsp(), artifact_path(&fx.gain_v1)).expect("bootstrap A");
    let host_b =
        BlockHost::bootstrap(ExpectedAbi::dsp(), artifact_path(&fx.iir_v1)).expect("bootstrap B");
    let _warm_a = warm_images(fx);
    let _warm_b = vec![prepare(&fx.iir_v1), prepare(&fx.iir_v2)];
    let barrier = std::sync::Barrier::new(3);
    let iterations = scale.iterations;
    let (a_stats, b_stats) = std::thread::scope(|scope| {
        let a = scope.spawn(|| {
            barrier.wait();
            let mut state = DspState::default();
            let input = vec![1.0f32; 64];
            let mut output = vec![0f32; 64];
            let mut bad_values = 0u64;
            let mut allocations = 0u64;
            for index in 0..iterations {
                let candidate = if index % 2 == 0 {
                    &fx.gain_v2
                } else {
                    &fx.gain_v1
                };
                apply(&host_a, candidate);
                reset_thread_counters();
                host_a.process_rt(&input, &mut output, &mut state);
                allocations += allocations_on_this_thread();
                for sample in &output {
                    if !(*sample == 0.5 || *sample == 2.0) {
                        bad_values += 1;
                    }
                }
            }
            (bad_values, allocations, host_a.status().activations)
        });
        let b = scope.spawn(|| {
            barrier.wait();
            let mut state = DspState::default();
            let input = sine(64, 0);
            let mut output = vec![0f32; 64];
            let mut violations = 0u64;
            let mut max_abs = 0f32;
            for index in 0..iterations {
                let candidate = if index % 2 == 0 {
                    &fx.iir_v2
                } else {
                    &fx.iir_v1
                };
                apply(&host_b, candidate);
                host_b.process_rt(&input, &mut output, &mut state);
                for sample in &output {
                    if !sample.is_finite() || sample.abs() > 1.01 {
                        violations += 1;
                    }
                    max_abs = max_abs.max(sample.abs());
                }
            }
            (violations, max_abs, host_b.status().activations)
        });
        barrier.wait();
        (
            a.join().expect("host A thread"),
            b.join().expect("host B thread"),
        )
    });
    let (bad_values, allocations, activations_a) = a_stats;
    let (violations, max_abs, activations_b) = b_stats;
    assert_eq!(
        bad_values, 0,
        "host A produced a value from another host's module"
    );
    assert_eq!(allocations, 0, "host A render thread allocated");
    assert_eq!(violations, 0, "host B produced an invalid sample");
    assert_eq!(activations_a as usize, iterations);
    assert_eq!(activations_b as usize, iterations);

    Scenario::record(
        test,
        test,
        started,
        json!({
            "iterations_per_host": iterations,
            "host_a": {"fixtures": ["gain_v1", "gain_v2"], "bad_values": bad_values, "allocations": allocations, "activations": activations_a},
            "host_b": {"fixtures": ["iir_v1", "iir_v2"], "violations": violations, "max_abs_output": max_abs, "activations": activations_b},
        }),
    )
}

#[test]
fn two_hosts_in_parallel_keep_independent_state() {
    let _serial = serial();
    let scenario = scenario_two_hosts_in_parallel(fixtures(), Scale::full());
    assert_eq!(scenario.facts["host_a"]["bad_values"], json!(0));
}

// ---------------------------------------------------------------------------------------
// Scenario 3 — repeated stop/start of a render worker, host reused afterwards.
// ---------------------------------------------------------------------------------------

fn render_until_stop(
    host: &BlockHost,
    stop: &AtomicBool,
    blocks: &AtomicU64,
    bad: &AtomicU64,
    progress: &AtomicU64,
) {
    let mut state = DspState::default();
    let mut input = vec![0.5f32; 64];
    let mut output = vec![0f32; 64];
    while !stop.load(Ordering::SeqCst) {
        host.process_rt(&input, &mut output, &mut state);
        if output.iter().any(|sample| !sample.is_finite()) {
            bad.fetch_add(1, Ordering::SeqCst);
        }
        blocks.fetch_add(1, Ordering::SeqCst);
        progress.fetch_add(1, Ordering::SeqCst);
        input.rotate_left(1);
    }
    reset_thread_counters();
    let _ = allocations_on_this_thread();
    let _ = deallocations_on_this_thread();
}

fn scenario_stop_start_cycles(fx: &Fixtures, scale: Scale) -> Scenario {
    let started = Instant::now();
    let test = "stop_start_cycles_reuse_the_host_without_leaking_modules";
    let host =
        BlockHost::bootstrap(ExpectedAbi::dsp(), artifact_path(&fx.gain_v1)).expect("bootstrap");
    let warm = warm_images(fx);
    let baseline = clean_baseline();
    let progress = progress_counter();
    let watchdog = ProgressWatchdog::start(
        "stop/start cycles",
        Duration::from_secs(30),
        Arc::clone(&progress),
    );
    let mut total_blocks = 0u64;
    let mut render_allocations = 0u64;
    let mut render_deallocations = 0u64;
    let mut control_blocks = 0u64;
    for cycle in 0..scale.cycles {
        let stop = AtomicBool::new(false);
        let blocks = AtomicU64::new(0);
        let bad = AtomicU64::new(0);
        std::thread::scope(|scope| {
            let worker = scope.spawn(|| render_until_stop(&host, &stop, &blocks, &bad, &progress));
            // Let the worker render, then stop it while it is mid-loop.
            while blocks.load(Ordering::SeqCst) < 8 {
                std::thread::yield_now();
            }
            let candidate = if cycle % 2 == 0 {
                &fx.gain_v2
            } else {
                &fx.gain_v1
            };
            apply(&host, candidate);
            std::thread::sleep(Duration::from_millis(2));
            stop.store(true, Ordering::SeqCst);
            worker.join().expect("render worker");
            assert_eq!(bad.load(Ordering::SeqCst), 0);
            total_blocks += blocks.load(Ordering::SeqCst);
        });
        // Host reused after the worker stopped: one measured block on the control thread.
        let mut state = DspState::default();
        let input = vec![1.0f32; 64];
        let mut output = vec![0f32; 64];
        reset_thread_counters();
        host.process_rt(&input, &mut output, &mut state);
        render_allocations += allocations_on_this_thread();
        render_deallocations += deallocations_on_this_thread();
        control_blocks += 1;
        assert_finite_and_bounded(&output, 2.01, "control block after stop");
    }
    assert_eq!(render_allocations, 0, "render thread allocated");
    assert_eq!(render_deallocations, 0, "render thread freed");
    assert_eq!(
        host.status().activations as usize,
        scale.cycles,
        "one activation per cycle"
    );
    assert!(
        total_blocks >= 8 * scale.cycles as u64,
        "the worker never rendered enough blocks"
    );
    drop(warm);
    let live_after = clean_baseline();
    assert_eq!(
        live_after,
        baseline - 2,
        "module count drifted after cycles"
    );
    assert_eq!(pending_reclamations(), 0);
    drop(watchdog);

    Scenario::record(
        test,
        test,
        started,
        json!({
            "cycles": scale.cycles,
            "worker_blocks": total_blocks,
            "control_blocks": control_blocks,
            "activations": host.status().activations,
            "render_allocations": render_allocations,
            "render_deallocations": render_deallocations,
            "live_modules_baseline": baseline,
            "live_modules_after": live_after,
        }),
    )
}

#[test]
fn stop_start_cycles_reuse_the_host_without_leaking_modules() {
    let _serial = serial();
    let scenario = scenario_stop_start_cycles(fixtures(), Scale::full());
    assert_eq!(scenario.facts["render_allocations"], json!(0));
}

// ---------------------------------------------------------------------------------------
// Scenario 4 — variable block sizes, including during a version change and a fade.
// ---------------------------------------------------------------------------------------

fn scenario_variable_buffers(fx: &Fixtures, _scale: Scale) -> Scenario {
    let started = Instant::now();
    let test = "variable_block_sizes_are_defined_across_a_version_change";
    let host =
        BlockHost::bootstrap(ExpectedAbi::dsp(), artifact_path(&fx.gain_v1)).expect("bootstrap");
    let mut state = DspState::default();
    let sizes = [
        1usize, 7, 63, 64, 65, 127, 128, 129, 255, 256, 257, 511, 512, 513, 1024, 4096,
    ];
    let mut processed = 0u64;
    let mut zeros = 0u64;
    let input = vec![1.0f32; 4096];
    for (index, size) in sizes.iter().copied().enumerate() {
        if index == 4 {
            // Version change in the middle of the size sequence.
            apply(&host, &fx.gain_v2);
        }
        let mut output = vec![7.0f32; size];
        let frames = host.process_rt(&input[..size], &mut output, &mut state);
        assert_eq!(frames, size);
        processed += size as u64;
        if index == 2 || index == 6 {
            // Zero-frame block: defined as a no-op (no frame processed, output untouched).
            let mut untouched = vec![3.5f32; 8];
            assert_eq!(host.process_rt(&[], &mut untouched, &mut state), 0);
            assert_eq!(untouched, vec![3.5f32; 8]);
            zeros += 1;
        }
        // Gain 0.5 before the swap, 2.0 after: exact per version, no partial buffer.
        let expected = if index < 4 { 0.5 } else { 2.0 };
        for sample in &output {
            assert!(sample.is_finite());
            assert!(
                (sample - expected).abs() <= 1e-6,
                "block size {size}: {sample} != {expected} (mixed version?)"
            );
        }
    }

    // Crossfade with block sizes larger than MAX_FADE_FRAMES: the fade is chunked and the
    // output must stay inside the bounded mix of both gains, without allocation.
    let faded = BlockHost::bootstrap_with_crossfade(
        ExpectedAbi::dsp(),
        artifact_path(&fx.gain_v1),
        ravum_patch_host::DEFAULT_CROSSFADE_FRAMES,
    )
    .expect("bootstrap faded");
    let mut fade_state = DspState::default();
    let mut allocation_violations = 0u64;
    for size in [1000usize, 64, 2000, 128, 512, 333] {
        let mut output = vec![0f32; size];
        let (allocations, deallocations) =
            process_measured(&faded, &input[..size], &mut output, &mut fade_state);
        if allocations > 0 || deallocations > 0 {
            allocation_violations += 1;
        }
        assert_finite_and_bounded(&output, 2.01, "faded variable block");
    }
    apply(&faded, &fx.gain_v2);
    for size in [1000usize, 4096, 129] {
        let mut output = vec![0f32; size];
        let (allocations, deallocations) =
            process_measured(&faded, &input[..size], &mut output, &mut fade_state);
        if allocations > 0 || deallocations > 0 {
            allocation_violations += 1;
        }
        assert_finite_and_bounded(&output, 2.01, "faded variable block after swap");
        for sample in &output {
            assert!(
                *sample >= 0.5 - 1e-3 && *sample <= 2.0 + 1e-3,
                "crossfade output {sample} outside the [0.5, 2.0] envelope"
            );
        }
    }
    assert_eq!(allocation_violations, 0);

    Scenario::record(
        test,
        test,
        started,
        json!({
            "sizes": sizes.to_vec(),
            "frames_processed": processed,
            "zero_frame_blocks": zeros,
            "versions_before_after": ["dsp-gain-v1", "dsp-gain-v2"],
            "crossfade_large_blocks": [1000, 64, 2000, 128, 512, 333],
            "allocation_violations": allocation_violations,
            "defined_behaviour": "frames=0 is a no-op; any other size is processed as one block; the fade is chunked at MAX_FADE_FRAMES and mixed output stays inside the [0.5, 2.0] envelope",
        }),
    )
}

#[test]
fn variable_block_sizes_are_defined_across_a_version_change() {
    let _serial = serial();
    let scenario = scenario_variable_buffers(fixtures(), Scale::full());
    assert_eq!(scenario.facts["allocation_violations"], json!(0));
}

// ---------------------------------------------------------------------------------------
// Scenario 5 — drop of the host while / after the render worker stops.
// ---------------------------------------------------------------------------------------

fn scenario_drop_during_closure(fx: &Fixtures, scale: Scale) -> Scenario {
    let started = Instant::now();
    let test = "dropping_the_host_after_stop_never_destroys_on_the_render_thread";
    let baseline = clean_baseline();
    let unloaded_before = unloaded_modules();
    let progress = progress_counter();
    let watchdog = ProgressWatchdog::start(
        "drop during closure",
        Duration::from_secs(30),
        Arc::clone(&progress),
    );
    let (blocks, allocations, deallocations) = {
        let host = BlockHost::bootstrap_with_crossfade(
            ExpectedAbi::dsp(),
            artifact_path(&fx.gain_v1),
            ravum_patch_host::DEFAULT_CROSSFADE_FRAMES,
        )
        .expect("bootstrap");
        let stop = AtomicBool::new(false);
        let blocks = AtomicU64::new(0);
        std::thread::scope(|scope| {
            let worker = scope.spawn(|| {
                let mut state = DspState::default();
                let input = sine(256, 0);
                let mut output = vec![0f32; 256];
                // Warm-up, then measure only the render thread's own counters.
                host.process_rt(&input, &mut output, &mut state);
                reset_thread_counters();
                while !stop.load(Ordering::SeqCst) {
                    host.process_rt(&input, &mut output, &mut state);
                    if output.iter().any(|sample| !sample.is_finite()) {
                        panic!("non-finite sample on the render thread");
                    }
                    blocks.fetch_add(1, Ordering::SeqCst);
                    progress.fetch_add(1, Ordering::SeqCst);
                }
                (allocations_on_this_thread(), deallocations_on_this_thread())
            });
            apply(&host, &fx.gain_v2);
            for _ in 0..scale.iterations {
                std::hint::black_box(host.active_version());
            }
            stop.store(true, Ordering::SeqCst);
            let (allocations, deallocations) = worker.join().expect("render worker");
            (blocks.load(Ordering::SeqCst), allocations, deallocations)
        })
        // `host` is dropped here on the control thread, after the worker has joined.
    };
    assert_eq!(allocations, 0, "the render thread allocated");
    assert_eq!(
        deallocations, 0,
        "the render thread freed/destroyed a module"
    );
    assert!(
        wait_until_idle(Duration::from_secs(30)),
        "the reclaimer did not unload the host modules"
    );
    let live_after = live_modules();
    assert_eq!(live_after, baseline, "module leak after host drop");
    assert!(
        unloaded_modules() > unloaded_before,
        "the deferred unloader never ran"
    );
    assert_eq!(pending_reclamations(), 0);
    drop(watchdog);

    Scenario::record(
        test,
        test,
        started,
        json!({
            "worker_blocks": blocks,
            "render_allocations": allocations,
            "render_deallocations": deallocations,
            "live_modules_baseline": baseline,
            "live_modules_after_drop": live_after,
            "pending_reclamations_after": pending_reclamations(),
            "unloaded_before": unloaded_before,
            "unloaded_after": unloaded_modules(),
            "note": "destruction runs on the control thread and the deferred unloader; the render thread counters stay 0/0",
        }),
    )
}

#[test]
fn dropping_the_host_after_stop_never_destroys_on_the_render_thread() {
    let _serial = serial();
    let scenario = scenario_drop_during_closure(fixtures(), Scale::full());
    assert_eq!(scenario.facts["render_deallocations"], json!(0));
}

// ---------------------------------------------------------------------------------------
// Scenario 6 — worker stop during a swap: no deadlock (hard watchdog bound).
// ---------------------------------------------------------------------------------------

fn scenario_stop_during_swap(fx: &Fixtures, scale: Scale) -> Scenario {
    let started = Instant::now();
    let test = "stop_during_swaps_terminates_without_deadlock";
    let progress = progress_counter();
    let watchdog = ProgressWatchdog::start(
        "stop during swap",
        Duration::from_secs(30),
        Arc::clone(&progress),
    );
    let host =
        BlockHost::bootstrap(ExpectedAbi::dsp(), artifact_path(&fx.gain_v1)).expect("bootstrap");
    let _warm = warm_images(fx);
    let stop = AtomicBool::new(false);
    let blocks = AtomicU64::new(0);
    let bad = AtomicU64::new(0);
    let activations = std::thread::scope(|scope| {
        let _stop = StopOnDrop(&stop);
        let worker = scope.spawn(|| {
            render_until_stop(&host, &stop, &blocks, &bad, &progress);
        });
        let mut activations = 0u64;
        for index in 0..scale.iterations {
            let candidate = if index % 2 == 0 {
                &fx.gain_v2
            } else {
                &fx.gain_v1
            };
            apply(&host, candidate);
            activations += 1;
            progress.fetch_add(1, Ordering::SeqCst);
            if index % 7 == 0 {
                std::thread::yield_now();
            }
        }
        // Stop while the last publication may still be in flight.
        stop.store(true, Ordering::SeqCst);
        worker.join().expect("render worker");
        activations
    });
    drop(watchdog);
    assert_eq!(bad.load(Ordering::SeqCst), 0);
    assert!(
        blocks.load(Ordering::SeqCst) > 0,
        "the worker never rendered"
    );
    assert!(activations > 0);
    assert_eq!(host.status().activations, activations);

    Scenario::record(
        test,
        test,
        started,
        json!({
            "swaps": activations,
            "worker_blocks": blocks.load(Ordering::SeqCst),
            "non_finite_blocks": bad.load(Ordering::SeqCst),
            "deadlock": false,
            "hard_bound": "progress watchdog: aborts only if no block and no activation happen for 30 s",
        }),
    )
}

#[test]
fn stop_during_swaps_terminates_without_deadlock() {
    let _serial = serial();
    let scenario = scenario_stop_during_swap(fixtures(), Scale::full());
    assert_eq!(scenario.facts["non_finite_blocks"], json!(0));
}

// ---------------------------------------------------------------------------------------
// Scenario 7 — saturation: dozens of modules loaded, then reclaimed, bounded memory.
// ---------------------------------------------------------------------------------------

fn scenario_module_saturation(fx: &Fixtures, scale: Scale) -> Scenario {
    let started = Instant::now();
    let test = "saturation_dozens_of_modules_are_reclaimed_and_memory_stays_bounded";
    let baseline = clean_baseline();
    let rss_before = process_memory::working_set_bytes();
    let work_dir = root().join(format!(
        "target/ravum-tests/dsp-stress-mvp0c-{}/modules",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&work_dir);
    std::fs::create_dir_all(&work_dir).expect("modules dir");
    // Distinct file paths so every prepare is a distinct native module handle.
    let mut copied = Vec::new();
    for index in 0..scale.modules {
        let destination = work_dir.join(format!("gain-v1-{index:03}.dll"));
        std::fs::copy(artifact_path(&fx.gain_v1), &destination).expect("copy artifact");
        copied.push(destination);
    }
    let mut peak_live = baseline;
    let mut cycle_live = Vec::new();
    let mut cycles = 0u64;
    for _cycle in 0..3 {
        let mut prepared = Vec::with_capacity(copied.len());
        for path in &copied {
            prepared
                .push(prepare_block(path, &ExpectedAbi::dsp()).expect("prepare saturating module"));
        }
        peak_live = peak_live.max(live_modules());
        assert!(
            live_modules() >= baseline + copied.len() as u64 - 2,
            "not every prepared module is accounted as live"
        );
        drop(prepared);
        assert!(
            wait_until_idle(Duration::from_secs(30)),
            "the reclaimer did not drain the saturated batch"
        );
        let live = live_modules();
        cycle_live.push(live);
        assert_eq!(live, baseline, "module leak after a saturation cycle");
        assert_eq!(pending_reclamations(), 0);
        cycles += 1;
    }
    let rss_after = process_memory::working_set_bytes();
    let rss_delta_mib = match (rss_before, rss_after) {
        (Some(before), Some(after)) => Some((after as f64 - before as f64) / (1024.0 * 1024.0)),
        _ => None,
    };
    if let Some(delta) = rss_delta_mib {
        assert!(
            delta < 256.0,
            "working set grew by {delta:.1} MiB, above the 256 MiB bound"
        );
    }

    Scenario::record(
        test,
        test,
        started,
        json!({
            "modules_per_cycle": copied.len(),
            "cycles": cycles,
            "peak_live_modules": peak_live,
            "live_after_each_cycle": cycle_live,
            "live_modules_after_drain": clean_baseline(),
            "pending_reclamations_after": pending_reclamations(),
            "rss_before_bytes": rss_before,
            "rss_after_bytes": rss_after,
            "rss_delta_mib": rss_delta_mib,
            "rss_bound_mib": 256,
        }),
    )
}

#[test]
fn saturation_dozens_of_modules_are_reclaimed_and_memory_stays_bounded() {
    let _serial = serial();
    let scenario = scenario_module_saturation(fixtures(), Scale::full());
    assert_eq!(scenario.facts["pending_reclamations_after"], json!(0));
}

// ---------------------------------------------------------------------------------------
// Scenario 8 — races between DSP state and published versions.
// ---------------------------------------------------------------------------------------

/// One publisher thread. Each publisher has a **fixed phase offset** (0 starts with
/// `gain_v2`, 1 starts with `gain_v1`) and alternates. A rejection means the other
/// publisher won that artifact; the next attempt targets the other artifact and is
/// therefore guaranteed to succeed. This removes the former retry loop, which could
/// livelock forever when both threads ran the same candidate sequence in lockstep (the
/// CI failure: the loser exhausted its retries and panicked, then the render thread kept
/// the scope alive until the watchdog aborted the process).
fn publisher_loop(
    host: &BlockHost,
    fx: &Fixtures,
    activations: usize,
    phase: u64,
    progress: &AtomicU64,
) -> Result<u64, String> {
    let mut local = 0u64;
    let mut attempts = 0u64;
    while local < activations as u64 {
        attempts += 1;
        if attempts > (activations as u64).saturating_mul(4).max(64) {
            return Err(format!(
                "publisher (phase {phase}) gave up after {attempts} attempts for {activations} activations"
            ));
        }
        let candidate = if (attempts + phase) % 2 == 0 {
            &fx.gain_v2
        } else {
            &fx.gain_v1
        };
        match try_apply(host, candidate).status {
            ActivationStatus::Activated => {
                local += 1;
                progress.fetch_add(1, Ordering::SeqCst);
            }
            // Documented race: the other publisher activated exactly this artifact first.
            // The next attempt selects the other artifact, which cannot be active then.
            ActivationStatus::Rejected => continue,
        }
    }
    Ok(local)
}

fn scenario_version_races(fx: &Fixtures, scale: Scale) -> Scenario {
    let started = Instant::now();
    let test = "concurrent_publications_render_whole_versions_only";
    let progress = progress_counter();
    let watchdog = ProgressWatchdog::start(
        "publication/render races",
        Duration::from_secs(30),
        Arc::clone(&progress),
    );
    let host =
        BlockHost::bootstrap(ExpectedAbi::dsp(), artifact_path(&fx.gain_v1)).expect("bootstrap");
    let _warm = warm_images(fx);
    let publishers_done = AtomicBool::new(false);
    let blocks = AtomicU64::new(0);
    let bad_blocks = AtomicU64::new(0);
    let (activations, render_blocks, failure) = std::thread::scope(|scope| {
        let render = scope.spawn(|| {
            let mut state = DspState::default();
            let input = vec![1.0f32; 128];
            let mut output = vec![0f32; 128];
            while !publishers_done.load(Ordering::SeqCst) {
                host.process_rt(&input, &mut output, &mut state);
                // A block must be entirely one published version: the gain fixtures give
                // 0.5 or 2.0, never a mixed buffer.
                let first = output[0];
                let uniform = output.iter().all(|sample| *sample == first);
                let known = first == 0.5 || first == 2.0;
                if !uniform || !known {
                    bad_blocks.fetch_add(1, Ordering::SeqCst);
                }
                blocks.fetch_add(1, Ordering::SeqCst);
                progress.fetch_add(1, Ordering::SeqCst);
                // On a 4 vCPU CI runner the render loop must not monopolise a core: the
                // publishers do the expensive work (module registration). Yielding keeps
                // the race progressing in both directions without reducing coverage.
                std::thread::yield_now();
            }
        });
        // Stop the render loop even if a publisher panics: no scope deadlock.
        let _stop = StopOnDrop(&publishers_done);
        let mut activations = 0u64;
        let mut failure: Option<String> = None;
        for _round in 0..2 {
            let work = scale.race_activations / 2;
            // Two publisher threads racing on the same host, with distinct phases so a
            // successful publication never turns into an unbounded retry loop.
            let (a, b) = std::thread::scope(|inner| {
                let a = inner.spawn(|| publisher_loop(&host, fx, work, 0, &progress));
                let b = inner.spawn(|| publisher_loop(&host, fx, work, 1, &progress));
                (
                    a.join().expect("publisher A"),
                    b.join().expect("publisher B"),
                )
            });
            match (a, b) {
                (Ok(a), Ok(b)) => activations += a + b,
                (first, second) => {
                    failure = Some(format!("publisher failure: {first:?}"));
                    let _ = second;
                    break;
                }
            }
        }
        publishers_done.store(true, Ordering::SeqCst);
        render.join().expect("render thread");
        (activations, blocks.load(Ordering::SeqCst), failure)
    });
    drop(watchdog);
    if let Some(failure) = failure {
        panic!("{failure}");
    }
    assert_eq!(
        bad_blocks.load(Ordering::SeqCst),
        0,
        "a block mixed versions"
    );
    assert!(
        render_blocks > 0,
        "the render thread never processed a block during the race"
    );
    assert_eq!(host.status().activations, activations);

    Scenario::record(
        test,
        test,
        started,
        json!({
            "publishers": 2,
            "activations": activations,
            "render_blocks_during_races": render_blocks,
            "mixed_blocks": bad_blocks.load(Ordering::SeqCst),
            "rule": "each block is processed under one non-owning guard: every sample must come from the same published gain (0.5 or 2.0)",
            "hard_bound": "progress watchdog: aborts only if no block and no activation happen for 30 s",
        }),
    )
}

#[test]
fn concurrent_publications_render_whole_versions_only() {
    let _serial = serial();
    let scenario = scenario_version_races(fixtures(), Scale::full());
    assert_eq!(scenario.facts["mixed_blocks"], json!(0));
}

// ---------------------------------------------------------------------------------------
// Evidence campaign — always runs a real (compact) campaign; writes the JSON only when
// `RAVUM_STRESS_EVIDENCE` names a path. This keeps `cargo test` free of repository writes.
// ---------------------------------------------------------------------------------------

/// UTC calendar date (`YYYY-MM-DD`) of the moment the evidence is written.
///
/// The evidence file must carry the date of its own run: a hard-coded constant
/// silently back-dates every file, which is exactly the kind of stale evidence
/// INV-08 forbids. No dependency is added: days since the Unix epoch are converted
/// with Howard Hinnant's `civil_from_days` algorithm (proleptic Gregorian).
fn utc_date() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0);
    let days = seconds.div_euclid(86_400);

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // day of era, [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let mut year = yoe + era * 400;
    if month <= 2 {
        year += 1;
    }
    format!("{year:04}-{month:02}-{day:02}")
}

#[test]
fn stress_evidence_campaign() {
    let _serial = serial();
    let fx = fixtures();
    let evidence_path = std::env::var("RAVUM_STRESS_EVIDENCE").ok();
    if evidence_path.is_none() {
        // Non-empty smoke: the same invariants, smaller sizes, no file written.
        let scale = Scale::compact();
        let mut scenarios = Vec::new();
        eprintln!("[dsp-stress] compact 1/8 ten_thousand_swaps");
        scenarios.push(scenario_ten_thousand_swaps(fx, scale));
        eprintln!("[dsp-stress] compact 2/8 two_hosts_in_parallel");
        scenarios.push(scenario_two_hosts_in_parallel(fx, scale));
        eprintln!("[dsp-stress] compact 3/8 stop_start_cycles");
        scenarios.push(scenario_stop_start_cycles(fx, scale));
        eprintln!("[dsp-stress] compact 4/8 variable_buffers");
        scenarios.push(scenario_variable_buffers(fx, scale));
        eprintln!("[dsp-stress] compact 5/8 drop_during_closure");
        scenarios.push(scenario_drop_during_closure(fx, scale));
        eprintln!("[dsp-stress] compact 6/8 stop_during_swap");
        scenarios.push(scenario_stop_during_swap(fx, scale));
        eprintln!("[dsp-stress] compact 7/8 module_saturation");
        scenarios.push(scenario_module_saturation(fx, scale));
        eprintln!("[dsp-stress] compact 8/8 version_races");
        scenarios.push(scenario_version_races(fx, scale));
        assert_eq!(scenarios.len(), 8);
        eprintln!(
            "[dsp-stress] compact campaign done (8 scenarios); set RAVUM_STRESS_EVIDENCE=<path> for the full evidence run"
        );
        return;
    }

    let evidence_path = PathBuf::from(evidence_path.expect("checked above"));
    let scale = Scale::full();
    let suite_started = Instant::now();
    let rss_start = process_memory::working_set_bytes();
    let scenarios = vec![
        scenario_ten_thousand_swaps(fx, scale),
        scenario_two_hosts_in_parallel(fx, scale),
        scenario_stop_start_cycles(fx, scale),
        scenario_variable_buffers(fx, scale),
        scenario_drop_during_closure(fx, scale),
        scenario_stop_during_swap(fx, scale),
        scenario_module_saturation(fx, scale),
        scenario_version_races(fx, scale),
    ];
    let rss_end = process_memory::working_set_bytes();
    let suite_ms = suite_started.elapsed().as_millis() as u64;
    assert!(
        suite_ms < 5 * 60 * 1000,
        "the full campaign took {suite_ms} ms, above the 5 minute budget"
    );
    let cases: Vec<Value> = scenarios
        .iter()
        .map(|scenario| {
            json!({
                "case": scenario.name,
                "test": scenario.test,
                "status": "pass",
                "duration_ms": scenario.duration_ms,
                "facts": scenario.facts,
            })
        })
        .collect();
    let evidence = json!({
        "schema_version": 1,
        "kind": "dsp-stress-campaign",
        "date": utc_date(),
        "tool": "cargo test -p ravum-dsp-harness --test dsp_stress",
        "profile": "test (debug)",
        "platform": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "logic_cpus": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
        "suite_duration_ms": suite_ms,
        "suite_budget_ms": 5 * 60 * 1000,
        "scenario_bound_s": MAX_SCENARIO_SECONDS,
        "rss_start_bytes": rss_start,
        "rss_end_bytes": rss_end,
        "cases": cases,
        "limits": [
            "in-memory block host only: no audio device, no real-time priority, no XRUN measurement, no hard real-time qualification",
            "the allocation counters are the counting global allocator of ravum-dsp-harness; zero proves no allocation happened on the measured thread during that window, nothing more",
            "module live/unloaded counters are process-wide; the scenarios are serialized in this test binary so the counts are meaningful",
            "the reference machine is shared with other agents: the campaign asserts invariants and generous duration bounds, never a throughput",
            "Windows-only toolchain for the native fixture builds; the stress invariants themselves are platform-neutral",
            "the watchdog turns a hypothetical deadlock into a process abort (hard bound) instead of a hung test run",
        ],
    });
    if let Some(parent) = evidence_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).expect("evidence dir");
        }
    }
    std::fs::write(
        &evidence_path,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&evidence).expect("serialise evidence")
        ),
    )
    .expect("write evidence");
    eprintln!(
        "[dsp-stress] wrote {} ({} cases, {} ms)",
        evidence_path.display(),
        scenarios.len(),
        suite_ms
    );
}
