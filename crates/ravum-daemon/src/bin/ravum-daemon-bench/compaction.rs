//! Incremental-session compaction measurement (P0-H §13).
//!
//! 1. accumulate real state with ~20 distinct edits under one build context;
//! 2. record the pre-compaction analysis latency of one new content (baseline);
//! 3. run a GC with `compact_incremental=true, keep_incremental_sessions=1`;
//! 4. record disk/sessions before and after, GC duration and bytes freed;
//! 5. analyse a brand-new content after compaction and compare its `analysis_ms`
//!    with the baseline and with a cold rebuild (fresh work directory, no state).
//!    If the retained session is not reused, the numbers say so instead of the
//!    documentation claiming a gain.

use crate::edits::{build_states, categories, CategoryKind};
use crate::harness::{
    base_files, git_json, load_fixture, load_json, machine_json, toolchain_json, write_evidence,
    Fixture,
};
use ravum_daemon::{AnalysisRequest, GcPolicy, Priority, SourceFile};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

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

fn count_sessions(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("s-") {
                found.push(path);
            } else {
                stack.push(path);
            }
        }
    }
    found
}

fn distinct_states(fixture: &Fixture, wanted: usize) -> Result<Vec<Vec<SourceFile>>, String> {
    let base = base_files(fixture);
    let mut states = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for category in categories() {
        if category.kind != CategoryKind::EditChain {
            continue;
        }
        let built = build_states(&base, category.steps.len(), &category)?;
        for state in built {
            let mut pairs: Vec<(&str, &str)> = state
                .iter()
                .map(|file| (file.path.as_str(), file.content.as_str()))
                .collect();
            pairs.sort();
            let mut text = String::new();
            for (path, content) in pairs {
                text.push_str(path);
                text.push('\0');
                text.push_str(content);
                text.push('\0');
            }
            if seen.insert(ravum_daemon::hash::sha256_hex(text.as_bytes())) {
                states.push(state);
                if states.len() >= wanted {
                    return Ok(states);
                }
            }
        }
    }
    Ok(states)
}

fn new_content(seed: u64) -> String {
    format!(
        "//! compaction measurement {seed}\npub fn compact_{seed}(x: f64) -> f64 {{\n    x * {}.0 + {seed}.0\n}}\n",
        seed + 3
    )
}

pub struct CompactionOptions {
    pub repo_root: PathBuf,
    pub work_root: PathBuf,
    pub edits: usize,
}

pub fn run(options: &CompactionOptions) -> Result<Value, String> {
    let fixture = load_fixture(&options.repo_root)?;
    let (toolchain, toolchain_evidence) = toolchain_json(&options.repo_root)?;
    let rustc = crate::harness::rustc_path();
    let work_root = options.work_root.join("compaction-suite");
    let _ = std::fs::remove_dir_all(&work_root);
    std::fs::create_dir_all(&work_root)
        .map_err(|error| format!("cannot create compaction work root: {error}"))?;
    let load_start = load_json(&[], &rustc);
    let generated_unix = now_unix();
    let daemon = crate::harness::open_daemon(&work_root, 1)?;
    let daemon_work = work_root.join("daemon");
    let states = distinct_states(&fixture, options.edits)?;
    if states.len() < options.edits {
        return Err(format!(
            "fixture produced {} distinct states for {} requested",
            states.len(),
            options.edits
        ));
    }
    let mut revision = 0u64;
    let mut edit_ms = Vec::new();
    let mut last_analysis_ms = 0u64;
    for (index, state) in states.iter().enumerate() {
        revision += 1;
        let request = AnalysisRequest {
            revision,
            label: format!("compact-edit-{index}"),
            root: fixture.root.clone(),
            files: state.clone(),
            context: fixture.context.clone(),
            priority: Priority::Normal,
        };
        let report = daemon
            .analyze(&request)
            .map_err(|error| format!("edit {index} failed: {error}"))?;
        if !report.is_current() || !report.success || report.cache.hit {
            return Err(format!("edit {index} was not a fresh successful miss"));
        }
        edit_ms.push(report.timings.analysis_ms);
        last_analysis_ms = report.timings.analysis_ms;
    }
    let context_id = daemon
        .analyze(&AnalysisRequest {
            revision: revision + 1,
            label: "warm-context".to_owned(),
            root: fixture.root.clone(),
            files: states[states.len() - 1].clone(),
            context: fixture.context.clone(),
            priority: Priority::Normal,
        })
        .map_err(|error| format!("warm analysis failed: {error}"))?
        .context_id;
    revision += 1;
    let short = &context_id[..12];
    let incremental_context = daemon_work.join("incremental").join(short);
    let sessions_before = count_sessions(&incremental_context);
    let disk_before = dir_size(&daemon_work);

    // Baseline: one new content analysed *before* compaction (full warm state).
    revision += 1;
    let baseline_content = new_content(9001);
    let baseline_files = vec![SourceFile {
        path: "src/lib.rs".to_owned(),
        content: baseline_content.clone(),
    }];
    let baseline_started = Instant::now();
    let baseline_report = daemon
        .analyze(&AnalysisRequest {
            revision,
            label: "baseline-before-compaction".to_owned(),
            root: fixture.root.clone(),
            files: baseline_files.clone(),
            context: fixture.context.clone(),
            priority: Priority::Normal,
        })
        .map_err(|error| format!("baseline analysis failed: {error}"))?;
    let baseline_wall_ms = baseline_started.elapsed().as_millis() as u64;
    if !baseline_report.success {
        return Err("baseline analysis failed".to_owned());
    }

    // Compaction GC (no byte caps, only session compaction).
    let policy = GcPolicy {
        compact_incremental: true,
        keep_incremental_sessions: 1,
        keep_recent: 4,
        ..Default::default()
    };
    let gc_started = Instant::now();
    let gc_report = daemon
        .gc(&policy)
        .map_err(|error| format!("compacting GC failed: {error}"))?;
    let gc_ms = gc_started.elapsed().as_millis() as u64;
    let disk_after = dir_size(&daemon_work);
    let sessions_after = count_sessions(&incremental_context);

    // A brand-new content after compaction: is the retained session reused?
    revision += 1;
    let post_content = new_content(9002);
    let post_files = vec![SourceFile {
        path: "src/lib.rs".to_owned(),
        content: post_content.clone(),
    }];
    let post_started = Instant::now();
    let post_report = daemon
        .analyze(&AnalysisRequest {
            revision,
            label: "after-compaction".to_owned(),
            root: fixture.root.clone(),
            files: post_files.clone(),
            context: fixture.context.clone(),
            priority: Priority::Normal,
        })
        .map_err(|error| format!("post-compaction analysis failed: {error}"))?;
    let post_wall_ms = post_started.elapsed().as_millis() as u64;
    // Cache intact: the pre-compaction baseline content is still served.
    revision += 1;
    let cache_hit_report = daemon
        .analyze(&AnalysisRequest {
            revision,
            label: "cache-intact".to_owned(),
            root: fixture.root.clone(),
            files: baseline_files,
            context: fixture.context.clone(),
            priority: Priority::Normal,
        })
        .map_err(|error| format!("cache-intact analysis failed: {error}"))?;
    let cache_intact = cache_hit_report.cache.hit;
    drop(daemon);

    // Cold reference: same content, fresh daemon, no incremental state.
    let cold_work = work_root.join("cold-daemon");
    let cold_daemon = crate::harness::open_daemon(&cold_work, 1)?;
    let cold_started = Instant::now();
    let cold_report = cold_daemon
        .analyze(&AnalysisRequest {
            revision: 1,
            label: "cold-reference".to_owned(),
            root: fixture.root.clone(),
            files: post_files,
            context: fixture.context.clone(),
            priority: Priority::Normal,
        })
        .map_err(|error| format!("cold analysis failed: {error}"))?;
    let cold_wall_ms = cold_started.elapsed().as_millis() as u64;
    drop(cold_daemon);

    let sessions_before_count = sessions_before.len();
    let sessions_after_count = sessions_after.len();
    let gc_compaction = &gc_report.compaction;
    let reuse_retained = post_report.success
        && post_report.timings.analysis_ms > 0
        && post_report.timings.analysis_ms < cold_report.timings.analysis_ms;
    let valid = gc_compaction.enabled
        && gc_compaction.sessions_deleted >= 1
        && sessions_after_count == 1
        && gc_report.freed_bytes > 0
        && disk_after < disk_before
        && post_report.is_current()
        && post_report.success
        && cache_intact
        && cold_report.success;
    let load_end = load_json(&[], &rustc);
    let _ = last_analysis_ms;
    Ok(json!({
        "schema_version": 1,
        "kind": "mvp0b-daemon-compaction-2026-11-08",
        "generated_unix": generated_unix,
        "machine": machine_json(),
        "toolchain": {
            "release": toolchain.release,
            "host": toolchain.host,
            "commit_hash": toolchain.commit_hash,
        },
        "toolchain_evidence": toolchain_evidence,
        "git": git_json(&options.repo_root),
        "command": "cargo run -q -p ravum-daemon --bin ravum-daemon-bench -- --suite compaction",
        "load_start": load_start,
        "load_end": load_end,
        "policy": {
            "compact_incremental": true,
            "keep_incremental_sessions": 1,
            "keep_recent": 4,
        },
        "edits": edit_ms.len(),
        "edit_analysis_ms": edit_ms,
        "last_edit_analysis_ms": last_analysis_ms,
        "context_id": context_id,
        "before": {
            "disk_bytes": disk_before,
            "sessions": sessions_before_count,
            "session_paths": sessions_before.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
        },
        "gc": {
            "duration_ms": gc_ms,
            "freed_bytes": gc_report.freed_bytes,
            "deleted": {
                "cache_keys": gc_report.deleted.cache_keys.len(),
                "snapshots": gc_report.deleted.snapshots.len(),
                "contexts": gc_report.deleted.contexts.len(),
            },
            "compaction": gc_compaction,
            "errors": gc_report.errors,
            "notes": gc_report.notes,
        },
        "after": {
            "disk_bytes": disk_after,
            "disk_freed_bytes": disk_before.saturating_sub(disk_after),
            "sessions": sessions_after_count,
            "session_paths": sessions_after.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
        },
        "verification": {
            "baseline_before_compaction": {
                "analysis_ms": baseline_report.timings.analysis_ms,
                "wall_ms": baseline_wall_ms,
            },
            "after_compaction": {
                "analysis_ms": post_report.timings.analysis_ms,
                "wall_ms": post_wall_ms,
                "success": post_report.success,
                "cache_kind": post_report.cache.kind.as_str(),
            },
            "cold_reference_no_state": {
                "analysis_ms": cold_report.timings.analysis_ms,
                "wall_ms": cold_wall_ms,
                "cache_kind": cold_report.cache.kind.as_str(),
            },
            "reuse_retained": reuse_retained,
            "cache_intact_after_compaction": cache_intact,
            "note": "reuse_retained is true only when the post-compaction analysis is a real compiler run (analysis_ms > 0) and strictly faster than the cold reference; otherwise the retained session was not demonstrably reused",
        },
        "valid": valid,
        "limits": [
            "single build context, single daemon, sequential edits: this measures the compaction path, not concurrent behaviour",
            "the post-compaction latency includes both the retained incremental session and any rustc re-computation: a single sample is not a distribution",
            "rustc owns the internal session format; compaction only removes whole old `s-*` directories and orphan `*.tmp` files, never rewrites rustc state",
            "the shared machine was loaded by other agents (see load_start/load_end): the baseline vs cold comparison was taken in the same run",
        ],
    }))
}

pub fn write(path: &Path, value: &Value) -> Result<(), String> {
    write_evidence(path, value)
}

/// Keeps the type import honest for readers: `run_daemon` is not used here because
/// the campaign calls `Daemon::analyze` directly to read compiler timings.
#[allow(dead_code)]
fn _unused(_fixture: &Fixture) {}
