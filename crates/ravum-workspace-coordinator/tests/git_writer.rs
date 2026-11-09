//! The single Git writer must never sweep unrelated work into a commit.

use ravum_workspace_coordinator::{GitError, GitWriter};
use std::path::{Path, PathBuf};
use std::process::Command;

fn temp_repo(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/ravum-tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create repo dir");
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.name", "test"]);
    git(&dir, &["config", "user.email", "test@ravum.invalid"]);
    std::fs::write(dir.join("a.rs"), "fn a() {}\n").unwrap();
    std::fs::write(dir.join("b.rs"), "fn b() {}\n").unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "init"]);
    dir
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn commits_only_the_integrated_paths_with_attribution() {
    let repo = temp_repo("gitwriter-paths");
    // Agent A's change (to integrate) and agent B's untouched work.
    std::fs::write(repo.join("a.rs"), "fn a() { println!(\"a\"); }\n").unwrap();
    std::fs::write(repo.join("b.rs"), "fn b() { println!(\"b\"); }\n").unwrap();

    let writer = GitWriter::new(&repo, "agent-a", "agent-a@ravum.invalid");
    let head_before = writer.head().expect("head");
    let result = writer
        .commit_paths(
            &["a.rs".to_owned()],
            "feat(collab): integrate agent A",
            &[
                ("Ravum-Proposal".to_owned(), "agent-a/task".to_owned()),
                ("Ravum-Agent".to_owned(), "agent-a".to_owned()),
            ],
        )
        .expect("commit");

    assert_eq!(result.parent, head_before);
    assert_ne!(result.sha, head_before);

    let body = git(&repo, &["log", "-1", "--format=%B"]);
    assert!(body.contains("Ravum-Agent: agent-a"), "body was: {body}");

    let status = git(&repo, &["status", "--porcelain"]);
    assert!(
        status.contains("b.rs"),
        "agent B's modification must remain uncommitted, status: {status}"
    );
    let committed_b = git(&repo, &["show", "HEAD:b.rs"]);
    assert!(
        committed_b.contains("fn b() {}"),
        "b.rs content must be unchanged in the commit"
    );
}

#[test]
fn committing_again_without_new_changes_is_refused() {
    let repo = temp_repo("gitwriter-nothing");
    let writer = GitWriter::new(&repo, "agent-a", "agent-a@ravum.invalid");
    let err = writer
        .commit_paths(&["a.rs".to_owned()], "chore: no change", &[])
        .expect_err("nothing to commit");
    assert!(
        matches!(err, GitError::NothingToCommit),
        "unexpected: {err:?}"
    );
}
