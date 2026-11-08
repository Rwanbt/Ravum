//! Transactional Git publication: the twelve negative scenarios of the mandate.
//!
//! Every test builds a throw-away repository under `target/ravum-tests/` and only
//! asserts observable Git state (references, trees, index, working tree, journal
//! lines). None of them trusts the writer's own claim: the repository is re-read
//! with `git` plumbing after each attempt.

use ravum_protocol::{sha256_hex, ChangeProposal, DecisionKind, COLLAB_SCHEMA_VERSION};
use ravum_snapshot::SourceSnapshot;
use ravum_workspace_coordinator::{
    Coordinator, GitError, GitWriter, PathUpdate, PublicationRecord, PublicationStatus,
    PublishRequest, RecoveryReport,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Barrier};

const OLD_A: &str = "fn a() { 1 }\n";
const NEW_A: &str = "fn a() { 2 }\n";
const OLD_B: &str = "fn b() { 1 }\n";

// ---------------------------------------------------------------------------
// fixtures
// ---------------------------------------------------------------------------

fn tests_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ravum-tests")
}

fn test_dir(name: &str) -> PathBuf {
    let dir = tests_root().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create test directory");
    dir
}

fn fresh_journal(name: &str) -> PathBuf {
    let path = tests_root().join(name).with_extension("journal.jsonl");
    let _ = std::fs::remove_file(&path);
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

fn git_try(dir: &Path, args: &[&str]) -> Output {
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
    // Byte-deterministic fixtures: no end-of-line conversion in the test repo.
    git(&repo, &["config", "core.autocrlf", "false"]);
    git(&repo, &["config", "commit.gpgsign", "false"]);
    write_file(&repo, "a.rs", OLD_A);
    write_file(&repo, "b.rs", OLD_B);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    repo
}

fn write_file(repo: &Path, relative: &str, content: &str) {
    let path = repo.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent directory");
    }
    std::fs::write(&path, content).expect("write file");
}

fn read_file(repo: &Path, relative: &str) -> String {
    std::fs::read_to_string(repo.join(relative)).expect("read file")
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

fn base_hash(repo: &Path, path: &str) -> String {
    sha256_hex(head_file(repo, path).as_bytes())
}

fn writer(repo: &Path) -> GitWriter {
    GitWriter::new(repo, "agent-a", "agent-a@ravum.invalid")
}

fn install_hook(repo: &Path, name: &str, body: &str) {
    let hooks = repo.join(".git").join("hooks");
    std::fs::create_dir_all(&hooks).expect("create hooks directory");
    let path = hooks.join(name);
    std::fs::write(&path, body).expect("write hook");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&path)
            .expect("hook metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).expect("make hook executable");
    }
}

/// Request that rewrites `a.rs` to `target`, validated against the current `HEAD`
/// content of `a.rs`. Optionally pins the publication id.
fn modify_a(repo: &Path, target: &str, publication_id: Option<&str>) -> PublishRequest {
    let mut request = PublishRequest::new("feat(collab): integrate agent A")
        .with_change(
            PathUpdate::write("a.rs", target).with_expected_old_sha256(base_hash(repo, "a.rs")),
        )
        .with_trailer("Ravum-Proposal", "agent-a/t1")
        .with_trailer("Ravum-Agent", "agent-a");
    if let Some(id) = publication_id {
        request = request.with_publication_id(id);
    }
    request
}

fn coordinator_with_lib() -> Coordinator {
    Coordinator::new(SourceSnapshot::new([(
        "lib.rs".to_string(),
        "fn alpha() -> u32 {\n    1\n}\n".to_string(),
    )]))
}

// ---------------------------------------------------------------------------
// 1. two simultaneous proposals: at most one publication
// ---------------------------------------------------------------------------

#[test]
fn concurrent_proposals_publish_at_most_one() {
    let repo = init_repo("txn-concurrent");
    let base = head(&repo);
    let writer = Arc::new(writer(&repo));
    let barrier = Arc::new(Barrier::new(2));
    let mut handles = Vec::new();
    for (path, content, expected_old) in [
        ("a.rs", "fn a() { 2 }\n", base_hash(&repo, "a.rs")),
        ("b.rs", "fn b() { 2 }\n", base_hash(&repo, "b.rs")),
    ] {
        let writer = Arc::clone(&writer);
        let barrier = Arc::clone(&barrier);
        let path = path.to_owned();
        let content = content.to_owned();
        handles.push(std::thread::spawn(move || {
            let request = PublishRequest::new("concurrent publication")
                .with_change(
                    PathUpdate::write(path.clone(), content).with_expected_old_sha256(expected_old),
                )
                .with_publication_id(format!("concurrent-{path}"));
            let prepared = writer.prepare(&request)?;
            // Both candidates exist before either publication is attempted.
            barrier.wait();
            writer.publish(prepared)
        }));
    }
    let results: Vec<Result<_, GitError>> = handles
        .into_iter()
        .map(|handle| handle.join().expect("publication thread"))
        .collect();
    let published = results.iter().filter(|result| result.is_ok()).count();
    let refused = results
        .iter()
        .filter(|result| matches!(result, Err(GitError::ConcurrentWriter { .. })))
        .count();
    assert_eq!(
        published, 1,
        "exactly one publication must win: {results:?}"
    );
    assert_eq!(
        refused, 1,
        "the loser must be refused as a concurrent writer: {results:?}"
    );
    assert_eq!(
        commit_count(&repo),
        2,
        "one initial commit plus one publication"
    );
    assert_eq!(
        git(&repo, &["rev-parse", "refs/heads/main"]).trim(),
        head(&repo)
    );
    assert_ne!(head(&repo), base, "the branch must have moved exactly once");

    // Exactly one of the two files was published; the loser's file is untouched and
    // the whole working tree is clean (the loser wrote nothing and rolled nothing).
    let a = read_file(&repo, "a.rs");
    let b = read_file(&repo, "b.rs");
    assert!(
        (a == "fn a() { 2 }\n") ^ (b == "fn b() { 2 }\n"),
        "exactly one file published, got a={a:?} b={b:?}"
    );
    assert_eq!(
        status(&repo).trim(),
        "",
        "working tree/index must be consistent"
    );
}

// ---------------------------------------------------------------------------
// 2. an external writer moves HEAD between preparation and publication
// ---------------------------------------------------------------------------

#[test]
fn external_head_move_between_prepare_and_publish_is_refused() {
    let repo = init_repo("txn-external-head");
    let prepared = writer(&repo)
        .prepare(&modify_a(&repo, NEW_A, None))
        .expect("prepare");
    let candidate = prepared.new_commit().to_owned();

    // Another writer publishes on the same branch before we do.
    git(
        &repo,
        &["commit", "--allow-empty", "-q", "-m", "external writer"],
    );
    let external_head = head(&repo);

    let error = writer(&repo)
        .publish(prepared)
        .expect_err("publication must be refused");
    assert!(
        matches!(
            error,
            GitError::ConcurrentWriter { ref found, .. } if found == &external_head
        ),
        "unexpected error: {error}"
    );
    assert_eq!(head(&repo), external_head, "the reference must not move");
    assert_eq!(commit_count(&repo), 2, "no publication was added");
    assert!(
        !git(&repo, &["rev-list", "--all"]).contains(&candidate),
        "the abandoned candidate commit must stay unreachable"
    );
}

// ---------------------------------------------------------------------------
// 3. a failed commit creation publishes nothing
// ---------------------------------------------------------------------------

#[test]
fn failed_preparation_publishes_nothing() {
    let repo = init_repo("txn-prepare-failure");
    let before = head(&repo);

    // A precondition that can never hold: the file is not the content the caller
    // claims to have validated.
    let stale = PublishRequest::new("stale precondition")
        .with_change(PathUpdate::write("a.rs", NEW_A).with_expected_old_sha256("0".repeat(64)));
    let error = writer(&repo).prepare(&stale).expect_err("must refuse");
    assert!(
        matches!(error, GitError::StaleWorkingTree { .. }),
        "unexpected error: {error}"
    );

    // A request that cannot be assembled at all.
    let duplicate = PublishRequest::new("duplicate path")
        .with_change(PathUpdate::write("a.rs", "fn a() { 2 }\n"))
        .with_change(PathUpdate::write("./a.rs", "fn a() { 3 }\n"));
    let error = writer(&repo).prepare(&duplicate).expect_err("must refuse");
    assert!(
        matches!(error, GitError::InvalidRequest(_)),
        "unexpected error: {error}"
    );

    assert_eq!(head(&repo), before, "no reference moved");
    assert_eq!(commit_count(&repo), 1, "no commit object was published");
    assert_eq!(read_file(&repo, "a.rs"), OLD_A, "working tree untouched");
    assert_eq!(status(&repo).trim(), "");
}

// ---------------------------------------------------------------------------
// 4. a local hook refuses: nothing is published, the hook really ran
// ---------------------------------------------------------------------------

#[test]
fn refusing_pre_commit_hook_blocks_publication_and_really_runs() {
    let repo = init_repo("txn-hook-refusal");
    // The marker lives outside the repository so it cannot pollute `git status`.
    install_hook(
        &repo,
        "pre-commit",
        "#!/bin/sh\necho ran > \"$(git rev-parse --show-toplevel)/../hook-ran\"\nexit 1\n",
    );
    let marker = repo.parent().expect("parent").join("hook-ran");
    let _ = std::fs::remove_file(&marker);
    let before = head(&repo);

    let error = writer(&repo)
        .publish_request(&modify_a(&repo, NEW_A, None))
        .expect_err("the hook must refuse");
    assert!(
        matches!(error, GitError::HookFailed { ref hook, .. } if hook == "pre-commit"),
        "unexpected error: {error}"
    );
    assert!(
        marker.is_file(),
        "the pre-commit hook must really have been executed"
    );
    assert_eq!(
        head(&repo),
        before,
        "a hook refusal must not move the reference"
    );
    assert_eq!(commit_count(&repo), 1);
    assert_eq!(read_file(&repo, "a.rs"), OLD_A);
}

#[test]
fn commit_msg_hook_is_executed_and_its_rewrite_is_published() {
    let repo = init_repo("txn-commit-msg");
    install_hook(
        &repo,
        "commit-msg",
        "#!/bin/sh\nprintf 'Signed-off-by: hook <hook@ravum.invalid>\\n' >> \"$1\"\n",
    );
    let result = writer(&repo)
        .publish_request(&modify_a(&repo, NEW_A, None))
        .expect("publish");
    let body = git(&repo, &["log", "-1", "--format=%B"]);
    assert!(
        body.contains("Signed-off-by: hook <hook@ravum.invalid>"),
        "commit-msg rewrite must be honoured, body: {body}"
    );
    assert!(
        body.contains("Ravum-Agent: agent-a"),
        "attribution kept: {body}"
    );
    assert!(
        body.contains(&format!("Ravum-Publication-Id: {}", result.publication_id)),
        "publication attribution kept: {body}"
    );
}

#[test]
fn reference_transaction_hook_can_abort_the_publication() {
    let repo = init_repo("txn-reference-transaction");
    install_hook(
        &repo,
        "reference-transaction",
        "#!/bin/sh\nif [ \"$1\" = prepared ]; then exit 1; fi\nexit 0\n",
    );
    let before = head(&repo);
    let error = writer(&repo)
        .publish_request(&modify_a(&repo, NEW_A, None))
        .expect_err("the reference-transaction hook must abort the update");
    // The exact plumbing error is surfaced explicitly; what matters is that the
    // reference did not move and the attempt is reported as an error.
    assert!(
        matches!(
            error,
            GitError::Git { .. } | GitError::ConcurrentWriter { .. }
        ),
        "unexpected error: {error}"
    );
    assert_eq!(head(&repo), before, "the reference must not move");
    assert_eq!(read_file(&repo, "a.rs"), OLD_A);
}

// ---------------------------------------------------------------------------
// 5. unrelated worked-on files are preserved byte for byte
// ---------------------------------------------------------------------------

#[test]
fn unrelated_work_in_progress_is_preserved() {
    let repo = init_repo("txn-unrelated-work");
    // Another agent's modification, staged in the real index.
    write_file(&repo, "b.rs", "fn b() { 99 }\n");
    git(&repo, &["add", "b.rs"]);
    let b_index_before = git(&repo, &["ls-files", "-s", "--", "b.rs"]);
    // And an untracked file.
    write_file(&repo, "notes.txt", "not ours\n");

    let result = writer(&repo)
        .publish_request(&modify_a(&repo, NEW_A, None))
        .expect("publish");
    assert_eq!(result.paths, vec!["a.rs".to_owned()]);

    assert_eq!(
        read_file(&repo, "b.rs"),
        "fn b() { 99 }\n",
        "unrelated file preserved"
    );
    assert_eq!(
        git(&repo, &["ls-files", "-s", "--", "b.rs"]),
        b_index_before,
        "the real index entry of an unrelated path must not change"
    );
    assert_eq!(read_file(&repo, "notes.txt"), "not ours\n");
    let status = status(&repo);
    assert!(
        status.contains("b.rs"),
        "unrelated staged work stays visible: {status}"
    );
    assert!(
        status.contains("notes.txt"),
        "untracked file stays untracked: {status}"
    );
    assert!(
        !status.lines().any(|line| line.trim_end().ends_with("a.rs")),
        "the published path must be clean: {status}"
    );
    assert_eq!(read_file(&repo, "a.rs"), NEW_A);
}

// ---------------------------------------------------------------------------
// 6. additions, deletions and a rename produce the expected tree
// ---------------------------------------------------------------------------

#[test]
fn additions_deletions_and_renames_produce_the_expected_tree() {
    let repo = init_repo("txn-tree-shapes");
    write_file(&repo, "old.rs", "fn old() {}\n");
    write_file(&repo, "move_from.rs", "fn moving() {}\n");
    git(&repo, &["add", "old.rs", "move_from.rs"]);
    git(&repo, &["commit", "-q", "-m", "add shapes"]);
    let old_hash = base_hash(&repo, "old.rs");
    let move_hash = base_hash(&repo, "move_from.rs");

    let request = PublishRequest::new("tree shapes")
        .with_change(PathUpdate::delete("old.rs").with_expected_old_sha256(old_hash))
        .with_change(PathUpdate::delete("move_from.rs").with_expected_old_sha256(move_hash))
        .with_change(
            PathUpdate::write("move_to.rs", "fn moving() {}\n").with_expected_old_sha256(""),
        )
        .with_change(
            PathUpdate::write("nested/added.rs", "fn added() {}\n").with_expected_old_sha256(""),
        );
    writer(&repo).publish_request(&request).expect("publish");

    let names: BTreeSet<String> = git(&repo, &["ls-tree", "-r", "--name-only", "HEAD"])
        .lines()
        .map(ToOwned::to_owned)
        .collect();
    assert!(names.contains("move_to.rs"), "tree: {names:?}");
    assert!(names.contains("nested/added.rs"), "tree: {names:?}");
    assert!(!names.contains("old.rs"), "tree: {names:?}");
    assert!(!names.contains("move_from.rs"), "tree: {names:?}");
    assert_eq!(head_file(&repo, "move_to.rs"), "fn moving() {}\n");
    assert!(!repo.join("old.rs").exists());
    assert!(!repo.join("move_from.rs").exists());
    assert_eq!(read_file(&repo, "nested/added.rs"), "fn added() {}\n");
    assert_eq!(status(&repo).trim(), "", "published tree must be clean");
}

// ---------------------------------------------------------------------------
// 7. a file changed after validation is refused, fail-closed
// ---------------------------------------------------------------------------

#[test]
fn a_file_modified_between_validation_and_publication_is_refused() {
    let repo = init_repo("txn-mutated-after-validation");
    let prepared = writer(&repo)
        .prepare(&modify_a(&repo, NEW_A, None))
        .expect("prepare");
    // An external edit lands after validation and before publication.
    write_file(&repo, "a.rs", "fn a() { EXTERNAL }\n");
    let before = head(&repo);

    let error = writer(&repo)
        .publish(prepared)
        .expect_err("publication must be refused");
    assert!(
        matches!(error, GitError::StaleWorkingTree { ref path, .. } if path == "a.rs"),
        "unexpected error: {error}"
    );
    assert_eq!(head(&repo), before, "nothing published");
    assert_eq!(
        read_file(&repo, "a.rs"),
        "fn a() { EXTERNAL }\n",
        "the external content must never be overwritten"
    );
}

#[test]
fn a_target_already_materialized_by_the_caller_is_not_rewritten() {
    let repo = init_repo("txn-materialized");
    let prepared = writer(&repo)
        .prepare(&modify_a(&repo, NEW_A, None))
        .expect("prepare");
    // The caller materialized the exact published content before publication.
    write_file(&repo, "a.rs", NEW_A);
    let modified_before = std::fs::metadata(repo.join("a.rs"))
        .expect("metadata")
        .modified()
        .expect("mtime");
    let result = writer(&repo).publish(prepared).expect("publish");
    assert!(
        result.warnings.is_empty(),
        "warnings: {:?}",
        result.warnings
    );
    let modified_after = std::fs::metadata(repo.join("a.rs"))
        .expect("metadata")
        .modified()
        .expect("mtime");
    assert_eq!(
        modified_before, modified_after,
        "a file already identical to the published blob must not be touched"
    );
}

// ---------------------------------------------------------------------------
// 8. interruption after write-tree/commit-tree: no partial reference update
// ---------------------------------------------------------------------------

#[test]
fn interruption_after_commit_tree_leaves_no_partial_state_and_restarts_cleanly() {
    let repo = init_repo("txn-interrupted");
    let journal = fresh_journal("txn-interrupted");
    let journaled = writer(&repo)
        .with_publication_journal(&journal)
        .expect("publication journal");
    let before = head(&repo);
    let request = modify_a(&repo, NEW_A, Some("interrupted-1"));

    let prepared = journaled.prepare(&request).expect("prepare");
    let candidate = prepared.new_commit().to_owned();
    drop(prepared); // simulated crash right after write-tree/commit-tree

    assert_eq!(head(&repo), before, "no reference may move before publish");
    assert_eq!(commit_count(&repo), 1);
    assert!(
        !git(&repo, &["rev-list", "--all"]).contains(&candidate),
        "the candidate commit must not be reachable"
    );
    assert_eq!(
        journaled
            .publication_status("interrupted-1")
            .expect("status"),
        None,
        "a pending intent has no outcome after the crash"
    );

    // Restart: reopen the journal, resolve the pending intent, then publish.
    let restarted = writer(&repo)
        .with_publication_journal(&journal)
        .expect("journal reopen");
    let report = restarted.recover().expect("recover");
    assert_eq!(
        report.unpublished,
        vec!["interrupted-1".to_owned()],
        "report: {report:?}"
    );
    assert_eq!(
        restarted
            .publication_status("interrupted-1")
            .expect("status"),
        Some(PublicationStatus::RecoveredUnpublished)
    );

    let result = restarted
        .publish_request(&request)
        .expect("publication after recovery");
    assert_eq!(result.parent, before);
    assert_eq!(commit_count(&repo), 2, "exactly one publication");
    assert_eq!(read_file(&repo, "a.rs"), NEW_A);
    assert_eq!(
        restarted
            .publication_status("interrupted-1")
            .expect("status"),
        Some(PublicationStatus::Published)
    );
}

// ---------------------------------------------------------------------------
// 9. a wrong expected old value: the reference swap fails, ref unchanged
// ---------------------------------------------------------------------------

#[test]
fn a_wrong_expected_old_makes_the_reference_swap_fail_without_moving_it() {
    let repo = init_repo("txn-compare-and-swap");
    let writer = writer(&repo);
    let before = head(&repo);
    let prepared = writer
        .prepare(&modify_a(&repo, NEW_A, None))
        .expect("prepare");
    let candidate = prepared.new_commit().to_owned();

    // The candidate is a valid commit id but certainly not the current value.
    let error = writer
        .compare_and_swap_ref("refs/heads/main", &candidate, &candidate)
        .expect_err("the swap must fail");
    assert!(
        matches!(
            error,
            GitError::ConcurrentWriter { ref expected, ref found }
                if expected == &candidate && found == &before
        ),
        "unexpected error: {error}"
    );
    assert_eq!(head(&repo), before, "the reference must be unchanged");

    // The same swap with the observed old value succeeds.
    writer
        .compare_and_swap_ref("refs/heads/main", &candidate, &before)
        .expect("swap with the observed old value");
    assert_eq!(head(&repo), candidate);
}

// ---------------------------------------------------------------------------
// 10. an attempt to write Git metadata is refused
// ---------------------------------------------------------------------------

#[test]
fn paths_inside_git_metadata_are_refused_by_the_writer() {
    let repo = init_repo("txn-dot-git");
    let config_before = read_file(&repo, ".git/config");
    let before = head(&repo);
    for path in [
        ".git/config",
        ".git/hooks/pre-commit",
        "../escape.rs",
        "/absolute.rs",
    ] {
        let request =
            PublishRequest::new("metadata write").with_change(PathUpdate::write(path, "pwned\n"));
        let error = writer(&repo)
            .publish_request(&request)
            .expect_err("unsafe path must be refused");
        assert!(
            matches!(error, GitError::UnsafePath { .. }),
            "{path}: unexpected error: {error}"
        );
    }
    assert_eq!(
        read_file(&repo, ".git/config"),
        config_before,
        ".git/config untouched"
    );
    assert_eq!(head(&repo), before);
}

#[test]
fn a_proposal_targeting_git_metadata_is_rejected_by_the_coordinator() {
    let mut coordinator = coordinator_with_lib();
    let snapshot_before = coordinator.current_id();
    let proposal = ChangeProposal {
        schema_version: COLLAB_SCHEMA_VERSION,
        agent_id: "agent-a".to_owned(),
        task_id: "t1".to_owned(),
        base_snapshot_id: coordinator.current_id(),
        target_paths: vec![".git/config".to_owned()],
        target_symbols: vec![],
        expected_hashes: BTreeMap::new(),
        declared_dependencies: vec![],
        impacted_contracts: vec![],
        unified_patch: "+++ b/.git/config\n@@ -1 +1 @@\n-a\n+b\n".to_owned(),
        note: None,
    };
    let decision = coordinator.submit(&proposal);
    assert_eq!(decision.decision, DecisionKind::Reject);
    assert!(
        decision
            .reasons
            .iter()
            .any(|reason| reason.contains(".git")),
        "the refusal must name the metadata path: {:?}",
        decision.reasons
    );
    assert_eq!(
        coordinator.current_id(),
        snapshot_before,
        "nothing integrated"
    );
}

// ---------------------------------------------------------------------------
// 11. generated content: an explicit decision, never a silent overwrite
// ---------------------------------------------------------------------------

#[test]
fn generated_content_requires_an_explicit_decision() {
    let mut coordinator = coordinator_with_lib();
    let proposal = ChangeProposal {
        schema_version: COLLAB_SCHEMA_VERSION,
        agent_id: "agent-a".to_owned(),
        task_id: "t1".to_owned(),
        base_snapshot_id: coordinator.current_id(),
        target_paths: vec!["Cargo.lock".to_owned()],
        target_symbols: vec![],
        expected_hashes: BTreeMap::new(),
        declared_dependencies: vec![],
        impacted_contracts: vec![],
        unified_patch: "+++ b/Cargo.lock\n@@ -1 +1 @@\n-a\n+b\n".to_owned(),
        note: None,
    };
    let decision = coordinator.submit(&proposal);
    assert_eq!(
        decision.decision,
        DecisionKind::ManualRequired,
        "a generated file needs an explicit decision: {:?}",
        decision.reasons
    );

    let repo = init_repo("txn-generated");
    write_file(&repo, "Cargo.lock", "# generated v1\n");
    let before = head(&repo);
    let request = PublishRequest::new("generated file").with_change(
        PathUpdate::write("Cargo.lock", "# generated v2\n").with_expected_old_sha256(""),
    );
    let error = writer(&repo)
        .publish_request(&request)
        .expect_err("generated content must be refused without an explicit decision");
    assert!(
        matches!(error, GitError::ProtectedPath { ref path, .. } if path == "Cargo.lock"),
        "unexpected error: {error}"
    );
    assert_eq!(
        read_file(&repo, "Cargo.lock"),
        "# generated v1\n",
        "no silent overwrite"
    );
    assert_eq!(head(&repo), before);

    // Explicit opt-in: the decision is taken, documented in the message, and the
    // publication happens.
    let existing = sha256_hex(b"# generated v1\n");
    let request = PublishRequest::new("generated file, explicitly accepted")
        .with_change(
            PathUpdate::write("Cargo.lock", "# generated v2\n").with_expected_old_sha256(existing),
        )
        .with_trailer("Ravum-Decision", "generated-path-accepted")
        .allowing_generated_paths();
    writer(&repo)
        .publish_request(&request)
        .expect("explicit opt-in publishes");
    assert_eq!(head_file(&repo, "Cargo.lock"), "# generated v2\n");
    assert_eq!(status(&repo).trim(), "");
}

// ---------------------------------------------------------------------------
// 12. a coordinator restart: journal replayed, no double publication
// ---------------------------------------------------------------------------

#[test]
fn a_restart_does_not_publish_twice_and_recovers_an_interrupted_swap() {
    let repo = init_repo("txn-restart");
    let journal = fresh_journal("txn-restart");

    // First run: successful publication, durable journal.
    let first = writer(&repo)
        .with_publication_journal(&journal)
        .expect("publication journal");
    let request = modify_a(&repo, NEW_A, Some("restart-1"));
    first.publish_request(&request).expect("publish");
    assert_eq!(
        first.publication_status("restart-1").expect("status"),
        Some(PublicationStatus::Published)
    );
    // A clean publication must leave exactly one outcome, never a spurious
    // "recovered unpublished" line for an intent that was still live in-process.
    let first_outcomes: Vec<PublicationStatus> = first
        .journal()
        .expect("journal")
        .records()
        .expect("records")
        .iter()
        .filter_map(|record| match record {
            PublicationRecord::Outcome {
                publication_id,
                status,
                ..
            } if publication_id == "restart-1" => Some(*status),
            _ => None,
        })
        .collect();
    assert_eq!(
        first_outcomes,
        vec![PublicationStatus::Published],
        "a live intent must not be reported as recovered"
    );
    drop(first);
    let head_after_first = head(&repo);
    assert_eq!(commit_count(&repo), 2);

    // Restart: a new writer reopens the journal, sees no pending intent, and
    // refuses to publish the same proposal a second time.
    let second = writer(&repo)
        .with_publication_journal(&journal)
        .expect("journal reopen");
    let report = second.recover().expect("recover");
    assert_eq!(
        report,
        RecoveryReport::default(),
        "nothing pending after a clean run"
    );
    let error = second.prepare(&request).expect_err("already published");
    assert!(
        matches!(
            error,
            GitError::AlreadyPublished { ref publication_id, ref commit }
                if publication_id == "restart-1" && commit == &head_after_first
        ),
        "unexpected error: {error}"
    );
    assert_eq!(head(&repo), head_after_first, "no second publication");
    assert_eq!(commit_count(&repo), 2);

    // Crash window between the reference swap and the outcome line: recovery
    // observes the swap instead of publishing again.
    let third_request = modify_a(&repo, "fn a() { 3 }\n", Some("restart-2"));
    let third = writer(&repo)
        .with_publication_journal(&journal)
        .expect("journal reopen");
    let prepared = third.prepare(&third_request).expect("prepare");
    let candidate = prepared.new_commit().to_owned();
    let current = head(&repo);
    git(
        &repo,
        &["update-ref", "refs/heads/main", &candidate, &current],
    );
    drop(prepared); // simulated crash after the swap, before the outcome record

    let fourth = writer(&repo)
        .with_publication_journal(&journal)
        .expect("journal reopen");
    let report = fourth.recover().expect("recover");
    assert_eq!(
        report.published,
        vec!["restart-2".to_owned()],
        "report: {report:?}"
    );
    assert_eq!(
        fourth.publication_status("restart-2").expect("status"),
        Some(PublicationStatus::RecoveredPublished)
    );
    let error = fourth
        .prepare(&third_request)
        .expect_err("a recovered publication must not be replayed");
    assert!(
        matches!(error, GitError::AlreadyPublished { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(head(&repo), candidate, "the reference stays as recovered");
    assert_eq!(commit_count(&repo), 3, "no double publication");
}
