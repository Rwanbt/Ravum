//! P0-D — `ravum-lang run` must never turn a program failure into exit 0.
//!
//! The defect this file pins down: `ExitStatus::code()` is a **signed** `i32`
//! on Windows, so an exception such as an access violation arrives as
//! `-1073741819` (`0xC0000005`). `code.clamp(0, 255)` used to turn that into
//! `0` — a crash reported as success. Every case below asserts the exact exit
//! code and, for statuses that cannot be forwarded verbatim, the explanation on
//! stderr.
//!
//! Hermetic strategy: a Ravum program can choose any `i32` for
//! `std::process::exit`. Exiting with a negative value produces the same
//! process status a Windows structured exception produces, so the mapping is
//! exercised end to end through the real binary without `unsafe` and without
//! depending on a particular crash. A genuine access violation is documented in
//! `crates/ravum-lang-cli/README.md`'s manual reproduction note instead; it
//! cannot be produced from safe Core Ravum source.
//!
//! Platform split: a status above `255` is observable only where the parent sees
//! the full 32-bit value (Windows). POSIX truncates in `_exit`/`waitpid`, so the
//! tests below come in pairs — the Windows half asserts the explanation, the
//! Unix half asserts that the CLI forwards what the kernel reports and invents
//! nothing. The first Linux CI run failed here because the original single test
//! assumed the Windows observation everywhere; that was a wrong claim about the
//! platform, not a CLI defect, and the pair fixes it without weakening either
//! half.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

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
            "ravum-cli-exit-{tag}-{}-{}-{nanos}",
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
        // Best effort: a failed watch child could still hold a file on Windows.
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

fn cli_with_env(args: &[&str], key: &str, value: &str) -> Output {
    Command::new(binary())
        .args(args)
        .env(key, value)
        .output()
        .expect("run ravum-lang")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn rustc_available() -> bool {
    Command::new("rustc")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Create a package and write `src/main.rv`. `init` is used so the manifest and
/// lockfile follow the real layout.
fn package(temp: &TempDir, name: &str, source: &str) -> PathBuf {
    let root = temp.path().join(name);
    let output = cli(&["init", root.to_str().unwrap()]);
    assert!(output.status.success(), "init failed: {}", stderr(&output));
    fs::write(root.join("src/main.rv"), source).expect("write main.rv");
    root
}

fn skip_without_rustc(test: &str) -> bool {
    if rustc_available() {
        return false;
    }
    eprintln!("SKIP {test}: rustc is not installed (the CLI cannot compile)");
    true
}

#[test]
fn run_forwards_normal_exit_codes() {
    if skip_without_rustc("run_forwards_normal_exit_codes") {
        return;
    }
    let temp = TempDir::new("normal");
    let root = package(
        &temp,
        "codes",
        "use std::io;\nfn main() { io::println(\"ok\"); }\n",
    );

    let output = cli(&["run", root.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert!(stdout(&output).contains("ok"));

    for code in [1, 7, 42, 255] {
        fs::write(
            root.join("src/main.rv"),
            format!("use std::process;\nfn main() {{ process::exit({code}); }}\n"),
        )
        .unwrap();
        let output = cli(&["run", root.to_str().unwrap()]);
        assert_eq!(
            output.status.code(),
            Some(code),
            "program exit {code} must pass through; stderr: {}",
            stderr(&output)
        );
    }
}

/// What `ravum-lang run` can say about a status above `255` depends on what the
/// operating system hands to the parent, and that is measurably different:
///
/// * Windows keeps the full 32-bit value, so the CLI sees `300` and must both
///   forward `44` and explain the truncation (or a `256` would silently become
///   the `0` of success);
/// * POSIX truncates in `_exit`/`waitpid`, so the CLI only ever sees `44` — or
///   `0` for `256`. Naming `300` there would be an invention, not an
///   observation, so the test below pins the opposite behaviour: forward what
///   the OS reports and stay silent about a value that was never observed.
///
/// The POSIX case is a real limitation of the platform, not of the CLI: a
/// program exiting `256` is indistinguishable from success for every parent
/// process on Linux and macOS. It is documented in `crates/ravum-lang-cli/README.md`.
#[cfg(windows)]
#[test]
fn run_explains_windows_truncation_of_large_codes() {
    if skip_without_rustc("run_explains_windows_truncation_of_large_codes") {
        return;
    }
    let temp = TempDir::new("truncate");
    let root = package(
        &temp,
        "big",
        "use std::process;\nfn main() { process::exit(300); }\n",
    );

    let output = cli(&["run", root.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(44),
        "300 & 0xFF == 44; stderr: {}",
        stderr(&output)
    );
    let err = stderr(&output);
    assert!(
        err.contains("exited with status 300"),
        "the original status must be named: {err}"
    );
    assert!(
        err.contains("reporting 44"),
        "the forwarded status must be named: {err}"
    );
}

#[cfg(unix)]
#[test]
fn run_forwards_the_status_posix_actually_reports() {
    if skip_without_rustc("run_forwards_the_status_posix_actually_reports") {
        return;
    }
    let temp = TempDir::new("big");
    let root = package(
        &temp,
        "big",
        "use std::process;\nfn main() { process::exit(300); }\n",
    );

    let output = cli(&["run", root.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(44),
        "300 & 0xFF == 44; stderr: {}",
        stderr(&output)
    );
    // The parent never observes 300, so any note about it would be invented. The
    // assertions target the note's wording: `run` always echoes `running: <path>`
    // on stderr, and a temporary directory called `truncate*` would defeat a
    // substring test on "truncat" (that is exactly why the first Linux run of
    // this test failed).
    let err = stderr(&output);
    assert!(
        !err.contains("exited with status") && !err.contains("POSIX truncates"),
        "the parent never observes 300 on POSIX, so naming it would be a guess: {err}"
    );
}

#[cfg(windows)]
#[test]
fn run_never_reports_a_truncated_failure_as_success() {
    if skip_without_rustc("run_never_reports_a_truncated_failure_as_success") {
        return;
    }
    let temp = TempDir::new("truncate-zero");
    let root = package(
        &temp,
        "wraps",
        "use std::process;\nfn main() { process::exit(256); }\n",
    );

    let output = cli(&["run", root.to_str().unwrap()]);
    // 256 & 0xFF == 0. A POSIX shell would see 0; on Windows this CLI must not
    // claim success for a status the kernel shows in full.
    assert_eq!(
        output.status.code(),
        Some(1),
        "exit 256 must not become success; stderr: {}",
        stderr(&output)
    );
    let err = stderr(&output);
    assert!(
        err.contains("exited with status 256"),
        "the original status must be named: {err}"
    );
    assert!(
        err.contains("reporting exit code 1 instead"),
        "the refusal to report success must be explained: {err}"
    );
}

#[cfg(unix)]
#[test]
fn posix_cannot_distinguish_exit_256_from_success() {
    if skip_without_rustc("posix_cannot_distinguish_exit_256_from_success") {
        return;
    }
    let temp = TempDir::new("wraps");
    let root = package(
        &temp,
        "wraps",
        "use std::process;\nfn main() { process::exit(256); }\n",
    );

    // `_exit(256)` is `_exit(0)`: the kernel discards the high bits before
    // `waitpid` can see them, so every parent on POSIX observes success. The CLI
    // forwards the observed status (0) and must not fabricate a truncation note.
    let output = cli(&["run", root.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "the kernel reports 0 for `_exit(256)`; stderr: {}",
        stderr(&output)
    );
    let err = stderr(&output);
    assert!(
        !err.contains("exited with status") && !err.contains("POSIX truncates"),
        "the CLI cannot know about 256 on POSIX, so it must say nothing: {err}"
    );
}

#[cfg(windows)]
#[test]
fn run_reports_windows_exception_codes() {
    if skip_without_rustc("run_reports_windows_exception_codes") {
        return;
    }
    let temp = TempDir::new("exception");
    let root = package(
        &temp,
        "crash",
        "use std::process;\nfn main() { process::exit(-1073741819); }\n",
    );

    let output = cli(&["run", root.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "an access-violation status must be a failure; stderr: {}",
        stderr(&output)
    );
    let err = stderr(&output);
    assert!(err.contains("0xC0000005"), "exact exception code: {err}");
    assert!(err.contains("access violation"), "explanation: {err}");

    // 0xC000013A = STATUS_CONTROL_C_EXIT: the child was interrupted, not done.
    fs::write(
        root.join("src/main.rv"),
        "use std::process;\nfn main() { process::exit(-1073741510); }\n",
    )
    .unwrap();
    let output = cli(&["run", root.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1), "stderr: {}", stderr(&output));
    assert!(
        stderr(&output).contains("0xC000013A"),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn run_compiler_refusal_exits_one_before_running() {
    let temp = TempDir::new("refusal");
    // `analyze` refuses before rustc is needed: this test works without a toolchain.
    let root = package(&temp, "bad", "fn main( {\n");

    let output = cli(&["run", root.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1), "stderr: {}", stderr(&output));
    let err = stderr(&output);
    assert!(err.contains("not executed"), "stderr: {err}");
    assert!(
        !err.contains("running:") && !stdout(&output).contains("running:"),
        "a refused program must never be executed"
    );
}

#[test]
fn run_missing_toolchain_exits_three() {
    let temp = TempDir::new("tool-missing");
    let root = package(&temp, "app", "fn main() {}\n");

    let output = cli_with_env(
        &["run", root.to_str().unwrap()],
        "RAVUM_RUSTC",
        "ravum-no-such-rustc-9f13",
    );
    assert_eq!(
        output.status.code(),
        Some(3),
        "a missing external tool is exit 3, not 1; stderr: {}",
        stderr(&output)
    );
    let err = stderr(&output);
    assert!(
        err.contains("ravum-no-such-rustc-9f13"),
        "the missing command must be named: {err}"
    );
}
