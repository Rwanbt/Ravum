//! `ravum dsp` — run a measured DSP session and, optionally, swap the processing
//! module while the session is running.
//!
//! Honest scope: the cadence is simulated, there is no audio device, no real-time
//! thread priority, and no hard-RT claim. A level change without crossfade *does*
//! produce a click, and that click is measured and reported instead of hidden.

use crate::commands::{
    flag, flag_u64, has, load_snapshot, pipeline, positional, render_diagnostics,
};
use ravum_dsp_harness::{block_evidence, measure_from, SessionConfig};
use ravum_patch_host::{prepare_block, BlockHost, ExpectedAbi};
use ravum_protocol::DspState;
use serde_json::json;
use std::path::Path;

pub fn dsp(args: &[String]) -> u8 {
    let Some(first) = positional(args, 0) else {
        eprintln!(
            "usage: ravum dsp <file.rs> [--next <file.rs>] [--blocks N] [--frames N] [--rate N] [--period-sleep-us N] [--json]"
        );
        return 2;
    };
    let config = SessionConfig {
        blocks: flag_u64(args, "--blocks", 400),
        frames: flag_u64(args, "--frames", 256) as usize,
        sample_rate: flag_u64(args, "--rate", 48_000) as u32,
        amplitude: 0.5,
        frequency_hz: 440.0,
        start_sample: 0,
        period_sleep_us: flag_u64(args, "--period-sleep-us", 0),
    };
    // Crossfade length for the swap, in frames. `0` keeps the MVP-0a immediate-switch
    // semantics; L4-B measures the click removal with a non-zero value.
    let crossfade_frames = flag_u64(args, "--crossfade-frames", 0).min(u32::MAX as u64) as u32;
    let json_out = has(args, "--json");
    let pipeline = pipeline();

    let Some((host, first_build)) = build_and_bootstrap(&pipeline, &first, crossfade_frames) else {
        return 1;
    };

    let before = measure_from(config, DspState::default(), 0.0, |input, output, state| {
        host.process_rt(input, output, state);
    });

    let mut swaps = 0u64;
    let mut swap_report = serde_json::Value::Null;
    let mut after = None;

    if let Some(next) = flag(args, "--next") {
        let Some(second) = build_candidate(&pipeline, &next) else {
            return 1;
        };
        let evidence = match block_evidence(&second) {
            Ok(e) => e,
            Err(reason) => {
                eprintln!("evidence refused: {reason:?}");
                return 1;
            }
        };
        let prepared = match prepare_block(Path::new(&second.artifact_path), &ExpectedAbi::dsp()) {
            Ok(p) => p,
            Err(reason) => {
                eprintln!("module refused: {reason:?}");
                return 1;
            }
        };
        let report = host.apply(
            prepared,
            &second,
            Some(&evidence),
            &second.source_snapshot_id,
        );
        swap_report = serde_json::to_value(&report).unwrap_or(serde_json::Value::Null);
        if report.status != ravum_protocol::ActivationStatus::Activated {
            eprintln!("activation refused: {:?}", report.reject_reason);
            return 1;
        }
        swaps = 1;
        // Chain the host-owned state and the last output sample so the second
        // session's max delta really covers the swap boundary.
        let continued = SessionConfig {
            start_sample: config.blocks.saturating_mul(config.frames as u64),
            ..config
        };
        after = Some(measure_from(
            continued,
            before.final_state,
            before.last_sample,
            |input, output, state| {
                host.process_rt(input, output, state);
            },
        ));
    }

    let unloaded = host.status().unloaded_modules;
    let report_before = before
        .measurement
        .clone()
        .into_report("session-before", 0, unloaded);
    let report_after = after.as_ref().map(|o| {
        o.measurement
            .clone()
            .into_report("session-after", swaps, unloaded)
    });

    if json_out {
        let payload = json!({
            "schema_version": 1,
            "first_source": first,
            "first_artifact": first_build.artifact_id,
            "config": {
                "blocks": config.blocks,
                "frames": config.frames,
                "sample_rate": config.sample_rate,
                "period_us": config.period_us(),
                "period_sleep_us": config.period_sleep_us,
                "crossfade_frames": crossfade_frames,
            },
            "session_before": report_before,
            "swap": swap_report,
            "session_after": report_after,
            "limits": [
                "simulated cadence: no audio device, no driver, no real-time priority",
                "the allocations and deallocations counters instrument this whole process, per thread; a zero means the instrumented allocator saw no call on that thread, not a hard-RT proof",
                "a level change without crossfade is expected to produce a discontinuity and it is measured",
            ],
        });
        println!("{}", serde_json::to_string_pretty(&payload).unwrap());
    } else {
        print_session("avant remplacement", &report_before);
        println!("  crossfade      {} frames", crossfade_frames);
        if let Some(after) = &report_after {
            println!(
                "remplacement   activé en {} ms (chargement {} ms)",
                swap_report["timings"]["total_ms"], swap_report["timings"]["load_ms"]
            );
            print_session("après remplacement", after);
        }
        println!(
            "note           cadence simulée, aucun périphérique audio, aucune priorité temps réel"
        );
    }

    let ok = report_after
        .as_ref()
        .map(|r| r.allocations_on_audio_thread == 0 && r.deallocations_on_audio_thread == 0)
        .unwrap_or(true);
    if ok {
        0
    } else {
        1
    }
}

fn print_session(label: &str, report: &ravum_protocol::DspSessionReport) {
    println!("--- {label} ---");
    println!(
        "  blocs={} frames={} période={}us",
        report.blocks, report.frames, report.period_us
    );
    println!(
        "  temps/bloc p50={}us p95={}us p99={}us max={}us",
        report.block_timings.p50_us,
        report.block_timings.p95_us,
        report.block_timings.p99_us,
        report.block_timings.max_us
    );
    println!(
        "  dépassements={} allocations_thread_audio={} désallocations_thread_audio={} swap={}",
        report.overruns,
        report.allocations_on_audio_thread,
        report.deallocations_on_audio_thread,
        report.swaps
    );
    println!(
        "  |delta| max={:.4} (référence sinusoïdale={:.4})",
        report.max_sample_delta, report.baseline_sample_delta
    );
}

fn build_and_bootstrap(
    pipeline: &ravum_coordinator::Pipeline,
    file: &str,
    crossfade_frames: u32,
) -> Option<(BlockHost, ravum_protocol::BuildOutput)> {
    let build = build_candidate(pipeline, file)?;
    match BlockHost::bootstrap_with_crossfade(
        ExpectedAbi::dsp(),
        Path::new(&build.artifact_path),
        crossfade_frames,
    ) {
        Ok(host) => Some((host, build)),
        Err(reason) => {
            eprintln!("module refused at bootstrap: {reason:?}");
            None
        }
    }
}

fn build_candidate(
    pipeline: &ravum_coordinator::Pipeline,
    file: &str,
) -> Option<ravum_protocol::BuildOutput> {
    let snapshot = match load_snapshot(file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return None;
        }
    };
    match pipeline.analyze(&snapshot) {
        Ok(check) if check.diagnostics.has_errors() => {
            eprintln!("{}", render_diagnostics(&check.diagnostics));
            eprintln!("refused: analysis reported errors");
            return None;
        }
        Ok(_) => {}
        Err(e) => {
            eprintln!("adapter error: {e}");
            return None;
        }
    }
    match pipeline.build(&snapshot) {
        Ok(outcome) if outcome.success => Some(outcome.build),
        Ok(_) => {
            eprintln!("refused: build failed");
            None
        }
        Err(e) => {
            eprintln!("adapter error: {e}");
            None
        }
    }
}
