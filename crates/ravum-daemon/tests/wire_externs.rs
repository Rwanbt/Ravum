//! MVP-0c L1.3 — externs in the daemon JSON-lines protocol (stdio + TCP).
//!
//! Real end-to-end tests: a dependency `.rmeta` is produced by a real daemon
//! analysis, then the dependent analysis is sent over the actual stdio and TCP
//! protocol with the wire extern schema `{name, crate_name, interface_id,
//! bytes_hex}`. The daemon binary is the one built by Cargo for this test.
//!
//! Accepted protocol properties proven here:
//! * legacy requests (no `externs`) keep their identity and behaviour;
//! * a wire request with externs produces a different `snapshot_id`, a real miss,
//!   the extern usage in the report, and a HIT on an identical repeat;
//! * invalid externs are refused with an explicit error response and the daemon
//!   keeps serving afterwards;
//! * changing the extern at constant sources is a real miss (breaking interface
//!   surfacing a real rustc diagnostic).

use ravum_daemon::{
    AnalysisRequest, BuildContext, Daemon, DaemonConfig, Priority, SourceFile, WireAnalyzeRequest,
};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static RUSTC_LOCK: Mutex<()> = Mutex::new(());

fn rustc_guard() -> std::sync::MutexGuard<'static, ()> {
    match RUSTC_LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn daemon_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ravum-daemon"))
}

fn temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "ravum-daemon-wire-{name}-{}-{nanos}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn rustc() -> PathBuf {
    std::env::var_os("RAVUM_RUSTC")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("rustc"))
}

fn context(crate_name: &str) -> BuildContext {
    BuildContext {
        edition: "2021".to_owned(),
        crate_name: crate_name.to_owned(),
        crate_type: "lib".to_owned(),
        flags: Vec::new(),
    }
}

fn analysis_request(
    revision: u64,
    label: &str,
    crate_name: &str,
    content: &str,
) -> AnalysisRequest {
    AnalysisRequest {
        revision,
        label: label.to_owned(),
        root: "src/lib.rs".to_owned(),
        files: vec![SourceFile {
            path: "src/lib.rs".to_owned(),
            content: content.to_owned(),
        }],
        context: context(crate_name),
        priority: Priority::Normal,
    }
}

/// Produces a real dependency `.rmeta` with the library daemon on `work_dir`
/// (the protocol server is started afterwards, on a fresh process).
fn dependency_artifact(work_dir: &Path, content: &str) -> (String, Vec<u8>) {
    let daemon = Daemon::open(DaemonConfig {
        rustc: rustc(),
        work_dir: work_dir.to_path_buf(),
        max_concurrent_jobs: 1,
        poll_interval_ms: 2,
        analysis_timeout_ms: 60_000,
        ..Default::default()
    })
    .expect("library daemon");
    let report = daemon
        .analyze(&analysis_request(1, "core", "core", content))
        .expect("dependency analysis");
    assert!(report.success, "dependency must compile: {report:?}");
    let interface = report.artifact_sha256.clone().expect("artifact id");
    let bytes = daemon.artifact(&interface).expect("artifact bytes");
    drop(daemon);
    (interface, bytes)
}

fn wire_request(
    revision: u64,
    label: &str,
    crate_name: &str,
    content: &str,
    externs: &[(&str, &str, &str, &[u8])],
) -> Value {
    let request = analysis_request(revision, label, crate_name, content);
    let mut value = serde_json::to_value(&request).expect("request json");
    value["cmd"] = json!("analyze");
    if !externs.is_empty() {
        let items: Vec<Value> = externs
            .iter()
            .map(|(name, crate_name, interface_id, bytes)| {
                json!({
                    "name": name,
                    "crate_name": crate_name,
                    "interface_id": interface_id,
                    "bytes_hex": hex_encode(bytes),
                })
            })
            .collect();
        value["externs"] = Value::Array(items);
    }
    value
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

fn summarize(response: &Value) -> Value {
    let report = &response["report"];
    json!({
        "ok": response["ok"],
        "error": response["error"],
        "wall_ms": response["wall_ms"],
        "status": report["status"],
        "success": report["success"],
        "cache_hit": report["cache"]["hit"],
        "cache_kind": report["cache"]["kind"],
        "snapshot_id": report["snapshot_id"],
        "artifact_sha256": report["artifact_sha256"],
        "externs": report["externs"],
        "diagnostics": report["diagnostics"].as_array().map(|diagnostics| diagnostics.len()),
    })
}

fn evidence_request(request: &Value) -> Value {
    let mut sanitized = request.clone();
    if let Some(externs) = sanitized.get_mut("externs").and_then(Value::as_array_mut) {
        for extern_value in externs {
            if let Some(hex) = extern_value.get("bytes_hex").and_then(Value::as_str) {
                let prefix: String = hex.chars().take(32).collect();
                extern_value["bytes_hex"] = json!(format!("{prefix}… ({} hex chars)", hex.len()));
            }
        }
    }
    sanitized
}

struct StdioServer {
    child: Child,
    stdin: BufWriter<std::process::ChildStdin>,
    stdout: BufReader<std::process::ChildStdout>,
}

impl StdioServer {
    fn start(work_dir: &Path) -> Self {
        let mut child = Command::new(daemon_binary())
            .arg("--work-dir")
            .arg(work_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn stdio daemon");
        let stdin = BufWriter::new(child.stdin.take().expect("stdin"));
        let mut stdout = BufReader::new(child.stdout.take().expect("stdout"));
        let mut ready = String::new();
        stdout.read_line(&mut ready).expect("ready line");
        let ready: Value = serde_json::from_str(ready.trim()).expect("ready JSON");
        assert_eq!(ready["event"], "ready");
        Self {
            child,
            stdin,
            stdout,
        }
    }

    fn send(&mut self, request: &Value) -> Value {
        let mut line = serde_json::to_string(request).expect("serialize request");
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .expect("write request");
        self.stdin.flush().expect("flush request");
        let mut response = String::new();
        self.stdout.read_line(&mut response).expect("response line");
        serde_json::from_str(response.trim()).expect("response JSON")
    }

    fn stop(mut self) -> i32 {
        let _ = self.send(&json!({"id": "shutdown", "cmd": "shutdown"}));
        for _ in 0..100 {
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

struct TcpServer {
    child: Child,
    address: String,
}

impl TcpServer {
    fn start(work_dir: &Path) -> Self {
        let mut child = Command::new(daemon_binary())
            .arg("--work-dir")
            .arg(work_dir)
            .arg("--listen")
            .arg("127.0.0.1:0")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .spawn()
            .expect("spawn tcp daemon");
        let mut stdout = BufReader::new(child.stdout.take().expect("stdout"));
        let mut line = String::new();
        stdout.read_line(&mut line).expect("listening line");
        let listening: Value = serde_json::from_str(line.trim()).expect("listening JSON");
        let address = listening["listening"]
            .as_str()
            .expect("listening address")
            .to_owned();
        Self { child, address }
    }

    fn connect(&self) -> TcpStream {
        let stream = TcpStream::connect(&self.address).expect("connect tcp daemon");
        stream
            .set_read_timeout(Some(Duration::from_secs(120)))
            .expect("read timeout");
        stream
    }

    fn stop(mut self) -> i32 {
        if let Ok(mut stream) = TcpStream::connect(&self.address) {
            let mut line = json!({"id": "shutdown", "cmd": "shutdown"}).to_string();
            line.push('\n');
            let _ = stream.write_all(line.as_bytes());
            let mut reader = BufReader::new(stream);
            let mut response = String::new();
            let _ = reader.read_line(&mut response);
        }
        for _ in 0..100 {
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

fn tcp_send(stream: &mut TcpStream, request: &Value) -> Value {
    let mut line = serde_json::to_string(request).expect("serialize request");
    line.push('\n');
    stream.write_all(line.as_bytes()).expect("write request");
    stream.flush().expect("flush request");
    let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
    let mut response = String::new();
    reader.read_line(&mut response).expect("tcp response");
    serde_json::from_str(response.trim()).expect("tcp response JSON")
}

fn done(step: &str, request: &Value, response: &Value, transcript: &mut Vec<Value>) {
    transcript.push(json!({
        "step": step,
        "request": evidence_request(request),
        "response": summarize(response),
    }));
}

fn error_kind(response: &Value) -> Option<&str> {
    response["error"]["kind"].as_str()
}

fn spawn_rustc_version() -> String {
    let output = Command::new("rustc")
        .arg("-vV")
        .output()
        .expect("rustc -vV");
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn wire_externs_stdio_and_tcp_evidence() {
    let _guard = rustc_guard();
    let work = temp_dir("protocol");
    let core_v1 = "pub struct Point { pub x: f64 }\npub fn scale(x: f64) -> f64 { x * 2.0 }\n";
    let core_v2 = "pub struct Point { pub x: f64 }\npub fn scale(x: f64) -> f64 { x * 2.0 }\npub fn extra() -> f64 { 1.0 }\n";
    let core_v3 = "pub struct Point { pub x: f64 }\npub fn scale(x: f64, factor: f64) -> f64 { x * factor }\n";
    let api_source = "pub fn scaled(x: f64) -> f64 { core::scale(x) }\n";
    let (interface_v1, bytes_v1) = dependency_artifact(&work, core_v1);
    let (interface_v2, bytes_v2) = dependency_artifact(&work, core_v2);
    let (interface_v3, bytes_v3) = dependency_artifact(&work, core_v3);
    assert_ne!(interface_v1, interface_v2);
    assert_ne!(interface_v2, interface_v3);
    let mut transcript: Vec<Value> = Vec::new();
    let mut assertions: Vec<String> = Vec::new();

    // --- stdio protocol: legacy, wire externs, invalid externs ----------------
    let mut server = StdioServer::start(&work);
    let legacy = wire_request(1, "legacy", "legacy_unit", "pub fn legacy() {}\n", &[]);
    let legacy_first = server.send(&legacy);
    done(
        "stdio/legacy-analyze",
        &legacy,
        &legacy_first,
        &mut transcript,
    );
    assert_eq!(legacy_first["ok"], true, "legacy response: {legacy_first}");
    assert_eq!(legacy_first["report"]["success"], true);
    let legacy_snapshot = legacy_first["report"]["snapshot_id"].clone();
    assertions.push("legacy request without externs still analyses".to_owned());

    let legacy_repeat = server.send(&wire_request(
        2,
        "legacy",
        "legacy_unit",
        "pub fn legacy() {}\n",
        &[],
    ));
    done(
        "stdio/legacy-repeat",
        &legacy,
        &legacy_repeat,
        &mut transcript,
    );
    assert_eq!(legacy_repeat["report"]["cache"]["hit"], true);
    assert_eq!(legacy_repeat["report"]["snapshot_id"], legacy_snapshot);
    assertions.push("legacy repeat is a HIT with the same snapshot_id".to_owned());

    let no_extern_request = wire_request(3, "api", "api", api_source, &[]);
    let no_extern = server.send(&no_extern_request);
    done(
        "stdio/dependent-without-externs",
        &no_extern_request,
        &no_extern,
        &mut transcript,
    );
    let snapshot_without = no_extern["report"]["snapshot_id"].clone();

    let with_externs = wire_request(
        4,
        "api",
        "api",
        api_source,
        &[("core", "core", &interface_v1, &bytes_v1)],
    );
    let extern_response = server.send(&with_externs);
    done(
        "stdio/dependent-with-externs",
        &with_externs,
        &extern_response,
        &mut transcript,
    );
    assert_eq!(extern_response["ok"], true);
    assert_eq!(extern_response["report"]["success"], true);
    assert_eq!(extern_response["report"]["cache"]["hit"], false);
    assert_ne!(
        extern_response["report"]["snapshot_id"], snapshot_without,
        "externs must change the snapshot identity"
    );
    assert_eq!(
        extern_response["report"]["externs"][0]["interface_id"],
        serde_json::json!(interface_v1)
    );
    assert_eq!(
        extern_response["report"]["externs"][0]["rmeta_sha256"],
        serde_json::json!(interface_v1)
    );
    assert!(extern_response["report"]["artifact_sha256"].is_string());
    let dependent_artifact = extern_response["report"]["artifact_sha256"]
        .as_str()
        .expect("dependent artifact")
        .to_owned();
    assertions
        .push("wire externs: success, real miss, snapshot_id differs, usage reported".to_owned());

    let repeat = server.send(&wire_request(
        5,
        "api",
        "api",
        api_source,
        &[("core", "core", &interface_v1, &bytes_v1)],
    ));
    done(
        "stdio/dependent-repeat",
        &with_externs,
        &repeat,
        &mut transcript,
    );
    assert_eq!(repeat["report"]["cache"]["hit"], true);
    assert_eq!(
        repeat["report"]["artifact_sha256"].as_str(),
        Some(dependent_artifact.as_str())
    );
    assertions.push("identical wire request is a HIT and serves the same artifact".to_owned());

    // Invalid externs: malformed hex, wrong SHA, duplicate name. Each is an
    // explicit error response, never a panic; the daemon keeps working after.
    let mut bad_hex = with_externs.clone();
    bad_hex["externs"][0]["bytes_hex"] = json!("zz");
    let bad_hex_response = server.send(&bad_hex);
    done(
        "stdio/invalid-hex",
        &bad_hex,
        &bad_hex_response,
        &mut transcript,
    );
    assert_eq!(bad_hex_response["ok"], false);
    assert_eq!(error_kind(&bad_hex_response), Some("invalid_request"));

    let mut bad_sha = with_externs.clone();
    bad_sha["externs"][0]["interface_id"] = json!("a".repeat(64));
    let bad_sha_response = server.send(&bad_sha);
    done(
        "stdio/invalid-sha",
        &bad_sha,
        &bad_sha_response,
        &mut transcript,
    );
    assert_eq!(bad_sha_response["ok"], false);
    assert_eq!(error_kind(&bad_sha_response), Some("invalid_request"));

    let mut duplicate = with_externs.clone();
    duplicate["externs"] = json!([
        {"name":"core","crate_name":"core","interface_id": interface_v1,"bytes_hex": hex_encode(&bytes_v1)},
        {"name":"core","crate_name":"core","interface_id": interface_v2,"bytes_hex": hex_encode(&bytes_v2)},
    ]);
    let duplicate_response = server.send(&duplicate);
    done(
        "stdio/duplicate-extern",
        &duplicate,
        &duplicate_response,
        &mut transcript,
    );
    assert_eq!(duplicate_response["ok"], false);
    assert_eq!(error_kind(&duplicate_response), Some("invalid_request"));
    assertions.push("invalid hex/SHA/duplicate externs are explicit errors".to_owned());

    let after_errors = server.send(&wire_request(
        6,
        "legacy-after-errors",
        "legacy_unit",
        "pub fn legacy() -> u32 { 1 }\n",
        &[],
    ));
    done(
        "stdio/still-works-after-errors",
        &after_errors,
        &after_errors,
        &mut transcript,
    );
    assert_eq!(after_errors["ok"], true);
    assert_eq!(after_errors["report"]["success"], true);
    assertions.push("daemon remains functional after invalid extern requests".to_owned());

    let exit = server.stop();
    assert_eq!(exit, 0);
    assertions.push("stdio daemon shuts down cleanly".to_owned());

    // --- TCP protocol: extern change at constant sources is a real miss ------
    let tcp = TcpServer::start(&work);
    let mut stream = tcp.connect();
    let first = tcp_send(
        &mut stream,
        &wire_request(
            1,
            "api-tcp",
            "api",
            api_source,
            &[("core", "core", &interface_v1, &bytes_v1)],
        ),
    );
    done(
        "tcp/extern-v1",
        &json!({"transport":"tcp"}),
        &first,
        &mut transcript,
    );
    assert_eq!(first["ok"], true);
    assert_eq!(first["report"]["success"], true);
    let first_snapshot = first["report"]["snapshot_id"].clone();

    let repeated = tcp_send(
        &mut stream,
        &wire_request(
            2,
            "api-tcp",
            "api",
            api_source,
            &[("core", "core", &interface_v1, &bytes_v1)],
        ),
    );
    done(
        "tcp/extern-v1-repeat",
        &json!({"transport":"tcp"}),
        &repeated,
        &mut transcript,
    );
    assert_eq!(repeated["report"]["cache"]["hit"], true);
    assertions.push("TCP identical extern request is a HIT".to_owned());

    let compatible = tcp_send(
        &mut stream,
        &wire_request(
            3,
            "api-tcp",
            "api",
            api_source,
            &[("core", "core", &interface_v2, &bytes_v2)],
        ),
    );
    done(
        "tcp/extern-v2-compatible",
        &json!({"transport":"tcp"}),
        &compatible,
        &mut transcript,
    );
    assert_eq!(compatible["ok"], true);
    assert_eq!(compatible["report"]["cache"]["hit"], false);
    assert_eq!(compatible["report"]["success"], true);
    assert_ne!(compatible["report"]["snapshot_id"], first_snapshot);
    assertions
        .push("extern change at constant sources is a real miss with a new snapshot_id".to_owned());

    let breaking = tcp_send(
        &mut stream,
        &wire_request(
            4,
            "api-tcp",
            "api",
            api_source,
            &[("core", "core", &interface_v3, &bytes_v3)],
        ),
    );
    done(
        "tcp/extern-v3-breaking",
        &json!({"transport":"tcp"}),
        &breaking,
        &mut transcript,
    );
    assert_eq!(breaking["ok"], true);
    assert_eq!(breaking["report"]["success"], false);
    assert_eq!(breaking["report"]["cache"]["hit"], false);
    let diagnostics = breaking["report"]["diagnostics"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic["rendered"]
            .as_str()
            .unwrap_or("")
            .contains("E0061")),
        "real rustc diagnostic expected: {diagnostics:?}"
    );
    assertions.push("breaking extern yields a real rustc diagnostic over TCP".to_owned());
    drop(stream);
    let exit = tcp.stop();
    assert_eq!(exit, 0);
    assertions.push("TCP daemon shuts down cleanly".to_owned());

    // --- JSON evidence -------------------------------------------------------
    let evidence = json!({
        "schema_version": 1,
        "kind": "mvp0c-daemon-wire-externs-2026-11-09",
        "generated_unix": SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
        "commands": [
            "cargo test -p ravum-daemon --test wire_externs -- --test-threads=1 --nocapture",
        ],
        "rustc_vV": spawn_rustc_version(),
        "protocol": {
            "request": "legacy AnalysisRequest fields + optional externs: [{name, crate_name, interface_id, bytes_hex}]",
            "serde_default": "externs absent => empty list, legacy identity unchanged",
            "responses": "existing JSON-lines envelope; errors are {ok:false,error:{kind,message}}",
        },
        "interfaces": {
            "v1": interface_v1,
            "v2_compatible": interface_v2,
            "v3_breaking": interface_v3,
        },
        "transcript": transcript,
        "assertions": assertions,
        "valid": true,
        "limits": [
            "local loopback/stdio only, one daemon work_dir, one toolchain",
            "bytes_hex is bounded by MAX_EXTERN_BYTES (8 MiB) and MAX_EXTERNS_PER_UNIT (64)",
            "an extern supplied by a third party has no transitive sidecar; its own dependencies must be provided explicitly",
            "no PROVED: these are bounded protocol observations",
        ],
    });
    let evidence_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("evidence");
    std::fs::create_dir_all(&evidence_dir).expect("evidence dir");
    std::fs::write(
        evidence_dir.join("mvp0c-daemon-wire-externs-2026-11-09.json"),
        format!(
            "{}\n",
            serde_json::to_string_pretty(&evidence).expect("evidence json")
        ),
    )
    .expect("write evidence");
    println!(
        "wire externs evidence written; {} assertions",
        assertions.len()
    );
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn wire_request_schema_is_fail_closed_and_backward_compatible() {
    // Legacy JSON without `externs` deserializes as an empty extern list.
    let legacy = serde_json::json!({
        "revision": 1,
        "label": "unit",
        "root": "src/lib.rs",
        "files": [{"path": "src/lib.rs", "content": "pub fn a() {}"}],
        "context": {"edition": "2021", "crate_name": "unit", "crate_type": "lib", "flags": []},
        "priority": "normal",
    });
    let wire: WireAnalyzeRequest = serde_json::from_value(legacy.clone()).expect("legacy wire");
    assert!(wire.externs.is_empty());
    let (request, externs) = wire.into_parts().expect("legacy parts");
    assert_eq!(request.revision, 1);
    assert!(externs.is_empty());

    // A valid wire extern converts to the internal ExternInput.
    let bytes = b"real-rmeta-bytes".to_vec();
    let interface_id = ravum_daemon::hash::sha256_hex(&bytes);
    let with_extern = serde_json::json!({
        "revision": 2,
        "label": "unit",
        "root": "src/lib.rs",
        "files": [{"path": "src/lib.rs", "content": "pub fn a() {}"}],
        "context": {"edition": "2021", "crate_name": "unit", "crate_type": "lib", "flags": []},
        "priority": "normal",
        "externs": [{
            "name": "core",
            "crate_name": "core",
            "interface_id": interface_id,
            "bytes_hex": hex_encode(&bytes),
        }],
    });
    let wire: WireAnalyzeRequest = serde_json::from_value(with_extern).expect("wire with extern");
    let (_, externs) = wire.into_parts().expect("valid extern");
    assert_eq!(externs.len(), 1);
    assert_eq!(externs[0].bytes, bytes);

    // Unknown wire fields are refused (no silent fallback).
    let mut unknown = serde_json::json!({
        "revision": 3,
        "label": "unit",
        "root": "src/lib.rs",
        "files": [{"path": "src/lib.rs", "content": "pub fn a() {}"}],
        "context": {"edition": "2021", "crate_name": "unit", "crate_type": "lib", "flags": []},
        "externs": [{
            "name": "core", "crate_name": "core",
            "interface_id": "a".repeat(64),
            "bytes_hex": "00",
            "extra": true,
        }],
    });
    assert!(serde_json::from_value::<WireAnalyzeRequest>(unknown.clone()).is_err());
    unknown["externs"][0]
        .as_object_mut()
        .unwrap()
        .remove("extra");

    // Wrong interface id (not the SHA-256 of bytes_hex) is refused at conversion.
    let wire: WireAnalyzeRequest = serde_json::from_value(unknown).expect("shape ok");
    assert!(wire.into_parts().is_err());

    // Malformed hex is refused too.
    let mut bad_hex = serde_json::json!({
        "revision": 4,
        "label": "unit",
        "root": "src/lib.rs",
        "files": [{"path": "src/lib.rs", "content": "pub fn a() {}"}],
        "context": {"edition": "2021", "crate_name": "unit", "crate_type": "lib", "flags": []},
        "externs": [{"name": "core", "crate_name": "core", "interface_id": "a".repeat(64), "bytes_hex": "not-hex"}],
    });
    let wire: WireAnalyzeRequest = serde_json::from_value(bad_hex.clone()).expect("shape ok");
    assert!(wire.into_parts().is_err());
    bad_hex["externs"][0]["bytes_hex"] = serde_json::json!("00");
    let wire: WireAnalyzeRequest = serde_json::from_value(bad_hex).expect("shape ok");
    assert!(wire.into_parts().is_err());
}
