//! JSON report types: structured diagnostics, per-stage latencies, cache state.
//!
//! A report is the daemon's only output contract. `status` is the stale-result
//! guard: only `fresh` reports carry diagnostics that the caller may treat as
//! current.

use crate::cache::CacheReport;
use crate::config::{ToolchainInfo, DAEMON_SCHEMA_VERSION};
use crate::diagnostics::Diagnostic;
use serde::{Deserialize, Serialize};

/// Lifecycle status of one analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportStatus {
    /// The answer belongs to the newest revision and may be used.
    Fresh,
    /// A newer revision arrived before the answer: the result is not current.
    Superseded,
    /// The in-flight compiler process was killed by a superseding revision.
    Cancelled,
    /// The compiler exceeded the configured wall-clock budget and was killed.
    TimedOut,
}

impl ReportStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ReportStatus::Fresh => "fresh",
            ReportStatus::Superseded => "superseded",
            ReportStatus::Cancelled => "cancelled",
            ReportStatus::TimedOut => "timed_out",
        }
    }
}

/// Per-stage wall-clock latencies of one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Timings {
    /// Validation + content hashing + snapshot/checkout materialization.
    pub accept_ms: u64,
    /// Waiting for a compiler slot in the job gate.
    pub queue_ms: u64,
    /// Cache lookup (memory then disk).
    pub lookup_ms: u64,
    /// `rustc` wall time; `0` on a cache hit.
    pub analysis_ms: u64,
    /// Whole request.
    pub total_ms: u64,
}

/// What the daemon did with the immutable snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotReport {
    pub files: usize,
    pub bytes: u64,
    pub written_files: usize,
    pub removed_files: usize,
    pub verified_files: usize,
    /// Immutable snapshot directory (daemon-private, never a user path).
    pub dir: String,
}

/// One analysis answer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisReport {
    pub schema_version: u32,
    pub status: ReportStatus,
    pub revision: u64,
    pub current_revision: u64,
    pub unit: String,
    pub root: String,
    pub snapshot_id: String,
    pub context_id: String,
    pub toolchain: ToolchainInfo,
    /// Persistent incremental directory actually passed to `rustc`.
    pub incremental_dir: String,
    pub cache: CacheReport,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub diagnostics: Vec<Diagnostic>,
    pub timings: Timings,
    pub snapshot: Option<SnapshotReport>,
    pub daemon_version: String,
    /// Human-readable reason for a non-fresh status; `None` when fresh.
    pub message: Option<String>,
}

impl AnalysisReport {
    /// Only a fresh report may be consumed as a current analysis.
    pub fn is_current(&self) -> bool {
        self.status == ReportStatus::Fresh
    }

    pub fn from_cache(&self) -> bool {
        self.cache.hit
    }

    pub fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity.is_error())
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == crate::diagnostics::Severity::Warning)
            .count()
    }

    /// Explicit JSON serialization used by the binary protocol and evidence.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_else(|error| {
            serde_json::json!({
                "schema_version": DAEMON_SCHEMA_VERSION,
                "status": "serialization_error",
                "message": format!("{error}"),
            })
        })
    }
}

/// Cumulative daemon counters. Only observed facts: no estimated savings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DaemonStats {
    pub requests: u64,
    pub fresh: u64,
    pub superseded: u64,
    pub cancelled: u64,
    pub timed_out: u64,
    pub cache_hits_memory: u64,
    pub cache_hits_disk: u64,
    pub cache_misses: u64,
    pub cache_corrupt: u64,
    pub rustc_runs: u64,
    pub rustc_ms_total: u64,
    pub peak_active_jobs: u64,
    pub snapshots_created: u64,
    pub snapshots_verified: u64,
    pub unit_files_written: u64,
    /// Explicit GC runs performed (P0-H retention).
    pub gc_runs: u64,
    /// Bytes actually reclaimed by successful GC runs.
    pub gc_freed_bytes: u64,
    /// Cache entries / snapshots / build contexts removed by GC.
    pub gc_deleted_entries: u64,
}

impl DaemonStats {
    /// Cache hit rate over cache lookups. `None` when nothing was looked up.
    pub fn cache_hit_rate(&self) -> Option<f64> {
        let lookups = self.cache_hits_memory + self.cache_hits_disk + self.cache_misses;
        (lookups > 0).then(|| {
            (self.cache_hits_memory + self.cache_hits_disk) as f64 / lookups as f64
        })
    }
}
