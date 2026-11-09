//! Conservative cross-crate unit graph and chained `--extern` compilation.
//!
//! This crate is the MVP-0c L1 building block. It deliberately does **not** parse
//! `Cargo.toml` itself: the workspace model comes from the public
//! `cargo metadata --no-deps` interface, and every uncertainty falls back to Cargo
//! as the oracle. No `PROVED` claim is ever produced.

pub mod archive;
pub mod graph;
pub mod identity;
pub mod proposals;
pub mod runner;

pub use archive::{ArtifactStore, ExternArtifact, VerdictArchive, VerdictEntry};
pub use graph::{Dependency, DiscoverError, FallbackReason, GraphError, Unit, UnitGraph, UnitKind};
pub use identity::{content_id, externs_digest, sha256_hex, verdict_key, ContextSpec};
pub use proposals::{Proposal, ProposalDecision, ProposalSet};
pub use runner::{run_chain, CargoTrace, ChainOptions, RunReport, RustcTrace, UnitOutcome};

/// Explicit status of one unit in a campaign. There is intentionally **no**
/// `PROVED` variant: a successful targeted chain is a bounded, reproducible
/// observation, not a proof of the whole program's correctness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitStatus {
    /// The complete `--extern` chain was verified with fresh interface artifacts.
    TargetedOk,
    /// Cargo (`cargo check -p`) was the oracle for this package.
    FallbackPackage,
    /// The whole campaign fell back to Cargo (e.g. non-host target).
    FallbackFull,
    /// The sources changed during verification: the result was discarded.
    Stale,
    /// Real diagnostics were produced (targeted rustc or Cargo).
    Error,
}

impl UnitStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            UnitStatus::TargetedOk => "targeted_ok",
            UnitStatus::FallbackPackage => "fallback_package",
            UnitStatus::FallbackFull => "fallback_full",
            UnitStatus::Stale => "stale",
            UnitStatus::Error => "error",
        }
    }
}

impl Default for UnitStatus {
    fn default() -> Self {
        UnitStatus::Error
    }
}

/// Version of the JSON report produced by [`runner::run_chain`].
pub const REPORT_SCHEMA_VERSION: u32 = 1;

use serde::{Deserialize, Serialize};
