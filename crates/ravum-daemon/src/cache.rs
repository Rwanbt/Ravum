//! Content-addressed result cache: L1 in memory, L2 on disk.
//!
//! The key is the snapshot identity (source bytes + build context + toolchain)
//! plus the cache schema version. A cached verdict carries a self-digest: any
//! hand-edited or partially-written entry is rejected as corrupt and degrades to
//! a real `rustc` run, never to a wrong answer.

use crate::config::DAEMON_SCHEMA_VERSION;
use crate::diagnostics::Diagnostic;
use crate::error::{DaemonError, Result};
use crate::fsutil::atomic_write;
use crate::hash::push_field;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Where a cache hit came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CacheKind {
    /// In-process LRU hit.
    Memory,
    /// Persistent on-disk hit (works across daemon restarts).
    Disk,
    /// No usable entry: the analysis was really performed.
    Miss,
}

impl CacheKind {
    pub fn is_hit(self) -> bool {
        !matches!(self, CacheKind::Miss)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            CacheKind::Memory => "memory",
            CacheKind::Disk => "disk",
            CacheKind::Miss => "miss",
        }
    }
}

/// Machine-readable cache outcome attached to every report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheReport {
    pub hit: bool,
    pub kind: CacheKind,
    pub entries: usize,
}

impl CacheReport {
    pub fn miss(entries: usize) -> Self {
        Self {
            hit: false,
            kind: CacheKind::Miss,
            entries,
        }
    }

    pub fn hit(kind: CacheKind, entries: usize) -> Self {
        Self {
            hit: true,
            kind,
            entries,
        }
    }
}

/// One cached analysis verdict.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedVerdict {
    pub schema_version: u32,
    pub key: String,
    pub snapshot_id: String,
    pub context_id: String,
    pub toolchain_id: String,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub diagnostics: Vec<Diagnostic>,
    pub analysis_ms: u64,
    pub created_at_unix: u64,
    /// Ordered digest of the content-addressed externs used by the analysis
    /// (empty when none). Part of the cache identity, never optional.
    #[serde(default)]
    pub externs_digest: String,
    /// SHA-256 of the produced `.rmeta` artifact, so a cache HIT can still serve
    /// the exact interface to a dependent analysis.
    #[serde(default)]
    pub artifact_sha256: Option<String>,
    /// SHA-256 of every other field, canonicalized field by field.
    pub payload_digest: String,
}

impl CachedVerdict {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        key: String,
        snapshot_id: String,
        context_id: String,
        toolchain_id: String,
        success: bool,
        exit_code: Option<i32>,
        diagnostics: Vec<Diagnostic>,
        analysis_ms: u64,
    ) -> Self {
        let mut verdict = Self {
            schema_version: DAEMON_SCHEMA_VERSION,
            key,
            snapshot_id,
            context_id,
            toolchain_id,
            success,
            exit_code,
            diagnostics,
            analysis_ms,
            created_at_unix: now_unix(),
            externs_digest: String::new(),
            artifact_sha256: None,
            payload_digest: String::new(),
        };
        verdict.payload_digest = verdict.compute_digest();
        verdict
    }

    /// Adds the extern identity to a verdict and recomputes its payload digest.
    pub fn with_externs_digest(mut self, externs_digest: impl Into<String>) -> Self {
        self.externs_digest = externs_digest.into();
        self.payload_digest = self.compute_digest();
        self
    }

    /// Records the content-addressed interface artifact of a successful analysis.
    pub fn with_artifact_sha256(mut self, artifact_sha256: Option<String>) -> Self {
        self.artifact_sha256 = artifact_sha256;
        self.payload_digest = self.compute_digest();
        self
    }

    /// Recomputes the payload digest and checks every identity field.
    pub fn verify(&self, expected: &CacheLookupKey) -> std::result::Result<(), String> {
        if self.schema_version != DAEMON_SCHEMA_VERSION {
            return Err(format!(
                "cache schema {} != daemon schema {DAEMON_SCHEMA_VERSION}",
                self.schema_version
            ));
        }
        if self.key != expected.key
            || self.snapshot_id != expected.snapshot_id
            || self.context_id != expected.context_id
            || self.toolchain_id != expected.toolchain_id
            || self.externs_digest != expected.externs_digest
        {
            return Err("cache entry identity does not match the lookup key".to_owned());
        }
        let actual = self.compute_digest();
        if actual != self.payload_digest {
            return Err("cache entry payload digest mismatch".to_owned());
        }
        Ok(())
    }

    fn compute_digest(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"ravum-daemon-cache-verdict-v1");
        push_field(&mut hasher, &self.schema_version.to_le_bytes());
        push_field(&mut hasher, self.key.as_bytes());
        push_field(&mut hasher, self.snapshot_id.as_bytes());
        push_field(&mut hasher, self.context_id.as_bytes());
        push_field(&mut hasher, self.toolchain_id.as_bytes());
        push_field(&mut hasher, &[self.success as u8]);
        push_field(
            &mut hasher,
            &self.exit_code.unwrap_or(i32::MIN).to_le_bytes(),
        );
        push_field(&mut hasher, &self.analysis_ms.to_le_bytes());
        push_field(&mut hasher, &self.created_at_unix.to_le_bytes());
        push_field(&mut hasher, self.externs_digest.as_bytes());
        push_field(
            &mut hasher,
            self.artifact_sha256.as_deref().unwrap_or("").as_bytes(),
        );
        push_field(&mut hasher, &(self.diagnostics.len() as u64).to_le_bytes());
        for diagnostic in &self.diagnostics {
            push_field(&mut hasher, diagnostic.signature().as_bytes());
            push_field(
                &mut hasher,
                diagnostic.rendered.as_deref().unwrap_or("").as_bytes(),
            );
        }
        hex(&hasher.finalize())
    }
}

/// The exact identity a cache lookup expects. Passing it explicitly keeps the
/// cache generic and makes a mismatched entry impossible to accept by accident.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheLookupKey {
    pub key: String,
    pub snapshot_id: String,
    pub context_id: String,
    pub toolchain_id: String,
    /// Ordered digest of the content-addressed externs (defence in depth: even a
    /// bug in snapshot identity cannot make an extern change a HIT).
    pub externs_digest: String,
}

impl CacheLookupKey {
    /// Legacy lookup without externs (empty digest); byte-for-byte v1 key.
    pub fn new(snapshot_id: &str, context_id: &str, toolchain_id: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"ravum-daemon-cache-key-v1");
        push_field(&mut hasher, &DAEMON_SCHEMA_VERSION.to_le_bytes());
        push_field(&mut hasher, snapshot_id.as_bytes());
        push_field(&mut hasher, context_id.as_bytes());
        push_field(&mut hasher, toolchain_id.as_bytes());
        Self {
            key: hex(&hasher.finalize()),
            snapshot_id: snapshot_id.to_owned(),
            context_id: context_id.to_owned(),
            toolchain_id: toolchain_id.to_owned(),
            externs_digest: String::new(),
        }
    }

    /// Lookup including the extern identity of the request. The zero-extern case
    /// keeps the legacy v1 key byte-for-byte (existing caches/tests unchanged);
    /// a non-empty extern digest switches to v2, which adds that digest to the key.
    pub fn new_with_externs(
        snapshot_id: &str,
        context_id: &str,
        toolchain_id: &str,
        externs_digest: &str,
    ) -> Self {
        if externs_digest.is_empty() {
            return Self::new(snapshot_id, context_id, toolchain_id);
        }
        let mut hasher = Sha256::new();
        hasher.update(b"ravum-daemon-cache-key-v2");
        push_field(&mut hasher, &DAEMON_SCHEMA_VERSION.to_le_bytes());
        push_field(&mut hasher, snapshot_id.as_bytes());
        push_field(&mut hasher, context_id.as_bytes());
        push_field(&mut hasher, toolchain_id.as_bytes());
        push_field(&mut hasher, externs_digest.as_bytes());
        Self {
            key: hex(&hasher.finalize()),
            snapshot_id: snapshot_id.to_owned(),
            context_id: context_id.to_owned(),
            toolchain_id: toolchain_id.to_owned(),
            externs_digest: externs_digest.to_owned(),
        }
    }
}

/// Outcome of a lookup; `Corrupt` is reported separately so the daemon can count
/// integrity faults without treating them as ordinary misses.
#[derive(Debug)]
pub enum LookupOutcome {
    Hit {
        verdict: Box<CachedVerdict>,
        kind: CacheKind,
    },
    Miss,
    Corrupt {
        reason: String,
    },
}

struct CacheInner {
    map: HashMap<String, CachedVerdict>,
    order: VecDeque<String>,
}

/// Two-level content-addressed result cache.
pub struct ResultCache {
    dir: PathBuf,
    max_entries: usize,
    inner: Mutex<CacheInner>,
}

impl ResultCache {
    pub fn new(dir: PathBuf, max_entries: usize) -> Result<Self> {
        std::fs::create_dir_all(&dir)?;
        Ok(Self {
            dir,
            max_entries: max_entries.max(1),
            inner: Mutex::new(CacheInner {
                map: HashMap::new(),
                order: VecDeque::new(),
            }),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Number of entries currently held in memory.
    pub fn entries(&self) -> usize {
        self.inner.lock().map(|inner| inner.map.len()).unwrap_or(0)
    }

    /// Number of verdict files currently on disk.
    pub fn disk_entries(&self) -> usize {
        std::fs::read_dir(&self.dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|entry| {
                        entry
                            .path()
                            .extension()
                            .map(|ext| ext == "json")
                            .unwrap_or(false)
                    })
                    .count()
            })
            .unwrap_or(0)
    }

    /// Drops the given keys from the in-memory level only (used by GC after the
    /// on-disk verdicts were collected). Returns how many memory entries were
    /// evicted. Disk files are removed by the GC pass itself.
    pub fn remove_keys(&self, keys: &[String]) -> usize {
        let Ok(mut inner) = self.inner.lock() else {
            return 0;
        };
        let mut removed = 0usize;
        for key in keys {
            if inner.map.remove(key).is_some() {
                removed += 1;
            }
            inner.order.retain(|existing| existing != key);
        }
        removed
    }

    pub fn lookup(&self, expected: &CacheLookupKey) -> LookupOutcome {
        // L1: memory.
        {
            let mut inner = match self.inner.lock() {
                Ok(inner) => inner,
                Err(_) => {
                    return LookupOutcome::Corrupt {
                        reason: "cache lock poisoned".to_owned(),
                    }
                }
            };
            if let Some(verdict) = inner.map.get(&expected.key).cloned() {
                if verdict.verify(expected).is_ok() {
                    touch(&mut inner, &expected.key);
                    return LookupOutcome::Hit {
                        verdict: Box::new(verdict),
                        kind: CacheKind::Memory,
                    };
                }
                inner.map.remove(&expected.key);
                inner.order.retain(|key| key != &expected.key);
            }
        }
        // L2: disk.
        let path = self.dir.join(format!("{}.json", expected.key));
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return LookupOutcome::Miss
            }
            Err(error) => {
                return LookupOutcome::Corrupt {
                    reason: format!("cannot read cache entry {}: {error}", path.display()),
                }
            }
        };
        let verdict: CachedVerdict = match serde_json::from_slice(&bytes) {
            Ok(verdict) => verdict,
            Err(error) => {
                return LookupOutcome::Corrupt {
                    reason: format!("cache entry {} is unparsable: {error}", path.display()),
                }
            }
        };
        if let Err(reason) = verdict.verify(expected) {
            return LookupOutcome::Corrupt { reason };
        }
        self.insert_memory(verdict.clone());
        LookupOutcome::Hit {
            verdict: Box::new(verdict),
            kind: CacheKind::Disk,
        }
    }

    /// Persists then caches a verdict. Disk persistence makes a daemon restart
    /// reuse work; a failed disk write degrades to memory-only rather than
    /// failing the analysis (the caller already has a real verdict).
    pub fn insert(&self, verdict: &CachedVerdict) -> Result<()> {
        let bytes = serde_json::to_vec(verdict)
            .map_err(|e| DaemonError::internal(format!("cannot serialize cache verdict: {e}")))?;
        let path = self.dir.join(format!("{}.json", verdict.key));
        atomic_write(&path, &bytes)?;
        self.insert_memory(verdict.clone());
        Ok(())
    }

    fn insert_memory(&self, verdict: CachedVerdict) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        let key = verdict.key.clone();
        inner.map.insert(key.clone(), verdict);
        inner.order.retain(|existing| existing != &key);
        inner.order.push_back(key);
        while inner.map.len() > self.max_entries {
            let Some(oldest) = inner.order.pop_front() else {
                break;
            };
            inner.map.remove(&oldest);
        }
    }

    /// Drops both cache levels. Used by operational resets and by the
    /// invalidation tests that need to force a true cache miss.
    pub fn clear(&self) -> Result<()> {
        if let Ok(mut inner) = self.inner.lock() {
            inner.map.clear();
            inner.order.clear();
        }
        if self.dir.exists() {
            for entry in std::fs::read_dir(&self.dir)?.flatten() {
                let path = entry.path();
                if path.is_file() {
                    std::fs::remove_file(&path)?;
                }
            }
        }
        Ok(())
    }
}

fn touch(inner: &mut CacheInner, key: &str) {
    inner.order.retain(|existing| existing != key);
    inner.order.push_back(key.to_owned());
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::Severity;

    fn key() -> CacheLookupKey {
        CacheLookupKey::new("snapshot-a", "context-a", "toolchain-a")
    }

    fn verdict(expected: &CacheLookupKey) -> CachedVerdict {
        CachedVerdict::new(
            expected.key.clone(),
            expected.snapshot_id.clone(),
            expected.context_id.clone(),
            expected.toolchain_id.clone(),
            false,
            Some(1),
            vec![Diagnostic {
                severity: Severity::Error,
                code: Some("E0425".to_owned()),
                message: "cannot find value `x`".to_owned(),
                file: Some("src/lib.rs".to_owned()),
                line: Some(2),
                column: Some(5),
                end_line: Some(2),
                end_column: Some(6),
                rendered: None,
            }],
            42,
        )
    }

    /// A directory that no other test of this binary can use.
    ///
    /// The previous helper keyed the directory on `process::id()` and a
    /// second-granularity timestamp: tests running in parallel inside the same second
    /// shared the same directory (and `remove_dir_all` deleted each other's entries),
    /// which made `corrupt_payload_is_rejected_as_corrupt`, `clear_empties_both_levels`
    /// and `max_entries_evicts_lru_entries` fail intermittently under `--test-threads>1`.
    /// Observed in the MVP-0b full-workspace run; fixed with a process-unique sequence.
    fn unique_dir(tag: &str) -> PathBuf {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("ravum-daemon-{tag}-{}-{seq}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn cache() -> (ResultCache, PathBuf) {
        let dir = unique_dir("cache");
        let cache = ResultCache::new(dir.clone(), 4).unwrap();
        (cache, dir)
    }

    #[test]
    fn memory_hit_and_disk_persistence() {
        let (cache, dir) = cache();
        let expected = key();
        assert!(matches!(cache.lookup(&expected), LookupOutcome::Miss));
        cache.insert(&verdict(&expected)).unwrap();
        match cache.lookup(&expected) {
            LookupOutcome::Hit { kind, verdict } => {
                assert_eq!(kind, CacheKind::Memory);
                assert_eq!(verdict.diagnostics.len(), 1);
            }
            other => panic!("expected memory hit, got {other:?}"),
        }
        drop(cache);
        // A fresh cache over the same directory is a disk hit: restart-safe.
        let reopened = ResultCache::new(dir.clone(), 4).unwrap();
        match reopened.lookup(&expected) {
            LookupOutcome::Hit { kind, .. } => assert_eq!(kind, CacheKind::Disk),
            other => panic!("expected disk hit, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_payload_is_rejected_as_corrupt() {
        let (cache, dir) = cache();
        let expected = key();
        cache.insert(&verdict(&expected)).unwrap();
        let path = dir.join(format!("{}.json", expected.key));
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        value["diagnostics"][0]["message"] = serde_json::json!("tampered message");
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        // Drop the in-memory level so the tampered disk entry is really read.
        drop(cache);
        let cache = ResultCache::new(dir.clone(), 4).unwrap();
        match cache.lookup(&expected) {
            LookupOutcome::Corrupt { reason } => {
                assert!(reason.contains("digest"), "reason: {reason}")
            }
            other => panic!("expected corrupt, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn max_entries_evicts_lru_entries() {
        let dir = unique_dir("cache-lru");
        let cache = ResultCache::new(dir.clone(), 2).unwrap();
        for index in 0..4 {
            let expected =
                CacheLookupKey::new(&format!("snapshot-{index}"), "context", "toolchain");
            cache.insert(&verdict(&expected)).unwrap();
        }
        assert_eq!(cache.entries(), 2, "memory cache must respect max_entries");
        assert_eq!(cache.disk_entries(), 4, "disk cache is content-addressed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clear_empties_both_levels() {
        let (cache, dir) = cache();
        let expected = key();
        cache.insert(&verdict(&expected)).unwrap();
        cache.clear().unwrap();
        assert!(matches!(cache.lookup(&expected), LookupOutcome::Miss));
        assert_eq!(cache.disk_entries(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
