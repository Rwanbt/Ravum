//! GC measurement suite: accumulate real state with real compilations, then run
//! the explicit retention GC and measure what it costs, what it reclaims, and
//! that live content stays served while collected content is rebuilt.
//!
//! Campaign outline:
//!
//! 1. accumulate ~40 distinct edits under one build context (one real `rustc` per
//!    edit, all cache misses), plus a few compilations under two extra build
//!    contexts so the incremental state has several context directories;
//! 2. audit the state (GC with no cap, dry-run) to record sizes before;
//! 3. GC #1 with cache+snapshot byte caps and `keep_recent = 4`;
//! 4. verify: the last served content is a cache hit without `rustc`, the first
//!    content is a miss and is rebuilt by a real compiler run;
//! 5. GC #2 with an incremental-state cap: old build contexts are collected,
//!    the last used one is protected; a content of a collected context is
//!    analysed again to prove correctness after removal;
//! 6. dry-run audit: the plan is non-empty but the filesystem is unchanged.
//!
//! Every number is measured on the reference machine; the load at start/end is
//! recorded in the evidence file.

use crate::edits::{build_states, categories, CategoryKind};
use crate::harness::{
    base_files, git_json, load_fixture, load_json, machine_json, open_daemon, rustc_path,
    toolchain_json, write_evidence, Fixture,
};
use ravum_daemon::{
    AnalysisRequest, AnalysisReport, BuildContext, Daemon, GcPolicy, GcReport, Priority,
    SourceFile,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub struct GcOptions {
    pub repo_root: PathBuf,
    pub work_root: PathBuf,
    /// Number of distinct real edits to accumulate before GC.
    pub edits: usize,
}

fn gc_call(daemon: &Daemon, policy: &GcPolicy) -> Result<GcReport, String> {
    daemon
        .gc(policy)
        .map_err(|error| format!("gc failed: {error}"))
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

fn count_entries(path: &Path) -> usize {
    std::fs::read_dir(path)
        .map(|entries| entries.flatten().count())
        .unwrap_or(0)
}

struct Accumulator {
    daemon: Daemon,
    fixture: Fixture,
    revision: u64,
    reports: Vec<Value>,
}

impl Accumulator {
    fn analyze(
        &mut self,
        label: &str,
        state: &[SourceFile],
        context: &BuildContext,
    ) -> Result<(AnalysisReport, u64), String> {
        self.revision += 1;
        let request = AnalysisRequest {
            revision: self.revision,
            label: label.to_owned(),
            root: self.fixture.root.clone(),
            files: state.to_vec(),
            context: context.clone(),
            priority: Priority::Normal,
        };
        let started = Instant::now();
        let report = self
            .daemon
            .analyze(&request)
            .map_err(|error| format!("daemon analyze failed: {error}"))?;
        let wall_ms = started.elapsed().as_millis() as u64;
        Ok((report, wall_ms))
    }
}

/// Builds a deterministic list of distinct edited states from the fixture edit
/// chains (the same real edits the L2 campaign used).
fn distinct_states(fixture: &Fixture, wanted: usize) -> Result<Vec<Vec<SourceFile>>, String> {
    let base = base_files(fixture);
    let mut states: Vec<Vec<SourceFile>> = Vec::new();
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for category in categories() {
        if category.kind != CategoryKind::EditChain {
            continue;
        }
        let built = build_states(&base, category.steps.len(), &category)?;
        for state in built {
            let digest = content_digest(&state);
            if seen.insert(digest) {
                states.push(state);
                if states.len() >= wanted {
                    return Ok(states);
                }
            }
        }
    }
    Ok(states)
}

fn content_digest(files: &[SourceFile]) -> String {
    let mut pairs: Vec<(&str, &str)> = files
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
    ravum_daemon::hash::sha256_hex(text.as_bytes())
}

fn context_with_crate_name(base: &BuildContext, crate_name: &str) -> BuildContext {
    let mut context = base.clone();
    context.crate_name = crate_name.to_owned();
    context
}

pub fn run(options: &GcOptions) -> Result<Value, String> {
    if options.edits < 3 {
        return Err("--gc-edits must be >= 3".to_owned());
    }
    let fixture = load_fixture(&options.repo_root)?;
    let (toolchain, toolchain_evidence) = toolchain_json(&options.repo_root)?;
    let rustc = rustc_path();
    std::fs::create_dir_all(&options.work_root)
        .map_err(|error| format!("cannot create work root: {error}"))?;
    let daemon = open_daemon(&options.work_root, 1)?;
    let mut accumulator = Accumulator {
        daemon,
        fixture: fixture.clone(),
        revision: 0,
        reports: Vec::new(),
    };
    let load_start = load_json(&[], &rustc);
    let started = Instant::now();

    // --- 1. Accumulate real state under the main context. ---
    let states = distinct_states(&fixture, options.edits)?;
    if states.len() < options.edits {
        return Err(format!(
            "fixture produced only {} distinct states for {} requested",
            states.len(),
            options.edits
        ));
    }
    let mut accumulation_ms = 0u64;
    let mut accumulation_rustc_ms = 0u64;
    let mut misses = 0usize;
    for (index, state) in states.iter().enumerate() {
        let (report, wall_ms) =
            accumulator.analyze(&format!("gc-edit-{index}"), state, &fixture.context)?;
        if !report.is_current() || !report.success {
            return Err(format!(
                "edit {index} did not produce a fresh successful analysis: status={:?}",
                report.status
            ));
        }
        if report.cache.hit {
            return Err(format!(
                "edit {index} unexpectedly hit the cache; states are not distinct"
            ));
        }
        misses += 1;
        accumulation_ms += wall_ms;
        accumulation_rustc_ms += report.timings.analysis_ms;
        accumulator.reports.push(json!({
            "index": index,
            "wall_ms": wall_ms,
            "analysis_ms": report.timings.analysis_ms,
            "snapshot_id": report.snapshot_id,
            "context_id": report.context_id,
        }));
    }

    // --- 2. Extra build contexts: gives the incremental category several dirs. ---
    let mut extra_contexts = Vec::new();
    for suffix in ["b", "c"] {
        let crate_name = format!("acme_unit_{suffix}");
        let context = context_with_crate_name(&fixture.context, &crate_name);
        let mut compiles = 0usize;
        for index in 0..5 {
            // Samples spread over the accumulated history.
            let sample = (index * states.len() / 5).min(states.len() - 1);
            let state = &states[sample];
            let (report, _wall_ms) =
                accumulator.analyze(&format!("gc-{suffix}-{index}"), state, &context)?;
            if !report.success {
                return Err(format!(
                    "extra context {crate_name} failed to compile state {sample}"
                ));
            }
            compiles += 1;
        }
        extra_contexts.push(json!({
            "crate_name": crate_name,
            "compiles": compiles,
        }));
    }

    // --- 2b. Pin the most recent main-context content as "served now".
    //     This is what an editor does when it returns to the current buffer: the
    //     verdict already exists (cache hit), and the retention hints mark it as
    //     the live content GC must protect. ---
    let pin_state = states[states.len() - 1].clone();
    let pin_started = Instant::now();
    let (pin, pin_wall_ms) =
        accumulator.analyze("gc-pin-recent", &pin_state, &fixture.context)?;
    let pin_wall_ms = pin_wall_ms.max(pin_started.elapsed().as_millis() as u64);
    if !pin.cache.hit {
        return Err("pinning the recent content was expected to be a cache hit".to_owned());
    }

    // --- 3. Audit: inventory + sizes before GC (no cap, dry-run = no deletion). ---
    let audit_policy = GcPolicy {
        keep_recent: 4,
        dry_run: true,
        ..Default::default()
    };
    let audit_started = Instant::now();
    let audit = gc_call(&accumulator.daemon, &audit_policy)?;
    let audit_ms = audit_started.elapsed().as_millis() as u64;
    let work_dir = accumulator.daemon.work_dir().to_path_buf();
    let before_disk_bytes = dir_size(&work_dir);
    let stats_before = accumulator.daemon.stats();

    // --- 4. GC #1: cache + snapshot caps. ---
    let gc1_policy = GcPolicy {
        max_cache_bytes: Some(16 * 1024),
        max_snapshot_bytes: Some(128 * 1024),
        keep_recent: 4,
        ..Default::default()
    };
    let gc1_started = Instant::now();
    let gc1 = gc_call(&accumulator.daemon, &gc1_policy)?;
    let gc1_ms = gc1_started.elapsed().as_millis() as u64;
    let after_gc1_disk_bytes = dir_size(&work_dir);

    // Verify: last served content is a hit without rustc, oldest is rebuilt.
    let last_state = states[states.len() - 1].clone();
    let first_state = states[0].clone();
    let runs_before_hits = accumulator.daemon.stats().rustc_runs;
    let (recent, recent_wall_ms) =
        accumulator.analyze("gc-verify-recent", &last_state, &fixture.context)?;
    let (rebuilt, rebuilt_wall_ms) =
        accumulator.analyze("gc-verify-rebuild", &first_state, &fixture.context)?;
    let runs_after_hits = accumulator.daemon.stats().rustc_runs;
    let recent_hit = recent.cache.hit;
    let rebuilt_miss = !rebuilt.cache.hit;
    let rebuilt_rustc_ms = rebuilt.timings.analysis_ms;
    let gc1_collected_entries = gc1.deleted.cache_keys.len()
        + gc1.deleted.snapshots.len()
        + gc1.deleted.contexts.len();
    // The oldest content is only expected to be a miss if GC actually collected
    // something; with undersized accumulated state the caps are not exceeded and
    // the measurement stays valid instead of failing artificially.
    let expected_old_miss = !gc1.deleted.cache_keys.is_empty();
    let expected_runs_after = runs_before_hits + u64::from(rebuilt_miss);
    let no_compiler_for_recent = recent_hit && runs_after_hits == expected_runs_after;

    // --- 5. GC #2: incremental state cap (old build contexts collected). ---
    let gc2_policy = GcPolicy {
        max_incremental_bytes: Some(1), // force collection of every unprotected context
        keep_recent: 4,
        ..Default::default()
    };
    let gc2_started = Instant::now();
    let gc2 = gc_call(&accumulator.daemon, &gc2_policy)?;
    let gc2_ms = gc2_started.elapsed().as_millis() as u64;
    let after_gc2_disk_bytes = dir_size(&work_dir);

    // A content of a collected context must be correctly rebuilt. The content is
    // chosen so that this (content, context) pair has no cached verdict: the
    // proof is a real compiler run in a fresh incremental directory, not a cache
    // hit that would hide a broken state.
    let extra_c = context_with_crate_name(&fixture.context, "acme_unit_c");
    let sampled_in_extra: std::collections::BTreeSet<usize> =
        (0..5).map(|index| index * states.len() / 5).collect();
    let extra_state_index = (0..states.len())
        .rev()
        .find(|index| !sampled_in_extra.contains(index))
        .ok_or_else(|| "no unsampled state for the extra-context verification".to_owned())?;
    let (rebuilt_extra, rebuilt_extra_wall_ms) = accumulator.analyze(
        "gc-verify-extra-context",
        &states[extra_state_index],
        &extra_c,
    )?;
    let rebuilt_extra_miss = rebuilt_extra.cache.kind.as_str() == "miss";
    let rebuilt_extra_ok = rebuilt_extra.is_current()
        && rebuilt_extra.success
        && rebuilt_extra_miss
        && rebuilt_extra.timings.analysis_ms > 0;
    let rebuilt_extra_context_short = rebuilt_extra
        .context_id
        .get(..12)
        .unwrap_or_default()
        .to_owned();
    let context_recreated = accumulator
        .daemon
        .work_dir()
        .join("incremental")
        .join(&rebuilt_extra_context_short)
        .exists();

    // --- 6. Dry-run audit: plan non-empty, disk unchanged. ---
    let dry_policy = GcPolicy {
        max_cache_bytes: Some(1),
        max_snapshot_bytes: Some(1),
        max_incremental_bytes: Some(1),
        keep_recent: 4,
        dry_run: true,
        ..Default::default()
    };
    // Disk size is sampled immediately before the dry run: the verification
    // compiler above legitimately wrote new state in between.
    let before_dry_disk_bytes = dir_size(&work_dir);
    let dry_started = Instant::now();
    let dry = gc_call(&accumulator.daemon, &dry_policy)?;
    let dry_ms = dry_started.elapsed().as_millis() as u64;
    let after_dry_disk_bytes = dir_size(&work_dir);

    let final_stats = accumulator.daemon.stats();
    let total_wall_ms = started.elapsed().as_millis() as u64;
    let load_end = load_json(&[], &rustc);

    let to_report = |report: &GcReport| -> Value {
        json!({
            "mode": report.mode.as_str(),
            "dry_run": report.dry_run,
            "duration_ms": report.duration_ms,
            "before": report.before,
            "after": report.after,
            "planned_freed_bytes": report.planned_freed_bytes,
            "freed_bytes": report.freed_bytes,
            "deleted": {
                "cache_keys": report.deleted.cache_keys.len(),
                "snapshots": report.deleted.snapshots.len(),
                "contexts": report.deleted.contexts.len(),
                "files": report.deleted.files,
                "dirs": report.deleted.dirs,
            },
            "planned": {
                "cache_keys": report.planned.cache_keys.len(),
                "snapshots": report.planned.snapshots.len(),
                "contexts": report.planned.contexts.len(),
            },
            "kept": {
                "cache_keys": report.kept.cache_keys.len(),
                "snapshots": report.kept.snapshots.len(),
                "contexts": report.kept.contexts.clone(),
                "recent": report.kept.recent,
                "reasons": report.kept.reasons,
            },
            "budgets": report.budgets,
            "protected": report.protected,
            "errors": report.errors,
            "notes": report.notes,
        })
    };

    Ok(json!({
        "schema_version": 1,
        "kind": "mvp0b-daemon-gc-2026-11-08",
        "generated_unix": now_unix(),
        "machine": machine_json(),
        "toolchain": {
            "release": toolchain.release,
            "host": toolchain.host,
            "commit_hash": toolchain.commit_hash,
        },
        "toolchain_evidence": toolchain_evidence,
        "git": git_json(&options.repo_root),
        "command": "cargo run -q -p ravum-daemon --bin ravum-daemon-bench -- --suite gc",
        "fixture": {
            "dir": fixture.dir.display().to_string(),
            "root": fixture.root,
            "files": fixture.files.iter().map(|file| json!({
                "path": file.path,
                "bytes": file.content.len(),
                "lines": file.content.lines().count(),
            })).collect::<Vec<_>>(),
            "context": {
                "edition": fixture.context.edition,
                "crate_name": fixture.context.crate_name,
                "crate_type": fixture.context.crate_type,
            },
        },
        "load_start": load_start,
        "load_end": load_end,
        "accumulation": {
            "distinct_edits_analysed": misses,
            "extra_contexts": extra_contexts,
            "rustc_runs": stats_before.rustc_runs,
            "accumulation_wall_ms": accumulation_ms,
            "accumulation_rustc_ms": accumulation_rustc_ms,
            "per_edit": accumulator.reports,
            "stats_before_gc": stats_before,
            "total_wall_ms": total_wall_ms,
        },
        "audit_before": {
            "policy": audit_policy,
            "duration_ms": audit_ms,
            "sizes": audit.before,
            "entries": {
                "cache": count_entries(&work_dir.join("cache")),
                "snapshots": count_entries(&work_dir.join("snapshots")),
                "incremental_contexts": count_entries(&work_dir.join("incremental")),
            },
            "work_dir_bytes": before_disk_bytes,
            "report": to_report(&audit),
        },
        "pin_recent": {
            "expected_cache_hit": true,
            "cache_kind": pin.cache.kind.as_str(),
            "wall_ms": pin_wall_ms,
        },
        "gc1_cache_snapshots": {
            "policy": gc1_policy,
            "duration_ms": gc1_ms,
            "report": to_report(&gc1),
            "disk_bytes_before": before_disk_bytes,
            "disk_bytes_after": after_gc1_disk_bytes,
            "disk_bytes_freed": before_disk_bytes.saturating_sub(after_gc1_disk_bytes),
            "verification": {
                "recent_cache_hit": recent_hit,
                "recent_wall_ms": recent_wall_ms,
                "recent_analysis_ms": recent.timings.analysis_ms,
                "old_cache_miss": rebuilt_miss,
                "old_cache_miss_expected": expected_old_miss,
                "gc1_collected_entries": gc1_collected_entries,
                "gc1_collected_cache_entries": gc1.deleted.cache_keys.len(),
                "old_wall_ms": rebuilt_wall_ms,
                "old_analysis_ms": rebuilt_rustc_ms,
                "rustc_runs_before": runs_before_hits,
                "rustc_runs_after": runs_after_hits,
                "no_compiler_for_recent": no_compiler_for_recent,
            },
        },
        "gc2_incremental": {
            "policy": gc2_policy,
            "duration_ms": gc2_ms,
            "report": to_report(&gc2),
            "disk_bytes_before": after_gc1_disk_bytes,
            "disk_bytes_after": after_gc2_disk_bytes,
            "disk_bytes_freed": after_gc1_disk_bytes.saturating_sub(after_gc2_disk_bytes),
            "verification": {
                "collected_contexts": gc2.deleted.contexts,
                "kept_contexts": gc2.kept.contexts,
                "last_context_protected": gc2.protected.last_context_short,
                "new_content_in_collected_context_compiled": rebuilt_extra_ok,
                "new_content_cache_miss": rebuilt_extra_miss,
                "new_content_analysis_ms": rebuilt_extra.timings.analysis_ms,
                "new_content_wall_ms": rebuilt_extra_wall_ms,
                "new_content_state_index": extra_state_index,
                "rebuilt_context_id": rebuilt_extra.context_id,
                "incremental_context_recreated": context_recreated,
            },
        },
        "dry_run_audit": {
            "policy": dry_policy,
            "duration_ms": dry_ms,
            "plan_non_empty": dry.planned.cache_keys.len()
                + dry.planned.snapshots.len()
                + dry.planned.contexts.len() > 0,
            "freed_bytes": dry.freed_bytes,
            "planned_freed_bytes": dry.planned_freed_bytes,
            "disk_bytes_before": before_dry_disk_bytes,
            "disk_bytes_after": after_dry_disk_bytes,
            "filesystem_unchanged": before_dry_disk_bytes == after_dry_disk_bytes,
        },
        "after": {
            "sizes": dry.before,
            "work_dir_bytes": after_dry_disk_bytes,
            "daemon_stats": final_stats,
            "cache_entries_memory": accumulator.daemon.cache_entries().0,
            "cache_entries_disk": accumulator.daemon.cache_entries().1,
            "incremental_context_dirs": count_entries(&work_dir.join("incremental")),
            "out_context_dirs": count_entries(&work_dir.join("out")),
            "unit_context_dirs": count_entries(&work_dir.join("units")),
        },
        "limits": [
            "OBSERVED numbers on the shared reference machine (13th Gen i7-13620H, 16 Go, ~1.8-3.7 Go free, other agents running): not portable guarantees; see load_start/load_end.",
            "GC is explicit only: no background/automatic collection is measured or implemented; a long-running daemon must call it itself (server command or one-shot).",
            "the incremental state is collected **only per whole build context**: rustc's own incremental internals are never compacted (explicitly out of scope). With a single active context and a protected last context, the incremental bytes cannot be reduced: the budget is reported unmet instead.",
            "one-shot GC assumes no live daemon is writing the same work directory (multi-process concurrent GC is not tested).",
            "recency of one-shot GC comes from the persisted retention hints (retention.json) plus file timestamps; a cold start without hints orders contents by timestamp/key, which is coarse (second granularity).",
            "no soak test of several hours: growth over tens of real edits only.",
            "the fixture is a small real library unit (~230 lines); GC cost is dominated by directory scans and file metadata, not by unit size.",
        ],
    }))
}

pub fn write(path: &Path, value: &Value) -> Result<(), String> {
    write_evidence(path, value)
}
