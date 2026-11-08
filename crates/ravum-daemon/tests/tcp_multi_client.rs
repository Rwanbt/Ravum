//! Loopback TCP transport tests (P0-H §14): multi-client, refusals, clean stop.
//!
//! The server is the real `ravum-daemon` binary listening on `127.0.0.1:0`; each
//! test speaks the real JSON-lines protocol over a real TCP connection. Analyses
//! are real `rustc` runs, serialized by [`RUSTC_LOCK`] so the suite stays within
//! the machine budget.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

static RUSTC_LOCK: Mutex<()> = Mutex::new(());
static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

fn rustc_guard() -> std::sync::MutexGuard<'static, ()> {
    match RUSTC_LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "ravum-daemon-tcp-test-{name}-{}-{seq}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn daemon_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ravum-daemon"))
}

struct Server {
    child: Child,
    address: String,
}

impl Server {
    fn start(work_dir: &Path, extra_args: &[&str]) -> Server {
        let mut command = Command::new(daemon_binary());
        command
            .arg("--work-dir")
            .arg(work_dir)
            .arg("--listen")
            .arg("127.0.0.1:0")
            .args(extra_args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());
        let mut child = command.spawn().expect("spawn daemon");
        let stdout = child.stdout.take().expect("stdout");
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line).expect("listening line");
        let value: Value = serde_json::from_str(line.trim()).expect("listening line is JSON");
        let address = value["listening"]
            .as_str()
            .expect("listening address")
            .to_owned();
        Server { child, address }
    }

    fn connect(&self) -> TcpStream {
        let stream = TcpStream::connect(&self.address).expect("connect daemon");
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .expect("read timeout");
        stream
    }

    fn stop(mut self) -> i32 {
        // A dedicated connection requests the clean shutdown.
        let _ = Self::round_trip(&self, &json!({"id": "shutdown", "cmd": "shutdown"}));
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

    fn round_trip(&self, command: &Value) -> Value {
        let mut stream = self.connect();
        send_command(&mut stream, command);
        read_response(&mut stream)
    }
}

fn send_command(stream: &mut TcpStream, command: &Value) {
    let mut text = serde_json::to_string(command).expect("serialize command");
    text.push('\n');
    stream.write_all(text.as_bytes()).expect("write command");
    stream.flush().expect("flush command");
}

fn read_response(stream: &mut TcpStream) -> Value {
    let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
    let mut line = String::new();
    reader.read_line(&mut line).expect("read response");
    serde_json::from_str(line.trim()).expect("response is JSON")
}

fn analyze_command(seed: u32) -> Value {
    let content = format!(
        "pub fn tcp_value_{seed}(x: f64) -> f64 {{ x * {}.0 + 1.0 }}\n",
        seed + 2
    );
    json!({
        "id": format!("analyze-{seed}"),
        "cmd": "analyze",
        "revision": 1,
        "label": format!("tcp-{seed}"),
        "root": "src/lib.rs",
        "files": [{"path": "src/lib.rs", "content": content}],
        "context": {"edition": "2021", "crate_name": "tcp_unit", "crate_type": "lib", "flags": []},
    })
}

#[test]
fn two_simultaneous_clients_get_correct_answers_and_clean_shutdown() {
    let _guard = rustc_guard();
    let work = temp_dir("two-clients");
    let server = Server::start(&work, &[]);
    let address = server.address.clone();
    // Two real TCP connections analysed concurrently.
    let mut handles = Vec::new();
    for client in 0..2u32 {
        let address = address.clone();
        handles.push(std::thread::spawn(move || {
            let mut stream = TcpStream::connect(&address).expect("connect");
            stream
                .set_read_timeout(Some(Duration::from_secs(60)))
                .expect("timeout");
            let mut responses = Vec::new();
            for index in 0..2u32 {
                let seed = client * 10 + index + 1;
                send_command(&mut stream, &analyze_command(seed));
                let response = read_response(&mut stream);
                responses.push(response);
            }
            responses
        }));
    }
    let mut analyzed = 0usize;
    for handle in handles {
        for response in handle.join().expect("client thread") {
            assert_eq!(response["ok"], true, "response: {response}");
            let report = &response["report"];
            assert_eq!(report["status"], "fresh");
            assert_eq!(report["success"], true);
            analyzed += 1;
        }
    }
    assert_eq!(analyzed, 4);
    // The current revision stayed monotone (revision 1 in every request).
    let stats = server.round_trip(&json!({"id": "stats", "cmd": "stats"}));
    assert_eq!(stats["ok"], true);
    assert!(stats["stats"]["requests"].as_u64().unwrap_or(0) >= 4);
    assert_eq!(stats["stats"]["cache_corrupt"], 0);
    let exit = server.stop();
    assert_eq!(exit, 0, "the daemon must shut down cleanly");
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn listen_and_gc_modes_are_mutually_exclusive() {
    let work = temp_dir("double-mode");
    let mut command = Command::new(daemon_binary());
    command
        .arg("--work-dir")
        .arg(&work)
        .arg("--listen")
        .arg("127.0.0.1:0")
        .arg("--gc")
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let output = command.output().expect("run daemon");
    assert!(!output.status.success(), "double mode must be refused");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot be combined"),
        "clear refusal message expected, got: {stderr}"
    );
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn occupied_port_is_a_clear_error() {
    let work = temp_dir("port-occupied");
    let occupied = TcpListener::bind("127.0.0.1:0").expect("bind probe listener");
    let port = occupied.local_addr().expect("probe address").port();
    let mut command = Command::new(daemon_binary());
    command
        .arg("--work-dir")
        .arg(&work)
        .arg("--listen")
        .arg(format!("127.0.0.1:{port}"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = command.output().expect("run daemon");
    assert!(!output.status.success(), "a busy port must fail closed");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot listen") || stderr.contains("occup"),
        "clear bind error expected, got: {stderr}"
    );
    drop(occupied);
    let _ = std::fs::remove_dir_all(&work);
}
