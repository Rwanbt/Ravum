//! COLLAB-1/2/3 — multi-agent proposal coordination on a shared branch.
//!
//! Demonstrated properties (and only these):
//! * agents submit proposals, they never write the workspace directly;
//! * every proposal is checked against a base snapshot and expected hashes;
//! * the coordinator decides AUTO_SAFE / REBASE_REVALIDATE / CONFLICT / REJECT /
//!   MANUAL_REQUIRED with recorded reasons;
//! * integration is an in-memory overlay, applied sequentially;
//! * in semantic mode (`Coordinator::with_merge_verifier`) a textually clean
//!   merge is **not** `AUTO_SAFE` until an injected `MergeVerifier` compiled the
//!   exact candidate snapshot (and, when the change is behavioural and the
//!   snapshot ships tests, ran its test harness). A required check that cannot
//!   run yields `MANUAL_REQUIRED`, never a silent pass;
//! * contract/ABI changes and undeclared semantic dependencies are refused with
//!   journaled reasons and evidence ids instead of being auto-integrated;
//! * every accepted change produces an `AcceptedChange` record (snapshots before
//!   and after, paths, symbols, decision, verification record, evidence ids) that
//!   an L2 daemon can consume without re-running the coordinator;
//! * one `GitWriter` owns every mutating Git command, and publication is
//!   transactional: an isolated index builds an unpublished commit, the reference
//!   is moved by one compare-and-swap, hooks are executed (never bypassed by
//!   default), and a concurrent writer is detected *before* publication;
//! * accepted and refused publications are journaled durably, so a restart can
//!   recover an interrupted attempt without publishing it twice.
//!
//! Not demonstrated: full Rust parsing (the semantic facts are heuristics), CRDT
//! convergence, automatic rebase with correctness, cross-repository contract
//! arbitration, or conflict-free concurrent edits of the same function.

mod coordinator;
mod git_writer;
pub mod patch;
mod path_policy;
mod publication_journal;
pub mod records;
pub mod semantics;
pub mod verifier;

pub use coordinator::{Coordinator, IntegrationSummary};
pub use git_writer::{
    CommitResult, GitError, GitWriter, HookPolicy, PathUpdate, PreparedPublication, PublishRequest,
    RecoveryReport, RecoveryUnknown,
};
pub use patch::{apply, parse, touched_ranges, FilePatch, Hunk, PatchError};
pub use publication_journal::{
    PublicationJournal, PublicationRecord, PublicationStatus, PUBLICATION_JOURNAL_SCHEMA_VERSION,
};
pub use records::{
    accepted_change_evidence_id, merge_check_evidence_id, verification_evidence_id, AcceptedChange,
    MergeCheck, MergeCheckKind, MergeCheckStatus, MergeDiagnostic, VerificationRecord,
    COLLAB_RECORD_SCHEMA_VERSION,
};
pub use verifier::{
    extra_checks, missing_required_checks, MergeVerifier, VerificationOutcome, VerificationRequest,
};
