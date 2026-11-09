//! JSON report types: structured diagnostics, per-stage latencies, cache state.
//!
//! A report is the daemon's only output contract. `status` is the stale-result
//! guard: only `fresh` reports carry diagnostics that the caller may treat as
//! current.

use crate::auto_gc::AutoGcStats;
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

/// Result of the last automatic background GC attempt.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoGcLast {
    pub at_unix: u64,
    pub duration_ms: u64,
    pub freed_bytes: u64,
    pub total_before_bytes: u64,
    pub total_after_bytes: u64,
    pub deleted_entries: u64,
    pub ok: bool,
    pub error: Option<String>,
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
    /// Automatic background GC counters and last attempt. Disabled by default.
    #[serde(default)]
    pub auto_gc: AutoGcStats,
}

impl DaemonStats {
    /// Cache hit rate over cache lookups. `None` when nothing was looked up.
    pub fn cache_hit_rate(&self) -> Option<f64> {
        let lookups = self.cache_hits_memory + self.cache_hits_disk + self.cache_misses;
        (lookups > 0)
            .then(|| (self.cache_hits_memory + self.cache_hits_disk) as f64 / lookups as f64)
    }
}
/// One direct extern interface artifact used by a cross-crate analysis
/// (snapshot schema 2). The interface id is the SHA-256 of the `.rmeta` bytes; a
/// report whose two ids disagree is refused, never treated as a verified extern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternUsageReport {
    /// Name under which the dependent imported the crate (`--extern NAME=...`).
    pub name: String,
    /// Crate name of the dependency target.
    pub crate_name: String,
    /// Interface identity (SHA-256 of the `.rmeta` bytes) used at analysis time.
    pub interface_id: String,
    /// SHA-256 of the artifact bytes actually stored in the snapshot.
    pub rmeta_sha256: String,
}

impl ExternUsageReport {
    /// Both identities must agree and look like SHA-256; otherwise the report is
    /// inconsistent and must not be consumed.
    pub fn validates(&self) -> bool {
        self.interface_id == self.rmeta_sha256
            && is_sha256(&self.interface_id)
            && !self.name.is_empty()
            && !self.crate_name.is_empty()
    }
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .chars()
            .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase())
}

/// Ordered digest of the externs of one analysis, for report equality and audit
/// evidence (never used alone as a cache key).
pub fn externs_report_digest(externs: &[ExternUsageReport]) -> String {
    let mut ordered: Vec<&ExternUsageReport> = externs.iter().collect();
    ordered.sort_by(|a, b| a.name.cmp(&b.name));
    let mut canonical = String::new();
    for extern_usage in ordered {
        canonical.push_str(&extern_usage.name);
        canonical.push('=');
        canonical.push_str(&extern_usage.interface_id);
        canonical.push('\n');
    }
    crate::hash::sha256_hex(canonical.as_bytes())
}

#[cfg(test)]
mod extern_report_tests {
    use super::{externs_report_digest, ExternUsageReport};

    fn usage(name: &str, interface_id: &str) -> ExternUsageReport {
        ExternUsageReport {
            name: name.to_owned(),
            crate_name: name.to_owned(),
            interface_id: interface_id.to_owned(),
            rmeta_sha256: interface_id.to_owned(),
        }
    }

    #[test]
    fn extern_usage_roundtrips_and_validates() {
        let valid = usage("core", &"a".repeat(64));
        assert!(valid.validates());
        let text = serde_json::to_string(&valid).unwrap();
        let parsed: ExternUsageReport = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed, valid);
        // Unknown fields are rejected: a typo must not silently fall back.
        let with_unknown = format!("{{\"name\":\"core\",\"crate_name\":\"core\",\"interface_id\":\"{}\",\"rmeta_sha256\":\"{}\",\"extra\":1}}", "a".repeat(64), "a".repeat(64));
        assert!(serde_json::from_str::<ExternUsageReport>(&with_unknown).is_err());
        let mut inconsistent = valid.clone();
        inconsistent.rmeta_sha256 = "b".repeat(64);
        assert!(!inconsistent.validates());
        let mut uppercase = valid;
        uppercase.interface_id = uppercase.interface_id.to_uppercase();
        uppercase.rmeta_sha256 = uppercase.interface_id.clone();
        assert!(!uppercase.validates());
    }

    #[test]
    fn extern_digest_is_order_independent_but_interface_sensitive() {
        let a = usage("api", &"1".repeat(64));
        let b = usage("core", &"2".repeat(64));
        assert_eq!(
            externs_report_digest(&[a.clone(), b.clone()]),
            externs_report_digest(&[b.clone(), a.clone()])
        );
        let mut changed = b;
        changed.interface_id = "3".repeat(64);
        changed.rmeta_sha256 = changed.interface_id.clone();
        assert_ne!(
            externs_report_digest(&[a.clone(), changed]),
            externs_report_digest(&[a, usage("core", &"2".repeat(64))])
        );
    }
}
