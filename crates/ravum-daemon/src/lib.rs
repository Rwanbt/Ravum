//! `ravum-daemon` — persistent incremental analysis daemon for Ravum MVP-0b.
//!
//! # Problem
//!
//! MVP-0a launched a cold `rustc` for every edit: the measured `check` stage was
//! ~132–190 ms and the empty-compiler floor alone (`rustc -vV`) was 80–136 ms on
//! the reference machine. Content-identical edits were cached, but a *real*
//! modification always paid the full cold cost.
//!
//! # Design
//!
//! A long-lived daemon owns:
//!
//! * **immutable snapshots** addressed by `SHA-256(source bytes + build context
//!   + toolchain)` — unsaved editor buffers are supported, user files are never
//!   modified;
//! * a **stable compile unit** per build context, so repeated `rustc` runs with
//!   `-C incremental=<persistent dir>` see the same source paths across edits;
//! * a **two-level content-addressed result cache** (memory LRU + persistent
//!   JSON verdicts with self-digests);
//! * a **monotonic revision** tracker: a superseded revision is never returned
//!   as current, and an in-flight compiler is killed when possible;
//! * **structured JSON reports**: diagnostics (file/line/column/severity/code/
//!   message), per-stage latencies (acceptance, cache lookup, analysis, total)
//!   and the cache hit/miss indicator.
//!
//! # Public interface only
//!
//! Compilation uses the documented `rustc` CLI (`--emit=metadata`,
//! `--error-format=json`, `--diagnostic-width`, `-C incremental=...`) and
//! nothing else: no `rustc_private`, no `RUSTC_BOOTSTRAP`, no wrapper
//! inheritance (INV-11). The daemon's advertised capability therefore matches
//! what any external tool can rely on.
//!
//! # Honest limits
//!
//! * Every cache miss still forks a compiler: "persistent" means cached state
//!   persists, not that the compiler runs in-process.
//! * `-C incremental` on `--emit=metadata` is measured, not assumed: see
//!   `docs/mvp/MVP-0b-incremental.md`. On the reference machine it does not beat
//!   a cold invocation for small/medium units, because process start and
//!   incremental-session I/O dominate front-end reuse.
//! * The build/verify/activation stages remain the responsibility of the
//!   existing Ravum pipeline; this daemon only covers the analysis stage.

pub mod auto_gc;
pub mod cache;
pub mod config;
pub mod daemon;
pub mod diagnostics;
pub mod error;
pub mod fsutil;
pub mod gate;
pub mod gc;
pub mod hash;
pub mod report;
pub mod revision;
pub mod rustc_driver;
pub mod snapshot;
pub mod sysinfo;

pub use auto_gc::{AutoGcDecision, AutoGcJournalEntry, AutoGcPolicy, AutoGcStats};
pub use cache::{CacheKind, CacheLookupKey, CacheReport, CachedVerdict};
pub use config::{
    context_id, BuildContext, DaemonConfig, Priority, ToolchainInfo, ANALYSIS_ENGINE_VERSION,
    DAEMON_SCHEMA_VERSION,
};
pub use daemon::{single_file_request, work_dir_below, Daemon};
pub use diagnostics::{parse_rustc_json, signature_set, Diagnostic, ParsedOutput, Severity};
pub use error::{DaemonError, ErrorKind, Result};
pub use gc::{
    run_gc, BudgetStatus, CategorySizes, GcBudgets, GcDeleted, GcKept, GcMode, GcPolicy,
    GcProtection, GcReport, RecentAnalysis, RetentionState,
};
pub use report::{
    AnalysisReport, AutoGcLast, DaemonStats, ReportStatus, SnapshotReport, Timings,
};
pub use snapshot::{
    sync_unit_dir, AnalysisRequest, MaterializedSnapshot, SnapshotIdentity, SnapshotStore,
    SourceFile, SyncStats,
};
