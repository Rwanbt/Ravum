//! Long-running demo application whose patchable function is hot-swapped.
//!
//! The process never restarts: it computes on a ticker thread and accepts patch
//! commands on stdin. Every activation is validated by `ravum-patch-host`.
//!
//! Line protocol on stdout (one JSON object per line):
//! * `{"event":"ready","pid":N,"status":{...}}`
//! * `{"event":"tick","tick":N,"version":"vK","value":<f64>}`
//! * `{"event":"apply_result", ...ActivationReport}`
//! * `{"event":"status", ...HostStatus}`
//! * `{"event":"error","message":"..."}`
//!
//! Commands on stdin:
//! * `{"cmd":"status"}`
//! * `{"cmd":"apply","artifact_path":"...","expected_snapshot":"...","build":{...},"evidence":{...}}`
//! * `{"cmd":"quit"}`

use ravum_patch_host::{prepare, ExpectedAbi, PatchHost};
use ravum_protocol::{
    ActivationReport, ActivationStatus, BuildOutput, EvidenceRecord, RejectReason, SnapshotId,
};
use serde::Deserialize;
use serde_json::json;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum Command {
    Status,
    Apply {
        artifact_path: String,
        expected_snapshot: SnapshotId,
        build: BuildOutput,
        #[serde(default)]
        evidence: Option<EvidenceRecord>,
    },
    Quit,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(patch_path) = arg_value(&args, "--patch") else {
        eprintln!("usage: ravum-demo-host --patch <artifact> [--tick-ms <n>]");
        std::process::exit(2);
    };
    let tick_ms: u64 = arg_value(&args, "--tick-ms")
        .and_then(|v| v.parse().ok())
        .unwrap_or(100);

    let host = match PatchHost::bootstrap(ExpectedAbi::default(), &PathBuf::from(&patch_path)) {
        Ok(h) => Arc::new(h),
        Err(reason) => {
            emit(&json!({"event": "error", "message": "bootstrap refused", "reason": reason}));
            std::process::exit(3);
        }
    };

    emit(&json!({"event": "ready", "pid": std::process::id(), "status": host.status()}));
    let running = Arc::new(AtomicBool::new(true));
    spawn_ticker(Arc::clone(&host), Arc::clone(&running), tick_ms);

    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Command>(&line) {
            Ok(Command::Quit) => break,
            Ok(Command::Status) => emit(&json!({"event": "status", "status": host.status()})),
            Ok(Command::Apply {
                artifact_path,
                expected_snapshot,
                build,
                evidence,
            }) => {
                let report = apply(&host, &artifact_path, &expected_snapshot, &build, evidence);
                emit(&json!({"event": "apply_result", "report": report}));
            }
            Err(e) => emit(&json!({"event": "error", "message": format!("bad command: {e}")})),
        }
    }
    running.store(false, Ordering::SeqCst);
}

fn apply(
    host: &PatchHost,
    artifact_path: &str,
    expected_snapshot: &SnapshotId,
    build: &BuildOutput,
    evidence: Option<EvidenceRecord>,
) -> ActivationReport {
    let prepared = match prepare(&PathBuf::from(artifact_path), host.expected_abi()) {
        Ok(p) => p,
        Err(reason) => return rejected(reason),
    };
    if prepared.module.artifact_id != build.artifact_id {
        return rejected(RejectReason::ArtifactMismatch {
            expected: build.artifact_id.clone(),
            found: prepared.module.artifact_id.clone(),
        });
    }
    host.apply(prepared, build, evidence.as_ref(), expected_snapshot)
}

fn rejected(reason: RejectReason) -> ActivationReport {
    let mut report = ActivationReport::rejected(reason);
    report.status = ActivationStatus::Rejected;
    report
}

fn spawn_ticker(host: Arc<PatchHost>, running: Arc<AtomicBool>, tick_ms: u64) {
    std::thread::Builder::new()
        .name("ravum-demo-ticker".to_owned())
        .spawn(move || {
            let mut tick: u64 = 0;
            while running.load(Ordering::SeqCst) {
                tick += 1;
                let value = host.call(tick as f64);
                emit(&json!({
                    "event": "tick",
                    "tick": tick,
                    "version": host.active_version().to_string(),
                    "value": value,
                }));
                std::thread::sleep(std::time::Duration::from_millis(tick_ms));
            }
        })
        .expect("spawn ticker");
}

fn emit(value: &serde_json::Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{value}");
    let _ = out.flush();
}

fn arg_value(args: &[String], key: &str) -> Option<String> {
    args.iter().position(|a| a == key).and_then(|i| args.get(i + 1)).cloned()
}
