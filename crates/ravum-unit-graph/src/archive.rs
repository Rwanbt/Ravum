//! Content-addressed verdict archive and extern artifacts.
//!
//! The archive is a plain JSON file keyed by [`crate::identity::verdict_key`];
//! artifacts are stored by their own SHA-256, so a stale or tampered artifact can
//! never be passed to `rustc` as if it were the verified interface.

use crate::identity::sha256_hex;
use crate::UnitStatus;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One direct extern passed to `rustc --extern name=<artifact>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternArtifact {
    /// Name under which the dependent imports the crate.
    pub name: String,
    /// Crate name of the dependency target (underscored).
    pub crate_name: String,
    /// SHA-256 of the `.rmeta` bytes (interface identity).
    pub interface_id: String,
}

/// A verdict cached for one exact key. The verdict is not a proof: it is only
/// reused when its key and its artifact bytes match exactly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerdictEntry {
    pub key: String,
    pub unit: String,
    pub status: UnitStatus,
    pub content_id: String,
    pub context_id: String,
    pub externs_digest: String,
    pub toolchain_id: String,
    pub interface_id: Option<String>,
    pub rmeta_sha256: Option<String>,
    pub diagnostics_digest: String,
    pub at_unix: u64,
    pub hits: u64,
}

/// On-disk verdict archive.
#[derive(Debug, Clone)]
pub struct VerdictArchive {
    path: PathBuf,
    entries: BTreeMap<String, VerdictEntry>,
}

impl VerdictArchive {
    /// Opens (or creates in memory) the archive at `path`. A corrupt archive is
    /// treated as empty: a corrupt cache must never be trusted.
    pub fn open(path: PathBuf) -> Self {
        let entries = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<BTreeMap<String, VerdictEntry>>(&bytes).ok())
            .unwrap_or_default();
        Self { path, entries }
    }

    pub fn count(&self) -> usize {
        self.entries.len()
    }

    pub fn get(&self, key: &str) -> Option<&VerdictEntry> {
        self.entries.get(key)
    }

    pub fn insert(&mut self, entry: VerdictEntry) {
        self.entries.insert(entry.key.clone(), entry);
    }

    pub fn note_hit(&mut self, key: &str, at_unix: u64) -> Option<&VerdictEntry> {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.hits = entry.hits.saturating_add(1);
            entry.at_unix = at_unix;
            return Some(entry);
        }
        None
    }

    /// Atomic save (temporary file then rename) so a crash never leaves a
    /// truncated archive behind.
    pub fn save(&self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(&self.entries)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        let temporary = self.path.with_extension("json.tmp");
        std::fs::write(&temporary, bytes)?;
        std::fs::rename(&temporary, &self.path)?;
        Ok(())
    }
}

/// Content-addressed artifact store (`artifacts/<sha>.rmeta`).
#[derive(Debug, Clone)]
pub struct ArtifactStore {
    root: PathBuf,
}

impl ArtifactStore {
    pub fn new(root: PathBuf) -> std::io::Result<Self> {
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path_of(&self, sha256: &str) -> PathBuf {
        self.root.join(format!("{sha256}.rmeta"))
    }

    /// Stores `bytes` under their own hash and returns that hash.
    pub fn store(&self, bytes: &[u8]) -> std::io::Result<String> {
        let sha = sha256_hex(bytes);
        let path = self.path_of(&sha);
        if path.is_file() {
            return Ok(sha);
        }
        std::fs::create_dir_all(&self.root)?;
        let temporary = self.root.join(format!("{sha}.tmp"));
        std::fs::write(&temporary, bytes)?;
        std::fs::rename(&temporary, &path)?;
        Ok(sha)
    }

    /// Returns the verified bytes only when the on-disk bytes still hash to
    /// `sha256`; a tampered/stale artifact yields `None` (never reused).
    pub fn load_verified(&self, sha256: &str) -> Option<Vec<u8>> {
        let bytes = std::fs::read(self.path_of(sha256)).ok()?;
        if sha256_hex(&bytes) == sha256 {
            Some(bytes)
        } else {
            None
        }
    }
}
