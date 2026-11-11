//! Integration tests for the MVP-0b analysis daemon.
//!
//! Every test that compiles real Rust acquires [`RUSTC_LOCK`] so at most one
//! compiler runs at a time: the reference machine has ~2 Go of free RAM and the
//! mission forbids measuring or provoking paging. The suite is also run with
//! `--test-threads=1` (documented command), but the mutex keeps the guarantee
//! even if someone forgets the flag.
//!
//! Tests cover the positive path and the required negative cases: stale
//! revisions, supersession during compilation, incomplete buffers, cache and
//! snapshot corruption, restarts, budgets, timeouts and the JSON contract.

use ravum_daemon::{
    parse_rustc_json, signature_set, sync_unit_dir, AnalysisRequest, BuildContext, Daemon,
    DaemonConfig, ErrorKind, Priority, ReportStatus, SourceFile, ToolchainInfo,
};
use serde_json::Value;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Serializes compiler-spawning tests; see module docs.
static RUSTC_LOCK: Mutex<()> = Mutex::new(());

/// Acquires the compiler lock, ignoring poisoning so one failing test cannot
/// mask the others with poison panics.
fn rustc_guard() -> std::sync::MutexGuard<'static, ()> {
    match RUSTC_LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}
static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

fn rustc() -> PathBuf {
    std::env::var_os("RAVUM_RUSTC")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("rustc"))
}

fn fixture_files() -> Vec<SourceFile> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join("unit")
        .join("src");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("fixture directory")
        .flatten()
        .filter(|entry| entry.path().extension().map(|ext| ext == "rs") == Some(true))
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|name| SourceFile {
            path: format!("src/{name}"),
            content: std::fs::read_to_string(dir.join(&name)).expect("fixture file"),
        })
        .collect()
}

fn context() -> BuildContext {
    BuildContext {
        edition: "2021".to_owned(),
        crate_name: "acme_unit".to_owned(),
        crate_type: "lib".to_owned(),
        flags: Vec::new(),
    }
}

fn request(revision: u64, files: &[SourceFile]) -> AnalysisRequest {
    AnalysisRequest {
        revision,
        label: "integration-test".to_owned(),
        root: "src/lib.rs".to_owned(),
        files: files.to_vec(),
        context: context(),
        priority: Priority::Normal,
    }
}

fn single_file(content: &str) -> Vec<SourceFile> {
    vec![SourceFile {
        path: "src/lib.rs".to_owned(),
        content: content.to_owned(),
    }]
}

fn temp_dir(name: &str) -> PathBuf {
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "ravum-daemon-test-{name}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn daemon_at(work_dir: &Path, max_jobs: usize, max_cache_entries: usize) -> Daemon {
    Daemon::open(DaemonConfig {
        rustc: rustc(),
        work_dir: work_dir.to_path_buf(),
        max_concurrent_jobs: max_jobs,
        max_cache_entries,
        poll_interval_ms: 2,
        analysis_timeout_ms: 60_000,
        toolchain_override: None,
        auto_gc: ravum_daemon::AutoGcPolicy::default(),
    })
    .expect("daemon open")
}

fn diagnostics_of(report: &ravum_daemon::AnalysisReport) -> Vec<String> {
    signature_set(&report.diagnostics)
}

/// Compiles `files` with the same public CLI as the daemon, in its own temporary
/// directory, to obtain the reference diagnostic signature set.
fn cold_signatures(files: &[SourceFile]) -> (bool, Vec<String>) {
    let dir = temp_dir("cold");
    let unit = dir.join("unit");
    let pairs: Vec<(String, &str)> = files
        .iter()
        .map(|file| (file.path.clone(), file.content.as_str()))
        .collect();
    sync_unit_dir(&unit, &pairs).expect("write cold unit");
    let out = dir.join("out");
    std::fs::create_dir_all(&out).expect("out dir");
    let output = Command::new(rustc())
        .args([
            "--edition=2021",
            "--crate-name=acme_unit",
            "--crate-type=lib",
            "--emit=metadata",
            "--error-format=json",
            "--diagnostic-width=120",
        ])
        .arg(format!("--out-dir={}", out.display()))
        .arg(unit.join("src").join("lib.rs"))
        .env_remove("RUSTFLAGS")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_BOOTSTRAP")
        .output()
        .expect("cold rustc");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let parsed = parse_rustc_json(&stderr, Some(&unit), output.status.success());
    let _ = std::fs::remove_dir_all(&dir);
    (output.status.success(), signature_set(&parsed.diagnostics))
}

fn apply(files: &mut [SourceFile], file: &str, find: &str, replace: &str) {
    let target = files
        .iter_mut()
        .find(|candidate| candidate.path == file)
        .unwrap_or_else(|| panic!("missing fixture file {file}"));
    assert_eq!(
        target.content.matches(find).count(),
        1,
        "anchor {find:?} in {file} must be unique"
    );
    target.content = target.content.replacen(find, replace, 1);
}

// ---------------------------------------------------------------------------
// Positive path and JSON contract.
// ---------------------------------------------------------------------------

#[test]
fn analysis_reports_structured_diagnostics_and_timings() {
    let _guard = rustc_guard();
    let work = temp_dir("diagnostics");
    let daemon = daemon_at(&work, 1, 16);
    let broken = single_file("pub fn broken( -> f64 {\n    let x = ;\n    x\n}\n");
    let report = daemon
        .analyze(&request(1, &broken))
        .expect("analysis must return a report");
    assert!(report.is_current());
    assert!(!report.success, "broken source must fail");
    assert!(!report.cache.hit);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.severity.is_error())
        .expect("a structured error diagnostic");
    assert_eq!(diagnostic.file.as_deref(), Some("src/lib.rs"));
    // rustc reports the unclosed delimiter at the EOF (line 4) as the primary
    // span; the point is that file/line/column are real structured values.
    assert_eq!(diagnostic.line, Some(4));
    assert!(diagnostic.column.unwrap_or(0) > 0);
    assert!(!diagnostic.message.is_empty());
    // Timings must all be present; analysis really happened (cache miss).
    assert!(report.timings.analysis_ms > 0);
    assert!(report.timings.total_ms >= report.timings.analysis_ms);

    // The report serializes to valid JSON with the documented fields.
    let value = report.to_json();
    for key in [
        "schema_version",
        "status",
        "revision",
        "current_revision",
        "snapshot_id",
        "context_id",
        "toolchain",
        "cache",
        "success",
        "diagnostics",
        "timings",
    ] {
        assert!(value.get(key).is_some(), "missing JSON field {key}");
    }
    for key in [
        "accept_ms",
        "queue_ms",
        "lookup_ms",
        "analysis_ms",
        "total_ms",
    ] {
        assert!(
            value["timings"].get(key).is_some(),
            "missing timing field {key}"
        );
    }
    let text = serde_json::to_string(&value).unwrap();
    let parsed: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed["status"], "fresh");

    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn identifier_rename_and_re_export_stay_consistent() {
    let _guard = rustc_guard();
    let work = temp_dir("rename");
    let daemon = daemon_at(&work, 1, 16);
    let mut files = fixture_files();
    apply(
        &mut files,
        "src/math.rs",
        "pub fn scale(x: f64) -> f64 {",
        "pub fn scaled_by(x: f64) -> f64 {",
    );
    apply(
        &mut files,
        "src/pipeline.rs",
        "use crate::math::{accumulate, clamp01, histogram, map_all, scale, SCALE};",
        "use crate::math::{accumulate, clamp01, histogram, map_all, scaled_by, SCALE};",
    );
    apply(
        &mut files,
        "src/pipeline.rs",
        "map_all(values, |value| scale(clamp01(value)))",
        "map_all(values, |value| scaled_by(clamp01(value)))",
    );
    apply(
        &mut files,
        "src/lib.rs",
        "pub use math::{accumulate, clamp01, histogram, map_all, scale, SCALE};",
        "pub use math::{accumulate, clamp01, histogram, map_all, scaled_by, SCALE};",
    );
    let report = daemon.analyze(&request(1, &files)).expect("analysis");
    assert!(report.is_current());
    assert!(
        report.success,
        "renamed unit must compile: {:?}",
        report.diagnostics
    );
    assert_eq!(report.diagnostics.len(), 0);

    let _ = std::fs::remove_dir_all(&work);
}

// ---------------------------------------------------------------------------
// Cache, context, toolchain and restart.
// ---------------------------------------------------------------------------

#[test]
fn identical_content_is_a_cache_hit_and_survives_restart() {
    let _guard = rustc_guard();
    let work = temp_dir("cache");
    let files = fixture_files();
    let daemon = daemon_at(&work, 1, 16);
    let first = daemon.analyze(&request(1, &files)).expect("first analysis");
    assert!(!first.cache.hit);
    assert!(first.timings.analysis_ms > 0);
    assert!(first.success);

    let second = daemon
        .analyze(&request(2, &files))
        .expect("second analysis");
    assert!(second.cache.hit, "identical bytes must hit the cache");
    assert_eq!(second.timings.analysis_ms, 0);
    assert_eq!(diagnostics_of(&first), diagnostics_of(&second));

    // Restart: the persistent level must still serve the verdict.
    drop(daemon);
    let reopened = daemon_at(&work, 1, 16);
    let third = reopened
        .analyze(&request(1, &files))
        .expect("analysis after restart");
    assert!(third.cache.hit, "disk cache must survive a restart");
    assert_eq!(third.cache.kind.as_str(), "disk");
    assert_eq!(diagnostics_of(&first), diagnostics_of(&third));

    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn build_context_flags_and_toolchain_identity_invalidate_cache() {
    let _guard = rustc_guard();
    let work = temp_dir("context");
    let files = fixture_files();
    let daemon = daemon_at(&work, 1, 16);

    let plain = daemon.analyze(&request(1, &files)).expect("plain");
    assert!(!plain.cache.hit);
    let repeat = daemon.analyze(&request(2, &files)).expect("repeat");
    assert!(repeat.cache.hit);

    let mut warnings_context = context();
    warnings_context.flags = vec!["-D".to_owned(), "warnings".to_owned()];
    let mut changed = request(3, &files);
    changed.context = warnings_context;
    let flags = daemon.analyze(&changed).expect("flags changed");
    assert_eq!(
        flags.cache.kind.as_str(),
        "miss",
        "a different build context must not reuse the verdict"
    );
    assert!(flags.timings.analysis_ms > 0);

    // Forced toolchain identity: documented test seam, no second toolchain.
    let simulated = ToolchainInfo {
        release: "1.98.1-simulated".to_owned(),
        host: daemon.toolchain().host.clone(),
        commit_hash: "simulated".to_owned(),
    };
    let other = Daemon::open(DaemonConfig {
        rustc: rustc(),
        work_dir: work.join("other-toolchain"),
        max_concurrent_jobs: 1,
        max_cache_entries: 8,
        poll_interval_ms: 2,
        analysis_timeout_ms: 60_000,
        toolchain_override: Some(simulated),
        auto_gc: ravum_daemon::AutoGcPolicy::default(),
    })
    .expect("toolchain daemon");
    let forced = other.analyze(&request(1, &files)).expect("toolchain");
    assert_eq!(
        forced.cache.kind.as_str(),
        "miss",
        "a different toolchain identity must not reuse the verdict"
    );
    let forced_hit = other
        .analyze(&request(2, &files))
        .expect("toolchain repeat");
    assert!(forced_hit.cache.hit);

    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn corrupt_cache_entry_degrades_to_a_miss_not_a_wrong_answer() {
    let _guard = rustc_guard();
    let work = temp_dir("corrupt-cache");
    let files = fixture_files();
    {
        let daemon = daemon_at(&work, 1, 16);
        daemon.analyze(&request(1, &files)).expect("seed");
    }
    let cache_dir = work.join("cache");
    let mut corrupted = 0usize;
    for entry in std::fs::read_dir(&cache_dir).expect("cache dir").flatten() {
        let path = entry.path();
        if path.extension().map(|ext| ext == "json") != Some(true) {
            continue;
        }
        let mut value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        // Change a payload field without recomputing the self-digest: the entry
        // must be rejected as corrupt instead of being trusted.
        value["analysis_ms"] = serde_json::json!(u64::MAX);
        value["success"] = serde_json::json!(false);
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        corrupted += 1;
    }
    assert!(corrupted > 0, "seed must have produced a cache entry");

    let reopened = daemon_at(&work, 1, 16);
    let report = reopened
        .analyze(&request(1, &files))
        .expect("analysis after corruption");
    assert_eq!(report.cache.kind.as_str(), "miss");
    assert!(report.success);
    assert!(reopened.stats().cache_corrupt > 0);

    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn tampered_snapshot_is_refused_fail_closed() {
    let _guard = rustc_guard();
    let work = temp_dir("snapshot-tamper");
    let files = fixture_files();
    let daemon = daemon_at(&work, 1, 16);
    let report = daemon.analyze(&request(1, &files)).expect("seed analysis");
    let snapshot_id = report.snapshot_id.clone();
    let checkout = work
        .join("snapshots")
        .join(&snapshot_id)
        .join("checkout")
        .join("src")
        .join("math.rs");
    std::fs::write(&checkout, "pub fn tampered() {}\n").unwrap();
    daemon.clear_caches().expect("clear caches");
    let error = daemon
        .analyze(&request(2, &files))
        .expect_err("tampered snapshot must be refused");
    assert_eq!(error.kind, ErrorKind::SnapshotCorrupt);

    let _ = std::fs::remove_dir_all(&work);
}

// ---------------------------------------------------------------------------
// Revisions, staleness and cancellation.
// ---------------------------------------------------------------------------

#[test]
fn stale_revision_is_never_returned_as_current() {
    let _guard = rustc_guard();
    let work = temp_dir("stale");
    let files = fixture_files();
    let daemon = daemon_at(&work, 1, 16);
    let fresh = daemon.analyze(&request(5, &files)).expect("fresh");
    assert!(fresh.is_current());
    assert_eq!(daemon.current_revision(), 5);

    let stale = daemon.analyze(&request(4, &files)).expect("stale");
    assert!(!stale.is_current());
    assert_eq!(stale.status, ReportStatus::Superseded);
    assert!(
        stale.diagnostics.is_empty(),
        "a stale answer must not carry diagnostics as current output"
    );

    daemon.advance_revision(10);
    let older = daemon.analyze(&request(9, &files)).expect("older");
    assert!(!older.is_current());

    let newer = daemon.analyze(&request(11, &files)).expect("newer");
    assert!(newer.is_current());
    assert!(newer.success);

    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn supersession_during_compilation_never_returns_a_current_answer() {
    let _guard = rustc_guard();
    let work = temp_dir("supersede");
    let daemon = daemon_at(&work, 1, 16);
    // Warm the compile unit so this test measures the supersession path.
    let mut state = fixture_files();
    let _ = daemon.analyze(&request(1, &state)).expect("warmup");
    apply(&mut state, "src/math.rs", "x * SCALE", "x * SCALE + 0.5");
    let revision = 2u64;
    let mut slow_request = request(revision, &state);
    slow_request.label = "slow".to_owned();
    let handle = daemon.clone();
    // Synchronise on the worker *entering* `analyze` instead of sleeping a
    // fixed delay: on a fast Linux runner the previous 40 ms sleep let the
    // compilation finish before the revision was advanced, so the outcome was
    // legitimately current and the test flaked. The channel makes the
    // "supersede while the request is in flight" scenario deterministic enough
    // without weakening the assertion.
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let _ = entered_tx.send(());
        handle.analyze(&slow_request)
    });
    entered_rx.recv().expect("worker entered analyze");
    daemon.advance_revision(revision + 10);
    let outcome = worker.join().expect("worker").expect("analysis");
    assert!(
        !outcome.is_current(),
        "a superseded answer must never be current"
    );
    assert!(matches!(
        outcome.status,
        ReportStatus::Superseded | ReportStatus::Cancelled
    ));
    // The compiler was either killed or its answer discarded; a later analysis
    // of the same bytes must still be correct.
    let after = daemon
        .analyze(&request(revision + 11, &state))
        .expect("after");
    assert!(after.is_current());
    assert!(after.success);
    let (cold_success, cold_diagnostics) = cold_signatures(&state);
    assert!(cold_success);
    assert_eq!(diagnostics_of(&after), cold_diagnostics);

    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn incomplete_buffer_then_repaired() {
    let _guard = rustc_guard();
    let work = temp_dir("incomplete");
    let daemon = daemon_at(&work, 1, 16);
    let broken = single_file("pub fn broken() -> f64 {\n    let x = ;\n    x\n}\n");
    let invalid = daemon.analyze(&request(1, &broken)).expect("invalid");
    assert!(invalid.is_current());
    assert!(!invalid.success);
    assert!(invalid.error_count() > 0);

    let repaired = single_file("pub fn repaired() -> f64 {\n    let x = 1.0;\n    x\n}\n");
    let valid = daemon.analyze(&request(2, &repaired)).expect("repaired");
    assert!(valid.is_current());
    assert!(valid.success, "repaired buffer must compile");

    let _ = std::fs::remove_dir_all(&work);
}

// ---------------------------------------------------------------------------
// Invalidation granularity across modules.
// ---------------------------------------------------------------------------

#[test]
fn signature_change_is_reported_in_the_caller_and_fixed_after_update() {
    let _guard = rustc_guard();
    let work = temp_dir("signature");
    let daemon = daemon_at(&work, 1, 16);
    let mut broken = fixture_files();
    apply(
        &mut broken,
        "src/math.rs",
        "pub fn scale(x: f64) -> f64 {",
        "pub fn scale(x: f64, factor: f64) -> f64 {",
    );
    apply(&mut broken, "src/math.rs", "x * SCALE", "x * factor");
    let report = daemon.analyze(&request(1, &broken)).expect("broken");
    assert!(!report.success);
    assert!(
        report
            .diagnostics
            .iter()
            .any(
                |diagnostic| diagnostic.file.as_deref() == Some("src/pipeline.rs")
                    && diagnostic.severity.is_error()
            ),
        "the caller in pipeline.rs must be re-checked: {:?}",
        diagnostics_of(&report)
    );
    let (cold_success, cold_diagnostics) = cold_signatures(&broken);
    assert!(
        !cold_success,
        "cold rustc must also reject the broken caller"
    );
    assert_eq!(
        diagnostics_of(&report),
        cold_diagnostics,
        "daemon diagnostics must equal cold rustc diagnostics"
    );

    let mut fixed = broken.clone();
    apply(
        &mut fixed,
        "src/pipeline.rs",
        "scale(clamp01(value))",
        "scale(clamp01(value), SCALE)",
    );
    let fixed_report = daemon.analyze(&request(2, &fixed)).expect("fixed");
    assert!(fixed_report.success, "{:?}", fixed_report.diagnostics);
    let (cold_fixed_success, cold_fixed_diagnostics) = cold_signatures(&fixed);
    assert!(cold_fixed_success);
    assert_eq!(diagnostics_of(&fixed_report), cold_fixed_diagnostics);

    let _ = std::fs::remove_dir_all(&work);
}

// ---------------------------------------------------------------------------
// Budgets, timeouts, user files, protocol.
// ---------------------------------------------------------------------------

#[test]
fn process_and_memory_budgets_are_enforced() {
    let _guard = rustc_guard();
    let work = temp_dir("budget");
    let daemon = daemon_at(&work, 1, 1);
    let mut states = Vec::new();
    for variant in 0..3 {
        let mut state = fixture_files();
        let lib = state
            .iter_mut()
            .find(|file| file.path == "src/lib.rs")
            .unwrap();
        lib.content.push_str(&format!("\n// Variant {variant}.\n"));
        states.push(state);
    }
    let mut handles = Vec::new();
    for (index, state) in states.into_iter().enumerate() {
        let daemon = daemon.clone();
        handles.push((daemon, state, index));
    }
    let mut all_fresh = true;
    for (daemon, state, index) in handles {
        let mut request = request((index + 1) as u64, &state);
        request.priority = Priority::Interactive;
        let report = daemon.analyze(&request).expect("budget analysis");

        all_fresh &= report.is_current() && report.success;
    }
    let stats = daemon.stats();
    assert!(all_fresh);
    assert!(
        stats.peak_active_jobs <= 1,
        "gate must enforce at most one compiler: {}",
        stats.peak_active_jobs
    );
    assert_eq!(stats.rustc_runs, 3);
    let (memory_entries, disk_entries) = daemon.cache_entries();
    assert!(memory_entries <= 1, "memory cache budget");
    assert!(
        disk_entries >= 3,
        "disk cache keeps content-addressed verdicts"
    );

    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn runaway_compiler_is_timed_out() {
    let _guard = rustc_guard();
    let work = temp_dir("timeout");
    let daemon = Daemon::open(DaemonConfig {
        rustc: rustc(),
        work_dir: work.clone(),
        max_concurrent_jobs: 1,
        max_cache_entries: 4,
        poll_interval_ms: 1,
        analysis_timeout_ms: 1,
        toolchain_override: None,
        auto_gc: ravum_daemon::AutoGcPolicy::default(),
    })
    .expect("timeout daemon");
    let report = daemon
        .analyze(&request(1, &fixture_files()))
        .expect("timeout report");
    assert_eq!(report.status, ReportStatus::TimedOut);
    assert!(!report.is_current());

    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn user_files_are_never_written() {
    let _guard = rustc_guard();
    let work = temp_dir("user-files");
    let user_dir = work.join("user-project");
    let user_file = user_dir.join("src").join("lib.rs");
    std::fs::create_dir_all(user_file.parent().unwrap()).unwrap();
    let original = "pub fn user_function(x: f64) -> f64 {\n    x + 1.0\n}\n";
    std::fs::write(&user_file, original).unwrap();
    let files = single_file(original);
    let daemon = daemon_at(&work.join("daemon"), 1, 8);
    let report = daemon.analyze(&request(1, &files)).expect("analysis");
    assert!(report.success);
    let on_disk = std::fs::read_to_string(&user_file).unwrap();
    assert_eq!(on_disk, original, "the daemon must never write user files");

    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn invalid_requests_are_rejected_before_any_compiler_runs() {
    let _guard = rustc_guard();
    let work = temp_dir("invalid-requests");
    let daemon = daemon_at(&work, 1, 8);

    let mut traversal = request(1, &single_file("pub fn ok() {}\n"));
    traversal.files[0].path = "../escape.rs".to_owned();
    traversal.root = "../escape.rs".to_owned();
    assert_eq!(
        daemon.analyze(&traversal).expect_err("traversal").kind,
        ErrorKind::InvalidRequest
    );

    let mut empty = request(2, &[]);
    empty.files.clear();
    assert_eq!(
        daemon.analyze(&empty).expect_err("empty unit").kind,
        ErrorKind::InvalidRequest
    );

    let mut reserved = request(3, &single_file("pub fn ok() {}\n"));
    reserved.context.flags = vec!["--out-dir=elsewhere".to_owned()];
    assert_eq!(
        daemon.analyze(&reserved).expect_err("reserved flag").kind,
        ErrorKind::InvalidRequest
    );

    let mut bad_crate = request(4, &single_file("pub fn ok() {}\n"));
    bad_crate.context.crate_name = "not-a-crate".to_owned();
    assert_eq!(
        daemon.analyze(&bad_crate).expect_err("crate name").kind,
        ErrorKind::InvalidRequest
    );

    let huge = "x".repeat(ravum_daemon::config::MAX_FILE_BYTES + 1);
    let mut oversized = single_file("pub fn ok() {}\n");
    oversized[0].content = huge;
    assert_eq!(
        daemon
            .analyze(&request(5, &oversized))
            .expect_err("oversize")
            .kind,
        ErrorKind::InvalidRequest
    );

    assert_eq!(
        daemon.stats().rustc_runs,
        0,
        "rejected requests must not spawn a compiler"
    );

    let _ = std::fs::remove_dir_all(&work);
}

// ---------------------------------------------------------------------------
// Separate binary: persistent JSON-lines protocol.
// ---------------------------------------------------------------------------

#[test]
fn binary_protocol_serves_multiple_analyses_and_shuts_down() {
    let _guard = rustc_guard();
    let work = temp_dir("binary-protocol");
    let binary = env!("CARGO_BIN_EXE_ravum-daemon");
    let mut child = Command::new(binary)
        .arg("--work-dir")
        .arg(&work)
        .arg("--max-jobs")
        .arg("1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("daemon binary");
    let mut stdin = child.stdin.take().expect("stdin");
    let content = "pub fn protocol_fn(x: f64) -> f64 {\n    x * 2.0\n}\n";
    let analyze = serde_json::json!({
        "id": "a1",
        "cmd": "analyze",
        "revision": 1,
        "label": "protocol",
        "root": "src/lib.rs",
        "files": [{"path": "src/lib.rs", "content": content}],
        "context": {"edition": "2021", "crate_name": "protocol_unit", "crate_type": "lib", "flags": []},
    });
    let repeat = serde_json::json!({
        "id": "a2",
        "cmd": "analyze",
        "revision": 2,
        "label": "protocol",
        "root": "src/lib.rs",
        "files": [{"path": "src/lib.rs", "content": content}],
        "context": {"edition": "2021", "crate_name": "protocol_unit", "crate_type": "lib", "flags": []},
    });
    for command in [
        analyze,
        repeat,
        serde_json::json!({"id": "s1", "cmd": "stats"}),
        serde_json::json!({"id": "x1", "cmd": "shutdown"}),
    ] {
        writeln!(stdin, "{command}").expect("write command");
    }
    drop(stdin);
    let mut stdout = String::new();
    child
        .stdout
        .take()
        .expect("stdout")
        .read_to_string(&mut stdout)
        .expect("read stdout");
    let status = child.wait().expect("wait");
    assert!(status.success(), "daemon binary must exit cleanly");

    let mut events = Vec::new();
    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        let value: Value = serde_json::from_str(line).expect("each output line must be JSON");
        events.push(value);
    }
    assert_eq!(
        events[0]["event"], "ready",
        "first line must announce readiness"
    );
    let first_report = &events[1]["report"];
    assert_eq!(events[1]["id"], "a1");
    assert_eq!(first_report["status"], "fresh");
    assert_eq!(first_report["cache"]["kind"], "miss");
    assert_eq!(first_report["success"], true);
    let second_report = &events[2]["report"];
    assert_eq!(events[2]["id"], "a2");
    assert_eq!(second_report["cache"]["kind"], "memory");
    assert_eq!(second_report["timings"]["analysis_ms"], 0);
    assert!(events[3]["stats"]["rustc_runs"].as_u64().unwrap_or(0) >= 1);
    assert_eq!(events[4]["shutdown"], true);

    let _ = std::fs::remove_dir_all(&work);
}
