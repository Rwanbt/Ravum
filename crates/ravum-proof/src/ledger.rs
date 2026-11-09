//! Append-only evidence ledger.
//!
//! One JSON object per line, appended and flushed after every verdict, so a crash
//! cannot lose the decisions already taken. Reading is **fail-closed**: a malformed
//! line is an error, not something to skip — a corrupted ledger must not silently
//! drop evidence.

use crate::obligation::ProofContext;
use ravum_protocol::{EvidenceRecord, VerificationKey, VerificationStatus};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum LedgerError {
    Io(String),
    Malformed { line: usize, detail: String },
}

impl fmt::Display for LedgerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LedgerError::Io(d) => write!(f, "ledger io error: {d}"),
            LedgerError::Malformed { line, detail } => {
                write!(f, "corrupt ledger at line {line}: {detail}")
            }
        }
    }
}

impl std::error::Error for LedgerError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerLine {
    pub seq: u64,
    pub record: EvidenceRecord,
}

/// Result of a cache lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    /// Same key and same context triple: the verdict is authoritative.
    Fresh(EvidenceRecord),
    /// Same key, but recorded for another snapshot/build/artifact.
    Stale(EvidenceRecord),
    Missing,
}

pub struct EvidenceLedger {
    path: PathBuf,
    lines: Vec<LedgerLine>,
}

impl EvidenceLedger {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, LedgerError> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| LedgerError::Io(e.to_string()))?;
        }
        let mut lines = Vec::new();
        if path.is_file() {
            let text =
                std::fs::read_to_string(&path).map_err(|e| LedgerError::Io(e.to_string()))?;
            for (index, raw) in text.lines().enumerate() {
                let raw = raw.trim();
                if raw.is_empty() {
                    continue;
                }
                let line: LedgerLine =
                    serde_json::from_str(raw).map_err(|e| LedgerError::Malformed {
                        line: index + 1,
                        detail: e.to_string(),
                    })?;
                lines.push(line);
            }
        }
        Ok(Self { path, lines })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn records(&self) -> impl Iterator<Item = &EvidenceRecord> {
        self.lines.iter().map(|l| &l.record)
    }

    /// Append one verdict durably (write + flush). Returns its sequence number.
    pub fn append(&mut self, record: EvidenceRecord) -> Result<u64, LedgerError> {
        let seq = self.lines.len() as u64 + 1;
        let line = LedgerLine {
            seq,
            record: record.clone(),
        };
        let json = serde_json::to_string(&line).map_err(|e| LedgerError::Io(e.to_string()))?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| LedgerError::Io(e.to_string()))?;
        writeln!(file, "{json}").map_err(|e| LedgerError::Io(e.to_string()))?;
        file.flush().map_err(|e| LedgerError::Io(e.to_string()))?;
        self.lines.push(line);
        Ok(seq)
    }

    /// Look up the last verdict recorded for `key`.
    ///
    /// `Fresh` requires the record to apply to the *current* snapshot, build
    /// context and artifact, to have been produced by the **same tool and tool
    /// version**, and not to be marked `stale`. Anything else must be re-verified.
    pub fn lookup(&self, key: &VerificationKey, ctx: &ProofContext) -> Lookup {
        let Some(record) = self
            .lines
            .iter()
            .rev()
            .map(|l| &l.record)
            .find(|r| &r.verification_key == key)
        else {
            return Lookup::Missing;
        };
        let same_tool = record.tool == ctx.tool && record.tool_version == ctx.tool_version;
        let fresh = record.applies_to(
            &ctx.source_snapshot_id,
            &ctx.build_context_id,
            &ctx.artifact_id,
        ) && same_tool
            && record.status != VerificationStatus::Stale;
        if fresh {
            Lookup::Fresh(record.clone())
        } else {
            Lookup::Stale(record.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ravum_protocol::{ArtifactId, BuildContextId, SnapshotId};

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ravum-ledger-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn ctx(tag: &str) -> ProofContext {
        ProofContext::new(
            SnapshotId::new(format!("snap-{tag}")),
            BuildContextId::new("build"),
            ArtifactId::new(format!("artifact-{tag}")),
            "ravum-exhaustive",
            "0.1.0",
        )
    }

    fn record(key: &str, tag: &str) -> EvidenceRecord {
        EvidenceRecord {
            schema_version: ravum_protocol::EVIDENCE_SCHEMA_VERSION,
            verification_key: VerificationKey::new(key),
            status: VerificationStatus::Proved,
            property: "p".to_owned(),
            assumptions: vec![],
            tool: "ravum-exhaustive".to_owned(),
            tool_version: "0.1.0".to_owned(),
            source_snapshot_id: SnapshotId::new(format!("snap-{tag}")),
            build_context_id: BuildContextId::new("build"),
            artifact_id: ArtifactId::new(format!("artifact-{tag}")),
            passed: true,
            counterexample: None,
            recorded_at_unix: 0,
        }
    }

    #[test]
    fn verdicts_survive_a_restart() {
        let path = dir("restart").join("ledger.jsonl");
        {
            let mut ledger = EvidenceLedger::open(&path).unwrap();
            ledger.append(record("k1", "a")).unwrap();
        }
        let ledger = EvidenceLedger::open(&path).unwrap();
        assert_eq!(ledger.len(), 1);
        assert!(matches!(
            ledger.lookup(&VerificationKey::new("k1"), &ctx("a")),
            Lookup::Fresh(_)
        ));
    }

    #[test]
    fn another_context_is_stale_not_fresh() {
        let path = dir("stale").join("ledger.jsonl");
        let mut ledger = EvidenceLedger::open(&path).unwrap();
        ledger.append(record("k1", "a")).unwrap();
        assert!(matches!(
            ledger.lookup(&VerificationKey::new("k1"), &ctx("b")),
            Lookup::Stale(_)
        ));
        assert_eq!(
            ledger.lookup(&VerificationKey::new("other"), &ctx("a")),
            Lookup::Missing
        );
    }

    #[test]
    fn a_stale_status_is_never_fresh() {
        let path = dir("status").join("ledger.jsonl");
        let mut ledger = EvidenceLedger::open(&path).unwrap();
        let mut stale = record("k1", "a");
        stale.status = VerificationStatus::Stale;
        ledger.append(stale).unwrap();
        assert!(matches!(
            ledger.lookup(&VerificationKey::new("k1"), &ctx("a")),
            Lookup::Stale(_)
        ));
    }

    #[test]
    fn corrupt_ledger_is_refused() {
        let path = dir("corrupt").join("ledger.jsonl");
        std::fs::write(&path, "{\"seq\":1,\"record\":{}}\nnot json\n").unwrap();
        assert!(matches!(
            EvidenceLedger::open(&path),
            Err(LedgerError::Malformed { .. })
        ));
    }

    #[test]
    fn a_different_tool_version_is_stale() {
        let path = dir("tool").join("ledger.jsonl");
        let mut ledger = EvidenceLedger::open(&path).unwrap();
        ledger.append(record("k1", "a")).unwrap();
        let mut other = ctx("a");
        other.tool_version = "9.9.9".to_owned();
        assert!(matches!(
            ledger.lookup(&VerificationKey::new("k1"), &other),
            Lookup::Stale(_)
        ));
    }

    #[test]
    fn sequence_numbers_keep_increasing() {
        let path = dir("seq").join("ledger.jsonl");
        let mut ledger = EvidenceLedger::open(&path).unwrap();
        assert_eq!(ledger.append(record("k1", "a")).unwrap(), 1);
        assert_eq!(ledger.append(record("k2", "a")).unwrap(), 2);
        let reopened = EvidenceLedger::open(&path).unwrap();
        assert_eq!(reopened.len(), 2);
    }
}
