//! `ravum bench` — measure every stage of the loop separately, honestly labelled.

use crate::commands::{flag, flag_u64, load_snapshot, pipeline, positional, work_dir};use crate::metrics::{env_info, stats, working_set_bytes};
use ravum_coordinator::{LoopOutcome, LoopReport, Pipeline};
use ravum_patch_host::{ExpectedAbi, PatchHost};
use ravum_snapshot::SourceSnapshot;
use serde_json::json;
use std::path::Path;

pub fn bench(args: &[String]) -> u8 {
    let (Some(file_a), Some(file_b)) = (positional(args, 0), positional(args, 1)) else {
        eprintln!("usage: ravum bench <file-a.rs> <file-b.rs> [--iterations N] [--out PATH]");
        return 2;
    };
    let iterations = flag_u64(args, "--iterations", 20).max(2);
    let out_path = flag(args, "--out")
        .unwrap_or_else(|| work_dir().join("bench.json").to_string_lossy().to_string());

    let (Ok(source_a), Ok(source_b)) = (load_snapshot(&file_a), load_snapshot(&file_b)) else {
        eprintln!("cannot read both benchmark sources");
        return 2;
    };
    let pipeline = pipeline().without_artifact_cache();
    let rustc = pipeline
        .adapter()
        .toolchain()
        .map(|t| t.release)
        .unwrap_or_else(|_| "unknown".to_owned());

    let Some(host) = bootstrap(&pipeline, &source_a) else {
        return 1;
    };
    let memory_before = working_set_bytes();

    let mut samples: Vec<Sample> = Vec::with_capacity(iterations as usize);
    for i in 0..iterations {
        let source = if i % 2 == 0 { &source_b } else { &source_a };
        let current = source.id().clone();
        let report = pipeline.run_once(&host, source, &|| current.clone());
        samples.push(Sample::from_report(i, &report));
    }

    let memory_after = working_set_bytes();
    // Measured after the loop: representative of the conditions the loop ran in.
    let calibration = rustc_spawn_calibration(&pipeline, 5);
    let activated = samples.iter().filter(|s| s.activated).count() as u64;
    let refused = iterations - activated;
    let payload = json!({
        "schema_version": 1,
        "tool": "ravum bench",
        "tool_version": env!("CARGO_PKG_VERSION"),
        "rustc": rustc,
        "env": env_info(),
        "machine": {
            "cpu": std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_else(|_| "unknown".to_owned()),
            "note": "16 Go RAM reference machine; concurrent cargo builds were avoided",
        },
        "iterations": iterations,
        "activated": activated,
        "refused": refused,
        "stats": {
            "check_ms": stats(&samples.iter().map(|s| s.check_ms).collect::<Vec<_>>()),
            "build_ms": stats(&samples.iter().map(|s| s.build_ms).collect::<Vec<_>>()),
            "verify_ms": stats(&samples.iter().map(|s| s.verify_ms).collect::<Vec<_>>()),
            "load_ms": stats(&samples.iter().map(|s| s.load_ms).collect::<Vec<_>>()),
            "activate_ms": stats(&samples.iter().map(|s| s.activate_ms).collect::<Vec<_>>()),
            "total_ms": stats(&samples.iter().map(|s| s.total_ms).collect::<Vec<_>>()),
        },
        "memory": {
            "working_set_bytes_before": memory_before,
            "working_set_bytes_after": memory_after,
            "delta_bytes": match (memory_before, memory_after) {
                (Some(a), Some(b)) => Some(b.saturating_sub(a) as i64),
                _ => None,
            },
        },
        "calibration": {
            "rustc_spawn_ms": stats(&calibration),
            "note": "median wall time of a bare `rustc -vV` spawn, measured in the same run: it characterises machine load, not rustc's analysis cost"
        },
        "artifact_cache": "disabled for this run: every build is a real rustc invocation",
        "samples": samples.iter().map(|s| s.to_json()).collect::<Vec<_>>(),
        "limits": [
            "caller-side and OBSERVED numbers on the machine described in env/machine; not portable guarantees",
            "rustc performs codegen and link in one process: build_ms covers both",
            "p99 over a small sample is the maximum of that sample, not a stable tail estimate",
        ],
    });

    if let Some(parent) = Path::new(&out_path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(&out_path, serde_json::to_string_pretty(&payload).unwrap_or_default()) {
        Ok(()) => eprintln!("written: {out_path}"),
        Err(e) => {
            eprintln!("cannot write {out_path}: {e}");
            return 2;
        }
    }

    println!(
        "iterations={iterations} activated={activated} refused={refused} rustc={rustc}"
    );
    for stage in ["check", "build", "verify", "load", "activate", "total"] {
        let s = &payload["stats"][format!("{stage}_ms")];
        println!(
            "  {stage:<9} min={:>5}ms p50={:>5}ms p95={:>5}ms p99={:>5}ms max={:>5}ms",
            s["min"], s["p50"], s["p95"], s["p99"], s["max"]
        );
    }
    if let (Some(before), Some(after)) = (memory_before, memory_after) {
        println!("  memory    before={}KiB after={}KiB", before / 1024, after / 1024);
    }
    if !calibration.is_empty() {
        let mut sorted = calibration.clone();
        sorted.sort_unstable();
        println!(
            "  calibration rustc spawn: p50={}ms (machine load indicator)",
            sorted[sorted.len() / 2]
        );
    }
    if refused == 0 {
        0
    } else {
        1
    }
}

/// Machine-load calibration: how long a bare `rustc` process takes to start and
/// answer. This is the floor any "check" measurement is built on.
fn rustc_spawn_calibration(_pipeline: &Pipeline, samples: usize) -> Vec<u64> {
    let rustc = std::env::var("RAVUM_RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let mut out = Vec::with_capacity(samples);
    for _ in 0..samples {
        let started = std::time::Instant::now();
        let _ = std::process::Command::new(&rustc).arg("-vV").output();
        out.push(started.elapsed().as_millis() as u64);
    }
    out
}

fn bootstrap(pipeline: &Pipeline, source: &SourceSnapshot) -> Option<PatchHost> {    let warm = pipeline.prepare_candidate(source, &|| source.id().clone());
    if !matches!(warm.outcome, LoopOutcome::CandidateReady) {
        eprintln!("bootstrap refused: {:?}", warm.outcome);
        return None;
    }
    let artifact = warm.artifact_path()?;
    match PatchHost::bootstrap(ExpectedAbi::default(), Path::new(artifact)) {
        Ok(host) => Some(host),
        Err(reason) => {
            eprintln!("bootstrap refused by host: {reason:?}");
            None
        }
    }
}

struct Sample {
    iteration: u64,
    activated: bool,
    outcome: String,
    check_ms: u64,
    build_ms: u64,
    verify_ms: u64,
    load_ms: u64,
    activate_ms: u64,
    total_ms: u64,
}

impl Sample {
    fn from_report(iteration: u64, report: &LoopReport) -> Self {
        Self {
            iteration,
            activated: report.activated(),
            outcome: report.outcome.label().to_owned(),
            check_ms: report.timings.check_ms,
            build_ms: report.timings.build_ms,
            verify_ms: report.timings.verify_ms,
            load_ms: report.timings.load_ms,
            activate_ms: report.timings.activate_ms,
            total_ms: report.timings.total_ms,
        }
    }

    fn to_json(&self) -> serde_json::Value {
        json!({
            "iteration": self.iteration,
            "outcome": self.outcome,
            "activated": self.activated,
            "check_ms": self.check_ms,
            "build_ms": self.build_ms,
            "verify_ms": self.verify_ms,
            "load_ms": self.load_ms,
            "activate_ms": self.activate_ms,
            "total_ms": self.total_ms,
        })
    }
}
