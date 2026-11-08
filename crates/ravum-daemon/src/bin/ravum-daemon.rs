//! `ravum-daemon` binary: a persistent JSON-lines server plus a one-shot GC mode.
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
//! {"id":"4","cmd":"gc","policy":{"max_cache_bytes":65536,"max_snapshot_bytes":131072,"keep_recent":4}}
//! {"id":"5","cmd":"clear_cache"}
//! {"id":"6","cmd":"shutdown"}
//! ```
//!
//! One-shot maintenance (no daemon running on the same work directory):
//!
//! ```text
//! ravum-daemon --gc --work-dir target/ravum-daemon \
//!   --policy '{"max_cache_bytes":1048576,"keep_recent":4,"dry_run":true}'
//! ravum-daemon --gc --work-dir target/ravum-daemon --max-cache-bytes 1048576 --keep-recent 4
//! ```
//!
//! Exit codes: `0` GC/report success, `1` invalid policy or error, `2` usage.
//!
//! The same lib API (`ravum_daemon::Daemon`) can be used in-process; this
//! binary is a thin transport, not a second implementation.

use ravum_daemon::{
    AnalysisRequest, BuildContext, Daemon, DaemonConfig, GcMode, GcPolicy, GcProtection,
    GcReport, Priority, RetentionState, SourceFile,
};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Per-field CLI overrides applied on top of `--policy`.
#[derive(Default)]
struct PolicyFlags {
    max_cache_bytes: Option<u64>,
    max_snapshot_bytes: Option<u64>,
    max_incremental_bytes: Option<u64>,
    max_total_bytes: Option<u64>,
    keep_recent: Option<usize>,
    dry_run: Option<bool>,
}

fn main() {
    let mut config = DaemonConfig::default();
    let mut gc_mode = false;
    let mut policy_json: Option<String> = None;
    let mut overrides = PolicyFlags::default();
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
            // --- automatic background GC (opt-in, disabled by default) ---
            "--auto-gc" => config.auto_gc.enabled = true,
            "--auto-gc-check-every" => match args.next().and_then(|value| value.parse().ok()) {
                Some(value) => config.auto_gc.check_every_requests = value,
                None => usage_exit("--auto-gc-check-every needs an integer >= 1"),
            },
            "--auto-gc-min-total-bytes" => match args.next().and_then(|value| value.parse().ok()) {
                Some(value) => config.auto_gc.min_total_bytes = Some(value),
                None => usage_exit("--auto-gc-min-total-bytes needs an integer"),
            },
            "--auto-gc-max-total-bytes" => match args.next().and_then(|value| value.parse().ok()) {
                Some(value) => config.auto_gc.max_total_bytes = Some(value),
                None => usage_exit("--auto-gc-max-total-bytes needs an integer"),
            },
            "--auto-gc-keep-recent" => match args.next().and_then(|value| value.parse().ok()) {
                Some(value) => config.auto_gc.keep_recent = value,
                None => usage_exit("--auto-gc-keep-recent needs an integer >= 1"),
            },
            "--auto-gc-idle-only" => config.auto_gc.idle_only = true,
            "--auto-gc-max-cache-bytes" => {
                match args.next().and_then(|value| value.parse().ok()) {
                    Some(value) => config.auto_gc.max_cache_bytes = Some(value),
                    None => usage_exit("--auto-gc-max-cache-bytes needs an integer"),
                }
            }
            "--auto-gc-max-snapshot-bytes" => {
                match args.next().and_then(|value| value.parse().ok()) {
                    Some(value) => config.auto_gc.max_snapshot_bytes = Some(value),
                    None => usage_exit("--auto-gc-max-snapshot-bytes needs an integer"),
                }
            }
            "--auto-gc-max-incremental-bytes" => {
                match args.next().and_then(|value| value.parse().ok()) {
                    Some(value) => config.auto_gc.max_incremental_bytes = Some(value),
                    None => usage_exit("--auto-gc-max-incremental-bytes needs an integer"),
                }
            }
            // --- one-shot GC options ---
            "--gc" => gc_mode = true,
            "--policy" => match args.next() {
                Some(value) => policy_json = Some(value),
                None => usage_exit("--policy needs inline JSON or a path to a JSON file"),
            },
            "--max-cache-bytes" => match args.next().and_then(|value| value.parse().ok()) {
                Some(value) => overrides.max_cache_bytes = Some(value),
                None => usage_exit("--max-cache-bytes needs an integer"),
            },
            "--max-snapshot-bytes" => match args.next().and_then(|value| value.parse().ok()) {
                Some(value) => overrides.max_snapshot_bytes = Some(value),
                None => usage_exit("--max-snapshot-bytes needs an integer"),
            },
            "--max-incremental-bytes" => match args.next().and_then(|value| value.parse().ok()) {
                Some(value) => overrides.max_incremental_bytes = Some(value),
                None => usage_exit("--max-incremental-bytes needs an integer"),
            },
            "--max-total-bytes" => match args.next().and_then(|value| value.parse().ok()) {
                Some(value) => overrides.max_total_bytes = Some(value),
                None => usage_exit("--max-total-bytes needs an integer"),
            },
            "--keep-recent" => match args.next().and_then(|value| value.parse().ok()) {
                Some(value) => overrides.keep_recent = Some(value),
                None => usage_exit("--keep-recent needs an integer >= 1"),
            },
            "--dry-run" => overrides.dry_run = Some(true),
            "--help" | "-h" => print_help_exit_ok(),
            other => usage_exit(&format!("unknown argument {other:?}")),
        }
    }

    if gc_mode {
        run_one_shot_gc(&config.work_dir, policy_json.as_deref(), &overrides);
    }
    if policy_json.is_some() || !policy_flags_empty(&overrides) {
        usage_exit("--policy/--keep-recent/--max-*-bytes are only valid with --gc");
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
    // Clean shutdown: stop the automatic-GC maintenance thread and let a pending
    // collection finish before the process exits (bounded wait).
    daemon.shutdown_auto_gc();
    let _ = daemon.wait_auto_gc_idle(5000);
}

fn policy_flags_empty(overrides: &PolicyFlags) -> bool {
    overrides.max_cache_bytes.is_none()
        && overrides.max_snapshot_bytes.is_none()
        && overrides.max_incremental_bytes.is_none()
        && overrides.max_total_bytes.is_none()
        && overrides.keep_recent.is_none()
        && overrides.dry_run.is_none()
}

/// Builds the effective policy: `--policy` (inline JSON or file), then explicit
/// per-field flags override it. Errors are reported with exit code 1.
fn effective_policy(policy_json: Option<&str>, overrides: &PolicyFlags) -> Result<GcPolicy, String> {
    let mut policy = match policy_json {
        Some(raw) => {
            let text = if raw.trim_start().starts_with('{') {
                raw.to_owned()
            } else {
                std::fs::read_to_string(raw).map_err(|error| {
                    format!("cannot read policy file {raw:?}: {error}")
                })?
            };
            serde_json::from_str::<GcPolicy>(&text)
                .map_err(|error| format!("invalid --policy JSON: {error}"))?
        }
        None => GcPolicy::default(),
    };
    if let Some(value) = overrides.max_cache_bytes {
        policy.max_cache_bytes = Some(value);
    }
    if let Some(value) = overrides.max_snapshot_bytes {
        policy.max_snapshot_bytes = Some(value);
    }
    if let Some(value) = overrides.max_incremental_bytes {
        policy.max_incremental_bytes = Some(value);
    }
    if let Some(value) = overrides.max_total_bytes {
        policy.max_total_bytes = Some(value);
    }
    if let Some(value) = overrides.keep_recent {
        policy.keep_recent = value;
    }
    if let Some(value) = overrides.dry_run {
        policy.dry_run = value;
    }
    policy
        .validate()
        .map_err(|error| format!("invalid gc policy: {error}"))?;
    Ok(policy)
}

/// One-shot GC: no `rustc` query, no daemon lock. Recency hints persisted by the
/// last daemon run (`retention.json`) are loaded so live content is protected.
fn run_one_shot_gc(work_dir: &Path, policy_json: Option<&str>, overrides: &PolicyFlags) -> ! {
    let policy = match effective_policy(policy_json, overrides) {
        Ok(policy) => policy,
        Err(message) => {
            let line = json!({
                "ok": false,
                "event": "gc_error",
                "error": {"kind": "invalid_policy", "message": message},
            });
            eprintln!("ravum-daemon: {message}");
            println!("{line}");
            std::process::exit(1);
        }
    };
    let retention = RetentionState::load(work_dir);
    let protection = GcProtection::from_retention(&retention, retention.last_revision);
    match ravum_daemon::gc::run_gc(work_dir, &policy, &protection, GcMode::OneShot) {
        Ok(report) => {
            emit_gc_report(&report);
            eprintln!(
                "gc: mode=one_shot dry_run={} freed={}B cache_entries={}->{} snapshots={}->{} contexts={}->{} duration={}ms",
                report.dry_run,
                report.freed_bytes,
                report.before.cache_entries,
                report.after.cache_entries,
                report.before.snapshots,
                report.after.snapshots,
                report.before.incremental_contexts,
                report.after.incremental_contexts,
                report.duration_ms
            );
            std::process::exit(0);
        }
        Err(error) => {
            let line = json!({
                "ok": false,
                "event": "gc_error",
                "error": {"kind": error.kind.as_str(), "message": error.message},
            });
            eprintln!("ravum-daemon: gc failed: {error}");
            println!("{line}");
            std::process::exit(1);
        }
    }
}

fn emit_gc_report(report: &GcReport) {
    let mut stdout = std::io::stdout();
    let text = serde_json::to_string(report).unwrap_or_else(|error| {
        format!(
            "{{\"ok\":false,\"event\":\"gc_error\",\"error\":{{\"kind\":\"internal\",\"message\":\"cannot serialize GC report: {error}\"}}}}"
        )
    });
    let _ = writeln!(stdout, "{text}");
    let _ = stdout.flush();
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
        "gc" => {
            let policy = match value.get("policy") {
                Some(policy_value) => match serde_json::from_value::<GcPolicy>(policy_value.clone()) {
                    Ok(policy) => policy,
                    Err(error) => {
                        emit(&json!({
                            "id": id,
                            "ok": false,
                            "error": {"kind": "invalid_request", "message": format!("invalid gc policy: {error}")},
                        }));
                        return false;
                    }
                },
                None => GcPolicy::default(),
            };
            match daemon.gc(&policy) {
                Ok(report) => emit(&json!({
                    "id": id,
                    "ok": true,
                    "report": report,
                })),
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

fn print_help_exit_ok() -> ! {
    println!(
        "usage: ravum-daemon [server options]\n\
         \n\
         Server (persistent JSON-lines daemon on stdin/stdout):\n\
         \x20 --work-dir PATH               private state directory (default target/ravum-daemon)\n\
         \x20 --max-jobs N                  max concurrent rustc processes (default 1)\n\
         \x20 --max-cache-entries N         in-memory verdict cache size (default 256)\n\
         \x20 --analysis-timeout-ms N       hard cap for one compiler run (default 30000)\n\
         \x20 --poll-interval-ms N          cancellation polling interval (default 2)\n\
         \n\
         Automatic background GC (opt-in; one maintenance thread, never on the analysis path):\n\
         \x20 --auto-gc                     enable automatic retention GC\n\
         \x20 --auto-gc-check-every N       evaluate the threshold every N analyses (default 20)\n\
         \x20 --auto-gc-min-total-bytes N   trigger when the collected state reaches N bytes\n\
         \x20 --auto-gc-max-total-bytes N   target cap passed to the bounded GC (< trigger)\n\
         \x20 --auto-gc-keep-recent N       recent served contents never deleted (default 8)\n\
         \x20 --auto-gc-idle-only           collect only while no analysis is in flight\n\
         \x20 --auto-gc-max-cache-bytes N / --auto-gc-max-snapshot-bytes N /\n\
         \x20 --auto-gc-max-incremental-bytes N   category caps for the automatic pass\n\
         \n\
         One-shot retention GC (explicit; exits after writing one JSON report):\n\
         \x20 --gc                          run one GC pass instead of starting the server\n\
         \x20 --policy JSON|PATH            retention policy as inline JSON or a JSON file\n\
         \x20 --max-cache-bytes N           cap for the verdict cache directory\n\
         \x20 --max-snapshot-bytes N        cap for the immutable snapshot store\n\
         \x20 --max-incremental-bytes N     cap for incremental/out/units build state\n\
         \x20 --max-total-bytes N           global cap for all collected state\n\
         \x20 --keep-recent N               most recent served contents never deleted (>=1)\n\
         \x20 --dry-run                     report the plan, delete nothing\n\
         \n\
         Policy JSON fields: max_cache_bytes, max_snapshot_bytes, max_incremental_bytes,\n\
         max_total_bytes, keep_recent, dry_run (unknown fields are rejected).\n\
         Server GC command: {{\"cmd\":\"gc\",\"policy\":{{...}}}}\n\
         \n\
         Exit codes: 0 success, 1 invalid policy or GC error, 2 usage error."
    );
    std::process::exit(0);
}

fn usage_exit(message: &str) -> ! {
    if !message.is_empty() {
        eprintln!("ravum-daemon: {message}");
    }
    eprintln!(
        "usage: ravum-daemon [--work-dir PATH] [--max-jobs N] [--max-cache-entries N] \
         [--analysis-timeout-ms N] [--poll-interval-ms N]\n       \
         ravum-daemon --gc --work-dir PATH [--policy JSON|PATH] \
         [--max-cache-bytes N] [--max-snapshot-bytes N] [--max-incremental-bytes N] \
         [--max-total-bytes N] [--keep-recent N] [--dry-run]\n       \
         ravum-daemon --help"
    );
    std::process::exit(2);
}
