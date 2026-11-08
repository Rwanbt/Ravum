//! Integration tests for the explicit retention GC of the analysis daemon.
//!
//! Every content is really analysed (a real `rustc` runs): the tests check that
//! GC keeps live content served from the cache without a compiler and that
//! collected content is really reconstructed. Compiler-spawning tests hold
//! [`RUSTC_LOCK`] so at most one compiler runs at a time on the shared machine.

use ravum_daemon::{
    CacheLookupKey, Daemon, DaemonConfig, DaemonError, ErrorKind, GcPolicy, SourceFile,
    ToolchainInfo,
};
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

static RUSTC_LOCK: Mutex<()> = Mutex::new(());
static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

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
        "ravum-daemon-gc-test-{name}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn daemon_at(work_dir: &Path) -> Daemon {
    Daemon::open(DaemonConfig {
        rustc: rustc(),
        work_dir: work_dir.to_path_buf(),
        max_concurrent_jobs: 1,
        max_cache_entries: 64,
        poll_interval_ms: 2,
        analysis_timeout_ms: 60_000,
        toolchain_override: None,
        auto_gc: ravum_daemon::AutoGcPolicy::default(),
    })
    .expect("daemon open")
}

fn content(seed: u32) -> String {
    format!(
        "//! GC integration fixture {seed}.\n\npub fn value_{seed}(x: f64) -> f64 {{\n    x * {}.0 + {seed}.0\n}}\n",
        seed + 1
    )
}

fn files(seed: u32) -> Vec<SourceFile> {
    vec![SourceFile {
        path: "src/lib.rs".to_owned(),
        content: content(seed),
    }]
}

fn request(revision: u64, seed: u32) -> ravum_daemon::AnalysisRequest {
    ravum_daemon::AnalysisRequest {
        revision,
        label: format!("gc-test-{seed}"),
        root: "src/lib.rs".to_owned(),
        files: files(seed),
        context: ravum_daemon::BuildContext {
            edition: "2021".to_owned(),
            crate_name: "gc_unit".to_owned(),
            crate_type: "lib".to_owned(),
            flags: Vec::new(),
        },
        priority: ravum_daemon::Priority::Normal,
    }
}

fn cache_key_for(context: &ravum_daemon::BuildContext, toolchain: &ToolchainInfo, seed: u32) -> String {
    // Recompute the daemon identities from the request content exactly like the
    // daemon does, so tests can name the keys the GC deletes.
    let request = request(1, seed);
    let identity = request.identity(toolchain).expect("identity");
    let context_id = context.id(toolchain);
    CacheLookupKey::new(&identity.snapshot_id, &context_id, &toolchain.id()).key
}

fn snapshot_for(toolchain: &ToolchainInfo, seed: u32) -> String {
    request(1, seed).identity(toolchain).expect("identity").snapshot_id
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

/// Analyses three distinct contents (real compilations) and returns the daemon.
fn seed_three(work: &Path) -> (Daemon, ravum_daemon::BuildContext, ToolchainInfo) {
    let daemon = daemon_at(work);
    let context = request(1, 1).context;
    for seed in 1..=3 {
        let report = daemon.analyze(&request(seed as u64, seed)).expect("analysis");
        assert!(report.is_current() && report.success, "seed {seed} must compile");
        assert!(!report.cache.hit, "distinct contents must be cache misses");
    }
    let stats = daemon.stats();
    assert_eq!(stats.rustc_runs, 3, "three real compiler runs");
    let toolchain = daemon.toolchain().clone();
    (daemon, context, toolchain)
}

// ---------------------------------------------------------------------------
// Core behaviour: keep recent, delete old, recent hits, old recompiles.
// ---------------------------------------------------------------------------

#[test]
fn gc_keep_recent_serves_recent_without_rustc_and_recompiles_old() {
    let _guard = rustc_guard();
    let work = temp_dir("keep-recent");
    let daemon = daemon_at(&work);
    let context = request(1, 1).context;
    for seed in 1..=4u32 {
        let report = daemon.analyze(&request(seed as u64, seed)).expect("analysis");
        assert!(report.is_current() && report.success);
        assert!(!report.cache.hit);
    }
    assert_eq!(daemon.stats().rustc_runs, 4);
    let toolchain = daemon.toolchain().clone();
    let key1 = cache_key_for(&context, &toolchain, 1);
    let key2 = cache_key_for(&context, &toolchain, 2);
    let key3 = cache_key_for(&context, &toolchain, 3);
    let snapshot1 = snapshot_for(&toolchain, 1);
    let snapshot2 = snapshot_for(&toolchain, 2);
    let snapshot3 = snapshot_for(&toolchain, 3);
    let snapshot4 = snapshot_for(&toolchain, 4);
    assert!(work.join("cache").join(format!("{key1}.json")).exists());
    assert!(work
        .join("snapshots")
        .join(&snapshot1)
        .join("manifest.json")
        .exists());

    let policy = GcPolicy {
        max_cache_bytes: Some(1),
        max_snapshot_bytes: Some(1),
        keep_recent: 2,
        ..Default::default()
    };
    let report = daemon.gc(&policy).expect("gc");
    assert_eq!(report.mode, ravum_daemon::GcMode::Daemon);
    assert!(!report.dry_run);
    // Newest two served contents (2 and 3) are protected; content 1 is collected.
    assert!(
        report.deleted.cache_keys.contains(&key1),
        "oldest verdict must be collected: {:?}",
        report.deleted.cache_keys
    );
    assert!(report.deleted.cache_keys.contains(&key2));
    assert!(!report.deleted.cache_keys.contains(&key3));
    assert!(report.deleted.snapshots.contains(&snapshot1));
    assert!(report.deleted.snapshots.contains(&snapshot2));
    assert!(!work.join("cache").join(format!("{key1}.json")).exists());
    assert!(!work.join("snapshots").join(&snapshot1).exists());
    assert!(work.join("snapshots").join(&snapshot3).exists());
    assert!(work.join("snapshots").join(&snapshot4).exists());
    assert!(report.freed_bytes > 0, "collection must reclaim bytes");
    // keep_recent wins over the byte cap: the cap is reported unmet, not hidden.
    assert!(!report.budgets.cache.met);

    // A recent content is served from the cache: no compiler runs.
    let runs_before = daemon.stats().rustc_runs;
    let recent = daemon.analyze(&request(10, 4)).expect("recent analysis");
    assert!(recent.cache.hit, "recent content must be a cache hit");
    assert_eq!(recent.timings.analysis_ms, 0);
    assert_eq!(
        daemon.stats().rustc_runs,
        runs_before,
        "a cache hit must not spawn rustc"
    );

    // A collected content is rebuilt by a real compiler run.
    let old = daemon.analyze(&request(11, 1)).expect("old analysis");
    assert!(!old.cache.hit, "collected content must be a miss");
    assert!(old.success && old.timings.analysis_ms > 0);
    assert_eq!(daemon.stats().rustc_runs, runs_before + 1);

    // After a restart the kept content is still served from the disk cache.
    drop(daemon);
    let reopened = daemon_at(&work);
    let disk_hit = reopened.analyze(&request(1, 4)).expect("replay after restart");
    assert!(disk_hit.cache.hit);
    assert_eq!(disk_hit.cache.kind.as_str(), "disk");
    // Content 2 was collected by GC and never recompiled in this session: after a
    // restart it is still a miss, proving the disk entry was really removed.
    let rebuilt = reopened.analyze(&request(2, 2)).expect("rebuilt after restart");
    assert!(!rebuilt.cache.hit, "collected content must not be served after restart");
    assert_eq!(reopened.stats().rustc_runs, 1);

    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn gc_never_deletes_current_content() {
    let _guard = rustc_guard();
    let work = temp_dir("current");
    let (daemon, _context, toolchain) = seed_three(&work);
    // The last analysed content is the "current revision" content.
    let current_snapshot = snapshot_for(&toolchain, 3);
    let policy = GcPolicy {
        max_cache_bytes: Some(1),
        max_snapshot_bytes: Some(1),
        max_incremental_bytes: Some(1),
        max_total_bytes: Some(1),
        keep_recent: 1,
        ..Default::default()
    };
    let report = daemon.gc(&policy).expect("gc");
    assert_eq!(
        report.protected.last_snapshot_id.as_deref(),
        Some(current_snapshot.as_str())
    );
    assert!(report.kept.recent.iter().any(|entry| {
        entry.snapshot_id == current_snapshot && entry.revision == 3
    }));
    assert!(work
        .join("snapshots")
        .join(&current_snapshot)
        .join("manifest.json")
        .exists());

    // The current content is still served without a compiler.
    let runs_before = daemon.stats().rustc_runs;
    let current = daemon.analyze(&request(20, 3)).expect("current analysis");
    assert!(current.cache.hit);
    assert_eq!(daemon.stats().rustc_runs, runs_before);

    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn gc_is_idempotent() {
    let _guard = rustc_guard();
    let work = temp_dir("idempotent");
    let (daemon, _context, _toolchain) = seed_three(&work);
    let policy = GcPolicy {
        max_cache_bytes: Some(1),
        keep_recent: 1,
        ..Default::default()
    };
    let first = daemon.gc(&policy).expect("first gc");
    let second = daemon.gc(&policy).expect("second gc");
    assert!(first.freed_bytes > 0);
    assert_eq!(second.freed_bytes, 0, "nothing left to collect");
    assert_eq!(second.deleted.entry_count(), 0);
    assert_eq!(second.before, second.after);
    assert_eq!(second.duration_ms < 60_000, true);
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn gc_dry_run_plans_without_deleting() {
    let _guard = rustc_guard();
    let work = temp_dir("dry-run");
    let (daemon, _context, _toolchain) = seed_three(&work);
    let cache_before = dir_size(&work.join("cache"));
    let snapshots_before = dir_size(&work.join("snapshots"));
    let policy = GcPolicy {
        max_cache_bytes: Some(1),
        max_snapshot_bytes: Some(1),
        keep_recent: 1,
        dry_run: true,
        ..Default::default()
    };
    let report = daemon.gc(&policy).expect("dry-run gc");
    assert!(report.dry_run);
    assert_eq!(report.freed_bytes, 0);
    assert!(!report.planned.cache_keys.is_empty(), "plan must list victims");
    assert_eq!(report.before, report.after);
    assert_eq!(dir_size(&work.join("cache")), cache_before);
    assert_eq!(dir_size(&work.join("snapshots")), snapshots_before);
    // A real run then executes the plan.
    let real = daemon
        .gc(&GcPolicy {
            dry_run: false,
            ..policy.clone()
        })
        .expect("real gc");
    assert!(real.freed_bytes > 0);
    assert_eq!(real.deleted.entry_count(), report.planned.entry_count());
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn gc_stats_are_coherent_with_filesystem_and_report() {
    let _guard = rustc_guard();
    let work = temp_dir("stats");
    let (daemon, _context, _toolchain) = seed_three(&work);
    assert_eq!(daemon.stats().gc_runs, 0);
    let policy = GcPolicy {
        max_cache_bytes: Some(1),
        max_snapshot_bytes: Some(1),
        max_incremental_bytes: Some(1),
        keep_recent: 1,
        ..Default::default()
    };
    let report = daemon.gc(&policy).expect("gc");
    let stats = daemon.stats();
    assert_eq!(stats.gc_runs, 1);
    assert_eq!(stats.gc_freed_bytes, report.freed_bytes);
    assert_eq!(stats.gc_deleted_entries, report.deleted.entry_count() as u64);
    // Report sizes match the real filesystem (cache + snapshots + build state).
    assert_eq!(report.after.cache_bytes, dir_size(&work.join("cache")));
    assert_eq!(report.after.snapshots_bytes, dir_size(&work.join("snapshots")));
    let build_state = dir_size(&work.join("incremental"))
        + dir_size(&work.join("out"))
        + dir_size(&work.join("units"));
    assert_eq!(report.after.incremental_bytes, build_state);
    assert_eq!(
        report.after.total_bytes,
        report.after.cache_bytes + report.after.snapshots_bytes + report.after.incremental_bytes
    );
    assert_eq!(
        report.before.total_bytes.saturating_sub(report.freed_bytes),
        report.after.total_bytes
    );
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn invalid_gc_policies_are_rejected() {
    let work = temp_dir("invalid-policy");
    let daemon = daemon_at(&work);
    let zero_keep = GcPolicy {
        keep_recent: 0,
        ..Default::default()
    };
    let error = daemon.gc(&zero_keep).expect_err("keep_recent=0 must be rejected");
    assert_eq!(error.kind, ErrorKind::InvalidRequest);
    let zero_cap = GcPolicy {
        max_cache_bytes: Some(0),
        ..Default::default()
    };
    let error = daemon.gc(&zero_cap).expect_err("cap 0 must be rejected");
    assert_eq!(error.kind, ErrorKind::InvalidRequest);
    // A rejected policy must not have run any collection.
    assert_eq!(daemon.stats().gc_runs, 0);
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn gc_tolerates_partial_work_directory() {
    let _guard = rustc_guard();
    let work = temp_dir("partial");
    let (daemon, _context, _toolchain) = seed_three(&work);
    // Simulate a partially cleaned / partially created work directory.
    let _ = std::fs::remove_dir_all(work.join("snapshots"));
    let _ = std::fs::remove_dir_all(work.join("incremental"));
    let _ = std::fs::remove_dir_all(work.join("out"));
    let _ = std::fs::remove_dir_all(work.join("units"));
    let report = daemon
        .gc(&GcPolicy {
            max_snapshot_bytes: Some(1),
            max_incremental_bytes: Some(1),
            keep_recent: 1,
            ..Default::default()
        })
        .expect("gc on partial directory");
    assert_eq!(report.after.snapshots, 0);
    assert_eq!(report.after.incremental_contexts, 0);
    // The daemon still answers (recompiles if the verdict was collected).
    let report = daemon.analyze(&request(30, 3)).expect("analysis after partial gc");
    assert!(report.is_current());
    let _ = std::fs::remove_dir_all(&work);
}

// ---------------------------------------------------------------------------
// Access paths: one-shot CLI and JSON-lines server command.
// ---------------------------------------------------------------------------

fn run_binary(args: &[&str], stdin: Option<&str>) -> (i32, String, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ravum-daemon"));
    command.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    if stdin.is_some() {
        command.stdin(Stdio::piped());
    } else {
        command.stdin(Stdio::null());
    }
    let mut child = command.spawn().expect("spawn ravum-daemon");
    if let Some(input) = stdin {
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(input.as_bytes())
            .expect("write stdin");
    }
    let output = child.wait_with_output().expect("wait daemon");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

fn populate_work_dir(work: &Path) {
    let daemon = daemon_at(work);
    for seed in 1..=2 {
        let report = daemon.analyze(&request(seed as u64, seed)).expect("analysis");
        assert!(!report.cache.hit);
    }
    drop(daemon);
}

#[test]
fn one_shot_cli_gc_reports_json_and_exit_codes() {
    let _guard = rustc_guard();
    let work = temp_dir("cli-gc");
    populate_work_dir(&work);
    let work_text = work.display().to_string();

    // Valid policy with dry-run: exit 0, one JSON report on stdout.
    let (code, stdout, stderr) = run_binary(
        &["--gc", "--work-dir", &work_text, "--dry-run", "--keep-recent", "1", "--max-cache-bytes", "1"],
        None,
    );
    assert_eq!(code, 0, "dry-run gc must exit 0; stderr={stderr}");
    let report: Value = serde_json::from_str(stdout.trim()).expect("stdout must be one JSON report");
    assert_eq!(report["kind"], "ravum-daemon-gc-report");
    assert_eq!(report["mode"], "one_shot");
    assert_eq!(report["dry_run"], true);
    assert!(report["planned"]["cache_keys"].as_array().unwrap().len() >= 1);
    assert_eq!(report["freed_bytes"], 0);
    assert!(work.join("cache").exists());

    // Inline JSON policy, real run: exit 0 and bytes really freed.
    let policy = r#"{"max_cache_bytes":1,"max_snapshot_bytes":1,"keep_recent":1}"#;
    let (code, stdout, stderr) = run_binary(&["--gc", "--work-dir", &work_text, "--policy", policy], None);
    assert_eq!(code, 0, "real gc must exit 0; stderr={stderr}");
    let report: Value = serde_json::from_str(stdout.trim()).expect("valid JSON report");
    assert_eq!(report["dry_run"], false);
    assert!(report["freed_bytes"].as_u64().unwrap_or(0) > 0);
    assert!(!report["deleted"]["cache_keys"].as_array().unwrap().is_empty());

    // Invalid policy: exit 1 with an explicit error.
    let (code, _stdout, stderr) = run_binary(
        &["--gc", "--work-dir", &work_text, "--keep-recent", "0"],
        None,
    );
    assert_eq!(code, 1, "invalid policy must exit 1; stderr={stderr}");
    assert!(
        stderr.to_lowercase().contains("keep_recent") || stderr.to_lowercase().contains("policy"),
        "error must explain the invalid policy: {stderr}"
    );

    // Help must succeed and document the GC mode.
    let (code, stdout, _stderr) = run_binary(&["--help"], None);
    assert_eq!(code, 0);
    assert!(stdout.contains("--gc"));
    assert!(stdout.contains("--keep-recent"));

    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn server_gc_command_protects_recent_and_collects_old() {
    let _guard = rustc_guard();
    let work = temp_dir("server-gc");
    let work_text = work.display().to_string();
    let mut input = String::new();
    for seed in 1..=2u32 {
        input.push_str(
            &serde_json::json!({
                "id": format!("a{seed}"),
                "cmd": "analyze",
                "revision": seed,
                "label": format!("srv-{seed}"),
                "root": "src/lib.rs",
                "files": files(seed).iter().map(|file| serde_json::json!({
                    "path": file.path,
                    "content": file.content,
                })).collect::<Vec<_>>(),
                "context": {
                    "edition": "2021",
                    "crate_name": "gc_unit",
                    "crate_type": "lib",
                    "flags": [],
                },
            })
            .to_string(),
        );
        input.push('\n');
    }
    input.push_str(
        &serde_json::json!({
            "id": "g1",
            "cmd": "gc",
            "policy": {"max_cache_bytes": 1, "max_snapshot_bytes": 1, "keep_recent": 1},
        })
        .to_string(),
    );
    input.push('\n');
    input.push_str("{\"id\":\"s1\",\"cmd\":\"stats\"}\n");
    input.push_str("{\"id\":\"x1\",\"cmd\":\"shutdown\"}\n");
    let (code, stdout, stderr) = run_binary(&["--work-dir", &work_text], Some(&input));
    assert_eq!(code, 0, "server must exit cleanly; stderr={stderr}");
    let events: Vec<Value> = stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("each line is JSON"))
        .collect();
    assert_eq!(events[0]["event"], "ready");
    let gc_response = events
        .iter()
        .find(|event| event["id"] == "g1")
        .expect("gc response");
    assert_eq!(gc_response["ok"], true);
    let report = &gc_response["report"];
    assert_eq!(report["kind"], "ravum-daemon-gc-report");
    assert_eq!(report["mode"], "daemon");
    assert!(report["freed_bytes"].as_u64().unwrap_or(0) > 0);
    let stats = events
        .iter()
        .find(|event| event["id"] == "s1")
        .expect("stats response");
    assert!(stats["stats"]["gc_runs"].as_u64().unwrap_or(0) >= 1);
    assert!(stats["stats"]["gc_freed_bytes"].as_u64().unwrap_or(0) > 0);

    let _ = std::fs::remove_dir_all(&work);
}

/// The `DaemonError` import is used by the error-kind assertions above; keep the
/// type explicitly referenced so refactors do not silently drop the check.
#[allow(dead_code)]
fn _assert_error_type(_: DaemonError) {}
