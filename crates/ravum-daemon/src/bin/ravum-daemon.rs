//! `ravum-daemon` demonstration binary: a persistent JSON-lines server.
//!
//! The daemon process stays alive between edits; each stdin line is one command
//! and each stdout line is one JSON response. This is the minimal protocol an
//! editor integration needs, and it is exercised by the integration tests
//! (start, analyse, stats, restart with the same work directory, shutdown).
//!
//! ```text
//! {"id":"1","cmd":"analyze","revision":1,"label":"demo","root":"src/lib.rs",
//!  "files":[{"path":"src/lib.rs","content":"pub fn f() {}"}],
//!  "context":{"edition":"2021","crate_name":"demo","crate_type":"lib","flags":[]}}
//! {"id":"2","cmd":"advance_revision","revision":2}
//! {"id":"3","cmd":"stats"}
//! {"id":"4","cmd":"clear_cache"}
//! {"id":"5","cmd":"shutdown"}
//! ```
//!
//! The same lib API (`ravum_daemon::Daemon`) can be used in-process; this
//! binary is a thin transport, not a second implementation.

use ravum_daemon::{
    AnalysisRequest, BuildContext, Daemon, DaemonConfig, Priority, SourceFile,
};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let mut config = DaemonConfig::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--work-dir" => match args.next() {
                Some(value) => config.work_dir = PathBuf::from(value),
                None => usage_exit("--work-dir needs a value"),
            },
            "--max-jobs" => match args.next().and_then(|value| value.parse().ok()) {
                Some(value) => config.max_concurrent_jobs = value,
                None => usage_exit("--max-jobs needs an integer"),
            },
            "--max-cache-entries" => match args.next().and_then(|value| value.parse().ok()) {
                Some(value) => config.max_cache_entries = value,
                None => usage_exit("--max-cache-entries needs an integer"),
            },
            "--analysis-timeout-ms" => match args.next().and_then(|value| value.parse().ok()) {
                Some(value) => config.analysis_timeout_ms = value,
                None => usage_exit("--analysis-timeout-ms needs an integer"),
            },
            "--poll-interval-ms" => match args.next().and_then(|value| value.parse().ok()) {
                Some(value) => config.poll_interval_ms = value,
                None => usage_exit("--poll-interval-ms needs an integer"),
            },
            "--help" | "-h" => usage_exit(""),
            other => usage_exit(&format!("unknown argument {other:?}")),
        }
    }

    let started = Instant::now();
    let daemon = match Daemon::open(config) {
        Ok(daemon) => daemon,
        Err(error) => {
            let line = json!({
                "event": "error",
                "phase": "startup",
                "error": {"kind": error.kind.as_str(), "message": error.message},
            });
            println!("{line}");
            std::process::exit(1);
        }
    };
    emit(&json!({
        "event": "ready",
        "daemon_version": env!("CARGO_PKG_VERSION"),
        "work_dir": daemon.work_dir().display().to_string(),
        "toolchain": daemon.toolchain(),
        "max_concurrent_jobs": daemon.config().max_concurrent_jobs,
        "startup_ms": started.elapsed().as_millis() as u64,
        "protocol": "ravum-daemon-jsonl-v1",
    }));

    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                emit(&json!({"ok": false, "error": {"kind": "io", "message": format!("{error}")}}));
                break;
            }
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value: Value = match serde_json::from_str(trimmed) {
            Ok(value) => value,
            Err(error) => {
                emit(&json!({
                    "ok": false,
                    "error": {"kind": "protocol", "message": format!("invalid JSON: {error}")},
                }));
                continue;
            }
        };
        if handle(&daemon, &value) {
            break;
        }
    }
}

/// Handles one command. Returns `true` when the daemon must shut down.
fn handle(daemon: &Daemon, value: &Value) -> bool {
    let id = value.get("id").cloned().unwrap_or(Value::Null);
    let command = value.get("cmd").and_then(Value::as_str).unwrap_or_default();
    match command {
        "ping" => {
            emit(&json!({"id": id, "ok": true, "pong": true}));
            false
        }
        "analyze" => {
            let request = match parse_request(value) {
                Ok(request) => request,
                Err(message) => {
                    emit(&json!({
                        "id": id,
                        "ok": false,
                        "error": {"kind": "invalid_request", "message": message},
                    }));
                    return false;
                }
            };
            let started = Instant::now();
            match daemon.analyze(&request) {
                Ok(report) => {
                    let wall_ms = started.elapsed().as_millis() as u64;
                    emit(&json!({
                        "id": id,
                        "ok": true,
                        "wall_ms": wall_ms,
                        "report": report.to_json(),
                    }));
                }
                Err(error) => emit(&json!({
                    "id": id,
                    "ok": false,
                    "error": {"kind": error.kind.as_str(), "message": error.message},
                })),
            }
            false
        }
        "advance_revision" => {
            let revision = value
                .get("revision")
                .and_then(Value::as_u64)
                .unwrap_or_else(|| daemon.current_revision());
            let current = daemon.advance_revision(revision);
            emit(&json!({"id": id, "ok": true, "current_revision": current}));
            false
        }
        "stats" => {
            emit(&json!({"id": id, "ok": true, "stats": daemon.stats()}));
            false
        }
        "clear_cache" => match daemon.clear_caches() {
            Ok(()) => {
                emit(&json!({"id": id, "ok": true, "cleared": true}));
                false
            }
            Err(error) => {
                emit(&json!({
                    "id": id,
                    "ok": false,
                    "error": {"kind": error.kind.as_str(), "message": error.message},
                }));
                false
            }
        },
        "command_template" => {
            let request = match parse_request(value) {
                Ok(request) => request,
                Err(message) => {
                    emit(&json!({
                        "id": id,
                        "ok": false,
                        "error": {"kind": "invalid_request", "message": message},
                    }));
                    return false;
                }
            };
            match daemon.command_template(&request) {
                Ok(template) => emit(&json!({"id": id, "ok": true, "command_template": template})),
                Err(error) => emit(&json!({
                    "id": id,
                    "ok": false,
                    "error": {"kind": error.kind.as_str(), "message": error.message},
                })),
            }
            false
        }
        "shutdown" => {
            emit(&json!({"id": id, "ok": true, "shutdown": true}));
            true
        }
        other => {
            emit(&json!({
                "id": id,
                "ok": false,
                "error": {"kind": "protocol", "message": format!("unknown command {other:?}")},
            }));
            false
        }
    }
}

fn parse_request(value: &Value) -> Result<AnalysisRequest, String> {
    let revision = value
        .get("revision")
        .and_then(Value::as_u64)
        .ok_or_else(|| "analyze needs an integer `revision`".to_owned())?;
    let label = value
        .get("label")
        .and_then(Value::as_str)
        .unwrap_or("unit")
        .to_owned();
    let root = value
        .get("root")
        .and_then(Value::as_str)
        .ok_or_else(|| "analyze needs a logical `root` path".to_owned())?
        .to_owned();
    let files = value
        .get("files")
        .and_then(Value::as_array)
        .ok_or_else(|| "analyze needs a non-empty `files` array".to_owned())?
        .iter()
        .map(|file| {
            let path = file
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| "each file needs a string `path`".to_owned())?;
            let content = file
                .get("content")
                .and_then(Value::as_str)
                .ok_or_else(|| "each file needs a string `content`".to_owned())?;
            Ok(SourceFile {
                path: path.to_owned(),
                content: content.to_owned(),
            })
        })
        .collect::<Result<Vec<SourceFile>, String>>()?;
    let context_value = value.get("context").cloned().unwrap_or_else(|| json!({}));
    let context = BuildContext {
        edition: context_value
            .get("edition")
            .and_then(Value::as_str)
            .unwrap_or("2021")
            .to_owned(),
        crate_name: context_value
            .get("crate_name")
            .and_then(Value::as_str)
            .unwrap_or("ravum_unit")
            .to_owned(),
        crate_type: context_value
            .get("crate_type")
            .and_then(Value::as_str)
            .unwrap_or("lib")
            .to_owned(),
        flags: context_value
            .get("flags")
            .and_then(Value::as_array)
            .map(|flags| {
                flags
                    .iter()
                    .filter_map(|flag| flag.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
    };
    let priority = match value.get("priority").and_then(Value::as_str) {
        Some("interactive") => Priority::Interactive,
        Some("background") => Priority::Background,
        _ => Priority::Normal,
    };
    Ok(AnalysisRequest {
        revision,
        label,
        root,
        files,
        context,
        priority,
    })
}

fn emit(value: &Value) {
    let mut stdout = std::io::stdout();
    let text = serde_json::to_string(value).unwrap_or_else(|_| {
        "{\"ok\":false,\"error\":{\"kind\":\"internal\",\"message\":\"cannot serialize response\"}}"
            .to_owned()
    });
    let _ = writeln!(stdout, "{text}");
    let _ = stdout.flush();
}

fn usage_exit(message: &str) -> ! {
    if !message.is_empty() {
        eprintln!("ravum-daemon: {message}");
    }
    eprintln!(
        "usage: ravum-daemon [--work-dir PATH] [--max-jobs N] [--max-cache-entries N] \
         [--analysis-timeout-ms N] [--poll-interval-ms N]"
    );
    std::process::exit(2);
}
