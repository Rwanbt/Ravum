//! Ravum proofs — M2.
//!
//! Three pieces and nothing more:
//! * `Obligation` — a property over an **explicit finite domain**, with declared
//!   assumptions that are part of the cache key (ADR-013);
//! * `Verifier` backends — an exhaustive enumerator that can honestly say `Proved`
//!   for its declared domain, and an external-prover adapter that refuses to claim
//!   anything without a harness;
//! * `EvidenceLedger` + `ProofBroker` — an append-only record of verdicts and a
//!   cache that is invalidated by mutating any part of the context.
//!
//! What this module never does: turn `Unknown`, `Timeout`, `Unsupported` or
//! `Stale` into success, or describe a bounded enumeration as a general proof.

pub mod broker;
pub mod ledger;
pub mod obligation;
pub mod verifier;

pub use broker::{ProofBroker, ProofResult, ProofStats};
pub use ledger::{EvidenceLedger, LedgerError, LedgerLine, Lookup};
pub use obligation::{
    verification_key, Claim, Obligation, ProofContext, OBLIGATION_SCHEMA_VERSION,
};
pub use verifier::{ExhaustiveVerifier, KaniVerifier, Verifier, VerifierOutcome};
