//! COLLAB-1 — multi-agent proposal coordination on a shared branch.
//!
//! Demonstrated properties (and only these):
//! * agents submit proposals, they never write the workspace directly;
//! * every proposal is checked against a base snapshot and expected hashes;
//! * the coordinator decides AUTO_SAFE / REBASE_REVALIDATE / CONFLICT / REJECT /
//!   MANUAL_REQUIRED with recorded reasons;
//! * integration is an in-memory overlay, applied sequentially;
//! * one `GitWriter` owns every mutating Git command.
//!
//! Not demonstrated: semantic/AST merge, CRDT convergence, automatic rebase with
//! correctness, cross-module contract arbitration, or conflict-free concurrent
//! edits of the same function.

mod coordinator;
mod git_writer;
pub mod patch;

pub use coordinator::{Coordinator, IntegrationSummary};
pub use git_writer::{CommitResult, GitError, GitWriter};
pub use patch::{apply, parse, touched_ranges, FilePatch, Hunk, PatchError};
