//! The three-way campaign: cold `rustc` vs persistent `-C incremental` vs the
//! daemon, per editing category.

use crate::edits::{build_states, categories, CategoryKind, CategorySpec};
use crate::harness::{
    base_files, compare_runs, content_digest, load_fixture, load_json, machine_json,
    measurement_definitions, open_daemon, run_cold, run_daemon, run_incremental, stats,
    toolchain_json, write_evidence, Fixture, ModeRun,
};
use ravum_daemon::{AnalysisReport, Daemon, SourceFile};
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy)]
enum Mode {
    Cold,
    Incremental,
    Daemon,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Mode::Cold => "cold",
            Mode::Incremental => "incremental",
            Mode::Daemon => "daemon",
        }
    }
}

pub struct CampaignOptions {
    pub repo_root: PathBuf,
    pub work_root: PathBuf,
    pub samples: usize,
}

pub fn run(options: &CampaignOptions) -> Result<Value, String> {
    if options.samples == 0 {
        return Err("--samples must be >= 1".to_owned());
    }
    std::fs::create_dir_all(&options.work_root)
        .map_err(|error| format!("cannot create {}: {error}", options.work_root.display()))?;
    let fixture = load_fixture(&options.repo_root)?;
    let (toolchain, toolchain_evidence) = toolchain_json(&options.repo_root)?;
    let rustc = crate::harness::rustc_path();
    let daemon = open_daemon(&options.work_root, 1)?;
    let mut revision: u64 = 0;

    let start_load = load_json(&[], &rustc);
    let mut category_values = Vec::new();
    let mut calibration: Vec<u64> = start_load["rustc_spawn_calibration"]["samples_ms"]
        .as_array()
        .map(|values| values.iter().filter_map(|value| value.as_u64()).collect())
        .unwrap_or_default();
    let mut unexpected_failures = 0usize;
    let mut consistency_failures = 0usize;
    let mut sequence: u64 = 0;

    let base = base_files(&fixture);
    for category in categories() {
        let states = build_states(&base, options.samples, &category)?;
        let category_dir = options.work_root.join("cold").join(category.name);
        let _ = std::fs::remove_dir_all(&category_dir);

        // Unrecorded warm-up: one call per mode on the base content so the
        // first recorded sample is not a "first ever compile".
        let mut warmup = Vec::new();
        warmup.push(json!({
            "mode": "cold",
            "ms": run_cold(&options.work_root, &rustc, &fixture, category.name, sequence, &base)?.ms,
        }));
        sequence += 1;
        warmup.push(json!({
            "mode": "incremental",
            "ms": run_incremental(&options.work_root, &rustc, &fixture, category.name, &base)?.ms,
        }));
        revision += 1;
        let warm_daemon = run_daemon(
            &daemon,
            revision,
            format!("{}#warmup", category.name),
            &fixture,
            &base,
        )?;
        warmup.push(json!({
            "mode": "daemon",
            "ms": warm_daemon.ms,
            "cache_kind": warm_daemon.cache_kind,
            "status": warm_daemon.report_status,
        }));

        let mut samples = Vec::new();
        let mut cold_ms = Vec::new();
        let mut incremental_ms = Vec::new();
        let mut daemon_ms = Vec::new();
        let mut daemon_accept = Vec::new();
        let mut daemon_lookup = Vec::new();
        let mut daemon_analysis = Vec::new();
        let mut daemon_cache_kinds: Vec<String> = Vec::new();
        let mut invalid_phase_ms = [Vec::new(), Vec::new(), Vec::new()];
        let mut repair_phase_ms = [Vec::new(), Vec::new(), Vec::new()];

        let phases: Vec<(&'static str, Vec<SourceFile>, bool)> = match category.kind {
            CategoryKind::InvalidRepair => {
                let mut phases = Vec::new();
                for pair in states.chunks(2) {
                    phases.push(("invalid", pair[0].clone(), false));
                    phases.push(("repair", pair[1].clone(), true));
                }
                phases
            }
            _ => states
                .iter()
                .map(|state| ("edit", state.clone(), true))
                .collect(),
        };

        for (index, (phase, state, expect_success)) in phases.iter().enumerate() {
            revision += 1;
            let label = format!("{}#{}", category.name, index + 1);
            let mut runs: Vec<ModeRun> = Vec::new();
            let order = match index % 3 {
                0 => [Mode::Cold, Mode::Incremental, Mode::Daemon],
                1 => [Mode::Incremental, Mode::Daemon, Mode::Cold],
                _ => [Mode::Daemon, Mode::Cold, Mode::Incremental],
            };
            for mode in order {
                let run = match mode {
                    Mode::Cold => {
                        let run = run_cold(
                            &options.work_root,
                            &rustc,
                            &fixture,
                            category.name,
                            sequence,
                            state,
                        )?;
                        sequence += 1;
                        run
                    }
                    Mode::Incremental => {
                        run_incremental(&options.work_root, &rustc, &fixture, category.name, state)?
                    }
                    Mode::Daemon => run_daemon(&daemon, revision, label.clone(), &fixture, state)?,
                };
                runs.push(run);
            }
            let by_mode = |mode: Mode| -> &ModeRun {
                runs.iter()
                    .find(|run| run.mode == mode.name())
                    .expect("every mode ran exactly once")
            };
            let cold = by_mode(Mode::Cold);
            let incremental = by_mode(Mode::Incremental);
            let daemon_run = by_mode(Mode::Daemon);

            let mismatch = compare_runs(&[cold, incremental, daemon_run]);
            if mismatch.is_some() {
                consistency_failures += 1;
            }
            let outcome_valid = cold.success == *expect_success
                && incremental.success == *expect_success
                && daemon_run.success == *expect_success;
            if !outcome_valid {
                unexpected_failures += 1;
            }

            cold_ms.push(cold.ms);
            incremental_ms.push(incremental.ms);
            daemon_ms.push(daemon_run.ms);
            daemon_cache_kinds.push(
                daemon_run
                    .cache_kind
                    .clone()
                    .unwrap_or_else(|| "unknown".to_owned()),
            );
            if let Some(timings) = &daemon_run.timings {
                daemon_accept.push(timings.accept_ms);
                daemon_lookup.push(timings.lookup_ms);
                daemon_analysis.push(timings.analysis_ms);
            }
            if category.kind == CategoryKind::InvalidRepair {
                let bucket = if *phase == "invalid" {
                    &mut invalid_phase_ms
                } else {
                    &mut repair_phase_ms
                };
                bucket[0].push(cold.ms);
                bucket[1].push(incremental.ms);
                bucket[2].push(daemon_run.ms);
            }

            samples.push(json!({
                "index": index + 1,
                "phase": phase,
                "content_sha16": content_digest(state),
                "expected_success": expect_success,
                "runs": {
                    "cold": cold.to_json(),
                    "incremental": incremental.to_json(),
                    "daemon": daemon_run.to_json(),
                },
                "diagnostics_consistent": mismatch.is_none(),
                "diagnostics_mismatch": mismatch,
                "outcome_as_expected": outcome_valid,
            }));
        }

        let mut category_value = json!({
            "name": category.name,
            "description": category.description,
            "kind": format!("{:?}", category.kind),
            "warmup": warmup,
            "samples": samples,
            "stats_ms": {
                "cold": stats(&cold_ms),
                "incremental": stats(&incremental_ms),
                "daemon": stats(&daemon_ms),
            },
            "daemon_timings_ms": {
                "accept": stats(&daemon_accept),
                "lookup": stats(&daemon_lookup),
                "analysis": stats(&daemon_analysis),
            },
            "daemon_cache_kinds": daemon_cache_kinds,
            "consistency": {
                "failures": consistency_failures,
                "note": "diagnostic signature sets must be identical in all three modes",
            },
            "unexpected_failures": unexpected_failures,
        });
        if category.kind == CategoryKind::InvalidRepair {
            category_value["phase_stats_ms"] = json!({
                "invalid_buffer": {
                    "cold": stats(&invalid_phase_ms[0]),
                    "incremental": stats(&invalid_phase_ms[1]),
                    "daemon": stats(&invalid_phase_ms[2]),
                },
                "repaired_buffer": {
                    "cold": stats(&repair_phase_ms[0]),
                    "incremental": stats(&repair_phase_ms[1]),
                    "daemon": stats(&repair_phase_ms[2]),
                },
            });
        }
        let end_load = load_json(&calibration, &rustc);
        calibration = end_load["rustc_spawn_calibration"]["samples_ms"]
            .as_array()
            .map(|values| values.iter().filter_map(|value| value.as_u64()).collect())
            .unwrap_or_default();
        category_value["load_end"] = end_load;
        category_values.push(category_value);
    }

    let daemon_stats = daemon.stats();
    let daemon_work_size = dir_size(&options.work_root.join("daemon"));
    let end_load = load_json(&calibration, &rustc);
    let consistency_ok = consistency_failures == 0;
    let invalid = unexpected_failures > 0 || !consistency_ok;

    Ok(json!({
        "schema_version": 1,
        "kind": "mvp0b-incremental-campaign",
        "generated_unix": ravum_daemon::cache::now_unix(),
        "machine": machine_json(),
        "toolchain": {
            "release": toolchain.release,
            "host": toolchain.host,
            "commit_hash": toolchain.commit_hash,
            "identity_sha256": toolchain.id(),
        },
        "toolchain_evidence": toolchain_evidence,
        "git": crate::harness::git_json(&options.repo_root),
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
                "flags": fixture.context.flags,
            },
        },
        "measurement": measurement_definitions(),
        "command": "cargo run -q -p ravum-daemon --bin ravum-daemon-bench -- --suite campaign --samples 10",
        "rustc_reference_commands": {
            "cold": "rustc --edition=2021 --crate-name=acme_unit --crate-type=lib --emit=metadata --error-format=json --diagnostic-width=120 --out-dir=<fresh dir> <unit>/src/lib.rs",
            "incremental": "same command with -C incremental=<persistent dir>, source files kept at the same paths",
            "daemon": format!("ravum-daemon API: Daemon::analyze(AnalysisRequest) with -C incremental=<work>/incremental/<context> and content cache"),
        },
        "parallelism": {
            "rustc_concurrent_max": 1,
            "note": "campaign runs one compiler at a time; the daemon under test is configured with max_concurrent_jobs=1 (reference machine: 16 Go RAM, ~2 Go free)",
        },
        "load_start": start_load,
        "load_end": end_load,
        "categories": category_values,
        "daemon_state": {
            "stats": daemon_stats,
            "cache_entries_memory": daemon.cache_entries().0,
            "cache_entries_disk": daemon.cache_entries().1,
            "work_dir": options.work_root.join("daemon").display().to_string(),
            "work_dir_bytes": daemon_work_size,
            "incremental_dir_bytes": dir_size(&options.work_root.join("daemon").join("incremental")),
        },
        "valid": !invalid,
        "consistency_failures": consistency_failures,
        "unexpected_failures": unexpected_failures,
        "limits": [
            "OBSERVED numbers on the shared reference machine; not portable guarantees. The host ran other MVP-0b agents concurrently (see load_start/load_end and per-category load_end).",
            "cold/incremental `ms` measure the rustc process only (source files already written); daemon `ms` is end-to-end `Daemon::analyze` including hashing, snapshot sync and cache lookup, while daemon `timings.analysis_ms` isolates the compiler process.",
            "the daemon does not do codegen, linking, artifact verification or activation: this is an analysis-stage measurement only",
            "10 samples per cell: p95 is the 10th value, a coarse tail estimate, not a p95 confidence interval",
            "the fixture is a small real library unit (~280 lines, 4 modules); absolute numbers scale with unit size and are not extrapolated to large crates",
            "the daemon's stable compile-unit directory mirrors an editor that keeps the same file paths; snapshots are immutable but compilation happens from the stable unit, which is what makes -C incremental reusable. Compiling from a fresh directory per edit would invalidate the incremental cache.",
        ],
    }))
}

fn dir_size(path: &std::path::Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(metadata) = entry.metadata() {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    total
}

/// Writes the campaign evidence to `path`.
pub fn write(path: &std::path::Path, value: &Value) -> Result<(), String> {
    write_evidence(path, value)
}

/// Card of the request type used by the campaign (kept for API relevance).
#[allow(dead_code)]
fn analysis_report_success(report: &AnalysisReport) -> bool {
    report.success
}

/// Unused import guard for `CategorySpec`/`Daemon`: they document the types the
/// campaign is driven by.
#[allow(dead_code)]
fn _types(_: &CategorySpec, _: &Daemon, _: &Fixture) {}
