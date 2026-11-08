//! COLLAB-2 — two agents modify two independent functions of the same file, the
//! merged snapshot is really compiled (and its tests really executed), and the
//! Git publication keeps both changes.
//!
//! Everything here goes through the real `ravum-collab` binary with the explicit
//! `--verify` / `--verify-tests` opt-in, so what is tested is the production
//! pipeline (CLI flags, injected `rustc` verifier, single `GitWriter`), not an
//! in-process arrangement.

mod support;

use support::*;

fn demo_crate_repo(name: &str) -> std::path::PathBuf {
    init_repo(
        name,
        &[
            ("Cargo.toml", fixture("demo-crate/Cargo.toml")),
            ("src/lib.rs", fixture("demo-crate/src/lib.rs")),
        ],
    )
}

fn name_collision_repo(name: &str) -> std::path::PathBuf {
    init_repo(name, &[("src/lib.rs", fixture("name_collision/src/lib.rs"))])
}

fn patch(name: &str) -> String {
    fixture(name)
}

#[test]
fn two_independent_functions_are_auto_safe_only_after_a_real_compile_and_test_run() {
    let repo = demo_crate_repo("collab2-two-functions");
    let base = run_base(&repo);
    let proposals = write_proposals(
        &repo,
        &[
            (
                "01-a.json",
                proposal_json(&base, "agent-a", "alpha", "src/lib.rs", &["alpha"], &patch("demo-crate/a.patch")),
            ),
            (
                "02-b.json",
                proposal_json(&base, "agent-b", "beta", "src/lib.rs", &["beta"], &patch("demo-crate/b.patch")),
            ),
        ],
    );
    let scratch = repo.with_extension("scratch");
    let run = run_integrate(
        &repo,
        &proposals,
        &["--verify-tests", "--commit", "--author", "agent-x", "--email", "agent-x@ravum.invalid"],
        &scratch,
    );

    assert_eq!(run.code, 0, "stderr: {}\nstdout: {}", run.stderr, run.stdout);
    assert_eq!(run.decision_kinds(), vec!["AUTO_SAFE", "AUTO_SAFE"]);
    assert_eq!(run.summary("auto_safe"), 2);
    assert_eq!(run.report["verification"]["mode"], "semantic");
    assert_eq!(run.report["verification"]["requested"], true);
    assert_eq!(run.report["verification"]["verifier"], "rustc-cli");
    assert_eq!(run.report["verification"]["test_execution"], true);

    // Each accepted change carries a passed verification bound to its snapshots.
    let accepted = run.accepted();
    assert_eq!(accepted.len(), 2);
    for (index, record) in accepted.iter().enumerate() {
        assert_eq!(record["decision"], "AUTO_SAFE");
        assert_eq!(record["verification_required"], true);
        assert_eq!(record["verification"]["status"], "passed");
        assert_eq!(record["journal_seq"], (index + 1) as u64);
        assert_ne!(record["snapshot_before"], record["snapshot_after"]);
        assert_eq!(record["paths"][0], "src/lib.rs");
        let checks = record["verification"]["checks"].as_array().expect("checks");
        let kinds: Vec<&str> = checks
            .iter()
            .map(|check| check["kind"].as_str().unwrap_or_default())
            .collect();
        // A behavioural change in a snapshot that ships tests requires the test
        // harness to compile *and* run, not only a metadata check.
        assert!(kinds.contains(&"metadata_check"), "kinds: {kinds:?}");
        assert!(kinds.contains(&"test_compile"), "kinds: {kinds:?}");
        assert!(kinds.contains(&"test_run"), "kinds: {kinds:?}");
        for check in checks {
            assert_eq!(check["status"], "passed", "check: {check}");
            assert!(
                check["tool"].as_str().unwrap_or_default().contains("rustc"),
                "check must be attributed to rustc: {check}"
            );
            assert_eq!(
                check["evidence_id"].as_str().unwrap().len(),
                64,
                "check: {check}"
            );
        }
        let test_run = checks
            .iter()
            .find(|check| check["kind"] == "test_run")
            .expect("test_run check");
        assert!(
            test_run["summary"].as_str().unwrap().contains("test result: ok"),
            "the merged test harness must really have run: {test_run}"
        );
        assert_eq!(record["verification"]["evidence_id"].as_str().unwrap().len(), 64);
        assert_eq!(record["evidence_ids"].as_array().unwrap().len(), 4);
    }

    // Both changes are preserved by the single transactional publication.
    let head = git(&repo, &["show", "HEAD:src/lib.rs"]);
    assert!(head.contains("    10"), "HEAD:\n{head}");
    assert!(head.contains("    20"), "HEAD:\n{head}");
    assert_eq!(git(&repo, &["rev-list", "--count", "HEAD"]).trim(), "2");
    assert_eq!(git(&repo, &["status", "--porcelain"]).trim(), "");
    let body = git(&repo, &["log", "-1", "--format=%B"]);
    assert!(body.contains("Ravum-Agent: agent-a"), "body:\n{body}");
    assert!(body.contains("Ravum-Agent: agent-b"), "body:\n{body}");
    assert!(body.contains("Ravum-Evidence:"), "body:\n{body}");
    assert!(body.contains("Ravum-Check-Evidence:"), "body:\n{body}");

    // The durable journal carries the verification evidence next to the decision.
    let journal = journal_lines(&scratch);
    assert_eq!(journal.len(), 2);
    for entry in &journal {
        let reasons = entry["decision"]["reasons"]
            .as_array()
            .expect("reasons")
            .iter()
            .filter_map(|reason| reason.as_str())
            .collect::<Vec<_>>()
            .join(" | ");
        assert!(
            reasons.contains("semantic verification: passed"),
            "journaled reasons: {reasons}"
        );
        assert!(reasons.contains("metadata_check: "), "journaled reasons: {reasons}");
    }
}

#[test]
fn a_merged_snapshot_that_does_not_compile_rejects_the_faulty_proposal_and_publishes_only_the_rest() {
    let repo = name_collision_repo("collab2-compile-failure");
    let base = run_base(&repo);
    let proposals = write_proposals(
        &repo,
        &[
            (
                "01-a.json",
                proposal_json(&base, "agent-a", "rename-a", "src/lib.rs", &["slot_a"], &patch("name_collision/a.patch")),
            ),
            (
                "02-b.json",
                proposal_json(&base, "agent-b", "rename-b", "src/lib.rs", &["slot_b"], &patch("name_collision/b.patch")),
            ),
        ],
    );
    let scratch = repo.with_extension("scratch");
    let run = run_integrate(&repo, &proposals, &["--verify", "--commit"], &scratch);

    assert_eq!(run.code, 0, "stderr: {}\nstdout: {}", run.stderr, run.stdout);
    assert_eq!(run.decision_kinds(), vec!["AUTO_SAFE", "REJECT"]);
    assert_eq!(run.summary("auto_safe"), 1);
    assert_eq!(run.summary("reject"), 1);

    // The refusal is an explicit semantic verdict with compiler evidence.
    let reasons = run.reasons(1);
    assert!(
        reasons.contains("does not compile with rustc"),
        "reasons: {reasons}"
    );
    assert!(
        reasons.contains("E0428"),
        "the compiler diagnostic code must be journaled: {reasons}"
    );
    assert!(reasons.contains("evidence"), "reasons: {reasons}");
    assert_eq!(run.accepted().len(), 1);
    assert_eq!(run.accepted()[0]["proposal_id"], "agent-a/rename-a");

    // Only agent A's change is published; the faulty proposal changed nothing.
    let head = git(&repo, &["show", "HEAD:src/lib.rs"]);
    assert!(head.contains("fn helper2() -> u32 {"), "HEAD:\n{head}");
    assert!(head.contains("fn slot_b() -> u32 {"), "HEAD:\n{head}");
    assert!(!head.contains("fn slot_a() -> u32 {"), "HEAD:\n{head}");
    assert_eq!(git(&repo, &["rev-list", "--count", "HEAD"]).trim(), "2");
    assert_eq!(git(&repo, &["status", "--porcelain"]).trim(), "");

    let journal = journal_lines(&scratch);
    assert_eq!(journal.len(), 2);
    let refusal = journal[1]["decision"]["reasons"]
        .as_array()
        .expect("reasons")
        .iter()
        .filter_map(|reason| reason.as_str())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        refusal.contains("E0428"),
        "the refusal must be durable with its proof: {refusal}"
    );
}

#[test]
fn a_merge_needing_semantic_verification_that_is_not_opted_in_is_never_silently_accepted() {
    // Even with a real crate, the *default* remains COLLAB-1: `AUTO_SAFE` does not
    // claim the merge was compiled. This is what keeps existing callers intact;
    // the verification is one explicit flag away (covered above).
    let repo = demo_crate_repo("collab2-opt-in-default");
    let base = run_base(&repo);
    let proposals = write_proposals(
        &repo,
        &[(
            "01-a.json",
            proposal_json(&base, "agent-a", "alpha", "src/lib.rs", &["alpha"], &patch("demo-crate/a.patch")),
        )],
    );
    let scratch = repo.with_extension("scratch");
    let run = run_integrate(&repo, &proposals, &[], &scratch);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    assert_eq!(run.decision_kinds(), vec!["AUTO_SAFE"]);
    assert_eq!(run.report["verification"]["mode"], "textual_only");
    let record = &run.accepted()[0];
    assert_eq!(record["verification_required"], false);
    assert_eq!(record["verification"]["status"], "skipped");
}

