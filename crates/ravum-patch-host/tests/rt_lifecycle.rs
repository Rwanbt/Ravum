//! MVP-0b / L1-A — adversarial real-time lifecycle tests.
//!
//! The MVP-0a hazard: `process_rt` took an owning `Arc` on the audio thread. If a
//! concurrent publication retired the module and the control thread released its own
//! references first, the audio thread's release was the **last** one: `BlockModule`
//! `Drop` then ran on the callback, freeing the struct allocation and sending the
//! native library to the reclamation channel (allocation + lock inside a `Drop`).
//!
//! These tests try to force exactly that interleaving and assert the observable
//! consequence: the audio thread performs **zero allocations and zero deallocations**,
//! the retired payload is freed by the writer, and every native handle is eventually
//! unloaded. They are property tests over a real process (real `rustc` builds, real
//! `LoadLibrary`/`FreeLibrary`), not a model.
//!
//! The audio thread *borrows* the host through `std::thread::scope`: it never holds an
//! owning `Arc<BlockHost>`, so the host itself is always destroyed by the control
//! thread. That is the pattern this MVP recommends for any host that can be asked to
//! finish while a callback may still be running.
//!
//! Scope, honestly: a zero count proves the instrumented allocator saw no work on that
//! thread during that campaign. It does not prove hard real-time behaviour (no audio
//! device, no RT priority, no WCET, no page-fault measurement).

use ravum_dsp_harness::{
    allocations_on_this_thread, block_evidence, deallocations_on_this_thread,
    reset_thread_counters,
};
use ravum_patch_host::{
    live_modules, pending_reclamations, prepare_block, wait_until_idle, BlockHost, ExpectedAbi,
};
use ravum_protocol::{ActivationStatus, BuildOutput, DspState, EvidenceRecord};
use ravum_rust_adapter::RustAdapter;
use ravum_snapshot::SourceSnapshot;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// The module/handle counters are process-wide, so tests in this file must not run in
/// parallel with each other: each one asserts exact `live_modules()` transitions.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Drain any deferred unload before reading the process-wide module counters.
///
/// Building a fixture and deriving its evidence both load and release the module, so a
/// baseline taken immediately after `versions()` would still have one library queued.
fn clean_baseline() -> u64 {
    assert!(
        wait_until_idle(Duration::from_secs(20)),
        "the reclaimer did not drain before the baseline"
    );
    live_modules()
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

struct Versions {
    build: BuildOutput,
    evidence: EvidenceRecord,
}

/// Build both fixture versions once; the artifact cache makes this affordable.
fn versions(dir: &Path) -> (Versions, Versions) {
    let v1_build = build(dir, "gain_v1.rs");
    let v1_evidence = block_evidence(&v1_build).expect("evidence v1");
    let v2_build = build(dir, "gain_v2.rs");
    let v2_evidence = block_evidence(&v2_build).expect("evidence v2");
    (
        Versions {
            build: v1_build,
            evidence: v1_evidence,
        },
        Versions {
            build: v2_build,
            evidence: v2_evidence,
        },
    )
}

fn apply_once(host: &BlockHost, candidate: &Versions) -> ravum_protocol::ActivationReport {
    let prepared = prepare_block(
        Path::new(&candidate.build.artifact_path),
        &ExpectedAbi::dsp(),
    )
    .expect("prepare");
    host.apply(
        prepared,
        &candidate.build,
        Some(&candidate.evidence),
        &candidate.build.source_snapshot_id,
    )
}

/// Stop flag, readiness flag and progress counter shared by a campaign.
struct Campaign {
    stop: AtomicBool,
    ready: AtomicBool,
    blocks: AtomicU64,
}

impl Campaign {
    fn new() -> Self {
        Self {
            stop: AtomicBool::new(false),
            ready: AtomicBool::new(false),
            blocks: AtomicU64::new(0),
        }
    }

    fn wait_ready(&self) {
        while !self.ready.load(Ordering::SeqCst) {
            std::thread::yield_now();
        }
    }

    fn wait_blocks(&self, minimum: u64) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.blocks.load(Ordering::SeqCst) < minimum {
            if Instant::now() > deadline {
                break;
            }
            std::thread::yield_now();
        }
    }
}

#[derive(Debug)]
struct AudioResult {
    blocks: u64,
    allocations: u64,
    deallocations: u64,
    versions_seen: BTreeSet<String>,
}

/// Body of one "audio callback" loop: one guard per block, counters read on this thread.
fn audio_loop(host: &BlockHost, campaign: &Campaign) -> AudioResult {
    let mut state = DspState::default();
    let mut input = vec![0.0f32; 256];
    let mut output = vec![0.0f32; 256];
    for (i, sample) in input.iter_mut().enumerate() {
        *sample = (i as f32 * 0.01).sin() * 0.5;
    }
    // Warm-up block: TLS and the module's own lazy state are touched before measuring.
    host.process_rt(&input, &mut output, &mut state);
    reset_thread_counters();
    campaign.ready.store(true, Ordering::SeqCst);

    let mut blocks = 0u64;
    // No `String`/`BTreeSet` work inside the callback loop: the test must not be the
    // thing that allocates on the audio thread it is measuring.
    let mut saw_v1 = false;
    let mut saw_v2 = false;
    while !campaign.stop.load(Ordering::SeqCst) {
        // One guard per block: the whole block sees exactly one version.
        let module = host.enter_rt();
        match module.manifest.patch_id.as_str() {
            "dsp-gain-v1" => saw_v1 = true,
            "dsp-gain-v2" => saw_v2 = true,
            _ => {}
        }
        module.process_block(&input, &mut output, &mut state);
        blocks += 1;
        campaign.blocks.fetch_add(1, Ordering::SeqCst);
        std::hint::black_box(output[0]);
    }
    let allocations = allocations_on_this_thread();
    let deallocations = deallocations_on_this_thread();
    let mut versions_seen = BTreeSet::new();
    if saw_v1 {
        versions_seen.insert("dsp-gain-v1".to_owned());
    }
    if saw_v2 {
        versions_seen.insert("dsp-gain-v2".to_owned());
    }
    AudioResult {
        blocks,
        allocations,
        deallocations,
        versions_seen,
    }
}

/// The central acceptance test of L1-A: repeated publications while a callback runs
/// must never make the callback allocate or free.
#[test]
fn audio_thread_neither_allocates_nor_frees_during_repeated_swaps() {
    let _serial = serial();
    let dir = work("rt-lifecycle-alloc");
    let (v1, v2) = versions(&dir);
    let host = BlockHost::bootstrap(ExpectedAbi::dsp(), Path::new(&v1.build.artifact_path))
        .expect("bootstrap");
    let campaign = Campaign::new();

    let result = std::thread::scope(|scope| {
        let audio = scope.spawn(|| audio_loop(&host, &campaign));
        campaign.wait_ready();
        for round in 0..12 {
            let candidate = if round % 2 == 0 { &v2 } else { &v1 };
            let report = apply_once(&host, candidate);
            assert_eq!(
                report.status,
                ActivationStatus::Activated,
                "swap {round} refused: {:?}",
                report.reject_reason
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        campaign.wait_blocks(campaign.blocks.load(Ordering::SeqCst) + 50);
        campaign.stop.store(true, Ordering::SeqCst);
        audio.join().expect("audio thread")
    });

    assert!(result.blocks >= 50, "the callback ran only {} blocks", result.blocks);
    assert!(
        result.allocations == 0,
        "the audio thread allocated {} time(s) during {} blocks",
        result.allocations,
        result.blocks
    );
    assert!(
        result.deallocations == 0,
        "the audio thread performed {} deallocation(s) during {} blocks: a retired \
         module was freed on the callback",
        result.deallocations,
        result.blocks
    );
    assert!(
        result.versions_seen.len() >= 2,
        "the callback should have observed both published versions, saw {:?}",
        result.versions_seen
    );

    assert_eq!(host.status().activations, 12);
    eprintln!(
        "[l1a] e2e campaign: blocks={} swaps=12 allocations_on_audio_thread={} deallocations_on_audio_thread={} versions_seen={:?}",
        result.blocks, result.allocations, result.deallocations, result.versions_seen
    );
    drop(host);
    assert!(wait_until_idle(Duration::from_secs(20)), "reclaimer backlog");
}

/// Every retired native library must be released off the callback, and closing the host
/// must leave no loaded handle behind: no leak, no unbounded backlog.
#[test]
fn retired_modules_are_freed_off_the_audio_thread_and_nothing_leaks() {
    let _serial = serial();
    assert!(wait_until_idle(Duration::from_secs(20)), "clean baseline");
    let dir = work("rt-lifecycle-leak");
    let (v1, v2) = versions(&dir);
    let live_before_host = clean_baseline();
    let host = BlockHost::bootstrap(ExpectedAbi::dsp(), Path::new(&v1.build.artifact_path))
        .expect("bootstrap");
    assert_eq!(live_modules(), live_before_host + 1, "one active module");
    let campaign = Campaign::new();

    let result = std::thread::scope(|scope| {
        let audio = scope.spawn(|| audio_loop(&host, &campaign));
        campaign.wait_ready();
        for round in 0..20 {
            let candidate = if round % 2 == 0 { &v2 } else { &v1 };
            let report = apply_once(&host, candidate);
            assert_eq!(report.status, ActivationStatus::Activated);
        }
        campaign.stop.store(true, Ordering::SeqCst);
        audio.join().expect("audio thread")
    });
    assert_eq!(result.deallocations, 0, "frees happened on the callback");

    assert!(
        wait_until_idle(Duration::from_secs(20)),
        "the reclaimer did not drain the retired modules"
    );
    assert_eq!(pending_reclamations(), 0);
    assert_eq!(
        live_modules(),
        live_before_host + 1,
        "only the active module may remain loaded"
    );

    drop(host);
    assert!(wait_until_idle(Duration::from_secs(20)));
    assert_eq!(
        live_modules(),
        live_before_host,
        "closing the host must release its active module: no leak, no inline unload"
    );
}

/// Several readers and one writer: each block sees one version, and no reader allocates
/// or frees while publications happen.
#[test]
fn several_audio_threads_and_one_writer_share_one_version_per_block() {
    let _serial = serial();
    assert!(wait_until_idle(Duration::from_secs(20)), "clean baseline");
    let dir = work("rt-lifecycle-multi");
    let (v1, v2) = versions(&dir);
    let live_before_host = clean_baseline();
    let host = BlockHost::bootstrap(ExpectedAbi::dsp(), Path::new(&v1.build.artifact_path))
        .expect("bootstrap");
    let campaign = Campaign::new();

    let results = std::thread::scope(|scope| {
        let mut readers = Vec::new();
        for _ in 0..4 {
            readers.push(scope.spawn(|| audio_loop(&host, &campaign)));
        }
        campaign.wait_ready();
        for round in 0..16 {
            let candidate = if round % 2 == 0 { &v2 } else { &v1 };
            let report = apply_once(&host, candidate);
            assert_eq!(report.status, ActivationStatus::Activated);
            std::thread::sleep(Duration::from_millis(1));
        }
        campaign.wait_blocks(campaign.blocks.load(Ordering::SeqCst) + 40);
        campaign.stop.store(true, Ordering::SeqCst);
        readers
            .into_iter()
            .map(|handle| handle.join().expect("audio thread"))
            .collect::<Vec<_>>()
    });

    for result in results {
        assert_eq!(result.allocations, 0, "reader allocated");
        assert_eq!(result.deallocations, 0, "reader freed a module");
        assert!(result.blocks > 0);
        // Every block sees one version: at most one patch id per guard, and the set is
        // a subset of the two published versions.
        assert!(result
            .versions_seen
            .iter()
            .all(|id| id == "dsp-gain-v1" || id == "dsp-gain-v2"));
    }

    drop(host);
    assert!(wait_until_idle(Duration::from_secs(20)));
    assert_eq!(live_modules(), live_before_host, "no library left loaded");
}

/// A long block delays publication (the writer waits for quiescence) but never blocks
/// the audio path itself: the callback can enter while the writer spins.
#[test]
fn a_long_block_delays_publication_but_the_audio_path_never_waits_on_the_writer() {
    let _serial = serial();
    let dir = work("rt-lifecycle-slow-block");
    let (v1, v2) = versions(&dir);
    let host = BlockHost::bootstrap(ExpectedAbi::dsp(), Path::new(&v1.build.artifact_path))
        .expect("bootstrap");

    // Simulate a block that lasts 60 ms while a publication is attempted.
    let long_block = host.enter_rt();
    let started = Instant::now();
    let report = std::thread::scope(|scope| {
        let writer = scope.spawn(|| {
            let prepared =
                prepare_block(Path::new(&v2.build.artifact_path), &ExpectedAbi::dsp())
                    .expect("prepare");
            host.apply(
                prepared,
                &v2.build,
                Some(&v2.evidence),
                &v2.build.source_snapshot_id,
            )
        });

        // The audio path keeps running without waiting for the writer.
        let mut output = vec![0f32; 64];
        let input = vec![1f32; 64];
        let mut state = DspState::default();
        let enter_started = Instant::now();
        host.process_rt(&input, &mut output, &mut state);
        let audio_elapsed = enter_started.elapsed();
        assert!(
            audio_elapsed < Duration::from_millis(30),
            "the audio path waited {audio_elapsed:?} on the writer"
        );

        std::thread::sleep(Duration::from_millis(60));
        drop(long_block);
        writer.join().expect("writer")
    });

    assert_eq!(report.status, ActivationStatus::Activated);
    assert!(
        started.elapsed() >= Duration::from_millis(40),
        "the writer should have waited for the in-flight block"
    );

    drop(host);
    assert!(wait_until_idle(Duration::from_secs(20)));
}

/// The reclaimer is process-wide and must survive a large backlog created by a
/// publication storm, without leaking or blocking the control thread.
#[test]
fn a_publication_storm_never_leaks_a_native_handle() {
    let _serial = serial();
    assert!(wait_until_idle(Duration::from_secs(20)), "clean baseline");
    let dir = work("rt-lifecycle-storm");
    let (v1, v2) = versions(&dir);
    let live_before_host = clean_baseline();
    let host = BlockHost::bootstrap(ExpectedAbi::dsp(), Path::new(&v1.build.artifact_path))
        .expect("bootstrap");

    let storm_started = Instant::now();
    for round in 0..40 {
        let candidate = if round % 2 == 0 { &v2 } else { &v1 };
        let report = apply_once(&host, candidate);
        assert_eq!(report.status, ActivationStatus::Activated);
    }
    let storm = storm_started.elapsed();
    eprintln!("[l1a] publication storm: 40 publications in {storm:?} without any reader");

    assert!(
        wait_until_idle(Duration::from_secs(30)),
        "pending reclamations: {}",
        pending_reclamations()
    );
    assert_eq!(pending_reclamations(), 0);
    assert_eq!(live_modules(), live_before_host + 1);
    // 40 publications without an audio reader must stay cheap: the writer never waits
    // for a block when no reader is in flight.
    assert!(
        storm < Duration::from_secs(20),
        "40 publications took {storm:?}, publication should not wait without readers"
    );

    drop(host);
    assert!(wait_until_idle(Duration::from_secs(20)));
    assert_eq!(live_modules(), live_before_host);
}
