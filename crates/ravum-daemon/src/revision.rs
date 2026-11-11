//! Monotonic revisions, stale-result rejection and cooperative cancellation.
//!
//! Contract: an analysis result is only reported as *current* when the
//! requested revision is still the daemon's newest revision when the answer is
//! produced. A newer revision supersedes older ones, and a superseded answer
//! never carries diagnostics: callers cannot mistake stale output for a current
//! analysis. When possible the superseded `rustc` process is actually killed
//! (see `rustc_driver`), not merely ignored.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Cooperative cancellation flag shared between the revision tracker and a
/// running `rustc` job. The driver polls it and kills its child process.
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

/// Latest revision observed by the daemon. Monotonic by construction: `note`
/// never lowers it, so revision numbers from different clients cannot make the
/// daemon "go back in time".
#[derive(Debug, Default)]
pub struct RevisionTracker {
    current: AtomicU64,
}

impl RevisionTracker {
    pub fn current(&self) -> u64 {
        self.current.load(Ordering::SeqCst)
    }

    /// Records `revision` and returns the previous current revision.
    pub fn note(&self, revision: u64) -> u64 {
        self.current.fetch_max(revision, Ordering::SeqCst)
    }

    /// True when `revision` is strictly older than the current revision.
    pub fn is_superseded(&self, revision: u64) -> bool {
        revision < self.current()
    }
}

struct RunningJob {
    revision: u64,
    token: CancelToken,
}

/// Registry of in-flight analyses so a newer revision can cancel older work.
#[derive(Default)]
pub struct RunningJobs {
    next_seq: AtomicU64,
    jobs: Mutex<HashMap<u64, RunningJob>>,
}

impl RunningJobs {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a running job; the returned guard unregisters it on drop.
    pub fn register(&self, revision: u64) -> RunningGuard<'_> {
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        let token = CancelToken::new();
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.insert(
                seq,
                RunningJob {
                    revision,
                    token: token.clone(),
                },
            );
        }
        RunningGuard { jobs: self, seq }
    }

    /// Cancels every running job strictly older than `revision`. Returns how
    /// many jobs were signalled.
    pub fn cancel_below(&self, revision: u64) -> usize {
        let Ok(jobs) = self.jobs.lock() else {
            return 0;
        };
        let mut cancelled = 0;
        for job in jobs.values() {
            if job.revision < revision {
                job.token.cancel();
                cancelled += 1;
            }
        }
        cancelled
    }

    pub fn len(&self) -> usize {
        self.jobs.lock().map(|jobs| jobs.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// RAII registration of one in-flight analysis.
pub struct RunningGuard<'a> {
    jobs: &'a RunningJobs,
    seq: u64,
}

impl RunningGuard<'_> {
    pub fn token(&self) -> CancelToken {
        self.jobs
            .jobs
            .lock()
            .ok()
            .and_then(|jobs| jobs.get(&self.seq).map(|job| job.token.clone()))
            .unwrap_or_default()
    }
}

impl Drop for RunningGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut jobs) = self.jobs.jobs.lock() {
            jobs.remove(&self.seq);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revisions_are_monotonic() {
        let tracker = RevisionTracker::default();
        assert_eq!(tracker.note(5), 0);
        assert_eq!(tracker.current(), 5);
        assert_eq!(tracker.note(3), 5, "older revision must not lower current");
        assert_eq!(tracker.current(), 5);
        assert!(tracker.is_superseded(4));
        assert!(!tracker.is_superseded(5));
        assert!(!tracker.is_superseded(6));
    }

    #[test]
    fn newer_revision_cancels_running_jobs() {
        let jobs = RunningJobs::new();
        let old = jobs.register(3);
        let old_token = old.token();
        let newer = jobs.register(7);
        let newer_token = newer.token();
        assert_eq!(jobs.cancel_below(5), 1);
        assert!(old_token.is_cancelled());
        assert!(!newer_token.is_cancelled());
        assert_eq!(jobs.len(), 2);
        drop(old);
        assert_eq!(jobs.len(), 1);
        drop(newer);
        assert!(jobs.is_empty());
    }
}
