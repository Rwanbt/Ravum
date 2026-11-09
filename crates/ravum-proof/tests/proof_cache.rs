//! M2 acceptance tests: real artifacts, real counterexamples, and cache
//! invalidation proven by mutation rather than asserted by comment.
//!
//! Every test builds a real native library with the public `rustc` CLI.

use ravum_proof::{
    Claim, EvidenceLedger, ExhaustiveVerifier, KaniVerifier, Lookup, Obligation, ProofBroker,
    ProofContext, Verifier,
};
use ravum_protocol::{BuildOutput, VerificationStatus};
use ravum_rust_adapter::RustAdapter;
use ravum_snapshot::SourceSnapshot;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn build(tag: &str, fixture: &str) -> BuildOutput {
    let source = root().join("fixtures/proof").join(fixture);
    let snapshot = SourceSnapshot::from_file(&source).expect("read fixture");
    let out = root().join("target/ravum-tests/proof").join(tag);
    let _ = std::fs::remove_dir_all(&out);
    let adapter = RustAdapter::new("rustc", out.join("work"));
    let outcome = adapter
        .build_cdylib(&snapshot, &out.join("artifacts"))
        .expect("adapter error");
    assert!(
        outcome.success,
        "build failed for {fixture}: {:?}",
        outcome.build.diagnostics
    );
    outcome.build
}

fn broker(tag: &str, timeout: Duration) -> ProofBroker {
    let path = root()
        .join("target/ravum-tests/proof-ledgers")
        .join(format!("{tag}.jsonl"));
    let _ = std::fs::remove_file(&path);
    ProofBroker::new(
        Box::new(ExhaustiveVerifier::with_timeout(timeout)),
        EvidenceLedger::open(path).expect("ledger"),
    )
}

#[test]
fn proves_a_bounded_property_and_then_serves_the_cache() {
    let artifact = build("positive-prove", "positive.rs");
    let mut broker = broker("positive-prove", Duration::from_secs(10));
    let ctx = broker.context_for(&artifact);
    let obligation = Obligation::new(
        "compute.non_negative",
        Claim::NonNegative,
        vec![0.0, 1.0, 2.0, 3.0, 4.0],
    )
    .with_assumption("inputs are the exact domain enumerated by the verifier");

    let first = broker
        .prove(Path::new(&artifact.artifact_path), &obligation, &ctx)
        .expect("prove");
    assert_eq!(first.record.status, VerificationStatus::Proved);
    assert!(first.record.passed);
    assert!(first.record.authorizes_activation());
    assert!(!first.from_cache);
    assert!(first
        .record
        .assumptions
        .iter()
        .any(|a| a.contains("declared domain: 5")));

    let second = broker
        .prove(Path::new(&artifact.artifact_path), &obligation, &ctx)
        .expect("prove again");
    assert!(second.from_cache);
    assert_eq!(broker.stats().verifier_runs, 1);
    assert_eq!(broker.stats().cache_hits, 1);
}

#[test]
fn a_single_bad_input_yields_a_precise_counterexample() {
    let artifact = build("negative-ce", "negative.rs");
    let mut broker = broker("negative-ce", Duration::from_secs(10));
    let ctx = broker.context_for(&artifact);
    let obligation = Obligation::new(
        "compute.non_negative",
        Claim::NonNegative,
        vec![0.0, 1.0, 2.0, 3.0],
    );

    let result = broker
        .prove(Path::new(&artifact.artifact_path), &obligation, &ctx)
        .expect("prove");

    assert_eq!(result.record.status, VerificationStatus::Tested);
    assert!(!result.record.passed);
    assert!(!result.record.authorizes_activation());
    let counterexample = result.record.counterexample.expect("counterexample");
    assert!(counterexample.contains("f(2)"), "got: {counterexample}");
    assert!(counterexample.contains("-1"), "got: {counterexample}");
}

#[test]
fn nan_violates_the_finite_claim() {
    let artifact = build("nan-finite", "nan.rs");
    let mut broker = broker("nan-finite", Duration::from_secs(10));
    let ctx = broker.context_for(&artifact);
    let obligation = Obligation::new("compute.finite", Claim::Finite, vec![0.0, 1.0, 2.0]);

    let result = broker
        .prove(Path::new(&artifact.artifact_path), &obligation, &ctx)
        .expect("prove");
    assert!(!result.record.passed);
    assert!(result
        .record
        .counterexample
        .unwrap_or_default()
        .contains("not finite"));
}

#[test]
fn mutating_the_artifact_invalidates_the_cache() {
    let positive = build("mut-artifact-pos", "positive.rs");
    let negative = build("mut-artifact-neg", "negative.rs");
    let mut broker = broker("mut-artifact", Duration::from_secs(10));
    let obligation = Obligation::new("compute.non_negative", Claim::NonNegative, vec![0.0, 2.0]);

    let good = {
        let ctx_pos = broker.context_for(&positive);
        broker
            .prove(Path::new(&positive.artifact_path), &obligation, &ctx_pos)
            .expect("prove positive")
    };
    assert!(good.record.passed);

    let ctx_neg = broker.context_for(&negative);
    assert_ne!(ctx_neg.artifact_id, good.record.artifact_id);
    let bad = broker
        .prove(Path::new(&negative.artifact_path), &obligation, &ctx_neg)
        .expect("prove negative");
    assert!(
        !bad.from_cache,
        "a different artifact must not reuse a verdict"
    );
    assert!(!bad.record.passed);
    assert_eq!(broker.stats().verifier_runs, 2);
}

#[test]
fn mutating_the_domain_or_the_assumptions_invalidates_the_cache() {
    let artifact = build("mut-obligation", "positive.rs");
    let mut broker = broker("mut-obligation", Duration::from_secs(10));
    let ctx = broker.context_for(&artifact);
    let base = Obligation::new("compute.finite", Claim::Finite, vec![0.0, 1.0]);
    broker
        .prove(Path::new(&artifact.artifact_path), &base, &ctx)
        .unwrap();

    let wider_domain = Obligation::new("compute.finite", Claim::Finite, vec![0.0, 1.0, 2.0]);
    assert!(
        !broker
            .prove(Path::new(&artifact.artifact_path), &wider_domain, &ctx)
            .unwrap()
            .from_cache
    );

    let stricter = base.clone().with_assumption("f is pure");
    assert!(
        !broker
            .prove(Path::new(&artifact.artifact_path), &stricter, &ctx)
            .unwrap()
            .from_cache
    );

    assert_eq!(broker.stats().verifier_runs, 3);
}

#[test]
fn mutating_the_toolchain_invalidates_and_marks_the_old_verdict_stale() {
    let artifact = build("mut-toolchain", "positive.rs");
    let mut broker = broker("mut-toolchain", Duration::from_secs(10));
    let ctx = broker.context_for(&artifact);
    let obligation = Obligation::new("compute.finite", Claim::Finite, vec![0.0, 1.0]);

    let first = broker
        .prove(Path::new(&artifact.artifact_path), &obligation, &ctx)
        .unwrap();
    assert!(!first.from_cache);

    let mut upgraded = ctx.clone();
    upgraded.tool_version = "0.2.0".to_owned();
    let second = broker
        .prove(Path::new(&artifact.artifact_path), &obligation, &upgraded)
        .unwrap();
    assert!(
        !second.from_cache,
        "another toolchain must not reuse a verdict"
    );
    assert_ne!(first.key, second.key);

    // The old key is still in the ledger, but not valid for the new context.
    assert!(matches!(
        broker.ledger().lookup(&first.key, &upgraded),
        Lookup::Stale(_)
    ));
}

#[test]
fn exhausting_the_budget_yields_timeout_not_success() {
    let artifact = build("timeout", "slow.rs");
    let mut broker = broker("timeout", Duration::from_millis(100));
    let ctx = broker.context_for(&artifact);
    let obligation = Obligation::new("compute.finite", Claim::Finite, vec![99.0]);

    let result = broker
        .prove(Path::new(&artifact.artifact_path), &obligation, &ctx)
        .expect("prove");
    assert_eq!(result.record.status, VerificationStatus::Timeout);
    assert!(!result.record.authorizes_activation());
}

#[test]
fn an_absent_external_prover_is_unsupported_not_proved() {
    let kani = KaniVerifier::detect();
    let artifact = build("kani", "positive.rs");
    let ctx = ProofContext::new(
        artifact.source_snapshot_id.clone(),
        artifact.build_context_id.clone(),
        artifact.artifact_id.clone(),
        kani.name(),
        kani.version(),
    );
    let outcome = kani.check(
        Path::new(&artifact.artifact_path),
        &Obligation::new("o", Claim::Finite, vec![0.0]),
    );
    assert!(
        matches!(outcome, ravum_proof::VerifierOutcome::Unsupported { .. }),
        "unexpected outcome: {outcome}"
    );
    // Whatever the detection result, no proof is claimed.
    let mut broker = ProofBroker::new(
        Box::new(kani),
        EvidenceLedger::open(root().join("target/ravum-tests/proof-ledgers/kani.jsonl")).unwrap(),
    );
    let result = broker
        .prove(
            Path::new(&artifact.artifact_path),
            &Obligation::new("o", Claim::Finite, vec![0.0]),
            &ctx,
        )
        .unwrap();
    assert!(!result.record.authorizes_activation());
}

#[test]
fn verdicts_are_reused_after_a_broker_restart() {
    let artifact = build("restart", "positive.rs");
    let ledger_path = root().join("target/ravum-tests/proof-ledgers/restart.jsonl");
    let _ = std::fs::remove_file(&ledger_path);
    let obligation = Obligation::new("compute.finite", Claim::Finite, vec![0.0, 1.0]);

    let (key, ctx) = {
        let mut broker = ProofBroker::new(
            Box::new(ExhaustiveVerifier::default()),
            EvidenceLedger::open(&ledger_path).unwrap(),
        );
        let ctx = broker.context_for(&artifact);
        let result = broker
            .prove(Path::new(&artifact.artifact_path), &obligation, &ctx)
            .unwrap();
        assert!(!result.from_cache);
        (result.key, ctx)
    };

    let mut reopened = ProofBroker::new(
        Box::new(ExhaustiveVerifier::default()),
        EvidenceLedger::open(&ledger_path).unwrap(),
    );
    let result = reopened
        .prove(Path::new(&artifact.artifact_path), &obligation, &ctx)
        .unwrap();
    assert!(
        result.from_cache,
        "the ledger must survive a process restart"
    );
    assert_eq!(result.key, key);
    assert_eq!(reopened.stats().verifier_runs, 0);
}
