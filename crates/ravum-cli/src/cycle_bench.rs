//! `ravum cycle-bench` — end-to-end measurement of the continuous loop.
//!
//! Answers the only P0-A/P0-I question that matters: **how long does a real edit take,
//! from the content change to an activated patch?**, and what does the persistent
//! analysis engine (MVP-0b) actually change compared to a cold `rustc` (MVP-0a)?
//!
//! Protocol, frozen before execution:
//! * one fixture source file, N *distinct real edits* (each iteration appends a new
//!   public function, so every edit changes what rustc has to analyse and build);
//! * two independent pipelines and hosts (separate work directories and artifact caches)
//!   so the two variants never share a cached build;
//! * per content, two submissions: `edit` (the content is new) and `resubmit` (the same
//!   content is sent again, the interactive "unchanged buffer" case);
//! * variant order alternates per iteration to spread machine-load drift;
//! * every stage is timed around the real call: snapshot, analysis (cold `rustc` or
//!   persistent daemon), build, smoke verification, native load, host activation;
//! * activation refusal is recorded, never hidden (a resubmit is refused `AlreadyActive`
//!   by design: it is a cache scenario, not a new activation).
//!
//! Honest limits are written into the JSON: local non-portable numbers, one small
//! single-file unit, `build_ms` covers codegen + link inside one `rustc` process, and the
//! daemon's analysis-only gains are not extrapolated beyond what was measured.

use crate::commands::{flag, flag_u64, positional, work_dir};
use crate::metrics::{env_info, stats, working_set_bytes};
use ravum_coordinator::{smoke_verify, Pipeline};
use ravum_daemon::{single_file_request, BuildContext, Daemon, DaemonConfig};
use ravum_patch_host::{prepare, ExpectedAbi, PatchHost};
use ravum_protocol::{ActivationStatus, BuildOutput, EvidenceRecord, SnapshotId};
use ravum_snapshot::SourceSnapshot;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Instant;

const LOGICAL_PATH: &str = "unit.rs";

pub fn cycle_bench(args: &[String]) -> u8 {
    let Some(file) = positional(args, 0) else {
        eprintln!(
            "usage: ravum cycle-bench <fixture.rs> [--iterations N] [--out PATH] \
             [--daemon-work DIR] [--reuse-daemon] [--concurrent] [--json]"
        );
        return 2;
    };
    let iterations = flag_u64(args, "--iterations", 8).max(2);
    let out_path = flag(args, "--out").unwrap_or_else(|| {
        work_dir()
            .join("cycle-bench.json")
            .to_string_lossy()
            .to_string()
    });
    let daemon_dir: PathBuf = flag(args, "--daemon-work")
        .map(PathBuf::from)
        .unwrap_or_else(|| work_dir().join("cycle-daemon"));
    let reuse_daemon = args.iter().any(|a| a == "--reuse-daemon");
    let concurrent = args.iter().any(|a| a == "--concurrent");
    let print_json = args.iter().any(|a| a == "--json");

    // Optional load probes, injected by the caller (PowerShell) so the evidence file
    // carries the machine conditions instead of only the compiler calibration.
    let load_start = env_u64("RAVUM_CPU_LOAD_PERCENT");
    let free_start = env_u64("RAVUM_FREE_BYTES");
    let base_content = match std::fs::read_to_string(&file) {
        Ok(content) => content,
        Err(error) => {
            eprintln!("cannot read {file}: {error}");
            return 2;
        }
    };

    // Separate identities per variant: no artifact cache or host state is shared.
    let cold_dir = work_dir().join("cycle-cold");
    let daemon_pipeline_dir = work_dir().join("cycle-daemon-pipeline");
    for dir in [&cold_dir, &daemon_pipeline_dir] {
        let _ = std::fs::remove_dir_all(dir);
    }
    if !reuse_daemon {
        let _ = std::fs::remove_dir_all(&daemon_dir);
    }

    let rustc = std::env::var("RAVUM_RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let cold_pipeline = Pipeline::new(&rustc, &cold_dir);
    let warm_pipeline = Pipeline::new(&rustc, &daemon_pipeline_dir);
    let rustc_release = cold_pipeline
        .adapter()
        .toolchain()
        .map(|t| t.release)
        .unwrap_or_else(|_| "unknown".to_owned());

    let mut daemon = match Daemon::open(DaemonConfig {
        rustc: PathBuf::from(&rustc),
        work_dir: daemon_dir.clone(),
        ..DaemonConfig::default()
    }) {
        Ok(daemon) => daemon,
        Err(error) => {
            eprintln!("cannot open the analysis daemon: {error}");
            return 2;
        }
    };

    // Bootstrap both hosts from the unmodified content.
    let base_snapshot = SourceSnapshot::new([(LOGICAL_PATH.to_owned(), base_content.clone())]);
    let Some((mut cold_host, cold_base_build)) = bootstrap(&cold_pipeline, &base_snapshot) else {
        return 1;
    };
    let Some((mut warm_host, warm_base_build)) = bootstrap(&warm_pipeline, &base_snapshot) else {
        return 1;
    };
    eprintln!(
        "bootstrap: cold artifact {} / warm artifact {}",
        short(&cold_base_build.artifact_id.to_string()),
        short(&warm_base_build.artifact_id.to_string())
    );

    let memory_before = working_set_bytes();
    let mut samples: Vec<Sample> = Vec::with_capacity((iterations * 4) as usize);
    let mut revision = 1u64;
    let mut failed_edits = 0u64;

    for iteration in 0..iterations {
        let content = edited_content(&base_content, iteration);
        let cold_first = iteration % 2 == 0;
        let order: [Variant; 2] = if cold_first {
            [Variant::Cold, Variant::Daemon]
        } else {
            [Variant::Daemon, Variant::Cold]
        };

        for variant in order {
            for phase in [Phase::Edit, Phase::Resubmit] {
                revision += 1;
                let sample = run_submission(
                    variant,
                    phase,
                    iteration,
                    revision,
                    &content,
                    &mut cold_host,
                    &mut warm_host,
                    &cold_pipeline,
                    &warm_pipeline,
                    &mut daemon,
                    concurrent,
                );
                if phase == Phase::Edit && !sample.activated {
                    failed_edits += 1;
                }
                samples.push(sample);
            }
        }
    }

    let memory_after = working_set_bytes();
    let load_end = env_u64("RAVUM_CPU_LOAD_PERCENT");
    let free_end = env_u64("RAVUM_FREE_BYTES");
    let calibration = rustc_spawn_calibration(&rustc, 5);
    let daemon_stats = daemon.stats();

    let scenarios = json!({
        "cold": scenario_stats(&samples, Variant::Cold),
        "daemon": scenario_stats(&samples, Variant::Daemon),
    });
    let payload = json!({
        "schema_version": 1,
        "tool": "ravum cycle-bench",
        "tool_version": env!("CARGO_PKG_VERSION"),
        "rustc": rustc_release,
        "env": env_info(),
        "fixture": {
            "path": file,
            "logical_path": LOGICAL_PATH,
            "base_bytes": base_content.len(),
            "edit_kind": "each iteration appends a new public function: rustc must analyse and build new code",
        },
        "iterations": iterations,
        "submissions": samples.len(),
        "failed_edits": failed_edits,
        "scenarios": scenarios,
        "daemon": {
            "work_dir": daemon_dir.to_string_lossy(),
            "reused": reuse_daemon,
            "requests": daemon_stats.requests,
            "cache_hits_memory": daemon_stats.cache_hits_memory,
            "cache_hits_disk": daemon_stats.cache_hits_disk,
            "cache_misses": daemon_stats.cache_misses,
            "verifier_runs": daemon_stats.rustc_runs,
        },
        "machine": {
            "processor": std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_else(|_| "unknown".to_owned()),
            "cpu_load_percent_at_start": load_start,
            "cpu_load_percent_at_end": load_end,
            "free_bytes_at_start": free_start,
            "free_bytes_at_end": free_end,
        },
        "memory": {
            "working_set_bytes_before": memory_before,
            "working_set_bytes_after": memory_after,
        },
        "calibration": {
            "rustc_spawn_ms": stats(&calibration),
            "note": "median wall time of a bare `rustc -vV` spawn in this run: machine-load floor, not analysis cost",
        },
        "scheduling": if concurrent { "concurrent" } else { "sequential" },
        "artifact_cache": "enabled in both variants, but each variant has its own work directory: an edit is a real build in both, a resubmit is a cache hit in both",
        "samples": samples.iter().map(Sample::to_json).collect::<Vec<_>>(),
        "limits": [
            "local, observed numbers on the machine described in env; not portable guarantees",
            "one small single-file unit; gains do not extrapolate to large units or multi-crate workspaces",
            "build_ms covers codegen and link inside one rustc process",
            "the daemon covers analysis only: build, verify and load stay cold and unchanged",
            "p95/p99 over a small sample is a coarse nearest-rank estimate, not a stable tail",
            "a resubmit is refused AlreadyActive by design: it measures the cache path, not a new activation",
            "with --concurrent, the analysis and build share the machine: each stage wall time includes contention, while total_ms is the real overlapped wall clock",
        ],
    });

    if let Some(parent) = Path::new(&out_path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(
        &out_path,
        serde_json::to_string_pretty(&payload).unwrap_or_default(),
    ) {
        Ok(()) => eprintln!("written: {out_path}"),
        Err(error) => {
            eprintln!("cannot write {out_path}: {error}");
            return 2;
        }
    }

    if print_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_default()
        );
    } else {
        print_summary(&payload);
    }
    if failed_edits == 0 {
        0
    } else {
        1
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Variant {
    Cold,
    Daemon,
}

impl Variant {
    fn label(self) -> &'static str {
        match self {
            Variant::Cold => "cold",
            Variant::Daemon => "daemon",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Edit,
    Resubmit,
}

impl Phase {
    fn label(self) -> &'static str {
        match self {
            Phase::Edit => "edit",
            Phase::Resubmit => "resubmit",
        }
    }
}

fn bootstrap(pipeline: &Pipeline, snapshot: &SourceSnapshot) -> Option<(PatchHost, BuildOutput)> {
    let outcome = match pipeline.build(snapshot) {
        Ok(outcome) => outcome,
        Err(error) => {
            eprintln!("bootstrap build failed: {}", error.0);
            return None;
        }
    };
    if !outcome.success {
        eprintln!("bootstrap build refused: {:?}", outcome.build.diagnostics);
        return None;
    }
    match PatchHost::bootstrap(
        ExpectedAbi::default(),
        Path::new(&outcome.build.artifact_path),
    ) {
        Ok(host) => Some((host, outcome.build)),
        Err(reason) => {
            eprintln!("bootstrap host refused: {reason:?}");
            None
        }
    }
}

/// A real source edit: a new public function is appended, so the content changes and
/// rustc really has to analyse and build something new.
fn edited_content(base: &str, iteration: u64) -> String {
    let mut content = base.to_owned();
    content.push_str(&format!(
        "\n// cycle-bench edit {iteration}: real code change, not a comment-only edit\n\
         #[allow(dead_code)]\n\
         pub fn cycle_bench_edit_{iteration}(value: u64) -> u64 {{\n    \
         value.wrapping_mul({}u64)\n}}\n",
        iteration + 3
    ));
    content
}

#[allow(clippy::too_many_arguments)]
/// Result of the analysis stage, shared by the sequential and concurrent paths.
enum AnalysisStage {
    Clean {
        wall_ms: u64,
        engine_ms: u64,
        cache_kind: String,
    },
    Diagnostics {
        wall_ms: u64,
        engine_ms: u64,
        cache_kind: String,
    },
    Failed {
        stage: &'static str,
        detail: String,
    },
}

/// Result of the codegen+link stage.
enum BuildStage {
    Built { build: BuildOutput, wall_ms: u64 },
    Failed { stage: &'static str, detail: String },
}

#[allow(clippy::too_many_arguments)]
fn analyze_stage(
    variant: Variant,
    phase: Phase,
    iteration: u64,
    revision: u64,
    content: &str,
    pipeline: &Pipeline,
    daemon: &Daemon,
) -> AnalysisStage {
    match variant {
        Variant::Cold => {
            let snapshot = SourceSnapshot::new([(LOGICAL_PATH.to_owned(), content.to_owned())]);
            let begin = Instant::now();
            match pipeline.analyze(&snapshot) {
                Ok(outcome) => {
                    let wall_ms = begin.elapsed().as_millis() as u64;
                    if outcome.diagnostics.has_errors() {
                        AnalysisStage::Diagnostics {
                            wall_ms,
                            engine_ms: wall_ms,
                            cache_kind: "cold".to_owned(),
                        }
                    } else {
                        AnalysisStage::Clean {
                            wall_ms,
                            engine_ms: wall_ms,
                            cache_kind: "cold".to_owned(),
                        }
                    }
                }
                Err(error) => AnalysisStage::Failed {
                    stage: "adapter error",
                    detail: error.0,
                },
            }
        }
        Variant::Daemon => {
            let request = single_file_request(
                revision,
                format!("cycle-bench-{iteration}-{}", phase.label()),
                LOGICAL_PATH,
                content.to_owned(),
                BuildContext::default(),
            );
            let begin = Instant::now();
            match daemon.analyze(&request) {
                Ok(report) => {
                    let wall_ms = begin.elapsed().as_millis() as u64;
                    let cache_kind = format!("{:?}", report.cache.kind);
                    if report.error_count() > 0 {
                        AnalysisStage::Diagnostics {
                            wall_ms,
                            engine_ms: report.timings.analysis_ms,
                            cache_kind,
                        }
                    } else {
                        AnalysisStage::Clean {
                            wall_ms,
                            engine_ms: report.timings.analysis_ms,
                            cache_kind,
                        }
                    }
                }
                Err(error) => AnalysisStage::Failed {
                    stage: "daemon error",
                    detail: error.to_string(),
                },
            }
        }
    }
}

fn build_stage(pipeline: &Pipeline, snapshot: &SourceSnapshot) -> BuildStage {
    let begin = Instant::now();
    match pipeline.build(snapshot) {
        Ok(outcome) if outcome.success => BuildStage::Built {
            build: outcome.build,
            wall_ms: begin.elapsed().as_millis() as u64,
        },
        Ok(_) => BuildStage::Failed {
            stage: "build refused",
            detail: "build failed".to_owned(),
        },
        Err(error) => BuildStage::Failed {
            stage: "adapter error",
            detail: error.0,
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn run_submission(
    variant: Variant,
    phase: Phase,
    iteration: u64,
    revision: u64,
    content: &str,
    cold_host: &mut PatchHost,
    warm_host: &mut PatchHost,
    cold_pipeline: &Pipeline,
    warm_pipeline: &Pipeline,
    daemon: &mut Daemon,
    concurrent: bool,
) -> Sample {
    let base = Sample {
        iteration,
        variant,
        phase,
        analysis_ms: 0,
        analysis_engine_ms: 0,
        build_ms: 0,
        verify_ms: 0,
        load_ms: 0,
        activate_ms: 0,
        total_ms: 0,
        critical_path_ms: 0,
        sequential_estimate_ms: 0,
        activated: false,
        outcome: "unstarted".to_owned(),
        cache_kind: "none".to_owned(),
        cached_build: false,
    };
    let started = Instant::now();
    let snapshot = SourceSnapshot::new([(LOGICAL_PATH.to_owned(), content.to_owned())]);

    let (pipeline, host) = match variant {
        Variant::Cold => (cold_pipeline, cold_host),
        Variant::Daemon => (warm_pipeline, warm_host),
    };

    // 1 + 2. analysis and codegen+link: sequential by default, or overlapped on two
    // threads when `--concurrent`. Overlapping is the two-track architecture the mandate
    // asks for (interactive diagnostics as soon as possible, heavy build in parallel);
    // the measurement keeps the two durations and the real wall clock separately so the
    // contention effect is visible instead of hidden.
    let (analysis, build) = if concurrent {
        std::thread::scope(|scope| {
            let analysis = scope.spawn(|| {
                analyze_stage(
                    variant, phase, iteration, revision, content, pipeline, daemon,
                )
            });
            let build = scope.spawn(|| build_stage(pipeline, &snapshot));
            (
                analysis.join().unwrap_or(AnalysisStage::Failed {
                    stage: "analysis panicked",
                    detail: "analysis thread panicked".to_owned(),
                }),
                build.join().unwrap_or(BuildStage::Failed {
                    stage: "build panicked",
                    detail: "build thread panicked".to_owned(),
                }),
            )
        })
    } else {
        (
            analyze_stage(
                variant, phase, iteration, revision, content, pipeline, daemon,
            ),
            build_stage(pipeline, &snapshot),
        )
    };

    let (analysis_ms, engine_ms, cache_kind) = match analysis {
        AnalysisStage::Failed { stage, detail } => return base.failed(stage, detail, started),
        AnalysisStage::Diagnostics {
            wall_ms,
            engine_ms,
            cache_kind,
        } => {
            return base.analysis_failed(wall_ms, engine_ms, cache_kind, started);
        }
        AnalysisStage::Clean {
            wall_ms,
            engine_ms,
            cache_kind,
        } => (wall_ms, engine_ms, cache_kind),
    };

    let (build, build_ms) = match build {
        BuildStage::Failed { stage, detail } => return base.failed(stage, detail, started),
        BuildStage::Built { build, wall_ms } => (build, wall_ms),
    };
    let cached_build = build.cached_build;
    let (critical_path_ms, sequential_estimate_ms) = schedule_effect(analysis_ms, build_ms);

    // 3. smoke verification (the same TESTED evidence the loop uses)
    let begin = Instant::now();
    let evidence: EvidenceRecord = match smoke_verify(&build) {
        Ok(evidence) => evidence,
        Err(reason) => return base.failed("verify refused", format!("{reason:?}"), started),
    };
    let verify_ms = begin.elapsed().as_millis() as u64;

    // 4. native load + ABI/manifest validation
    let begin = Instant::now();
    let prepared = match prepare(Path::new(&build.artifact_path), host.expected_abi()) {
        Ok(prepared) => prepared,
        Err(reason) => return base.failed("load refused", format!("{reason:?}"), started),
    };
    let load_ms = begin.elapsed().as_millis() as u64;

    // 5. activation
    let begin = Instant::now();
    let current: SnapshotId = snapshot.id().clone();
    let report = host.apply(prepared, &build, Some(&evidence), &current);
    let activate_ms = begin.elapsed().as_millis() as u64;
    let activated = report.status == ActivationStatus::Activated;
    let outcome = if activated {
        "activated".to_owned()
    } else {
        format!("{:?}", report.reject_reason)
    };

    Sample {
        iteration,
        variant,
        phase,
        analysis_ms,
        analysis_engine_ms: engine_ms,
        build_ms,
        verify_ms,
        load_ms,
        activate_ms,
        total_ms: started.elapsed().as_millis() as u64,
        critical_path_ms,
        sequential_estimate_ms,
        activated,
        outcome,
        cache_kind,
        cached_build,
    }
}

/// Two overlapping stages: the slowest one is the critical path, their sum is what a
/// strictly sequential loop would spend inside them (before the shared tail stages).
fn schedule_effect(analysis_ms: u64, build_ms: u64) -> (u64, u64) {
    (analysis_ms.max(build_ms), analysis_ms + build_ms)
}

fn env_u64(name: &str) -> Option<u64> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse().ok())
}

fn short(value: &str) -> &str {
    &value[..value.len().min(12)]
}

fn rustc_spawn_calibration(rustc: &str, samples: usize) -> Vec<u64> {
    let mut out = Vec::with_capacity(samples);
    for _ in 0..samples {
        let begin = Instant::now();
        let _ = std::process::Command::new(rustc).arg("-vV").output();
        out.push(begin.elapsed().as_millis() as u64);
    }
    out
}

#[derive(Clone)]
struct Sample {
    iteration: u64,
    variant: Variant,
    phase: Phase,
    analysis_ms: u64,
    analysis_engine_ms: u64,
    build_ms: u64,
    verify_ms: u64,
    load_ms: u64,
    activate_ms: u64,
    total_ms: u64,
    /// Wall time of the slowest of the two overlapped stages (analysis vs build).
    critical_path_ms: u64,
    /// Sum of the two stages: what a strictly sequential loop would have spent inside them.
    sequential_estimate_ms: u64,
    activated: bool,
    outcome: String,
    cache_kind: String,
    cached_build: bool,
}

impl Sample {
    fn failed(mut self, outcome: &str, detail: String, started: Instant) -> Self {
        self.outcome = format!("{outcome}: {detail}");
        self.total_ms = started.elapsed().as_millis() as u64;
        self
    }

    fn analysis_failed(
        mut self,
        analysis_ms: u64,
        engine_ms: u64,
        cache_kind: String,
        started: Instant,
    ) -> Self {
        self.analysis_ms = analysis_ms;
        self.analysis_engine_ms = engine_ms;
        self.cache_kind = cache_kind;
        self.outcome = "diagnostics contained errors".to_owned();
        self.total_ms = started.elapsed().as_millis() as u64;
        self
    }

    fn to_json(&self) -> serde_json::Value {
        json!({
            "iteration": self.iteration,
            "variant": self.variant.label(),
            "phase": self.phase.label(),
            "analysis_ms": self.analysis_ms,
            "analysis_engine_ms": self.analysis_engine_ms,
            "build_ms": self.build_ms,
            "verify_ms": self.verify_ms,
            "load_ms": self.load_ms,
            "activate_ms": self.activate_ms,
            "total_ms": self.total_ms,
            "critical_path_ms": self.critical_path_ms,
            "sequential_estimate_ms": self.sequential_estimate_ms,
            "activated": self.activated,
            "outcome": self.outcome,
            "analysis_cache": self.cache_kind,
            "cached_build": self.cached_build,
        })
    }
}

fn scenario_stats(samples: &[Sample], variant: Variant) -> serde_json::Value {
    let mut out = serde_json::Map::new();
    for phase in [Phase::Edit, Phase::Resubmit] {
        let selected: Vec<&Sample> = samples
            .iter()
            .filter(|s| s.variant == variant && s.phase == phase)
            .collect();
        let column =
            |f: &dyn Fn(&Sample) -> u64| -> Vec<u64> { selected.iter().map(|s| f(s)).collect() };
        out.insert(
            phase.label().to_owned(),
            json!({
                "submissions": selected.len(),
                "activated": selected.iter().filter(|s| s.activated).count(),
                "analysis_ms": stats(&column(&|s| s.analysis_ms)),
                "analysis_engine_ms": stats(&column(&|s| s.analysis_engine_ms)),
                "build_ms": stats(&column(&|s| s.build_ms)),
                "verify_ms": stats(&column(&|s| s.verify_ms)),
                "load_ms": stats(&column(&|s| s.load_ms)),
                "activate_ms": stats(&column(&|s| s.activate_ms)),
                "total_ms": stats(&column(&|s| s.total_ms)),
                "critical_path_ms": stats(&column(&|s| s.critical_path_ms)),
                "sequential_estimate_ms": stats(&column(&|s| s.sequential_estimate_ms)),
                "cached_builds": selected.iter().filter(|s| s.cached_build).count(),
                "outcomes": outcome_counts(&selected),
            }),
        );
    }
    serde_json::Value::Object(out)
}

fn outcome_counts(samples: &[&Sample]) -> serde_json::Value {
    let mut counts = serde_json::Map::new();
    for sample in samples {
        let key = if sample.activated {
            "activated".to_owned()
        } else {
            sample
                .outcome
                .split(':')
                .next()
                .unwrap_or("refused")
                .trim()
                .to_owned()
        };
        let entry = counts.entry(key).or_insert_with(|| json!(0));
        if let Some(value) = entry.as_u64() {
            *entry = json!(value + 1);
        }
    }
    serde_json::Value::Object(counts)
}

fn print_summary(payload: &serde_json::Value) {
    println!(
        "iterations={} submissions={} failed_edits={}",
        payload["iterations"], payload["submissions"], payload["failed_edits"]
    );
    for variant in ["cold", "daemon"] {
        for phase in ["edit", "resubmit"] {
            let block = &payload["scenarios"][variant][phase];
            println!(
                "  {variant:<6} {phase:<8} analysis p50={:>4}ms build p50={:>4}ms total p50={:>4}ms p95={:>4}ms activated={} cached_builds={}",
                block["analysis_ms"]["p50"],
                block["build_ms"]["p50"],
                block["total_ms"]["p50"],
                block["total_ms"]["p95"],
                block["activated"],
                block["cached_builds"]
            );
        }
    }
    println!(
        "  daemon cache: hits_memory={} hits_disk={} misses={} rustc_runs={}",
        payload["daemon"]["cache_hits_memory"],
        payload["daemon"]["cache_hits_disk"],
        payload["daemon"]["cache_misses"],
        payload["daemon"]["verifier_runs"]
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_edit_really_changes_the_source() {
        let base = "pub fn compute(x: f64) -> f64 { x * 2.0 }\n";
        let first = edited_content(base, 0);
        let second = edited_content(base, 1);
        assert_ne!(first, base, "the edit must change the source content");
        assert_ne!(first, second, "distinct iterations must be distinct edits");
        assert!(first.contains("pub fn cycle_bench_edit_0"));
        assert!(second.contains("pub fn cycle_bench_edit_1"));
        assert!(
            first.len() > base.len(),
            "the edit adds code, not only whitespace"
        );
    }

    #[test]
    fn a_refused_resubmit_is_counted_by_outcome_not_hidden() {
        let sample = Sample {
            iteration: 0,
            variant: Variant::Daemon,
            phase: Phase::Resubmit,
            analysis_ms: 0,
            analysis_engine_ms: 0,
            build_ms: 1,
            verify_ms: 1,
            load_ms: 1,
            activate_ms: 1,
            total_ms: 8,
            critical_path_ms: 8,
            sequential_estimate_ms: 8,
            activated: false,
            outcome: "Some(AlreadyActive { artifact_id: ArtifactId(\"x\") })".to_owned(),
            cache_kind: "Memory".to_owned(),
            cached_build: true,
        };
        let counts = outcome_counts(&[&sample]);
        assert_eq!(
            counts.as_object().map(|o| o.len()),
            Some(1),
            "one outcome group: {counts}"
        );
    }

    #[test]
    fn overlapping_stages_use_the_slowest_as_critical_path() {
        assert_eq!(schedule_effect(100, 250), (250, 350));
        assert_eq!(schedule_effect(300, 120), (300, 420));
        assert_eq!(schedule_effect(0, 0), (0, 0));
    }

    #[test]
    fn env_probe_parses_only_real_numbers() {
        std::env::set_var("RAVUM_CYCLE_BENCH_TEST_LOAD", "42");
        assert_eq!(env_u64("RAVUM_CYCLE_BENCH_TEST_LOAD"), Some(42));
        assert_eq!(env_u64("RAVUM_CYCLE_BENCH_TEST_ABSENT"), None);
        std::env::remove_var("RAVUM_CYCLE_BENCH_TEST_LOAD");
    }
}
