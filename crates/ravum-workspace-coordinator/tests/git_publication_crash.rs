//! MVP-0c L3.2 — crash, restart, permissions and generated-file robustness for the
//! transactional Git publication.
//!
//! Scope: this file only proves *observable* properties of the existing
//! implementation in `src/git_writer.rs` + `src/publication_journal.rs`. It never
//! patches the coordinator: the production code is the thing under test.
//!
//! # Crash injection (real OS process termination, not a simulation hook)
//!
//! The writer executes commit hooks itself (`pre-commit`, `commit-msg`) and Git runs
//! `reference-transaction`/`post-commit` natively. A hook can therefore terminate the
//! **publishing process** at a precise point of the transaction:
//!
//! ```text
//! prepare: index -> blobs -> write-tree -> hooks(pre-commit/commit-msg) -> commit-tree -> journal INTENT
//! publish: HEAD/disk checks -> update-ref CAS -> working tree -> real index -> journal OUTCOME -> post-commit
//! ```
//!
//! | Stage | Hook that kills the publisher | Crash window proven |
//! |---|---|---|
//! | `pre_commit` | `pre-commit` | after candidate tree, before candidate commit |
//! | `commit_msg` | `commit-msg` | after candidate tree/hooks, before `commit-tree` |
//! | `cas_prepared` | `reference-transaction` (state `prepared`) | after `commit-tree` + INTENT, inside `update-ref` before the swap |
//! | `cas_committed` | `reference-transaction` (state `committed`) | after the ref swap, before working-tree/index/OUTCOME reconciliation |
//! | `post_commit` | `post-commit` | after the full publication (OUTCOME written) |
//!
//! The parent test spawns the current test binary (`--exact crash_injection_child_entry`)
//! with a scenario, a repository and a journal path. The child writes its PID to a file;
//! the installed hook calls a tiny OS-specific helper that reads that file, appends a
//! proof line to a hook log and terminates the child (`Stop-Process -Force` on Windows,
//! `kill -9` on Unix). A `completed` marker must be absent when the child was killed
//! (otherwise the harness itself is broken and the test fails loudly).
//!
//! # Invariant proven by every crash case
//!
//! After a crash, the canonical state (`refs/heads/main`, objects, journal) is one of:
//! * **not published** — `HEAD` is exactly the old commit, no INTENT was written, or
//!   exactly one INTENT is pending and `recover()` resolves it as `RECOVERED_UNPUBLISHED`;
//! * **fully published** — `HEAD` is exactly the candidate commit, its tree contains the
//!   target blob, exactly one OUTCOME exists, and a retry of the same publication id is
//!   refused with `AlreadyPublished` (never a double publication).
//!
//! The intermediate "reference moved but commit incomplete" state cannot exist: the tree,
//! the blobs and the candidate commit are created before the CAS, and the CAS is the single
//! commit point. Post-CAS reconciliation (working tree/index) may be stale after a crash;
//! that residue is explicit (`git status`, `recover()`), never silent.

use ravum_protocol::sha256_hex;
use ravum_workspace_coordinator::{
    GitError, GitWriter, PathUpdate, PublishRequest, RecoveryReport,
};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

const OLD_A: &str = "fn a() { 1 }\n";
const NEW_A: &str = "fn a() { 2 }\n";
const OLD_B: &str = "fn b() { 1 }\n";
const NEW_B: &str = "fn b() { 2 }\n";

const CHILD_SCENARIO: &str = "RAVUM_TEST_CRASH_SCENARIO";
const CHILD_REPO: &str = "RAVUM_TEST_CRASH_REPO";
const CHILD_JOURNAL: &str = "RAVUM_TEST_CRASH_JOURNAL";
const CHILD_PID_FILE: &str = "RAVUM_TEST_CRASH_PID_FILE";
const CHILD_HOOK_LOG: &str = "RAVUM_TEST_CRASH_HOOK_LOG";
const CHILD_COMPLETED: &str = "RAVUM_TEST_CRASH_COMPLETED";
const CHILD_EXPECTED_OLD: &str = "RAVUM_TEST_CRASH_EXPECTED_OLD";
const CHILD_KILLER: &str = "RAVUM_TEST_CRASH_KILLER";

// ---------------------------------------------------------------------------
// fixtures (throw-away repositories only: the Ravum checkout is never touched)
// ---------------------------------------------------------------------------

fn tests_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ravum-tests")
}

fn clear_readonly_recursively(path: &Path) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    let mut permissions = metadata.permissions();
    if permissions.readonly() {
        // Windows exposes the read-only attribute as the only relevant write bit; the
        // clippy recommendation to use `set_permissions` does not apply to attributes.
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        let _ = fs::set_permissions(path, permissions);
    }
    if metadata.is_dir() {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                clear_readonly_recursively(&entry.path());
            }
        }
    }
}

fn test_dir(name: &str) -> PathBuf {
    let dir = tests_root().join(name);
    if dir.exists() {
        clear_readonly_recursively(&dir);
    }
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test directory");
    dir
}

fn fresh_journal(name: &str) -> PathBuf {
    let path = tests_root().join(name).with_extension("journal.jsonl");
    let _ = fs::remove_file(&path);
    let _ = fs::remove_dir_all(&path);
    path
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = git_try(dir, args);
    assert!(
        output.status.success(),
        "git {args:?} failed:\nstderr: {}\nstdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn git_try(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("run git")
}

fn init_repo(name: &str) -> PathBuf {
    let repo = test_dir(name);
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.name", "test"]);
    git(&repo, &["config", "user.email", "test@ravum.invalid"]);
    git(&repo, &["config", "core.autocrlf", "false"]);
    git(&repo, &["config", "commit.gpgsign", "false"]);
    fs::write(repo.join("a.rs"), OLD_A).expect("write a.rs");
    fs::write(repo.join("b.rs"), OLD_B).expect("write b.rs");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    repo
}

fn write_file(repo: &Path, relative: &str, content: &str) {
    let path = repo.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent directory");
    }
    fs::write(&path, content).expect("write file");
}

fn read_file(repo: &Path, relative: &str) -> String {
    fs::read_to_string(repo.join(relative)).expect("read file")
}

fn head(repo: &Path) -> String {
    git(repo, &["rev-parse", "HEAD"]).trim().to_owned()
}

fn commit_count(repo: &Path) -> usize {
    git(repo, &["rev-list", "--count", "HEAD"])
        .trim()
        .parse()
        .expect("commit count")
}

fn status(repo: &Path) -> String {
    git(repo, &["status", "--porcelain"])
}

fn head_file(repo: &Path, path: &str) -> String {
    git(repo, &["show", &format!("HEAD:{path}")])
}

fn index_file(repo: &Path, path: &str) -> String {
    git(repo, &["cat-file", "-p", &format!(":{path}")])
}

fn base_hash(repo: &Path, path: &str) -> String {
    sha256_hex(head_file(repo, path).as_bytes())
}

fn writer(repo: &Path) -> GitWriter {
    GitWriter::new(repo, "agent-crash", "agent-crash@ravum.invalid")
}

fn install_hook(repo: &Path, name: &str, body: &str) {
    let hooks = repo.join(".git").join("hooks");
    fs::create_dir_all(&hooks).expect("create hooks directory");
    let path = hooks.join(name);
    fs::write(&path, body).expect("write hook");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&path).expect("hook metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("make hook executable");
    }
}

fn modify_a(repo: &Path, target: &str, publication_id: &str) -> PublishRequest {
    PublishRequest::new("crash injection")
        .with_change(
            PathUpdate::write("a.rs", target).with_expected_old_sha256(base_hash(repo, "a.rs")),
        )
        .with_trailer("Ravum-Proposal", "agent-crash/t1")
        .with_trailer("Ravum-Agent", "agent-crash")
        .with_publication_id(publication_id)
}

/// Canonical-state sanity checks used after every crash or refusal.
fn assert_repository_not_corrupted(repo: &Path) {
    git(repo, &["fsck", "--no-progress"]);
    let _ = status(repo); // must not fail: the index is readable
    let _ = head(repo);
}

// ---------------------------------------------------------------------------
// journal inspection (raw JSON lines: readable even when the journal is read-only)
// ---------------------------------------------------------------------------

fn journal_records(journal: &Path) -> Vec<serde_json::Value> {
    match fs::read_to_string(journal) {
        Ok(text) => text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                serde_json::from_str::<serde_json::Value>(line).expect("journal line parses")
            })
            .collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => panic!("journal {} unreadable: {error}", journal.display()),
    }
}

fn intents(journal: &Path) -> Vec<serde_json::Value> {
    journal_records(journal)
        .into_iter()
        .filter(|record| record["event"] == "INTENT")
        .collect()
}

fn outcomes(journal: &Path) -> Vec<serde_json::Value> {
    journal_records(journal)
        .into_iter()
        .filter(|record| record["event"] == "OUTCOME")
        .collect()
}

fn last_outcome_status(journal: &Path) -> Option<String> {
    outcomes(journal)
        .last()
        .map(|record| record["status"].as_str().unwrap_or_default().to_owned())
}

fn outcome_statuses(journal: &Path) -> Vec<String> {
    outcomes(journal)
        .iter()
        .map(|record| record["status"].as_str().unwrap_or_default().to_owned())
        .collect()
}

// ---------------------------------------------------------------------------
// crash-injection harness: spawn the test binary, kill it at a chosen hook
// ---------------------------------------------------------------------------

/// Platform-specific helper that logs the stage, then hard-kills the publishing PID.
fn ensure_kill_helper() -> PathBuf {
    let root = tests_root();
    fs::create_dir_all(&root).expect("create tests root");
    #[cfg(windows)]
    {
        let helper = root.join("crash-kill-helper.ps1");
        let body = concat!(
            "param([string]$Tag)\n",
            "$ErrorActionPreference = 'Stop'\n",
            "Add-Content -LiteralPath $env:RAVUM_TEST_CRASH_HOOK_LOG -Value $Tag\n",
            "$raw = Get-Content -LiteralPath $env:RAVUM_TEST_CRASH_PID_FILE -Raw\n",
            "Stop-Process -Id ([int]($raw.Trim())) -Force\n",
            "exit 1\n"
        );
        fs::write(&helper, body).expect("write kill helper");
        helper
    }
    #[cfg(not(windows))]
    {
        let helper = root.join("crash-kill-helper.sh");
        let body = concat!(
            "#!/bin/sh\n",
            "echo \"$1\" >> \"$RAVUM_TEST_CRASH_HOOK_LOG\"\n",
            "kill -9 \"$(cat \"$RAVUM_TEST_CRASH_PID_FILE\")\" 2>/dev/null\n",
            "exit 1\n"
        );
        fs::write(&helper, body).expect("write kill helper");
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&helper)
            .expect("helper metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&helper, permissions).expect("helper executable");
        helper
    }
}

fn helper_invocation(tag: &str) -> String {
    let helper = ensure_kill_helper();
    let path = if cfg!(windows) {
        helper.to_string_lossy().replace('\\', "/")
    } else {
        helper.to_string_lossy().to_string()
    };
    #[cfg(windows)]
    {
        format!(
            "powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"{path}\" \"{tag}\" >/dev/null 2>&1"
        )
    }
    #[cfg(not(windows))]
    {
        format!("\"{path}\" \"{tag}\" >/dev/null 2>&1")
    }
}

/// Shell hook that kills the publishing process (and proves it ran via the helper).
fn kill_hook(tag: &str, only_state: Option<&str>) -> String {
    let mut script = String::from("#!/bin/sh\n");
    if let Some(state) = only_state {
        script.push_str(&format!("if [ \"$1\" = \"{state}\" ]; then\n"));
    }
    script.push_str(&helper_invocation(tag));
    script.push('\n');
    script.push_str("exit 1\n");
    if only_state.is_some() {
        script.push_str("fi\nexit 0\n");
    }
    script
}

struct ChildRun {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

struct ChildPaths {
    pid_file: PathBuf,
    hook_log: PathBuf,
    completed: PathBuf,
}

fn child_paths(scenario: &str) -> ChildPaths {
    let root = tests_root();
    ChildPaths {
        pid_file: root.join(format!("{scenario}-child.pid")),
        hook_log: root.join(format!("{scenario}-child-hooks.log")),
        completed: root.join(format!("{scenario}-child-completed.txt")),
    }
}

/// Remove the injection hooks once the crash has been observed, so the *restart*
/// publication is not blocked by remains of the harness.
fn clear_crash_hooks(repo: &Path) {
    let hooks = repo.join(".git").join("hooks");
    if let Ok(entries) = fs::read_dir(&hooks) {
        for entry in entries.flatten() {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Install the stage hook(s), spawn the publishing child, and wait for it to die.
fn run_crash_child(
    repo: &Path,
    journal: &Path,
    scenario: &str,
    hooks: &[(&str, String)],
) -> (ChildRun, ChildPaths) {
    for (name, body) in hooks {
        install_hook(repo, name, body);
    }
    let paths = child_paths(scenario);
    for path in [&paths.pid_file, &paths.hook_log, &paths.completed] {
        let _ = fs::remove_file(path);
    }
    let expected_old = base_hash(repo, "a.rs");
    let mut command = Command::new(std::env::current_exe().expect("current test binary"));
    command
        .args(["--exact", "crash_injection_child_entry", "--test-threads=1"])
        .env(CHILD_SCENARIO, scenario)
        .env(CHILD_REPO, repo)
        .env(CHILD_JOURNAL, journal)
        .env(CHILD_PID_FILE, &paths.pid_file)
        .env(CHILD_HOOK_LOG, &paths.hook_log)
        .env(CHILD_COMPLETED, &paths.completed)
        .env(CHILD_EXPECTED_OLD, &expected_old)
        .env(CHILD_KILLER, ensure_kill_helper())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("spawn crash child");
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        match child.try_wait().expect("wait for crash child") {
            Some(status) => {
                let mut stdout = String::new();
                if let Some(mut pipe) = child.stdout.take() {
                    let _ = pipe.read_to_string(&mut stdout);
                }
                let mut stderr = String::new();
                if let Some(mut pipe) = child.stderr.take() {
                    let _ = pipe.read_to_string(&mut stderr);
                }
                // The crash has been produced: remove the harness hooks so a later
                // publication in the same repository is not killed/refused by them.
                clear_crash_hooks(repo);
                return (
                    ChildRun {
                        status,
                        stdout,
                        stderr,
                    },
                    paths,
                );
            }
            None if Instant::now() > deadline => {
                let _ = child.kill();
                panic!("crash child did not terminate within 120 s");
            }
            None => std::thread::sleep(Duration::from_millis(25)),
        }
    }
}

fn assert_child_killed_at(run: &ChildRun, paths: &ChildPaths, scenario: &str, hook: &str) {
    assert!(
        !run.status.success(),
        "scenario {scenario}: the child should have been killed, but it exited successfully.\nstdout: {}\nstderr: {}",
        run.stdout, run.stderr
    );
    assert!(
        !paths.completed.exists(),
        "scenario {scenario}: the child reached the end of publish_request — the injected kill did not happen"
    );
    let hook_log = fs::read_to_string(&paths.hook_log).unwrap_or_default();
    assert!(
        hook_log.contains(hook),
        "scenario {scenario}: hook log must prove the injected stage ran; log was {hook_log:?}"
    );
}

/// The child entry point. In a normal `cargo test` run it checks a cheap invariant;
/// when spawned by a parent crash test it runs one publication and waits to be killed.
#[test]
fn crash_injection_child_entry() {
    let Ok(scenario) = std::env::var(CHILD_SCENARIO) else {
        // Normal run: preparing must never move a reference.
        let repo = init_repo("crash-child-entry-smoke");
        let before = head(&repo);
        let unpublished = writer(&repo)
            .prepare(&modify_a(&repo, NEW_A, "child-entry-smoke"))
            .expect("prepare");
        assert_eq!(head(&repo), before, "prepare must not publish");
        assert_eq!(commit_count(&repo), 1, "prepare must not publish a commit");
        drop(unpublished);
        return;
    };

    let repo = PathBuf::from(std::env::var(CHILD_REPO).expect("child repo"));
    let journal = std::env::var(CHILD_JOURNAL).expect("child journal");
    let pid_file = PathBuf::from(std::env::var(CHILD_PID_FILE).expect("child pid file"));
    let completed = PathBuf::from(std::env::var(CHILD_COMPLETED).expect("child completed marker"));
    let expected_old = std::env::var(CHILD_EXPECTED_OLD).expect("expected old");
    fs::write(&pid_file, std::process::id().to_string()).expect("write child pid");
    let request = PublishRequest::new("crash injection")
        .with_change(PathUpdate::write("a.rs", NEW_A).with_expected_old_sha256(expected_old))
        .with_trailer("Ravum-Proposal", "agent-crash/t1")
        .with_trailer("Ravum-Agent", "agent-crash")
        .with_publication_id("crash-case");
    let writer = writer(&repo)
        .with_publication_journal(&journal)
        .expect("child publication journal");
    let outcome = writer.publish_request(&request);
    // Reaching this point means the kill was not injected: record it so the parent
    // fails loudly instead of accepting a false green.
    let marker = match outcome {
        Ok(result) => format!(
            "{scenario}: completed without crash; published {}",
            result.sha
        ),
        Err(error) => format!("{scenario}: completed without crash; error {error}"),
    };
    fs::write(&completed, marker).expect("write completed marker");
    std::process::exit(0);
}

// ---------------------------------------------------------------------------
// 1-2. crash before the candidate commit exists (pre-commit / commit-msg)
// ---------------------------------------------------------------------------

fn assert_nothing_was_published(repo: &Path, journal: &Path, before: &str) {
    assert_eq!(head(repo), before, "reference must not move");
    assert_eq!(commit_count(repo), 1, "no commit may be published");
    assert!(
        intents(journal).is_empty(),
        "no INTENT may be journaled before the candidate commit exists"
    );
    assert!(
        outcomes(journal).is_empty(),
        "no OUTCOME may be journaled when nothing was published"
    );
    assert_repository_not_corrupted(repo);
}

#[test]
fn crash_in_pre_commit_hook_publishes_nothing_and_restarts_cleanly() {
    let repo = init_repo("crash-pre-commit");
    let journal = fresh_journal("crash-pre-commit");
    let before = head(&repo);
    let (run, paths) = run_crash_child(
        &repo,
        &journal,
        "pre_commit",
        &[("pre-commit", kill_hook("pre-commit", None))],
    );
    assert_child_killed_at(&run, &paths, "pre_commit", "pre-commit");
    assert_nothing_was_published(&repo, &journal, &before);

    // A later publication starts from a clean base and publishes exactly once.
    let result = writer(&repo)
        .publish_request(&modify_a(&repo, NEW_A, "after-pre-commit-crash"))
        .expect("publication after crash");
    assert_eq!(result.parent, before);
    assert_eq!(commit_count(&repo), 2);
    assert_eq!(read_file(&repo, "a.rs"), NEW_A);
    assert_eq!(status(&repo).trim(), "");
}

#[test]
fn crash_in_commit_msg_hook_publishes_nothing_and_restarts_cleanly() {
    let repo = init_repo("crash-commit-msg");
    let journal = fresh_journal("crash-commit-msg");
    let before = head(&repo);
    let (run, paths) = run_crash_child(
        &repo,
        &journal,
        "commit_msg",
        &[("commit-msg", kill_hook("commit-msg", None))],
    );
    assert_child_killed_at(&run, &paths, "commit_msg", "commit-msg");
    assert_nothing_was_published(&repo, &journal, &before);

    let result = writer(&repo)
        .publish_request(&modify_a(&repo, NEW_A, "after-commit-msg-crash"))
        .expect("publication after crash");
    assert_eq!(result.parent, before);
    assert_eq!(commit_count(&repo), 2);
    assert_eq!(status(&repo).trim(), "");
}

// ---------------------------------------------------------------------------
// 3. crash inside update-ref, before the reference swap (`prepared` state)
// ---------------------------------------------------------------------------

#[test]
fn crash_before_reference_swap_is_recovered_as_unpublished_without_double_publication() {
    let repo = init_repo("crash-cas-prepared");
    let journal = fresh_journal("crash-cas-prepared");
    let before = head(&repo);
    let (run, paths) = run_crash_child(
        &repo,
        &journal,
        "cas_prepared",
        &[(
            "reference-transaction",
            kill_hook("reference-transaction prepared", Some("prepared")),
        )],
    );
    assert_child_killed_at(
        &run,
        &paths,
        "cas_prepared",
        "reference-transaction prepared",
    );

    // Reference untouched; the candidate commit exists only as a durable INTENT.
    assert_eq!(
        head(&repo),
        before,
        "the CAS must not have moved the reference"
    );
    assert_eq!(commit_count(&repo), 1);
    assert_eq!(intents(&journal).len(), 1, "exactly one INTENT must exist");
    assert!(outcomes(&journal).is_empty(), "no OUTCOME was written");
    let candidate = intents(&journal)[0]["new_commit"]
        .as_str()
        .expect("intent commit")
        .to_owned();
    assert!(
        !git(&repo, &["rev-list", "--all"]).contains(&candidate),
        "the candidate commit must stay unreachable"
    );
    assert_repository_not_corrupted(&repo);

    // Restart: recovery classifies the intent from the repository, then the same
    // publication is replayed once and only once.
    let restarted = writer(&repo)
        .with_publication_journal(&journal)
        .expect("journal reopen");
    let report = restarted.recover().expect("recover");
    assert_eq!(report.published, Vec::<String>::new());
    assert_eq!(report.unpublished, vec!["crash-case".to_owned()]);
    assert_eq!(
        last_outcome_status(&journal).as_deref(),
        Some("RECOVERED_UNPUBLISHED")
    );
    assert_eq!(head(&repo), before, "recovery must not publish anything");

    // Second recovery is a no-op (no duplicate outcome).
    let outcomes_after_first_recovery = outcomes(&journal).len();
    let second = restarted.recover().expect("second recover");
    assert_eq!(second, RecoveryReport::default());
    assert_eq!(outcomes(&journal).len(), outcomes_after_first_recovery);

    let result = restarted
        .publish_request(&modify_a(&repo, NEW_A, "crash-case"))
        .expect("retry after recovery");
    assert_eq!(result.parent, before);
    assert_eq!(commit_count(&repo), 2, "exactly one publication");
    assert_eq!(read_file(&repo, "a.rs"), NEW_A);
    assert_eq!(status(&repo).trim(), "");
    assert_eq!(last_outcome_status(&journal).as_deref(), Some("PUBLISHED"));
}

// ---------------------------------------------------------------------------
// 4. crash after the reference swap, before OUTCOME / reconciliation
// ---------------------------------------------------------------------------

#[test]
fn crash_after_reference_swap_is_recovered_as_published_and_never_republished() {
    let repo = init_repo("crash-cas-committed");
    let journal = fresh_journal("crash-cas-committed");
    let before = head(&repo);
    let (run, paths) = run_crash_child(
        &repo,
        &journal,
        "cas_committed",
        &[(
            "reference-transaction",
            kill_hook("reference-transaction committed", Some("committed")),
        )],
    );
    assert_child_killed_at(
        &run,
        &paths,
        "cas_committed",
        "reference-transaction committed",
    );

    // The commit point happened: the candidate is the new HEAD and its tree is complete.
    let published = head(&repo);
    assert_ne!(
        published, before,
        "the CAS must have published the candidate"
    );
    assert_eq!(commit_count(&repo), 2);
    assert_eq!(
        head_file(&repo, "a.rs"),
        NEW_A,
        "the published tree contains the target blob"
    );
    let published_intent = intents(&journal);
    assert_eq!(published_intent.len(), 1);
    assert_eq!(
        published_intent[0]["new_commit"].as_str(),
        Some(published.as_str()),
        "the journal INTENT names the candidate that is now HEAD"
    );
    assert!(
        outcomes(&journal).is_empty(),
        "the crash happened before the OUTCOME line"
    );
    // The crash also happened before the post-CAS reconciliation: canonical state is
    // complete, working tree/index are stale — and that residue must be visible.
    assert_eq!(
        read_file(&repo, "a.rs"),
        OLD_A,
        "working tree was not reconciled"
    );
    assert_eq!(
        index_file(&repo, "a.rs"),
        OLD_A,
        "real index was not reconciled"
    );
    assert!(
        !status(&repo).is_empty(),
        "the stale reconciliation must be visible"
    );
    assert_repository_not_corrupted(&repo);

    // Restart: recovery observes the swap instead of republishing.
    let restarted = writer(&repo)
        .with_publication_journal(&journal)
        .expect("journal reopen");
    let report = restarted.recover().expect("recover");
    assert_eq!(report.published, vec!["crash-case".to_owned()]);
    assert!(report.unpublished.is_empty());
    assert_eq!(
        last_outcome_status(&journal).as_deref(),
        Some("RECOVERED_PUBLISHED")
    );
    let outcome_commit = outcomes(&journal)
        .last()
        .and_then(|record| record["commit"].as_str())
        .map(ToOwned::to_owned);
    assert_eq!(outcome_commit.as_deref(), Some(published.as_str()));
    assert_eq!(
        head(&repo),
        published,
        "recovery must not move the reference"
    );

    // A retry of the same publication id is refused: no double publication.
    let error = restarted
        .prepare(&modify_a(&repo, NEW_A, "crash-case"))
        .expect_err("already published");
    assert!(
        matches!(error, GitError::AlreadyPublished { ref publication_id, .. } if publication_id == "crash-case"),
        "unexpected error: {error}"
    );
    assert_eq!(commit_count(&repo), 2, "no second commit");
    assert_eq!(head(&repo), published);

    // Unrelated work can still be published once; the stale path is not overwritten.
    let unrelated = PublishRequest::new("unrelated repair")
        .with_change(
            PathUpdate::write("b.rs", NEW_B).with_expected_old_sha256(base_hash(&repo, "b.rs")),
        )
        .with_publication_id("unrelated-after-crash");
    writer(&repo)
        .publish_request(&unrelated)
        .expect("unrelated publication");
    assert_eq!(commit_count(&repo), 3);
    assert_eq!(read_file(&repo, "b.rs"), NEW_B);
    assert_eq!(
        read_file(&repo, "a.rs"),
        OLD_A,
        "a divergent working-tree file must never be silently overwritten"
    );
    assert_eq!(
        head_file(&repo, "a.rs"),
        NEW_A,
        "canonical content stays the published one"
    );
}

// ---------------------------------------------------------------------------
// 5. crash after a fully completed publication (post-commit hook)
// ---------------------------------------------------------------------------

#[test]
fn crash_in_post_commit_hook_leaves_fully_published_state() {
    let repo = init_repo("crash-post-commit");
    let journal = fresh_journal("crash-post-commit");
    let before = head(&repo);
    let (run, paths) = run_crash_child(
        &repo,
        &journal,
        "post_commit",
        &[("post-commit", kill_hook("post-commit", None))],
    );
    assert_child_killed_at(&run, &paths, "post_commit", "post-commit");

    let published = head(&repo);
    assert_ne!(published, before);
    assert_eq!(commit_count(&repo), 2);
    assert_eq!(head_file(&repo, "a.rs"), NEW_A);
    assert_eq!(
        read_file(&repo, "a.rs"),
        NEW_A,
        "working tree reconciled before post-commit"
    );
    assert_eq!(
        index_file(&repo, "a.rs"),
        NEW_A,
        "index reconciled before post-commit"
    );
    assert_eq!(status(&repo).trim(), "");
    assert_eq!(intents(&journal).len(), 1);
    assert_eq!(
        outcome_statuses(&journal),
        vec!["PUBLISHED".to_owned()],
        "the OUTCOME was durable before post-commit"
    );
    assert_repository_not_corrupted(&repo);

    let restarted = writer(&repo)
        .with_publication_journal(&journal)
        .expect("journal reopen");
    assert_eq!(
        restarted.recover().expect("recover"),
        RecoveryReport::default()
    );
    let error = restarted
        .prepare(&modify_a(&repo, NEW_A, "crash-case"))
        .expect_err("already published");
    assert!(matches!(error, GitError::AlreadyPublished { .. }));
    assert_eq!(commit_count(&repo), 2);
}

// ---------------------------------------------------------------------------
// 6. harness control: no hook means the child publishes exactly once
// ---------------------------------------------------------------------------

#[test]
fn crash_harness_control_without_hooks_publishes_exactly_once() {
    let repo = init_repo("crash-control");
    let journal = fresh_journal("crash-control");
    let before = head(&repo);
    let (run, paths) = run_crash_child(&repo, &journal, "control", &[]);
    assert!(
        run.status.success(),
        "control child must complete successfully\nstdout: {}\nstderr: {}",
        run.stdout,
        run.stderr
    );
    assert!(
        paths.completed.exists(),
        "control child must report completion (harness sanity)"
    );
    let published = head(&repo);
    assert_ne!(published, before);
    assert_eq!(commit_count(&repo), 2);
    assert_eq!(head_file(&repo, "a.rs"), NEW_A);
    assert_eq!(read_file(&repo, "a.rs"), NEW_A);
    assert_eq!(status(&repo).trim(), "");
    assert_eq!(intents(&journal).len(), 1);
    assert_eq!(outcome_statuses(&journal), vec!["PUBLISHED".to_owned()]);

    let retry = writer(&repo)
        .with_publication_journal(&journal)
        .expect("journal reopen")
        .prepare(&modify_a(&repo, NEW_A, "crash-case"))
        .expect_err("already published");
    assert!(matches!(retry, GitError::AlreadyPublished { .. }));
    assert_eq!(commit_count(&repo), 2);
}

// ---------------------------------------------------------------------------
// 7. permission failures: the commit point is complete, residue is explicit
// ---------------------------------------------------------------------------

struct ReadOnlyGuard {
    path: PathBuf,
}

impl ReadOnlyGuard {
    fn new(path: PathBuf) -> Self {
        let mut permissions = fs::metadata(&path).expect("metadata").permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&path, permissions).expect("set read-only");
        Self { path }
    }
}

impl Drop for ReadOnlyGuard {
    fn drop(&mut self) {
        if let Ok(metadata) = fs::metadata(&self.path) {
            let mut permissions = metadata.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            let _ = fs::set_permissions(&self.path, permissions);
        }
    }
}

#[test]
fn read_only_worktree_file_is_reported_as_warning_not_silently_overwritten() {
    let repo = init_repo("perm-readonly-file");
    let journal = fresh_journal("perm-readonly-file");
    let before = head(&repo);
    let publisher = writer(&repo)
        .with_publication_journal(&journal)
        .expect("journal");
    let prepared = publisher
        .prepare(&modify_a(&repo, NEW_A, "perm-readonly"))
        .expect("prepare");
    let _guard = ReadOnlyGuard::new(repo.join("a.rs"));

    let result = publisher
        .publish(prepared)
        .expect("publication completes: the working-tree write is a warning");
    assert!(
        !result.warnings.is_empty(),
        "the failed working-tree write must be reported, not silent: {result:?}"
    );
    assert_ne!(head(&repo), before, "the commit point happened");
    assert_eq!(commit_count(&repo), 2);
    assert_eq!(
        head_file(&repo, "a.rs"),
        NEW_A,
        "published tree is complete"
    );
    assert_eq!(
        read_file(&repo, "a.rs"),
        OLD_A,
        "the read-only file was not overwritten"
    );
    assert_eq!(
        index_file(&repo, "a.rs"),
        NEW_A,
        "the real index was reconciled"
    );
    assert!(
        status(&repo).contains("a.rs"),
        "the divergence must stay visible in the working tree"
    );
    assert_eq!(last_outcome_status(&journal).as_deref(), Some("PUBLISHED"));
    assert_repository_not_corrupted(&repo);

    let retry = publisher
        .prepare(&modify_a(&repo, NEW_A, "perm-readonly"))
        .expect_err("already published");
    assert!(matches!(retry, GitError::AlreadyPublished { .. }));
    assert_eq!(commit_count(&repo), 2);
}

/// Denies deletion of a file while keeping it readable, so the post-CAS unlink fails
/// and must be reported as a warning (permission scenario of the L3.2 mandate).
///
/// Windows: a helper PowerShell process holds the file open with `FileShare.Read`
/// only, which blocks `DeleteFile` (sharing violation) but allows our reader.
/// POSIX: the parent directory is made read-only, which blocks `unlink`.
#[cfg(windows)]
struct DenyUnlinkGuard {
    child: std::process::Child,
    ready: PathBuf,
    release: PathBuf,
}

#[cfg(windows)]
impl DenyUnlinkGuard {
    fn new(file: &Path) -> Self {
        let root = tests_root();
        let ready = root.join("deny-unlink-ready.txt");
        let release = root.join("deny-unlink-release.txt");
        let _ = fs::remove_file(&ready);
        let _ = fs::remove_file(&release);
        let path = file.to_string_lossy();
        let script = format!(
            "$fs=[IO.File]::Open('{path}','Open','Read','Read'); \
             [IO.File]::WriteAllText('{}','locked'); \
             while(-not (Test-Path '{}')){{Start-Sleep -Milliseconds 20}}; \
             $fs.Close()",
            ready.display(),
            release.display()
        );
        let child = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn file-share holder");
        let deadline = Instant::now() + Duration::from_secs(30);
        while !ready.exists() {
            assert!(
                Instant::now() < deadline,
                "file-share holder did not become ready"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        Self {
            child,
            ready,
            release,
        }
    }
}

#[cfg(windows)]
impl Drop for DenyUnlinkGuard {
    fn drop(&mut self) {
        let _ = fs::write(&self.release, b"release");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                Ok(None) => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    break;
                }
                Err(_) => break,
            }
        }
        let _ = fs::remove_file(&self.ready);
        let _ = fs::remove_file(&self.release);
    }
}

#[cfg(not(windows))]
type DenyUnlinkGuard = UnixParentModeGuard;

#[test]
fn delete_denied_by_permissions_is_reported_and_canonical_state_stays_published() {
    let repo = init_repo("perm-delete-denied");
    let journal = fresh_journal("perm-delete-denied");
    let before = head(&repo);
    let publisher = writer(&repo)
        .with_publication_journal(&journal)
        .expect("journal");
    let request = PublishRequest::new("delete a file whose unlink is denied")
        .with_change(PathUpdate::delete("b.rs").with_expected_old_sha256(base_hash(&repo, "b.rs")))
        .with_publication_id("perm-delete-denied");
    let prepared = publisher.prepare(&request).expect("prepare");
    // Block the unlink itself: the commit point must still be complete, and the
    // failed working-tree removal must be reported as a warning (never a silent
    // success, never a partial canonical state).
    let _guard = DenyUnlinkGuard::new(&repo.join("b.rs"));

    let result = publisher
        .publish(prepared)
        .expect("publication completes: the unlink failure is a warning");
    assert!(
        !result.warnings.is_empty(),
        "unlink failure must be reported: {result:?}"
    );
    assert_ne!(head(&repo), before);
    assert!(
        !git_try(&repo, &["show", "HEAD:b.rs"]).status.success(),
        "the published tree must no longer contain the deleted path"
    );
    assert!(
        repo.join("b.rs").exists(),
        "the unlink was denied: the file still exists on disk"
    );
    assert_eq!(
        git(&repo, &["ls-files", "--", "b.rs"]),
        "",
        "index reconciled the deletion"
    );
    assert!(
        status(&repo).contains("b.rs"),
        "the residue is visible as untracked work"
    );
    assert_eq!(last_outcome_status(&journal).as_deref(), Some("PUBLISHED"));
    assert_repository_not_corrupted(&repo);
    drop(_guard);
}

#[test]
fn maintenance_index_lock_blocks_reconciliation_without_partial_publication() {
    // "Maintenance lock" at Git level: the real index is locked by another process.
    let repo = init_repo("perm-index-lock");
    let journal = fresh_journal("perm-index-lock");
    let before = head(&repo);
    let publisher = writer(&repo)
        .with_publication_journal(&journal)
        .expect("journal");
    let prepared = publisher
        .prepare(&modify_a(&repo, NEW_A, "perm-index-lock"))
        .expect("prepare");
    let lock = repo.join(".git").join("index.lock");
    fs::write(&lock, b"").expect("create index lock");

    let error = publisher
        .publish(prepared)
        .expect_err("locked index must be reported");
    let published = match &error {
        GitError::IndexOutOfSync { commit, .. } => commit.clone(),
        other => panic!("unexpected error: {other}"),
    };
    assert_ne!(published, before);
    assert_eq!(
        head(&repo),
        published,
        "the commit is published (single commit point)"
    );
    assert_eq!(commit_count(&repo), 2);
    assert_eq!(head_file(&repo, "a.rs"), NEW_A);
    assert_eq!(
        read_file(&repo, "a.rs"),
        NEW_A,
        "working tree was reconciled"
    );
    assert_eq!(
        index_file(&repo, "a.rs"),
        OLD_A,
        "the real index could not be reconciled"
    );
    assert_eq!(
        last_outcome_status(&journal).as_deref(),
        Some("PUBLISHED"),
        "the publication is durable even when index reconciliation fails"
    );
    fs::remove_file(&lock).expect("remove index lock");

    // Documented repair path (test repository only): reset the index to HEAD.
    git(&repo, &["reset", "--mixed", "--quiet", "HEAD"]);
    assert_eq!(status(&repo).trim(), "");
    let retry = publisher
        .prepare(&modify_a(&repo, NEW_A, "perm-index-lock"))
        .expect_err("already published");
    assert!(matches!(retry, GitError::AlreadyPublished { .. }));
    assert_repository_not_corrupted(&repo);
}

#[test]
fn concurrent_reference_lock_is_refused_before_any_write() {
    // Concurrent-writer variant at the reference level: another Git process holds the
    // branch lock while the CAS is attempted. No reference may move and no working-tree
    // write may happen before the refused commit point.
    let repo = init_repo("perm-ref-lock");
    let journal = fresh_journal("perm-ref-lock");
    let before = head(&repo);
    let publisher = writer(&repo)
        .with_publication_journal(&journal)
        .expect("journal");
    let prepared = publisher
        .prepare(&modify_a(&repo, NEW_A, "perm-ref-lock"))
        .expect("prepare");
    let lock = repo
        .join(".git")
        .join("refs")
        .join("heads")
        .join("main.lock");
    fs::write(&lock, b"").expect("create reference lock");

    let error = publisher
        .publish(prepared)
        .expect_err("a locked reference must refuse the publication");
    assert!(
        matches!(
            error,
            GitError::ConcurrentWriter { .. } | GitError::Git { .. }
        ),
        "the refusal must be explicit: {error}"
    );
    assert_eq!(head(&repo), before, "the reference must stay untouched");
    assert_eq!(commit_count(&repo), 1);
    assert_eq!(
        read_file(&repo, "a.rs"),
        OLD_A,
        "no working-tree write before the commit point"
    );
    assert_eq!(last_outcome_status(&journal).as_deref(), Some("REFUSED"));
    fs::remove_file(&lock).expect("remove reference lock");

    // After the competing lock is gone, the same publication succeeds exactly once.
    let result = publisher
        .publish_request(&modify_a(&repo, NEW_A, "perm-ref-lock"))
        .expect("publication after the competing lock is gone");
    assert_eq!(result.parent, before);
    assert_eq!(commit_count(&repo), 2);
    assert_eq!(read_file(&repo, "a.rs"), NEW_A);
    assert_eq!(last_outcome_status(&journal).as_deref(), Some("PUBLISHED"));
    assert_repository_not_corrupted(&repo);
}

#[cfg(windows)]
struct IcaclsGuard {
    target: PathBuf,
    user: String,
}

#[cfg(windows)]
impl IcaclsGuard {
    fn deny(target: &Path, deny_spec: &str) -> Self {
        let user = std::env::var("USERNAME").expect("USERNAME");
        run_icacls(target, &["/deny", &format!("{user}:{deny_spec}")]);
        Self {
            target: target.to_path_buf(),
            user,
        }
    }
}

#[cfg(windows)]
impl Drop for IcaclsGuard {
    fn drop(&mut self) {
        run_icacls(&self.target, &["/remove:d", &self.user]);
    }
}

#[cfg(windows)]
fn run_icacls(target: &Path, args: &[&str]) {
    let output = Command::new("icacls")
        .arg(target)
        .args(args)
        .output()
        .expect("run icacls");
    assert!(
        output.status.success(),
        "icacls {args:?} failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(not(windows))]
struct UnixModeGuard {
    path: PathBuf,
    mode: u32,
}

#[cfg(not(windows))]
impl Drop for UnixModeGuard {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(self.mode));
    }
}

/// Denies unlink of a file by making its parent directory read-only (POSIX semantics).
#[cfg(not(windows))]
struct UnixParentModeGuard {
    dir: PathBuf,
    mode: u32,
}

#[cfg(not(windows))]
impl UnixParentModeGuard {
    fn new(file: &Path) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let dir = file.parent().expect("parent directory").to_path_buf();
        let mode = fs::metadata(&dir)
            .expect("directory metadata")
            .permissions()
            .mode();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).expect("chmod parent");
        Self { dir, mode }
    }
}

#[cfg(not(windows))]
impl Drop for UnixParentModeGuard {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&self.dir, fs::Permissions::from_mode(self.mode));
    }
}

#[test]
fn protected_git_directory_refuses_publication_without_touching_the_reference() {
    let repo = init_repo("perm-git-protected");
    let journal = fresh_journal("perm-git-protected");
    let before = head(&repo);
    let publisher = writer(&repo)
        .with_publication_journal(&journal)
        .expect("journal");
    let prepared = publisher
        .prepare(&modify_a(&repo, NEW_A, "perm-git-protected"))
        .expect("prepare");

    let refs_heads = repo.join(".git").join("refs").join("heads");
    #[cfg(windows)]
    let _guard = IcaclsGuard::deny(&refs_heads, "(W)");
    #[cfg(not(windows))]
    let _guard = {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&refs_heads)
            .expect("refs metadata")
            .permissions()
            .mode();
        fs::set_permissions(&refs_heads, fs::Permissions::from_mode(0o555))
            .expect("chmod refs/heads");
        UnixModeGuard {
            path: refs_heads.clone(),
            mode,
        }
    };

    let error = publisher
        .publish(prepared)
        .expect_err("a protected .git directory must refuse the publication");
    assert!(
        matches!(
            error,
            GitError::Git { .. } | GitError::ConcurrentWriter { .. } | GitError::Io(_)
        ),
        "the refusal must be explicit, not a silent success: {error}"
    );
    assert_eq!(head(&repo), before, "the reference must stay untouched");
    assert_eq!(commit_count(&repo), 1);
    assert_eq!(
        read_file(&repo, "a.rs"),
        OLD_A,
        "no working-tree write before the commit point"
    );
    assert_eq!(last_outcome_status(&journal).as_deref(), Some("REFUSED"));
    drop(_guard);
    assert_repository_not_corrupted(&repo);

    // After the permission is restored, the same publication succeeds exactly once.
    let result = publisher
        .publish_request(&modify_a(&repo, NEW_A, "perm-git-protected"))
        .expect("publication after restoring permissions");
    assert_eq!(result.parent, before);
    assert_eq!(commit_count(&repo), 2);
    assert_eq!(read_file(&repo, "a.rs"), NEW_A);
}

// ---------------------------------------------------------------------------
// 8. journal failures (before and after the commit point)
// ---------------------------------------------------------------------------

#[test]
fn unwritable_journal_before_publication_publishes_nothing() {
    let repo = init_repo("perm-journal-prepare");
    let journal = fresh_journal("perm-journal-prepare");
    let publisher = writer(&repo)
        .with_publication_journal(&journal)
        .expect("journal");
    // Replace the journal file with a directory: reading and appending both fail.
    fs::remove_file(&journal).expect("remove journal file");
    fs::create_dir(&journal).expect("journal path as directory");
    let before = head(&repo);

    let error = publisher
        .prepare(&modify_a(&repo, NEW_A, "perm-journal-prepare"))
        .expect_err("journal failure must be fatal");
    assert!(
        matches!(error, GitError::Journal(_)),
        "unexpected error: {error}"
    );
    assert_eq!(
        head(&repo),
        before,
        "nothing may be published without a durable journal"
    );
    assert_eq!(commit_count(&repo), 1);
    assert_eq!(read_file(&repo, "a.rs"), OLD_A);
    fs::remove_dir(&journal).expect("restore journal path");
    assert_repository_not_corrupted(&repo);

    // A missing journal file is also fail-closed: the writer refuses to publish
    // without a durable record instead of silently running journal-less.
    let error = publisher
        .publish_request(&modify_a(&repo, NEW_A, "perm-journal-prepare"))
        .expect_err("a missing journal must refuse the publication");
    assert!(
        matches!(error, GitError::Journal(_)),
        "unexpected error: {error}"
    );
    assert_eq!(
        head(&repo),
        before,
        "nothing published without a journal file"
    );
    assert_eq!(commit_count(&repo), 1);

    // Operator repair: recreate an empty append-only journal, then publish.
    fs::write(&journal, b"").expect("recreate empty journal");
    let result = publisher
        .publish_request(&modify_a(&repo, NEW_A, "perm-journal-prepare"))
        .expect("publication after restoring the journal");
    assert_eq!(result.parent, before);
    assert_eq!(commit_count(&repo), 2);
    assert_eq!(last_outcome_status(&journal).as_deref(), Some("PUBLISHED"));
}

#[test]
fn journal_failure_after_swap_is_recovered_without_double_publication() {
    let repo = init_repo("perm-journal-publish");
    let journal = fresh_journal("perm-journal-publish");
    let before = head(&repo);
    let publisher = writer(&repo)
        .with_publication_journal(&journal)
        .expect("journal");
    let prepared = publisher
        .prepare(&modify_a(&repo, NEW_A, "perm-journal-publish"))
        .expect("prepare");
    let _guard = ReadOnlyGuard::new(journal.clone());

    let error = publisher
        .publish(prepared)
        .expect_err("outcome append must fail on a read-only journal");
    assert!(
        matches!(error, GitError::Journal(_)),
        "unexpected error: {error}"
    );
    let published = head(&repo);
    assert_ne!(
        published, before,
        "the commit point happened before the journal failure"
    );
    assert_eq!(commit_count(&repo), 2);
    assert_eq!(head_file(&repo, "a.rs"), NEW_A);
    assert_eq!(read_file(&repo, "a.rs"), NEW_A);
    assert_eq!(index_file(&repo, "a.rs"), NEW_A);
    assert_eq!(
        status(&repo).trim(),
        "",
        "canonical and working state are complete"
    );
    assert_eq!(intents(&journal).len(), 1, "the INTENT is durable");
    assert!(
        outcomes(&journal).is_empty(),
        "the OUTCOME could not be written"
    );
    drop(_guard);
    assert_repository_not_corrupted(&repo);

    let restarted = writer(&repo)
        .with_publication_journal(&journal)
        .expect("journal reopen");
    let report = restarted.recover().expect("recover");
    assert_eq!(report.published, vec!["perm-journal-publish".to_owned()]);
    assert_eq!(
        last_outcome_status(&journal).as_deref(),
        Some("RECOVERED_PUBLISHED")
    );
    let retry = restarted
        .prepare(&modify_a(&repo, NEW_A, "perm-journal-publish"))
        .expect_err("already published");
    assert!(matches!(retry, GitError::AlreadyPublished { .. }));
    assert_eq!(commit_count(&repo), 2, "no double publication");
}

// ---------------------------------------------------------------------------
// 9. generated / auxiliary files: preserved, never silently deleted or overwritten
// ---------------------------------------------------------------------------

#[test]
fn modified_generated_and_auxiliary_files_are_preserved_by_publication() {
    let repo = init_repo("generated-preserved");
    // Files that generators / the AI tooling own, already modified in the worktree.
    write_file(&repo, "BOARD.md", "board v1\n");
    write_file(&repo, "LOG.md", "log v1\n");
    write_file(&repo, "target/ravum/board.json", "{\"version\":1}\n");
    write_file(&repo, "b.rs", "fn b() { 99 }\n");
    let board_before = read_file(&repo, "BOARD.md");
    let log_before = read_file(&repo, "LOG.md");
    let target_before = read_file(&repo, "target/ravum/board.json");
    let b_before = read_file(&repo, "b.rs");

    let result = writer(&repo)
        .publish_request(&modify_a(&repo, NEW_A, "generated-preserved"))
        .expect("publication of the unrelated path");
    assert_eq!(result.paths, vec!["a.rs".to_owned()]);
    assert_eq!(read_file(&repo, "BOARD.md"), board_before);
    assert_eq!(read_file(&repo, "LOG.md"), log_before);
    assert_eq!(read_file(&repo, "target/ravum/board.json"), target_before);
    assert_eq!(
        read_file(&repo, "b.rs"),
        b_before,
        "unrelated work preserved"
    );
    assert_eq!(git(&repo, &["ls-files", "--", "BOARD.md", "LOG.md"]), "");
    assert!(
        status(&repo).contains("BOARD.md") && status(&repo).contains("LOG.md"),
        "generated files stay untracked/touched, not deleted: {}",
        status(&repo)
    );

    // A caller asking to publish generated content without an explicit decision is
    // refused, and a divergent generated file is never overwritten silently.
    let protected = PublishRequest::new("write lockfile")
        .with_change(PathUpdate::write("Cargo.lock", "# generated\n"));
    let error = writer(&repo)
        .publish_request(&protected)
        .expect_err("generated path must be protected");
    assert!(matches!(error, GitError::ProtectedPath { .. }));
    assert!(
        !repo.join("Cargo.lock").exists(),
        "no generated file may be created silently"
    );

    write_file(&repo, "Cargo.lock", "# generated v1\n");
    let divergent = PublishRequest::new("overwrite lockfile").with_change(
        PathUpdate::write("Cargo.lock", "# generated v2\n")
            .with_expected_old_sha256("0".repeat(64)),
    );
    let error = writer(&repo)
        .publish_request(&divergent)
        .expect_err("divergent generated content must be refused");
    assert!(
        matches!(
            error,
            GitError::ProtectedPath { .. } | GitError::StaleWorkingTree { .. }
        ),
        "unexpected error: {error}"
    );
    assert_eq!(
        read_file(&repo, "Cargo.lock"),
        "# generated v1\n",
        "no silent overwrite"
    );
    assert_repository_not_corrupted(&repo);
}
