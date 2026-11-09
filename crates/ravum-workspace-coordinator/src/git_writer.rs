//! The single Git writer.
//!
//! Only this type runs mutating Git commands. It commits *only* the paths it was
//! given (`git commit --only -- <paths>`), so unrelated staged or unstaged work
//! belonging to other agents is never swept into the commit. After the commit it
//! verifies that the new commit's parent is exactly the HEAD it observed, which
//! detects a concurrent writer instead of silently building on it.

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct CommitResult {
    pub sha: String,
    pub parent: String,
    pub paths: Vec<String>,
}

#[derive(Debug)]
pub enum GitError {
    Io(String),
    Git { command: String, output: String },
    ConcurrentWriter { expected: String, found: String },
    NothingToCommit,
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitError::Io(d) => write!(f, "git io error: {d}"),
            GitError::Git { command, output } => write!(f, "{command} failed: {output}"),
            GitError::ConcurrentWriter { expected, found } => write!(
                f,
                "concurrent writer detected: expected parent {expected}, got {found}"
            ),
            GitError::NothingToCommit => write!(f, "nothing to commit for the given paths"),
        }
    }
}

impl std::error::Error for GitError {}

pub struct GitWriter {
    repo: PathBuf,
    author_name: String,
    author_email: String,
}

impl GitWriter {
    pub fn new(repo: impl Into<PathBuf>, author_name: impl Into<String>, author_email: impl Into<String>) -> Self {
        Self {
            repo: repo.into(),
            author_name: author_name.into(),
            author_email: author_email.into(),
        }
    }

    pub fn repo(&self) -> &Path {
        &self.repo
    }

    pub fn head(&self) -> Result<String, GitError> {
        let text = self.git(&["rev-parse", "HEAD"])?;
        Ok(text.trim().to_owned())
    }

    /// Commit exactly `paths` (workspace-relative) with an attributing message.
    pub fn commit_paths(
        &self,
        paths: &[String],
        subject: &str,
        trailers: &[(String, String)],
    ) -> Result<CommitResult, GitError> {
        if paths.is_empty() {
            return Err(GitError::NothingToCommit);
        }
        let before = self.head()?;
        let mut message = subject.to_owned();
        if !trailers.is_empty() {
            message.push_str("\n\n");
            for (key, value) in trailers {
                message.push_str(&format!("{key}: {value}\n"));
            }
        }

        let mut args: Vec<String> = vec![
            "commit".into(),
            "--only".into(),
            "--no-verify".into(),
            "-m".into(),
            message,
            "--".into(),
        ];
        args.extend(paths.iter().cloned());
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        match self.git(&refs) {
            Ok(_) => {}
            Err(GitError::Git { output, .. })
                if output.contains("nothing to commit")
                    || output.contains("no changes added to commit") =>
            {
                return Err(GitError::NothingToCommit)
            }
            Err(e) => return Err(e),
        }

        let after = self.head()?;
        let parent = self.git(&["rev-parse", "HEAD^"])?.trim().to_owned();
        if parent != before {
            return Err(GitError::ConcurrentWriter {
                expected: before,
                found: parent,
            });
        }
        Ok(CommitResult {
            sha: after,
            parent,
            paths: paths.to_vec(),
        })
    }

    fn git(&self, args: &[&str]) -> Result<String, GitError> {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.repo)
            .args(args)
            .env("GIT_AUTHOR_NAME", &self.author_name)
            .env("GIT_AUTHOR_EMAIL", &self.author_email)
            .env("GIT_COMMITTER_NAME", &self.author_name)
            .env("GIT_COMMITTER_EMAIL", &self.author_email)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .map_err(|e| GitError::Io(e.to_string()))?;
        let command = format!("git {}", args.join(" "));
        if !output.status.success() {
            let mut text = String::from_utf8_lossy(&output.stderr).to_string();
            text.push_str(&String::from_utf8_lossy(&output.stdout));
            return Err(GitError::Git {
                command,
                output: text,
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}
