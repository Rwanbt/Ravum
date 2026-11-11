//! Real multi-client soak over the loopback TCP transport (P0-H §14).
//!
//! Starts the real `ravum-daemon` binary with `--listen 127.0.0.1:0`, connects N
//! real TCP clients (JSON-lines protocol), runs them concurrently for a bounded
//! duration, samples the server process RSS / on-disk state / stats every second
//! from an admin connection, then runs a single-client control campaign of the
//! same length. All analyses are real `rustc` runs; the daemon's internal job gate
//! bounds concurrency.

use crate::harness::{git_json, load_json, machine_json, toolchain_json, write_evidence};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

pub struct MultiSoakOptions {
    pub repo_root: PathBuf,
    pub work_root: PathBuf,
    pub clients: usize,
    pub seconds: u64,
    pub pace_ms: u64,
    pub control_seconds: u64,
    pub control_pace_ms: u64,
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
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
            let entry_path = entry.path();
            if entry_path.is_dir() {
                stack.push(entry_path);
            } else if let Ok(metadata) = entry.metadata() {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    total
}

fn daemon_binary() -> Result<PathBuf, String> {
    let current = std::env::current_exe()
        .map_err(|error| format!("cannot resolve current executable: {error}"))?;
    let directory = current
        .parent()
        .ok_or_else(|| "current executable has no parent directory".to_owned())?;
    let candidate = directory.join(if cfg!(windows) {
        "ravum-daemon.exe"
    } else {
        "ravum-daemon"
    });
    if candidate.is_file() {
        Ok(candidate)
    } else {
        Err(format!(
            "daemon binary not found next to the bench binary: {}",
            candidate.display()
        ))
    }
}

struct Server {
    child: Child,
    address: String,
}

impl Server {
    fn start(binary: &Path, work_dir: &Path) -> Result<Server, String> {
        let _ = std::fs::remove_dir_all(work_dir);
        std::fs::create_dir_all(work_dir)
            .map_err(|error| format!("cannot create work dir: {error}"))?;
        let mut child = Command::new(binary)
            .arg("--work-dir")
            .arg(work_dir)
            .arg("--listen")
            .arg("127.0.0.1:0")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .spawn()
            .map_err(|error| format!("cannot spawn daemon: {error}"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "daemon stdout pipe missing".to_owned())?;
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|error| format!("cannot read listening line: {error}"))?;
        let value: Value = serde_json::from_str(line.trim())
            .map_err(|error| format!("listening line is not JSON: {error} ({line:?})"))?;
        let address = value["listening"]
            .as_str()
            .ok_or_else(|| format!("listening line has no address: {line:?}"))?
            .to_owned();
        Ok(Server { child, address })
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn rss(&self) -> Option<u64> {
        ravum_daemon::sysinfo::process_working_set(self.pid())
    }

    fn connect(&self) -> Result<TcpStream, String> {
        let stream = TcpStream::connect(&self.address)
            .map_err(|error| format!("cannot connect to {}: {error}", self.address))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(120)))
            .map_err(|error| format!("cannot set read timeout: {error}"))?;
        Ok(stream)
    }

    fn stop(&mut self) -> i32 {
        if let Ok(mut stream) = self.connect() {
            let _ = send(&mut stream, &json!({"id": "shutdown", "cmd": "shutdown"}));
            let _ = read(&mut stream);
        }
        for _ in 0..200 {
            match self.child.try_wait() {
                Ok(Some(status)) => return status.code().unwrap_or(-1),
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(_) => break,
            }
        }
        let _ = self.child.kill();
        -1
    }
}

fn send(stream: &mut TcpStream, command: &Value) -> Result<(), String> {
    let mut text = serde_json::to_string(command).map_err(|error| error.to_string())?;
    text.push('\n');
    stream
        .write_all(text.as_bytes())
        .map_err(|error| format!("write failed: {error}"))?;
    stream.flush().map_err(|error| error.to_string())
}

fn read(stream: &mut TcpStream) -> Result<Value, String> {
    let mut reader = BufReader::new(stream.try_clone().map_err(|error| error.to_string())?);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|error| format!("read failed: {error}"))?;
    serde_json::from_str(line.trim()).map_err(|error| format!("invalid response JSON: {error}"))
}

fn content(seed: u64) -> String {
    format!(
        "//! multi-client soak {seed}\npub fn multi_{seed}(x: f64) -> f64 {{\n    x * {}.0 + 1.0\n}}\n",
        seed + 2
    )
}

fn analyze_command(seed: u64) -> Value {
    json!({
        "id": format!("req-{seed}"),
        "cmd": "analyze",
        "revision": 1,
        "label": format!("multi-{seed}"),
        "root": "src/lib.rs",
        "files": [{"path": "src/lib.rs", "content": content(seed)}],
        "context": {"edition": "2021", "crate_name": "multi_unit", "crate_type": "lib", "flags": []},
    })
}

struct ClientResult {
    client: usize,
    requests: usize,
    latencies: Vec<u64>,
    errors: Vec<String>,
    timeouts: usize,
    last_seed: u64,
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
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let sum: u64 = sorted.iter().sum();
    json!({
        "count": sorted.len(),
        "min": sorted[0],
        "p50": percentile(&sorted, 50.0),
        "p95": percentile(&sorted, 95.0),
        "p99": percentile(&sorted, 99.0),
        "max": sorted[sorted.len() - 1],
        "mean": (sum as f64) / (sorted.len() as f64),
    })
}

fn client_thread(address: String, client: usize, deadline: Instant, pace_ms: u64) -> ClientResult {
    let mut result = ClientResult {
        client,
        requests: 0,
        latencies: Vec::new(),
        errors: Vec::new(),
        timeouts: 0,
        last_seed: 0,
    };
    let mut stream = match TcpStream::connect(&address) {
        Ok(stream) => stream,
        Err(error) => {
            result.errors.push(format!("connect failed: {error}"));
            return result;
        }
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(120)));
    let mut index: u64 = 0;
    let mut last: Option<u64> = None;
    while Instant::now() < deadline {
        index += 1;
        let seed = if index % 4 == 0 {
            last.unwrap_or(client as u64 * 1_000 + index)
        } else {
            client as u64 * 1_000_000 + index
        };
        last = Some(seed);
        let started = Instant::now();
        let command = analyze_command(seed);
        let mut text = match serde_json::to_string(&command) {
            Ok(text) => text,
            Err(error) => {
                result.errors.push(format!("serialize: {error}"));
                break;
            }
        };
        text.push('\n');
        if let Err(error) = stream.write_all(text.as_bytes()) {
            result
                .errors
                .push(format!("request {index} write failed: {error}"));
            break;
        }
        let mut reader = match stream.try_clone() {
            Ok(clone) => BufReader::new(clone),
            Err(error) => {
                result.errors.push(format!("clone failed: {error}"));
                break;
            }
        };
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => {
                result
                    .errors
                    .push("server closed the connection".to_owned());
                break;
            }
            Ok(_) => match serde_json::from_str::<Value>(line.trim()) {
                Ok(response) => {
                    let ok = response["ok"].as_bool() == Some(true);
                    let report_ok = response["report"]["success"].as_bool() == Some(true)
                        && response["report"]["status"].as_str() == Some("fresh");
                    if !ok || !report_ok {
                        result.errors.push(format!(
                            "request {index} failed: {}",
                            serde_json::to_string(&response).unwrap_or_default()
                        ));
                    }
                    result.latencies.push(started.elapsed().as_millis() as u64);
                    result.requests += 1;
                    result.last_seed = seed;
                }
                Err(error) => {
                    result
                        .errors
                        .push(format!("request {index} invalid JSON: {error}"));
                }
            },
            Err(error) => {
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::TimedOut
                {
                    result.timeouts += 1;
                    result.errors.push(format!("request {index} timeout"));
                    break;
                }
                result
                    .errors
                    .push(format!("request {index} read failed: {error}"));
                break;
            }
        }
        if pace_ms > 0 {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let pause = Duration::from_millis(pace_ms).min(remaining);
            if !pause.is_zero() {
                std::thread::sleep(pause);
            }
        }
    }
    result
}

struct Campaign {
    summary: Value,
    valid: bool,
}

fn run_campaign(
    name: &str,
    binary: &Path,
    work_root: &Path,
    clients: usize,
    seconds: u64,
    pace_ms: u64,
) -> Result<Campaign, String> {
    let work = work_root.join(format!("{name}-daemon"));
    let started = Instant::now();
    let mut server = Server::start(binary, &work)?;
    let address = server.address.clone();
    let deadline = started + Duration::from_secs(seconds.max(1));
    let disk_start = dir_size(&work);
    let rss_start = server.rss().unwrap_or(0);
    let mut samples: Vec<Value> = Vec::new();
    let mut rss_max = rss_start;
    let mut disk_max = disk_start;
    let mut stats_final = json!({});
    let requests_total = Arc::new(AtomicUsize::new(0));
    let admin = server
        .connect()
        .map_err(|error| format!("{name}: admin connection failed: {error}"))?;
    let admin = Mutex::new(admin);
    let handles: Vec<_> = (0..clients)
        .map(|client| {
            let address = address.clone();
            let requests_total = Arc::clone(&requests_total);
            std::thread::spawn(move || {
                let result = client_thread(address, client, deadline, pace_ms);
                requests_total.fetch_add(result.requests, Ordering::SeqCst);
                result
            })
        })
        .collect();
    // Sampling loop: one sample per second while the clients run.
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1000));
        let rss = server.rss().unwrap_or(0);
        rss_max = rss_max.max(rss);
        let disk = dir_size(&work);
        disk_max = disk_max.max(disk);
        if let Ok(mut stream) = admin.lock() {
            if send(&mut stream, &json!({"id": "sample", "cmd": "stats"})).is_ok() {
                if let Ok(response) = read(&mut stream) {
                    stats_final = response["stats"].clone();
                }
            }
        }
        samples.push(json!({
            "elapsed_s": started.elapsed().as_secs(),
            "requests_total": requests_total.load(Ordering::SeqCst),
            "rss_bytes": rss,
            "disk_bytes": disk,
            "cache_hits": stats_final["cache_hits_memory"].as_u64().unwrap_or(0)
                + stats_final["cache_hits_disk"].as_u64().unwrap_or(0),
            "cache_misses": stats_final["cache_misses"],
            "gc_runs": stats_final["gc_runs"],
            "auto_gc_runs": stats_final["auto_gc"]["runs"],
            "requests_stats": stats_final["requests"],
        }));
    }
    let mut results = Vec::new();
    for handle in handles {
        results.push(
            handle
                .join()
                .map_err(|_| format!("{name}: client thread panicked"))?,
        );
    }
    let mut all_latencies: Vec<u64> = Vec::new();
    let mut client_jsons = Vec::new();
    let mut errors = 0usize;
    let mut timeouts = 0usize;
    for result in &results {
        all_latencies.extend(result.latencies.iter().copied());
        errors += result.errors.len();
        timeouts += result.timeouts;
        client_jsons.push(json!({
            "client": result.client,
            "requests": result.requests,
            "latency_ms": latency_stats(&result.latencies),
            "errors": result.errors,
            "timeouts": result.timeouts,
        }));
    }
    // Final stats and a recent-content cache-hit verification via the admin link.
    let mut verification = json!({});
    {
        let mut stream = admin
            .lock()
            .map_err(|_| format!("{name}: admin lock poisoned"))?;
        send(&mut stream, &json!({"id": "final", "cmd": "stats"}))
            .map_err(|error| format!("{name}: final stats failed: {error}"))?;
        let response = read(&mut stream).map_err(|error| format!("{name}: {error}"))?;
        stats_final = response["stats"].clone();
        if let Some(first) = results.first() {
            send(&mut stream, &analyze_command(first.last_seed))
                .map_err(|error| format!("{name}: verification send failed: {error}"))?;
            let response = read(&mut stream).map_err(|error| format!("{name}: {error}"))?;
            let report = &response["report"];
            verification = json!({
                "recent_cache_kind": report["cache"]["kind"],
                "recent_analysis_ms": report["timings"]["analysis_ms"],
                "recent_hit_without_compiler": report["cache"]["hit"].as_bool() == Some(true)
                    && report["timings"]["analysis_ms"].as_u64() == Some(0),
            });
        }
    }
    let disk_end = dir_size(&work);
    let rss_end = server.rss().unwrap_or(0);
    drop(admin);
    let exit_code = server.stop();
    let duration_s = started.elapsed().as_secs();
    let valid = errors == 0
        && timeouts == 0
        && exit_code == 0
        && verification["recent_hit_without_compiler"].as_bool() == Some(true)
        && results.iter().all(|result| result.requests > 0);
    let summary = json!({
        "name": name,
        "clients": clients,
        "duration_s": duration_s,
        "requests_total": results.iter().map(|result| result.requests).sum::<usize>(),
        "errors": errors,
        "timeouts": timeouts,
        "per_client": client_jsons,
        "latency_ms": latency_stats(&all_latencies),
        "server": {
            "pid": server.pid(),
            "rss_start_bytes": rss_start,
            "rss_max_bytes": rss_max,
            "rss_end_bytes": rss_end,
            "rss_drift_bytes": rss_end as i64 - rss_start as i64,
            "disk_start_bytes": disk_start,
            "disk_max_bytes": disk_max,
            "disk_end_bytes": disk_end,
            "daemon_exit_code": exit_code,
        },
        "stats": stats_final,
        "verification": verification,
        "samples": samples,
        "limits": [
            "one daemon process on the loopback interface; not multi-machine",
            "clients use a fixed revision (1): the daemon accepts equal revisions but a strictly older revision would be superseded by design; the multi-client campaign measures transport + analysis, not revision choreography",
            "the internal job gate still serializes rustc processes (max_concurrent_jobs=1); client concurrency is real but compiler concurrency is bounded on purpose",
        ],
    });
    Ok(Campaign { summary, valid })
}

pub fn run(options: &MultiSoakOptions) -> Result<Value, String> {
    if options.clients == 0 || options.clients > 16 {
        return Err("--clients must be in 1..=16".to_owned());
    }
    let binary = daemon_binary()?;
    let (toolchain, toolchain_evidence) = toolchain_json(&options.repo_root)?;
    let rustc = crate::harness::rustc_path();
    std::fs::create_dir_all(&options.work_root)
        .map_err(|error| format!("cannot create multi-soak work root: {error}"))?;
    let load_start = load_json(&[], &rustc);
    let generated_unix = now_unix();
    let multi = run_campaign(
        "multi",
        &binary,
        &options.work_root,
        options.clients,
        options.seconds,
        options.pace_ms,
    )?;
    let control = run_campaign(
        "control",
        &binary,
        &options.work_root,
        1,
        options.control_seconds,
        options.control_pace_ms,
    )?;
    let load_end = load_json(&[], &rustc);
    let valid = multi.valid && control.valid;
    Ok(json!({
        "schema_version": 1,
        "kind": "mvp0b-daemon-soak-multi-2026-11-08",
        "generated_unix": generated_unix,
        "machine": machine_json(),
        "toolchain": {
            "release": toolchain.release,
            "host": toolchain.host,
            "commit_hash": toolchain.commit_hash,
        },
        "toolchain_evidence": toolchain_evidence,
        "git": git_json(&options.repo_root),
        "command": format!(
            "cargo run -q -p ravum-daemon --bin ravum-daemon-bench -- --suite multi-soak --clients {} --multi-seconds {} --multi-control-seconds {}",
            options.clients, options.seconds, options.control_seconds
        ),
        "transport": "loopback TCP JSON-lines (127.0.0.1:0)",
        "load_start": load_start,
        "load_end": load_end,
        "multi": multi.summary,
        "control": control.summary,
        "comparison": {
            "multi_requests": multi.summary["requests_total"],
            "control_requests": control.summary["requests_total"],
            "multi_latency_p50_ms": multi.summary["latency_ms"]["p50"],
            "control_latency_p50_ms": control.summary["latency_ms"]["p50"],
            "multi_latency_p95_ms": multi.summary["latency_ms"]["p95"],
            "control_latency_p95_ms": control.summary["latency_ms"]["p95"],
            "multi_rss_drift_bytes": multi.summary["server"]["rss_drift_bytes"],
            "control_rss_drift_bytes": control.summary["server"]["rss_drift_bytes"],
            "multi_disk_end_bytes": multi.summary["server"]["disk_end_bytes"],
            "control_disk_end_bytes": control.summary["server"]["disk_end_bytes"],
            "note": "multi and control campaigns share the machine sequentially; compare error counts and per-client latency, not absolute duration",
        },
        "valid": valid,
        "limits": [
            "loopback only, single machine: no network/multi-host claim",
            "rustc concurrency is bounded by the daemon job gate (max_concurrent_jobs=1): clients are concurrent, compilers are serialized by design on this machine",
            "bounded campaign (minutes); it detects deadlocks, errors and gross RSS drift, not multi-hour stability",
            "clients use a fixed revision; the revision contract is tested separately in the integration tests",
        ],
    }))
}

pub fn write(path: &Path, value: &Value) -> Result<(), String> {
    write_evidence(path, value)
}

#[allow(dead_code)]
fn _unique_suffix() -> u64 {
    TEMP_SEQ.fetch_add(1, Ordering::Relaxed)
}
