//! Lock-free publication adapter over the ABI 3 module and state types.

use std::path::Path;
use std::sync::Arc;

use ravum_protocol::{BuildOutput, EvidenceRecord, RejectReason, SnapshotId};

use crate::block3::{layout_refusal, prepare_block3, Block3Module, StateBuffer};
use crate::host::evaluate_gates;

mod live;
pub use live::{OpaqueEditor, OpaqueRunner, OpaqueUpdate};

pub type PreparedOpaqueBlock = Arc<Block3Module>;

/// Load a trusted ABI 3 artifact on the editor thread.
///
/// # Safety
/// Metadata validates compatibility. The native artifact must honor the ABI.
pub unsafe fn prepare_opaque_block(path: &Path) -> Result<PreparedOpaqueBlock, RejectReason> {
    prepare_block3(path)
}

impl Block3Module {
    fn authorize(
        &self,
        build: &BuildOutput,
        evidence: Option<&EvidenceRecord>,
        current_snapshot: &SnapshotId,
    ) -> Result<(), RejectReason> {
        if self.artifact_id != build.artifact_id {
            return Err(RejectReason::ArtifactMismatch {
                expected: build.artifact_id.clone(),
                found: self.artifact_id.clone(),
            });
        }
        evaluate_gates(build, &self.artifact_id, evidence, current_snapshot)?;
        Ok(())
    }
}

/// Exclusive state owner. Run lifecycle methods on the editor side at a block
/// boundary, after the preceding `process` call has released its borrow.
pub struct OpaqueBlock {
    module: PreparedOpaqueBlock,
    state: StateBuffer,
}

impl OpaqueBlock {
    pub fn start(module: PreparedOpaqueBlock) -> Result<Self, RejectReason> {
        let state = module.initialize()?;
        Ok(Self { module, state })
    }

    pub fn replace(
        &mut self,
        module: PreparedOpaqueBlock,
        build: &BuildOutput,
        evidence: Option<&EvidenceRecord>,
        current_snapshot: &SnapshotId,
    ) -> Result<(), RejectReason> {
        module.authorize(build, evidence, current_snapshot)?;
        if module.schema == self.module.schema {
            if module.layout != self.module.layout {
                return Err(layout_refusal(module.layout));
            }
        } else {
            let mut snapshot = StateBuffer::allocate(self.module.layout)?;
            snapshot.copy_from(&self.state);
            self.state = module.migrate_state(&self.module, &snapshot)?;
        }
        self.module = module;
        Ok(())
    }

    /// Audio path: exclusive access to the state and no allocation or locks.
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) -> usize {
        self.module.render(&mut self.state, input, output)
    }
}
