//! P0-C — `watch --run` must keep the previous version running until a new one
//! is ready, and must never activate a stale build.
//!
//! What these tests prove end to end, through the real binary:
//!
//! 1. **Continuity across a failed build.** With a long-running child (it blocks
//!    on stdin), an edit that does not compile prints diagnostics and
//!    `build failed (exit 1)`, creates no new child and keeps the old one alive.
//!    On Windows the old child provably still holds its staged executable (a
//!    running image cannot be deleted), which is the strongest available
//!    evidence without inspecting the process table.
//! 2. **No stale activation.** A slow `RAVUM_RUSTC` wrapper (a real helper
//!    executable that sleeps before invoking rustc) stretches the build; the
//!    source is edited again while that build runs. The build result is
//!    discarded and rebuilt, the intermediate version is never started, and the
//!    final version is the one that runs.
//! 3. **Cleanup on every exercised exit path.** After `--max-rebuilds` the child
//!    is reaped and `.ravum/watch/` is removed (no orphan process directory);
//!    `--max-rebuilds 0` builds once, starts nothing without `--run`, and leaves
//!    no staging directory.
//!
//! Not covered here (stated honestly): the Ctrl+C handler cannot be delivered
//! portably to a spawned child in an integration test. `SIGKILL`/termination
//! cannot run any cleanup by definition. Both are documented in
//! `docs/CLI-WATCH-WASM.md`.
//!
//! Every wait is event-based with a generous deadline; there is no
//! `sleep`-then-assert. Temp dirs are removed on drop.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "ravum-cli-watch-{tag}-{}-{}-{nanos}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_ravum-lang")
}

fn cli(args: &[&str]) -> Output {
    Command::new(binary())
        .args(args)
        .output()
        .expect("run ravum-lang")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

/// A spawned `ravum-lang watch` whose stdin pipe is kept open (so a child that
/// blocks on `io::read_line` stays alive) and whose stdout/stderr are captured
/// line by line with a shared log for absence assertions.
struct Watch {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    lines: Arc<Mutex<Vec<String>>>,
    stderr_lines: Arc<Mutex<Vec<String>>>,
    receiver: mpsc::Receiver<String>,
}

impl Watch {
    fn spawn(root: &Path, args: &[&str], envs: &[(&str, &str)]) -> Self {
        let mut command = Command::new(binary());
        command
            .args(args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in envs {
            command.env(key, value);
        }
        let mut child = command.spawn().expect("spawn ravum-lang watch");
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().expect("watch stdout");
        let stderr_handle = child.stderr.take().expect("watch stderr");
        let lines = Arc::new(Mutex::new(Vec::new()));
        let stderr_lines = Arc::new(Mutex::new(Vec::new()));
        let (sender, receiver) = mpsc::channel();
        {
            let lines = Arc::clone(&lines);
            thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if let Ok(mut log) = lines.lock() {
                        log.push(line.clone());
                    }
                    if sender.send(line).is_err() {
                        break;
                    }
                }
            });
        }
        {
            let stderr_lines = Arc::clone(&stderr_lines);
            thread::spawn(move || {
                for line in BufReader::new(stderr_handle).lines().map_while(Result::ok) {
                    eprintln!("[watch stderr] {line}");
                    if let Ok(mut log) = stderr_lines.lock() {
                        log.push(line);
                    }
                }
            });
        }
        Self {
            child: Some(child),
            stdin,
            lines,
            stderr_lines,
            receiver,
        }
    }

    fn log(&self) -> Vec<String> {
        self.lines.lock().map(|log| log.clone()).unwrap_or_default()
    }

    fn stderr_log(&self) -> Vec<String> {
        self.stderr_lines
            .lock()
            .map(|log| log.clone())
            .unwrap_or_default()
    }

    fn fail(&self, what: &str) -> ! {
        panic!(
            "timeout waiting for {what}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            self.log().join("\n"),
            self.stderr_log().join("\n")
        )
    }

    fn wait_line(
        &mut self,
        what: &str,
        timeout: Duration,
        predicate: impl Fn(&str) -> bool,
    ) -> String {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                self.fail(what);
            }
            match self.receiver.recv_timeout(remaining) {
                Ok(line) => {
                    if predicate(&line) {
                        return line;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => self.fail(what),
                Err(mpsc::RecvTimeoutError::Disconnected) => self.fail(what),
            }
        }
    }

    /// Read lines for a bounded window; returns the first match, if any.
    fn wait_line_within(
        &mut self,
        window: Duration,
        predicate: impl Fn(&str) -> bool,
    ) -> Option<String> {
        let deadline = Instant::now() + window;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            match self.receiver.recv_timeout(remaining) {
                Ok(line) => {
                    if predicate(&line) {
                        return Some(line);
                    }
                }
                Err(_) => return None,
            }
        }
    }

    fn wait_exit(&mut self, timeout: Duration) -> Option<i32> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(child) = self.child.as_mut() {
                match child.try_wait().expect("try_wait on watch") {
                    Some(status) => {
                        self.child.take();
                        return status.code();
                    }
                    None => {}
                }
            } else {
                return None;
            }
            if Instant::now() >= deadline {
                return None;
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn count_lines(&self, needle: &str) -> usize {
        self.log()
            .iter()
            .filter(|line| line.contains(needle))
            .count()
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        // Close the stdin pipe first so a blocked child can exit, then reap.
        self.stdin.take();
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn rustc_available() -> bool {
    Command::new("rustc")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn skip_without_rustc(test: &str) -> bool {
    if rustc_available() {
        return false;
    }
    eprintln!("SKIP {test}: rustc is not installed (watch cannot build)");
    true
}

fn package(temp: &TempDir, name: &str) -> PathBuf {
    let root = temp.path().join(name);
    let output = cli(&["init", root.to_str().unwrap()]);
    assert!(output.status.success(), "init failed: {}", stderr(&output));
    root
}

/// A program that announces its version and then blocks on stdin, so it stays
/// alive until watch kills it.
fn blocking_program(version: &str) -> String {
    format!(
        "use std::io;\nfn main() {{\n    io::println(\"{version}-start\");\n    \
         let line = io::read_line();\n}}\n"
    )
}

fn write_source(root: &Path, source: &str) {
    fs::write(root.join("src/main.rv"), source).expect("write main.rv");
}

/// Executables staged under `.ravum/watch/<pid>/`.
fn staged_copies(root: &Path) -> Vec<PathBuf> {
    let mut copies = Vec::new();
    let Ok(sessions) = fs::read_dir(root.join(".ravum/watch")) else {
        return copies;
    };
    for session in sessions.flatten() {
        let path = session.path();
        if !path.is_dir() {
            continue;
        }
        if let Ok(files) = fs::read_dir(&path) {
            for file in files.flatten() {
                if file.path().is_file() {
                    copies.push(file.path());
                }
            }
        }
    }
    copies.sort();
    copies
}

fn wait_for_copies(root: &Path, expected: usize, timeout: Duration) -> Vec<PathBuf> {
    let deadline = Instant::now() + timeout;
    loop {
        let copies = staged_copies(root);
        if copies.len() == expected {
            return copies;
        }
        if Instant::now() >= deadline {
            panic!(
                "expected {expected} staged copy/copies under {}/.ravum/watch, found {copies:?}",
                root.display()
            );
        }
        thread::sleep(Duration::from_millis(20));
    }
}

/// On Windows a running executable cannot be deleted, so a failed removal proves
/// the child that owns it is still alive.
#[cfg(windows)]
fn assert_child_still_holds(path: &Path, context: &str) {
    match fs::remove_file(path) {
        Ok(()) => panic!(
            "{context}: the staged copy {} could be deleted, so no child is holding it \
             (the process is gone)",
            path.display()
        ),
        Err(_) => {}
    }
}

#[cfg(not(windows))]
fn assert_child_still_holds(path: &Path, context: &str) {
    // Unix allows unlinking a running executable; there is no portable,
    // dependency-free way to prove liveness from the test process.
    eprintln!(
        "NOTE {context}: deferred liveness proof for {} (Unix permits unlinking a running image)",
        path.display()
    );
}

#[test]
fn failed_build_keeps_the_previous_version_running_and_cleans_up() {
    if skip_without_rustc("failed_build_keeps_the_previous_version_running_and_cleans_up") {
        return;
    }
    let temp = TempDir::new("continuity");
    let root = package(&temp, "app");
    write_source(&root, &blocking_program("v1"));

    let mut watch = Watch::spawn(&root, &["watch", ".", "--run", "--max-rebuilds", "2"], &[]);
    let long = Duration::from_secs(120);

    watch.wait_line("watching", long, |line| line.contains("[ravum] watching"));
    watch.wait_line("initial build ok", long, |line| line == "[ravum] build ok");
    watch.wait_line("child spawn", long, |line| line.contains("[ravum] running"));
    watch.wait_line("v1 output", long, |line| line == "v1-start");

    let copies = wait_for_copies(&root, 1, Duration::from_secs(30));
    let running_copy = copies[0].clone();
    assert_child_still_holds(&running_copy, "before the failed build");

    // An edit that does not compile must not interrupt the working program.
    write_source(&root, "fn main( {\n");
    watch.wait_line("change detected", long, |line| {
        line.contains("[ravum] change detected")
    });
    watch.wait_line("build failed", long, |line| {
        line.contains("[ravum] build failed (exit 1)")
    });

    // No new child was started, a new copy was not staged, and the old child
    // still holds its executable.
    assert_eq!(
        watch.count_lines("[ravum] running"),
        1,
        "a failed build must not restart anything; log:\n{}",
        watch.log().join("\n")
    );
    let copies_after_failure = staged_copies(&root);
    assert_eq!(
        copies_after_failure, copies,
        "a failed build must not stage a new artifact"
    );
    assert_child_still_holds(&running_copy, "after the failed build");

    // Fixing the source activates v2 and the loop ends after the 2nd rebuild.
    write_source(&root, &blocking_program("v2"));
    watch.wait_line("second change", long, |line| {
        line.contains("[ravum] change detected")
    });
    watch.wait_line("recovery build ok", long, |line| line == "[ravum] build ok");
    watch.wait_line("v2 output", long, |line| line == "v2-start");

    assert_eq!(
        watch.wait_exit(long),
        Some(0),
        "watch must exit 0 after --max-rebuilds; stderr:\n{}",
        watch.stderr_log().join("\n")
    );

    // The child was reaped and the session staging directory removed.
    assert!(
        !root.join(".ravum/watch").exists(),
        "orphan watch staging directory survived: {:?}",
        staged_copies(&root)
    );
    assert_eq!(
        watch.count_lines("[ravum] change detected"),
        2,
        "each edit must trigger exactly one rebuild event; log:\n{}",
        watch.log().join("\n")
    );
}

#[test]
fn a_change_during_the_build_is_discarded_and_never_activated() {
    if skip_without_rustc("a_change_during_the_build_is_discarded_and_never_activated") {
        return;
    }
    let temp = TempDir::new("stale");
    let Some(slow_rustc) = build_slow_rustc(&temp) else {
        eprintln!("SKIP a_change_during_the_build_is_discarded_and_never_activated: cannot build the slow-rustc helper");
        return;
    };
    let helper = slow_rustc
        .to_str()
        .expect("helper path is UTF-8")
        .to_string();
    let root = package(&temp, "app");
    write_source(&root, &blocking_program("v1"));

    let mut watch = Watch::spawn(
        &root,
        &["watch", ".", "--run", "--max-rebuilds", "1"],
        &[("RAVUM_RUSTC", helper.as_str())],
    );
    let long = Duration::from_secs(180);

    watch.wait_line("watching", long, |line| line.contains("[ravum] watching"));
    watch.wait_line("initial build ok", long, |line| line == "[ravum] build ok");
    watch.wait_line("v1 output", long, |line| line == "v1-start");

    // v2 is written, then v3 while the (slow) v2 build is still compiling.
    write_source(&root, &blocking_program("v2"));
    watch.wait_line("change detected", long, |line| {
        line.contains("[ravum] change detected")
    });

    // Keep modifying the source until the runner reports that it discarded a
    // build; this makes the stale-detection path deterministic instead of
    // depending on a scheduling race. Each write changes the length, so it is
    // always a visible change.
    let mut discarded = false;
    for attempt in 0..80 {
        write_source(
            &root,
            &format!("{}// attempt {attempt}\n", blocking_program("v3")),
        );
        if watch
            .wait_line_within(Duration::from_millis(300), |line| {
                line.contains("discarding that result")
            })
            .is_some()
        {
            discarded = true;
            break;
        }
    }
    assert!(
        discarded,
        "the runner never reported a stale build being discarded; log:\n{}",
        watch.log().join("\n")
    );

    watch.wait_line("final build ok", long, |line| line == "[ravum] build ok");
    watch.wait_line("v3 output", long, |line| line == "v3-start");

    assert_eq!(
        watch.wait_exit(long),
        Some(0),
        "watch must exit 0 after --max-rebuilds; stderr:\n{}",
        watch.stderr_log().join("\n")
    );

    // INV-07: the build superseded while compiling was never activated.
    assert!(
        !watch.log().iter().any(|line| line == "v2-start"),
        "the superseded version was activated; log:\n{}",
        watch.log().join("\n")
    );
    assert!(
        !root.join(".ravum/watch").exists(),
        "orphan watch staging directory survived"
    );
}

#[test]
fn max_rebuilds_zero_builds_once_and_leaves_no_staging_directory() {
    if skip_without_rustc("max_rebuilds_zero_builds_once_and_leaves_no_staging_directory") {
        return;
    }
    let temp = TempDir::new("zero");
    let root = package(&temp, "app");
    write_source(&root, &blocking_program("v1"));

    let output = cli(&["watch", root.to_str().unwrap(), "--max-rebuilds", "0"]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    assert!(text.contains("[ravum] build ok"), "stdout: {text}");
    assert!(
        !text.contains("[ravum] running"),
        "without --run no child may be started; stdout: {text}"
    );
    assert!(
        !root.join(".ravum/watch").exists(),
        "no staging directory may be created without a child"
    );
}

/// Build a helper executable that sleeps before delegating to the real `rustc`.
/// It is used as `RAVUM_RUSTC` for one watch process only, so other tests and
/// builds are unaffected; no global environment variable is mutated.
fn build_slow_rustc(temp: &TempDir) -> Option<PathBuf> {
    let real = find_rustc()?;
    let dir = temp.path().join("slow-rustc");
    fs::create_dir_all(&dir).ok()?;
    let source = format!(
        r#"fn main() {{
    std::thread::sleep(std::time::Duration::from_millis(2000));
    let status = std::process::Command::new({real:?})
        .args(std::env::args_os().skip(1))
        .status()
        .expect("run the real rustc");
    std::process::exit(status.code().unwrap_or(1));
}}
"#,
        real = real.to_string_lossy().to_string()
    );
    let source_path = dir.join("slow_rustc.rs");
    fs::write(&source_path, source).ok()?;
    let helper = dir.join(if cfg!(windows) {
        "slow-rustc.exe"
    } else {
        "slow-rustc"
    });
    let output = Command::new(&real)
        .arg("--edition")
        .arg("2021")
        .arg("-o")
        .arg(&helper)
        .arg(&source_path)
        .output()
        .ok()?;
    if !output.status.success() {
        eprintln!(
            "slow-rustc helper failed to build: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return None;
    }
    Some(helper)
}

fn find_rustc() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("RAVUM_RUSTC") {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let finder = if cfg!(windows) { "where" } else { "which" };
    let output = Command::new(finder).arg("rustc").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let first = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()?
        .trim()
        .to_string();
    let candidate = PathBuf::from(first);
    if candidate.is_file() {
        Some(candidate)
    } else {
        None
    }
}
