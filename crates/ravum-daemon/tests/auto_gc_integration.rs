//! Integration tests for the automatic background GC (P0-H suite).
//!
//! All analysands are really compiled (no mocks). Automatic GC runs in the
//! daemon's maintenance thread; tests observe it through `stats`, the bounded
//! JSONL journal and the *next* analyses (recent content must stay a cache hit,
//! collected content must be rebuilt). Compiler-spawning tests hold [`RUSTC_LOCK`]
//! so at most one compiler runs at a time on the shared reference machine.

use ravum_daemon::{
    AnalysisRequest, AutoGcPolicy, BuildContext, Daemon, DaemonConfig, ErrorKind, Priority,
    SourceFile,
};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

static RUSTC_LOCK: Mutex<()> = Mutex::new(());
static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);
static REVISION_SEQ: AtomicU64 = AtomicU64::new(0);

fn rustc() -> PathBuf {
    std::env::var_os("RAVUM_RUSTC")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("rustc"))
}

fn rustc_guard() -> std::sync::MutexGuard<'static, ()> {
    match RUSTC_LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "ravum-daemon-auto-gc-test-{name}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn config(work_dir: &Path, auto_gc: AutoGcPolicy) -> DaemonConfig {
    DaemonConfig {
        rustc: rustc(),
        work_dir: work_dir.to_path_buf(),
        max_concurrent_jobs: 1,
        max_cache_entries: 64,
        poll_interval_ms: 2,
        analysis_timeout_ms: 60_000,
        toolchain_override: None,
        auto_gc,
    }
}

/// A small but real unit; `seed` and `crate_name` make contents/contexts distinct.
fn content(seed: u32) -> String {
    format!(
        "//! auto-GC integration fixture {seed}.\n\npub fn value_{seed}(x: f64) -> f64 {{\n    x * {}.0 + {seed}.0\n}}\n",
        seed + 1
    )
}

fn files(seed: u32) -> Vec<SourceFile> {
    vec![SourceFile {
        path: "src/lib.rs".to_owned(),
        content: content(seed),
    }]
}

fn context(crate_name: &str) -> BuildContext {
    BuildContext {
        edition: "2021".to_owned(),
        crate_name: crate_name.to_owned(),
        crate_type: "lib".to_owned(),
        flags: Vec::new(),
    }
}

fn next_revision() -> u64 {
    REVISION_SEQ.fetch_add(1, Ordering::SeqCst) + 1
}

fn request(seed: u32, crate_name: &str) -> AnalysisRequest {
    request_with_revision(seed, crate_name, next_revision())
}

fn request_with_revision(seed: u32, crate_name: &str, revision: u64) -> AnalysisRequest {
    AnalysisRequest {
        revision,
        label: format!("auto-gc-{crate_name}-{seed}"),
        root: "src/lib.rs".to_owned(),
        files: files(seed),
        context: context(crate_name),
        priority: Priority::Normal,
    }
}

/// Trigger policy calibrated on measured state sizes: one `rustc` incremental
/// context for this tiny unit is ~27 KB (first compile) then ~54 KB (two
/// sessions); 12 alternating analyses across 2 contexts plus verdicts/snapshots
/// exceed `min_total_bytes`, and the target is above the surviving context.
fn policy_trigger_low() -> AutoGcPolicy {
    AutoGcPolicy {
        enabled: true,
        check_every_requests: 2,
        min_total_bytes: Some(115_000),
        max_total_bytes: Some(100_000),
        max_cache_bytes: Some(2_000),
        max_snapshot_bytes: Some(10_000),
        max_incremental_bytes: Some(60_000),
        keep_recent: 2,
        idle_only: true,
    }
}

/// Policy used to test the failure path: the injected failure is consumed before
/// any threshold decision, so the state may stay below the trigger.
fn policy_fail_injection() -> AutoGcPolicy {
    AutoGcPolicy {
        enabled: true,
        check_every_requests: 1,
        min_total_bytes: Some(400_000),
        max_total_bytes: Some(300_000),
        max_cache_bytes: None,
        max_snapshot_bytes: None,
        max_incremental_bytes: None,
        keep_recent: 2,
        idle_only: true,
    }
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if entry_path.is_dir() {
                stack.push(entry_path);
            } else if let Ok(metadata) = entry.metadata() {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    total
}

/// Bounded wait for the asynchronous auto-GC maintenance thread. The trigger is
/// not synchronous with `analyze`, so the test must poll `runs` instead of
/// asserting immediately (a timeout is a real failure with full diagnostics).
fn wait_for_auto_gc_run(daemon: &Daemon) -> ravum_daemon::AutoGcStats {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut stats = daemon.stats();
    while stats.auto_gc.runs == 0 && std::time::Instant::now() < deadline {
        let _ = daemon.wait_auto_gc_idle(500);
        std::thread::sleep(std::time::Duration::from_millis(50));
        stats = daemon.stats();
    }
    if stats.auto_gc.runs == 0 {
        let journal_path = daemon.auto_gc_journal_path();
        let journal_count = journal_path
            .as_deref()
            .map(|path| journal_lines(path).len())
            .unwrap_or(0);
        panic!(
            "automatic GC never ran within 10 s: runs={} failures={} checks={} busy={} pending={} \
             requests_since_check={} journal_path={journal_path:?} journal_lines={journal_count} \
             work_dir={}",
            stats.auto_gc.runs,
            stats.auto_gc.failures,
            stats.auto_gc.checks,
            stats.auto_gc.busy,
            stats.auto_gc.pending,
            stats.auto_gc.requests_since_check,
            daemon.work_dir().display()
        );
    }
    stats.auto_gc
}

fn journal_lines(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .map(|text| {
            text.lines()
                .filter(|line| !line.trim().is_empty())
                .map(|line| serde_json::from_str::<Value>(line).expect("journal line is JSON"))
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Defaults and threshold behaviour
// ---------------------------------------------------------------------------

#[test]
fn disabled_by_default_no_thread_no_journal_no_cost() {
    let _guard = rustc_guard();
    let work = temp_dir("disabled");
    let daemon = Daemon::open(config(&work, AutoGcPolicy::default())).expect("daemon");
    assert!(!daemon.auto_gc_stats().enabled);
    assert!(daemon.auto_gc_journal_path().is_none());
    for seed in 1..=2 {
        let report = daemon
            .analyze(&request(seed, "auto_unit"))
            .expect("analysis");
        assert!(report.is_current() && report.success);
    }
    std::thread::sleep(Duration::from_millis(100));
    let stats = daemon.stats();
    assert!(!stats.auto_gc.enabled);
    assert_eq!(stats.auto_gc.runs, 0);
    assert_eq!(stats.auto_gc.checks, 0);
    assert!(!work.join("gc-auto.log").exists());
    assert!(daemon.wait_auto_gc_idle(100));
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn below_threshold_runs_no_collection() {
    let _guard = rustc_guard();
    let work = temp_dir("below-threshold");
    let policy = AutoGcPolicy {
        enabled: true,
        check_every_requests: 1,
        min_total_bytes: Some(1_000_000_000),
        max_total_bytes: Some(500_000_000),
        keep_recent: 4,
        idle_only: true,
        ..Default::default()
    };
    let daemon = Daemon::open(config(&work, policy)).expect("daemon");
    for seed in 1..=3 {
        let report = daemon
            .analyze(&request(seed, "auto_unit"))
            .expect("analysis");
        assert!(report.is_current() && report.success && !report.cache.hit);
    }
    assert!(daemon.wait_auto_gc_idle(5_000), "auto-GC must settle");
    let stats = daemon.stats();
    assert_eq!(stats.auto_gc.checks, 3, "one threshold check per request");
    assert_eq!(stats.auto_gc.runs, 0, "below threshold: no collection");
    assert_eq!(stats.auto_gc.failures, 0);
    assert!(stats.auto_gc.last.is_none());
    assert!(
        !work.join("gc-auto.log").exists(),
        "no collection -> no journal"
    );
    // No overhead beyond the checks: the next analysis still works.
    let next = daemon.analyze(&request(4, "auto_unit")).expect("analysis");
    assert!(next.is_current() && next.success);
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn threshold_reached_collects_bounds_disk_and_keeps_recent_served() {
    let _guard = rustc_guard();
    let work = temp_dir("trigger");
    let policy = policy_trigger_low();
    let daemon = Daemon::open(config(&work, policy.clone())).expect("daemon");
    let mut last_seen = None;
    for index in 0..12u32 {
        let crate_name = if index % 2 == 0 {
            "auto_unit_a"
        } else {
            "auto_unit_b"
        };
        let report = daemon
            .analyze(&request(index + 1, crate_name))
            .expect("analysis");
        assert!(report.is_current() && report.success);
        last_seen = Some((index + 1, crate_name.to_owned()));
    }
    // Wait for the *observed effect*, not merely for a moment of idleness: the
    // maintenance thread may not have been scheduled yet when a single idle check
    // returns true (under CPU contention the wake-up can be delayed), and the point
    // of this test is that a collection eventually happens and is recorded.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut stats = daemon.stats();
    while stats.auto_gc.runs == 0 && std::time::Instant::now() < deadline {
        let _ = daemon.wait_auto_gc_idle(500);
        std::thread::sleep(std::time::Duration::from_millis(25));
        stats = daemon.stats();
    }
    assert!(
        daemon.wait_auto_gc_idle(15_000),
        "auto-GC must settle after the collection"
    );
    let stats = daemon.stats();
    assert!(stats.auto_gc.enabled);
    assert!(stats.auto_gc.checks >= 1);
    assert!(
        stats.auto_gc.runs >= 1,
        "threshold reached: at least one automatic collection, stats={:?}",
        stats.auto_gc
    );
    assert_eq!(stats.auto_gc.failures, 0);
    assert!(stats.auto_gc.last.as_ref().unwrap().ok);
    assert_eq!(
        stats.gc_runs, 0,
        "automatic collections must not be counted as manual GC runs"
    );
    assert!(stats.auto_gc.journal_entries >= 1);
    let entries = journal_lines(&work.join("gc-auto.log"));
    assert_eq!(entries.len(), stats.auto_gc.journal_entries as usize);
    assert!(entries.iter().all(|entry| entry["mode"] == "auto"));
    // The collected state is under the target cap after the automatic pass.
    let audit = daemon
        .gc(&ravum_daemon::GcPolicy {
            keep_recent: 8,
            dry_run: true,
            ..Default::default()
        })
        .expect("audit");
    assert!(
        audit.before.total_bytes <= policy.max_total_bytes.unwrap(),
        "disk after automatic GC ({} B) must be under the target ({} B)",
        audit.before.total_bytes,
        policy.max_total_bytes.unwrap()
    );
    // The most recent content is still served from the cache without a compiler.
    let (last_seed, last_crate) = last_seen.expect("analysed at least once");
    let runs_before = daemon.stats().rustc_runs;
    let recent = daemon
        .analyze(&request(last_seed, &last_crate))
        .expect("recent analysis");
    assert!(recent.cache.hit, "recent content must survive automatic GC");
    assert_eq!(recent.timings.analysis_ms, 0);
    assert_eq!(daemon.stats().rustc_runs, runs_before);
    // An old/collected content is still answered correctly (rebuilt if collected).
    let old = daemon
        .analyze(&request(1, "auto_unit_a"))
        .expect("old analysis");
    assert!(old.is_current() && old.success);
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn invalid_auto_gc_policy_fails_closed() {
    let work = temp_dir("invalid-policy");
    let mut bad = policy_trigger_low();
    bad.min_total_bytes = None;
    let error = Daemon::open(config(&work, bad))
        .err()
        .expect("min missing must be rejected");
    assert_eq!(error.kind, ErrorKind::InvalidRequest);
    let mut bad = policy_trigger_low();
    bad.max_total_bytes = Some(200_000); // equal to min: cannot reduce under trigger
    let error = Daemon::open(config(&work, bad))
        .err()
        .expect("max == min must be rejected");
    assert_eq!(error.kind, ErrorKind::InvalidRequest);
    let mut bad = policy_trigger_low();
    bad.check_every_requests = 0;
    assert!(Daemon::open(config(&work, bad)).is_err());
    let mut bad = policy_trigger_low();
    bad.keep_recent = 0;
    assert!(Daemon::open(config(&work, bad)).is_err());
    // A disabled policy is inert and accepts any values.
    let disabled = AutoGcPolicy {
        enabled: false,
        min_total_bytes: None,
        max_total_bytes: None,
        ..Default::default()
    };
    let daemon = Daemon::open(config(&work, disabled)).expect("disabled daemon opens");
    assert!(!daemon.auto_gc_stats().enabled);
    let _ = std::fs::remove_dir_all(&work);
}

// ---------------------------------------------------------------------------
// Robustness: failure injection, clients, journal
// ---------------------------------------------------------------------------

#[test]
fn injected_auto_gc_failure_never_fails_an_analysis() {
    let _guard = rustc_guard();
    let work = temp_dir("injected-failure");
    let daemon = Daemon::open(config(&work, policy_fail_injection())).expect("daemon");
    daemon.inject_auto_gc_failure(Some("injected for test".to_owned()));
    let first = daemon.analyze(&request(1, "auto_unit")).expect("analysis");
    assert!(first.is_current() && first.success);
    assert!(daemon.wait_auto_gc_idle(5_000), "auto-GC must settle");
    let stats = daemon.stats();
    assert!(
        stats.auto_gc.failures >= 1,
        "injected failure must be visible"
    );
    let last = stats.auto_gc.last.as_ref().expect("last attempt recorded");
    assert!(!last.ok);
    assert!(
        last.error.as_deref().unwrap_or("").contains("injected"),
        "error must be exposed: {:?}",
        last.error
    );
    let journal = journal_lines(&work.join("gc-auto.log"));
    assert!(journal.iter().any(|entry| entry["ok"] == false));
    // The next analysis is unaffected.
    let second = daemon.analyze(&request(2, "auto_unit")).expect("analysis");
    assert!(second.is_current() && second.success);
    assert!(daemon.wait_auto_gc_idle(5_000));
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn concurrent_clients_with_auto_gc_no_corruption() {
    let _guard = rustc_guard();
    let work = temp_dir("concurrent");
    let policy = AutoGcPolicy {
        idle_only: false,
        ..policy_trigger_low()
    };
    let daemon = Daemon::open(config(&work, policy)).expect("daemon");
    let mut handles = Vec::new();
    for client in 0..2u32 {
        let daemon = daemon.clone();
        handles.push(std::thread::spawn(move || {
            let mut results = Vec::new();
            for index in 0..6u32 {
                let seed = client * 20 + index + 1;
                let crate_name = if client == 0 {
                    "auto_unit_x"
                } else {
                    "auto_unit_y"
                };
                // Fixed revision: two independent clients interleave freely and a
                // strictly-older revision would be (correctly) superseded. Equal
                // revisions are accepted by the monotonic contract.
                let report = daemon
                    .analyze(&request_with_revision(seed, crate_name, 1_000))
                    .expect("analysis");
                results.push(report);
            }
            results
        }));
    }
    let mut all_fresh = true;
    for handle in handles {
        for report in handle.join().expect("client thread") {
            all_fresh &= report.is_current() && report.success;
        }
    }
    assert!(
        all_fresh,
        "every concurrent analysis must be fresh and successful"
    );
    // Wait for the *observed effect*, not merely for a moment of idleness: the
    // maintenance thread may not have been scheduled yet when a single idle check
    // returns true (under CPU contention the wake-up can be delayed), and the point
    // of this test is that a collection eventually happens and is recorded.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut stats = daemon.stats();
    while stats.auto_gc.runs == 0 && std::time::Instant::now() < deadline {
        let _ = daemon.wait_auto_gc_idle(500);
        std::thread::sleep(std::time::Duration::from_millis(25));
        stats = daemon.stats();
    }
    assert!(
        daemon.wait_auto_gc_idle(15_000),
        "auto-GC must settle after the collection"
    );
    let stats = daemon.stats();
    assert_eq!(stats.cache_corrupt, 0);
    assert_eq!(stats.auto_gc.failures, 0);
    assert!(
        stats.auto_gc.runs >= 1,
        "two clients accumulate enough state to trigger automatic GC"
    );
    // Journal stays valid JSON and bounded after concurrent runs.
    let entries = journal_lines(&work.join("gc-auto.log"));
    assert!(entries.len() <= ravum_daemon::auto_gc::AUTO_GC_JOURNAL_MAX_ENTRIES);
    assert!(entries.iter().all(|entry| entry["ok"].is_boolean()));
    // The daemon still answers correctly after the soak-in-miniature.
    let final_report = daemon
        .analyze(&request_with_revision(99, "auto_unit_x", 1_001))
        .expect("analysis");
    assert!(final_report.is_current() && final_report.success);
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn auto_gc_journal_and_stats_are_bounded_and_coherent() {
    let _guard = rustc_guard();
    let work = temp_dir("journal-coherence");
    let policy = policy_trigger_low();
    let daemon = Daemon::open(config(&work, policy.clone())).expect("daemon");
    for index in 0..12u32 {
        let crate_name = if index % 2 == 0 {
            "auto_unit_a"
        } else {
            "auto_unit_b"
        };
        let report = daemon
            .analyze(&request(index + 1, crate_name))
            .expect("analysis");
        assert!(report.success);
    }
    // The auto-GC trigger is asynchronous: poll with a bounded timeout (50 ms,
    // 10 s) so load cannot turn a real collection into a spurious failure.
    let _ = wait_for_auto_gc_run(&daemon);
    assert!(
        daemon.wait_auto_gc_idle(15_000),
        "auto-GC must settle after the collection"
    );
    let stats = daemon.stats();
    let entries = journal_lines(&work.join("gc-auto.log"));
    assert_eq!(entries.len() as u64, stats.auto_gc.journal_entries);
    assert!(entries.len() as u64 <= ravum_daemon::auto_gc::AUTO_GC_JOURNAL_MAX_ENTRIES as u64);
    assert_eq!(
        stats.auto_gc.runs + stats.auto_gc.failures,
        stats.auto_gc.journal_entries,
        "one journal line per automatic attempt"
    );
    if let Some(last) = &stats.auto_gc.last {
        let matches = entries
            .iter()
            .any(|entry| entry["at_unix"].as_u64() == Some(last.at_unix));
        assert!(matches, "last attempt must be present in the journal");
    }
    assert!(stats.auto_gc.runs >= 1);
    // Disk measured on the filesystem is bounded by the target after settling.
    let audit = daemon
        .gc(&ravum_daemon::GcPolicy {
            keep_recent: 8,
            dry_run: true,
            ..Default::default()
        })
        .expect("audit");
    assert!(audit.before.total_bytes <= policy.max_total_bytes.unwrap());
    assert!(dir_size(&work) >= audit.before.total_bytes);
    let _ = std::fs::remove_dir_all(&work);
}
