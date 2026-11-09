//! MVP-0c L1.2 — measured cost of chained cross-crate analysis in the daemon.
//!
//! Compares, with the real `Daemon` API and real `rustc` runs:
//! * **isolated**: one self-contained unit analysed alone (legacy path, no externs);
//! * **chained**: `core` → `api` (with `core`'s `.rmeta` as extern) → `consumer`
//!   (with `api`'s `.rmeta` as extern), each dependency artifact coming from a
//!   previous daemon analysis (`report.artifact_sha256` + `Daemon::artifact`).
//!
//! Reports exact per-round timings, rustc invocation counts, snapshot sizes,
//! hit/miss, and the cost of an extern change (miss + new snapshot identity).
//! No speed-up is claimed: L1.2 is about correctness and usable wiring.

use crate::harness::{git_json, load_json, machine_json, toolchain_json, write_evidence};
use ravum_daemon::{
    AnalysisRequest, BuildContext, Daemon, DaemonConfig, ExternInput, Priority, SourceFile,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct CrossCrateOptions {
    pub repo_root: PathBuf,
    pub work_root: PathBuf,
    pub rounds: usize,
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

fn percentile(values: &[u64], p: f64) -> u64 {
    if values.is_empty() {
        return 0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

fn latency_stats(values: &[u64]) -> Value {
    if values.is_empty() {
        return json!({"count": 0});
    }
    let sum: u64 = values.iter().sum();
    json!({
        "count": values.len(),
        "min": values.iter().min().copied().unwrap_or(0),
        "mean": (sum as f64) / (values.len() as f64),
        "p50": percentile(values, 50.0),
        "p95": percentile(values, 95.0),
        "max": values.iter().max().copied().unwrap_or(0),
        "samples": values,
    })
}

fn fixture_source(repo_root: &Path, unit: &str) -> Result<String, String> {
    let path = repo_root
        .join("fixtures/multicrate")
        .join(unit)
        .join("src/lib.rs");
    std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read fixture {}: {error}", path.display()))
}

fn daemon(work_dir: &Path) -> Result<Daemon, String> {
    let _ = std::fs::remove_dir_all(work_dir);
    std::fs::create_dir_all(work_dir)
        .map_err(|error| format!("cannot create {}: {error}", work_dir.display()))?;
    Daemon::open(DaemonConfig {
        rustc: crate::harness::rustc_path(),
        work_dir: work_dir.to_path_buf(),
        max_concurrent_jobs: 1,
        poll_interval_ms: 2,
        analysis_timeout_ms: 120_000,
        ..Default::default()
    })
    .map_err(|error| format!("cannot open daemon: {error}"))
}

fn request(revision: u64, label: &str, crate_name: &str, content: String) -> AnalysisRequest {
    AnalysisRequest {
        revision,
        label: label.to_owned(),
        root: "src/lib.rs".to_owned(),
        files: vec![SourceFile {
            path: "src/lib.rs".to_owned(),
            content,
        }],
        context: BuildContext {
            edition: "2021".to_owned(),
            crate_name: crate_name.to_owned(),
            crate_type: "lib".to_owned(),
            flags: Vec::new(),
        },
        priority: Priority::Normal,
    }
}

fn record(unit: &str, report: &ravum_daemon::AnalysisReport) -> Value {
    let snapshot_bytes = report
        .snapshot
        .as_ref()
        .map(|snapshot| dir_size(Path::new(&snapshot.dir)))
        .unwrap_or(0);
    json!({
        "unit": unit,
        "status": report.status.as_str(),
        "success": report.success,
        "cache_hit": report.cache.hit,
        "cache_kind": report.cache.kind.as_str(),
        "analysis_ms": report.timings.analysis_ms,
        "queue_ms": report.timings.queue_ms,
        "lookup_ms": report.timings.lookup_ms,
        "total_ms": report.timings.total_ms,
        "externs": report.externs.len(),
        "extern_names": report.externs.iter().map(|extern_usage| extern_usage.name.clone()).collect::<Vec<_>>(),
        "artifact_sha256": report.artifact_sha256,
        "snapshot_id": report.snapshot_id,
        "snapshot_bytes": snapshot_bytes,
        "diagnostics": report.diagnostics.len(),
    })
}

struct ChainStep {
    record: Value,
    artifact: Option<(String, Vec<u8>)>,
}

fn chain_step(
    daemon: &Daemon,
    revision: u64,
    label: &str,
    crate_name: &str,
    content: String,
    externs: &[ExternInput],
) -> Result<ChainStep, String> {
    let report = daemon
        .analyze_with_externs(&request(revision, label, crate_name, content), externs)
        .map_err(|error| format!("{label} analysis failed: {error}"))?;
    let artifact = match &report.artifact_sha256 {
        Some(interface_id) => {
            let bytes = daemon
                .artifact(interface_id)
                .map_err(|error| format!("cannot read {label} artifact: {error}"))?;
            Some((interface_id.clone(), bytes))
        }
        None => None,
    };
    Ok(ChainStep {
        record: record(label, &report),
        artifact,
    })
}

fn extern_for(artifact: &Option<(String, Vec<u8>)>, name: &str) -> Result<ExternInput, String> {
    let (_, bytes) = artifact
        .as_ref()
        .ok_or_else(|| format!("no artifact produced for dependency {name}"))?;
    ExternInput::new(name, name, bytes.clone())
        .map_err(|error| format!("invalid extern {name}: {error}"))
}

pub fn run(options: &CrossCrateOptions) -> Result<Value, String> {
    if options.rounds == 0 || options.rounds > 20 {
        return Err("--cross-crate-rounds must be in 1..=20".to_owned());
    }
    let core_source = fixture_source(&options.repo_root, "core")?;
    let api_source = fixture_source(&options.repo_root, "api")?;
    let consumer_source = fixture_source(&options.repo_root, "consumer")?;
    let rustc = crate::harness::rustc_path();
    let (toolchain, toolchain_evidence) = toolchain_json(&options.repo_root)?;
    let load_start = load_json(&[], &rustc);
    let generated_unix = now_unix();
    std::fs::create_dir_all(&options.work_root)
        .map_err(|error| format!("cannot create work root: {error}"))?;

    // --- Isolated phase: one self-contained unit, no externs (legacy path).
    let isolated_daemon = daemon(&options.work_root.join("isolated"))?;
    let mut isolated_rounds = Vec::new();
    for round in 0..options.rounds {
        let revision = (round as u64) + 1;
        let report = isolated_daemon
            .analyze(&request(
                revision,
                "isolated-core",
                "core",
                core_source.clone(),
            ))
            .map_err(|error| format!("isolated analysis failed: {error}"))?;
        isolated_rounds.push(record("core", &report));
    }
    drop(isolated_daemon);

    // --- Chained phase: core -> api(extern core) -> consumer(extern api).
    let chained_daemon = daemon(&options.work_root.join("chained"))?;
    let mut chained_rounds = Vec::new();
    let mut revision: u64 = 0;
    for round in 0..options.rounds {
        let phase_started = std::time::Instant::now();
        revision += 1;
        let core = chain_step(
            &chained_daemon,
            revision,
            "core",
            "core",
            core_source.clone(),
            &[],
        )?;
        revision += 1;
        let core_extern = extern_for(&core.artifact, "core")?;
        let api = chain_step(
            &chained_daemon,
            revision,
            "api",
            "api",
            api_source.clone(),
            &[core_extern],
        )?;
        revision += 1;
        let api_extern = extern_for(&api.artifact, "api")?;
        let consumer = chain_step(
            &chained_daemon,
            revision,
            "consumer",
            "consumer",
            consumer_source.clone(),
            &[api_extern],
        )?;
        let elapsed_ms = phase_started.elapsed().as_millis() as u64;
        let steps = [&core, &api, &consumer];
        chained_rounds.push(json!({
            "round": round + 1,
            "phase_total_ms": elapsed_ms,
            "steps": steps.iter().map(|step| step.record.clone()).collect::<Vec<_>>(),
            "all_success": steps.iter().all(|step| step.record["success"] == true),
        }));
    }

    // --- Extern change: same api sources, changed core interface => real misses.
    let mut extern_change = json!({});
    {
        revision += 1;
        let changed_core = format!("{core_source}\npub fn cross_crate_marker() -> f64 {{ 1.0 }}\n");
        let core_v2 = chain_step(
            &chained_daemon,
            revision,
            "core-v2",
            "core",
            changed_core,
            &[],
        )?;
        revision += 1;
        let core_extern = extern_for(&core_v2.artifact, "core")?;
        let api_v2 = chain_step(
            &chained_daemon,
            revision,
            "api-after-extern-change",
            "api",
            api_source.clone(),
            &[core_extern],
        )?;
        extern_change = json!({
            "core_before": chained_rounds.last().map(|round| round["steps"][0].clone()),
            "core_after": core_v2.record,
            "api_after": api_v2.record,
            "api_is_miss": api_v2.record["cache_hit"] == false,
            "api_snapshot_changed": chained_rounds
                .last()
                .map(|round| round["steps"][1]["snapshot_id"].clone())
                != Some(api_v2.record["snapshot_id"].clone()),
            "note": "same api sources, different core interface artifact: the extern identity is part of the snapshot and cache key, so the dependent is really recompiled (miss), never a stale HIT",
        });
    }
    drop(chained_daemon);

    // --- Aggregates.

    fn numeric(records: &[Value], field: &str) -> Vec<u64> {
        records
            .iter()
            .filter_map(|record| record[field].as_u64())
            .collect()
    }
    let isolated_records = isolated_rounds.clone();
    let isolated_total = numeric(&isolated_records, "total_ms");
    let isolated_analysis = numeric(&isolated_records, "analysis_ms");
    // Snapshot artifacts only exist on real misses: a HIT reuses the snapshot and
    // reports no snapshot directory, so the size aggregate is over miss rounds.
    let isolated_bytes: Vec<u64> = isolated_records
        .iter()
        .filter_map(|record| record["snapshot_bytes"].as_u64())
        .filter(|bytes| *bytes > 0)
        .collect();
    let mut chained_per_unit = Vec::new();
    for (index, unit) in ["core", "api", "consumer"].iter().enumerate() {
        let records = chained_rounds
            .iter()
            .map(|round| round["steps"][index].clone())
            .collect::<Vec<_>>();
        chained_per_unit.push(json!({
            "unit": unit,
            "total_ms": latency_stats(&numeric(&records, "total_ms")),
            "analysis_ms": latency_stats(&numeric(&records, "analysis_ms")),
            "snapshot_bytes_miss_rounds": latency_stats(
                &records
                    .iter()
                    .filter_map(|record| record["snapshot_bytes"].as_u64())
                    .filter(|bytes| *bytes > 0)
                    .collect::<Vec<_>>(),
            ),
            "cache_hits": records.iter().filter(|record| record["cache_hit"] == true).count(),
            "cache_misses": records.iter().filter(|record| record["cache_hit"] == false).count(),
            "records": records,
        }));
    }
    let chained_phase_total: Vec<u64> = chained_rounds
        .iter()
        .filter_map(|round| round["phase_total_ms"].as_u64())
        .collect();
    let chained_rustc_runs = chained_rounds
        .iter()
        .flat_map(|round| round["steps"].as_array().cloned().unwrap_or_default())
        .filter(|step| step["analysis_ms"].as_u64().unwrap_or(0) > 0)
        .count();
    let isolated_rustc_runs = isolated_records
        .iter()
        .filter(|record| record["analysis_ms"].as_u64().unwrap_or(0) > 0)
        .count();
    let load_end = load_json(&[], &rustc);

    Ok(json!({
        "schema_version": 1,
        "kind": "mvp0c-cross-crate-daemon-2026-11-09",
        "generated_unix": generated_unix,
        "commands": [
            "cargo run -q -p ravum-daemon --bin ravum-daemon-bench -- --suite cross-crate",
        ],
        "machine": machine_json(),
        "toolchain": {
            "release": toolchain.release,
            "host": toolchain.host,
            "commit_hash": toolchain.commit_hash,
        },
        "toolchain_evidence": toolchain_evidence,
        "git": git_json(&options.repo_root),
        "load_start": load_start,
        "load_end": load_end,
        "fixture": "fixtures/multicrate core -> api -> consumer (real sources, real .rmeta artifacts)",
        "rounds": options.rounds,
        "isolated": {
            "description": "one self-contained unit analysed alone, no externs (legacy path)",
            "rustc_invocations": isolated_rustc_runs,
            "total_ms": latency_stats(&isolated_total),
            "analysis_ms": latency_stats(&isolated_analysis),
            "snapshot_bytes": latency_stats(&isolated_bytes),
            "records": isolated_rounds,
        },
        "chained": {
            "description": "core then api with core .rmeta extern, then consumer with api .rmeta extern",
            "phase_total_ms": latency_stats(&chained_phase_total),
            "rustc_invocations": chained_rustc_runs,
            "per_unit": chained_per_unit,
            "rounds": chained_rounds,
        },
        "extern_change": extern_change,
        "comparison": {
            "isolated_total_ms_mean": latency_stats(&isolated_total)["mean"],
            "chained_phase_total_ms_mean": latency_stats(&chained_phase_total)["mean"],
            "isolated_rustc_invocations": isolated_rustc_runs,
            "chained_rustc_invocations": chained_rustc_runs,
            "note": "chained analysis requires dependency artifacts and extern linkage; it is not expected to be faster than a single isolated unit and no speed-up is claimed",
        },
        "valid": true,
        "limits": [
            "tiny fixture, one machine, one local toolchain (rustc 1.95.0)",
            "only lib targets are chained; binaries/features/build scripts still require the Cargo fallback path",
            "the cache is local and content-addressed; first round is cold, later rounds are hits",
            "an extern is a .rmeta interface artifact: its change invalidates dependents by design, which costs a real recompilation",
            "no whole-program correctness claim: reports are bounded observations, never PROVED",
        ],
    }))
}

pub fn write(path: &Path, value: &Value) -> Result<(), String> {
    write_evidence(path, value)
}
