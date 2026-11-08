//! Bounded retention / garbage collection for the persistent analysis daemon.
//!
//! The daemon accumulates three kinds of on-disk state:
//!
//! * **verdict cache** (`<work>/cache/*.json`) — content-addressed results;
//! * **immutable snapshots** (`<work>/snapshots/<sha256>/`) — proof of analysed bytes;
//! * **incremental build state** (`<work>/incremental|out|units/<context>/`) —
//!   persistent `-C incremental` state, compiler output and the stable compile unit.
//!
//! Nothing here runs automatically: GC is *explicitly requested* (server
//! `{"cmd":"gc","policy":{...}}` or one-shot `ravum-daemon --gc ...`). A GC never
//! deletes content that is still being served: the most recent `keep_recent`
//! verdicts, the last analysed content, the last used build context and every
//! snapshot referenced by a kept verdict are protected. When protecting live
//! content conflicts with a byte cap, the cap is reported as **not met** instead
//! of silently deleting live data.
//!
//! Safety: deletions are two-phase (rename to a `.gc-trash-*` name, then remove)
//! and, through [`crate::daemon::Daemon::gc`], execute under the same exclusive
//! lock that every analysis takes in shared mode — no analysis can be writing a
//! cache entry or snapshot while it is collected.

use crate::cache::{now_unix, CachedVerdict};
use crate::config::DAEMON_SCHEMA_VERSION;
use crate::error::{DaemonError, Result};
use crate::fsutil::atomic_write;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Sub-directory names inside a daemon work directory. GC only looks below these.
pub const CACHE_DIR: &str = "cache";
pub const SNAPSHOTS_DIR: &str = "snapshots";
pub const INCREMENTAL_DIR: &str = "incremental";
pub const OUT_DIR: &str = "out";
pub const UNITS_DIR: &str = "units";
/// Persisted recency hints (last analyses) so a one-shot GC run after a daemon
/// shutdown can still protect what was recently served.
pub const RETENTION_FILE: &str = "retention.json";

/// Maximum number of recent analyses remembered/persisted.
pub const MAX_RECENT_ANALYSES: usize = 64;
/// Default `keep_recent` when a policy omits it.
pub const DEFAULT_KEEP_RECENT: usize = 8;
/// Safety bound for recursive directory scans (metadata-only; no file content).
const MAX_SCAN_ENTRIES: usize = 200_000;
/// Maximum number of error strings kept in a report.
const MAX_REPORT_ERRORS: usize = 32;
/// Maximum number of protected-object reasons kept in a report.
const MAX_REASONS: usize = 64;

static TRASH_SEQ: AtomicU64 = AtomicU64::new(0);

/// Explicit retention policy, serializable for the JSON-lines protocol.
/// All caps are optional; a policy with no cap is a legal no-op (it only
/// *protects*, it never deletes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GcPolicy {
    /// Byte cap for the verdict cache directory.
    #[serde(default)]
    pub max_cache_bytes: Option<u64>,
    /// Byte cap for the immutable snapshot store.
    #[serde(default)]
    pub max_snapshot_bytes: Option<u64>,
    /// Byte cap for the incremental state (incremental + out + units per context).
    #[serde(default)]
    pub max_incremental_bytes: Option<u64>,
    /// Global cap for cache + snapshots + incremental state.
    #[serde(default)]
    pub max_total_bytes: Option<u64>,
    /// Number of most recent served contents that are never deleted.
    #[serde(default = "default_keep_recent")]
    pub keep_recent: usize,
    /// Compute the plan and report it, but delete nothing.
    #[serde(default)]
    pub dry_run: bool,
}

fn default_keep_recent() -> usize {
    DEFAULT_KEEP_RECENT
}

impl Default for GcPolicy {
    fn default() -> Self {
        Self {
            max_cache_bytes: None,
            max_snapshot_bytes: None,
            max_incremental_bytes: None,
            max_total_bytes: None,
            keep_recent: DEFAULT_KEEP_RECENT,
            dry_run: false,
        }
    }
}

impl GcPolicy {
    /// Rejects policies that could silently delete live data or are meaningless:
    /// `keep_recent = 0` would let GC delete the content currently being served;
    /// a cap of `0` cannot be satisfied while protecting anything.
    pub fn validate(&self) -> Result<()> {
        if self.keep_recent == 0 {
            return Err(DaemonError::invalid(
                "gc policy: keep_recent must be >= 1 (0 would allow deleting the current content)",
            ));
        }
        for (name, cap) in [
            ("max_cache_bytes", self.max_cache_bytes),
            ("max_snapshot_bytes", self.max_snapshot_bytes),
            ("max_incremental_bytes", self.max_incremental_bytes),
            ("max_total_bytes", self.max_total_bytes),
        ] {
            if cap == Some(0) {
                return Err(DaemonError::invalid(format!(
                    "gc policy: {name} must be >= 1 when set"
                )));
            }
        }
        Ok(())
    }

    /// True when at least one byte cap is configured (otherwise GC deletes nothing).
    pub fn has_byte_budget(&self) -> bool {
        self.max_cache_bytes.is_some()
            || self.max_snapshot_bytes.is_some()
            || self.max_incremental_bytes.is_some()
            || self.max_total_bytes.is_some()
    }
}

/// Where the GC ran: inside a live daemon (holding its analysis lock) or as a
/// one-shot maintenance process (no daemon running on the same directory).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GcMode {
    Daemon,
    OneShot,
}

impl GcMode {
    pub fn as_str(self) -> &'static str {
        match self {
            GcMode::Daemon => "daemon",
            GcMode::OneShot => "one_shot",
        }
    }
}

/// One recently served analysis. `revision` and `snapshot_id` are what a reader
/// needs to verify that GC kept the current content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentAnalysis {
    pub revision: u64,
    pub cache_key: String,
    pub snapshot_id: String,
    pub context_short: String,
    pub created_at_unix: u64,
}

/// Persisted recency hints. Bounded and tolerant on read: a corrupt file is
/// ignored (GC then falls back to on-disk timestamps, reported as such).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionState {
    #[serde(default = "retention_schema_version")]
    pub schema_version: u32,
    /// Oldest first; newest at the back.
    #[serde(default)]
    pub recent: Vec<RecentAnalysis>,
    #[serde(default)]
    pub last_cache_key: Option<String>,
    #[serde(default)]
    pub last_snapshot_id: Option<String>,
    #[serde(default)]
    pub last_context_short: Option<String>,
    #[serde(default)]
    pub last_revision: u64,
    /// Set when a persisted file could not be read (informational only).
    #[serde(skip)]
    pub load_error: Option<String>,
}

fn retention_schema_version() -> u32 {
    1
}

impl Default for RetentionState {
    fn default() -> Self {
        Self {
            schema_version: 1,
            recent: Vec::new(),
            last_cache_key: None,
            last_snapshot_id: None,
            last_context_short: None,
            last_revision: 0,
            load_error: None,
        }
    }
}

impl RetentionState {
    /// Reads `<work>/retention.json`, tolerating absence and corruption.
    pub fn load(work_dir: &Path) -> Self {
        let path = work_dir.join(RETENTION_FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                let mut state = RetentionState::default();
                if error.kind() != std::io::ErrorKind::NotFound {
                    state.load_error = Some(format!("cannot read {}: {error}", path.display()));
                }
                return state;
            }
        };
        match serde_json::from_str::<RetentionState>(&text) {
            Ok(mut state) => {
                state.recent.truncate(MAX_RECENT_ANALYSES);
                state.load_error = None;
                state
            }
            Err(error) => {
                let mut state = RetentionState::default();
                state.load_error = Some(format!(
                    "retention hints ignored: {} is not valid JSON: {error}",
                    path.display()
                ));
                state
            }
        }
    }

    /// Atomically persists the hints. GC never needs this file to be
    /// authoritative: a failure only degrades protection to timestamps.
    pub fn save(&self, work_dir: &Path) -> Result<()> {
        let path = work_dir.join(RETENTION_FILE);
        let text = serde_json::to_vec_pretty(self)
            .map_err(|e| DaemonError::internal(format!("cannot serialize retention state: {e}")))?;
        atomic_write(&path, &text)
    }

    /// Records one served analysis (deduplicated by cache key, newest last).
    pub fn record(&mut self, entry: RecentAnalysis) {
        self.recent
            .retain(|existing| existing.cache_key != entry.cache_key);
        self.recent.push(entry.clone());
        if self.recent.len() > MAX_RECENT_ANALYSES {
            let excess = self.recent.len() - MAX_RECENT_ANALYSES;
            self.recent.drain(0..excess);
        }
        self.last_cache_key = Some(entry.cache_key);
        self.last_snapshot_id = Some(entry.snapshot_id);
        self.last_context_short = Some(entry.context_short);
        self.last_revision = self.last_revision.max(entry.revision);
    }

    /// Drops a collected cache key (kept coherent after GC).
    pub fn forget_cache_key(&mut self, cache_key: &str) {
        self.recent
            .retain(|entry| entry.cache_key != cache_key);
        if self.last_cache_key.as_deref() == Some(cache_key) {
            self.last_cache_key = None;
        }
    }

    /// Newest-first prefix of at most `count` recent analyses.
    pub fn recent_newest_first(&self, count: usize) -> Vec<RecentAnalysis> {
        self.recent.iter().rev().take(count).cloned().collect()
    }
}

/// What GC must never delete. Filled from the live daemon (memory + persisted
/// hints) or from the persisted hints alone in one-shot mode.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GcProtection {
    pub current_revision: u64,
    /// Most recent served analyses, newest first.
    pub recent: Vec<RecentAnalysis>,
    pub last_cache_key: Option<String>,
    pub last_snapshot_id: Option<String>,
    pub last_context_short: Option<String>,
}

impl GcProtection {
    pub fn from_retention(state: &RetentionState, current_revision: u64) -> Self {
        Self {
            current_revision: current_revision.max(state.last_revision),
            recent: state.recent_newest_first(MAX_RECENT_ANALYSES),
            last_cache_key: state.last_cache_key.clone(),
            last_snapshot_id: state.last_snapshot_id.clone(),
            last_context_short: state.last_context_short.clone(),
        }
    }

    fn recent_position(&self, cache_key: &str) -> Option<usize> {
        self.recent
            .iter()
            .position(|entry| entry.cache_key == cache_key)
    }

    /// Newest-first prefix used in reports ("ids de révisions conservées").
    pub fn recent_prefix(&self, count: usize) -> Vec<RecentAnalysis> {
        self.recent.iter().take(count.max(1)).cloned().collect()
    }
}

/// Sizes and entry counts of the three collected categories.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CategorySizes {
    pub cache_bytes: u64,
    pub cache_entries: usize,
    pub snapshots_bytes: u64,
    pub snapshots: usize,
    pub incremental_bytes: u64,
    pub incremental_contexts: usize,
    pub total_bytes: u64,
}

impl CategorySizes {
    fn recompute_total(&mut self) {
        self.total_bytes = self.cache_bytes + self.snapshots_bytes + self.incremental_bytes;
    }
}

/// Filesystem objects planned or removed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GcDeleted {
    pub cache_keys: Vec<String>,
    pub snapshots: Vec<String>,
    pub contexts: Vec<String>,
    pub files: u64,
    pub dirs: u64,
}

impl GcDeleted {
    /// Number of objects (cache entries + snapshots + build contexts) listed.
    pub fn entry_count(&self) -> usize {
        self.cache_keys.len() + self.snapshots.len() + self.contexts.len()
    }
}
/// Objects kept, with the reason they were protected.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GcKept {
    pub cache_keys: Vec<String>,
    pub snapshots: Vec<String>,
    pub contexts: Vec<String>,
    /// Newest-first served analyses that were protected.
    pub recent: Vec<RecentAnalysis>,
    /// `id -> reason` for protected objects (bounded).
    pub reasons: BTreeMap<String, String>,
}

/// One budget's status: a cap that cannot be met while protecting live content is
/// reported as `met: false`, never hidden.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetStatus {
    pub cap_bytes: Option<u64>,
    pub before_bytes: u64,
    /// Size after the plan (projection; equals `before` in `dry_run`).
    pub after_bytes: u64,
    pub met: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GcBudgets {
    pub cache: BudgetStatus,
    pub snapshots: BudgetStatus,
    pub incremental: BudgetStatus,
    pub total: BudgetStatus,
}

/// JSON report of one GC run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GcReport {
    pub schema_version: u32,
    pub kind: String,
    pub mode: GcMode,
    pub dry_run: bool,
    pub policy: GcPolicy,
    pub started_unix: u64,
    pub duration_ms: u64,
    /// Sizes observed before the plan.
    pub before: CategorySizes,
    /// Sizes actually left on disk after the run (`dry_run`: == before).
    pub after: CategorySizes,
    /// Byte size of the planned victims (== `freed_bytes` on a real run).
    pub planned_freed_bytes: u64,
    /// Bytes really reclaimed (`0` in `dry_run`, reduced by removal failures).
    pub freed_bytes: u64,
    /// Objects actually removed (`dry_run`: empty).
    pub deleted: GcDeleted,
    /// Objects the policy would remove (`dry_run`: the plan; real: same as
    /// `deleted` unless a removal failed).
    pub planned: GcDeleted,
    pub kept: GcKept,
    pub budgets: GcBudgets,
    pub protected: GcProtection,
    pub errors: Vec<String>,
    pub notes: Vec<String>,
}

// ---------------------------------------------------------------------------
// Internal inventory
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct CacheItem {
    key: String,
    path: PathBuf,
    bytes: u64,
    created_at_unix: u64,
    snapshot_id: Option<String>,
    corrupt: bool,
    protected: bool,
    planned: bool,
    deleted: bool,
    reason: Option<&'static str>,
}

#[derive(Debug, Clone)]
struct SnapshotItem {
    id: String,
    path: PathBuf,
    bytes: u64,
    files: u64,
    recency_unix: u64,
    protected: bool,
    planned: bool,
    deleted: bool,
    reason: Option<&'static str>,
}

#[derive(Debug, Clone)]
struct ContextItem {
    short: String,
    incremental: PathBuf,
    out: PathBuf,
    units: PathBuf,
    bytes: u64,
    files: u64,
    dir_count: usize,
    last_modified_unix: u64,
    protected: bool,
    planned: bool,
    deleted: bool,
    reason: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VictimKind {
    Cache(usize),
    Snapshot(usize),
    Context(usize),
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Runs one explicit GC pass over `work_dir`. This function takes no daemon lock:
/// [`crate::daemon::Daemon::gc`] wraps it under the analysis write lock; the
/// one-shot binary calls it directly (documented as "no live daemon on the same
/// work directory").
pub fn run_gc(
    work_dir: &Path,
    policy: &GcPolicy,
    protection: &GcProtection,
    mode: GcMode,
) -> Result<GcReport> {
    policy.validate()?;
    let started = Instant::now();
    let started_unix = now_unix();
    let mut errors: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();

    if !work_dir.exists() {
        notes.push(format!(
            "work directory {} does not exist: nothing to collect",
            work_dir.display()
        ));
    }
    if !policy.has_byte_budget() {
        notes.push(
            "policy has no byte cap: GC only protects live content and deletes nothing".to_owned(),
        );
    }

    let mut cache = inventory_cache(work_dir, &mut errors);
    let mut snapshots = inventory_snapshots(work_dir, &mut errors);
    let mut contexts = inventory_contexts(work_dir, &mut errors);
    let before = sizes(&cache, &snapshots, &contexts);

    protect(&mut cache, &mut snapshots, &mut contexts, protection, policy.keep_recent);

    // --- Selection (oldest unprotected first, per category then globally) ---
    let cache_order = cache_newest_order(&cache, protection);
    let snapshot_order = snapshot_newest_order(&snapshots, &cache);
    let context_order = context_newest_order(&contexts);

    let mut remaining_cache = before.cache_bytes;
    if let Some(cap) = policy.max_cache_bytes {
        for index in cache_order.iter().rev() {
            if remaining_cache <= cap {
                break;
            }
            if cache[*index].protected || cache[*index].planned || cache[*index].deleted {
                continue;
            }
            let bytes = cache[*index].bytes;
            cache[*index].planned = true;
            remaining_cache -= bytes;
        }
    }
    let mut remaining_snapshots = before.snapshots_bytes;
    if let Some(cap) = policy.max_snapshot_bytes {
        for index in snapshot_order.iter().rev() {
            if remaining_snapshots <= cap {
                break;
            }
            if snapshots[*index].protected || snapshots[*index].planned || snapshots[*index].deleted {
                continue;
            }
            let bytes = snapshots[*index].bytes;
            snapshots[*index].planned = true;
            remaining_snapshots -= bytes;
        }
    }
    let mut remaining_incremental = before.incremental_bytes;
    if let Some(cap) = policy.max_incremental_bytes {
        for index in context_order.iter().rev() {
            if remaining_incremental <= cap {
                break;
            }
            if contexts[*index].protected || contexts[*index].planned || contexts[*index].deleted {
                continue;
            }
            let bytes = contexts[*index].bytes;
            contexts[*index].planned = true;
            remaining_incremental -= bytes;
        }
    }

    // Global cap: pick the oldest unprotected object across categories.
    let mut remaining_total = before.total_bytes;
    if let Some(cap) = policy.max_total_bytes {
        loop {
            if remaining_total <= cap {
                break;
            }
            let candidate = oldest_overall_candidate(
                &cache,
                &snapshots,
                &contexts,
                &cache_order,
                &snapshot_order,
                &context_order,
            );
            let Some((kind, time, bytes)) = candidate else {
                break;
            };
            match kind {
                VictimKind::Cache(index) => cache[index].planned = true,
                VictimKind::Snapshot(index) => snapshots[index].planned = true,
                VictimKind::Context(index) => contexts[index].planned = true,
            }
            let _ = time;
            remaining_total -= bytes;
        }
    }

    // --- Apply ---
    let mut planned = GcDeleted::default();
    for item in cache.iter().filter(|item| item.planned) {
        planned.cache_keys.push(item.key.clone());
        planned.files += 1;
    }
    for item in snapshots.iter().filter(|item| item.planned) {
        planned.snapshots.push(item.id.clone());
        planned.files += item.files;
        planned.dirs += 1;
    }
    for item in contexts.iter().filter(|item| item.planned) {
        planned.contexts.push(item.short.clone());
        planned.files += item.files;
        planned.dirs += item.dir_count as u64;
    }

    let mut deleted = GcDeleted::default();
    let mut freed_bytes: u64 = 0;
    if !policy.dry_run {
        for index in 0..cache.len() {
            if !cache[index].planned || cache[index].deleted {
                continue;
            }
            match delete_path_safely(&cache[index].path) {
                Ok(()) => {
                    cache[index].deleted = true;
                    freed_bytes += cache[index].bytes;
                    deleted.cache_keys.push(cache[index].key.clone());
                    deleted.files += 1;
                }
                Err(error) => push_error(&mut errors, error),
            }
        }
        for index in 0..snapshots.len() {
            if !snapshots[index].planned || snapshots[index].deleted {
                continue;
            }
            match delete_path_safely(&snapshots[index].path) {
                Ok(()) => {
                    snapshots[index].deleted = true;
                    freed_bytes += snapshots[index].bytes;
                    deleted.snapshots.push(snapshots[index].id.clone());
                    deleted.files += snapshots[index].files;
                    deleted.dirs += 1;
                }
                Err(error) => push_error(&mut errors, error),
            }
        }
        for index in 0..contexts.len() {
            if !contexts[index].planned || contexts[index].deleted {
                continue;
            }
            let mut okay = true;
            for path in [
                contexts[index].incremental.clone(),
                contexts[index].out.clone(),
                contexts[index].units.clone(),
            ] {
                if !path.exists() {
                    continue;
                }
                if let Err(error) = delete_path_safely(&path) {
                    push_error(&mut errors, error);
                    okay = false;
                }
            }
            if okay {
                contexts[index].deleted = true;
                freed_bytes += contexts[index].bytes;
                deleted.contexts.push(contexts[index].short.clone());
                deleted.files += contexts[index].files;
                deleted.dirs += contexts[index].dir_count as u64;
            }
        }
    }

    let after = if policy.dry_run {
        before
    } else {
        sizes(&cache, &snapshots, &contexts)
    };
    let planned_freed_bytes = planned_bytes(&cache, &snapshots, &contexts);


    let budgets = GcBudgets {
        cache: category_budget(
            policy.max_cache_bytes,
            before.cache_bytes,
            remaining_cache,
        ),
        snapshots: category_budget(
            policy.max_snapshot_bytes,
            before.snapshots_bytes,
            remaining_snapshots,
        ),
        incremental: category_budget(
            policy.max_incremental_bytes,
            before.incremental_bytes,
            remaining_incremental,
        ),
        total: category_budget(policy.max_total_bytes, before.total_bytes, remaining_total),
    };

    let kept = GcKept {
        cache_keys: cache
            .iter()
            .filter(|item| !item.deleted)
            .take(64)
            .map(|item| item.key.clone())
            .collect(),
        snapshots: snapshots
            .iter()
            .filter(|item| !item.deleted)
            .take(64)
            .map(|item| item.id.clone())
            .collect(),
        contexts: contexts
            .iter()
            .filter(|item| !item.deleted)
            .map(|item| item.short.clone())
            .collect(),
        recent: protection.recent_prefix(policy.keep_recent),
        reasons: collect_reasons(&cache, &snapshots, &contexts),
    };

    for item in &cache {
        if item.corrupt {
            notes.push(format!(
                "cache entry {} is unparsable and was treated as the oldest candidate",
                item.key
            ));
        }
    }
    if let Some(load_error) = &protection_load_error(protection) {
        notes.push(load_error.clone());
    }
    for item in &cache {
        if item.planned && !item.deleted {
            notes.push(format!(
                "planned cache entry {} was not removed (see errors)",
                item.key
            ));
        }
    }

    Ok(GcReport {
        schema_version: DAEMON_SCHEMA_VERSION,
        kind: "ravum-daemon-gc-report".to_owned(),
        mode,
        dry_run: policy.dry_run,
        policy: policy.clone(),
        started_unix,
        duration_ms: started.elapsed().as_millis() as u64,
        before,
        after,
        planned_freed_bytes,
        freed_bytes,
        deleted,
        planned,
        kept,
        budgets,
        protected: protection.clone(),
        errors,
        notes: bounded(notes, 32),
    })
}

fn protection_load_error(_protection: &GcProtection) -> Option<String> {
    None
}

fn category_budget(cap: Option<u64>, before_bytes: u64, projected_after: u64) -> BudgetStatus {
    BudgetStatus {
        cap_bytes: cap,
        before_bytes,
        after_bytes: if cap.is_some() {
            projected_after
        } else {
            before_bytes
        },
        met: cap.map(|cap| projected_after <= cap).unwrap_or(true),
    }
}

fn planned_bytes(cache: &[CacheItem], snapshots: &[SnapshotItem], contexts: &[ContextItem]) -> u64 {
    cache
        .iter()
        .filter(|item| item.planned || item.deleted)
        .map(|item| item.bytes)
        .sum::<u64>()
        + snapshots
            .iter()
            .filter(|item| item.planned || item.deleted)
            .map(|item| item.bytes)
            .sum::<u64>()
        + contexts
            .iter()
            .filter(|item| item.planned || item.deleted)
            .map(|item| item.bytes)
            .sum::<u64>()
}

fn sizes(cache: &[CacheItem], snapshots: &[SnapshotItem], contexts: &[ContextItem]) -> CategorySizes {
    let mut sizes = CategorySizes {
        cache_bytes: cache
            .iter()
            .filter(|item| !item.deleted)
            .map(|item| item.bytes)
            .sum(),
        cache_entries: cache.iter().filter(|item| !item.deleted).count(),
        snapshots_bytes: snapshots
            .iter()
            .filter(|item| !item.deleted)
            .map(|item| item.bytes)
            .sum(),
        snapshots: snapshots.iter().filter(|item| !item.deleted).count(),
        incremental_bytes: contexts
            .iter()
            .filter(|item| !item.deleted)
            .map(|item| item.bytes)
            .sum(),
        incremental_contexts: contexts.iter().filter(|item| !item.deleted).count(),
        total_bytes: 0,
    };
    sizes.recompute_total();
    sizes
}

// ---------------------------------------------------------------------------
// Inventory
// ---------------------------------------------------------------------------

fn inventory_cache(work_dir: &Path, errors: &mut Vec<String>) -> Vec<CacheItem> {
    let dir = work_dir.join(CACHE_DIR);
    let mut items = Vec::new();
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => return items,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            continue;
        }
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if name.starts_with('.') {
            continue;
        }
        if path.extension().map(|ext| ext == "json") != Some(true) {
            continue;
        }
        let key = name.trim_end_matches(".json").to_owned();
        let bytes = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
        let mut created_at_unix = 0;
        let mut snapshot_id = None;
        let mut corrupt = false;
        match std::fs::read(&path) {
            Ok(raw) => match serde_json::from_slice::<CachedVerdict>(&raw) {
                Ok(verdict) => {
                    created_at_unix = verdict.created_at_unix;
                    snapshot_id = Some(verdict.snapshot_id);
                }
                Err(_) => corrupt = true,
            },
            Err(error) => {
                corrupt = true;
                push_error(
                    errors,
                    format!("cannot read cache entry {}: {error}", path.display()),
                );
            }
        }
        items.push(CacheItem {
            key,
            path,
            bytes,
            created_at_unix,
            snapshot_id,
            corrupt,
            protected: false,
            planned: false,
            deleted: false,
            reason: None,
        });
    }
    items.sort_by(|a, b| a.key.cmp(&b.key));
    items
}

fn inventory_snapshots(work_dir: &Path, errors: &mut Vec<String>) -> Vec<SnapshotItem> {
    let dir = work_dir.join(SNAPSHOTS_DIR);
    let mut items = Vec::new();
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => return items,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let id = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if id.starts_with('.') || id.len() != 64 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
            push_error(
                errors,
                format!("ignored unexpected snapshot entry {}", path.display()),
            );
            continue;
        }
        let (bytes, files, recency_unix, truncated) = tree_stats(&path);
        if truncated {
            push_error(
                errors,
                format!("snapshot {} scan truncated at {MAX_SCAN_ENTRIES} entries", id),
            );
        }
        items.push(SnapshotItem {
            id,
            path,
            bytes,
            files,
            recency_unix,
            protected: false,
            planned: false,
            deleted: false,
            reason: None,
        });
    }
    items.sort_by(|a, b| a.id.cmp(&b.id));
    items
}

fn inventory_contexts(work_dir: &Path, errors: &mut Vec<String>) -> Vec<ContextItem> {
    let roots = [
        work_dir.join(INCREMENTAL_DIR),
        work_dir.join(OUT_DIR),
        work_dir.join(UNITS_DIR),
    ];
    let mut names = std::collections::BTreeSet::new();
    for root in &roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if name.starts_with('.') {
                continue;
            }
            if name.len() != 12 || !name.chars().all(|c| c.is_ascii_hexdigit()) {
                continue;
            }
            names.insert(name);
        }
    }
    let mut items = Vec::new();
    for short in names {
        let incremental = work_dir.join(INCREMENTAL_DIR).join(&short);
        let out = work_dir.join(OUT_DIR).join(&short);
        let units = work_dir.join(UNITS_DIR).join(&short);
        let mut bytes = 0u64;
        let mut files = 0u64;
        let mut last_modified_unix = 0u64;
        let mut dir_count = 0usize;
        for path in [&incremental, &out, &units] {
            if !path.exists() {
                continue;
            }
            dir_count += 1;
            let (sub_bytes, sub_files, mtime, truncated) = tree_stats(path);
            bytes += sub_bytes;
            files += sub_files;
            last_modified_unix = last_modified_unix.max(mtime);
            if truncated {
                push_error(
                    errors,
                    format!("context {} scan truncated at {MAX_SCAN_ENTRIES} entries", short),
                );
            }
        }
        if last_modified_unix == 0 {
            last_modified_unix = now_unix();
        }
        items.push(ContextItem {
            short,
            incremental,
            out,
            units,
            bytes,
            files,
            dir_count,
            last_modified_unix,
            protected: false,
            planned: false,
            deleted: false,
            reason: None,
        });
    }
    items.sort_by(|a, b| a.short.cmp(&b.short));
    items
}

/// Metadata-only recursive scan: `(bytes, files, mtime_unix, truncated)`.
fn tree_stats(path: &Path) -> (u64, u64, u64, bool) {
    let mut bytes = 0u64;
    let mut files = 0u64;
    let mut mtime = 0u64;
    let mut seen = 0usize;
    let mut truncated = false;
    let mut stack = vec![path.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > MAX_SCAN_ENTRIES {
                truncated = true;
                break;
            }
            let entry_path = entry.path();
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            if metadata.is_dir() {
                stack.push(entry_path);
            } else {
                bytes = bytes.saturating_add(metadata.len());
                files = files.saturating_add(1);
                mtime = mtime.max(modified_unix(&metadata));
            }
        }
        if truncated {
            break;
        }
    }
    (bytes, files, mtime, truncated)
}

fn modified_unix(metadata: &std::fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Protection
// ---------------------------------------------------------------------------

fn cache_order_key(item: &CacheItem, protection: &GcProtection) -> (u8, u64, String) {
    if let Some(position) = protection.recent_position(&item.key) {
        return (0, position as u64, item.key.clone());
    }
    (
        1,
        u64::MAX.saturating_sub(item.created_at_unix),
        item.key.clone(),
    )
}

fn cache_newest_order(cache: &[CacheItem], protection: &GcProtection) -> Vec<usize> {
    let mut order: Vec<usize> = (0..cache.len()).collect();
    order.sort_by(|a, b| {
        cache_order_key(&cache[*a], protection).cmp(&cache_order_key(&cache[*b], protection))
    });
    order
}

fn snapshot_newest_order(snapshots: &[SnapshotItem], cache: &[CacheItem]) -> Vec<usize> {
    // A snapshot referenced by a verdict inherits the verdict's recency.
    let mut referenced: BTreeMap<String, u64> = BTreeMap::new();
    for item in cache {
        if let Some(snapshot_id) = &item.snapshot_id {
            let entry = referenced.entry(snapshot_id.clone()).or_insert(item.created_at_unix);
            *entry = (*entry).max(item.created_at_unix);
        }
    }
    let mut order: Vec<usize> = (0..snapshots.len()).collect();
    order.sort_by(|a, b| {
        let item_a = &snapshots[*a];
        let item_b = &snapshots[*b];
        let key = |item: &SnapshotItem| -> (u8, u64, String) {
            match referenced.get(&item.id) {
                Some(created) => (0, u64::MAX.saturating_sub(*created), item.id.clone()),
                None => (
                    1,
                    u64::MAX.saturating_sub(item.recency_unix),
                    item.id.clone(),
                ),
            }
        };
        key(item_a).cmp(&key(item_b))
    });
    order
}

fn context_newest_order(contexts: &[ContextItem]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..contexts.len()).collect();
    order.sort_by(|a, b| {
        let item_a = &contexts[*a];
        let item_b = &contexts[*b];
        item_b
            .last_modified_unix
            .cmp(&item_a.last_modified_unix)
            .then_with(|| item_a.short.cmp(&item_b.short))
    });
    order
}

fn protect(
    cache: &mut [CacheItem],
    snapshots: &mut [SnapshotItem],
    contexts: &mut [ContextItem],
    protection: &GcProtection,
    keep_recent: usize,
) {
    // 1. Cache: newest `keep_recent` plus the last served key.
    let order = cache_newest_order(cache, protection);
    for (rank, index) in order.iter().enumerate() {
        let item = &mut cache[*index];
        let forced = protection.last_cache_key.as_deref() == Some(item.key.as_str());
        if rank < keep_recent || forced {
            item.protected = true;
            item.reason = Some(if forced && rank >= keep_recent {
                "last_cache_key"
            } else {
                "keep_recent"
            });
        }
    }
    // 2. Snapshots are live content only when a protected verdict references
    //    them (or they are the last analysed snapshot). An orphan snapshot is a
    //    collection candidate even when its directory timestamp is recent.
    let protected_snapshot_ids: std::collections::BTreeSet<String> = cache
        .iter()
        .filter(|item| item.protected && !item.deleted)
        .filter_map(|item| item.snapshot_id.clone())
        .collect();
    for item in snapshots.iter_mut() {
        let is_referenced = protected_snapshot_ids.contains(&item.id);
        let is_last = protection.last_snapshot_id.as_deref() == Some(item.id.as_str());
        if is_referenced || is_last {
            item.protected = true;
            item.reason = Some(if is_last {
                "last_snapshot_id"
            } else {
                "referenced_by_kept_verdict"
            });
        }
    }    // 3. Build contexts: the last used one is never deleted. Only when no
    //    recency hint exists (cold one-shot GC) is the most recently modified
    //    context protected as a fallback, so multiple stale contexts remain
    //    collectable in the normal case.
    let order = context_newest_order(contexts);
    let last_used_known = protection.last_context_short.is_some();
    for (rank, index) in order.iter().enumerate() {
        let item = &mut contexts[*index];
        let last_used = protection.last_context_short.as_deref() == Some(item.short.as_str());
        if last_used {
            item.protected = true;
            item.reason = Some("last_used_context");
        } else if rank == 0 && !last_used_known {
            item.protected = true;
            item.reason = Some("most_recent_context");
        }
    }
}


// ---------------------------------------------------------------------------
// Selection helpers
// ---------------------------------------------------------------------------

fn oldest_overall_candidate(
    cache: &[CacheItem],
    snapshots: &[SnapshotItem],
    contexts: &[ContextItem],
    cache_order: &[usize],
    snapshot_order: &[usize],
    context_order: &[usize],
) -> Option<(VictimKind, u64, u64)> {
    let mut best: Option<(VictimKind, u64, u64)> = None;
    if let Some(index) = cache_order
        .iter()
        .rev()
        .find(|index| !cache[**index].protected && !cache[**index].planned && !cache[**index].deleted)
    {
        best = Some((VictimKind::Cache(*index), cache[*index].created_at_unix, cache[*index].bytes));
    }
    if let Some(index) = snapshot_order.iter().rev().find(|index| {
        !snapshots[**index].protected && !snapshots[**index].planned && !snapshots[**index].deleted
    }) {
        let candidate = (
            VictimKind::Snapshot(*index),
            snapshots[*index].recency_unix,
            snapshots[*index].bytes,
        );
        if best.map(|(_, time, _)| candidate.1 < time).unwrap_or(true) {
            best = Some(candidate);
        }
    }
    if let Some(index) = context_order.iter().rev().find(|index| {
        !contexts[**index].protected && !contexts[**index].planned && !contexts[**index].deleted
    }) {
        let candidate = (
            VictimKind::Context(*index),
            contexts[*index].last_modified_unix,
            contexts[*index].bytes,
        );
        if best.map(|(_, time, _)| candidate.1 < time).unwrap_or(true) {
            best = Some(candidate);
        }
    }
    best
}

fn collect_reasons(
    cache: &[CacheItem],
    snapshots: &[SnapshotItem],
    contexts: &[ContextItem],
) -> BTreeMap<String, String> {
    let mut reasons = BTreeMap::new();
    for item in cache.iter().filter(|item| item.protected).take(MAX_REASONS) {
        reasons.insert(
            format!("cache:{}", item.key),
            item.reason.unwrap_or("protected").to_owned(),
        );
    }
    for item in snapshots
        .iter()
        .filter(|item| item.protected)
        .take(MAX_REASONS)
    {
        reasons.insert(
            format!("snapshot:{}", item.id),
            item.reason.unwrap_or("protected").to_owned(),
        );
    }
    for item in contexts
        .iter()
        .filter(|item| item.protected)
        .take(MAX_REASONS)
    {
        reasons.insert(
            format!("context:{}", item.short),
            item.reason.unwrap_or("protected").to_owned(),
        );
    }
    reasons
}

fn push_error(errors: &mut Vec<String>, error: String) {
    if errors.len() < MAX_REPORT_ERRORS {
        errors.push(error);
    }
}

fn bounded(mut values: Vec<String>, limit: usize) -> Vec<String> {
    values.truncate(limit);
    values
}

// ---------------------------------------------------------------------------
// Deletion
// ---------------------------------------------------------------------------

/// Two-phase safe removal: rename to a unique `.gc-trash-*` sibling, then delete
/// the temp name. A reader that scanned the directory before the rename cannot
/// see a half-deleted object at the original path.
fn delete_path_safely(path: &Path) -> std::result::Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let parent = match path.parent() {
        Some(parent) => parent,
        None => return Err(format!("cannot remove {}: no parent directory", path.display())),
    };
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "object".to_owned());
    let seq = TRASH_SEQ.fetch_add(1, Ordering::Relaxed);
    let trash = parent.join(format!(".gc-trash-{name}-{}-{seq}", std::process::id()));
    std::fs::rename(path, &trash).map_err(|error| {
        format!(
            "cannot rename {} for removal: {error}",
            path.display()
        )
    })?;
    let is_dir = std::fs::metadata(&trash).map(|meta| meta.is_dir()).unwrap_or(false);
    let result = if is_dir {
        std::fs::remove_dir_all(&trash).map_err(|error| {
            format!("cannot remove directory {}: {error}", trash.display())
        })
    } else {
        std::fs::remove_file(&trash).map_err(|error| {
            format!("cannot remove file {}: {error}", trash.display())
        })
    };
    if result.is_err() {
        // Best effort: put the object back rather than leaving an anonymous
        // trash name that the inventory ignores.
        let _ = std::fs::rename(&trash, path);
    }
    result
}

/// Time helper for syntactically consistent reports (re-exported through cache).
pub fn current_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex_id(seed: u8) -> String {
        format!("{seed:02x}").repeat(32)
    }

    fn context_short(seed: u8) -> String {
        format!("{seed:02x}").repeat(6)
    }

    fn write_cache_entry(work: &Path, key: &str, snapshot_id: &str, created_at: u64) -> u64 {
        let dir = work.join(CACHE_DIR);
        std::fs::create_dir_all(&dir).unwrap();
        let verdict = serde_json::json!({
            "schema_version": 1,
            "key": key,
            "snapshot_id": snapshot_id,
            "context_id": "context",
            "toolchain_id": "toolchain",
            "success": true,
            "exit_code": 0,
            "diagnostics": [],
            "analysis_ms": 12,
            "created_at_unix": created_at,
            "payload_digest": "0".repeat(64),
        });
        let path = dir.join(format!("{key}.json"));
        std::fs::write(&path, serde_json::to_vec(&verdict).unwrap()).unwrap();
        verdict["analysis_ms"].as_u64().unwrap_or(0);
        std::fs::metadata(&path).unwrap().len()
    }

    fn write_snapshot(work: &Path, id: &str, payload: &[u8]) -> u64 {
        let dir = work.join(SNAPSHOTS_DIR).join(id).join("checkout").join("src");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("lib.rs"), payload).unwrap();
        std::fs::write(dir.parent().unwrap().join("manifest.json"), b"{}").unwrap();
        payload.len() as u64 + 2
    }

    fn write_context(work: &Path, short: &str, payload: &[u8]) -> u64 {
        let inc = work
            .join(INCREMENTAL_DIR)
            .join(short)
            .join("s-session");
        let out = work.join(OUT_DIR).join(short);
        let units = work.join(UNITS_DIR).join(short).join("src");
        for dir in [&inc, &out, &units] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(inc.join("dep-graph.bin"), payload).unwrap();
        std::fs::write(out.join("unit.rmeta"), b"rmeta").unwrap();
        std::fs::write(units.join("lib.rs"), b"pub fn f() {}").unwrap();
        payload.len() as u64 + 5 + 14
    }

    fn sample_work_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ravum-daemon-gc-unit-{name}-{}-{}",
            std::process::id(),
            current_unix()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn policy_validation_rejects_unsafe_values() {
        let mut policy = GcPolicy::default();
        assert!(policy.validate().is_ok());
        policy.keep_recent = 0;
        assert_eq!(policy.validate().unwrap_err().kind, crate::error::ErrorKind::InvalidRequest);
        let _ = policy;
        for field in 0..4 {
            let mut bad = GcPolicy::default();
            match field {
                0 => bad.max_cache_bytes = Some(0),
                1 => bad.max_snapshot_bytes = Some(0),
                2 => bad.max_incremental_bytes = Some(0),
                _ => bad.max_total_bytes = Some(0),
            }
            assert!(bad.validate().is_err(), "cap 0 must be rejected (field {field})");
        }
    }

    #[test]
    fn policy_roundtrips_through_json() {
        let policy = GcPolicy {
            max_cache_bytes: Some(1024),
            max_snapshot_bytes: None,
            max_incremental_bytes: Some(4096),
            max_total_bytes: None,
            keep_recent: 3,
            dry_run: true,
        };
        let text = serde_json::to_string(&policy).unwrap();
        let parsed: GcPolicy = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed, policy);
        // Unknown fields are rejected: a typo must not silently fall back to a
        // default (dangerous for a deletion policy).
        assert!(serde_json::from_str::<GcPolicy>(r#"{"keep_recent":2,"max_cach_bytes":1}"#).is_err());
    }

    #[test]
    fn retention_state_roundtrip_and_forget() {
        let dir = sample_work_dir("retention");
        let mut state = RetentionState::default();
        state.record(RecentAnalysis {
            revision: 3,
            cache_key: "k1".to_owned(),
            snapshot_id: "s1".to_owned(),
            context_short: "c1".to_owned(),
            created_at_unix: 10,
        });
        state.record(RecentAnalysis {
            revision: 4,
            cache_key: "k2".to_owned(),
            snapshot_id: "s2".to_owned(),
            context_short: "c1".to_owned(),
            created_at_unix: 11,
        });
        state.save(&dir).unwrap();
        let loaded = RetentionState::load(&dir);
        assert_eq!(loaded.recent.len(), 2);
        assert_eq!(loaded.last_cache_key.as_deref(), Some("k2"));
        assert_eq!(loaded.last_snapshot_id.as_deref(), Some("s2"));
        let newest = loaded.recent_newest_first(1);
        assert_eq!(newest[0].cache_key, "k2");
        let mut state = loaded;
        state.forget_cache_key("k2");
        assert_eq!(state.recent.len(), 1);
        assert!(state.last_cache_key.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn gc_protects_recent_and_deletes_oldest() {
        let work = sample_work_dir("protect");
        // Four contents, oldest to newest.
        for seed in 1u8..=4 {
            let key = hex_id(seed);
            let snapshot = hex_id(seed.wrapping_add(0x40));
            write_cache_entry(&work, &key, &snapshot, seed as u64);
            write_snapshot(&work, &snapshot, b"pub fn f() {}");
        }
        write_context(&work, &context_short(9), &[0u8; 64]);
        let mut protection = GcProtection::default();
        // Newest first: content 4 then 3.
        for seed in [4u8, 3] {
            protection.recent.push(RecentAnalysis {
                revision: seed as u64,
                cache_key: hex_id(seed),
                snapshot_id: hex_id(seed.wrapping_add(0x40)),
                context_short: context_short(9),
                created_at_unix: seed as u64,
            });
        }
        protection.last_cache_key = Some(hex_id(4));
        protection.last_snapshot_id = Some(hex_id(4u8.wrapping_add(0x40)));
        protection.last_context_short = Some(context_short(9));
        let policy = GcPolicy {
            max_cache_bytes: Some(1),
            max_snapshot_bytes: Some(1),
            max_incremental_bytes: None,
            max_total_bytes: None,
            keep_recent: 2,
            dry_run: false,
        };
        let report = run_gc(&work, &policy, &protection, GcMode::OneShot).unwrap();
        assert_eq!(report.deleted.cache_keys.len(), 2);
        assert!(report.deleted.cache_keys.contains(&hex_id(1)));
        assert!(report.deleted.cache_keys.contains(&hex_id(2)));
        assert!(report.kept.cache_keys.contains(&hex_id(3)));
        assert!(report.kept.cache_keys.contains(&hex_id(4)));
        // Referenced snapshots of kept verdicts survive; old ones are collected.
        assert!(report.deleted.snapshots.contains(&hex_id(0x41)));
        assert!(report.kept.snapshots.contains(&hex_id(0x44)));
        assert!(!report.budgets.cache.met, "keep_recent wins over the cap: reported honestly");
        assert!(report.freed_bytes > 0);
        // The files are really gone.
        assert!(!work.join(CACHE_DIR).join(format!("{}.json", hex_id(1))).exists());
        assert!(work.join(CACHE_DIR).join(format!("{}.json", hex_id(4))).exists());
        let _ = std::fs::remove_dir_all(&work);
    }

    #[test]
    fn gc_dry_run_changes_nothing() {
        let work = sample_work_dir("dry");
        for seed in 1u8..=3 {
            write_cache_entry(&work, &hex_id(seed), &hex_id(seed + 0x40), seed as u64);
        }
        let before: u64 = std::fs::read_dir(work.join(CACHE_DIR))
            .unwrap()
            .flatten()
            .filter_map(|entry| entry.metadata().ok().map(|meta| meta.len()))
            .sum();
        let policy = GcPolicy {
            max_cache_bytes: Some(1),
            keep_recent: 1,
            dry_run: true,
            ..Default::default()
        };
        let report = run_gc(&work, &policy, &GcProtection::default(), GcMode::OneShot).unwrap();
        assert!(report.dry_run);
        assert_eq!(report.freed_bytes, 0);
        assert!(!report.planned.cache_keys.is_empty());
        assert_eq!(report.before.cache_bytes, before);
        assert_eq!(report.after.cache_bytes, before);
        assert_eq!(std::fs::read_dir(work.join(CACHE_DIR)).unwrap().count(), 3);
        let _ = std::fs::remove_dir_all(&work);
    }

    #[test]
    fn gc_is_idempotent() {
        let work = sample_work_dir("idem");
        for seed in 1u8..=3 {
            write_cache_entry(&work, &hex_id(seed), &hex_id(seed + 0x40), seed as u64);
        }
        let policy = GcPolicy {
            max_cache_bytes: Some(1),
            keep_recent: 1,
            ..Default::default()
        };
        let first = run_gc(&work, &policy, &GcProtection::default(), GcMode::OneShot).unwrap();
        let second = run_gc(&work, &policy, &GcProtection::default(), GcMode::OneShot).unwrap();
        assert!(first.freed_bytes > 0);
        assert_eq!(second.deleted.cache_keys.len(), 0);
        assert_eq!(second.freed_bytes, 0);
        assert_eq!(second.before.cache_bytes, second.after.cache_bytes);
        let _ = std::fs::remove_dir_all(&work);
    }

    #[test]
    fn gc_tolerates_missing_and_partial_directories() {
        let missing = sample_work_dir("missing");
        let _ = std::fs::remove_dir_all(&missing);
        let report = run_gc(
            &missing,
            &GcPolicy::default(),
            &GcProtection::default(),
            GcMode::OneShot,
        )
        .unwrap();
        assert_eq!(report.before.total_bytes, 0);
        assert!(report.dry_run == false);

        let partial = sample_work_dir("partial");
        std::fs::create_dir_all(partial.join(CACHE_DIR)).unwrap();
        std::fs::write(partial.join(CACHE_DIR).join("garbage.json"), b"not json").unwrap();
        // A newer, valid entry: keep_recent=1 protects it, the corrupt/old file is
        // the oldest candidate even though it cannot be parsed.
        write_cache_entry(&partial, &hex_id(5), &hex_id(0x45), current_unix());
        let report = run_gc(
            &partial,
            &GcPolicy {
                max_cache_bytes: Some(1),
                keep_recent: 1,
                ..Default::default()
            },
            &GcProtection::default(),
            GcMode::OneShot,
        )
        .unwrap();
        assert_eq!(report.deleted.cache_keys.len(), 1);
        assert!(!partial.join(CACHE_DIR).join("garbage.json").exists());
        assert!(partial
            .join(CACHE_DIR)
            .join(format!("{}.json", hex_id(5)))
            .exists());
        let _ = std::fs::remove_dir_all(&partial);
        let _ = std::fs::remove_dir_all(&missing);
    }

    #[test]
    fn gc_never_deletes_last_content_even_beyond_keep_recent() {
        let work = sample_work_dir("last");
        for seed in 1u8..=3 {
            write_cache_entry(&work, &hex_id(seed), &hex_id(seed + 0x40), seed as u64);
        }
        let mut protection = GcProtection::default();
        protection.last_cache_key = Some(hex_id(1)); // oldest, but it is "current"
        protection.last_snapshot_id = Some(hex_id(0x41));
        let policy = GcPolicy {
            max_cache_bytes: Some(1),
            keep_recent: 1,
            ..Default::default()
        };
        let report = run_gc(&work, &policy, &protection, GcMode::OneShot).unwrap();
        assert!(report.kept.cache_keys.contains(&hex_id(1)));
        assert!(work.join(CACHE_DIR).join(format!("{}.json", hex_id(1))).exists());
        let _ = std::fs::remove_dir_all(&work);
    }

    #[test]
    fn gc_total_cap_spans_categories() {
        let work = sample_work_dir("total");
        write_cache_entry(&work, &hex_id(1), &hex_id(0x41), 1);
        write_cache_entry(&work, &hex_id(2), &hex_id(0x42), 2);
        write_snapshot(&work, &hex_id(0x41), &[0u8; 128]);
        write_snapshot(&work, &hex_id(0x42), &[0u8; 128]);
        write_context(&work, &context_short(7), &[0u8; 256]);
        write_context(&work, &context_short(9), &[0u8; 256]);
        let policy = GcPolicy {
            max_total_bytes: Some(1),
            keep_recent: 1,
            ..Default::default()
        };
        let report = run_gc(&work, &policy, &GcProtection::default(), GcMode::OneShot).unwrap();
        // The newest verdict + its referenced snapshot + the most recent build
        // context are protected; the older verdict/snapshot/context are collected
        // until nothing unprotected remains. The cap still cannot be met while
        // keeping live content: reported honestly.
        assert!(report.planned.entry_count() > 0);
        assert_eq!(report.planned.cache_keys, vec![hex_id(1)]);
        assert!(report.planned.contexts.contains(&context_short(9)));
        assert!(!report.budgets.total.met, "protected content exceeds the cap");
        let _ = std::fs::remove_dir_all(&work);
    }
}
