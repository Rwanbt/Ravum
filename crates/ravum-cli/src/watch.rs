//! `ravum watch` — the real continuous loop: edit → check → build → verify → hot activate.

use crate::commands::{
    flag_u64, has, load_snapshot, pipeline, positional, render_diagnostics, state_path, work_dir,
};
use crate::hostproc::HostProc;
use ravum_coordinator::{merge_activation, LoopOutcome, LoopReport, WorkspaceSession};
use ravum_protocol::{ActivationReport, SnapshotId};
use serde_json::json;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub fn watch(args: &[String]) -> u8 {
    let Some(file) = positional(args, 0) else {
        eprintln!("usage: ravum watch <file.rs> [--tick-ms N] [--interval-ms N] [--max-activations N] [--duration-ms N] [--concurrent] [--json]");
        return 2;
    };
    let tick_ms = flag_u64(args, "--tick-ms", 100);
    let interval = Duration::from_millis(flag_u64(args, "--interval-ms", 120).max(20));
    let max_activations = flag_u64(args, "--max-activations", u64::MAX);
    let duration_ms = flag_u64(args, "--duration-ms", 0);
    let post_activation_ms = flag_u64(args, "--post-activation-ms", 600);
    // Two-track scheduling: analysis and build overlap; activation still requires a clean
    // analysis AND a verified build (see `prepare_candidate_concurrent`).
    let concurrent = has(args, "--concurrent");
    let json_out = has(args, "--json");

    let pipeline = pipeline();
    let initial = match load_snapshot(&file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    let session = Arc::new(WorkspaceSession::new(initial.id().clone()));
    let stop = Arc::new(AtomicBool::new(false));
    spawn_file_watcher(file.clone(), Arc::clone(&session), Arc::clone(&stop), interval);

    let first = if concurrent {
        pipeline.prepare_candidate_concurrent(&initial, &|| session.current())
    } else {
        pipeline.prepare_candidate(&initial, &|| session.current())
    };
    if !matches!(first.outcome, LoopOutcome::CandidateReady) {
        eprintln!("initial build refused: {:?}", first.outcome);
        if let Some(check) = &first.check {
            eprintln!("{}", render_diagnostics(&check.diagnostics));
        }
        stop.store(true, Ordering::SeqCst);
        return 1;
    }
    let Some(artifact) = first.artifact_path() else {
        eprintln!("internal error: no artifact");
        stop.store(true, Ordering::SeqCst);
        return 2;
    };
    let mut host = match HostProc::spawn(Path::new(artifact), tick_ms) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("cannot spawn demo host: {e}");
            stop.store(true, Ordering::SeqCst);
            return 2;
        }
    };
    if host.wait_for("ready", Duration::from_secs(20)).is_none() {
        eprintln!("demo host did not become ready");
        stop.store(true, Ordering::SeqCst);
        return 2;
    }
    let pid = host.pid();
    eprintln!(
        "watching {file}  host_pid={pid}  initial={}  tick={tick_ms}ms",
        initial.id().short()
    );

    let started = Instant::now();
    let mut last_attempted: Option<SnapshotId> = Some(initial.id().clone());
    let mut activations = 0u64;
    let mut refusals = 0u64;
    let mut stale = 0u64;

    loop {
        if duration_ms > 0 && started.elapsed() >= Duration::from_millis(duration_ms) {
            break;
        }
        if !host.is_running() {
            eprintln!("demo host exited");
            break;
        }
        if let Some(msg) = host.recv_timeout(Duration::from_millis(5)) {
            if msg.get("event").and_then(|v| v.as_str()) == Some("tick") {
                print_tick(&msg, json_out);
            }
            continue;
        }

        let current = session.current();
        if last_attempted.as_ref() == Some(&current) {
            std::thread::sleep(interval.min(Duration::from_millis(20)));
            continue;
        }

        let snapshot = match load_snapshot(&file) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("read error: {e}");
                std::thread::sleep(interval);
                continue;
            }
        };
        // The file may have changed again between the poll and the read.
        session.supersede(snapshot.id().clone());
        last_attempted = Some(snapshot.id().clone());

        let mut report = if concurrent {
            pipeline.prepare_candidate_concurrent(&snapshot, &|| session.current())
        } else {
            pipeline.prepare_candidate(&snapshot, &|| session.current())
        };
        if matches!(report.outcome, LoopOutcome::CandidateReady) {
            match publish(&mut host, &mut report, &session) {
                Publish::Activated => activations += 1,
                Publish::Refused => refusals += 1,
                Publish::Stale => stale += 1,
                Publish::IoError(detail) => {
                    eprintln!("ipc error: {detail}");
                    break;
                }
            }
        } else if matches!(report.outcome, LoopOutcome::RefusedAsStale { .. }) {
            stale += 1;
        } else {
            refusals += 1;
        }

        record(&report, pid, activations, refusals, stale, json_out);
        if report.activated() {
            if let Some(active) = report.artifact_path() {
                match ravum_coordinator::prune_artifacts(
                    &work_dir().join("artifacts"),
                    8,
                    Some(Path::new(active)),
                ) {
                    Ok(retention) if !retention.removed.is_empty() => eprintln!(
                        "retention: {} old artifact(s) removed, active kept",
                        retention.removed.len()
                    ),
                    Ok(_) => {}
                    Err(e) => eprintln!("retention skipped: {e}"),
                }
            }
        }
        if activations >= max_activations {
            // Keep the demonstration honest and legible: show a few ticks produced
            // by the newly activated version before stopping.
            drain_ticks(&host, post_activation_ms, json_out);
            break;
        }
    }

    stop.store(true, Ordering::SeqCst);
    host.shutdown();
    eprintln!(
        "stopped: activations={activations} refusals={refusals} stale={stale} elapsed={}ms",
        started.elapsed().as_millis()
    );
    if activations == 0 {
        1
    } else {
        0
    }
}

enum Publish {
    Activated,
    Refused,
    Stale,
    IoError(String),
}

fn print_tick(msg: &serde_json::Value, json_out: bool) {
    if json_out {
        println!("{msg}");
    } else {
        println!(
            "  {} tick={} -> {}",
            msg["version"].as_str().unwrap_or("?"),
            msg["tick"],
            msg["value"]
        );
    }
}

fn drain_ticks(host: &HostProc, duration_ms: u64, json_out: bool) {
    let deadline = Instant::now() + Duration::from_millis(duration_ms);
    while Instant::now() < deadline {
        let Some(msg) = host.recv_timeout(Duration::from_millis(25)) else {
            continue;
        };
        if msg.get("event").and_then(|v| v.as_str()) == Some("tick") {
            print_tick(&msg, json_out);
        }
    }
}

fn publish(host: &mut HostProc, report: &mut LoopReport, session: &WorkspaceSession) -> Publish {
    let (Some(build), Some(evidence)) = (report.build.clone(), report.evidence.clone()) else {
        return Publish::Refused;
    };
    let command = json!({
        "cmd": "apply",
        "artifact_path": build.artifact_path,
        "expected_snapshot": session.current(),
        "build": build,
        "evidence": evidence,
    });
    if let Err(e) = host.send(&command) {
        return Publish::IoError(e.to_string());
    }
    let Some(msg) = host.wait_for("apply_result", Duration::from_secs(30)) else {
        return Publish::IoError("no apply_result from demo host".to_owned());
    };
    let Ok(activation) = serde_json::from_value::<ActivationReport>(msg["report"].clone()) else {
        return Publish::IoError("unparsable apply_result".to_owned());
    };
    let refused_stale = matches!(
        activation.reject_reason,
        Some(ravum_protocol::RejectReason::StaleBuild { .. })
    );
    merge_activation(report, activation);
    if report.activated() {
        Publish::Activated
    } else if refused_stale {
        Publish::Stale
    } else {
        Publish::Refused
    }
}

fn record(report: &LoopReport, pid: u32, activations: u64, refusals: u64, stale: u64, json_out: bool) {
    // Staleness is only meaningful through the session verdict; keep the report honest.
    let delivery_note = if matches!(report.outcome, LoopOutcome::RefusedAsStale { .. }) {
        "stale"
    } else {
        "fresh"
    };
    let entry = json!({
        "schema_version": 1,
        "event": "loop",
        "at_unix": ravum_protocol::now_unix(),
        "host_pid": pid,
        "outcome": report.outcome.label(),
        "detail": format!("{:?}", report.outcome),
        "freshness": delivery_note,
        "snapshot_id": report.snapshot_id,
        "artifact_id": report.artifact_id(),
        "activation": report.activation,
        "timings": {
            "check_ms": report.timings.check_ms,
            "build_ms": report.timings.build_ms,
            "verify_ms": report.timings.verify_ms,
            "load_ms": report.timings.load_ms,
            "activate_ms": report.timings.activate_ms,
            "total_ms": report.timings.total_ms,
        },
        "counters": {"activations": activations, "refusals": refusals, "stale": stale},
        "diagnostics": report.check.as_ref().map(|c| {
            json!({
                "errors": c.diagnostics.error_count(),
                "warnings": c.diagnostics.warning_count(),
            })
        }),
    });
    append_jsonl(&work_dir().join("m1-watch.jsonl"), &entry);
    write_state(&entry);
    if json_out {
        println!("{entry}");
    } else {
        println!(
            "loop {} snapshot={} total={}ms (check={} build={} verify={} load={} activate={})",
            report.outcome.label(),
            report.snapshot_id.short(),
            report.timings.total_ms,
            report.timings.check_ms,
            report.timings.build_ms,
            report.timings.verify_ms,
            report.timings.load_ms,
            report.timings.activate_ms
        );
        if let Some(check) = &report.check {
            if check.diagnostics.has_errors() {
                println!("{}", render_diagnostics(&check.diagnostics));
            }
        }
        if let Some(reason) = report.activation.as_ref().and_then(|a| a.reject_reason.clone()) {
            println!("  refused by host: {reason:?}");
        }
    }
}

fn write_state(entry: &serde_json::Value) {
    let path = state_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, serde_json::to_string_pretty(entry).unwrap_or_default());
}

fn append_jsonl(path: &Path, entry: &serde_json::Value) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        use std::io::Write;
        let _ = writeln!(file, "{entry}");
    }
}

/// Poll the edited file and supersede the session as soon as its content changes.
/// Keeping this on its own thread is what makes mid-build staleness observable.
fn spawn_file_watcher(
    file: String,
    session: Arc<WorkspaceSession>,
    stop: Arc<AtomicBool>,
    interval: Duration,
) {
    std::thread::Builder::new()
        .name("ravum-file-watcher".to_owned())
        .spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                std::thread::sleep(interval);
                if let Ok(snapshot) = ravum_snapshot::SourceSnapshot::from_file(Path::new(&file)) {
                    if snapshot.id() != &session.current() {
                        session.supersede(snapshot.id().clone());
                    }
                }
            }
        })
        .expect("spawn file watcher");
}
