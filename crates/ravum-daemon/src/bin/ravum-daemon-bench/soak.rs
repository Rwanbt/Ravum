//! Long soak with automatic background GC (P0-H suite).
//!
//! Two campaigns run in one evidence file:
//!
//! * **auto** — a daemon with `AutoGcPolicy` enabled (low thresholds) drives
//!   `-soak-requests` real analyses with a pacing delay, so the campaign lasts
//!   minutes. Every `-soak-sample-every` requests it records: process working set
//!   (RSS), on-disk state, cache hits/misses, `auto_gc_runs`/`auto_gc_last`,
//!   journal lines and cumulative analysis latency (p50/p95). After the loop it
//!   waits for the maintenance thread to settle, verifies that the most recent
//!   content is still a cache hit and that an old content is still answered;
//! * **control** — the same request pattern without automatic GC, shorter by
//!   design, to compare disk boundedness and RSS drift.
//!
//! Everything runs in-process through the real `Daemon` API; every "miss" spawns
//! the public `rustc` CLI. The long soak is bounded (minutes, not hours): it
//! detects gross growth/leaks and proves the automatic GC keeps the disk bounded,
//! it does not claim long-lived production stability.

use crate::harness::{git_json, load_json, machine_json, toolchain_json, write_evidence};
use ravum_daemon::{
    AnalysisRequest, AutoGcPolicy, BuildContext, Daemon, DaemonConfig, Priority, SourceFile,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

pub struct SoakOptions {
    pub repo_root: PathBuf,
    pub work_root: PathBuf,
    pub requests: usize,
    pub seconds: u64,
    pub pace_ms: u64,
    pub sample_every: usize,
    pub control_requests: usize,
    pub control_seconds: u64,
    pub control_pace_ms: u64,
    pub min_total_bytes: u64,
    pub max_total_bytes: u64,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
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

fn journal_lines(work_dir: &Path) -> usize {
    std::fs::read_to_string(work_dir.join("gc-auto.log"))
        .map(|text| text.lines().filter(|line| !line.trim().is_empty()).count())
        .unwrap_or(0)
}

fn content(seed: u64) -> String {
    format!(
        "//! soak request {seed}\npub fn soak_{seed}(x: f64) -> f64 {{\n    x * {}.0 + 1.0\n}}\n",
        seed + 2
    )
}

fn context(crate_name: &str) -> BuildContext {
    BuildContext {
        edition: "2021".to_owned(),
        crate_name: crate_name.to_owned(),
        crate_type: "lib".to_owned(),
        flags: Vec::new(),
    }
}

fn auto_policy(min_total_bytes: u64, max_total_bytes: u64) -> AutoGcPolicy {
    AutoGcPolicy {
        enabled: true,
        check_every_requests: 5,
        min_total_bytes: Some(min_total_bytes),
        max_total_bytes: Some(max_total_bytes),
        max_cache_bytes: Some(40_000),
        max_snapshot_bytes: Some(40_000),
        max_incremental_bytes: Some(120_000),
        keep_recent: 6,
        idle_only: true,
    }
}

fn daemon_config(work_dir: &Path, auto_gc: AutoGcPolicy) -> DaemonConfig {
    DaemonConfig {
        rustc: crate::harness::rustc_path(),
        work_dir: work_dir.to_path_buf(),
        max_concurrent_jobs: 1,
        max_cache_entries: 512,
        poll_interval_ms: 2,
        analysis_timeout_ms: 120_000,
        toolchain_override: None,
        auto_gc,
    }
}

struct CampaignOutcome {
    summary: Value,
    valid: bool,
}

/// Runs one campaign. `auto_gc.enabled` decides whether the daemon's maintenance
/// thread is active; everything else is identical.
fn run_campaign(
    name: &str,
    work_dir: &Path,
    auto_gc: AutoGcPolicy,
    requests: usize,
    seconds: u64,
    pace_ms: u64,
    sample_every: usize,
) -> Result<CampaignOutcome, String> {
    let _ = std::fs::remove_dir_all(work_dir);
    std::fs::create_dir_all(work_dir)
        .map_err(|error| format!("cannot create {name} work dir: {error}"))?;
    let daemon = Daemon::open(daemon_config(work_dir, auto_gc.clone()))
        .map_err(|error| format!("cannot open {name} daemon: {error}"))?;
    let mut revision: u64 = 0;
    let mut latencies: Vec<u64> = Vec::new();
    let mut failures: Vec<Value> = Vec::new();
    let mut samples: Vec<Value> = Vec::new();
    let mut seen: HashMap<u64, (String, String)> = HashMap::new();
    let mut last_new: Option<(u64, String, String)> = None;
    let mut first_new: Option<(u64, String, String)> = None;

    let mut disk_max;
    let mut rss_max;
    let started = Instant::now();
    let deadline = started + Duration::from_secs(seconds.max(1));
    let disk_start = dir_size(work_dir);
    disk_max = disk_start;
    let rss_start = ravum_daemon::sysinfo::capture(20)
        .daemon_working_set_bytes
        .unwrap_or(0);
    rss_max = rss_start;
    let mut journal_max = 0usize;
    let mut index = 0usize;
    println!(
        "[soak:{name}] auto={} requests={requests} seconds={seconds} pace={pace_ms}ms sample_every={sample_every}",
        auto_gc.enabled
    );

    while index < requests && Instant::now() < deadline {
        index += 1;
        revision += 1;
        let context_index = (index - 1) % 3;
        let crate_name = format!("soak_unit_{context_index}");
        let (seed, content_text, context_name, expect_hit) = if index % 4 == 0 {
            match &last_new {
                Some((seed, content, context)) => (*seed, content.clone(), context.clone(), true),
                None => {
                    let seed = index as u64;
                    let text = content(seed);
                    seen.insert(seed, (text.clone(), crate_name.clone()));
                    last_new = Some((seed, text.clone(), crate_name.clone()));
                    if first_new.is_none() {
                        first_new = Some((seed, text.clone(), crate_name.clone()));
                    }
                    (seed, text, crate_name, false)
                }
            }
        } else {
            let seed = index as u64;
            let text = content(seed);
            seen.insert(seed, (text.clone(), crate_name.clone()));
            last_new = Some((seed, text.clone(), crate_name.clone()));
            if first_new.is_none() {
                first_new = Some((seed, text.clone(), crate_name.clone()));
            }
            (seed, text, crate_name, false)
        };
        let request = AnalysisRequest {
            revision,
            label: format!("{name}-{seed}"),
            root: "src/lib.rs".to_owned(),
            files: vec![SourceFile {
                path: "src/lib.rs".to_owned(),
                content: content_text,
            }],
            context: context(&context_name),
            priority: Priority::Normal,
        };
        let started_at = Instant::now();
        let report = daemon
            .analyze(&request)
            .map_err(|error| format!("{name} analysis failed: {error}"))?;
        let wall_ms = started_at.elapsed().as_millis() as u64;
        latencies.push(wall_ms);

        let fresh = report.is_current();
        let success = report.success;
        if expect_hit && report.cache.kind.as_str() == "miss" {
            failures.push(json!({"index": index, "error": "expected cache hit", "got": "miss"}));
        }
        if !expect_hit && report.cache.kind.as_str() != "miss" {
            failures.push(json!({"index": index, "error": "expected cache miss", "got": report.cache.kind.as_str()}));
        }
        if !fresh || !success {
            failures.push(json!({
                "index": index,
                "status": report.status.as_str(),
                "success": report.success,
            }));
        }
        if index % sample_every == 0 || index == 1 || index == requests {
            let disk = dir_size(work_dir);
            if disk > disk_max {
                disk_max = disk;
            }
            let rss = ravum_daemon::sysinfo::capture(20)
                .daemon_working_set_bytes
                .unwrap_or(0);
            if rss > rss_max {
                rss_max = rss;
            }
            let journal = journal_lines(work_dir);
            if journal > journal_max {
                journal_max = journal;
            }
            let stats = daemon.stats();
            let auto = &stats.auto_gc;
            let latency_stats = crate::harness::stats(&latencies);
            samples.push(json!({
                "request": index,
                "elapsed_s": started.elapsed().as_secs(),
                "disk_bytes": disk,
                "working_set_bytes": rss,
                "cache_hits": stats.cache_hits_memory + stats.cache_hits_disk,
                "cache_misses": stats.cache_misses,
                "rustc_runs": stats.rustc_runs,
                "auto_gc_runs": auto.runs,
                "auto_gc_failures": auto.failures,
                "journal_lines": journal,
                "last_latency_ms": report.timings.total_ms,
                "latency_p50_ms": latency_stats["p50"],
                "latency_p95_ms": latency_stats["p95"],
            }));
            println!(
                "[soak:{name}] {index}/{requests} cache={} total={}ms rss={}KiB disk={}KiB auto_runs={}",
                report.cache.kind.as_str(),
                report.timings.total_ms,
                rss / 1024,
                disk / 1024,
                auto.runs
            );
        }
        if pace_ms > 0 && index < requests {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let pause = Duration::from_millis(pace_ms).min(remaining);
            if !pause.is_zero() {
                std::thread::sleep(pause);
            }
        }
    }

    let settled = daemon.wait_auto_gc_idle(30_000);
    let stats_before_verification = daemon.stats();
    let mut verification = json!({});
    let recent_hit;
    let old_success;
    if let Some((seed, text, context_name)) = &last_new {
        let request = AnalysisRequest {
            revision: revision + 1,
            label: format!("{name}-verify-recent"),
            root: "src/lib.rs".to_owned(),
            files: vec![SourceFile {
                path: "src/lib.rs".to_owned(),
                content: text.clone(),
            }],
            context: context(context_name),
            priority: Priority::Normal,
        };
        let report = daemon
            .analyze(&request)
            .map_err(|error| format!("{name} recent verification failed: {error}"))?;
        recent_hit = report.cache.kind.as_str() != "miss" && report.timings.analysis_ms == 0;
        verification["recent"] = json!({
            "seed": seed,
            "cache_kind": report.cache.kind.as_str(),
            "analysis_ms": report.timings.analysis_ms,
            "hit_without_compiler": recent_hit,
        });
    } else {
        recent_hit = false;
    }
    if let Some((seed, text, context_name)) = &first_new {
        let request = AnalysisRequest {
            revision: revision + 2,
            label: format!("{name}-verify-old"),
            root: "src/lib.rs".to_owned(),
            files: vec![SourceFile {
                path: "src/lib.rs".to_owned(),
                content: text.clone(),
            }],
            context: context(context_name),
            priority: Priority::Normal,
        };
        let report = daemon
            .analyze(&request)
            .map_err(|error| format!("{name} old verification failed: {error}"))?;
        old_success = report.is_current() && report.success;
        verification["old"] = json!({
            "seed": seed,
            "cache_kind": report.cache.kind.as_str(),
            "analysis_ms": report.timings.analysis_ms,
            "success": old_success,
        });
    } else {
        old_success = false;
    }
    // Verification analyses are requests too: wait for the maintenance thread to
    // settle again so stats/journal/disk are sampled at a coherent point.
    let verification_settled = daemon.wait_auto_gc_idle(20_000);
    let final_stats = daemon.stats();
    let disk_end = dir_size(work_dir);
    let rss_end = ravum_daemon::sysinfo::capture(20)
        .daemon_working_set_bytes
        .unwrap_or(0);
    let journal_end = journal_lines(work_dir);
    let duration_s = started.elapsed().as_secs();
    daemon.shutdown_auto_gc();
    let _ = daemon.wait_auto_gc_idle(5_000);
    drop(daemon);

    let latency_stats = crate::harness::stats(&latencies);
    let failures_count = failures.len();
    let auto_valid = if auto_gc.enabled {
        failures_count == 0
            && settled
            && verification_settled
            && final_stats.auto_gc.runs >= 1
            && final_stats.auto_gc.failures == 0
            && recent_hit
            && old_success
    } else {
        failures_count == 0 && old_success
    };
    let summary = json!({
        "name": name,
        "auto_gc_enabled": auto_gc.enabled,
        "requests": index,
        "duration_s": duration_s,
        "failures": failures_count,
        "failure_details": failures,
        "latency_ms": latency_stats,
        "cache": {
            "hits": final_stats.cache_hits_memory + final_stats.cache_hits_disk,
            "misses": final_stats.cache_misses,
            "rustc_runs": final_stats.rustc_runs,
        },
        "auto_gc": final_stats.auto_gc,
        "auto_gc_before_verification": stats_before_verification.auto_gc,
        "disk_bytes": {
            "start": disk_start,
            "max": disk_max,
            "end": disk_end,
            "growth": disk_end as i64 - disk_start as i64,
        },
        "rss_bytes": {
            "start": rss_start,
            "max": rss_max,
            "end": rss_end,
            "drift": rss_end as i64 - rss_start as i64,
        },
        "journal_lines": journal_end,
        "settled": settled && verification_settled,
        "verification": verification,
        "samples": samples,
        "policy": auto_gc.summary(),
    });
    Ok(CampaignOutcome {
        summary,
        valid: auto_valid,
    })
}

pub fn run(options: &SoakOptions) -> Result<Value, String> {
    if options.requests < 8 || options.control_requests < 8 {
        return Err("--soak-requests/--soak-control-requests must be >= 8".to_owned());
    }
    if options.max_total_bytes == 0 || options.max_total_bytes >= options.min_total_bytes {
        return Err(
            "--soak-max-total-bytes must be >= 1 and strictly below --soak-min-total-bytes"
                .to_owned(),
        );
    }
    let (toolchain, toolchain_evidence) = toolchain_json(&options.repo_root)?;
    let rustc = crate::harness::rustc_path();
    std::fs::create_dir_all(&options.work_root)
        .map_err(|error| format!("cannot create soak work root: {error}"))?;
    let load_start = load_json(&[], &rustc);
    let started_unix = now_unix();
    let auto = run_campaign(
        "auto",
        &options.work_root.join("auto"),
        auto_policy(options.min_total_bytes, options.max_total_bytes),
        options.requests,
        options.seconds,
        options.pace_ms,
        options.sample_every,
    )?;
    let control = run_campaign(
        "control",
        &options.work_root.join("control"),
        AutoGcPolicy::default(),
        options.control_requests,
        options.control_seconds,
        options.control_pace_ms,
        options.sample_every,
    )?;
    let load_end = load_json(&[], &rustc);
    let valid = auto.valid && control.valid;
    let comparison = json!({
        "auto_requests": auto.summary["requests"],
        "control_requests": control.summary["requests"],
        "auto_disk_end_bytes": auto.summary["disk_bytes"]["end"],
        "control_disk_end_bytes": control.summary["disk_bytes"]["end"],
        "auto_disk_max_bytes": auto.summary["disk_bytes"]["max"],
        "control_disk_max_bytes": control.summary["disk_bytes"]["max"],
        "auto_gc_runs": auto.summary["auto_gc"]["runs"],
        "auto_gc_failures": auto.summary["auto_gc"]["failures"],
        "auto_rss_drift_bytes": auto.summary["rss_bytes"]["drift"],
        "control_rss_drift_bytes": control.summary["rss_bytes"]["drift"],
        "note": "the auto campaign is longer by design; compare disk boundedness, failure counts and RSS drift, not absolute durations",
    });
    Ok(json!({
        "schema_version": 1,
        "kind": "mvp0b-daemon-gc-auto-2026-11-08",
        "generated_unix": started_unix,
        "machine": machine_json(),
        "toolchain": {
            "release": toolchain.release,
            "host": toolchain.host,
            "commit_hash": toolchain.commit_hash,
        },
        "toolchain_evidence": toolchain_evidence,
        "git": git_json(&options.repo_root),
        "command": format!(
            "cargo run -q -p ravum-daemon --bin ravum-daemon-bench -- --suite soak --soak-requests {} --soak-seconds {} --soak-pace-ms {}",
            options.requests, options.seconds, options.pace_ms
        ),
        "load_start": load_start,
        "load_end": load_end,
        "auto": auto.summary,
        "control": control.summary,
        "comparison": comparison,
        "valid": valid,
        "limits": [
            "bounded soak (minutes, not hours): it detects gross growth/leaks and proves automatic GC keeps the disk bounded; it is not a multi-hour production soak",
            "single daemon process and one sequential client; the concurrent path is covered by integration tests, not by this soak",
            "the daemon runs in-process for the soak, so RSS is the benchmark process working set (daemon + small harness overhead)",
            "automatic GC can only collect whole build contexts: rustc incremental internals are never compacted, so an active context is a floor on disk usage",
            "the control campaign is shorter by design and runs after the auto campaign; both share the machine (see load_start/load_end)",
        ],
    }))
}

pub fn write(path: &Path, value: &Value) -> Result<(), String> {
    write_evidence(path, value)
}

/// Keeps the temp-dir sequence meaningful for future campaign extensions.
#[allow(dead_code)]
fn unique_suffix() -> u64 {
    TEMP_SEQ.fetch_add(1, Ordering::Relaxed)
}
