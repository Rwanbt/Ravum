//! Proof broker: decide, cache, and never upgrade a status by convenience.

use crate::ledger::{EvidenceLedger, LedgerError, Lookup};
use crate::obligation::{verification_key, Obligation, ProofContext};
use crate::verifier::{Verifier, VerifierOutcome};
use ravum_protocol::{
    sha256_hex, BuildOutput, EvidenceRecord, VerificationKey, VerificationStatus,
    EVIDENCE_SCHEMA_VERSION,
};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProofResult {
    pub key: VerificationKey,
    pub record: EvidenceRecord,
    /// True when no verifier run was needed (same key, same context).
    pub from_cache: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProofStats {
    pub verifier_runs: u64,
    pub cache_hits: u64,
    pub recorded: u64,
}

pub struct ProofBroker {
    verifier: Box<dyn Verifier>,
    ledger: EvidenceLedger,
    stats: ProofStats,
}

impl ProofBroker {
    pub fn new(verifier: Box<dyn Verifier>, ledger: EvidenceLedger) -> Self {
        let recorded = ledger.len() as u64;
        Self {
            verifier,
            ledger,
            stats: ProofStats {
                recorded,
                ..ProofStats::default()
            },
        }
    }

    pub fn verifier_name(&self) -> &str {
        self.verifier.name()
    }

    pub fn verifier_version(&self) -> String {
        self.verifier.version()
    }

    pub fn ledger(&self) -> &EvidenceLedger {
        &self.ledger
    }

    /// Build the context for a build with **this verifier's** identity.
    ///
    /// Using this instead of hand-building the context prevents the classic bug
    /// where the cache key and the recorded tool disagree.
    pub fn context_for(&self, build: &BuildOutput) -> ProofContext {
        ProofContext::new(
            build.source_snapshot_id.clone(),
            build.build_context_id.clone(),
            build.artifact_id.clone(),
            self.verifier.name(),
            self.verifier.version(),
        )
    }

    pub fn stats(&self) -> ProofStats {
        self.stats
    }

    /// Obtain a verdict for `obligation` on `artifact` in `ctx`.
    ///
    /// Cache hit ⇒ zero verifier work. Cache miss or stale context ⇒ real run.
    /// A failure is also cached: refusing to re-run a known counterexample is a
    /// feature, not an optimisation, and it can never be upgraded to a success.
    pub fn prove(
        &mut self,
        artifact: &Path,
        obligation: &Obligation,
        ctx: &ProofContext,
    ) -> Result<ProofResult, LedgerError> {
        let key = verification_key(obligation, ctx);
        match self.ledger.lookup(&key, ctx) {
            Lookup::Fresh(record) => {
                self.stats.cache_hits += 1;
                return Ok(ProofResult {
                    key,
                    record,
                    from_cache: true,
                });
            }
            Lookup::Stale(_) | Lookup::Missing => {}
        }

        self.stats.verifier_runs += 1;
        let started = std::time::Instant::now();
        let outcome = self.verifier.check(artifact, obligation);
        let elapsed_ms = started.elapsed().as_millis() as u64;
        let mut record = record_of(obligation, ctx, &outcome, self.verifier.name(), &self.verifier.version(), elapsed_ms);
        record.verification_key = key.clone();
        self.ledger.append(record.clone())?;
        self.stats.recorded += 1;
        Ok(ProofResult {
            key,
            record,
            from_cache: false,
        })
    }
}

fn record_of(
    obligation: &Obligation,
    ctx: &ProofContext,
    outcome: &VerifierOutcome,
    tool: &str,
    tool_version: &str,
    elapsed_ms: u64,
) -> EvidenceRecord {
    let mut assumptions = if obligation.assumptions.is_empty() {
        vec!["no assumption was declared by the obligation author".to_owned()]
    } else {
        obligation.assumptions.clone()
    };
    assumptions.push(format!(
        "declared domain: {} explicit input(s); nothing outside it is covered",
        obligation.inputs.len()
    ));
    assumptions.push(format!(
        "trust base: rustc-built artifact, the native loader, and this harness ({tool} {tool_version})"
    ));
    assumptions.push(format!("verifier wall time: {elapsed_ms} ms"));

    let (status, passed, counterexample) = match outcome {
        VerifierOutcome::Proved => (VerificationStatus::Proved, true, None),
        VerifierOutcome::Failed { counterexample } => {
            (VerificationStatus::Tested, false, Some(counterexample.clone()))
        }
        VerifierOutcome::Unsupported { detail } => {
            assumptions.push(format!("backend unsupported: {detail}"));
            (VerificationStatus::Unknown, false, None)
        }
        VerifierOutcome::Timeout { after_ms } => {
            assumptions.push(format!("timeout after {after_ms} ms: the domain is not covered"));
            (VerificationStatus::Timeout, false, None)
        }
        VerifierOutcome::Unknown { detail } => {
            assumptions.push(format!("backend unknown: {detail}"));
            (VerificationStatus::Unknown, false, None)
        }
    };

    EvidenceRecord {
        schema_version: EVIDENCE_SCHEMA_VERSION,
        // Replaced by the caller with the exact key; a placeholder here would be a
        // silent mismatch waiting to happen.
        verification_key: VerificationKey::new(sha256_hex(b"placeholder")),
        status,
        property: obligation.describe(),
        assumptions,
        tool: tool.to_owned(),
        tool_version: tool_version.to_owned(),
        source_snapshot_id: ctx.source_snapshot_id.clone(),
        build_context_id: ctx.build_context_id.clone(),
        artifact_id: ctx.artifact_id.clone(),
        passed,
        counterexample,
        recorded_at_unix: ravum_protocol::now_unix(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::obligation::Claim;
    use crate::verifier::VerifierOutcome;
    use ravum_protocol::{ArtifactId, BuildContextId, SnapshotId};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    struct CountingVerifier {
        outcome: VerifierOutcome,
        runs: Arc<AtomicU64>,
    }

    impl Verifier for CountingVerifier {
        fn name(&self) -> &str {
            "counting"
        }
        fn version(&self) -> String {
            "1".to_owned()
        }
        fn check(&self, _artifact: &Path, _obligation: &Obligation) -> VerifierOutcome {
            self.runs.fetch_add(1, Ordering::SeqCst);
            self.outcome.clone()
        }
    }

    fn ctx() -> ProofContext {
        ProofContext::new(
            SnapshotId::new("snap"),
            BuildContextId::new("build"),
            ArtifactId::new("artifact"),
            "counting",
            "1",
        )
    }

    fn broker_with(outcome: VerifierOutcome, runs: Arc<AtomicU64>) -> ProofBroker {
        let path = std::env::temp_dir()
            .join(format!("ravum-broker-{}-{}.jsonl", std::process::id(), runs.load(Ordering::SeqCst)));
        let _ = std::fs::remove_file(&path);
        ProofBroker::new(
            Box::new(CountingVerifier { outcome, runs }),
            EvidenceLedger::open(path).unwrap(),
        )
    }

    #[test]
    fn second_identical_request_is_served_from_cache() {
        let runs = Arc::new(AtomicU64::new(0));
        let mut broker = broker_with(VerifierOutcome::Proved, Arc::clone(&runs));
        let obligation = Obligation::new("o", Claim::Finite, vec![1.0]);

        let first = broker.prove(Path::new("a.dll"), &obligation, &ctx()).unwrap();
        assert!(!first.from_cache);
        let second = broker.prove(Path::new("a.dll"), &obligation, &ctx()).unwrap();
        assert!(second.from_cache);
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(broker.stats().cache_hits, 1);
    }

    #[test]
    fn failure_is_cached_and_stays_a_failure() {
        let runs = Arc::new(AtomicU64::new(0));
        let mut broker = broker_with(
            VerifierOutcome::Failed {
                counterexample: "f(1) = -1".to_owned(),
            },
            Arc::clone(&runs),
        );
        let obligation = Obligation::new("o", Claim::NonNegative, vec![1.0]);
        let first = broker.prove(Path::new("a.dll"), &obligation, &ctx()).unwrap();
        assert_eq!(first.record.counterexample.as_deref(), Some("f(1) = -1"));
        assert!(!first.record.authorizes_activation());
        let second = broker.prove(Path::new("a.dll"), &obligation, &ctx()).unwrap();
        assert!(second.from_cache);
        assert!(!second.record.authorizes_activation());
        assert_eq!(runs.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn changing_the_artifact_forces_a_new_run() {
        let runs = Arc::new(AtomicU64::new(0));
        let mut broker = broker_with(VerifierOutcome::Proved, Arc::clone(&runs));
        let obligation = Obligation::new("o", Claim::Finite, vec![1.0]);
        broker.prove(Path::new("a.dll"), &obligation, &ctx()).unwrap();
        let mut other = ctx();
        other.artifact_id = ArtifactId::new("other");
        broker.prove(Path::new("a.dll"), &obligation, &other).unwrap();
        assert_eq!(runs.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn unsupported_backend_never_authorizes_activation() {
        let runs = Arc::new(AtomicU64::new(0));
        let mut broker = broker_with(
            VerifierOutcome::Unsupported {
                detail: "no prover".to_owned(),
            },
            runs,
        );
        let obligation = Obligation::new("o", Claim::Finite, vec![1.0]);
        let result = broker.prove(Path::new("a.dll"), &obligation, &ctx()).unwrap();
        assert_eq!(result.record.status, VerificationStatus::Unknown);
        assert!(!result.record.authorizes_activation());
    }

    #[test]
    fn timeout_never_authorizes_activation() {
        let runs = Arc::new(AtomicU64::new(0));
        let mut broker = broker_with(
            VerifierOutcome::Timeout { after_ms: 5 },
            runs,
        );
        let obligation = Obligation::new("o", Claim::Finite, vec![1.0]);
        let result = broker.prove(Path::new("a.dll"), &obligation, &ctx()).unwrap();
        assert_eq!(result.record.status, VerificationStatus::Timeout);
        assert!(!result.record.authorizes_activation());
    }
}
