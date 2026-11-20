use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ravum_protocol::{BuildOutput, EvidenceRecord, RejectReason, SnapshotId};

use super::{layout_refusal, OpaqueBlock, PreparedOpaqueBlock, StateBuffer};
use crate::ArcSlot;

struct Snapshot {
    state: UnsafeCell<StateBuffer>,
    ready: AtomicBool,
}

// SAFETY: only the runner writes before publishing ready. Only the editor then
// reads, and the runner never writes again. ArcSlot keeps each request alive.
unsafe impl Sync for Snapshot {}

struct Transfer {
    state: UnsafeCell<StateBuffer>,
    adopted: AtomicBool,
}

// SAFETY: the editor initializes the cell before publication and never accesses
// it afterward. Only the single runner can replace its content, once.
unsafe impl Sync for Transfer {}

struct Version {
    module: PreparedOpaqueBlock,
    transfer: Option<Transfer>,
}

struct Link {
    versions: ArcSlot<Version>,
    snapshot: ArcSlot<Option<Snapshot>>,
}

struct Pending {
    candidate: PreparedOpaqueBlock,
    source: SnapshotId,
}

pub struct OpaqueEditor {
    link: Arc<Link>,
    pending: Option<Pending>,
}

pub struct OpaqueRunner {
    link: Arc<Link>,
    state: StateBuffer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpaqueUpdate {
    Published,
    WaitingForBlock,
}

impl OpaqueBlock {
    /// Move state into the single audio runner. Keep the editor outside the
    /// callback and drop the runner after the device has stopped.
    pub fn split(self) -> (OpaqueEditor, OpaqueRunner) {
        let link = Arc::new(Link {
            versions: ArcSlot::new(Arc::new(Version {
                module: self.module,
                transfer: None,
            })),
            snapshot: ArcSlot::new(Arc::new(None)),
        });
        (
            OpaqueEditor {
                link: Arc::clone(&link),
                pending: None,
            },
            OpaqueRunner {
                link,
                state: self.state,
            },
        )
    }
}

impl OpaqueEditor {
    pub fn update(
        &mut self,
        candidate: PreparedOpaqueBlock,
        build: &BuildOutput,
        evidence: Option<&EvidenceRecord>,
        current_snapshot: &SnapshotId,
    ) -> Result<OpaqueUpdate, RejectReason> {
        candidate.authorize(build, evidence, current_snapshot)?;
        let active = self.link.versions.load_owning();
        if self.pending.is_some()
            || active
                .transfer
                .as_ref()
                .is_some_and(|transfer| !transfer.adopted.load(Ordering::Acquire))
        {
            return Err(RejectReason::StateUpdateInFlight);
        }
        if active.module.schema == candidate.schema {
            if active.module.layout != candidate.layout {
                return Err(layout_refusal(candidate.layout));
            }
            self.link.versions.store(Arc::new(Version {
                module: candidate,
                transfer: None,
            }));
            return Ok(OpaqueUpdate::Published);
        }
        let state = StateBuffer::allocate(active.module.layout)?;
        self.link.snapshot.store(Arc::new(Some(Snapshot {
            state: UnsafeCell::new(state),
            ready: AtomicBool::new(false),
        })));
        self.pending = Some(Pending {
            candidate,
            source: build.source_snapshot_id.clone(),
        });
        Ok(OpaqueUpdate::WaitingForBlock)
    }

    /// Complete a requested migration only when the current snapshot has been
    /// captured. The runner continues processing the old version meanwhile.
    pub fn finish_migration(
        &mut self,
        current_snapshot: &SnapshotId,
    ) -> Result<Option<OpaqueUpdate>, RejectReason> {
        let Some(pending) = &self.pending else {
            return Ok(None);
        };
        if pending.source != *current_snapshot {
            let reason = RejectReason::StaleBuild {
                built_from: pending.source.clone(),
                expected: current_snapshot.clone(),
            };
            self.cancel_migration();
            return Err(reason);
        }
        let request = self.link.snapshot.load_owning();
        let Some(snapshot) = request.as_ref() else {
            return Ok(None);
        };
        if !snapshot.ready.load(Ordering::Acquire) {
            return Ok(Some(OpaqueUpdate::WaitingForBlock));
        }
        let Some(pending) = self.pending.take() else {
            return Ok(None);
        };
        self.link.snapshot.store(Arc::new(None));
        let active = self.link.versions.load_owning();
        // SAFETY: acquire observed the runner's release. It cannot touch the
        // buffer again. Our owning guard keeps the snapshot in memory.
        let captured = unsafe { &*snapshot.state.get() };
        let state = pending.candidate.migrate_state(&active.module, captured)?;
        self.link.versions.store(Arc::new(Version {
            module: pending.candidate,
            transfer: Some(Transfer {
                state: UnsafeCell::new(state),
                adopted: AtomicBool::new(false),
            }),
        }));
        Ok(Some(OpaqueUpdate::Published))
    }

    pub fn cancel_migration(&mut self) {
        self.pending = None;
        self.link.snapshot.store(Arc::new(None));
    }
}

impl OpaqueRunner {
    /// Audio path. Snapshot and adoption happen only at block boundaries.
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) -> usize {
        let active = self.link.versions.enter();
        if let Some(transfer) = &active.transfer {
            if !transfer.adopted.load(Ordering::Relaxed) {
                // SAFETY: only this runner accesses the cell. The slot
                // retains ownership of the retired state until editor reclamation.
                let state = unsafe { &mut *transfer.state.get() };
                std::mem::swap(&mut self.state, state);
                transfer.adopted.store(true, Ordering::Release);
            }
        }
        let request = self.link.snapshot.enter();
        if let Some(snapshot) = request.as_ref() {
            if !snapshot.ready.load(Ordering::Relaxed) {
                // SAFETY: only this runner writes, and the editor reads only
                // after the release below. The request has the active layout.
                let state = unsafe { &mut *snapshot.state.get() };
                state.copy_from(&self.state);
                snapshot.ready.store(true, Ordering::Release);
            }
        }
        active.module.render(&mut self.state, input, output)
    }
}
