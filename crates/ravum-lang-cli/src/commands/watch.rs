//! `ravum-lang watch` (alias `dev`) — poll, rebuild, optionally restart.
//!
//! Polling only (no external crate): `.rv`, `ravum.toml` and `ravum.lock` under
//! the watched directory, every 250 ms. `.ravum/`, `target/`, `.git/` and
//! `*.ravum-tmp` are ignored.
//!
//! # Fail-closed activation (INV-03, INV-07)
//!
//! This is a **restart loop with continuity of service**, not hot reload and
//! not a state-preserving swap:
//!
//! * a change triggers a build **before the running child is touched**; the old
//!   version keeps running while the new one compiles;
//! * the built artifact is activated only when the source snapshot that produced
//!   it is still the newest one. Two independent guards enforce this: the
//!   pipeline re-reads the sources and compares content digests before publishing
//!   (`BuildError::Stale`), and watch re-scans the watched files immediately
//!   before activation. A source edit during the build discards the result and
//!   rebuilds it;
//! * a failed build (diagnostics, `rustc` failure, missing tool) keeps the old
//!   child alive and polling continues — a broken edit never interrupts a
//!   working program;
//! * a new child is spawned **before** the old one is stopped, so a failed
//!   start falls back to the previous version instead of leaving nothing
//!   running; the last known-good artifact is kept for that fallback;
//! * every exit path (`Ctrl+C`, `--max-rebuilds`, initial-build-only, errors)
//!   kills and reaps the child, and removes the per-session artifact copies.
//!
//! # Why the child runs a copy
//!
//! Windows locks a running executable against overwrite: `rustc`/`link` cannot
//! rebuild `.ravum/bin/<name>.exe` while that image is executing
//! (`LNK1104: cannot open file`), and the file cannot even be deleted. Building
//! before killing the child therefore requires the child to run a private copy
//! of the artifact. Watch copies the freshly built binary to
//! `.ravum/watch-<pid>/<name>-<n>[.exe]` and runs that copy; the pipeline's
//! stable output path stays free for the next build. When the pipeline later
//! makes artifacts immutable/per-build, the copy remains harmless.
//!
//! # Exit codes
//!
//! `0` on `Ctrl+C`/`--max-rebuilds` with no leaked child; `1` when `PATH` itself
//! is invalid (the loop never starts); `3` is *reported* as a build failure when
//! `rustc` disappears mid-watch, but the loop keeps running.

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
/// Consecutive builds that may be discarded because the sources changed while
/// compiling. After this many attempts watch waits for the next change instead
/// of spinning on a continuously rewritten file.
const MAX_STALE_REBUILDS: u32 = 16;

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

    let mut runner = Runner::new(&root);
    let mut pending_change;

    // Initial build: no previous version exists, so a build whose snapshot was
    // already superseded is discarded and retried. If the sources never settle,
    // nothing is started and the poll loop retries on the next change.
    let mut attempt = build_guarded(path, release, &root);
    let mut stale = 0u32;
    while needs_rebuild(&attempt) && stale < MAX_STALE_REBUILDS {
        stale += 1;
        say("[ravum] sources changed during the initial build; rebuilding");
        attempt = build_guarded(path, release, &root);
    }
    let initial_stable = attempt.stable() && !is_stale(&attempt);
    let mut snapshot = next_snapshot(&attempt, &root);
    match attempt.outcome {
        BuildOutcome::Ok(binary) if initial_stable && still_current(&attempt, &root) => {
            say("[ravum] build ok");
            if run_child {
                runner.activate(&root, &binary);
            }
            pending_change = false;
        }
        BuildOutcome::Failed(code) => {
            say(&format!("[ravum] build failed (exit {code})"));
            pending_change = false;
        }
        _ => {
            say(
                "[ravum] the initial build was superseded while compiling; it is not activated \
                 (waiting for a stable build)",
            );
            pending_change = true;
        }
    }

    if max_rebuilds == Some(0) {
        runner.shutdown(true);
        return util::EXIT_OK;
    }

    let mut rebuilds: u64 = 0;
    loop {
        if stop_requested() {
            runner.shutdown(false);
            say("[ravum] stopped");
            return util::EXIT_OK;
        }
        if pending_change {
            pending_change = false;
        } else {
            std::thread::sleep(POLL_INTERVAL);
            if stop_requested() {
                runner.shutdown(false);
                say("[ravum] stopped");
                return util::EXIT_OK;
            }
            let current = scan(&root);
            if current == snapshot {
                continue;
            }
        }
        say("[ravum] change detected");

        // Build first, without touching the running child.
        let mut attempt = build_guarded(path, release, &root);
        let mut stale = 0u32;
        while needs_rebuild(&attempt) && stale < MAX_STALE_REBUILDS {
            stale += 1;
            say("[ravum] source changed during the build; discarding that result and rebuilding");
            attempt = build_guarded(path, release, &root);
        }
        let stable = attempt.stable() && !is_stale(&attempt);
        // Absorb the build's own side effects (`ravum.lock`) *after* recording
        // the source snapshot, so our own write never triggers another rebuild
        // while a change that arrived after the last source scan still does.
        snapshot = next_snapshot(&attempt, &root);
        match attempt.outcome {
            BuildOutcome::Ok(binary) if stable && still_current(&attempt, &root) => {
                say("[ravum] build ok");
                if run_child {
                    runner.activate(&root, &binary);
                }
            }
            BuildOutcome::Failed(code) => {
                say(&format!("[ravum] build failed (exit {code})"));
            }
            _ => {
                if stable {
                    // The build itself was consistent, but a newer edit landed
                    // between the final scan and this point: rebuild immediately.
                    say(
                        "[ravum] the sources changed again before activation; this build was not \
                         activated (the previous version, if any, keeps running)",
                    );
                } else {
                    say(
                        "[ravum] the sources are still changing; this build was not activated \
                         (the previous version, if any, keeps running)",
                    );
                }
                // Retry without waiting for the next poll tick: the last edit
                // must be built once the sources settle.
                pending_change = true;
            }
        }
        rebuilds = rebuilds.saturating_add(1);

        if max_rebuilds.map(|max| rebuilds >= max).unwrap_or(false) {
            runner.shutdown(true);
            return util::EXIT_OK;
        }
    }
}

fn stop_requested() -> bool {
    STOP.load(Ordering::SeqCst)
}

enum BuildOutcome {
    Ok(PathBuf),
    /// The pipeline refused to publish because the sources changed while rustc
    /// ran (`pipeline::BuildError::Stale`): a discarded result, not a user error.
    Stale,
    Failed(i32),
}

/// `true` when the pipeline itself rejected the build as stale (content-digest
/// guard), independently of watch's own mtime/size snapshot.
fn is_stale(attempt: &BuildAttempt) -> bool {
    matches!(attempt.outcome, BuildOutcome::Stale)
}

fn needs_rebuild(attempt: &BuildAttempt) -> bool {
    !attempt.stable() || is_stale(attempt)
}

/// One build plus the source snapshots around it, used to reject stale results.
struct BuildAttempt {
    outcome: BuildOutcome,
    /// Watched sources (`ravum.lock` excluded) just before compiling.
    before: BTreeMap<PathBuf, Stamp>,
    /// Watched sources (`ravum.lock` excluded) just after compiling.
    after: BTreeMap<PathBuf, Stamp>,
}

impl BuildAttempt {
    /// `true` when no source changed while this build was running, i.e. the
    /// artifact corresponds to the newest source snapshot.
    fn stable(&self) -> bool {
        !sources_changed(&self.before, &self.after)
    }
}

fn build_guarded(path: &Path, release: bool, root: &Path) -> BuildAttempt {
    let before = source_snapshot(scan(root));
    let outcome = build_once(path, release);
    let after = source_snapshot(scan(root));
    BuildAttempt {
        outcome,
        before,
        after,
    }
}

/// Final gate immediately before activation: the sources must still match the
/// snapshot the build stabilized on. A newer edit discards the build.
fn still_current(attempt: &BuildAttempt, root: &Path) -> bool {
    !sources_changed(&attempt.after, &source_snapshot(scan(root)))
}

/// The snapshot the poll loop compares against: the stabilized source snapshot
/// of the build, plus the current `ravum.lock` state (which the build itself may
/// have rewritten, so it must never count as a user change).
fn next_snapshot(attempt: &BuildAttempt, root: &Path) -> BTreeMap<PathBuf, Stamp> {
    merge_lockfile_state(attempt.after.clone(), &scan(root))
}

/// Add the current `ravum.lock` entries to a stabilized source snapshot, so the
/// build's own lockfile rewrite is absorbed while any later lockfile change by
/// an external tool still triggers a rebuild.
fn merge_lockfile_state(
    mut snapshot: BTreeMap<PathBuf, Stamp>,
    full: &BTreeMap<PathBuf, Stamp>,
) -> BTreeMap<PathBuf, Stamp> {
    for (path, stamp) in full {
        if is_lockfile(path) {
            snapshot.insert(path.clone(), stamp.clone());
        }
    }
    snapshot
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
        Err(pipeline::BuildError::Stale(message)) => {
            // P0-B re-reads the sources before publishing; a difference means the
            // artifact no longer matches the disk. Watch treats it as a discarded
            // result and rebuilds, not as a diagnostic for the user.
            say(&format!("[ravum] build rejected as stale: {message}"));
            BuildOutcome::Stale
        }
        Err(error) => {
            error.print();
            BuildOutcome::Failed(error.exit_code())
        }
    }
}

// ---------------------------------------------------------------------------
// Child lifecycle (versioned copies, spawn-before-kill, fallback, cleanup)
// ---------------------------------------------------------------------------

struct Runner {
    /// Per-session directory holding the artifact copies this run executes.
    dir: PathBuf,
    child: Option<Child>,
    /// Copy that is currently running (or was last started successfully).
    last_good: Option<PathBuf>,
    counter: u64,
}

impl Runner {
    fn new(root: &Path) -> Self {
        Self {
            dir: root
                .join(crate::util::CACHE_DIR)
                .join("watch")
                .join(std::process::id().to_string()),
            child: None,
            last_good: None,
            counter: 0,
        }
    }

    /// Build a private copy of `artifact` next to the cache (never inside the
    /// watched source tree) and run it.
    fn activate(&mut self, root: &Path, artifact: &Path) {
        self.reap();
        let copy = match self.copy_artifact(artifact) {
            Ok(copy) => copy,
            Err(message) => {
                say(&format!("[ravum] cannot stage the new build: {message}"));
                self.fallback(root);
                return;
            }
        };
        match spawn_child(root, &copy) {
            Some(child) => {
                // The new version is running: only now is the old one stopped.
                self.stop();
                self.child = Some(child);
                let previous = self.last_good.replace(copy);
                if let Some(previous) = previous {
                    let _ = fs::remove_file(previous);
                }
            }
            None => {
                let _ = fs::remove_file(&copy);
                self.fallback(root);
            }
        }
    }

    /// A failed activation must never leave nothing running when a good version
    /// is still available: keep the live child, or restart the last good copy.
    fn fallback(&mut self, root: &Path) {
        self.reap();
        if self.child.is_some() {
            say("[ravum] the previous version keeps running");
            return;
        }
        if let Some(last_good) = self.last_good.clone() {
            say("[ravum] restarting the last known-good build");
            self.child = spawn_child(root, &last_good);
            if self.child.is_some() {
                return;
            }
        }
        say("[ravum] warning: no version is running; the next successful build will start one");
    }

    fn copy_artifact(&mut self, artifact: &Path) -> Result<PathBuf, String> {
        fs::create_dir_all(&self.dir).map_err(|error| {
            format!("cannot create `{}`: {error}", util::pretty_path(&self.dir))
        })?;
        self.counter = self.counter.saturating_add(1);
        let stem = artifact
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_else(|| "program".to_string());
        let extension = artifact
            .extension()
            .map(|extension| format!(".{}", extension.to_string_lossy()))
            .unwrap_or_default();
        let copy = self.dir.join(format!("{stem}-{}{extension}", self.counter));
        fs::copy(artifact, &copy).map_err(|error| {
            format!(
                "cannot copy `{}` to `{}`: {error}",
                util::pretty_path(artifact),
                util::pretty_path(&copy)
            )
        })?;
        // The copy is spawned only if it is a complete artifact: a truncated
        // copy must never be executed (and must never replace the old child).
        let expected = fs::metadata(artifact)
            .map_err(|error| format!("cannot inspect `{}`: {error}", util::pretty_path(artifact)))?
            .len();
        let actual = fs::metadata(&copy)
            .map_err(|error| format!("cannot inspect `{}`: {error}", util::pretty_path(&copy)))?
            .len();
        if expected != actual {
            let _ = fs::remove_file(&copy);
            return Err(format!(
                "staged copy `{}` is {} byte(s), expected {}; refusing to run it",
                util::pretty_path(&copy),
                actual,
                expected
            ));
        }
        Ok(copy)
    }

    /// Drop a child that already exited, so `child.is_some()` means "alive".
    fn reap(&mut self) {
        if let Some(child) = self.child.as_mut() {
            if let Ok(Some(_status)) = child.try_wait() {
                self.child = None;
            }
        }
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Give a short-lived child a chance to finish and print its output, then kill it.
    fn finish(&mut self) {
        if let Some(mut child) = self.child.take() {
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

    /// Every exit path: reap the child, then remove this session's copies and
    /// the shared `watch/` parent when it is left empty.
    fn shutdown(&mut self, grace: bool) {
        if grace {
            self.finish();
        } else {
            self.stop();
        }
        self.last_good = None;
        match fs::remove_dir_all(&self.dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => say(&format!(
                "[ravum] warning: cannot remove `{}`: {error}",
                util::pretty_path(&self.dir)
            )),
        }
        if let Some(parent) = self.dir.parent() {
            // Only succeeds when no other watch session is using the directory.
            let _ = fs::remove_dir(parent);
        }
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        // Safety net on panic/unwind; normal exits call `shutdown`.
        self.stop();
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

// ---------------------------------------------------------------------------
// Filesystem snapshot
// ---------------------------------------------------------------------------

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

#[derive(Debug, Clone, PartialEq, Eq)]
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

fn is_lockfile(path: &Path) -> bool {
    path.file_name()
        .map(|name| name == "ravum.lock")
        .unwrap_or(false)
}

/// Sources only: a build rewrites `ravum.lock`, so it must not be mistaken for
/// a user edit when deciding whether a build result is stale.
fn source_snapshot(snapshot: BTreeMap<PathBuf, Stamp>) -> BTreeMap<PathBuf, Stamp> {
    snapshot
        .into_iter()
        .filter(|(path, _)| !is_lockfile(path))
        .collect()
}

fn sources_changed(before: &BTreeMap<PathBuf, Stamp>, after: &BTreeMap<PathBuf, Stamp>) -> bool {
    before != after
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
#[allow(unsafe_code)]
mod signal {
    use super::STOP;
    use std::sync::atomic::Ordering;

    type HandlerRoutine = unsafe extern "system" fn(u32) -> i32;

    // SAFETY (declaration): `SetConsoleCtrlHandler` is a Win32 API whose
    // documented signature is `(Option<HandlerRoutine>, i32) -> i32`; the Rust
    // declaration below reproduces it exactly. Installation is best-effort: on
    // failure the default console behaviour applies and no memory is involved.
    unsafe extern "system" {
        fn SetConsoleCtrlHandler(handler: Option<HandlerRoutine>, add: i32) -> i32;
    }

    // SAFETY: a console control handler must only do async-signal-safe work.
    // This one stores `true` in an atomic and returns TRUE; it never allocates,
    // locks, unwinds or calls back into Rust.
    unsafe extern "system" fn handler(_control_type: u32) -> i32 {
        STOP.store(true, Ordering::SeqCst);
        1 // TRUE: handled; the poll loop exits and reaps the child.
    }

    pub fn install() {
        // SAFETY: `handler` has the exact `HandlerRoutine` signature and has
        // static lifetime; the return value is ignored on purpose (best-effort).
        unsafe {
            SetConsoleCtrlHandler(Some(handler), 1);
        }
    }
}

#[cfg(not(windows))]
#[allow(unsafe_code)]
mod signal {
    use super::STOP;
    use std::sync::atomic::Ordering;

    // SAFETY (declaration): `signal(2)` takes an `int` and a function pointer;
    // passing the pointer as `usize` matches the platform ABI used by this
    // binding. Only SIGINT (2) is installed.
    unsafe extern "C" {
        fn signal(sig: i32, handler: usize) -> usize;
    }

    // SAFETY: a signal handler must only do async-signal-safe work. This one
    // stores `true` in an atomic and returns; it never allocates or locks.
    unsafe extern "C" fn handler(_sig: i32) {
        STOP.store(true, Ordering::SeqCst);
    }

    pub fn install() {
        // SAFETY: `handler` is a valid function pointer for `signal`; the call
        // is best-effort and the previous handler's result is ignored.
        unsafe {
            signal(2, handler as usize); // SIGINT
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(len: u64) -> Stamp {
        Stamp {
            modified: None,
            len,
        }
    }

    fn snapshot(entries: &[(&str, Stamp)]) -> BTreeMap<PathBuf, Stamp> {
        entries
            .iter()
            .map(|(path, stamp)| (PathBuf::from(path), stamp.clone()))
            .collect()
    }

    #[test]
    fn source_snapshot_ignores_the_lockfile() {
        let full = snapshot(&[
            ("src/main.rv", stamp(10)),
            ("ravum.toml", stamp(20)),
            ("ravum.lock", stamp(30)),
        ]);
        let sources = source_snapshot(full);
        assert!(sources.contains_key(Path::new("src/main.rv")));
        assert!(sources.contains_key(Path::new("ravum.toml")));
        assert!(
            !sources.contains_key(Path::new("ravum.lock")),
            "the build writes ravum.lock; it must not look like a user edit"
        );
    }

    #[test]
    fn a_lockfile_only_change_is_not_a_stale_source() {
        let before = source_snapshot(snapshot(&[
            ("src/main.rv", stamp(10)),
            ("ravum.lock", stamp(30)),
        ]));
        let after = source_snapshot(snapshot(&[
            ("src/main.rv", stamp(10)),
            ("ravum.lock", stamp(99)),
        ]));
        assert!(!sources_changed(&before, &after));
    }

    #[test]
    fn a_source_edit_during_the_build_is_stale() {
        let before = source_snapshot(snapshot(&[("src/main.rv", stamp(10))]));
        let after = source_snapshot(snapshot(&[("src/main.rv", stamp(11))]));
        assert!(sources_changed(&before, &after));
    }

    #[test]
    fn an_added_or_removed_source_is_stale() {
        let before = source_snapshot(snapshot(&[("src/main.rv", stamp(10))]));
        let after = source_snapshot(snapshot(&[
            ("src/main.rv", stamp(10)),
            ("src/extra.rv", stamp(5)),
        ]));
        assert!(sources_changed(&before, &after));
    }

    #[test]
    fn the_next_snapshot_absorbs_the_lockfile_but_keeps_stable_sources() {
        let built_after = snapshot(&[("src/main.rv", stamp(10))]);
        let full = snapshot(&[("src/main.rv", stamp(10)), ("ravum.lock", stamp(42))]);
        let merged = merge_lockfile_state(built_after, &full);
        assert_eq!(
            merged.get(Path::new("src/main.rv")),
            Some(&stamp(10)),
            "the stabilized source snapshot is preserved"
        );
        assert_eq!(
            merged.get(Path::new("ravum.lock")),
            Some(&stamp(42)),
            "lock state must be absorbed so watch does not rebuild on its own write"
        );
    }

    #[test]
    fn a_later_lockfile_change_still_triggers_a_rebuild() {
        // The merged snapshot reflects the lock as it was after the build; a
        // third-party rewrite of `ravum.lock` must still be detected by the
        // ordinary full scan comparison.
        let merged = merge_lockfile_state(
            snapshot(&[("src/main.rv", stamp(10))]),
            &snapshot(&[("src/main.rv", stamp(10)), ("ravum.lock", stamp(42))]),
        );
        let current = snapshot(&[("src/main.rv", stamp(10)), ("ravum.lock", stamp(43))]);
        assert_ne!(current, merged);
    }
}
