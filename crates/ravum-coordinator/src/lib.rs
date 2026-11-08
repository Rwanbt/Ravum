//! Incremental coordinator for the M1 continuous loop.
//!
//! It owns three things and nothing else:
//! * which snapshot is current and which finished results are therefore `STALE`;
//! * the analyze → build → verify → publish order;
//! * the smoke contract used as activation evidence (`TESTED`, never `PROVED`).
//!
//! It does not schedule OS threads, own a Git checkout, or claim to prove anything.

mod pipeline;
mod session;

pub use pipeline::{
    merge_activation, smoke_verification_key, smoke_verify, LoopOutcome, LoopReport, Pipeline,
    SMOKE_INPUTS,
};
pub use session::{CoalescingQueue, Delivery, Job, WorkspaceSession};
