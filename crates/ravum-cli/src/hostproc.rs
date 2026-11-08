//! Child-process control for the demo host (line-delimited JSON over stdio).

use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;

pub struct HostProc {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Value>,
}

impl HostProc {
    pub fn spawn(artifact: &Path, tick_ms: u64) -> std::io::Result<Self> {
        let bin = host_binary()?;
        let mut child = Command::new(&bin)
            .arg("--patch")
            .arg(artifact)
            .arg("--tick-ms")
            .arg(tick_ms.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let (tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name("ravum-host-reader".to_owned())
            .spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
                        if tx.send(value).is_err() {
                            break;
                        }
                    }
                }
            })?;
        Ok(Self { child, stdin, rx })
    }

    pub fn send(&mut self, command: &Value) -> std::io::Result<()> {
        writeln!(self.stdin, "{command}")?;
        self.stdin.flush()
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Option<Value> {
        match self.rx.recv_timeout(timeout) {
            Ok(v) => Some(v),
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => None,
        }
    }

    /// Wait for the next message whose `event` matches, ignoring ticks.
    pub fn wait_for(&self, event: &str, timeout: Duration) -> Option<Value> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let msg = self.recv_timeout(remaining)?;
            if msg.get("event").and_then(Value::as_str) == Some(event) {
                return Some(msg);
            }
        }
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub fn shutdown(mut self) {
        let _ = self.send(&serde_json::json!({"cmd": "quit"}));
        thread::sleep(Duration::from_millis(50));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for HostProc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The demo host is a sibling binary produced by the same `cargo build`.
pub fn host_binary() -> std::io::Result<PathBuf> {
    if let Ok(explicit) = std::env::var("RAVUM_HOST_BIN") {
        return Ok(PathBuf::from(explicit));
    }
    let exe = std::env::current_exe()?;
    let dir = exe
        .parent()
        .ok_or_else(|| std::io::Error::other("cannot resolve CLI directory"))?;
    Ok(dir.join(format!("ravum-demo-host{}", std::env::consts::EXE_SUFFIX)))
}
