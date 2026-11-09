//! The single Git writer: *transactional* publication of accepted proposals.
//!
//! # Why this shape
//!
//! The prototype was `git commit --only --no-verify` followed by a check of the new
//! commit's parent *after* the commit existed. Two problems, both measured:
//! hooks were bypassed by construction, and a concurrent writer could already have
//! moved the branch, so "detection" happened after the fact and never prevented
//! anything.
//!
//! Publication is now split in two halves, and only the second one moves a
//! reference:
//!
//! 1. [`GitWriter::prepare`] reads `HEAD` once (`expected_old`), seeds an
//!    **isolated** index through `GIT_INDEX_FILE` (`git read-tree <expected_old>`),
//!    writes exactly the proposed entries into it (`git hash-object -w --stdin`
//!    for blobs, `git update-index --add --cacheinfo` / `--force-remove` for
//!    entries), builds the candidate tree (`git write-tree`), executes the commit
//!    hooks, and creates an **unpublished** commit (`git commit-tree -p
//!    <expected_old> -F -`). No reference has moved, and the real index is
//!    untouched. Every file of the proposed change is checked against the
//!    caller-validated hash before this point.
//! 2. [`GitWriter::publish`] re-validates immediately before publication — `HEAD`
//!    is still `expected_old`, and no published path diverged on disk — then
//!    performs one atomic compare-and-swap: `git update-ref <ref> <new>
//!    <expected_old>`. If the reference moved, Git refuses the swap and the writer
//!    reports [`GitError::ConcurrentWriter`] *without having published anything*.
//!    The orphan commit object stays unreachable until `git gc` (documented
//!    residue; no reference ever points at it).
//! 3. After a successful swap the writer reconciles *only the published paths* in
//!    the real index, and rewrites a working-tree file only when its current bytes
//!    are not already the published blob. A divergence detected before publication
//!    is refused ([`GitError::StaleWorkingTree`]) instead of being overwritten.
//!
//! # Hooks honoured, and how this was measured
//!
//! `git commit-tree` runs **no** hook, so the writer runs the commit hooks itself,
//! in Git's order, with `GIT_INDEX_FILE` pointing at the isolated index and the
//! repository root as working directory:
//!
//! * `prepare-commit-msg` (source `message`),
//! * `pre-commit`,
//! * `commit-msg` — the message file is re-read afterwards, so a hook that edits
//!   the message is honoured, exactly like Git;
//! * `post-commit` runs after a successful publication and its status is recorded
//!   as a warning, never as a failure (Git ignores it too);
//! * `reference-transaction` is honoured **natively** by `git update-ref` inside
//!   the compare-and-swap, so a hook can still abort the publication.
//!
//! `tests/git_publication.rs` proves each of these by installing real hooks and
//! observing the reference. Hooks are only skipped with the explicit
//! [`HookPolicy::Skip`]; [`HookPolicy::Enforce`] is the default.
//!
//! # Documented limits
//!
//! * An interrupted publication that already moved the reference keeps its commit:
//!   that is the commit point. The durable journal
//!   ([`crate::publication_journal`]) records the intent *before* the swap, and
//!   [`GitWriter::recover`] appends the observed outcome instead of publishing a
//!   second time.
//! * Blobs and trees of an abandoned preparation remain in the object database as
//!   unreachable objects until `git gc`; no reference points at them.
//! * Working-tree reconciliation happens after the commit point. A failure there
//!   is reported explicitly ([`GitError::IndexOutOfSync`] or a warning), never
//!   silently forced.
//! * Hooks are trusted local code and run with the caller's privileges.

use crate::path_policy;
use crate::publication_journal::{PublicationJournal, PublicationRecord, PublicationStatus};
use ravum_protocol::{now_unix, sha256_hex};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Environment passed to commands that must use the *real* index.
const NO_ENV: &[(&str, OsString)] = &[];
/// Upper bound on Git/hook output kept in an error value. Keeps diagnostics honest
/// without copying unbounded output (or anything a hook may print) into logs.
const MAX_ERROR_BYTES: usize = 8 * 1024;
/// Environment variables that would redirect Git to another repository/index.
const REDIRECTING_GIT_ENV: [&str; 8] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
    "GIT_PREFIX",
    "GIT_COMMON_DIR",
];

/// Result of one successful publication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitResult {
    /// Published commit.
    pub sha: String,
    /// Commit the published commit was built on (the compare-and-swap old value).
    pub parent: String,
    /// Published paths, canonical repository-relative form.
    pub paths: Vec<String>,
    /// Tree of the published commit.
    pub tree: String,
    /// Identifier used for idempotency in the publication journal.
    pub publication_id: String,
    /// True when the journal already knew this publication and nothing new was
    /// published. `publish` refuses instead, so this is reserved for future
    /// idempotent replay APIs; it is always `false` today.
    pub already_published: bool,
    /// Non-fatal observations: working-tree reconciliation skipped because the
    /// file changed during publication, or a failing `post-commit` hook.
    pub warnings: Vec<String>,
}

/// One path of a publication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathUpdate {
    /// Canonical repository-relative path.
    pub path: String,
    /// `Some(content)` writes the file, `None` deletes it.
    pub content: Option<String>,
    /// sha256 (the same digest convention as `ravum-snapshot`) of the content the
    /// caller validated on disk before the transaction.
    ///
    /// * `None` — derive it strictly from `HEAD`: a working-tree file that differs
    ///   from the committed base is refused as a divergence.
    /// * `Some("")` — the path must be absent (creation case).
    /// * `Some(sha256)` — the path must currently hold exactly this content.
    pub expected_old_sha256: Option<String>,
}

impl PathUpdate {
    pub fn write(path: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            content: Some(content.into()),
            expected_old_sha256: None,
        }
    }

    pub fn delete(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            content: None,
            expected_old_sha256: None,
        }
    }

    /// Declare the base content the caller validated (`""` means "must be absent").
    pub fn with_expected_old_sha256(mut self, sha256: impl Into<String>) -> Self {
        self.expected_old_sha256 = Some(sha256.into());
        self
    }
}

/// A publication request: exactly the paths to write, an attributing message and
/// the opt-ins a caller must state explicitly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishRequest {
    /// Idempotency key recorded in the durable journal. Derived from the request
    /// when `None`.
    pub publication_id: Option<String>,
    pub changes: Vec<PathUpdate>,
    /// Commit subject; the published message is `subject`, a blank line, then the
    /// trailers.
    pub subject: String,
    /// Attribution trailers (for example `Ravum-Proposal` / `Ravum-Agent`).
    pub trailers: Vec<(String, String)>,
    /// When set, publication is refused if `HEAD` is not already this commit.
    pub expected_head: Option<String>,
    /// Explicit human decision required to publish generated/lockfile content.
    pub allow_generated: bool,
}

impl PublishRequest {
    pub fn new(subject: impl Into<String>) -> Self {
        Self {
            publication_id: None,
            changes: Vec::new(),
            subject: subject.into(),
            trailers: Vec::new(),
            expected_head: None,
            allow_generated: false,
        }
    }

    pub fn with_change(mut self, change: PathUpdate) -> Self {
        self.changes.push(change);
        self
    }

    pub fn with_changes(mut self, changes: impl IntoIterator<Item = PathUpdate>) -> Self {
        self.changes.extend(changes);
        self
    }

    pub fn with_trailer(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.trailers.push((key.into(), value.into()));
        self
    }

    pub fn with_trailers(mut self, trailers: impl IntoIterator<Item = (String, String)>) -> Self {
        self.trailers.extend(trailers);
        self
    }

    pub fn with_publication_id(mut self, id: impl Into<String>) -> Self {
        self.publication_id = Some(id.into());
        self
    }

    pub fn with_expected_head(mut self, head: impl Into<String>) -> Self {
        self.expected_head = Some(head.into());
        self
    }

    /// State the explicit decision that makes generated content writable.
    pub fn allowing_generated_paths(mut self) -> Self {
        self.allow_generated = true;
        self
    }
}

/// Whether the commit hooks are executed. Bypassing hooks is never implicit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HookPolicy {
    /// Run `prepare-commit-msg`, `pre-commit`, `commit-msg` and `post-commit`
    /// exactly like Git would; a refusing hook aborts before publication.
    #[default]
    Enforce,
    /// Do not run commit hooks. Only for tests or an explicit operator decision;
    /// `reference-transaction` is still enforced by `git update-ref`.
    Skip,
}

/// A candidate commit that has been assembled but not published.
///
/// Dropping it publishes nothing: all that remains are unreachable Git objects and
/// an `INTENT` journal line that [`GitWriter::recover`] resolves.
#[derive(Debug)]
pub struct PreparedPublication {
    repo: PathBuf,
    publication_id: String,
    reference: String,
    expected_old: String,
    new_commit: String,
    tree: String,
    changes: Vec<ResolvedChange>,
    _temp: TempArea,
}

impl PreparedPublication {
    /// Identifier used for journal idempotency.
    pub fn publication_id(&self) -> &str {
        &self.publication_id
    }

    /// Reference the publication will move (`HEAD` or `refs/heads/<branch>`).
    pub fn reference(&self) -> &str {
        &self.reference
    }

    /// Commit the candidate was built on.
    pub fn expected_old(&self) -> &str {
        &self.expected_old
    }

    /// Candidate (unpublished) commit.
    pub fn new_commit(&self) -> &str {
        &self.new_commit
    }

    /// Candidate tree.
    pub fn tree(&self) -> &str {
        &self.tree
    }

    /// Paths the candidate changes.
    pub fn paths(&self) -> Vec<String> {
        self.changes.iter().map(|c| c.path.clone()).collect()
    }
}

/// One journal intent that recovery could not resolve from the repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryUnknown {
    pub publication_id: String,
    pub reason: String,
}

/// Outcome of [`GitWriter::recover`] on pending journal intents.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    /// Intents whose candidate commit the reference now points at (or is a
    /// descendant of): the publication happened before the crash.
    pub published: Vec<String>,
    /// Intents whose reference is still at the expected old commit: nothing was
    /// published and a later attempt can proceed.
    pub unpublished: Vec<String>,
    /// Intents that cannot be explained from the recorded commits. Kept explicit;
    /// never guessed into a publication.
    pub unknown: Vec<RecoveryUnknown>,
}

/// Every way the single Git writer can refuse or fail.
#[derive(Debug)]
pub enum GitError {
    Io(String),
    Git {
        command: String,
        output: String,
    },
    /// The reference is not the commit the transaction was prepared against. This
    /// is detected *before* publication: no reference was moved.
    ConcurrentWriter {
        expected: String,
        found: String,
    },
    NothingToCommit,
    InvalidRequest(String),
    /// Path is not a safe repository-relative path (`.git/...`, absolute, `..`).
    UnsafePath {
        path: String,
        reason: String,
    },
    /// Generated/lockfile content without the explicit opt-in decision.
    ProtectedPath {
        path: String,
        reason: String,
    },
    /// The working-tree bytes changed after validation. Nothing was overwritten.
    StaleWorkingTree {
        path: String,
        expected: String,
        found: String,
    },
    HookFailed {
        hook: String,
        status: Option<i32>,
        output: String,
    },
    /// The publication id is already recorded as published: a restart must not
    /// publish it a second time.
    AlreadyPublished {
        publication_id: String,
        commit: String,
    },
    EmptyRepository,
    /// A journal record could not be written or read; the transaction stops.
    Journal(String),
    /// Content is not UTF-8, so it cannot be published as a text blob.
    NotUtf8 {
        path: String,
    },
    /// The reference compare-and-swap succeeded but the real index could not be
    /// reconciled. The commit *is* published; this reports the residue explicitly.
    IndexOutOfSync {
        commit: String,
        detail: String,
    },
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitError::Io(detail) => write!(f, "git io error: {detail}"),
            GitError::Git { command, output } => write!(f, "{command} failed: {output}"),
            GitError::ConcurrentWriter { expected, found } => write!(
                f,
                "concurrent writer detected before publication: expected {expected}, found {found}"
            ),
            GitError::NothingToCommit => write!(f, "nothing to commit for the given paths"),
            GitError::InvalidRequest(detail) => write!(f, "invalid publication request: {detail}"),
            GitError::UnsafePath { path, reason } => {
                write!(f, "refusing unsafe path {path:?}: {reason}")
            }
            GitError::ProtectedPath { path, reason } => {
                write!(f, "refusing protected path {path:?}: {reason}")
            }
            GitError::StaleWorkingTree {
                path,
                expected,
                found,
            } => write!(
                f,
                "working tree diverged for {path:?}: expected {expected}, found {found}; nothing published"
            ),
            GitError::HookFailed {
                hook,
                status,
                output,
            } => write!(f, "{hook} hook refused (status {status:?}): {output}"),
            GitError::AlreadyPublished {
                publication_id,
                commit,
            } => write!(
                f,
                "publication {publication_id:?} is already recorded as published at {commit}"
            ),
            GitError::EmptyRepository => {
                write!(f, "repository has no commit yet: publication needs a HEAD")
            }
            GitError::Journal(detail) => write!(f, "publication journal failure: {detail}"),
            GitError::NotUtf8 { path } => {
                write!(f, "refusing non-UTF-8 content for {path:?}")
            }
            GitError::IndexOutOfSync { commit, detail } => write!(
                f,
                "commit {commit} is published but the real index could not be reconciled: {detail}"
            ),
        }
    }
}

impl std::error::Error for GitError {}

/// The single Git writer.
#[derive(Debug, Clone)]
pub struct GitWriter {
    repo: PathBuf,
    author_name: String,
    author_email: String,
    hook_policy: HookPolicy,
    journal: Option<PublicationJournal>,
    /// Publication ids whose `INTENT` was written by *this* writer and is still
    /// alive in this process. `recover` must not mistake them for a crash residue.
    live_intents: Arc<Mutex<BTreeSet<String>>>,
}

impl GitWriter {
    pub fn new(
        repo: impl Into<PathBuf>,
        author_name: impl Into<String>,
        author_email: impl Into<String>,
    ) -> Self {
        Self {
            repo: repo.into(),
            author_name: author_name.into(),
            author_email: author_email.into(),
            hook_policy: HookPolicy::Enforce,
            journal: None,
            live_intents: Arc::new(Mutex::new(BTreeSet::new())),
        }
    }

    pub fn with_hook_policy(mut self, policy: HookPolicy) -> Self {
        self.hook_policy = policy;
        self
    }

    /// Attach the durable publication journal. A failure to open it is returned,
    /// never ignored: an unjournaled writer is not used for accepted proposals.
    pub fn with_publication_journal(mut self, path: impl Into<PathBuf>) -> std::io::Result<Self> {
        self.journal = Some(PublicationJournal::open(path)?);
        Ok(self)
    }

    pub fn journal(&self) -> Option<&PublicationJournal> {
        self.journal.as_ref()
    }

    pub fn repo(&self) -> &Path {
        &self.repo
    }

    /// Current `HEAD`, or [`GitError::EmptyRepository`] when there is no commit.
    pub fn head(&self) -> Result<String, GitError> {
        let output = self.run_raw(&["rev-parse", "--verify", "HEAD"], None, NO_ENV)?;
        if output.status.success() {
            let sha = trim_output(&output.stdout);
            if is_full_hex_sha(&sha) {
                return Ok(sha);
            }
            return Err(GitError::Git {
                command: "git rev-parse --verify HEAD".to_owned(),
                output: format!("unexpected object id {sha:?}"),
            });
        }
        let text = combined_output(&output);
        if text.contains("not a git repository") {
            return Err(GitError::Git {
                command: "git rev-parse --verify HEAD".to_owned(),
                output: text,
            });
        }
        Err(GitError::EmptyRepository)
    }

    /// SHA-256 (convention `ravum-snapshot`) of the blob of `path` in `HEAD`, or
    /// `None` when the path is absent from the committed tree.
    ///
    /// Used by the cycle transaction to prove that the base snapshot really is the
    /// Git state for every changed path before publishing a diff computed against
    /// it. One Git invocation pair per path: cycles change few paths.
    pub fn head_file_sha256(&self, path: &str) -> Result<Option<String>, GitError> {
        let path =
            path_policy::validate_relative_path(path).map_err(|reason| GitError::UnsafePath {
                path: path.to_owned(),
                reason,
            })?;
        let head = self.head()?;
        match self.tree_blob(&head, &path)? {
            None => Ok(None),
            Some(blob) => {
                let bytes = self.git_bytes(&["cat-file", "blob", &blob], None, NO_ENV)?;
                Ok(Some(sha256_hex(&bytes)))
            }
        }
    }

    /// Build the candidate commit without moving any reference.
    ///
    /// On failure nothing canonical changed: no reference moved, the real index and
    /// the working tree were not modified, and a `REFUSED` journal line records the
    /// attempt. The filesystem may already hold the *target* content (a caller may
    /// have materialized it); that is recognised and not rewritten.
    pub fn prepare(&self, request: &PublishRequest) -> Result<PreparedPublication, GitError> {
        let publication_id = request
            .publication_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| derive_publication_id(request));
        match self.prepare_inner(request, &publication_id) {
            Ok(prepared) => Ok(prepared),
            Err(error) => {
                // A duplicate is not a refusal: recording REFUSED would hide the
                // PUBLISHED line that must keep blocking the replay.
                if matches!(error, GitError::AlreadyPublished { .. }) {
                    return Err(error);
                }
                self.record_refusal(&publication_id, None, &refusal_reason(&error))?;
                Err(error)
            }
        }
    }

    /// Publish a prepared commit, or refuse without touching the reference.
    ///
    /// The compare-and-swap is the commit point. Working-tree and real-index
    /// reconciliation happens afterwards and only for the published paths.
    pub fn publish(&self, prepared: PreparedPublication) -> Result<CommitResult, GitError> {
        let publication_id = prepared.publication_id.clone();
        let expected_old = prepared.expected_old.clone();
        match self.publish_inner(prepared) {
            Ok(result) => Ok(result),
            Err(error) => {
                // These errors already carry their own journal treatment (or mean
                // the journal itself is unavailable).
                if matches!(
                    error,
                    GitError::AlreadyPublished { .. }
                        | GitError::Journal(_)
                        | GitError::IndexOutOfSync { .. }
                ) {
                    return Err(error);
                }
                self.record_refusal(
                    &publication_id,
                    Some(&expected_old),
                    &refusal_reason(&error),
                )?;
                Err(error)
            }
        }
    }

    /// `prepare` + `publish` for callers that do not need the two phases split.
    pub fn publish_request(&self, request: &PublishRequest) -> Result<CommitResult, GitError> {
        let prepared = self.prepare(request)?;
        self.publish(prepared)
    }

    /// Commit exactly `paths` (repository-relative) with an attributing message.
    ///
    /// Disk-content flavour of the transactional API: content is read from the
    /// working tree, so only deletions and creations are derived. Unrelated work is
    /// never swept into the commit and only the given paths are reconciled in the
    /// real index. Generated paths are refused here as elsewhere.
    pub fn commit_paths(
        &self,
        paths: &[String],
        subject: &str,
        trailers: &[(String, String)],
    ) -> Result<CommitResult, GitError> {
        if paths.is_empty() {
            return Err(GitError::NothingToCommit);
        }
        let mut changes: Vec<PathUpdate> = Vec::with_capacity(paths.len());
        for raw in paths {
            let path = path_policy::validate_relative_path(raw).map_err(|reason| {
                GitError::UnsafePath {
                    path: raw.clone(),
                    reason,
                }
            })?;
            let absolute = self.absolute_path(&path);
            match std::fs::symlink_metadata(&absolute) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    return Err(GitError::InvalidRequest(format!(
                        "{path}: refusing to publish a symbolic link"
                    )))
                }
                Ok(meta) if meta.is_file() => {
                    let bytes = std::fs::read(&absolute)
                        .map_err(|error| GitError::Io(format!("{path}: {error}")))?;
                    let content = String::from_utf8(bytes)
                        .map_err(|_| GitError::NotUtf8 { path: path.clone() })?;
                    changes.push(PathUpdate::write(path.clone(), content));
                }
                Ok(_) => {
                    return Err(GitError::InvalidRequest(format!(
                        "{path}: not a regular file"
                    )))
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    changes.push(PathUpdate::delete(path.clone()));
                }
                Err(error) => return Err(GitError::Io(format!("{path}: {error}"))),
            }
        }
        let mut request = PublishRequest::new(subject).with_changes(changes);
        request.trailers = trailers.to_vec();
        self.publish_request(&request)
    }

    /// The atomic publication primitive: `git update-ref <reference> <new> <old>`.
    ///
    /// Git refuses the update when the reference was not at `old`, and that refusal
    /// leaves the reference untouched. This is exposed so a caller can test the
    /// compare-and-swap contract directly.
    pub fn compare_and_swap_ref(
        &self,
        reference: &str,
        new_commit: &str,
        expected_old: &str,
    ) -> Result<(), GitError> {
        path_policy::validate_reference(reference).map_err(GitError::InvalidRequest)?;
        if !is_full_hex_sha(new_commit) || !is_full_hex_sha(expected_old) {
            return Err(GitError::InvalidRequest(format!(
                "compare-and-swap needs full object ids, got new={new_commit:?} old={expected_old:?}"
            )));
        }
        let output = self.run_raw(
            &["update-ref", reference, new_commit, expected_old],
            None,
            NO_ENV,
        )?;
        if output.status.success() {
            return Ok(());
        }
        let text = combined_output(&output);
        if text.contains("cannot lock ref")
            || (text.contains(" is at ") && text.contains(" but expected "))
        {
            let found = self
                .rev_parse(reference)
                .unwrap_or_else(|_| "<unreadable>".to_owned());
            return Err(GitError::ConcurrentWriter {
                expected: expected_old.to_owned(),
                found,
            });
        }
        Err(GitError::Git {
            command: format!("git update-ref {reference} {new_commit} {expected_old}"),
            output: text,
        })
    }

    /// Resolve every `INTENT` journal line that has no outcome yet, appending the
    /// observed status. Idempotent; never publishes anything and never guesses.
    pub fn recover(&self) -> Result<RecoveryReport, GitError> {
        let Some(journal) = &self.journal else {
            return Ok(RecoveryReport::default());
        };
        let records = journal.records().map_err(journal_error)?;
        // Walk the journal in order: an INTENT is pending until an OUTCOME for the
        // same id follows it (a refused attempt may be retried with the same id).
        let mut order: Vec<String> = Vec::new();
        let mut pending: BTreeMap<String, (String, String, String)> = BTreeMap::new();
        for record in &records {
            match record {
                PublicationRecord::Intent {
                    publication_id,
                    reference,
                    expected_old,
                    new_commit,
                    ..
                } => {
                    if !pending.contains_key(publication_id) {
                        order.push(publication_id.clone());
                    }
                    pending.insert(
                        publication_id.clone(),
                        (reference.clone(), expected_old.clone(), new_commit.clone()),
                    );
                }
                PublicationRecord::Outcome { publication_id, .. } => {
                    pending.remove(publication_id);
                }
            }
        }
        let mut report = RecoveryReport::default();
        let live = self.live().clone();
        for publication_id in order {
            // Intents written by this still-live writer are not crash residue.
            if live.contains(&publication_id) {
                continue;
            }
            let Some((reference, expected_old, new_commit)) = pending.remove(&publication_id)
            else {
                continue;
            };
            let current = self.rev_parse(&reference).ok();
            let (status, reason) = match current {
                Some(current) if current == new_commit => {
                    (PublicationStatus::RecoveredPublished, None)
                }
                Some(current) if current == expected_old => (
                    PublicationStatus::RecoveredUnpublished,
                    Some("interrupted before publication: reference still at the expected parent"),
                ),
                Some(current) => {
                    if self.is_ancestor(&new_commit, &current)? {
                        (
                            PublicationStatus::RecoveredPublished,
                            Some("candidate commit is an ancestor of the reference"),
                        )
                    } else if self.is_ancestor(&expected_old, &current)? {
                        (
                            PublicationStatus::RecoveredUnknown,
                            Some(
                                "reference advanced from the expected parent without the candidate commit; publication not observed",
                            ),
                        )
                    } else {
                        (
                            PublicationStatus::RecoveredUnknown,
                            Some("reference diverged from the prepared base; publication not observed"),
                        )
                    }
                }
                None => (
                    PublicationStatus::RecoveredUnknown,
                    Some("reference is unreadable"),
                ),
            };
            journal
                .append_outcome(
                    &publication_id,
                    status,
                    status.is_published().then_some(new_commit.as_str()),
                    Some(&expected_old),
                    reason,
                    now_unix(),
                )
                .map_err(journal_error)?;
            match status {
                PublicationStatus::RecoveredPublished => report.published.push(publication_id),
                PublicationStatus::RecoveredUnpublished => report.unpublished.push(publication_id),
                _ => report.unknown.push(RecoveryUnknown {
                    publication_id,
                    reason: reason.unwrap_or("unknown").to_owned(),
                }),
            }
        }
        Ok(report)
    }

    /// Journal status for one publication id (`None` when no journal is attached).
    pub fn publication_status(
        &self,
        publication_id: &str,
    ) -> Result<Option<PublicationStatus>, GitError> {
        let Some(journal) = &self.journal else {
            return Ok(None);
        };
        journal.status_of(publication_id).map_err(journal_error)
    }

    // -----------------------------------------------------------------------
    // prepare
    // -----------------------------------------------------------------------

    fn prepare_inner(
        &self,
        request: &PublishRequest,
        publication_id: &str,
    ) -> Result<PreparedPublication, GitError> {
        let changes = self.normalize_changes(request)?;
        let expected_old = self.head()?;
        if let Some(expected) = &request.expected_head {
            if expected != &expected_old {
                return Err(GitError::ConcurrentWriter {
                    expected: expected.clone(),
                    found: expected_old,
                });
            }
        }
        let reference = self.current_reference()?;
        self.ensure_not_published(publication_id)?;

        let temp = TempArea::create()?;
        let index_env = temp.index_env();
        // Seed the isolated index from HEAD: the candidate tree is HEAD plus
        // exactly the proposed entries, never the real (possibly dirty) index.
        self.git_text(&["read-tree", expected_old.as_str()], None, &index_env)?;

        let mut resolved: Vec<ResolvedChange> = Vec::with_capacity(changes.len());
        for change in changes {
            let expected_base = match change.expected_old.as_deref() {
                Some(hash) => ExpectedOld::parse(hash)?,
                None => self.expected_old_from_head(&expected_old, &change.path)?,
            };
            let action = match change.content {
                Some(content) => {
                    let target_sha256 = sha256_hex(content.as_bytes());
                    let blob = self
                        .git_text(
                            &["hash-object", "-w", "--stdin"],
                            Some(content.as_bytes()),
                            NO_ENV,
                        )?
                        .trim()
                        .to_owned();
                    if !is_full_hex_sha(&blob) {
                        return Err(GitError::Git {
                            command: "git hash-object -w --stdin".to_owned(),
                            output: format!("unexpected object id {blob:?}"),
                        });
                    }
                    let mode = self
                        .mode_in_index(&change.path, &index_env)?
                        .unwrap_or_else(|| "100644".to_owned());
                    self.add_index_entry(&change.path, &mode, &blob, &index_env)?;
                    ChangeAction::Write {
                        content,
                        sha256: target_sha256,
                        blob,
                        mode,
                    }
                }
                None => {
                    self.remove_index_entry(&change.path, &index_env)?;
                    ChangeAction::Delete
                }
            };
            let resolved_change = ResolvedChange {
                path: change.path,
                action,
                expected_old: expected_base,
            };
            // Fail fast: a working tree that already diverged is not silently
            // overwritten later. publish() re-checks immediately before the swap.
            self.classify_disk(&resolved_change)?;
            resolved.push(resolved_change);
        }

        let tree = self
            .git_text(&["write-tree"], None, &index_env)?
            .trim()
            .to_owned();
        let head_tree = self.rev_parse(&format!("{expected_old}^{{tree}}"))?;
        if tree == head_tree {
            return Err(GitError::NothingToCommit);
        }

        // The message file exists on disk because commit-msg hooks receive a path
        // and may edit it; the final content is read back before commit-tree.
        let message_path = temp.path("COMMIT_MSG");
        let message = build_message(&request.subject, &request.trailers, publication_id);
        std::fs::write(&message_path, message.as_bytes())
            .map_err(|_| GitError::Io(format!("cannot write {}", message_path.display())))?;
        self.run_commit_hooks(&temp, &message_path, &index_env)?;
        let final_message = std::fs::read_to_string(&message_path)
            .map_err(|error| GitError::Io(format!("cannot read back commit message: {error}")))?;
        if final_message.trim().is_empty() {
            return Err(GitError::InvalidRequest(
                "the commit message is empty after the commit-msg hook".to_owned(),
            ));
        }

        // Candidate commit: object database only, no reference move.
        let new_commit = self
            .git_text(
                &["commit-tree", &tree, "-p", &expected_old, "-F", "-"],
                Some(final_message.as_bytes()),
                NO_ENV,
            )?
            .trim()
            .to_owned();
        if !is_full_hex_sha(&new_commit) {
            return Err(GitError::Git {
                command: "git commit-tree".to_owned(),
                output: format!("unexpected commit id {new_commit:?}"),
            });
        }
        let paths: Vec<String> = resolved.iter().map(|change| change.path.clone()).collect();
        if let Some(journal) = &self.journal {
            journal
                .append_intent(
                    publication_id,
                    &reference,
                    &expected_old,
                    &new_commit,
                    &tree,
                    &paths,
                    &request.trailers,
                    now_unix(),
                )
                .map_err(journal_error)?;
            // Registered only once durable: a crash before this line leaves nothing
            // to recover, a crash after it is resolvable from the repository.
            self.live().insert(publication_id.to_owned());
        }
        Ok(PreparedPublication {
            repo: self.repo.clone(),
            publication_id: publication_id.to_owned(),
            reference,
            expected_old,
            new_commit,
            tree,
            changes: resolved,
            _temp: temp,
        })
    }

    fn normalize_changes(
        &self,
        request: &PublishRequest,
    ) -> Result<Vec<NormalizedChange>, GitError> {
        if request.subject.trim().is_empty() {
            return Err(GitError::InvalidRequest(
                "publication has an empty commit subject".to_owned(),
            ));
        }
        if request.changes.is_empty() {
            return Err(GitError::InvalidRequest(
                "publication has no paths".to_owned(),
            ));
        }
        for (key, value) in &request.trailers {
            if key.trim().is_empty()
                || key.contains(':')
                || key.chars().any(char::is_whitespace)
                || key.contains('\0')
            {
                return Err(GitError::InvalidRequest(format!(
                    "invalid trailer key {key:?}"
                )));
            }
            if value.contains('\n') || value.contains('\r') || value.contains('\0') {
                return Err(GitError::InvalidRequest(format!(
                    "trailer {key:?} contains a line break; attributed values must stay on one line"
                )));
            }
        }
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut changes = Vec::with_capacity(request.changes.len());
        for change in &request.changes {
            let path = path_policy::validate_relative_path(&change.path).map_err(|reason| {
                GitError::UnsafePath {
                    path: change.path.clone(),
                    reason,
                }
            })?;
            if !seen.insert(path.clone()) {
                return Err(GitError::InvalidRequest(format!(
                    "path {path:?} appears twice in one publication"
                )));
            }
            if path_policy::is_generated_path(&path) && !request.allow_generated {
                return Err(GitError::ProtectedPath {
                    path,
                    reason: "generated or lockfile content requires an explicit opt-in decision"
                        .to_owned(),
                });
            }
            if let Some(hash) = &change.expected_old_sha256 {
                if !hash.is_empty() && !is_sha256_hex(hash) {
                    return Err(GitError::InvalidRequest(format!(
                        "expected_old_sha256 for {path:?} is not a sha256 hex digest"
                    )));
                }
            }
            changes.push(NormalizedChange {
                path,
                content: change.content.clone(),
                expected_old: change.expected_old_sha256.as_ref().map(|hash| {
                    if hash.is_empty() {
                        String::new()
                    } else {
                        hash.to_ascii_lowercase()
                    }
                }),
            });
        }
        Ok(changes)
    }

    // -----------------------------------------------------------------------
    // publish
    // -----------------------------------------------------------------------

    fn publish_inner(&self, prepared: PreparedPublication) -> Result<CommitResult, GitError> {
        let PreparedPublication {
            repo,
            publication_id,
            reference,
            expected_old,
            new_commit,
            tree,
            changes,
            _temp,
        } = prepared;
        if repo != self.repo {
            return Err(GitError::InvalidRequest(format!(
                "prepared publication belongs to {}, not {}",
                repo.display(),
                self.repo.display()
            )));
        }
        self.ensure_not_published(&publication_id)?;
        // From here on the intent is no longer a live preparation: publication or
        // an explicit refusal outcome must follow.
        self.live().remove(&publication_id);

        // Pre-publication validation, immediately before the swap.
        let head_now = self.head()?;
        if head_now != expected_old {
            return Err(GitError::ConcurrentWriter {
                expected: expected_old,
                found: head_now,
            });
        }
        let mut observed: Vec<DiskState> = Vec::with_capacity(changes.len());
        for change in &changes {
            observed.push(self.classify_disk(change)?);
        }

        // The commit point: one atomic compare-and-swap on the reference.
        self.compare_and_swap_ref(&reference, &new_commit, &expected_old)?;

        // Published. Reconcile the working tree (published paths only, never a
        // divergent file) and then the real index (published paths only).
        let mut warnings: Vec<String> = Vec::new();
        for (change, state) in changes.iter().zip(observed.iter()) {
            if let Err(warning) = self.apply_disk(change, state) {
                warnings.push(warning);
            }
        }
        let index_result = self.reconcile_index(&changes);

        // Journal the publication before reporting any post-publication residue, so
        // a restart can never publish the same id twice.
        if let Some(journal) = &self.journal {
            journal
                .append_outcome(
                    &publication_id,
                    PublicationStatus::Published,
                    Some(&new_commit),
                    Some(&expected_old),
                    warnings.first().map(String::as_str),
                    now_unix(),
                )
                .map_err(journal_error)?;
        }
        if let Err(error) = index_result {
            return Err(GitError::IndexOutOfSync {
                commit: new_commit,
                detail: error.to_string(),
            });
        }
        if let Err(warning) = self.run_post_commit_hook() {
            warnings.push(warning);
        }
        Ok(CommitResult {
            sha: new_commit,
            parent: expected_old,
            paths: changes.iter().map(|change| change.path.clone()).collect(),
            tree,
            publication_id,
            already_published: false,
            warnings,
        })
    }

    /// Validate one resolved change against the working tree, fail-closed.
    fn classify_disk(&self, change: &ResolvedChange) -> Result<DiskState, GitError> {
        let absolute = self.absolute_path(&change.path);
        let current: Option<Vec<u8>> = match std::fs::symlink_metadata(&absolute) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(GitError::InvalidRequest(format!(
                    "{}: refusing to touch a symbolic link",
                    change.path
                )))
            }
            Ok(meta) if meta.is_file() => Some(
                std::fs::read(&absolute)
                    .map_err(|error| GitError::Io(format!("{}: {error}", change.path)))?,
            ),
            Ok(_) => {
                return Err(GitError::InvalidRequest(format!(
                    "{}: path exists but is not a regular file",
                    change.path
                )))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(GitError::Io(format!("{}: {error}", change.path))),
        };
        match &change.action {
            ChangeAction::Write { sha256, .. } => match &current {
                Some(bytes) if &sha256_hex(bytes) == sha256 => Ok(DiskState::UpToDate),
                _ => {
                    let found = current.as_ref().map(|bytes| sha256_hex(bytes));
                    if change.expected_old.matches(found.as_deref()) {
                        Ok(DiskState::NeedsWrite {
                            current_sha256: found,
                        })
                    } else {
                        Err(GitError::StaleWorkingTree {
                            path: change.path.clone(),
                            expected: change.expected_old.describe(),
                            found: found.unwrap_or_else(|| "<absent>".to_owned()),
                        })
                    }
                }
            },
            ChangeAction::Delete => match &current {
                None => Ok(DiskState::UpToDate),
                Some(bytes) => {
                    let found = sha256_hex(bytes);
                    if change.expected_old.matches(Some(&found)) {
                        Ok(DiskState::NeedsRemove {
                            current_sha256: found,
                        })
                    } else {
                        Err(GitError::StaleWorkingTree {
                            path: change.path.clone(),
                            expected: change.expected_old.describe(),
                            found,
                        })
                    }
                }
            },
        }
    }

    /// Apply one reconciled change to the working tree.
    ///
    /// Returns `Ok(())` when nothing needed to be written, or a warning when the
    /// file changed between validation and reconciliation (that file is then left
    /// untouched: the divergence stays visible instead of being overwritten).
    fn apply_disk(&self, change: &ResolvedChange, observed: &DiskState) -> Result<(), String> {
        let current = self
            .classify_disk(change)
            .map_err(|error| format!("{}: {error}", change.path))?;
        if &current == observed || current == DiskState::UpToDate {
            return match (&change.action, &current) {
                (_, DiskState::UpToDate) => Ok(()),
                (ChangeAction::Write { content, .. }, DiskState::NeedsWrite { .. }) => {
                    let absolute = self.absolute_path(&change.path);
                    if let Some(parent) = absolute.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|error| format!("{}: {error}", change.path))?;
                    }
                    std::fs::write(&absolute, content.as_bytes())
                        .map_err(|error| format!("{}: {error}", change.path))
                }
                (ChangeAction::Delete, DiskState::NeedsRemove { .. }) => {
                    std::fs::remove_file(self.absolute_path(&change.path))
                        .map_err(|error| format!("{}: {error}", change.path))
                }
                _ => Ok(()),
            };
        }
        Err(format!(
            "{}: working tree changed between validation and publication; left untouched",
            change.path
        ))
    }

    /// Update only the published paths in the *real* index.
    fn reconcile_index(&self, changes: &[ResolvedChange]) -> Result<(), GitError> {
        for change in changes {
            match &change.action {
                ChangeAction::Write { blob, mode, .. } => {
                    if change.path.contains(',') {
                        self.git_text(
                            &[
                                "update-index",
                                "--add",
                                "--cacheinfo",
                                mode,
                                blob,
                                &change.path,
                            ],
                            None,
                            NO_ENV,
                        )?;
                    } else {
                        let spec = format!("{mode},{blob},{}", change.path);
                        self.git_text(
                            &["update-index", "--add", "--cacheinfo", &spec],
                            None,
                            NO_ENV,
                        )?;
                    }
                }
                ChangeAction::Delete => {
                    if self.index_entry_exists(&change.path, NO_ENV)? {
                        self.git_text(
                            &["update-index", "--force-remove", "--", &change.path],
                            None,
                            NO_ENV,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // hooks
    // -----------------------------------------------------------------------

    fn run_commit_hooks(
        &self,
        temp: &TempArea,
        message_path: &Path,
        index_env: &[(&str, OsString)],
    ) -> Result<(), GitError> {
        if self.hook_policy == HookPolicy::Skip {
            return Ok(());
        }
        let hooks_dir = self.hooks_directory()?;
        let message = OsString::from(message_path.as_os_str());
        let steps: [(&str, Vec<OsString>); 3] = [
            (
                "prepare-commit-msg",
                vec![message.clone(), OsString::from("message")],
            ),
            ("pre-commit", Vec::new()),
            ("commit-msg", vec![message.clone()]),
        ];
        for (name, args) in steps {
            let hook = hooks_dir.join(name);
            if !hook_is_runnable(&hook) {
                continue;
            }
            let output = self.spawn_hook(&hook, name, &args, index_env)?;
            if !output.status.success() {
                return Err(GitError::HookFailed {
                    hook: name.to_owned(),
                    status: output.status.code(),
                    output: combined_output(&output),
                });
            }
        }
        let _ = temp;
        Ok(())
    }

    fn run_post_commit_hook(&self) -> Result<(), String> {
        if self.hook_policy == HookPolicy::Skip {
            return Ok(());
        }
        let hooks_dir = self
            .hooks_directory()
            .map_err(|error| format!("post-commit hook lookup failed: {error}"))?;
        let hook = hooks_dir.join("post-commit");
        if !hook_is_runnable(&hook) {
            return Ok(());
        }
        match self.spawn_hook(&hook, "post-commit", &[], NO_ENV) {
            Ok(output) if output.status.success() => Ok(()),
            Ok(output) => Err(format!(
                "post-commit hook failed with status {:?}: {}",
                output.status.code(),
                combined_output(&output)
            )),
            Err(error) => Err(format!("post-commit hook could not run: {error}")),
        }
    }

    fn hooks_directory(&self) -> Result<PathBuf, GitError> {
        let raw = self
            .git_text(&["rev-parse", "--git-path", "hooks"], None, NO_ENV)?
            .trim()
            .to_owned();
        if raw.is_empty() {
            return Err(GitError::Git {
                command: "git rev-parse --git-path hooks".to_owned(),
                output: "empty hooks path".to_owned(),
            });
        }
        let path = PathBuf::from(raw);
        Ok(if path.is_absolute() {
            path
        } else {
            self.repo.join(path)
        })
    }

    fn spawn_hook(
        &self,
        hook: &Path,
        name: &str,
        args: &[OsString],
        envs: &[(&str, OsString)],
    ) -> Result<Output, GitError> {
        match self.run_hook_process(hook.as_os_str(), &[], args, envs) {
            Ok(output) => Ok(output),
            Err(error) if hook_shell_fallback_needed(&error) => {
                let Some(shell) = self.find_hook_shell() else {
                    return Err(GitError::HookFailed {
                        hook: name.to_owned(),
                        status: None,
                        output: format!(
                            "cannot execute hook {}: {error} (no POSIX shell found; set RAVUM_HOOK_SHELL)",
                            hook.display()
                        ),
                    });
                };
                self.run_hook_process(
                    shell.as_os_str(),
                    &[hook.as_os_str().to_owned()],
                    args,
                    envs,
                )
                .map_err(|error| {
                    GitError::Io(format!(
                        "cannot run hook {} through {}: {error}",
                        hook.display(),
                        shell.display()
                    ))
                })
            }
            Err(error) => Err(GitError::Io(format!(
                "cannot run hook {}: {error}",
                hook.display()
            ))),
        }
    }

    fn run_hook_process(
        &self,
        program: &OsStr,
        leading: &[OsString],
        args: &[OsString],
        envs: &[(&str, OsString)],
    ) -> std::io::Result<Output> {
        let mut command = Command::new(program);
        for argument in leading {
            command.arg(argument);
        }
        for argument in args {
            command.arg(argument);
        }
        command
            .current_dir(&self.repo)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_NAME", &self.author_name)
            .env("GIT_AUTHOR_EMAIL", &self.author_email)
            .env("GIT_COMMITTER_NAME", &self.author_name)
            .env("GIT_COMMITTER_EMAIL", &self.author_email);
        for key in REDIRECTING_GIT_ENV {
            command.env_remove(key);
        }
        for (key, value) in envs {
            command.env(key, value);
        }
        command.output()
    }

    /// Locate a POSIX shell for hook scripts on hosts where a shebang script is not
    /// directly executable (measured: Git for Windows runs hooks through its own
    /// `sh.exe`, which is not on `PATH`).
    fn find_hook_shell(&self) -> Option<PathBuf> {
        if let Ok(explicit) = std::env::var("RAVUM_HOOK_SHELL") {
            let explicit = explicit.trim();
            if !explicit.is_empty() {
                return Some(PathBuf::from(explicit));
            }
        }
        if command_is_runnable("sh") {
            return Some(PathBuf::from("sh"));
        }
        let output = self.git_text(&["--exec-path"], None, NO_ENV).ok()?;
        let exec_path = PathBuf::from(output.trim());
        for relative in [
            "../../../bin",
            "../../../usr/bin",
            "../../bin",
            "../../usr/bin",
        ] {
            for name in ["sh.exe", "sh"] {
                let candidate = exec_path.join(relative).join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        None
    }

    // -----------------------------------------------------------------------
    // git plumbing
    // -----------------------------------------------------------------------

    fn run_raw(
        &self,
        args: &[&str],
        stdin: Option<&[u8]>,
        envs: &[(&str, OsString)],
    ) -> Result<Output, GitError> {
        let mut command = Command::new("git");
        command.arg("-C").arg(&self.repo).args(args);
        for key in REDIRECTING_GIT_ENV {
            command.env_remove(key);
        }
        for (key, value) in envs {
            command.env(key, value);
        }
        command
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_NAME", &self.author_name)
            .env("GIT_AUTHOR_EMAIL", &self.author_email)
            .env("GIT_COMMITTER_NAME", &self.author_name)
            .env("GIT_COMMITTER_EMAIL", &self.author_email)
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|error| GitError::Io(format!("cannot run git {}: {error}", args.join(" "))))?;
        if let Some(input) = stdin {
            let mut handle = child
                .stdin
                .take()
                .ok_or_else(|| GitError::Io("git stdin handle unavailable".to_owned()))?;
            handle
                .write_all(input)
                .map_err(|error| GitError::Io(format!("cannot feed git stdin: {error}")))?;
            drop(handle);
        }
        child
            .wait_with_output()
            .map_err(|error| GitError::Io(format!("cannot collect git output: {error}")))
    }

    fn git_text(
        &self,
        args: &[&str],
        stdin: Option<&[u8]>,
        envs: &[(&str, OsString)],
    ) -> Result<String, GitError> {
        let output = self.run_raw(args, stdin, envs)?;
        if !output.status.success() {
            return Err(GitError::Git {
                command: format!("git {}", args.join(" ")),
                output: combined_output(&output),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    fn git_bytes(
        &self,
        args: &[&str],
        stdin: Option<&[u8]>,
        envs: &[(&str, OsString)],
    ) -> Result<Vec<u8>, GitError> {
        let output = self.run_raw(args, stdin, envs)?;
        if !output.status.success() {
            return Err(GitError::Git {
                command: format!("git {}", args.join(" ")),
                output: combined_output(&output),
            });
        }
        Ok(output.stdout)
    }

    fn rev_parse(&self, revision: &str) -> Result<String, GitError> {
        let output = self.run_raw(&["rev-parse", "--verify", revision], None, NO_ENV)?;
        if !output.status.success() {
            return Err(GitError::Git {
                command: format!("git rev-parse --verify {revision}"),
                output: combined_output(&output),
            });
        }
        Ok(trim_output(&output.stdout))
    }

    fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool, GitError> {
        let output = self.run_raw(
            &["merge-base", "--is-ancestor", ancestor, descendant],
            None,
            NO_ENV,
        )?;
        if output.status.success() {
            Ok(true)
        } else if output.status.code() == Some(1) {
            Ok(false)
        } else {
            Err(GitError::Git {
                command: format!("git merge-base --is-ancestor {ancestor} {descendant}"),
                output: combined_output(&output),
            })
        }
    }

    fn current_reference(&self) -> Result<String, GitError> {
        let output = self.run_raw(&["symbolic-ref", "-q", "HEAD"], None, NO_ENV)?;
        if output.status.success() {
            let reference = trim_output(&output.stdout);
            path_policy::validate_reference(&reference).map_err(GitError::InvalidRequest)?;
            return Ok(reference);
        }
        // Detached HEAD: publishing updates HEAD itself, still guarded by the old
        // value in the compare-and-swap.
        Ok("HEAD".to_owned())
    }

    fn expected_old_from_head(&self, head: &str, path: &str) -> Result<ExpectedOld, GitError> {
        match self.tree_blob(head, path)? {
            None => Ok(ExpectedOld::Absent),
            Some(blob) => {
                let bytes = self.git_bytes(&["cat-file", "blob", &blob], None, NO_ENV)?;
                Ok(ExpectedOld::Exact(sha256_hex(&bytes)))
            }
        }
    }

    /// Blob id of exactly `path` in `revision`, or `None` when it is absent.
    fn tree_blob(&self, revision: &str, path: &str) -> Result<Option<String>, GitError> {
        let bytes = self.git_bytes(&["ls-tree", "-z", revision, "--", path], None, NO_ENV)?;
        for entry in bytes.split(|byte| *byte == 0) {
            if entry.is_empty() {
                continue;
            }
            let Some(tab) = entry.iter().position(|byte| *byte == b'\t') else {
                continue;
            };
            let (header, entry_path) = entry.split_at(tab);
            if &entry_path[1..] != path.as_bytes() {
                continue;
            }
            let header = String::from_utf8_lossy(header);
            let mut fields = header.split_whitespace();
            let _mode = fields.next();
            let kind = fields.next();
            let blob = fields.next();
            if kind == Some("blob") {
                return Ok(blob.map(ToOwned::to_owned));
            }
            return Ok(None);
        }
        Ok(None)
    }

    fn index_entry_exists(&self, path: &str, envs: &[(&str, OsString)]) -> Result<bool, GitError> {
        let bytes = self.git_bytes(&["ls-files", "-z", "--", path], None, envs)?;
        Ok(!bytes.is_empty())
    }

    fn mode_in_index(
        &self,
        path: &str,
        envs: &[(&str, OsString)],
    ) -> Result<Option<String>, GitError> {
        let bytes = self.git_bytes(&["ls-files", "-s", "-z", "--", path], None, envs)?;
        for entry in bytes.split(|byte| *byte == 0) {
            if entry.is_empty() {
                continue;
            }
            let Some(tab) = entry.iter().position(|byte| *byte == b'\t') else {
                continue;
            };
            let (header, entry_path) = entry.split_at(tab);
            if &entry_path[1..] != path.as_bytes() {
                continue;
            }
            let header = String::from_utf8_lossy(header);
            let mode = header.split_whitespace().next();
            return Ok(mode.map(ToOwned::to_owned));
        }
        Ok(None)
    }

    fn add_index_entry(
        &self,
        path: &str,
        mode: &str,
        blob: &str,
        envs: &[(&str, OsString)],
    ) -> Result<(), GitError> {
        if path.contains(',') {
            // The comma form cannot represent a comma in the path; fall back to the
            // three-argument form Git still accepts for exactly this reason.
            self.git_text(
                &["update-index", "--add", "--cacheinfo", mode, blob, path],
                None,
                envs,
            )?;
        } else {
            let spec = format!("{mode},{blob},{path}");
            self.git_text(&["update-index", "--add", "--cacheinfo", &spec], None, envs)?;
        }
        Ok(())
    }

    fn remove_index_entry(&self, path: &str, envs: &[(&str, OsString)]) -> Result<(), GitError> {
        if self.index_entry_exists(path, envs)? {
            self.git_text(&["update-index", "--force-remove", "--", path], None, envs)?;
        }
        Ok(())
    }

    fn absolute_path(&self, relative: &str) -> PathBuf {
        self.repo
            .join(relative.replace('/', std::path::MAIN_SEPARATOR_STR))
    }

    // -----------------------------------------------------------------------
    // journal helpers
    // -----------------------------------------------------------------------

    /// Registrations of intents written by this writer that are still alive in this
    /// process. Lock poisoning is recovered from: the set is advisory bookkeeping,
    /// never a correctness authority.
    fn live(&self) -> std::sync::MutexGuard<'_, BTreeSet<String>> {
        match self.live_intents.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn ensure_not_published(&self, publication_id: &str) -> Result<(), GitError> {
        let Some(journal) = &self.journal else {
            return Ok(());
        };
        // A live intent of this writer is a preparation in progress, not a crash
        // residue: do not resolve it prematurely.
        if !self.live().contains(publication_id) {
            self.recover()?;
        }
        let status = journal.status_of(publication_id).map_err(journal_error)?;
        if matches!(status, Some(status) if status.is_published()) {
            let commit = journal
                .published_commit_of(publication_id)
                .map_err(journal_error)?
                .unwrap_or_default();
            return Err(GitError::AlreadyPublished {
                publication_id: publication_id.to_owned(),
                commit,
            });
        }
        Ok(())
    }

    fn record_refusal(
        &self,
        publication_id: &str,
        parent: Option<&str>,
        reason: &str,
    ) -> Result<(), GitError> {
        let Some(journal) = &self.journal else {
            return Ok(());
        };
        journal
            .append_outcome(
                publication_id,
                PublicationStatus::Refused,
                None,
                parent,
                Some(reason),
                now_unix(),
            )
            .map_err(journal_error)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// internal types
// ---------------------------------------------------------------------------

struct NormalizedChange {
    path: String,
    content: Option<String>,
    expected_old: Option<String>,
}

#[derive(Debug)]
struct ResolvedChange {
    path: String,
    action: ChangeAction,
    expected_old: ExpectedOld,
}

#[derive(Debug)]
enum ChangeAction {
    Write {
        content: String,
        sha256: String,
        blob: String,
        mode: String,
    },
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ExpectedOld {
    Absent,
    Exact(String),
}

impl ExpectedOld {
    fn parse(hash: &str) -> Result<Self, GitError> {
        if hash.is_empty() {
            return Ok(ExpectedOld::Absent);
        }
        if !is_sha256_hex(hash) {
            return Err(GitError::InvalidRequest(format!(
                "expected_old_sha256 {hash:?} is not a sha256 hex digest"
            )));
        }
        Ok(ExpectedOld::Exact(hash.to_ascii_lowercase()))
    }

    fn matches(&self, current: Option<&str>) -> bool {
        match (self, current) {
            (ExpectedOld::Absent, None) => true,
            (ExpectedOld::Exact(expected), Some(found)) => expected == found,
            _ => false,
        }
    }

    fn describe(&self) -> String {
        match self {
            ExpectedOld::Absent => "<absent>".to_owned(),
            ExpectedOld::Exact(hash) => hash.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DiskState {
    /// The disk already holds the published bytes (or the delete target is gone).
    UpToDate,
    /// The disk holds the validated base content; publication replaces it.
    NeedsWrite { current_sha256: Option<String> },
    /// The disk holds the validated base content; publication removes it.
    NeedsRemove { current_sha256: String },
}

/// Private temporary directory holding the isolated index and the message file.
#[derive(Debug)]
struct TempArea {
    directory: PathBuf,
    index: PathBuf,
}

impl TempArea {
    fn create() -> Result<Self, GitError> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir();
        for _ in 0..64 {
            let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0);
            let directory = base.join(format!(
                "ravum-git-txn-{}-{unique}-{nanos}",
                std::process::id()
            ));
            match std::fs::create_dir(&directory) {
                Ok(()) => {
                    let index = directory.join("index");
                    return Ok(Self { directory, index });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(GitError::Io(format!(
                        "cannot create isolated git index directory: {error}"
                    )))
                }
            }
        }
        Err(GitError::Io(
            "cannot create a unique isolated git index directory".to_owned(),
        ))
    }

    fn path(&self, name: &str) -> PathBuf {
        self.directory.join(name)
    }

    fn index_env(&self) -> Vec<(&'static str, OsString)> {
        vec![("GIT_INDEX_FILE", self.index.clone().into_os_string())]
    }
}

impl Drop for TempArea {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

// ---------------------------------------------------------------------------
// pure helpers
// ---------------------------------------------------------------------------

fn journal_error(error: std::io::Error) -> GitError {
    GitError::Journal(error.to_string())
}

/// Short, secret-free description of a failure for the durable journal.
fn refusal_reason(error: &GitError) -> String {
    match error {
        GitError::NothingToCommit => "candidate tree is identical to HEAD".to_owned(),
        GitError::HookFailed { hook, status, .. } => {
            format!("{hook} hook refused with status {status:?}")
        }
        other => other.to_string(),
    }
}

fn derive_publication_id(request: &PublishRequest) -> String {
    let mut buffer = String::from("ravum-publication-v1\n");
    buffer.push_str(&request.subject);
    buffer.push('\0');
    if let Some(head) = &request.expected_head {
        buffer.push_str(head);
        buffer.push('\0');
    }
    for change in &request.changes {
        buffer.push_str(&change.path);
        buffer.push('\0');
        match &change.content {
            Some(content) => buffer.push_str(&sha256_hex(content.as_bytes())),
            None => buffer.push_str("<delete>"),
        }
        buffer.push('\0');
        if let Some(expected) = &change.expected_old_sha256 {
            buffer.push_str(expected);
            buffer.push('\0');
        }
    }
    for (key, value) in &request.trailers {
        buffer.push_str(key);
        buffer.push('\0');
        buffer.push_str(value);
        buffer.push('\0');
    }
    format!("pub-{}", sha256_hex(buffer.as_bytes()))
}

fn build_message(subject: &str, trailers: &[(String, String)], publication_id: &str) -> String {
    let mut message = subject.trim_end().to_owned();
    message.push('\n');
    message.push('\n');
    for (key, value) in trailers {
        message.push_str(key);
        message.push_str(": ");
        message.push_str(value);
        message.push('\n');
    }
    message.push_str("Ravum-Publication-Id: ");
    message.push_str(publication_id);
    message.push('\n');
    message
}

fn trim_output(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_owned()
}

fn combined_output(output: &Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stderr).to_string();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    truncate(text, MAX_ERROR_BYTES)
}

fn truncate(mut text: String, max: usize) -> String {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let dropped = text.len() - end;
    text.truncate(end);
    text.push_str(&format!("… [truncated {dropped} bytes]"));
    text
}

fn is_full_hex_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn hook_is_runnable(hook: &Path) -> bool {
    match std::fs::metadata(hook) {
        Ok(metadata) if metadata.is_file() => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                return metadata.permissions().mode() & 0o111 != 0;
            }
            #[cfg(not(unix))]
            {
                // Measured: Git for Windows runs a hook file without an executable
                // bit (it invokes its own shell); match that behaviour.
                true
            }
        }
        _ => false,
    }
}

/// True when a failed hook spawn is "this host cannot execute a shebang script
/// directly", which is what a POSIX-shell fallback repairs.
fn hook_shell_fallback_needed(error: &std::io::Error) -> bool {
    match error.kind() {
        std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied => true,
        _ => matches!(error.raw_os_error(), Some(193) | Some(216)),
    }
}

fn command_is_runnable(program: &str) -> bool {
    Command::new(program)
        .arg("-c")
        .arg("exit 0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> PublishRequest {
        PublishRequest::new("feat: test")
            .with_change(PathUpdate::write("a.rs", "fn a() {}\n"))
            .with_trailer("Ravum-Agent", "agent-a")
    }

    #[test]
    fn publication_ids_are_deterministic_and_content_sensitive() {
        let first = derive_publication_id(&request());
        let same = derive_publication_id(&request());
        assert_eq!(first, same);
        assert!(first.starts_with("pub-"));
        assert_eq!(first.len(), 4 + 64);

        let mut changed = request();
        changed.changes[0].content = Some("fn a() { 2 }\n".to_owned());
        assert_ne!(first, derive_publication_id(&changed));

        let mut retitled = request();
        retitled.subject = "feat: other".to_owned();
        assert_ne!(first, derive_publication_id(&retitled));
    }

    #[test]
    fn message_keeps_subject_then_trailers() {
        let message = build_message(
            "feat(collab): integrate",
            &[
                ("Ravum-Proposal".to_owned(), "agent-a/t1".to_owned()),
                ("Ravum-Agent".to_owned(), "agent-a".to_owned()),
            ],
            "pub-123",
        );
        assert!(message.starts_with("feat(collab): integrate\n\nRavum-Proposal: agent-a/t1\n"));
        assert!(message.contains("Ravum-Agent: agent-a\n"));
        assert!(message.ends_with("Ravum-Publication-Id: pub-123\n"));
    }

    #[test]
    fn object_and_content_ids_are_checked_by_length_and_alphabet() {
        assert!(is_full_hex_sha(&"a".repeat(40)));
        assert!(is_full_hex_sha(&"a".repeat(64)));
        assert!(!is_full_hex_sha(&"a".repeat(39)));
        assert!(!is_full_hex_sha("zzzz"));
        assert!(is_sha256_hex(&"0".repeat(64)));
        assert!(!is_sha256_hex(&"0".repeat(40)));
        assert!(!is_sha256_hex("not-a-hash"));
    }

    #[test]
    fn stale_working_tree_states_are_compared_not_guessed() {
        let absent = ExpectedOld::Absent;
        assert!(absent.matches(None));
        assert!(!absent.matches(Some("abc")));
        let exact = ExpectedOld::Exact("abc".to_owned());
        assert!(exact.matches(Some("abc")));
        assert!(!exact.matches(Some("abd")));
        assert!(!exact.matches(None));
        assert_eq!(ExpectedOld::parse("").expect("absent"), ExpectedOld::Absent);
        assert!(ExpectedOld::parse("nope").is_err());
    }

    #[test]
    fn long_output_is_truncated_on_a_char_boundary() {
        let text = "é".repeat(10);
        let truncated = truncate(text, 5);
        assert!(truncated.starts_with("éé"));
        assert!(truncated.contains("truncated"));
    }
}
