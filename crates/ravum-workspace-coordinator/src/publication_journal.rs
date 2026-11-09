//! Durable, append-only journal of Git publications.
//!
//! The coordinator already journals *decisions* (`Coordinator::with_journal`). This
//! module journals the second, independent event: whether a decision actually
//! reached the Git reference. It reuses the same append-only JSONL discipline —
//! one record per line, written and flushed before the next step — so a crash
//! loses at most the record in progress.
//!
//! Two record kinds:
//!
//! * `INTENT` — written *after* the candidate commit object exists but *before*
//!   the reference compare-and-swap. It names the expected old commit, the
//!   candidate commit, the target reference and the paths. If the process dies
//!   between the intent and its outcome, `GitWriter::recover` decides from the
//!   repository what actually happened: the ref is at the candidate (published),
//!   at the expected old commit (not published), or something else (unknown, never
//!   guessed).
//! * `OUTCOME` — `PUBLISHED`, `REFUSED` or a `RECOVERED_*` resolution, with the
//!   commit when one exists and a short reason. A repeated publication id whose
//!   outcome is `PUBLISHED` (or `RECOVERED_PUBLISHED`) is refused, which is what
//!   makes a coordinator restart non-duplicating.
//!
//! Secrets: records carry publication identifiers, commit/tree hashes, repository
//! paths, commit trailers and short reasons. **No file content and no environment
//! value is ever written.** Callers must keep secrets out of subject/trailers too:
//! trailers are public commit metadata by definition.

use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Schema of a journal line. Bumped only with a compatible reader.
pub const PUBLICATION_JOURNAL_SCHEMA_VERSION: u32 = 1;

/// Lifecycle of one publication attempt as observed by the journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PublicationStatus {
    /// The reference compare-and-swap succeeded.
    Published,
    /// The attempt stopped before moving the reference; nothing was published.
    Refused,
    /// A crash left an `INTENT` behind and the reference was found at the
    /// candidate commit: the publication happened before the crash.
    RecoveredPublished,
    /// A crash left an `INTENT` behind and the reference was still at the expected
    /// old commit: the publication never happened.
    RecoveredUnpublished,
    /// A crash left an `INTENT` behind and the reference cannot be explained from
    /// the recorded commits. The journal keeps the facts; recovery does not guess.
    RecoveredUnknown,
}

impl PublicationStatus {
    /// True when this status proves the candidate commit is the published state.
    pub fn is_published(self) -> bool {
        matches!(
            self,
            PublicationStatus::Published | PublicationStatus::RecoveredPublished
        )
    }

    pub fn as_str(self) -> &'static str {
        match self {
            PublicationStatus::Published => "PUBLISHED",
            PublicationStatus::Refused => "REFUSED",
            PublicationStatus::RecoveredPublished => "RECOVERED_PUBLISHED",
            PublicationStatus::RecoveredUnpublished => "RECOVERED_UNPUBLISHED",
            PublicationStatus::RecoveredUnknown => "RECOVERED_UNKNOWN",
        }
    }
}

/// One line of the publication journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PublicationRecord {
    Intent {
        schema_version: u32,
        seq: u64,
        publication_id: String,
        /// Reference the publication will move (`HEAD` or `refs/heads/<branch>`).
        reference: String,
        /// Commit the reference must still be at for the swap to be allowed.
        expected_old: String,
        /// Candidate commit created by `git commit-tree`; unpublished at this point.
        new_commit: String,
        tree: String,
        paths: Vec<String>,
        /// Attribution trailers that will appear in the commit message.
        trailers: Vec<(String, String)>,
        started_at_unix: u64,
    },
    Outcome {
        schema_version: u32,
        seq: u64,
        publication_id: String,
        status: PublicationStatus,
        commit: Option<String>,
        parent: Option<String>,
        reason: Option<String>,
        finished_at_unix: u64,
    },
}

impl PublicationRecord {
    pub fn seq(&self) -> u64 {
        match self {
            PublicationRecord::Intent { seq, .. } | PublicationRecord::Outcome { seq, .. } => *seq,
        }
    }

    pub fn publication_id(&self) -> &str {
        match self {
            PublicationRecord::Intent { publication_id, .. }
            | PublicationRecord::Outcome { publication_id, .. } => publication_id,
        }
    }

    pub fn schema_version(&self) -> u32 {
        match self {
            PublicationRecord::Intent { schema_version, .. }
            | PublicationRecord::Outcome { schema_version, .. } => *schema_version,
        }
    }
}

/// Append-only publication journal.
///
/// Cloning is cheap and shares the sequence counter, so a cloned writer keeps a
/// single line order. Writes are serialized by the OS append mode; the journal is
/// intended for one writer per file (the coordinator is that writer).
#[derive(Debug, Clone)]
pub struct PublicationJournal {
    path: PathBuf,
    seq: Arc<AtomicU64>,
}

impl PublicationJournal {
    /// Open (creating if needed) a journal and restore its sequence number.
    ///
    /// Every existing line must parse: a corrupt journal fails closed instead of
    /// silently starting a second history.
    pub fn open(path: impl Into<PathBuf>) -> std::io::Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        OpenOptions::new().create(true).append(true).open(&path)?;
        let mut records = 0u64;
        for record in read_records(&path)? {
            records = records.max(record.seq());
        }
        Ok(Self {
            path,
            seq: Arc::new(AtomicU64::new(records)),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Last sequence number handed out (0 for an empty journal).
    pub fn last_seq(&self) -> u64 {
        self.seq.load(Ordering::SeqCst)
    }

    /// Append one intent and return the durable record.
    #[allow(clippy::too_many_arguments)]
    pub fn append_intent(
        &self,
        publication_id: &str,
        reference: &str,
        expected_old: &str,
        new_commit: &str,
        tree: &str,
        paths: &[String],
        trailers: &[(String, String)],
        started_at_unix: u64,
    ) -> std::io::Result<PublicationRecord> {
        let record = PublicationRecord::Intent {
            schema_version: PUBLICATION_JOURNAL_SCHEMA_VERSION,
            seq: self.next_seq(),
            publication_id: publication_id.to_owned(),
            reference: reference.to_owned(),
            expected_old: expected_old.to_owned(),
            new_commit: new_commit.to_owned(),
            tree: tree.to_owned(),
            paths: paths.to_vec(),
            trailers: trailers.to_vec(),
            started_at_unix,
        };
        self.append(&record)?;
        Ok(record)
    }

    /// Append one outcome and return the durable record.
    pub fn append_outcome(
        &self,
        publication_id: &str,
        status: PublicationStatus,
        commit: Option<&str>,
        parent: Option<&str>,
        reason: Option<&str>,
        finished_at_unix: u64,
    ) -> std::io::Result<PublicationRecord> {
        let record = PublicationRecord::Outcome {
            schema_version: PUBLICATION_JOURNAL_SCHEMA_VERSION,
            seq: self.next_seq(),
            publication_id: publication_id.to_owned(),
            status,
            commit: commit.map(ToOwned::to_owned),
            parent: parent.map(ToOwned::to_owned),
            reason: reason.map(ToOwned::to_owned),
            finished_at_unix,
        };
        self.append(&record)?;
        Ok(record)
    }

    /// Every record, in file order. Fails closed on a malformed line.
    pub fn records(&self) -> std::io::Result<Vec<PublicationRecord>> {
        read_records(&self.path)
    }

    /// Last recorded outcome for one publication id, if any.
    pub fn outcome_of(&self, publication_id: &str) -> std::io::Result<Option<PublicationRecord>> {
        Ok(self.records()?.into_iter().rfind(|record| {
            matches!(record, PublicationRecord::Outcome { .. })
                && record.publication_id() == publication_id
        }))
    }

    /// Last recorded status for one publication id, if any.
    pub fn status_of(&self, publication_id: &str) -> std::io::Result<Option<PublicationStatus>> {
        Ok(match self.outcome_of(publication_id)? {
            Some(PublicationRecord::Outcome { status, .. }) => Some(status),
            _ => None,
        })
    }

    /// Commit recorded as published for `publication_id`, when the journal has one.
    pub fn published_commit_of(&self, publication_id: &str) -> std::io::Result<Option<String>> {
        Ok(match self.outcome_of(publication_id)? {
            Some(PublicationRecord::Outcome {
                status,
                commit: Some(commit),
                ..
            }) if status.is_published() => Some(commit),
            _ => None,
        })
    }

    fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::SeqCst) + 1
    }

    fn append(&self, record: &PublicationRecord) -> std::io::Result<()> {
        let mut line = serde_json::to_string(record)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        line.push('\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        file.write_all(line.as_bytes())?;
        // Durability: the next step of the transaction must not start before the
        // record is on stable storage. Errors propagate as a fatal journal failure.
        file.sync_all()
    }
}

fn read_records(path: &Path) -> std::io::Result<Vec<PublicationRecord>> {
    let file = OpenOptions::new().read(true).open(path)?;
    let mut records = Vec::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let record: PublicationRecord = serde_json::from_str(&line).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("publication journal line {} is invalid: {e}", index + 1),
            )
        })?;
        records.push(record);
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/ravum-tests")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn intents_and_outcomes_round_trip_in_order() {
        let path = dir("publication-journal").join("journal.jsonl");
        let journal = PublicationJournal::open(&path).expect("open");
        journal
            .append_intent(
                "pub-1",
                "refs/heads/main",
                "old",
                "new",
                "tree",
                &["a.rs".to_owned()],
                &[("Ravum-Agent".to_owned(), "agent-a".to_owned())],
                1,
            )
            .expect("intent");
        journal
            .append_outcome(
                "pub-1",
                PublicationStatus::Published,
                Some("new"),
                Some("old"),
                None,
                2,
            )
            .expect("outcome");
        let records = journal.records().expect("records");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].seq(), 1);
        assert_eq!(records[1].seq(), 2);
        assert_eq!(
            records[0].schema_version(),
            PUBLICATION_JOURNAL_SCHEMA_VERSION
        );
        assert_eq!(
            journal.status_of("pub-1").expect("status"),
            Some(PublicationStatus::Published)
        );
        assert_eq!(
            journal.published_commit_of("pub-1").expect("commit"),
            Some("new".to_owned())
        );
        assert_eq!(journal.status_of("pub-2").expect("status"), None);
    }

    #[test]
    fn reopening_continues_the_sequence_and_survives_corruption_free() {
        let path = dir("publication-journal-reopen").join("journal.jsonl");
        {
            let journal = PublicationJournal::open(&path).expect("open");
            journal
                .append_outcome(
                    "pub-1",
                    PublicationStatus::Refused,
                    None,
                    Some("old"),
                    Some("x"),
                    1,
                )
                .expect("outcome");
        }
        let journal = PublicationJournal::open(&path).expect("reopen");
        assert_eq!(journal.last_seq(), 1);
        journal
            .append_outcome(
                "pub-2",
                PublicationStatus::Refused,
                None,
                Some("old"),
                Some("x"),
                2,
            )
            .expect("outcome");
        assert_eq!(journal.records().expect("records").len(), 2);
        assert_eq!(journal.records().expect("records")[1].seq(), 2);
    }

    #[test]
    fn a_corrupt_journal_fails_closed() {
        let path = dir("publication-journal-corrupt").join("journal.jsonl");
        std::fs::write(&path, "{not json}\n").unwrap();
        assert!(PublicationJournal::open(&path).is_err());
    }
}
