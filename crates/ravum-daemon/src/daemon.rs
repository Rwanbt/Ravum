//! The persistent analysis daemon: orchestration of revisions, snapshots,
//! cache, compiler jobs and reports.
//!
//! Lifecycle of one request:
//!
//! 1. **accept** — validate the contract, reduce `(source, context, toolchain)`
//!    to a SHA-256 identity, store the immutable snapshot and mirror the bytes
//!    into the stable compile unit;
//! 2. **lookup** — content-addressed cache (memory, then disk);
//! 3. **analyse** — on a miss, run the public `rustc` CLI with the persistent
//!    `-C incremental` directory;
//! 4. **answer** — never as current when the revision was superseded in the
//!    meantime; cancellation kills the child process when possible.
//!
//! The daemon owns its working directory; it never writes a user file.

use crate::auto_gc::{AutoGcDecision, AutoGcPolicy, AutoGcRuntime, AutoGcStats};
use crate::cache::{
    CacheKind, CacheLookupKey, CacheReport, CachedVerdict, LookupOutcome, ResultCache,
};
use crate::config::{BuildContext, DaemonConfig, Priority, ToolchainInfo};
use crate::diagnostics::parse_rustc_json;
use crate::error::{DaemonError, Result};
use crate::fsutil::safe_join;
use crate::gate::JobGate;
use crate::gc::{GcMode, GcPolicy, GcProtection, GcReport, RecentAnalysis, RetentionState};
use crate::report::{
    AnalysisReport, DaemonStats, ExternUsageReport, ReportStatus, SnapshotReport, Timings,
};
use crate::revision::{CancelToken, RevisionTracker, RunningJobs};
use crate::rustc_driver::{
    incremental_dir, out_dir, unit_dir, CompiledExtern, DriverOptions, RustcDriver,
};
use crate::snapshot::{
    externs_digest, sync_unit_dir, validate_externs, AnalysisRequest, ExternInput, SnapshotExtern,
    SnapshotIdentity, SnapshotStore,
};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock, TryLockError, Weak};
use std::time::{Duration, Instant};

/// Short form of a build-context identity, used in directory names and GC reports.
fn context_short(context_id: &str) -> String {
    context_id[..12.min(context_id.len())].to_owned()
}

/// Handle to a daemon. Cloning a handle shares the same daemon state, which is
/// what the integration tests use to drive concurrent requests.
#[derive(Clone)]
pub struct Daemon {
    inner: Arc<Inner>,
}

/// RAII counter of in-flight analyses (used by the `idle_only` auto-GC decision).
struct ActiveAnalysisGuard<'a> {
    counter: &'a AtomicUsize,
}

impl<'a> ActiveAnalysisGuard<'a> {
    fn new(counter: &'a AtomicUsize) -> Self {
        counter.fetch_add(1, Ordering::SeqCst);
        Self { counter }
    }
}

impl Drop for ActiveAnalysisGuard<'_> {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::SeqCst);
    }
}

struct Inner {
    config: DaemonConfig,
    toolchain: ToolchainInfo,
    driver: RustcDriver,
    snapshots: SnapshotStore,
    cache: ResultCache,
    revisions: RevisionTracker,
    running: RunningJobs,
    gate: JobGate,
    unit_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    stats: Mutex<DaemonStats>,
    /// Shared (analyses) / exclusive (GC) lock over the on-disk state. An
    /// analysis holds it in read mode for its whole duration; GC takes it in
    /// write mode, so collection never races a snapshot/verdict/unit write.
    io_lock: RwLock<()>,
    /// Recently served analyses (in memory + persisted hints). Used by GC to
    /// protect live content, including from a one-shot process after shutdown.
    retention: Mutex<RetentionState>,
    /// Opt-in automatic background GC runtime (maintenance thread). `None` when
    /// disabled, so the default configuration has no thread and no background cost.
    auto_gc: Option<Arc<AutoGcRuntime>>,
    /// Analyses currently in flight (used by `idle_only` decision).
    active_analyses: AtomicUsize,
}

impl Daemon {
    /// Opens a daemon over `config.work_dir`, creating its private layout and
    /// querying the toolchain identity once.
    pub fn open(config: DaemonConfig) -> Result<Self> {
        let work_dir = config.work_dir.clone();
        for sub in [
            "snapshots",
            "cache",
            "incremental",
            "units",
            "out",
            "artifacts",
        ] {
            std::fs::create_dir_all(work_dir.join(sub))?;
        }
        let toolchain = match config.toolchain_override.clone() {
            Some(toolchain) => toolchain,
            None => ToolchainInfo::query(&config.rustc)?,
        };
        let snapshots = SnapshotStore::new(work_dir.join("snapshots"))?;
        let cache = ResultCache::new(work_dir.join("cache"), config.max_cache_entries)?;
        let gate = JobGate::new(config.max_concurrent_jobs);
        let driver = RustcDriver::new(config.rustc.clone());
        let retention = RetentionState::load(&work_dir);
        // Fail-closed: an invalid automatic GC policy prevents opening the daemon
        // instead of silently running without the expected retention guarantees.
        config.auto_gc.validate()?;
        let auto_gc = if config.auto_gc.enabled {
            Some(AutoGcRuntime::new(config.auto_gc.clone(), &work_dir)?)
        } else {
            None
        };
        let inner = Arc::new(Inner {
            config,
            toolchain,
            driver,
            snapshots,
            cache,
            revisions: RevisionTracker::default(),
            running: RunningJobs::new(),
            gate,
            unit_locks: Mutex::new(HashMap::new()),
            stats: Mutex::new(DaemonStats::default()),
            io_lock: RwLock::new(()),
            retention: Mutex::new(retention),
            auto_gc: auto_gc.clone(),
            active_analyses: AtomicUsize::new(0),
        });
        if let Some(runtime) = auto_gc {
            // The thread only holds a `Weak` daemon handle: when every `Daemon`
            // handle is dropped, the upgrade fails and the thread exits, so it
            // never keeps the daemon (or its files) alive on its own.
            let weak: Weak<Inner> = Arc::downgrade(&inner);
            std::thread::Builder::new()
                .name("ravum-daemon-auto-gc".to_owned())
                .spawn(move || auto_gc_maintenance(weak, runtime))
                .map_err(|error| {
                    DaemonError::internal(format!(
                        "cannot start automatic GC maintenance thread: {error}"
                    ))
                })?;
        }
        Ok(Self { inner })
    }

    pub fn toolchain(&self) -> &ToolchainInfo {
        &self.inner.toolchain
    }

    pub fn work_dir(&self) -> &Path {
        &self.inner.config.work_dir
    }

    pub fn config(&self) -> &DaemonConfig {
        &self.inner.config
    }

    pub fn current_revision(&self) -> u64 {
        self.inner.revisions.current()
    }

    /// Advances the current revision and cancels every in-flight analysis
    /// strictly older than it. This is what makes "a new edit supersedes the
    /// previous answer" real instead of cosmetic.
    pub fn advance_revision(&self, revision: u64) -> u64 {
        let previous = self.inner.revisions.note(revision);
        let current = self.inner.revisions.current();
        if current > previous {
            self.inner.running.cancel_below(current);
        }
        current
    }

    pub fn stats(&self) -> DaemonStats {
        let mut stats = self
            .inner
            .stats
            .lock()
            .map(|stats| stats.clone())
            .unwrap_or_default();
        stats.peak_active_jobs = stats.peak_active_jobs.max(self.inner.gate.peak() as u64);
        if let Some(runtime) = &self.inner.auto_gc {
            stats.auto_gc = runtime.snapshot();
        }
        stats
    }

    /// Drops both cache levels. Operational reset, also used by tests that need
    /// a guaranteed cache miss.
    pub fn clear_caches(&self) -> Result<()> {
        self.inner.cache.clear()
    }

    /// Runs one **explicit** bounded GC pass over the daemon's private state.
    ///
    /// Must be explicitly requested: nothing is ever collected automatically
    /// during an analysis. The pass holds the exclusive I/O lock, so no analysis
    /// can write a snapshot, verdict or compile unit while objects are removed.
    /// Live content (recent verdicts, last served content, last used build
    /// context, snapshots referenced by kept verdicts) is never deleted; a byte
    /// cap that cannot be met while protecting it is reported as `met: false`.
    pub fn gc(&self, policy: &GcPolicy) -> Result<GcReport> {
        policy.validate()?;
        let started = Instant::now();
        let mut report = self.gc_locked(policy, GcMode::Daemon)?;
        report.duration_ms = started.elapsed().as_millis() as u64;
        let freed = report.freed_bytes;
        let entries = report.deleted.entry_count() as u64;
        self.bump(|stats| {
            stats.gc_runs = stats.gc_runs.saturating_add(1);
            stats.gc_freed_bytes = stats.gc_freed_bytes.saturating_add(freed);
            stats.gc_deleted_entries = stats.gc_deleted_entries.saturating_add(entries);
        });
        Ok(report)
    }

    /// One bounded GC pass under the exclusive I/O lock, shared by the manual GC
    /// and the automatic maintenance thread. Does not update manual GC counters:
    /// the caller decides which counters belong to its path.
    fn gc_locked(&self, policy: &GcPolicy, mode: GcMode) -> Result<GcReport> {
        let started = Instant::now();
        // Same lock as analyses, exclusive side: all in-flight analyses complete
        // before any file is collected, and new ones wait until GC is done.
        let _io_guard = self
            .inner
            .io_lock
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let retention = self
            .inner
            .retention
            .lock()
            .map(|state| state.clone())
            .unwrap_or_default();
        let protection = GcProtection::from_retention(&retention, self.inner.revisions.current());
        let mut report = crate::gc::run_gc(&self.inner.config.work_dir, policy, &protection, mode)?;
        if !policy.dry_run {
            let evicted = self.inner.cache.remove_keys(&report.deleted.cache_keys);
            if evicted > 0 {
                report.notes.push(format!(
                    "{evicted} in-memory cache entr{} evicted after collection",
                    if evicted == 1 { "y" } else { "ies" }
                ));
            }
            if let Ok(mut retention) = self.inner.retention.lock() {
                for cache_key in &report.deleted.cache_keys {
                    retention.forget_cache_key(cache_key);
                }
                let _ = retention.save(&self.inner.config.work_dir);
            }
        }
        report.duration_ms = started.elapsed().as_millis() as u64;
        Ok(report)
    }

    // ------------------------------------------------------------------
    // Automatic background GC (P0-H suite)
    // ------------------------------------------------------------------

    /// Current automatic GC counters and last attempt (disabled daemons report
    /// `enabled: false` and zeros).
    pub fn auto_gc_stats(&self) -> AutoGcStats {
        self.inner
            .auto_gc
            .as_ref()
            .map(|runtime| runtime.snapshot())
            .unwrap_or_default()
    }

    /// Effective automatic GC policy of this daemon.
    pub fn auto_gc_policy(&self) -> AutoGcPolicy {
        self.inner
            .auto_gc
            .as_ref()
            .map(|runtime| runtime.policy().clone())
            .unwrap_or_default()
    }

    /// JSONL journal receiving one line per automatic GC attempt (bounded).
    pub fn auto_gc_journal_path(&self) -> Option<PathBuf> {
        self.inner
            .auto_gc
            .as_ref()
            .map(|runtime| runtime.journal_path().to_path_buf())
    }

    /// Waits until no automatic check is pending or running. Returns `false` on
    /// timeout. Tests and maintenance tooling use this to observe a settled state;
    /// analyses never need it (the whole point is that it runs in background).
    pub fn wait_auto_gc_idle(&self, timeout_ms: u64) -> bool {
        let Some(runtime) = &self.inner.auto_gc else {
            return true;
        };
        let deadline = Instant::now() + Duration::from_millis(timeout_ms.max(1));
        loop {
            if !runtime.is_busy_or_pending() {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            if let Ok(guard) = runtime.signal_lock.lock() {
                let _ = runtime.signal.wait_timeout(guard, Duration::from_millis(5));
            } else {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    /// Stops the maintenance thread on its next wake-up (at most ~200 ms).
    /// Does not wait: callers that need a settled state use `wait_auto_gc_idle`.
    pub fn shutdown_auto_gc(&self) {
        if let Some(runtime) = &self.inner.auto_gc {
            runtime.request_shutdown();
        }
    }

    /// Test seam: the next automatic attempt fails with this message instead of
    /// touching the filesystem. Proves that an auto-GC failure is recorded and
    /// never fails the next analysis. Passing `None` clears the injection.
    pub fn inject_auto_gc_failure(&self, message: Option<String>) {
        if let Some(runtime) = &self.inner.auto_gc {
            runtime.inject_failure(message);
        }
    }

    fn run_auto_gc_once(&self, runtime: &Arc<AutoGcRuntime>) -> std::result::Result<(), String> {
        runtime.set_busy(true);
        let result = self.auto_gc_step(runtime);
        runtime.set_busy(false);
        runtime.signal.notify_all();
        result
    }

    /// One threshold evaluation + (optional) collection. Runs only in the
    /// maintenance thread; never on the analysis path, never under a read lock.
    fn auto_gc_step(&self, runtime: &Arc<AutoGcRuntime>) -> std::result::Result<(), String> {
        let requests_analyzed = self.stats().requests;
        if let Some(message) = runtime.take_injected_failure() {
            runtime.record_failure(message.clone(), 0, requests_analyzed);
            return Err(format!("injected auto-GC failure: {message}"));
        }
        let total_before = crate::gc::measure_total_bytes(&self.inner.config.work_dir)
            .map_err(|error| format!("auto-GC size measurement failed: {error}"))?;
        let active = self.inner.active_analyses.load(Ordering::SeqCst);
        match runtime.decision(total_before, active) {
            AutoGcDecision::BelowThreshold { .. } => Ok(()),
            AutoGcDecision::Busy { .. } => {
                // `idle_only`: retry after the next completed analysis.
                runtime.reschedule_retry();
                Ok(())
            }
            AutoGcDecision::Run { .. } => {
                let policy = runtime.policy().gc_policy();
                match self.gc_locked(&policy, GcMode::Auto) {
                    Ok(report) => {
                        runtime.record_success(&report, total_before, requests_analyzed);
                        Ok(())
                    }
                    Err(error) => {
                        let message = format!("{error}");
                        runtime.record_failure(message.clone(), total_before, requests_analyzed);
                        Err(message)
                    }
                }
            }
        }
    }

    /// Snapshot of the persisted recency hints (newest served analyses). Exposed
    /// so maintenance tooling and tests can inspect what GC will protect.
    pub fn retention_state(&self) -> RetentionState {
        self.inner
            .retention
            .lock()
            .map(|state| state.clone())
            .unwrap_or_default()
    }

    /// Records one served analysis as live content for retention/GC. Persisted
    /// best-effort: a failed write only degrades one-shot protection to
    /// timestamps, it never invalidates the analysis result.
    fn record_served(&self, revision: u64, cache_key: &str, snapshot_id: &str, context_id: &str) {
        let entry = RecentAnalysis {
            revision,
            cache_key: cache_key.to_owned(),
            snapshot_id: snapshot_id.to_owned(),
            context_short: context_short(context_id),
            created_at_unix: crate::cache::now_unix(),
        };
        if let Ok(mut state) = self.inner.retention.lock() {
            state.record(entry);
            let _ = state.save(&self.inner.config.work_dir);
        }
    }

    pub fn cache_entries(&self) -> (usize, usize) {
        (self.inner.cache.entries(), self.inner.cache.disk_entries())
    }

    /// Exact compiler command template this daemon would use for `request`
    /// (paths included). Evidence files quote it verbatim; nothing is executed.
    pub fn command_template(&self, request: &AnalysisRequest) -> Result<String> {
        request.validate()?;
        let context_id = request.context.id(&self.inner.toolchain);
        let files = request.normalized_files()?;
        let unit = unit_dir(&self.inner.config.work_dir, &context_id);
        let root = safe_join(&unit, &request.root)?;
        let out = out_dir(&self.inner.config.work_dir, &context_id);
        let incremental = incremental_dir(&self.inner.config.work_dir, &context_id);
        let options = DriverOptions {
            root_path: &root,
            out_dir: &out,
            incremental_dir: &incremental,
            context: &request.context,
            poll_interval_ms: self.inner.config.poll_interval_ms,
            timeout_ms: self.inner.config.analysis_timeout_ms,
        };
        let template = self.inner.driver.command_template(&options);
        // Sanity: `files` is used only to prove the request is well-formed.
        let _ = files;
        Ok(template)
    }

    /// Analyses one request: the public entry point.
    ///
    /// Automatic-GC bookkeeping happens **after** the answer is computed: one
    /// atomic increment per request (and a condition-variable signal once every
    /// `check_every_requests`); the threshold scan and collection run in the
    /// maintenance thread, never inside this call.
    pub fn analyze(&self, request: &AnalysisRequest) -> Result<AnalysisReport> {
        self.analyze_with_externs(request, &[])
    }

    /// Analyses one request with content-addressed direct externs (MVP-0c L1.2).
    ///
    /// The extern bytes are validated fail-closed, mixed into the snapshot identity
    /// (and into the cache key via the externs digest), stored content-addressed in
    /// the immutable snapshot, and passed to rustc as `--extern NAME=<.rmeta>`. An
    /// empty list is byte-for-byte the legacy [`Daemon::analyze`] path.
    pub fn analyze_with_externs(
        &self,
        request: &AnalysisRequest,
        externs: &[ExternInput],
    ) -> Result<AnalysisReport> {
        let result = {
            // Count the in-flight analysis only for its duration, so `idle_only`
            // sees a genuinely idle daemon after the answer is produced.
            let _active = ActiveAnalysisGuard::new(&self.inner.active_analyses);
            self.analyze_impl(request, externs)
        };
        if let Some(runtime) = &self.inner.auto_gc {
            let _ = runtime.note_request();
        }
        result
    }

    /// Analyses one request (inner implementation, see [`Daemon::analyze`]).
    fn analyze_impl(
        &self,
        request: &AnalysisRequest,
        externs: &[ExternInput],
    ) -> Result<AnalysisReport> {
        let started = Instant::now();
        // Shared side of the retention lock: analyses run concurrently with each
        // other but never with an explicit GC pass.
        let _io_guard = self
            .inner
            .io_lock
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        request.validate()?;
        validate_externs(externs)?;
        self.bump(|stats| stats.requests += 1);
        let context_id = request.context.id(&self.inner.toolchain);

        // Monotonic revision bookkeeping. An older revision is rejected before
        // any hashing, snapshot write or process spawn.
        let previous = self.inner.revisions.note(request.revision);
        if request.revision < previous {
            let current = self.inner.revisions.current();
            let mut report = self.stale_report(
                request,
                &context_id,
                ReportStatus::Superseded,
                format!(
                    "revision {} is older than the daemon's current revision {current}",
                    request.revision
                ),
            );
            report.timings.total_ms = started.elapsed().as_millis() as u64;
            self.bump(|stats| stats.superseded += 1);
            return Ok(report);
        }
        if request.revision > previous {
            // A newer request supersedes older in-flight ones.
            self.inner.running.cancel_below(request.revision);
        }

        // Register as in-flight so a later revision can cancel this job.
        let running = self.inner.running.register(request.revision);
        let token = running.token();
        let accept_started = Instant::now();
        let identity = request.identity_with_externs(&self.inner.toolchain, externs)?;
        // The zero-extern path keeps the legacy cache key and verdict digest
        // exactly (no behavioural change for existing clients/tests).
        let externs_hash = if identity.externs.is_empty() {
            String::new()
        } else {
            externs_digest(&identity.externs)
        };
        let mut accept_ms = accept_started.elapsed().as_millis() as u64;
        let key = CacheLookupKey::new_with_externs(
            &identity.snapshot_id,
            &identity.context_id,
            &identity.toolchain_id,
            &externs_hash,
        );

        // L1/L2 cache lookup.
        let lookup_started = Instant::now();
        let lookup = self.inner.cache.lookup(&key);
        let lookup_ms = lookup_started.elapsed().as_millis() as u64;
        match lookup {
            LookupOutcome::Hit { verdict, kind } => {
                self.bump(|stats| match kind {
                    CacheKind::Memory => stats.cache_hits_memory += 1,
                    CacheKind::Disk => stats.cache_hits_disk += 1,
                    CacheKind::Miss => stats.cache_misses += 1,
                });
                if self.inner.revisions.is_superseded(request.revision) {
                    let report = self.finish_stale(
                        request,
                        &identity,
                        &context_id,
                        ReportStatus::Superseded,
                        format!(
                            "revision {} superseded by {} while reading the cache",
                            request.revision,
                            self.inner.revisions.current()
                        ),
                        Timings {
                            accept_ms,
                            queue_ms: 0,
                            lookup_ms,
                            analysis_ms: 0,
                            total_ms: started.elapsed().as_millis() as u64,
                        },
                        CacheReport::hit(kind, self.inner.cache.entries()),
                    );
                    self.bump(|stats| stats.superseded += 1);
                    return Ok(report);
                }
                let cached_artifact = verdict.artifact_sha256.clone();
                let mut report = self.fresh_report_from_cache(
                    request,
                    &identity,
                    &context_id,
                    *verdict,
                    kind,
                    Timings {
                        accept_ms,
                        queue_ms: 0,
                        lookup_ms,
                        analysis_ms: 0,
                        total_ms: started.elapsed().as_millis() as u64,
                    },
                );
                report.artifact_sha256 = cached_artifact;
                self.bump(|stats| stats.fresh += 1);
                self.record_served(
                    request.revision,
                    &key.key,
                    &identity.snapshot_id,
                    &context_id,
                );
                return Ok(report);
            }
            LookupOutcome::Miss => {
                self.bump(|stats| stats.cache_misses += 1);
            }
            LookupOutcome::Corrupt { reason } => {
                // Fail-closed: a corrupt entry is a miss, never a served answer.
                self.bump(|stats| {
                    stats.cache_corrupt += 1;
                    stats.cache_misses += 1;
                });
                let _ = reason;
            }
        }

        // Bounded admission: at most `max_concurrent_jobs` compilers run at once.
        let queue_started = Instant::now();
        let slot = self.inner.gate.acquire(request.priority);
        let queue_ms = queue_started.elapsed().as_millis() as u64;
        if self.is_superseded(request) || token.is_cancelled() {
            let report = self.finish_stale(
                request,
                &identity,
                &context_id,
                ReportStatus::Superseded,
                format!(
                    "revision {} superseded before compilation",
                    request.revision
                ),
                Timings {
                    accept_ms,
                    queue_ms,
                    lookup_ms,
                    analysis_ms: 0,
                    total_ms: started.elapsed().as_millis() as u64,
                },
                CacheReport::miss(self.inner.cache.entries()),
            );
            drop(slot);
            self.bump(|stats| stats.superseded += 1);
            return Ok(report);
        }

        // Serialize only the same build context: `rustc` incremental state is
        // not shareable between concurrent invocations.
        let unit_lock = self.unit_lock(&context_id);
        let unit_guard = match self.lock_unit(&unit_lock, request, &token) {
            UnitLockOutcome::Locked(guard) => guard,
            UnitLockOutcome::Superseded(report) => {
                drop(slot);
                self.bump(|stats| stats.superseded += 1);
                return Ok(report);
            }
        };

        // Immutable snapshot + stable compile unit.
        let files = request.normalized_files()?;
        let materialized = if externs.is_empty() {
            self.inner.snapshots.materialize(&identity, &files)?
        } else {
            self.inner
                .snapshots
                .materialize_with_externs(&identity, &files, externs)?
        };
        if materialized.created {
            self.bump(|stats| stats.snapshots_created += 1);
        } else {
            self.bump(|stats| stats.snapshots_verified += 1);
        }
        let unit = unit_dir(&self.inner.config.work_dir, &context_id);
        let sync = sync_unit_dir(&unit, &files)?;
        accept_ms = accept_ms.saturating_add(accept_started.elapsed().as_millis() as u64);
        self.bump(|stats| stats.unit_files_written += sync.written_files as u64);
        if self.is_superseded(request) || token.is_cancelled() {
            let report = self.finish_stale(
                request,
                &identity,
                &context_id,
                ReportStatus::Superseded,
                format!(
                    "revision {} superseded before the compiler started",
                    request.revision
                ),
                Timings {
                    accept_ms,
                    queue_ms,
                    lookup_ms,
                    analysis_ms: 0,
                    total_ms: started.elapsed().as_millis() as u64,
                },
                CacheReport::miss(self.inner.cache.entries()),
            );
            self.bump(|stats| stats.superseded += 1);
            return Ok(report);
        }

        let root_path = safe_join(&unit, &request.root)?;
        let context_out_dir = out_dir(&self.inner.config.work_dir, &context_id);
        let context_incremental_dir = incremental_dir(&self.inner.config.work_dir, &context_id);
        let options = DriverOptions {
            root_path: &root_path,
            out_dir: &context_out_dir,
            incremental_dir: &context_incremental_dir,
            context: &request.context,
            poll_interval_ms: self.inner.config.poll_interval_ms,
            timeout_ms: self.inner.config.analysis_timeout_ms,
        };
        // rustc requires the `--extern` artifact to be named `lib<name>.rmeta`
        // (a bare `<sha>.rmeta` is rejected as "unknown type"). Direct artifacts
        // come from the immutable snapshot; the transitive interface closure comes
        // from the daemon's content-addressed artifact store, so a dependent can
        // decode re-exports of its externs.
        let mut compiled_externs: Vec<CompiledExtern> = Vec::new();
        if !materialized.externs.is_empty() {
            let closure = self.extern_closure(&materialized.externs)?;
            let extern_links = self
                .inner
                .config
                .work_dir
                .join("externs")
                .join(&identity.snapshot_id);
            std::fs::create_dir_all(&extern_links).map_err(|error| {
                DaemonError::io(format!(
                    "cannot create extern link dir {}: {error}",
                    extern_links.display()
                ))
            })?;
            let mut by_name: std::collections::BTreeMap<String, String> =
                std::collections::BTreeMap::new();
            for extern_ref in &closure {
                if let Some(existing) = by_name.get(&extern_ref.name) {
                    if existing != &extern_ref.interface_id {
                        return Err(DaemonError::invalid(format!(
                            "ambiguous extern name {:?}: two interfaces in the closure",
                            extern_ref.name
                        )));
                    }
                    continue;
                }
                by_name.insert(extern_ref.name.clone(), extern_ref.interface_id.clone());
                let bytes = if let Some(direct) = materialized
                    .externs
                    .iter()
                    .find(|direct| direct.interface_id == extern_ref.interface_id)
                {
                    std::fs::read(
                        materialized
                            .externs_dir
                            .join(format!("{}.rmeta", direct.interface_id)),
                    )
                    .map_err(|error| {
                        DaemonError::io(format!(
                            "cannot read verified extern {}: {error}",
                            extern_ref.name
                        ))
                    })?
                } else {
                    self.artifact(&extern_ref.interface_id)?
                };
                let link = extern_links.join(format!("lib{}.rmeta", extern_ref.name));
                if std::fs::read(&link).ok().as_deref() != Some(bytes.as_slice()) {
                    std::fs::write(&link, &bytes).map_err(|error| {
                        DaemonError::io(format!(
                            "cannot materialize extern {} at {}: {error}",
                            extern_ref.name,
                            link.display()
                        ))
                    })?;
                }
                compiled_externs.push(CompiledExtern {
                    name: extern_ref.name.clone(),
                    rmeta: link,
                });
            }
        }
        let output = if compiled_externs.is_empty() {
            self.inner.driver.run(&options, &token)?
        } else {
            self.inner
                .driver
                .run_with_externs(&options, &compiled_externs, &token)?
        };
        self.bump(|stats| {
            stats.rustc_runs += 1;
            stats.rustc_ms_total += output.analysis_ms;
        });

        if output.cancelled {
            self.bump(|stats| {
                stats.cancelled += 1;
                stats.superseded += 1;
            });
            let report = self.finish_stale(
                request,
                &identity,
                &context_id,
                ReportStatus::Cancelled,
                format!(
                    "compiler process killed: revision {} superseded by {}",
                    request.revision,
                    self.inner.revisions.current()
                ),
                Timings {
                    accept_ms,
                    queue_ms,
                    lookup_ms,
                    analysis_ms: output.analysis_ms,
                    total_ms: started.elapsed().as_millis() as u64,
                },
                CacheReport::miss(self.inner.cache.entries()),
            );
            return Ok(report);
        }

        if output.timed_out {
            self.bump(|stats| stats.timed_out += 1);
            let current = self.current_revision();
            let mut report = self.stale_report(
                request,
                &context_id,
                ReportStatus::TimedOut,
                format!(
                    "compiler exceeded {} ms and was killed",
                    self.inner.config.analysis_timeout_ms
                ),
            );
            report.revision = request.revision;
            report.current_revision = current;
            report.snapshot_id = identity.snapshot_id.clone();
            report.incremental_dir = context_incremental_dir.display().to_string();
            report.timings = Timings {
                accept_ms,
                queue_ms,
                lookup_ms,
                analysis_ms: output.analysis_ms,
                total_ms: started.elapsed().as_millis() as u64,
            };
            report.snapshot = Some(SnapshotReport {
                files: identity.files.len(),
                bytes: identity.total_bytes,
                written_files: sync.written_files,
                removed_files: sync.removed_files,
                verified_files: materialized.verified_files,
                externs: identity.externs.len(),
                dir: materialized.dir.display().to_string(),
            });
            return Ok(report);
        }

        let parsed = parse_rustc_json(&output.stderr, Some(&unit), output.success);
        let success = output.success && !parsed.has_errors();
        let artifact_sha256 = if success {
            self.store_artifact(
                &context_out_dir,
                &request.context.crate_name,
                &identity.externs,
            )
        } else {
            None
        };
        let verdict = CachedVerdict::new(
            key.key.clone(),
            identity.snapshot_id.clone(),
            identity.context_id.clone(),
            identity.toolchain_id.clone(),
            success,
            output.exit_code,
            parsed.diagnostics.clone(),
            output.analysis_ms,
        )
        .with_externs_digest(externs_hash.clone())
        .with_artifact_sha256(artifact_sha256.clone());
        // Cache the verdict by content identity even if this revision was
        // superseded meanwhile: the content-addressed answer stays valid for any
        // caller that submits the same bytes.
        if let Err(error) = self.inner.cache.insert(&verdict) {
            let _ = error;
        }
        drop(unit_guard);
        drop(slot);

        if self.inner.revisions.is_superseded(request.revision) {
            let report = self.finish_stale(
                request,
                &identity,
                &context_id,
                ReportStatus::Superseded,
                format!(
                    "revision {} superseded by {} while compiling",
                    request.revision,
                    self.inner.revisions.current()
                ),
                Timings {
                    accept_ms,
                    queue_ms,
                    lookup_ms,
                    analysis_ms: output.analysis_ms,
                    total_ms: started.elapsed().as_millis() as u64,
                },
                CacheReport::miss(self.inner.cache.entries()),
            );
            self.bump(|stats| stats.superseded += 1);
            return Ok(report);
        }

        let mut report = self.fresh_report_base(request, &identity, &context_id);
        report.success = success;
        report.exit_code = output.exit_code;
        report.diagnostics = parsed.diagnostics;
        report.artifact_sha256 = artifact_sha256;
        report.cache = CacheReport::miss(self.inner.cache.entries());
        report.timings = Timings {
            accept_ms,
            queue_ms,
            lookup_ms,
            analysis_ms: output.analysis_ms,
            total_ms: started.elapsed().as_millis() as u64,
        };
        report.snapshot = Some(SnapshotReport {
            files: identity.files.len(),
            bytes: identity.total_bytes,
            written_files: sync.written_files,
            removed_files: sync.removed_files,
            verified_files: materialized.verified_files,
            externs: identity.externs.len(),
            dir: materialized.dir.display().to_string(),
        });
        self.bump(|stats| stats.fresh += 1);
        self.record_served(
            request.revision,
            &key.key,
            &identity.snapshot_id,
            &context_id,
        );
        Ok(report)
    }

    /// Reads one content-addressed interface artifact (`<work>/artifacts/<sha>.rmeta`)
    /// produced by a previous successful analysis. The bytes are SHA-256 verified
    /// before being returned: a tampered or foreign artifact is an error, never a
    /// dependency interface.
    pub fn artifact(&self, interface_id: &str) -> Result<Vec<u8>> {
        crate::config::validate_interface_id(interface_id)?;
        let path = self
            .inner
            .config
            .work_dir
            .join("artifacts")
            .join(format!("{interface_id}.rmeta"));
        let bytes = std::fs::read(&path).map_err(|error| {
            DaemonError::io(format!(
                "artifact {interface_id} is unavailable at {}: {error}",
                path.display()
            ))
        })?;
        if bytes.is_empty() || bytes.len() > crate::config::MAX_EXTERN_BYTES {
            return Err(DaemonError::snapshot_corrupt(format!(
                "artifact {interface_id} has invalid size {}",
                bytes.len()
            )));
        }
        if crate::hash::sha256_hex(&bytes) != interface_id {
            return Err(DaemonError::snapshot_corrupt(format!(
                "artifact {interface_id} does not match its recorded SHA-256"
            )));
        }
        Ok(bytes)
    }

    /// Direct externs recorded for one interface artifact: a dependent analysis
    /// needs the interface closure, not just its immediate externs.
    fn record_artifact_externs(&self, interface_id: &str, externs: &[SnapshotExtern]) {
        if externs.is_empty() {
            return;
        }
        let artifacts = self.inner.config.work_dir.join("artifacts");
        let sidecar = artifacts.join(format!("{interface_id}.externs.json"));
        if sidecar.is_file() {
            return;
        }
        if let Ok(bytes) = serde_json::to_vec(externs) {
            let temporary = artifacts.join(format!(".tmp-{interface_id}.externs.json"));
            if std::fs::write(&temporary, bytes).is_ok() {
                let _ = std::fs::rename(&temporary, &sidecar);
            }
        }
    }

    fn artifact_externs(&self, interface_id: &str) -> Vec<SnapshotExtern> {
        let sidecar = self
            .inner
            .config
            .work_dir
            .join("artifacts")
            .join(format!("{interface_id}.externs.json"));
        std::fs::read(&sidecar)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Transitive interface closure of the direct externs: rustc needs the
    /// dependency metadata of an extern's own dependencies to decode re-exports
    /// (e.g. `api` re-exporting `core::Point`). Missing/ambiguous entries are a
    /// real error, never an invented interface.
    fn extern_closure(&self, direct: &[SnapshotExtern]) -> Result<Vec<SnapshotExtern>> {
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut queue: Vec<SnapshotExtern> = direct.to_vec();
        let mut closure: Vec<SnapshotExtern> = Vec::new();
        while let Some(extern_ref) = queue.pop() {
            if !seen.insert(extern_ref.interface_id.clone()) {
                continue;
            }
            let nested = self.artifact_externs(&extern_ref.interface_id);
            queue.extend(nested);
            closure.push(extern_ref);
        }
        closure.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(closure)
    }

    /// Stores the `.rmeta` interface produced by one successful analysis in the
    /// daemon's content-addressed artifact store and returns its SHA-256.
    fn store_artifact(
        &self,
        context_out_dir: &std::path::Path,
        crate_name: &str,
        direct_externs: &[SnapshotExtern],
    ) -> Option<String> {
        let produced = context_out_dir.join(format!("lib{crate_name}.rmeta"));
        let bytes = std::fs::read(&produced).ok()?;
        if bytes.is_empty() || bytes.len() > crate::config::MAX_EXTERN_BYTES {
            return None;
        }
        let sha256 = crate::hash::sha256_hex(&bytes);
        let artifacts = self.inner.config.work_dir.join("artifacts");
        std::fs::create_dir_all(&artifacts).ok()?;
        let final_path = artifacts.join(format!("{sha256}.rmeta"));
        if final_path.is_file() {
            self.record_artifact_externs(&sha256, direct_externs);
            return Some(sha256);
        }
        let temporary = artifacts.join(format!(".tmp-{sha256}-{}", std::process::id()));
        std::fs::write(&temporary, &bytes).ok()?;
        match std::fs::rename(&temporary, &final_path) {
            Ok(()) => {
                self.record_artifact_externs(&sha256, direct_externs);
                Some(sha256)
            }
            Err(_) => {
                let _ = std::fs::remove_file(&temporary);
                if final_path.is_file() {
                    self.record_artifact_externs(&sha256, direct_externs);
                    Some(sha256)
                } else {
                    None
                }
            }
        }
    }
    fn is_superseded(&self, request: &AnalysisRequest) -> bool {
        self.inner.revisions.is_superseded(request.revision)
    }

    fn unit_lock(&self, context_id: &str) -> Arc<Mutex<()>> {
        let mut locks = match self.inner.unit_locks.lock() {
            Ok(locks) => locks,
            Err(poisoned) => poisoned.into_inner(),
        };
        locks
            .entry(context_id.to_owned())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// Locks the per-context unit directory, aborting early when the revision is
    /// superseded. Polling instead of a blocking lock keeps supersession cheap.
    fn lock_unit<'a>(
        &self,
        lock: &'a Arc<Mutex<()>>,
        request: &AnalysisRequest,
        token: &CancelToken,
    ) -> UnitLockOutcome<'a> {
        let poll = Duration::from_millis(self.inner.config.poll_interval_ms.max(1));
        loop {
            match lock.try_lock() {
                Ok(guard) => return UnitLockOutcome::Locked(guard),
                Err(TryLockError::WouldBlock) => {
                    if self.is_superseded(request) || token.is_cancelled() {
                        let current = self.inner.revisions.current();
                        let context_id = request.context.id(&self.inner.toolchain);
                        let report = self.stale_report(
                            request,
                            &context_id,
                            ReportStatus::Superseded,
                            format!(
                                "revision {} superseded by {current} while queued for its \
                                 build-context lock",
                                request.revision
                            ),
                        );
                        return UnitLockOutcome::Superseded(report);
                    }
                    std::thread::sleep(poll);
                }
                Err(TryLockError::Poisoned(poisoned)) => {
                    return UnitLockOutcome::Locked(poisoned.into_inner())
                }
            }
        }
    }

    fn fresh_report_base(
        &self,
        request: &AnalysisRequest,
        identity: &SnapshotIdentity,
        context_id: &str,
    ) -> AnalysisReport {
        AnalysisReport {
            schema_version: crate::config::DAEMON_SCHEMA_VERSION,
            status: ReportStatus::Fresh,
            revision: request.revision,
            current_revision: self.inner.revisions.current(),
            unit: request.label.clone(),
            root: identity.root.clone(),
            snapshot_id: identity.snapshot_id.clone(),
            context_id: context_id.to_owned(),
            toolchain: self.inner.toolchain.clone(),
            incremental_dir: incremental_dir(&self.inner.config.work_dir, context_id)
                .display()
                .to_string(),
            cache: CacheReport::miss(self.inner.cache.entries()),
            success: false,
            exit_code: None,
            diagnostics: Vec::new(),
            timings: Timings::default(),
            snapshot: None,
            externs: identity
                .externs
                .iter()
                .map(|extern_ref| ExternUsageReport {
                    name: extern_ref.name.clone(),
                    crate_name: extern_ref.crate_name.clone(),
                    interface_id: extern_ref.interface_id.clone(),
                    rmeta_sha256: extern_ref.interface_id.clone(),
                })
                .collect(),
            artifact_sha256: None,
            daemon_version: env!("CARGO_PKG_VERSION").to_owned(),
            message: None,
        }
    }

    fn fresh_report_from_cache(
        &self,
        request: &AnalysisRequest,
        identity: &SnapshotIdentity,
        context_id: &str,
        verdict: CachedVerdict,
        kind: CacheKind,
        timings: Timings,
    ) -> AnalysisReport {
        let diagnostics = verdict.diagnostics;
        let error_count = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity.is_error())
            .count();
        let success = verdict.success && error_count == 0;
        let mut report = self.fresh_report_base(request, identity, context_id);
        report.success = success;
        report.exit_code = verdict.exit_code;
        report.diagnostics = diagnostics;
        report.cache = CacheReport::hit(kind, self.inner.cache.entries());
        report.timings = timings;
        report
    }

    fn stale_report(
        &self,
        request: &AnalysisRequest,
        context_id: &str,
        status: ReportStatus,
        message: String,
    ) -> AnalysisReport {
        let mut report = self.fresh_report_base(
            request,
            &SnapshotIdentity {
                snapshot_id: String::new(),
                context_id: context_id.to_owned(),
                toolchain_id: self.inner.toolchain.id(),
                root: request.root.clone(),
                files: Vec::new(),
                total_bytes: 0,
                externs: Vec::new(),
            },
            context_id,
        );
        report.status = status;
        report.current_revision = self.inner.revisions.current();
        report.success = false;
        report.diagnostics.clear();
        report.cache = CacheReport::miss(self.inner.cache.entries());
        report.message = Some(message);
        report
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_stale(
        &self,
        request: &AnalysisRequest,
        identity: &SnapshotIdentity,
        context_id: &str,
        status: ReportStatus,
        message: String,
        timings: Timings,
        cache: CacheReport,
    ) -> AnalysisReport {
        let mut report = self.fresh_report_base(request, identity, context_id);
        report.status = status;
        report.current_revision = self.inner.revisions.current();
        report.success = false;
        report.diagnostics.clear();
        report.timings = timings;
        report.cache = cache;
        report.message = Some(message);
        report
    }

    fn bump(&self, update: impl FnOnce(&mut DaemonStats)) {
        if let Ok(mut stats) = self.inner.stats.lock() {
            update(&mut stats);
        }
    }
}

enum UnitLockOutcome<'a> {
    Locked(std::sync::MutexGuard<'a, ()>),
    Superseded(AnalysisReport),
}

/// Convenience constructor for tests and tools: a request with one root file.
pub fn single_file_request(
    revision: u64,
    label: impl Into<String>,
    path: impl Into<String>,
    content: impl Into<String>,
    context: BuildContext,
) -> AnalysisRequest {
    let path = path.into();
    AnalysisRequest {
        revision,
        label: label.into(),
        root: path.clone(),
        files: vec![crate::snapshot::SourceFile {
            path,
            content: content.into(),
        }],
        context,
        priority: Priority::Normal,
    }
}

/// Default private work directory below `base`.
pub fn work_dir_below(base: impl AsRef<Path>) -> PathBuf {
    base.as_ref().join("ravum-daemon")
}

/// Maintenance thread of the automatic background GC.
///
/// Holds only a `Weak` daemon handle: when the last `Daemon` handle is dropped,
/// `upgrade` fails and the thread exits, so it never keeps the daemon alive on
/// its own. It waits on a condition variable with a bounded timeout, evaluates
/// the threshold at most once per check interval, and runs the collection through
/// the same exclusive I/O lock as the manual GC.
fn auto_gc_maintenance(weak: Weak<Inner>, runtime: Arc<AutoGcRuntime>) {
    let poll = Duration::from_millis(200);
    loop {
        if runtime.is_shutdown() {
            break;
        }
        let _ = runtime.wait_for_work(poll);
        if runtime.is_shutdown() {
            break;
        }
        let Some(inner) = weak.upgrade() else {
            break;
        };
        let daemon = Daemon { inner };
        if runtime.take_due() {
            // A failure is recorded in stats/journal by `run_auto_gc_once`; the
            // maintenance loop deliberately keeps running and no analysis fails.
            let _ = daemon.run_auto_gc_once(&runtime);
        }
        drop(daemon);
    }
}
