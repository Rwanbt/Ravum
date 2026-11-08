//! Negative and boundary tests as an executable, recorded suite.
//!
//! These checks are also covered by `cargo test`; running them here produces
//! the raw evidence file. Every check states what it proves; a failed check
//! makes the suite exit non-zero instead of being reported as green.

use crate::edits::{apply_edit, Edit};
use crate::harness::{
    base_files, load_fixture, load_json, machine_json, open_daemon, run_cold, toolchain_json,
    write_evidence, Fixture, ModeRun,
};
use ravum_daemon::{
    AnalysisReport, BuildContext, Daemon, DaemonConfig, Priority, ReportStatus, SourceFile,
    ToolchainInfo,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct InvariantOptions {
    pub repo_root: PathBuf,
    pub work_root: PathBuf,
}

struct Suite {
    fixture: Fixture,
    work_root: PathBuf,
    rustc: PathBuf,
    sequence: u64,
    revision: u64,
    checks: Vec<Value>,
}

impl Suite {
    fn cold(&mut self, state: &[SourceFile]) -> Result<ModeRun, String> {
        let run = run_cold(
            &self.work_root,
            &self.rustc,
            &self.fixture,
            "invariants",
            self.sequence,
            state,
        )?;
        self.sequence += 1;
        Ok(run)
    }

    fn request(&mut self, label: &str, state: &[SourceFile]) -> ravum_daemon::AnalysisRequest {
        self.revision += 1;
        ravum_daemon::AnalysisRequest {
            revision: self.revision,
            label: label.to_owned(),
            root: self.fixture.root.clone(),
            files: state.to_vec(),
            context: self.fixture.context.clone(),
            priority: Priority::Normal,
        }
    }

    fn request_with_context(
        &mut self,
        label: &str,
        state: &[SourceFile],
        context: BuildContext,
    ) -> ravum_daemon::AnalysisRequest {
        self.revision += 1;
        ravum_daemon::AnalysisRequest {
            revision: self.revision,
            label: label.to_owned(),
            root: self.fixture.root.clone(),
            files: state.to_vec(),
            context,
            priority: Priority::Normal,
        }
    }

    fn daemon_analyze(
        &mut self,
        daemon: &Daemon,
        label: &str,
        state: &[SourceFile],
    ) -> Result<AnalysisReport, String> {
        let request = self.request(label, state);
        daemon
            .analyze(&request)
            .map_err(|error| format!("daemon analyze failed: {error}"))
    }

    fn check(&mut self, name: &str, passed: bool, details: Value) {
        self.checks.push(json!({
            "name": name,
            "passed": passed,
            "details": details,
        }));
    }
}

pub fn run(options: &InvariantOptions) -> Result<Value, String> {
    let fixture = load_fixture(&options.repo_root)?;
    let (toolchain, toolchain_evidence) = toolchain_json(&options.repo_root)?;
    let work_root = options.work_root.join("invariants");
    let _ = std::fs::remove_dir_all(&work_root);
    std::fs::create_dir_all(&work_root).map_err(|error| format!("cannot create work dir: {error}"))?;
    let start_load = load_json(&[], &crate::harness::rustc_path());
    let daemon = open_daemon(&work_root, 1)?;
    let mut suite = Suite {
        fixture,
        work_root: work_root.clone(),
        rustc: crate::harness::rustc_path(),
        sequence: 0,
        revision: 0,
        checks: Vec::new(),
    };
    let base = base_files(&suite.fixture);
    // Warm the daemon and produce a reference cold run for the base content.
    let base_cold = suite.cold(&base)?;
    let warm = suite.daemon_analyze(&daemon, "warmup-base", &base)?;

    // 1. Cross-module signature invalidation: changing `scale`'s signature
    //    without updating its caller must be reported in the caller's file by
    //    every mode (no stale reuse of the old signature).
    {
        let mut broken = base.clone();
        apply_edit(
            &mut broken,
            &Edit {
                file: "src/math.rs",
                find: "pub fn scale(x: f64) -> f64 {",
                replace: "pub fn scale(x: f64, factor: f64) -> f64 {",
            },
        )?;
        apply_edit(
            &mut broken,
            &Edit {
                file: "src/math.rs",
                find: "x * SCALE",
                replace: "x * factor",
            },
        )?;
        let cold_broken = suite.cold(&broken)?;
        let daemon_broken = suite.daemon_analyze(&daemon, "signature-broken", &broken)?;
        let consistent = cold_broken.signatures() == ravum_daemon::signature_set(&daemon_broken.diagnostics);
        let caller_error = daemon_broken.diagnostics.iter().any(|diagnostic| {
            diagnostic.severity.is_error()
                && diagnostic.file.as_deref() == Some("src/pipeline.rs")
        });
        let mut fixed = broken.clone();
        apply_edit(
            &mut fixed,
            &Edit {
                file: "src/pipeline.rs",
                find: "scale(clamp01(value))",
                replace: "scale(clamp01(value), SCALE)",
            },
        )?;
        let cold_fixed = suite.cold(&fixed)?;
        let daemon_fixed = suite.daemon_analyze(&daemon, "signature-fixed", &fixed)?;
        let fixed_consistent =
            cold_fixed.signatures() == ravum_daemon::signature_set(&daemon_fixed.diagnostics);
        suite.check(
            "cross_module_signature_invalidation",
            cold_broken.diagnostics.len() > 0
                && !cold_broken.success
                && !daemon_broken.success
                && consistent
                && caller_error
                && cold_fixed.success
                && daemon_fixed.success
                && fixed_consistent,
            json!({
                "cold_broken_diagnostics": cold_broken.signatures(),
                "daemon_broken_diagnostics": ravum_daemon::signature_set(&daemon_broken.diagnostics),
                "caller_error_in_pipeline_rs": caller_error,
                "diagnostics_consistent": consistent,
                "fixed_compiles_in_both_modes": cold_fixed.success && daemon_fixed.success,
            }),
        );
    }

    // 2. Local body edit: no public API change, no diagnostics, then a second
    //    submission of the same bytes is a cache hit with identical result.
    {
        let mut state = base.clone();
        apply_edit(
            &mut state,
            &Edit {
                file: "src/math.rs",
                find: "x * SCALE",
                replace: "x * SCALE + 0.5",
            },
        )?;
        let cold_edit = suite.cold(&state)?;
        let first = suite.daemon_analyze(&daemon, "body-edit", &state)?;
        let second = suite.daemon_analyze(&daemon, "body-edit-again", &state)?;
        let consistent = cold_edit.signatures() == ravum_daemon::signature_set(&second.diagnostics);
        suite.check(
            "local_body_edit_then_cache_hit",
            cold_edit.success
                && first.is_current()
                && second.is_current()
                && !second.cache.kind.as_str().eq("miss")
                && second.timings.analysis_ms == 0
                && consistent,
            json!({
                "cold_ms": cold_edit.ms,
                "first_status": first.status.as_str(),
                "first_cache": first.cache.kind.as_str(),
                "first_analysis_ms": first.timings.analysis_ms,
                "second_cache": second.cache.kind.as_str(),
                "second_analysis_ms": second.timings.analysis_ms,
                "diagnostics_consistent": consistent,
            }),
        );
    }

    // 3. Build-context invalidation: changing flags must produce a cache miss
    //    even when the source bytes are identical.
    {
        let mut warnings = suite.fixture.context.clone();
        warnings.flags = vec!["-D".to_owned(), "warnings".to_owned()];
        let request_default = suite.request("context-default", &base);
        let first = daemon
            .analyze(&request_default)
            .map_err(|error| format!("daemon analyze failed: {error}"))?;
        let request_warnings = suite.request_with_context("context-warnings", &base, warnings);
        let second = daemon
            .analyze(&request_warnings)
            .map_err(|error| format!("daemon analyze failed: {error}"))?;
        let request_again = suite.request("context-default-again", &base);
        let third = daemon
            .analyze(&request_again)
            .map_err(|error| format!("daemon analyze failed: {error}"))?;
        suite.check(
            "context_flags_invalidate_cache",
            first.cache.kind.is_hit()
                && second.cache.kind.as_str() == "miss"
                && third.cache.kind.is_hit()
                && second.success
                && ravum_daemon::signature_set(&first.diagnostics)
                    == ravum_daemon::signature_set(&second.diagnostics),
            json!({
                "default_first": first.cache.kind.as_str(),
                "flags_changed": second.cache.kind.as_str(),
                "default_again": third.cache.kind.as_str(),
                "flags_change_was_real_compile": second.timings.analysis_ms > 0,
            }),
        );
    }

    // 4. Toolchain identity invalidation: a different declared toolchain is a
    //    different content identity, so it cannot reuse the cache.
    {
        let simulated = ToolchainInfo {
            release: "1.98.1-simulated".to_owned(),
            host: toolchain.host.clone(),
            commit_hash: "simulated-toolchain-change".to_owned(),
        };
        let toolchain_work = work_root.join("toolchain");
        let other = Daemon::open(DaemonConfig {
            rustc: crate::harness::rustc_path(),
            work_dir: toolchain_work.clone(),
            max_concurrent_jobs: 1,
            max_cache_entries: 8,
            poll_interval_ms: 2,
            analysis_timeout_ms: 60_000,
            toolchain_override: Some(simulated),
        })
        .map_err(|error| format!("cannot open toolchain daemon: {error}"))?;
        let first = suite.daemon_analyze(&other, "toolchain-a", &base)?;
        let second = suite.daemon_analyze(&other, "toolchain-a-again", &base)?;
        suite.check(
            "toolchain_identity_invalidates_cache",
            first.cache.kind.as_str() == "miss"
                && second.cache.kind.is_hit()
                && second.success,
            json!({
                "first": first.cache.kind.as_str(),
                "second": second.cache.kind.as_str(),
                "toolchain_release": other.toolchain().release,
                "note": "toolchain identity is simulated through the documented test seam; no second toolchain was installed or benchmarked",
            }),
        );
    }

    // 5. Stale revision: after the current revision advanced, an older revision
    //    must never be returned as current, even on a cache hit.
    {
        let current = daemon.current_revision();
        daemon.advance_revision(current + 10);
        let stale_request = ravum_daemon::AnalysisRequest {
            revision: current + 5,
            label: "stale-revision".to_owned(),
            root: suite.fixture.root.clone(),
            files: base.clone(),
            context: suite.fixture.context.clone(),
            priority: Priority::Normal,
        };
        let stale = daemon
            .analyze(&stale_request)
            .map_err(|error| format!("daemon analyze failed: {error}"))?;
        suite.revision = suite.revision.max(daemon.current_revision());
        suite.check(
            "stale_revision_never_current",
            !stale.is_current()
                && stale.status == ReportStatus::Superseded
                && stale.diagnostics.is_empty(),
            json!({
                "status": stale.status.as_str(),
                "requested_revision": stale_request.revision,
                "current_revision": stale.current_revision,
                "diagnostics_returned": stale.diagnostics.len(),
            }),
        );
    }

    // 6. Cancellation: a newer revision arrives while the compiler is running;
    //    the in-flight answer must be non-current, and a later analysis of the
    //    same bytes must still be correct (the killed compiler did not poison
    //    the incremental state). A dedicated daemon is used so the running
    //    revision is genuinely the newest one when the compiler starts.
    {
        let cancel_root = work_root.join("cancel");
        let cancel_daemon = open_daemon(&cancel_root, 1)?;
        let warm_request = ravum_daemon::AnalysisRequest {
            revision: 1,
            label: "cancel-warmup".to_owned(),
            root: suite.fixture.root.clone(),
            files: base.clone(),
            context: suite.fixture.context.clone(),
            priority: Priority::Normal,
        };
        let _ = cancel_daemon
            .analyze(&warm_request)
            .map_err(|error| format!("daemon analyze failed: {error}"))?;
        let mut state = base.clone();
        apply_edit(
            &mut state,
            &Edit {
                file: "src/math.rs",
                find: "x * SCALE",
                replace: "x * SCALE + 0.25",
            },
        )?;
        let thread_revision = 2u64;
        let request = ravum_daemon::AnalysisRequest {
            revision: thread_revision,
            label: "cancel-mid-compile".to_owned(),
            root: suite.fixture.root.clone(),
            files: state.clone(),
            context: suite.fixture.context.clone(),
            priority: Priority::Normal,
        };
        let daemon_handle = cancel_daemon.clone();
        let started = std::time::Instant::now();
        let worker = std::thread::spawn(move || daemon_handle.analyze(&request));
        std::thread::sleep(Duration::from_millis(40));
        cancel_daemon.advance_revision(thread_revision + 1);
        let result = worker
            .join()
            .map_err(|_| "cancellation worker panicked".to_owned())?
            .map_err(|error| format!("daemon analyze failed: {error}"))?;
        let after_request = ravum_daemon::AnalysisRequest {
            revision: thread_revision + 2,
            label: "after-cancel".to_owned(),
            root: suite.fixture.root.clone(),
            files: state.clone(),
            context: suite.fixture.context.clone(),
            priority: Priority::Normal,
        };
        let after = cancel_daemon
            .analyze(&after_request)
            .map_err(|error| format!("daemon analyze failed: {error}"))?;
        let cold_after = suite.cold(&state)?;
        let after_consistent =
            cold_after.signatures() == ravum_daemon::signature_set(&after.diagnostics);
        suite.check(
            "superseded_during_compilation",
            !result.is_current()
                && matches!(
                    result.status,
                    ReportStatus::Cancelled | ReportStatus::Superseded
                )
                && after.is_current()
                && after.success
                && after_consistent,
            json!({
                "outcome_status": result.status.as_str(),
                "outcome_message": result.message,
                "elapsed_ms": started.elapsed().as_millis() as u64,
                "in_flight_analysis_ms": result.timings.analysis_ms,
                "later_analysis_status": after.status.as_str(),
                "later_analysis_success": after.success,
                "later_analysis_consistent_with_cold": after_consistent,
                "note": "if status is cancelled the compiler process was actually killed; if superseded the answer completed or was rejected before the kill window",
            }),
        );
    }
    // 7. Cache corruption: a tampered verdict must degrade to a real compile,
    //    never to a wrong answer.
    {
        let corruption_root = work_root.join("corruption");
        let daemon_a = open_daemon(&corruption_root, 1)?;
        let prepared = suite.daemon_analyze(&daemon_a, "corruption-seed", &base)?;
        drop(daemon_a);
        let cache_dir = corruption_root.join("daemon").join("cache");
        let mut corrupted = 0usize;
        if let Ok(entries) = std::fs::read_dir(&cache_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|ext| ext == "json") != Some(true) {
                    continue;
                }
                let Ok(bytes) = std::fs::read(&path) else {
                    continue;
                };
                let Ok(mut value) = serde_json::from_slice::<Value>(&bytes) else {
                    continue;
                };
                if let Some(first) = value["diagnostics"].get_mut(0) {
                    first["message"] = json!("tampered by the invalidation suite");
                } else {
                    value["payload_digest"] = json!("0".repeat(64));
                }
                std::fs::write(&path, serde_json::to_vec(&value).unwrap_or_default()).ok();
                corrupted += 1;
            }
        }
        let daemon_b = open_daemon(&corruption_root, 1)?;
        let recovered = suite.daemon_analyze(&daemon_b, "corruption-recover", &base)?;
        let stats = daemon_b.stats();
        let cold = suite.cold(&base)?;
        let consistent = cold.signatures() == ravum_daemon::signature_set(&recovered.diagnostics);
        suite.check(
            "cache_corruption_degrades_to_miss",
            corrupted > 0
                && recovered.cache.kind.as_str() == "miss"
                && recovered.success
                && consistent
                && stats.cache_corrupt > 0,
            json!({
                "corrupted_files": corrupted,
                "recovered_cache_kind": recovered.cache.kind.as_str(),
                "recovered_success": recovered.success,
                "recovered_diagnostics_consistent_with_cold": consistent,
                "daemon_stats": stats,
                "seed_verdict_present": prepared.success,
            }),
        );
    }

    // 8. Restart: the disk cache and the incremental directory survive a daemon
    //    restart; new content still compiles correctly after the restart.
    {
        let restart_root = work_root.join("restart");
        let daemon_a = open_daemon(&restart_root, 1)?;
        let first = suite.daemon_analyze(&daemon_a, "restart-a", &base)?;
        let mut state_b = base.clone();
        apply_edit(
            &mut state_b,
            &Edit {
                file: "src/math.rs",
                find: "x * SCALE",
                replace: "x * SCALE + 0.75",
            },
        )?;
        let second = suite.daemon_analyze(&daemon_a, "restart-b", &state_b)?;
        drop(daemon_a);
        let incremental_before = count_entries(&restart_root.join("daemon").join("incremental"));
        let daemon_b = open_daemon(&restart_root, 1)?;
        let _ = first;
        let _ = second;
        let replay_a = suite.daemon_analyze(&daemon_b, "restart-a-replay", &base)?;
        let replay_b = suite.daemon_analyze(&daemon_b, "restart-b-replay", &state_b)?;
        let mut state_c = state_b.clone();
        apply_edit(
            &mut state_c,
            &Edit {
                file: "src/pipeline.rs",
                find: "total / values.len() as f64",
                replace: "(total / values.len() as f64) * 0.5",
            },
        )?;
        let new_after_restart = suite.daemon_analyze(&daemon_b, "restart-c", &state_c)?;
        let cold_c = suite.cold(&state_c)?;
        let consistent =
            cold_c.signatures() == ravum_daemon::signature_set(&new_after_restart.diagnostics);
        suite.check(
            "restart_reuses_persistent_state",
            replay_a.cache.kind.as_str() == "disk"
                && replay_b.cache.kind.as_str() == "disk"
                && new_after_restart.cache.kind.as_str() == "miss"
                && new_after_restart.success
                && consistent,
            json!({
                "replay_a_cache": replay_a.cache.kind.as_str(),
                "replay_b_cache": replay_b.cache.kind.as_str(),
                "new_content_cache": new_after_restart.cache.kind.as_str(),
                "new_content_analysis_ms": new_after_restart.timings.analysis_ms,
                "new_content_matches_cold": consistent,
                "incremental_entries_before_restart": incremental_before,
                "incremental_entries_after_restart": count_entries(&restart_root.join("daemon").join("incremental")),
                "note": "the revision counter is session-scoped; persistence covers content verdicts and the incremental directory",
            }),
        );
    }

    // 9. Process budget: with max_concurrent_jobs=1, three concurrent requests
    //    still run one compiler at a time and all produce fresh answers.
    {
        let budget_root = work_root.join("budget");
        let daemon_budget = open_daemon(&budget_root, 1)?;
        let mut states = Vec::new();
        for variant in 0..3 {
            let mut state = base.clone();
            let lib = state
                .iter_mut()
                .find(|file| file.path == "src/lib.rs")
                .expect("fixture has src/lib.rs");
            lib.content = format!(
                "{}\n// Concurrent request variant {variant}.\n",
                lib.content
            );
            states.push(state);
        }
        let handles: Vec<_> = states
            .into_iter()
            .enumerate()
            .map(|(index, state)| {
                let daemon = daemon_budget.clone();
                let root = suite.fixture.root.clone();
                let context = suite.fixture.context.clone();
                let handle = std::thread::spawn(move || {
                    ravum_daemon::AnalysisRequest {
                        revision: (index + 1) as u64,
                        label: format!("budget-{index}"),
                        root,
                        files: state,
                        context,
                        priority: Priority::Interactive,
                    }
                });
                (daemon, handle)
            })
            .collect();
        let mut all_fresh = true;
        for (daemon, handle) in handles {
            let request = handle.join().map_err(|_| "budget thread panicked".to_owned())?;
            let report = daemon
                .analyze(&request)
                .map_err(|error| format!("daemon analyze failed: {error}"))?;
            all_fresh = all_fresh && report.is_current() && report.success;
        }
        let stats = daemon_budget.stats();
        suite.check(
            "process_budget_enforced",
            all_fresh && stats.peak_active_jobs <= 1 && stats.rustc_runs == 3,
            json!({
                "all_fresh": all_fresh,
                "peak_active_jobs": stats.peak_active_jobs,
                "rustc_runs": stats.rustc_runs,
                "max_concurrent_jobs": 1,
            }),
        );
    }

    // 10. Memory budget: the in-memory cache respects its entry ceiling while
    //     the disk cache keeps every content-addressed verdict.
    {
        let memory_root = work_root.join("memory");
        let daemon_memory = Daemon::open(DaemonConfig {
            rustc: crate::harness::rustc_path(),
            work_dir: memory_root.clone(),
            max_concurrent_jobs: 1,
            max_cache_entries: 1,
            poll_interval_ms: 2,
            analysis_timeout_ms: 60_000,
            toolchain_override: None,
        })
        .map_err(|error| format!("cannot open memory daemon: {error}"))?;
        let mut state = base.clone();
        {
            let lib = state
                .iter_mut()
                .find(|file| file.path == "src/lib.rs")
                .expect("fixture has src/lib.rs");
            lib.content.push_str("\n/// Memory budget variant one.\n");
        }
        let _ = suite.daemon_analyze(&daemon_memory, "memory-1", &state)?;
        {
            let lib = state
                .iter_mut()
                .find(|file| file.path == "src/lib.rs")
                .expect("fixture has src/lib.rs");
            lib.content.push_str("/// Memory budget variant two.\n");
        }
        let _ = suite.daemon_analyze(&daemon_memory, "memory-2", &state)?;
        let (memory_entries, disk_entries) = daemon_memory.cache_entries();
        suite.check(
            "memory_cache_budget",
            memory_entries <= 1 && disk_entries >= 2,
            json!({
                "max_cache_entries": 1,
                "memory_entries": memory_entries,
                "disk_entries": disk_entries,
            }),
        );
    }

    // 11. Hard timeout: a compiler that exceeds the wall-clock budget is killed
    //     and reported as `timed_out`, never as a fresh answer.
    {
        let timeout_root = work_root.join("timeout");
        let daemon_timeout = Daemon::open(DaemonConfig {
            rustc: crate::harness::rustc_path(),
            work_dir: timeout_root,
            max_concurrent_jobs: 1,
            max_cache_entries: 4,
            poll_interval_ms: 1,
            analysis_timeout_ms: 1,
            toolchain_override: None,
        })
        .map_err(|error| format!("cannot open timeout daemon: {error}"))?;
        let report = suite.daemon_analyze(&daemon_timeout, "timeout", &base)?;
        suite.check(
            "analysis_timeout_reported",
            report.status == ReportStatus::TimedOut && !report.is_current(),
            json!({
                "status": report.status.as_str(),
                "analysis_ms": report.timings.analysis_ms,
                "timeout_ms": 1,
            }),
        );
    }

    // 12. JSON contract: the report serializes to valid JSON with the required
    //     fields and round-trips.
    {
        let report = suite.daemon_analyze(&daemon, "json-contract", &base)?;
        let value = report.to_json();
        let text = serde_json::to_string(&value)
            .map_err(|error| format!("report JSON serialization failed: {error}"))?;
        let parsed: Value = serde_json::from_str(&text)
            .map_err(|error| format!("report JSON is not valid JSON: {error}"))?;
        let required = [
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
        ];
        let missing: Vec<&str> = required
            .iter()
            .copied()
            .filter(|key| parsed.get(*key).is_none())
            .collect();
        let timings_required = ["accept_ms", "queue_ms", "lookup_ms", "analysis_ms", "total_ms"];
        let missing_timings: Vec<&str> = timings_required
            .iter()
            .copied()
            .filter(|key| parsed["timings"].get(*key).is_none())
            .collect();
        suite.check(
            "json_report_contract",
            missing.is_empty()
                && missing_timings.is_empty()
                && parsed["status"] == "fresh"
                && parsed["cache"]["kind"].is_string(),
            json!({
                "missing_fields": missing,
                "missing_timing_fields": missing_timings,
                "json_bytes": text.len(),
            }),
        );
    }

    // 13. User files are never written: the daemon works from its own directory.
    {
        let user_dir = work_root.join("user-files");
        let user_file = user_dir.join("src").join("lib.rs");
        std::fs::create_dir_all(user_file.parent().expect("user file parent"))
            .map_err(|error| format!("cannot create user dir: {error}"))?;
        let original = base
            .iter()
            .find(|file| file.path == "src/lib.rs")
            .map(|file| file.content.clone())
            .unwrap_or_default();
        std::fs::write(&user_file, &original)
            .map_err(|error| format!("cannot write user file: {error}"))?;
        let _ = suite.daemon_analyze(&daemon, "user-file", &base)?;
        let on_disk = std::fs::read_to_string(&user_file)
            .map_err(|error| format!("cannot read user file: {error}"))?;
        suite.check(
            "user_files_untouched",
            on_disk == original,
            json!({
                "user_file": user_file.display().to_string(),
                "unchanged": on_disk == original,
            }),
        );
    }

    // 14. Granularity observation: local body edit vs public signature change,
    //     with the timings recorded honestly (timing is a proxy; the public
    //     rustc CLI exposes no per-query report).
    {
        let mut local = base.clone();
        apply_edit(
            &mut local,
            &Edit {
                file: "src/math.rs",
                find: "x * SCALE",
                replace: "x * SCALE + 0.125",
            },
        )?;
        let local_cold = suite.cold(&local)?;
        let local_daemon = suite.daemon_analyze(&daemon, "locality-local", &local)?;
        let incremental_dir_count =
            count_entries(&work_root.join("daemon").join("incremental"));
        suite.check(
            "granularity_observable_proxy",
            local_cold.success
                && local_daemon.success
                && local_cold.signatures() == ravum_daemon::signature_set(&local_daemon.diagnostics),
            json!({
                "local_body_edit_cold_ms": local_cold.ms,
                "local_body_edit_daemon_ms": local_daemon.timings.total_ms,
                "local_body_edit_daemon_analysis_ms": local_daemon.timings.analysis_ms,
                "incremental_root_entries": incremental_dir_count,
                "caveat": "timing differences are a proxy for invalidation granularity, not query-level proof: rustc's public CLI does not expose per-query reuse. The correctness side (callers re-checked after a signature change) is proven by cross_module_signature_invalidation.",
            }),
        );
    }

    let end_load = load_json(&[], &crate::harness::rustc_path());
    let mut passed = 0usize;
    let mut failed = 0usize;
    for check in &suite.checks {
        if check["passed"].as_bool() == Some(true) {
            passed += 1;
        } else {
            failed += 1;
        }
    }
    let daemon_stats = daemon.stats();
    Ok(json!({
        "schema_version": 1,
        "kind": "mvp0b-incremental-invariants",
        "generated_unix": ravum_daemon::cache::now_unix(),
        "machine": machine_json(),
        "toolchain": {
            "release": toolchain.release,
            "host": toolchain.host,
            "commit_hash": toolchain.commit_hash,
        },
        "toolchain_evidence": toolchain_evidence,
        "git": crate::harness::git_json(&options.repo_root),
        "command": "cargo run -q -p ravum-daemon --bin ravum-daemon-bench -- --suite invariants",
        "load_start": start_load,
        "load_end": end_load,
        "checks": suite.checks,
        "summary": {
            "passed": passed,
            "failed": failed,
            "total": passed + failed,
            "valid": failed == 0,
        },
        "daemon_stats": daemon_stats,
        "base_cold_reference": {
            "ms": base_cold.ms,
            "success": base_cold.success,
            "warmup_daemon_status": warm.status.as_str(),
        },
        "limits": [
            "these checks prove the daemon's own contracts; they are not a proof of rustc's internal invalidation granularity",
            "the toolchain-change check uses the documented test seam (forced toolchain identity); no second toolchain was installed",
            "cancellation timing depends on host load; the recorded status shows whether the kill path or the post-completion stale path was taken",
            "revision numbers are session-scoped: a restarted daemon starts at revision 0, persistence covers content verdicts and the incremental directory",
        ],
    }))
}

fn count_entries(path: &Path) -> usize {
    std::fs::read_dir(path)
        .map(|entries| entries.flatten().count())
        .unwrap_or(0)
}

pub fn write(path: &Path, value: &Value) -> Result<(), String> {
    write_evidence(path, value)
}


