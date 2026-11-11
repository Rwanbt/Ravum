//! Workspace session: which snapshot is current, which jobs are still meaningful.
//!
//! This is the mechanism behind INV-07 ("a result from an outdated snapshot is
//! never activated"). It is deliberately small: a monotonic epoch, a one-slot
//! coalescing queue, and an explicit `Delivered`/`Stale` verdict.

use ravum_protocol::SnapshotId;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// A unit of work submitted for one exact snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub id: u64,
    pub snapshot: SnapshotId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
    /// The job still describes the current snapshot: its result may be used.
    Delivered,
    /// The snapshot changed while the job was running: the result is STALE.
    Stale { job: u64, current: SnapshotId },
}

/// Current snapshot + admission control.
#[derive(Debug)]
pub struct WorkspaceSession {
    current: Mutex<SnapshotId>,
    seq: AtomicU64,
    superseded: AtomicU64,
    stale_results: AtomicU64,
}

impl WorkspaceSession {
    pub fn new(initial: SnapshotId) -> Self {
        Self {
            current: Mutex::new(initial),
            seq: AtomicU64::new(0),
            superseded: AtomicU64::new(0),
            stale_results: AtomicU64::new(0),
        }
    }

    pub fn submit(&self) -> Job {
        let snapshot = self.current();
        Job {
            id: self.seq.fetch_add(1, Ordering::SeqCst) + 1,
            snapshot,
        }
    }

    /// Record a newer snapshot. Every job submitted before this call becomes stale.
    pub fn supersede(&self, snapshot: SnapshotId) {
        let mut current = self.current.lock().unwrap_or_else(|e| e.into_inner());
        if *current != snapshot {
            *current = snapshot;
            self.superseded.fetch_add(1, Ordering::SeqCst);
        }
    }

    pub fn current(&self) -> SnapshotId {
        self.current
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Decide whether a finished job may be used. Counts every refusal, so
    /// staleness is observable rather than silent.
    pub fn deliver(&self, job: &Job) -> Delivery {
        let current = self.current();
        if job.snapshot == current {
            Delivery::Delivered
        } else {
            self.stale_results.fetch_add(1, Ordering::SeqCst);
            Delivery::Stale {
                job: job.id,
                current,
            }
        }
    }

    pub fn superseded_count(&self) -> u64 {
        self.superseded.load(Ordering::SeqCst)
    }

    pub fn stale_result_count(&self) -> u64 {
        self.stale_results.load(Ordering::SeqCst)
    }
}

/// One-slot queue: while the user keeps typing, only the latest edit matters.
#[derive(Debug, Default)]
pub struct CoalescingQueue {
    slot: Mutex<Option<Job>>,
    dropped: AtomicU64,
}

impl CoalescingQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace any pending job with `job`; returns how many pending jobs were dropped.
    pub fn push(&self, job: Job) -> u64 {
        let mut slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        let dropped = u64::from(slot.is_some());
        *slot = Some(job);
        if dropped > 0 {
            self.dropped.fetch_add(1, Ordering::SeqCst);
        }
        dropped
    }

    pub fn pop(&self) -> Option<Job> {
        self.slot.lock().unwrap_or_else(|e| e.into_inner()).take()
    }

    pub fn coalesced_count(&self) -> u64 {
        self.dropped.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(v: &str) -> SnapshotId {
        SnapshotId::new(v)
    }

    #[test]
    fn job_from_superseded_snapshot_is_stale() {
        let session = WorkspaceSession::new(snap("s1"));
        let job = session.submit();
        assert_eq!(session.deliver(&job), Delivery::Delivered);
        session.supersede(snap("s2"));
        assert!(matches!(session.deliver(&job), Delivery::Stale { .. }));
        assert_eq!(session.stale_result_count(), 1);
    }

    #[test]
    fn resubmitting_the_same_snapshot_does_not_supersede() {
        let session = WorkspaceSession::new(snap("s1"));
        session.supersede(snap("s1"));
        assert_eq!(session.superseded_count(), 0);
    }

    #[test]
    fn queue_keeps_only_the_latest_edit() {
        let queue = CoalescingQueue::new();
        let session = WorkspaceSession::new(snap("s1"));
        queue.push(session.submit());
        queue.push(session.submit());
        assert_eq!(queue.push(session.submit()), 1);
        assert_eq!(queue.coalesced_count(), 2);
        let last = queue.pop().expect("pending job");
        assert_eq!(last.id, 3);
        assert!(queue.pop().is_none());
    }
}
