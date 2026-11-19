//! `ravum-lang watch --reload` — hot reload with state preservation
//! (RFC-E11-HOT-RELOAD, `spec/language/reload.md`).
//!
//! The host owns the event stream: every line on its standard input is one
//! event, handed to the module as `step(state, line)`. The module is a child
//! process built by the pipeline with the reload runtime; it keeps `State` in
//! memory and answers one protocol command at a time on its standard output
//! (lines that start with `\u{1}rvr `; every other line is program output and
//! is forwarded). Messages of the host go to standard error.
//!
//! Guarantees, each tested by `crates/ravum-lang-cli/tests/hot_reload.rs`:
//! * HR-1.2 — a swap happens only between two events: the event loop and the
//!   swap run on the same thread, and a command waits for its reply;
//! * HR-1.3 — a module whose state schema digest differs from the current one
//!   is refused, unless its `migrate` takes exactly the current schema
//!   (HR-1.7); the old module keeps running;
//! * HR-1.4 — a build failure (diagnostics, contract, rustc) changes nothing;
//! * HR-1.5 — a panic in `step` leaves the state of before the call (the
//!   module restores it) and the event is replayed on the previous module.
//!
//! No process-level unsafe code and no dynamic loading: the state crosses a
//! swap as its canonical text (§3 of the RFC), produced by the old module and
//! parsed by the new one.

use super::{scan, signal, source_snapshot, stop_requested, watch_root, POLL_INTERVAL};
use crate::pipeline::{self, BuildOptions};
use crate::util;
use ravum_lang_sema::Profile;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc;
use std::time::Instant;

/// Prefix of a protocol reply on the module's standard output.
const REPLY: &str = "\u{1}rvr ";

fn note(line: &str) {
    eprintln!("[ravum] reload: {line}");
}

/// One running module.
struct Module {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    binary: PathBuf,
    digest: String,
    old_digest: Option<String>,
}

impl Module {
    fn spawn(root: &Path, binary: &Path) -> Result<Module, String> {
        let mut child = Command::new(binary)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| format!("cannot start `{}`: {error}", binary.display()))?;
        let input = child.stdin.take().ok_or("no module stdin")?;
        let output = BufReader::new(child.stdout.take().ok_or("no module stdout")?);
        let mut module = Module {
            child,
            input,
            output,
            binary: binary.to_path_buf(),
            digest: String::new(),
            old_digest: None,
        };
        let reply = module.request("schema")?;
        let mut parts = reply.split(' ');
        match (parts.next(), parts.next(), parts.next()) {
            (Some("schema"), Some(digest), Some(old)) => {
                module.digest = digest.to_string();
                module.old_digest = (old != "-").then(|| old.to_string());
                Ok(module)
            }
            _ => Err(format!("unexpected reply to `schema`: {reply}")),
        }
    }

    /// Send one command and wait for its reply, forwarding program output.
    fn request(&mut self, command: &str) -> Result<String, String> {
        writeln!(self.input, "{command}")
            .and_then(|()| self.input.flush())
            .map_err(|error| format!("the module stopped: {error}"))?;
        let mut line = String::new();
        loop {
            line.clear();
            match self.output.read_line(&mut line) {
                Ok(0) => return Err("the module exited".to_string()),
                Err(error) => return Err(format!("the module stopped: {error}")),
                Ok(_) => {}
            }
            let text = line.trim_end_matches(['\n', '\r']);
            if let Some(reply) = text.strip_prefix(REPLY) {
                return Ok(reply.to_string());
            }
            println!("{text}");
            let _ = std::io::stdout().flush();
        }
    }

    fn dump(&mut self) -> Result<String, String> {
        let reply = self.request("dump")?;
        reply
            .strip_prefix("state ")
            .map(str::to_string)
            .ok_or_else(|| format!("unexpected reply to `dump`: {reply}"))
    }

    fn expect_ok(&mut self, command: &str) -> Result<(), String> {
        match self.request(command)?.as_str() {
            "ok" => Ok(()),
            other => Err(other.to_string()),
        }
    }
}

impl Drop for Module {
    fn drop(&mut self) {
        let _ = writeln!(self.input, "quit");
        let _ = self.input.flush();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Host {
    root: PathBuf,
    dir: PathBuf,
    counter: u64,
    current: Module,
    /// The module replaced by the last swap, kept alive with the state of the
    /// swap until the new module completes one event (HR-1.5 after `migrate`).
    probation: Option<Module>,
    /// Artifact and digest of the previous module, for a later reversion.
    previous: Option<(PathBuf, String)>,
}

/// Build one reloadable module and copy it into the session directory.
fn build(path: &Path, release: bool, dir: &Path, counter: u64) -> Result<PathBuf, String> {
    let mut frontend = pipeline::analyze(path, Profile::Core, false);
    let contract = pipeline::reload_contract(&mut frontend);
    if !frontend.success() {
        eprint!("{}", pipeline::render_diags(&frontend));
        return Err("the program has errors".to_string());
    }
    let options = BuildOptions {
        release,
        reload: contract,
        ..BuildOptions::default()
    };
    let artifacts = pipeline::compile_program(&frontend, &options).map_err(|e| e.message())?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let name = artifacts
        .binary
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let copy = dir.join(format!("{counter}-{name}"));
    std::fs::copy(&artifacts.binary, &copy).map_err(|e| e.to_string())?;
    Ok(copy)
}

impl Host {
    fn start(root: &Path, path: &Path, release: bool) -> Result<Host, String> {
        let dir = root
            .join(util::CACHE_DIR)
            .join("watch")
            .join(format!("reload-{}", std::process::id()));
        let binary = build(path, release, &dir, 0)?;
        let mut current = Module::spawn(root, &binary)?;
        current
            .expect_ok("init")
            .map_err(|reply| format!("`init` failed: {reply}"))?;
        note(&format!(
            "started (state schema {})",
            short(&current.digest)
        ));
        Ok(Host {
            root: root.to_path_buf(),
            dir,
            counter: 1,
            current,
            probation: None,
            previous: None,
        })
    }

    /// A new build: accept it only if its state schema is compatible.
    fn try_swap(&mut self, path: &Path, release: bool) {
        let binary = match build(path, release, &self.dir, self.counter) {
            Ok(binary) => binary,
            Err(message) => {
                note(&format!(
                    "refused: {message}; the previous module keeps running"
                ));
                return;
            }
        };
        self.counter += 1;
        let mut next = match Module::spawn(&self.root, &binary) {
            Ok(module) => module,
            Err(message) => {
                note(&format!(
                    "refused: {message}; the previous module keeps running"
                ));
                return;
            }
        };
        let migrating = next.digest != self.current.digest;
        if migrating && next.old_digest.as_deref() != Some(self.current.digest.as_str()) {
            note(&format!(
                "refused: the state schema changed ({} -> {}) and no `migrate` takes the current \
                 state; the previous module keeps running",
                short(&self.current.digest),
                short(&next.digest)
            ));
            return;
        }
        let state = match self.current.dump() {
            Ok(state) => state,
            Err(message) => {
                note(&format!(
                    "refused: cannot read the current state: {message}"
                ));
                return;
            }
        };
        let verb = if migrating { "migrate" } else { "load" };
        if let Err(reply) = next.expect_ok(&format!("{verb} {state}")) {
            note(&format!(
                "refused: `{verb}` failed ({reply}); the previous module keeps running"
            ));
            return;
        }
        let old = std::mem::replace(&mut self.current, next);
        self.previous = Some((old.binary.clone(), old.digest.clone()));
        self.probation = Some(old);
        note(if migrating {
            "applied (state migrated)"
        } else {
            "applied (state kept)"
        });
    }

    fn step(&mut self, line: &str) -> Result<(), String> {
        let command = format!("step {line}");
        let reply = match self.current.request(&command) {
            Ok(reply) => reply,
            Err(message) => {
                // The module died: only a module on probation can take over.
                let Some(old) = self.probation.take() else {
                    return Err(message);
                };
                note(&format!(
                    "the new module failed ({message}); reverted to the previous module"
                ));
                self.current = old;
                return self.replay(&command);
            }
        };
        if reply == "ok" {
            self.probation = None;
            return Ok(());
        }
        note(&format!("step failed: {reply}"));
        if let Some(old) = self.probation.take() {
            self.current = old;
            note("reverted to the previous module");
            return self.replay(&command);
        }
        if let Some((binary, digest)) = self.previous.take() {
            if digest == self.current.digest {
                let reverted = self.current.dump().and_then(|state| {
                    let mut module = Module::spawn(&self.root, &binary)?;
                    module.expect_ok(&format!("load {state}"))?;
                    Ok(module)
                });
                if let Ok(module) = reverted {
                    self.current = module;
                    note("reverted to the previous module");
                    return self.replay(&command);
                }
            }
        }
        note("no previous module to revert to; the event is dropped and the state is kept");
        Ok(())
    }

    fn replay(&mut self, command: &str) -> Result<(), String> {
        match self.current.request(command)?.as_str() {
            "ok" => Ok(()),
            other => {
                note(&format!(
                    "step failed again ({other}); the event is dropped and the state is kept"
                ));
                Ok(())
            }
        }
    }

    fn finish(mut self) -> i32 {
        match self.current.dump() {
            Ok(state) => {
                note(&format!("final state {state}"));
                util::EXIT_OK
            }
            Err(message) => {
                note(&format!("cannot read the final state: {message}"));
                util::EXIT_FAIL
            }
        }
    }
}

fn short(digest: &str) -> &str {
    digest.get(..12).unwrap_or(digest)
}

pub(crate) fn run(path: &Path, release: bool) -> i32 {
    let root = match watch_root(path) {
        Ok(root) => root,
        Err(message) => {
            util::err(message);
            return util::EXIT_FAIL;
        }
    };
    super::STOP.store(false, std::sync::atomic::Ordering::SeqCst);
    signal::install();
    let mut snapshot = source_snapshot(scan(&root));
    let mut host = match Host::start(&root, path, release) {
        Ok(host) => host,
        Err(message) => {
            note(&format!("cannot start: {message}"));
            return util::EXIT_FAIL;
        }
    };

    let (events, inbox) = mpsc::channel::<Option<String>>();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(line) => {
                    if events.send(Some(line)).is_err() {
                        return;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = events.send(None);
    });

    let dir = host.dir.clone();
    let mut last_scan = Instant::now();
    let code = loop {
        if stop_requested() {
            break host.finish();
        }
        // Events and polling share this thread, so a swap always falls between
        // two events (HR-1.2); a steady stream of events still lets the
        // sources be scanned once per interval.
        let wait = POLL_INTERVAL.saturating_sub(last_scan.elapsed());
        match inbox.recv_timeout(wait) {
            Ok(Some(line)) => {
                if let Err(message) = host.step(&line) {
                    note(&format!("stopped: {message}"));
                    break util::EXIT_FAIL;
                }
                if last_scan.elapsed() < POLL_INTERVAL {
                    continue;
                }
            }
            Ok(None) | Err(mpsc::RecvTimeoutError::Disconnected) => break host.finish(),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        last_scan = Instant::now();
        let now = source_snapshot(scan(&root));
        if now != snapshot {
            snapshot = now;
            note("change detected");
            host.try_swap(path, release);
            // An edit during the build is picked up on the next tick.
            snapshot = {
                let after = source_snapshot(scan(&root));
                if after != snapshot {
                    std::collections::BTreeMap::new()
                } else {
                    after
                }
            };
        }
    };
    let _ = std::fs::remove_dir_all(dir);
    code
}
