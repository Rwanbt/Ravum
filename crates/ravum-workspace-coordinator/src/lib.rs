//! COLLAB-1 — multi-agent proposal coordination on a shared branch.
//!
//! Demonstrated properties (and only these):
//! * agents submit proposals, they never write the workspace directly;
//! * every proposal is checked against a base snapshot and expected hashes;
//! * the coordinator decides AUTO_SAFE / REBASE_REVALIDATE / CONFLICT / REJECT /
//!   MANUAL_REQUIRED with recorded reasons;
//! * integration is an in-memory overlay, applied sequentially;
//! * one `GitWriter` owns every mutating Git command, and publication is
//!   transactional: an isolated index builds an unpublished commit, the reference
//!   is moved by one compare-and-swap, hooks are executed (never bypassed by
//!   default), and a concurrent writer is detected *before* publication;
//! * accepted and refused publications are journaled durably, so a restart can
//!   recover an interrupted attempt without publishing it twice.
//!
//! Not demonstrated: semantic/AST merge, CRDT convergence, automatic rebase with
//! correctness, cross-module contract arbitration, or conflict-free concurrent
//! edits of the same function.

mod coordinator;
mod git_writer;
pub mod patch;
mod path_policy;
mod publication_journal;

pub use coordinator::{Coordinator, IntegrationSummary};
pub use git_writer::{
    CommitResult, GitError, GitWriter, HookPolicy, PathUpdate, PreparedPublication, PublishRequest,
    RecoveryReport, RecoveryUnknown,
};
pub use patch::{apply, parse, touched_ranges, FilePatch, Hunk, PatchError};
pub use publication_journal::{
    PublicationJournal, PublicationRecord, PublicationStatus, PUBLICATION_JOURNAL_SCHEMA_VERSION,
};
