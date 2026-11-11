//! COLLAB-1→5 — multi-agent proposal coordination on a shared branch.
//!
//! Demonstrated properties (and only these):
//! * agents submit proposals, they never write the workspace directly;
//! * every proposal is checked against a base snapshot and expected hashes;
//! * the coordinator decides AUTO_SAFE / REBASE_REVALIDATE / CONFLICT / REJECT /
//!   MANUAL_REQUIRED with recorded reasons;
//! * integration is an in-memory overlay, applied sequentially;
//! * **MVP-0c L3**: every decision also carries a versioned and additive
//!   [`EvaluationLevel`] (`TEXTUALLY_APPLICABLE`, `SEMANTICALLY_CHECKED`,
//!   `TESTED`, `PROVED`, `REJECTED`, `MANUAL_REQUIRED`) under
//!   [`COORDINATOR_SCHEMA_VERSION`]. Under the v2 protocol
//!   ([`Coordinator::v2`]) a textually applicable patch is **never** `AUTO_SAFE`
//!   on its own: it needs a declared [`MergeVerifier`] and a [`SemanticOracle`]
//!   verdict; absence/unavailability fails closed to `MANUAL_REQUIRED` (or
//!   `REBASE_REVALIDATE` for an interface change used elsewhere);
//! * `PROVED` is only reachable through an explicit
//!   [`BoundedProof`](oracle::BoundedProof) with complete provenance bound to the
//!   exact candidate snapshot — there is no implicit proof;
//! * the v1 protocol is kept as an explicit COLLAB-1 compatibility profile whose
//!   `AUTO_SAFE` is marked `V1_COLLAB_COMPAT` + `TEXTUALLY_APPLICABLE`, and the
//!   tolerant journal reader accepts legacy v1 lines without data loss;
//! * contract/ABI changes and undeclared semantic dependencies are refused with
//!   journaled reasons and evidence ids instead of being auto-integrated;
//! * every accepted change produces an `AcceptedChange` record (snapshots before
//!   and after, paths, symbols, decision, verification record, evaluation record,
//!   evidence ids) that an L2 daemon can consume without re-running the
//!   coordinator;
//! * one `GitWriter` owns every mutating Git command, and publication is
//!   transactional: an isolated index builds an unpublished commit, the reference
//!   is moved by one compare-and-swap, hooks are executed (never bypassed by
//!   default), and a concurrent writer is detected *before* publication;
//! * accepted and refused publications are journaled durably, so a restart can
//!   recover an interrupted attempt without publishing it twice.
//!
//! Not demonstrated: full Rust parsing (the default oracle is textual heuristics),
//! general program proofs (`PROVED` is bounded only), L1 inter-crate analysis
//! (the `SemanticOracle` interface is stable and tested with fakes; the real
//! branch is wired later), CRDT convergence, or conflict-free concurrent edits of
//! the same function.

mod coordinator;
pub mod cycle;
pub mod evaluation;
mod git_writer;
pub mod oracle;
pub mod overlay;
pub mod patch;
mod path_policy;
mod publication_journal;
pub mod records;
pub mod semantics;
pub mod verifier;

pub use coordinator::{Coordinator, IntegrationSummary};
pub use cycle::{commit_cycle, CycleOutcome, CycleRequest};
pub use evaluation::{
    level_for_decision, parse_journal_line, read_journal_records, CoordinatorJournalRecord,
    EvaluationLevel, EvaluationProtocol, EvaluationRecord, JournalReadError,
    COORDINATOR_SCHEMA_VERSION,
};
pub use git_writer::{
    CommitResult, GitError, GitWriter, HookPolicy, PathUpdate, PreparedPublication, PublishRequest,
    RecoveryReport, RecoveryUnknown,
};
pub use oracle::{
    BoundedProof, OracleFinding, OracleFindingKind, OracleReport, OracleRequest, OracleSeverity,
    OracleVerdict, SemanticOracle, TextualSemanticsOracle,
};
pub use overlay::{
    validate_relative_path, BaseSnapshot, ChangeCoordinator, IntegrationOutcome, MaterializeLimits,
    Overlay, OverlayError, StorageReport, MAX_COMPONENT_BYTES, MAX_RELATIVE_PATH_BYTES,
    OVERLAY_SCHEMA_VERSION,
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
