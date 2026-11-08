//! The M1 loop, in one place: analyze → build → smoke-verify → re-check freshness → publish.
//!
//! The order is deliberate. Freshness is re-checked *after* the expensive stages,
//! because that is exactly when a result can become stale. Every refusal leaves
//! the previously active module untouched.
//!
//! `prepare_candidate` stops before publication so a caller can publish either
//! in-process (`run_once`) or in another process (the demo host over IPC) without
//! duplicating the stage logic.

use ravum_patch_host::{prepare, ExpectedAbi, PatchHost};
use ravum_proof::{Obligation, ProofBroker};
use ravum_protocol::{
    ActivationReport, ActivationStatus, ArtifactId, BuildOutput, EvidenceRecord, RejectReason,
    SnapshotId, StageTimings, VerificationKey, VerificationStatus, EVIDENCE_SCHEMA_VERSION,
};
use ravum_rust_adapter::{AdapterError, BuildOutcome, CheckOutcome, RustAdapter};
use ravum_snapshot::SourceSnapshot;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Inputs used by the M1 smoke contract. They are *not* a functional oracle.
pub const SMOKE_INPUTS: [f64; 5] = [0.0, 1.0, -1.0, 2.5, 1_000_000.0];

pub fn smoke_verification_key() -> VerificationKey {
    VerificationKey::new(ravum_protocol::sha256_hex(
        b"ravum-smoke-v1\0finite\0deterministic\0f64->f64",
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopOutcome {
    /// Every gate passed and the candidate is ready to be published.
    CandidateReady,
    /// The candidate was published by this process.
    Activated,
    /// Analysis found errors: nothing was built.
    RefusedByCheck { errors: usize },
    /// Compilation or link failed: nothing was loaded.
    RefusedByBuild,
    /// The artifact loaded but the smoke contract did not hold.
    RefusedByVerification { detail: String },
    /// A declared obligation was not satisfied (failed, unknown, timeout, unsupported).
    RefusedByProof { obligation: String, status: String },
    /// The snapshot changed while the pipeline was running.
    RefusedAsStale { built_from: SnapshotId, current: SnapshotId },
    /// The host refused the candidate (ABI, manifest, verification, ...).
    RefusedByHost { reason: RejectReason },
    /// Toolchain/IO problem; never silently treated as success.
    Error { detail: String },
}

impl LoopOutcome {
    pub fn is_refusal(&self) -> bool {
        matches!(
            self,
            LoopOutcome::RefusedByCheck { .. }
                | LoopOutcome::RefusedByBuild
                | LoopOutcome::RefusedByVerification { .. }
                | LoopOutcome::RefusedByProof { .. }
                | LoopOutcome::RefusedAsStale { .. }
                | LoopOutcome::RefusedByHost { .. }
                | LoopOutcome::Error { .. }
        )
    }

    pub fn label(&self) -> &'static str {
        match self {
            LoopOutcome::CandidateReady => "candidate_ready",
            LoopOutcome::Activated => "activated",
            LoopOutcome::RefusedByCheck { .. } => "refused_check",
            LoopOutcome::RefusedByBuild => "refused_build",
            LoopOutcome::RefusedByVerification { .. } => "refused_verification",
            LoopOutcome::RefusedByProof { .. } => "refused_proof",
            LoopOutcome::RefusedAsStale { .. } => "refused_stale",
            LoopOutcome::RefusedByHost { .. } => "refused_host",
            LoopOutcome::Error { .. } => "error",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoopReport {
    pub snapshot_id: SnapshotId,
    pub outcome: LoopOutcome,
    pub check: Option<CheckOutcome>,
    pub build: Option<BuildOutput>,
    pub evidence: Option<EvidenceRecord>,
    /// Verdicts for the pipeline's declared obligations, in declaration order.
    pub proofs: Vec<EvidenceRecord>,
    pub activation: Option<ActivationReport>,
    pub timings: StageTimings,
}

impl LoopReport {
    pub fn activated(&self) -> bool {
        matches!(self.outcome, LoopOutcome::Activated)
    }

    pub fn artifact_path(&self) -> Option<&str> {
        self.build.as_ref().map(|b| b.artifact_path.as_str())
    }

    pub fn artifact_id(&self) -> Option<&ArtifactId> {
        self.build.as_ref().map(|b| &b.artifact_id)
    }
}

pub struct Pipeline {
    adapter: RustAdapter,
    obligations: Vec<Obligation>,
}

enum Stage<T> {
    Ok(T),
    Adapter(AdapterError),
}

impl Pipeline {
    pub fn new(rustc: impl Into<PathBuf>, work_dir: impl Into<PathBuf>) -> Self {
        Self {
            adapter: RustAdapter::new(rustc, work_dir),
            obligations: Vec::new(),
        }
    }

    /// Declare the obligations every candidate must satisfy before publication.
    ///
    /// Declaring obligations without providing a `ProofBroker` at run time is an
    /// error, never a silent skip.
    pub fn with_obligations(mut self, obligations: Vec<Obligation>) -> Self {
        self.obligations = obligations;
        self
    }

    pub fn obligations(&self) -> &[Obligation] {
        &self.obligations
    }

    /// Turn off artifact reuse. Used by the measurement harness so that every
    /// measured build is a real `rustc` invocation.
    pub fn without_artifact_cache(mut self) -> Self {
        self.adapter.set_artifact_cache(false);
        self
    }

    pub fn adapter(&self) -> &RustAdapter {
        &self.adapter
    }

    pub fn analyze(&self, snapshot: &SourceSnapshot) -> Result<CheckOutcome, AdapterError> {
        self.adapter.check(snapshot)
    }

    pub fn build(&self, snapshot: &SourceSnapshot) -> Result<BuildOutcome, AdapterError> {
        let out_dir = self.adapter.work_dir().join("artifacts");
        self.adapter.build_cdylib(snapshot, &out_dir)
    }

    /// Run every stage up to (but excluding) publication.
    pub fn prepare_candidate(
        &self,
        snapshot: &SourceSnapshot,
        current: &dyn Fn() -> SnapshotId,
    ) -> LoopReport {
        self.prepare_candidate_with_proofs(snapshot, current, None)
    }

    /// Same, with an optional proof broker for the declared obligations.
    pub fn prepare_candidate_with_proofs(
        &self,
        snapshot: &SourceSnapshot,
        current: &dyn Fn() -> SnapshotId,
        mut broker: Option<&mut ProofBroker>,
    ) -> LoopReport {
        let mut report = LoopReport {
            snapshot_id: snapshot.id().clone(),
            outcome: LoopOutcome::Error {
                detail: "unstarted".to_owned(),
            },
            check: None,
            build: None,
            evidence: None,
            proofs: Vec::new(),
            activation: None,
            timings: StageTimings::default(),
        };
        let mut timings = StageTimings::default();

        let check = match self.stage(self.analyze(snapshot)) {
            Stage::Ok(c) => c,
            Stage::Adapter(e) => {
                report.outcome = LoopOutcome::Error { detail: e.0 };
                return report;
            }
        };
        timings.check_ms = check.check_ms;
        report.check = Some(check.clone());
        if check.diagnostics.has_errors() {
            report.outcome = LoopOutcome::RefusedByCheck {
                errors: check.diagnostics.error_count(),
            };
            report.timings = timings;
            return report;
        }

        let build = match self.stage(self.build(snapshot)) {
            Stage::Ok(b) => b,
            Stage::Adapter(e) => {
                report.outcome = LoopOutcome::Error { detail: e.0 };
                report.timings = timings;
                return report;
            }
        };
        timings.build_ms = build.build.build_ms;
        report.build = Some(build.build.clone());
        if !build.success {
            report.outcome = LoopOutcome::RefusedByBuild;
            report.timings = timings;
            return report;
        }

        let verify_started = Instant::now();
        let evidence = match smoke_verify(&build.build) {
            Ok(e) => e,
            Err(reason) => {
                timings.verify_ms = verify_started.elapsed().as_millis() as u64;
                report.outcome = LoopOutcome::RefusedByHost { reason };
                report.timings = timings;
                return report;
            }
        };
        timings.verify_ms = verify_started.elapsed().as_millis() as u64;
        report.evidence = Some(evidence.clone());
        if !evidence.authorizes_activation() {
            report.outcome = LoopOutcome::RefusedByVerification {
                detail: "smoke contract did not hold".to_owned(),
            };
            report.timings = timings;
            return report;
        }

        // Declared obligations must be satisfied by the same artifact. The verdicts
        // are evidence, not decoration: an unsatisfied one blocks publication.
        if !self.obligations.is_empty() {
            let Some(broker) = broker.as_deref_mut() else {
                report.outcome = LoopOutcome::Error {
                    detail: "obligations are declared but no proof broker was provided; refusing to skip verification"
                        .to_owned(),
                };
                report.timings = timings;
                return report;
            };
            let proof_started = Instant::now();
            let ctx = broker.context_for(&build.build);
            let mut failing: Option<(String, String)> = None;
            for obligation in &self.obligations {
                match broker.prove(Path::new(&build.build.artifact_path), obligation, &ctx) {
                    Ok(result) => {
                        if failing.is_none() && !result.record.authorizes_activation() {
                            failing =
                                Some((obligation.id.clone(), result.record.status.as_str().to_owned()));
                        }
                        report.proofs.push(result.record);
                    }
                    Err(e) => {
                        timings.proof_ms = proof_started.elapsed().as_millis() as u64;
                        report.outcome = LoopOutcome::Error {
                            detail: format!("evidence ledger failure: {e}"),
                        };
                        report.timings = timings;
                        return report;
                    }
                }
            }
            timings.proof_ms = proof_started.elapsed().as_millis() as u64;
            if let Some((obligation, status)) = failing {
                report.outcome = LoopOutcome::RefusedByProof { obligation, status };
                report.timings = timings;
                return report;
            }
        }

        // Re-check freshness after the expensive stages (INV-07).
        let now = current();
        if !build.build.is_current_for(&now) {
            report.outcome = LoopOutcome::RefusedAsStale {
                built_from: build.build.source_snapshot_id.clone(),
                current: now,
            };
            report.timings = timings;
            return report;
        }

        report.outcome = LoopOutcome::CandidateReady;
        report.timings = timings;
        report
    }

    /// Full in-process iteration: prepare, then publish into `host`.
    pub fn run_once(
        &self,
        host: &PatchHost,
        snapshot: &SourceSnapshot,
        current: &dyn Fn() -> SnapshotId,
    ) -> LoopReport {
        self.run_once_with_proofs(host, snapshot, current, None)
    }

    /// Full in-process iteration with obligation checking before publication.
    pub fn run_once_with_proofs(
        &self,
        host: &PatchHost,
        snapshot: &SourceSnapshot,
        current: &dyn Fn() -> SnapshotId,
        broker: Option<&mut ProofBroker>,
    ) -> LoopReport {
        let mut report = self.prepare_candidate_with_proofs(snapshot, current, broker);
        if !matches!(report.outcome, LoopOutcome::CandidateReady) {
            return report;
        }
        let (Some(build), Some(evidence)) = (report.build.clone(), report.evidence.clone()) else {
            report.outcome = LoopOutcome::Error {
                detail: "candidate reported ready without build or evidence".to_owned(),
            };
            return report;
        };
        match prepare(&PathBuf::from(&build.artifact_path), host.expected_abi()) {
            Ok(prepared) if prepared.module.artifact_id == build.artifact_id => {
                let activation = host.apply(prepared, &build, Some(&evidence), &current());
                merge_activation(&mut report, activation);
            }
            Ok(prepared) => {
                report.outcome = LoopOutcome::RefusedByHost {
                    reason: RejectReason::ArtifactMismatch {
                        expected: build.artifact_id.clone(),
                        found: prepared.module.artifact_id.clone(),
                    },
                };
            }
            Err(reason) => {
                report.outcome = LoopOutcome::RefusedByHost { reason };
            }
        }
        report
    }

    fn stage<T>(&self, result: Result<T, AdapterError>) -> Stage<T> {
        match result {
            Ok(v) => Stage::Ok(v),
            Err(e) => Stage::Adapter(e),
        }
    }
}

/// Fold a publication result (local or received over IPC) into a prepared report.
pub fn merge_activation(report: &mut LoopReport, activation: ActivationReport) {
    report.timings.load_ms = activation.timings.load_ms;
    report.timings.activate_ms = activation.timings.activate_ms;
    report.timings.total_ms = report
        .timings
        .check_ms
        .saturating_add(report.timings.build_ms)
        .saturating_add(report.timings.verify_ms)
        .saturating_add(report.timings.proof_ms)
        .saturating_add(report.timings.load_ms)
        .saturating_add(report.timings.activate_ms);
    report.outcome = match (&activation.status, &activation.reject_reason) {
        (ActivationStatus::Activated, _) => LoopOutcome::Activated,
        (_, Some(reason)) => LoopOutcome::RefusedByHost {
            reason: reason.clone(),
        },
        _ => LoopOutcome::Error {
            detail: "host returned an incomplete report".to_owned(),
        },
    };
    report.activation = Some(activation);
}

/// Weak, honest contract: the candidate must load, return finite values for a
/// fixed input set, and be deterministic across two consecutive calls.
///
/// This is `TESTED`, never `PROVED`. It does not check functional correctness,
/// thread safety, memory safety or performance.
pub fn smoke_verify(build: &BuildOutput) -> Result<EvidenceRecord, RejectReason> {
    let prepared = prepare(&PathBuf::from(&build.artifact_path), &ExpectedAbi::default())?;
    if prepared.module.artifact_id != build.artifact_id {
        return Err(RejectReason::ArtifactMismatch {
            expected: build.artifact_id.clone(),
            found: prepared.module.artifact_id.clone(),
        });
    }

    let mut failures: Vec<String> = Vec::new();
    for input in SMOKE_INPUTS {
        let first = prepared.module.call(input);
        let second = prepared.module.call(input);
        if !first.is_finite() {
            failures.push(format!("non-finite result for input {input}"));
        }
        if first != second {
            failures.push(format!(
                "non-deterministic result for input {input}: {first} != {second}"
            ));
        }
    }

    let mut evidence = EvidenceRecord {
        schema_version: EVIDENCE_SCHEMA_VERSION,
        verification_key: smoke_verification_key(),
        status: VerificationStatus::Tested,
        property: "smoke: finite + deterministic on the fixed M1 input set".to_owned(),
        assumptions: vec![
            "single-threaded call".to_owned(),
            "no panic for these inputs".to_owned(),
            "does not prove functional correctness, memory safety or real-time behaviour"
                .to_owned(),
        ],
        tool: "ravum-coordinator".to_owned(),
        tool_version: env!("CARGO_PKG_VERSION").to_owned(),
        source_snapshot_id: build.source_snapshot_id.clone(),
        build_context_id: build.build_context_id.clone(),
        artifact_id: build.artifact_id.clone(),
        passed: failures.is_empty(),
        counterexample: None,
        recorded_at_unix: ravum_protocol::now_unix(),
    };
    evidence.assumptions.extend(failures);
    Ok(evidence)
}
