//! `ravum-lang watch` (alias `dev`) — poll, rebuild, optionally restart.
//!
//! Polling only (no external crate): `.rv`, `ravum.toml` and `ravum.lock` under
//! the watched directory, every 250 ms. `.ravum/`, `target/`, `.git/` and
//! `*.ravum-tmp` are ignored. A build failure is reported and polling continues.
//!
//! This is a **restart** loop, not a state-preserving hot-reload. With `--run`
//! the previous child is killed before a new binary is launched. On normal
//! termination (including Ctrl+C and `--max-rebuilds`) the child is reaped so no
//! process is leaked.

use ravum_lang_sema::Profile;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use crate::pipeline::{self, BuildOptions};
use crate::util;

/// Poll interval (mission range: 200–400 ms).
const POLL_INTERVAL: Duration = Duration::from_millis(250);
/// Grace period granted to a short-lived child when `--max-rebuilds` ends the loop.
const CHILD_GRACE: Duration = Duration::from_secs(1);

/// Set by the console signal handler; checked by the poll loop.
static STOP: AtomicBool = AtomicBool::new(false);

pub fn run(path: &Path, run_child: bool, release: bool, max_rebuilds: Option<u64>) -> i32 {
    let root = match watch_root(path) {
        Ok(root) => root,
        Err(message) => {
            util::err(message);
            return util::EXIT_FAIL;
        }
    };
    STOP.store(false, Ordering::SeqCst);
    signal::install();

    say(&format!("[ravum] watching {}", util::pretty_path(&root)));

    let mut child: Option<Child> = None;
    let mut rebuilds: u64 = 0;
    let initial = build_once(path, release);
    // Absorb build side effects (lockfile rewrite, cache writes) *before*
    // announcing the result, so a test/client reacting to the output cannot
    // race the initial snapshot.
    let mut snapshot = scan(&root);
    match initial {
        BuildOutcome::Ok(binary) => {
            say("[ravum] build ok");
            if run_child {
                child = spawn_child(&root, &binary);
            }
        }
        BuildOutcome::Failed(code) => {
            say(&format!("[ravum] build failed (exit {code})"));
        }
    }

    if max_rebuilds == Some(0) {
        finish_child(&mut child);
        return util::EXIT_OK;
    }

    loop {
        if STOP.load(Ordering::SeqCst) {
            stop_child(&mut child);
            say("[ravum] stopped");
            return util::EXIT_OK;
        }
        std::thread::sleep(POLL_INTERVAL);
        if STOP.load(Ordering::SeqCst) {
            stop_child(&mut child);
            say("[ravum] stopped");
            return util::EXIT_OK;
        }

        let current = scan(&root);
        if current == snapshot {
            continue;
        }
        say("[ravum] change detected");

        stop_child(&mut child);
        let outcome = build_once(path, release);
        // The build may rewrite `ravum.lock`; re-scan before announcing so our
        // own write never triggers another rebuild and clients reacting to the
        // output cannot race the new snapshot.
        snapshot = scan(&root);
        match outcome {
            BuildOutcome::Ok(binary) => {
                say("[ravum] build ok");
                if run_child {
                    child = spawn_child(&root, &binary);
                }
            }
            BuildOutcome::Failed(code) => {
                say(&format!("[ravum] build failed (exit {code})"));
            }
        }
        rebuilds = rebuilds.saturating_add(1);

        if max_rebuilds.map(|max| rebuilds >= max).unwrap_or(false) {
            finish_child(&mut child);
            return util::EXIT_OK;
        }
    }
}

enum BuildOutcome {
    Ok(PathBuf),
    Failed(i32),
}

fn build_once(path: &Path, release: bool) -> BuildOutcome {
    let frontend = pipeline::analyze(path, Profile::Core, false);
    if !frontend.success() {
        eprint!("{}", pipeline::render_diags(&frontend));
        return BuildOutcome::Failed(util::EXIT_FAIL);
    }
    let options = BuildOptions {
        release,
        ..BuildOptions::default()
    };
    match pipeline::compile_program(&frontend, &options) {
        Ok(artifacts) => BuildOutcome::Ok(artifacts.binary),
        Err(error) => {
            error.print();
            BuildOutcome::Failed(error.exit_code())
        }
    }
}

fn spawn_child(root: &Path, binary: &Path) -> Option<Child> {
    let mut command = Command::new(binary);
    command.current_dir(root);
    match command.spawn() {
        Ok(child) => {
            say(&format!("[ravum] running {}", util::pretty_path(binary)));
            Some(child)
        }
        Err(error) => {
            say(&format!("[ravum] run failed: {error}"));
            None
        }
    }
}

fn stop_child(child: &mut Option<Child>) {
    if let Some(mut child) = child.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// Give a short-lived child a chance to finish and print its output, then kill it.
fn finish_child(child: &mut Option<Child>) {
    if let Some(mut child) = child.take() {
        let deadline = Instant::now() + CHILD_GRACE;
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => {
                    if Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => break,
            }
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn watch_root(path: &Path) -> Result<PathBuf, String> {
    let canonical = fs::canonicalize(path)
        .map_err(|error| format!("cannot resolve `{}`: {error}", path.display()))?;
    if canonical.is_file() {
        return canonical
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| format!("`{}` has no parent directory", canonical.display()));
    }
    if canonical.is_dir() {
        return Ok(canonical);
    }
    Err(format!(
        "`{}` is neither a file nor a directory",
        path.display()
    ))
}

#[derive(Debug, PartialEq, Eq)]
struct Stamp {
    modified: Option<SystemTime>,
    len: u64,
}

/// Recursive snapshot of the watched files (mtime + size). Symlinks are not
/// followed, matching the loader policy.
fn scan(root: &Path) -> BTreeMap<PathBuf, Stamp> {
    let mut snapshot = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if matches!(name.as_str(), ".ravum" | "target" | ".git") {
                continue;
            }
            let path = entry.path();
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                stack.push(path);
                continue;
            }
            if !meta.is_file() || name.ends_with(".ravum-tmp") {
                continue;
            }
            let watched = name == "ravum.toml"
                || name == "ravum.lock"
                || path.extension().and_then(|ext| ext.to_str()) == Some("rv");
            if watched {
                snapshot.insert(
                    path,
                    Stamp {
                        modified: meta.modified().ok(),
                        len: meta.len(),
                    },
                );
            }
        }
    }
    snapshot
}

fn say(line: &str) {
    println!("{line}");
    let _ = std::io::stdout().flush();
}

// ---------------------------------------------------------------------------
// Ctrl+C handling (no external crate)
//
// Two small FFI bindings, both audited here: on Windows the process installs a
// console control handler that only stores `true` in an atomic; on Unix a
// SIGINT handler does the same. The handler never allocates, locks or calls
// back into Rust code; the poll loop performs the cleanup.
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod signal {
    use super::STOP;
    use std::sync::atomic::Ordering;

    type HandlerRoutine = unsafe extern "system" fn(u32) -> i32;

    unsafe extern "system" {
        fn SetConsoleCtrlHandler(handler: Option<HandlerRoutine>, add: i32) -> i32;
    }

    unsafe extern "system" fn handler(_control_type: u32) -> i32 {
        STOP.store(true, Ordering::SeqCst);
        1 // TRUE: handled; the poll loop exits and reaps the child.
    }

    pub fn install() {
        unsafe {
            SetConsoleCtrlHandler(Some(handler), 1);
        }
    }
}

#[cfg(not(windows))]
mod signal {
    use super::STOP;
    use std::sync::atomic::Ordering;

    unsafe extern "C" {
        fn signal(sig: i32, handler: usize) -> usize;
    }

    unsafe extern "C" fn handler(_sig: i32) {
        STOP.store(true, Ordering::SeqCst);
    }

    pub fn install() {
        unsafe {
            signal(2, handler as usize); // SIGINT
        }
    }
}
