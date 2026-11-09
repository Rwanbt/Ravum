//! Automatic background retention GC (P0-H suite).
//!
//! The manual GC ([`crate::daemon::Daemon::gc`]) is explicit. This module adds an
//! *opt-in* automatic mode: the daemon counts requests, and when
//! `check_every_requests` is reached a **long-lived maintenance thread** evaluates
//! the disk threshold and, if needed, runs the same bounded GC under the same
//! exclusive I/O lock as analyses.
//!
//! Guarantees:
//!
//! * **off the analysis path**: an analysis only increments an atomic counter and
//!   (once every `check_every_requests`) signals a condition variable. The size
//!   scan and the collection run in the maintenance thread; below threshold no
//!   collection happens at all;
//! * **no deletion during an analysis**: the GC takes the exclusive side of the
//!   analysis I/O lock (see `Daemon::gc`), and `idle_only` additionally skips a
//!   pass while any analysis is in flight;
//! * **a failure never fails an analysis**: errors are recorded in stats, in the
//!   JSONL journal and returned to the maintenance loop, which keeps running;
//! * **bounded journal**: `<work>/gc-auto.log` keeps at most
//!   [`AUTO_GC_JOURNAL_MAX_ENTRIES`] JSON lines (oldest dropped).
//!
//! Nothing is enabled by default: `AutoGcPolicy::default().enabled == false`.

use crate::error::{DaemonError, Result};
use crate::fsutil::atomic_write;
use crate::gc::{GcMode, GcPolicy, GcReport};
use crate::report::AutoGcLast;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// JSONL journal of automatic GC events, below the daemon work directory.
pub const AUTO_GC_JOURNAL_FILE: &str = "gc-auto.log";
/// Hard bound on journal lines: the oldest events are dropped.
pub const AUTO_GC_JOURNAL_MAX_ENTRIES: usize = 256;
/// Default check interval when a policy omits it.
pub const DEFAULT_AUTO_GC_CHECK_EVERY: u64 = 20;
/// Default `keep_recent` reused from the manual GC policy.
pub const DEFAULT_AUTO_GC_KEEP_RECENT: usize = 8;

fn default_check_every() -> u64 {
    DEFAULT_AUTO_GC_CHECK_EVERY
}

fn default_keep_recent() -> usize {
    DEFAULT_AUTO_GC_KEEP_RECENT
}

/// Opt-in automatic background GC policy. Serializable so it can be exposed by a
/// configuration file or the JSON-lines protocol; unknown fields are rejected so
/// a typo cannot silently change a deletion policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutoGcPolicy {
    /// Master switch. Default `false`: no maintenance thread, no background cost.
    #[serde(default)]
    pub enabled: bool,
    /// Evaluate the threshold once every N completed analyses (>= 1).
    #[serde(default = "default_check_every")]
    pub check_every_requests: u64,
    /// Trigger: only run a collection when the collected state reaches this size.
    #[serde(default)]
    pub min_total_bytes: Option<u64>,
    /// Target: byte cap passed to the bounded GC (`max_total_bytes`).
    #[serde(default)]
    pub max_total_bytes: Option<u64>,
    /// Category caps forwarded to the bounded GC.
    #[serde(default)]
    pub max_cache_bytes: Option<u64>,
    #[serde(default)]
    pub max_snapshot_bytes: Option<u64>,
    #[serde(default)]
    pub max_incremental_bytes: Option<u64>,
    /// Recent served contents that are never deleted.
    #[serde(default = "default_keep_recent")]
    pub keep_recent: usize,
    /// Only collect while no analysis is in flight (the GC lock already prevents
    /// overlap; this makes the *choice* to collect wait for an idle window).
    #[serde(default)]
    pub idle_only: bool,
}

impl Default for AutoGcPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            check_every_requests: DEFAULT_AUTO_GC_CHECK_EVERY,
            min_total_bytes: None,
            max_total_bytes: None,
            max_cache_bytes: None,
            max_snapshot_bytes: None,
            max_incremental_bytes: None,
            keep_recent: DEFAULT_AUTO_GC_KEEP_RECENT,
            idle_only: false,
        }
    }
}

impl AutoGcPolicy {
    /// Fail-closed validation. A disabled policy is inert and accepts any values
    /// (it is never evaluated); an enabled one must be fully specified:
    /// `0 < max_total_bytes < min_total_bytes`, `check_every_requests >= 1`,
    /// `keep_recent >= 1`, no cap of zero.
    pub fn validate(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        if self.check_every_requests == 0 {
            return Err(DaemonError::invalid(
                "auto_gc policy: check_every_requests must be >= 1",
            ));
        }
        if self.keep_recent == 0 {
            return Err(DaemonError::invalid(
                "auto_gc policy: keep_recent must be >= 1",
            ));
        }
        let min = self.min_total_bytes.ok_or_else(|| {
            DaemonError::invalid("auto_gc policy: min_total_bytes is required when enabled")
        })?;
        let max = self.max_total_bytes.ok_or_else(|| {
            DaemonError::invalid("auto_gc policy: max_total_bytes is required when enabled")
        })?;
        if min == 0 || max == 0 {
            return Err(DaemonError::invalid(
                "auto_gc policy: byte thresholds must be >= 1",
            ));
        }
        if max >= min {
            return Err(DaemonError::invalid(format!(
                "auto_gc policy: max_total_bytes ({max}) must be strictly below \
                 min_total_bytes ({min}), otherwise a collection cannot bring the \
                 state under the trigger"
            )));
        }
        for (name, cap) in [
            ("max_cache_bytes", self.max_cache_bytes),
            ("max_snapshot_bytes", self.max_snapshot_bytes),
            ("max_incremental_bytes", self.max_incremental_bytes),
        ] {
            if cap == Some(0) {
                return Err(DaemonError::invalid(format!(
                    "auto_gc policy: {name} must be >= 1 when set"
                )));
            }
        }
        Ok(())
    }

    /// The bounded manual GC policy used for one automatic pass.
    pub fn gc_policy(&self) -> GcPolicy {
        GcPolicy {
            max_cache_bytes: self.max_cache_bytes,
            max_snapshot_bytes: self.max_snapshot_bytes,
            max_incremental_bytes: self.max_incremental_bytes,
            max_total_bytes: self.max_total_bytes,
            keep_recent: self.keep_recent,
            // Automatic GC keeps the manual defaults for compaction (off): an
            // operator can request compaction explicitly; it is never implied by
            // a background threshold pass.
            compact_incremental: false,
            keep_incremental_sessions: crate::gc::DEFAULT_KEEP_INCREMENTAL_SESSIONS,
            dry_run: false,
        }
    }

    /// Compact description used in stats and evidence.
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({
            "enabled": self.enabled,
            "check_every_requests": self.check_every_requests,
            "min_total_bytes": self.min_total_bytes,
            "max_total_bytes": self.max_total_bytes,
            "max_cache_bytes": self.max_cache_bytes,
            "max_snapshot_bytes": self.max_snapshot_bytes,
            "max_incremental_bytes": self.max_incremental_bytes,
            "keep_recent": self.keep_recent,
            "idle_only": self.idle_only,
        })
    }
}

/// Decision of one threshold evaluation (pure, unit-tested).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoGcDecision {
    /// Below the trigger: no collection, no extra work.
    BelowThreshold { total_bytes: u64 },
    /// Trigger reached but `idle_only` and an analysis is in flight: retry later.
    Busy { total_bytes: u64 },
    /// Collect now: the state reaches the trigger and the daemon is idle enough.
    Run { total_bytes: u64 },
}

/// One JSONL line in `<work>/gc-auto.log`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoGcJournalEntry {
    pub at_unix: u64,
    pub mode: String,
    pub ok: bool,
    pub trigger_total_bytes: u64,
    pub total_before_bytes: u64,
    pub total_after_bytes: u64,
    pub freed_bytes: u64,
    pub duration_ms: u64,
    pub deleted_entries: u64,
    pub requests_analyzed: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Appends one event to the bounded JSONL journal, dropping the oldest lines when
/// the bound is exceeded. Returns the number of lines kept. Best effort: callers
/// record a journal failure but never fail an analysis because of it.
pub fn append_journal(path: &Path, entry: &AutoGcJournalEntry) -> Result<usize> {
    let mut lines: Vec<String> = match std::fs::read_to_string(path) {
        Ok(text) => text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(str::to_owned)
            .collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => {
            return Err(DaemonError::io(format!(
                "cannot read auto-GC journal {}: {error}",
                path.display()
            )))
        }
    };
    let encoded = serde_json::to_string(entry).map_err(|error| {
        DaemonError::internal(format!("cannot serialize auto-GC journal entry: {error}"))
    })?;
    lines.push(encoded);
    if lines.len() > AUTO_GC_JOURNAL_MAX_ENTRIES {
        let excess = lines.len() - AUTO_GC_JOURNAL_MAX_ENTRIES;
        lines.drain(0..excess);
    }
    let text = format!("{}\n", lines.join("\n"));
    atomic_write(path, text.as_bytes())?;
    Ok(lines.len())
}

/// Recorded result of the last automatic GC attempt.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AutoGcStats {
    pub enabled: bool,
    pub checks: u64,
    pub runs: u64,
    pub failures: u64,
    pub journal_entries: u64,
    pub busy: bool,
    pub pending: bool,
    pub requests_since_check: u64,
    pub last: Option<AutoGcLast>,
}

/// Shared state between the daemon and its maintenance thread.
pub struct AutoGcRuntime {
    policy: AutoGcPolicy,
    journal_path: PathBuf,
    requests_since_check: AtomicU64,
    checks: AtomicU64,
    runs: AtomicU64,
    failures: AtomicU64,
    journal_entries: AtomicU64,
    last: Mutex<Option<AutoGcLast>>,
    failure_injection: Mutex<Option<String>>,
    shutdown: AtomicBool,
    busy: AtomicBool,
    pending: AtomicBool,
    /// Condvar signalling the maintenance thread. Also used to make
    /// `wait_auto_gc_idle` deterministic in tests.
    pub(crate) signal_lock: Mutex<()>,
    pub(crate) signal: Condvar,
}

impl AutoGcRuntime {
    /// Creates the runtime, validating the policy first (fail-closed).
    pub fn new(policy: AutoGcPolicy, work_dir: &Path) -> Result<Arc<Self>> {
        policy.validate()?;
        Ok(Arc::new(Self {
            policy,
            journal_path: work_dir.join(AUTO_GC_JOURNAL_FILE),
            requests_since_check: AtomicU64::new(0),
            checks: AtomicU64::new(0),
            runs: AtomicU64::new(0),
            failures: AtomicU64::new(0),
            journal_entries: AtomicU64::new(0),
            last: Mutex::new(None),
            failure_injection: Mutex::new(None),
            shutdown: AtomicBool::new(false),
            busy: AtomicBool::new(false),
            pending: AtomicBool::new(false),
            signal_lock: Mutex::new(()),
            signal: Condvar::new(),
        }))
    }

    pub fn policy(&self) -> &AutoGcPolicy {
        &self.policy
    }

    pub fn enabled(&self) -> bool {
        self.policy.enabled
    }

    pub fn journal_path(&self) -> &Path {
        &self.journal_path
    }

    /// Called by every completed analysis. Returns `true` when the check interval
    /// is reached (the caller then signals the maintenance thread). Cost below the
    /// interval: one atomic increment.
    pub fn note_request(&self) -> bool {
        if !self.policy.enabled {
            return false;
        }
        let completed = self.requests_since_check.fetch_add(1, Ordering::SeqCst) + 1;
        if completed >= self.policy.check_every_requests {
            self.pending.store(true, Ordering::SeqCst);
            self.signal.notify_all();
            true
        } else {
            false
        }
    }

    /// Consumed by the maintenance thread: true when a threshold evaluation is due.
    pub fn take_due(&self) -> bool {
        if !self.policy.enabled || self.shutdown.load(Ordering::SeqCst) {
            return false;
        }
        if self.requests_since_check.load(Ordering::SeqCst) < self.policy.check_every_requests {
            return false;
        }
        self.requests_since_check.store(0, Ordering::SeqCst);
        self.checks.fetch_add(1, Ordering::SeqCst);
        true
    }

    /// `idle_only` retry: make the next completed request re-trigger evaluation.
    pub fn reschedule_retry(&self) {
        let retry = self.policy.check_every_requests.saturating_sub(1);
        self.requests_since_check.store(retry, Ordering::SeqCst);
    }

    /// Pure decision used by the maintenance thread (and unit tests).
    pub fn decision(&self, total_bytes: u64, active_analyses: usize) -> AutoGcDecision {
        let trigger = self.policy.min_total_bytes.unwrap_or(u64::MAX);
        if total_bytes < trigger {
            return AutoGcDecision::BelowThreshold { total_bytes };
        }
        if self.policy.idle_only && active_analyses > 0 {
            return AutoGcDecision::Busy { total_bytes };
        }
        AutoGcDecision::Run { total_bytes }
    }

    /// True when a threshold evaluation is queued or running.
    pub fn is_busy_or_pending(&self) -> bool {
        self.busy.load(Ordering::SeqCst) || self.pending.load(Ordering::SeqCst)
    }

    pub fn set_busy(&self, busy: bool) {
        self.busy.store(busy, Ordering::SeqCst);
    }

    pub fn request_shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        self.signal.notify_all();
    }

    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    /// Blocks the maintenance thread until a signal or timeout. Returns true when
    /// a pending evaluation should be attempted.
    pub fn wait_for_work(&self, timeout: Duration) -> bool {
        let guard = match self.signal_lock.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if self.pending.swap(false, Ordering::SeqCst) {
            return true;
        }
        let (guard, _) = match self.signal.wait_timeout(guard, timeout) {
            Ok(result) => result,
            Err(poisoned) => poisoned.into_inner(),
        };
        drop(guard);
        self.pending.swap(false, Ordering::SeqCst)
    }

    /// Test seam: the next automatic attempt fails with this message instead of
    /// touching the filesystem. Used to prove that an auto-GC failure never fails
    /// an analysis and is exposed in stats/journal.
    pub fn inject_failure(&self, message: Option<String>) {
        if let Ok(mut slot) = self.failure_injection.lock() {
            *slot = message;
        }
    }

    pub fn take_injected_failure(&self) -> Option<String> {
        self.failure_injection
            .lock()
            .ok()
            .and_then(|mut slot| slot.take())
    }

    pub fn requests_analyzed(&self) -> u64 {
        self.checks.load(Ordering::SeqCst)
    }

    fn record(&self, entry: AutoGcLast, journal: AutoGcJournalEntry) {
        if entry.ok {
            self.runs.fetch_add(1, Ordering::SeqCst);
        } else {
            self.failures.fetch_add(1, Ordering::SeqCst);
        }
        if let Ok(mut slot) = self.last.lock() {
            *slot = Some(entry);
        }
        match append_journal(&self.journal_path, &journal) {
            Ok(lines) => {
                self.journal_entries.store(lines as u64, Ordering::SeqCst);
            }
            Err(_) => {
                // The journal is an observability aid: a write failure is not a
                // collection failure and is deliberately not escalated.
            }
        }
    }

    /// Records a successful automatic collection.
    pub fn record_success(
        &self,
        report: &GcReport,
        total_before_bytes: u64,
        requests_analyzed: u64,
    ) {
        let entry = AutoGcLast {
            at_unix: crate::cache::now_unix(),
            duration_ms: report.duration_ms,
            freed_bytes: report.freed_bytes,
            total_before_bytes,
            total_after_bytes: report.after.total_bytes,
            deleted_entries: report.deleted.entry_count() as u64,
            ok: true,
            error: None,
        };
        let journal = AutoGcJournalEntry {
            at_unix: entry.at_unix,
            mode: GcMode::Auto.as_str().to_owned(),
            ok: true,
            trigger_total_bytes: self.policy.min_total_bytes.unwrap_or(0),
            total_before_bytes,
            total_after_bytes: entry.total_after_bytes,
            freed_bytes: report.freed_bytes,
            duration_ms: report.duration_ms,
            deleted_entries: entry.deleted_entries,
            requests_analyzed,
            error: None,
        };
        self.record(entry, journal);
    }

    /// Records a failed automatic collection (the daemon keeps running).
    pub fn record_failure(&self, error: String, total_before_bytes: u64, requests_analyzed: u64) {
        let entry = AutoGcLast {
            at_unix: crate::cache::now_unix(),
            duration_ms: 0,
            freed_bytes: 0,
            total_before_bytes,
            total_after_bytes: total_before_bytes,
            deleted_entries: 0,
            ok: false,
            error: Some(error.clone()),
        };
        let journal = AutoGcJournalEntry {
            at_unix: entry.at_unix,
            mode: GcMode::Auto.as_str().to_owned(),
            ok: false,
            trigger_total_bytes: self.policy.min_total_bytes.unwrap_or(0),
            total_before_bytes,
            total_after_bytes: total_before_bytes,
            freed_bytes: 0,
            duration_ms: 0,
            deleted_entries: 0,
            requests_analyzed,
            error: Some(error),
        };
        self.record(entry, journal);
    }

    pub fn snapshot(&self) -> AutoGcStats {
        AutoGcStats {
            enabled: self.policy.enabled,
            checks: self.checks.load(Ordering::SeqCst),
            runs: self.runs.load(Ordering::SeqCst),
            failures: self.failures.load(Ordering::SeqCst),
            journal_entries: self.journal_entries.load(Ordering::SeqCst),
            busy: self.busy.load(Ordering::SeqCst),
            pending: self.pending.load(Ordering::SeqCst),
            requests_since_check: self.requests_since_check.load(Ordering::SeqCst),
            last: self.last.lock().ok().and_then(|slot| slot.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::GcReport;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ravum-daemon-auto-gc-{name}-{}-{}",
            std::process::id(),
            crate::cache::now_unix()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn policy(enabled: bool) -> AutoGcPolicy {
        AutoGcPolicy {
            enabled,
            check_every_requests: 5,
            min_total_bytes: Some(1_000_000),
            max_total_bytes: Some(500_000),
            max_cache_bytes: Some(10_000),
            max_snapshot_bytes: Some(20_000),
            max_incremental_bytes: Some(400_000),
            keep_recent: 4,
            idle_only: true,
        }
    }

    #[test]
    fn disabled_policy_is_inert_and_accepts_defaults() {
        let disabled = AutoGcPolicy::default();
        assert!(!disabled.enabled);
        disabled.validate().unwrap();
        assert!(!disabled.summary()["enabled"].as_bool().unwrap());
    }

    #[test]
    fn enabled_policy_is_validated_fail_closed() {
        let mut bad = policy(true);
        bad.min_total_bytes = None;
        assert!(bad.validate().is_err());
        bad = policy(true);
        bad.max_total_bytes = None;
        assert!(bad.validate().is_err());
        bad = policy(true);
        bad.max_total_bytes = Some(1_000_000); // equals trigger
        assert!(bad.validate().is_err());
        bad = policy(true);
        bad.check_every_requests = 0;
        assert!(bad.validate().is_err());
        bad = policy(true);
        bad.keep_recent = 0;
        assert!(bad.validate().is_err());
        bad = policy(true);
        bad.max_cache_bytes = Some(0);
        assert!(bad.validate().is_err());
        policy(true).validate().unwrap();
    }

    #[test]
    fn policy_roundtrips_and_rejects_unknown_fields() {
        let text = serde_json::to_string(&policy(true)).unwrap();
        let parsed: AutoGcPolicy = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed, policy(true));
        assert!(
            serde_json::from_str::<AutoGcPolicy>(r#"{"enabled":true,"min_total":"x"}"#).is_err()
        );
        let gc_policy = policy(true).gc_policy();
        assert!(!gc_policy.dry_run);
        assert_eq!(gc_policy.keep_recent, 4);
    }

    #[test]
    fn note_request_triggers_only_on_interval() {
        let dir = temp_dir("note");
        let runtime = AutoGcRuntime::new(policy(true), &dir).unwrap();
        for _ in 0..4 {
            assert!(!runtime.note_request());
        }
        assert!(runtime.note_request());
        assert!(runtime.take_due());
        assert!(runtime.checks.load(Ordering::SeqCst) >= 1);
        // Not due again until another interval.
        assert!(!runtime.take_due());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn decision_respects_threshold_and_idle_only() {
        let dir = temp_dir("decision");
        let runtime = AutoGcRuntime::new(policy(true), &dir).unwrap();
        assert_eq!(
            runtime.decision(999_999, 0),
            AutoGcDecision::BelowThreshold {
                total_bytes: 999_999
            }
        );
        assert_eq!(
            runtime.decision(1_000_000, 2),
            AutoGcDecision::Busy {
                total_bytes: 1_000_000
            }
        );
        assert_eq!(
            runtime.decision(1_500_000, 0),
            AutoGcDecision::Run {
                total_bytes: 1_500_000
            }
        );
        let mut not_idle = policy(true);
        not_idle.idle_only = false;
        let runtime = AutoGcRuntime::new(not_idle, &dir).unwrap();
        assert_eq!(
            runtime.decision(1_500_000, 3),
            AutoGcDecision::Run {
                total_bytes: 1_500_000
            }
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn journal_is_bounded_and_keeps_newest() {
        let dir = temp_dir("journal");
        let path = dir.join(AUTO_GC_JOURNAL_FILE);
        for index in 0..(AUTO_GC_JOURNAL_MAX_ENTRIES + 24) {
            let entry = AutoGcJournalEntry {
                at_unix: index as u64,
                mode: "auto".to_owned(),
                ok: true,
                trigger_total_bytes: 1,
                total_before_bytes: 2,
                total_after_bytes: 1,
                freed_bytes: 1,
                duration_ms: 1,
                deleted_entries: 1,
                requests_analyzed: index as u64,
                error: None,
            };
            append_journal(&path, &entry).unwrap();
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect();
        assert_eq!(lines.len(), AUTO_GC_JOURNAL_MAX_ENTRIES);
        assert!(lines.last().unwrap().contains("\"at_unix\""));
        let first: AutoGcJournalEntry = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first.at_unix, 24);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn failure_is_recorded_and_cleared() {
        let dir = temp_dir("failure");
        let runtime = AutoGcRuntime::new(policy(true), &dir).unwrap();
        runtime.inject_failure(Some("injected for tests".to_owned()));
        assert_eq!(
            runtime.take_injected_failure().as_deref(),
            Some("injected for tests")
        );
        assert!(runtime.take_injected_failure().is_none());
        runtime.record_failure("boom".to_owned(), 42, 1);
        let stats = runtime.snapshot();
        assert_eq!(stats.failures, 1);
        assert!(!stats.last.as_ref().unwrap().ok);
        assert_eq!(stats.last.as_ref().unwrap().error.as_deref(), Some("boom"));
        let text = std::fs::read_to_string(runtime.journal_path()).unwrap();
        assert!(text.contains("\"ok\":false"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Documents why record_success is exercised through integration tests: a
    /// `GcReport` is produced by `run_gc`, and the unit test only checks that the
    /// runtime accepts one without panicking on an empty report.
    #[test]
    fn record_success_updates_runs_and_last() {
        let dir = temp_dir("success");
        let runtime = AutoGcRuntime::new(policy(true), &dir).unwrap();
        let report = GcReport {
            schema_version: 1,
            kind: "ravum-daemon-gc-report".to_owned(),
            mode: GcMode::Auto,
            dry_run: false,
            policy: policy(true).gc_policy(),
            started_unix: 0,
            duration_ms: 12,
            before: Default::default(),
            after: Default::default(),
            planned_freed_bytes: 0,
            freed_bytes: 0,
            deleted: Default::default(),
            planned: Default::default(),
            kept: Default::default(),
            budgets: Default::default(),
            protected: Default::default(),
            compaction: Default::default(),
            errors: Vec::new(),
            notes: Vec::new(),
        };
        runtime.record_success(&report, 0, 1);
        let stats = runtime.snapshot();
        assert_eq!(stats.runs, 1);
        assert!(stats.last.as_ref().unwrap().ok);
        assert_eq!(stats.journal_entries, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
