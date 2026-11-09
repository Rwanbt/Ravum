//! M2 gate tests: a declared obligation decides whether a candidate may be
//! published, and the cache is reused inside the real pipeline.

use ravum_coordinator::{LoopOutcome, Pipeline};
use ravum_patch_host::{ExpectedAbi, PatchHost};
use ravum_proof::{Claim, EvidenceLedger, ExhaustiveVerifier, Obligation, ProofBroker};
use ravum_protocol::{RejectReason, VerificationStatus};
use ravum_snapshot::SourceSnapshot;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(rel: &str) -> SourceSnapshot {
    let path = root().join("fixtures").join(rel);
    SourceSnapshot::from_file(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

fn work(name: &str) -> PathBuf {
    let dir = root().join("target/ravum-tests").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("work dir");
    dir
}

fn broker(dir: &Path) -> ProofBroker {
    ProofBroker::new(
        Box::new(ExhaustiveVerifier::with_timeout(Duration::from_secs(10))),
        EvidenceLedger::open(dir.join("ledger.jsonl")).expect("ledger"),
    )
}

fn obligation(inputs: Vec<f64>) -> Obligation {
    Obligation::new("compute.non_negative", Claim::NonNegative, inputs)
        .with_assumption("the declared domain is the entire claim")
}

fn bootstrap(pipeline: &Pipeline, source: &SourceSnapshot) -> PatchHost {
    let built = pipeline.build(source).expect("adapter");
    assert!(built.success, "fixture must compile: {:?}", built.build.diagnostics);
    PatchHost::bootstrap(ExpectedAbi::default(), Path::new(&built.build.artifact_path))
        .expect("bootstrap")
}

#[test]
fn an_unsatisfied_obligation_blocks_activation_and_keeps_the_old_version() {
    let dir = work("m2-block");
    let positive = fixture("proof/positive.rs");
    let negative = fixture("proof/negative.rs");

    let pipeline = Pipeline::new("rustc", &dir).with_obligations(vec![obligation(vec![
        0.0, 1.0, 2.0,
    ])]);
    let host = bootstrap(&pipeline, &positive);
    assert_eq!(host.call(2.0), 4.0);

    let mut broker = broker(&dir);
    let report = pipeline.run_once_with_proofs(
        &host,
        &negative,
        &|| negative.id().clone(),
        Some(&mut broker),
    );

    match &report.outcome {
        LoopOutcome::RefusedByProof { obligation, status } => {
            assert_eq!(obligation, "compute.non_negative");
            assert_eq!(status, VerificationStatus::Tested.as_str());
        }
        other => panic!("expected RefusedByProof, got {other:?}"),
    }
    assert_eq!(host.call(2.0), 4.0, "the previous version must stay active");
    assert_eq!(host.status().activations, 0);
    assert_eq!(report.proofs.len(), 1);
    let counterexample = report.proofs[0].counterexample.clone().expect("counterexample");
    assert!(counterexample.contains("f(2)"), "got: {counterexample}");
    assert!(!report.proofs[0].authorizes_activation());
}

#[test]
fn a_narrower_declared_domain_lets_the_same_artifact_through() {
    let dir = work("m2-narrow");
    let positive = fixture("proof/positive.rs");
    let negative = fixture("proof/negative.rs");

    let pipeline = Pipeline::new("rustc", &dir).with_obligations(vec![obligation(vec![0.0, 1.0])]);
    let host = bootstrap(&pipeline, &positive);
    let mut broker = broker(&dir);

    let report = pipeline.run_once_with_proofs(
        &host,
        &negative,
        &|| negative.id().clone(),
        Some(&mut broker),
    );

    assert!(report.activated(), "expected activation, got {:?}", report.outcome);
    assert_eq!(report.proofs[0].status, VerificationStatus::Proved);
    // Honest consequence: the gate is exactly as wide as the declared domain.
    assert_eq!(host.call(1.0), 1.0);
    assert_eq!(host.call(2.0), -1.0);
}

#[test]
fn declaring_obligations_without_a_broker_is_an_error_not_a_skip() {
    let dir = work("m2-nobroker");
    let positive = fixture("proof/positive.rs");
    let negative = fixture("proof/negative.rs");

    let pipeline = Pipeline::new("rustc", &dir).with_obligations(vec![obligation(vec![0.0])]);
    let host = bootstrap(&pipeline, &positive);

    let report = pipeline.run_once(&host, &negative, &|| negative.id().clone());
    assert!(
        matches!(report.outcome, LoopOutcome::Error { .. }),
        "got {:?}",
        report.outcome
    );
    assert_eq!(host.call(2.0), 4.0);
    assert_eq!(host.status().activations, 0);
}

#[test]
fn a_repeated_request_inside_the_pipeline_reuses_the_cached_verdict() {
    let dir = work("m2-cache");
    let positive = fixture("proof/positive.rs");
    let negative = fixture("proof/negative.rs");

    let pipeline = Pipeline::new("rustc", &dir).with_obligations(vec![obligation(vec![0.0, 1.0])]);
    let host = bootstrap(&pipeline, &positive);
    let mut broker = broker(&dir);

    let first = pipeline.run_once_with_proofs(
        &host,
        &negative,
        &|| negative.id().clone(),
        Some(&mut broker),
    );
    assert!(first.activated());
    assert_eq!(broker.stats().verifier_runs, 1);
    assert!(
        !first.build.as_ref().expect("build").cached_build,
        "the first build must invoke rustc"
    );

    let second = pipeline.run_once_with_proofs(
        &host,
        &negative,
        &|| negative.id().clone(),
        Some(&mut broker),
    );
    assert_eq!(broker.stats().verifier_runs, 1, "no second verifier run");
    assert_eq!(broker.stats().cache_hits, 1);
    assert!(
        second.build.as_ref().expect("build").cached_build,
        "the artifact must be reused, not relinked over a loaded DLL"
    );
    assert_eq!(
        second.build.as_ref().unwrap().artifact_id,
        first.build.as_ref().unwrap().artifact_id
    );
    assert!(matches!(
        second.activation.as_ref().and_then(|a| a.reject_reason.clone()),
        Some(RejectReason::AlreadyActive { .. })
    ));
}
