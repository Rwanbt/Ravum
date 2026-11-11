//! Non-regression for COLLAB-1: **without an explicit opt-in**, `ravum-collab`
//! keeps its historical decisions.
//!
//! The historical COLLAB-1 demo (`scripts/demo-collab.ps1`) uses a workspace of
//! two loose `.rs` files and no `Cargo.toml`: it is not a crate and cannot be
//! compiled as one. Semantic verification is therefore opt-in; requesting it on
//! such a workspace must fail closed (`MANUAL_REQUIRED`), but the default must not
//! change any existing decision.

mod support;

use support::*;

/// Agent A: change `helper` only (COLLAB-1 fixture).
const PATCH_A: &str = "+++ b/two_functions.rs\n@@ -24,4 +24,4 @@\n /// Calcule un decalage simple.\n fn helper(x: f64) -> f64 {\n-    x + 1.0\n+    x + 10.0\n }\n";

/// Agent B: change `ravum_compute` only.
const PATCH_B: &str = "+++ b/two_functions.rs\n@@ -29,5 +29,5 @@\n /// Point d'entree patchable.\n #[no_mangle]\n pub extern \"C\" fn ravum_compute(x: f64) -> f64 {\n-    helper(x) * 2.0\n+    helper(x) * 3.0\n }\n";

fn collab_fixture_repo(name: &str) -> std::path::PathBuf {
    let two_functions =
        std::fs::read_to_string(workspace_root().join("fixtures/collab/two_functions.rs"))
            .expect("read COLLAB-1 two_functions fixture");
    let notes = std::fs::read_to_string(workspace_root().join("fixtures/collab/notes.rs"))
        .expect("read COLLAB-1 notes fixture");
    init_repo(
        name,
        &[("two_functions.rs", two_functions), ("notes.rs", notes)],
    )
}

#[test]
fn without_opt_in_the_collab1_non_crate_workspace_keeps_auto_safe_decisions() {
    let repo = collab_fixture_repo("collab1-non-regression-default");
    let base = run_base(&repo);
    let proposals = write_proposals(
        &repo,
        &[
            (
                "01-a.json",
                proposal_json(
                    &base,
                    "agent-a",
                    "tune-helper",
                    "two_functions.rs",
                    &["helper"],
                    PATCH_A,
                ),
            ),
            (
                "02-b.json",
                proposal_json(
                    &base,
                    "agent-b",
                    "tune-compute",
                    "two_functions.rs",
                    &["ravum_compute"],
                    PATCH_B,
                ),
            ),
        ],
    );
    let scratch = repo.with_extension("scratch");
    let run = run_integrate(&repo, &proposals, &["--commit"], &scratch);

    assert_eq!(
        run.code, 0,
        "stderr: {}\nstdout: {}",
        run.stderr, run.stdout
    );
    assert_eq!(
        run.decision_kinds(),
        vec!["AUTO_SAFE", "AUTO_SAFE"],
        "the historical textual decisions must be preserved: {:?}",
        run.decisions()
    );
    assert_eq!(run.summary("auto_safe"), 2);
    assert_eq!(run.report["verification"]["mode"], "textual_only");
    assert_eq!(run.report["verification"]["requested"], false);
    assert_eq!(
        run.report["verification"]["verifier"],
        serde_json::Value::Null
    );

    // A textual-mode acceptance is recorded, but never as a fake "passed" proof.
    let accepted = run.accepted();
    assert_eq!(accepted.len(), 2);
    for record in accepted {
        assert_eq!(record["verification_required"], false);
        assert_eq!(record["verification"]["status"], "skipped");
        assert!(record["verification"]["checks"]
            .as_array()
            .unwrap()
            .is_empty());
    }

    // The single Git writer still publishes both changes.
    let head = git(&repo, &["show", "HEAD:two_functions.rs"]);
    assert!(head.contains("x + 10.0"), "HEAD:\n{head}");
    assert!(head.contains("helper(x) * 3.0"), "HEAD:\n{head}");
    assert_eq!(git(&repo, &["rev-list", "--count", "HEAD"]).trim(), "2");
    assert_eq!(git(&repo, &["status", "--porcelain"]).trim(), "");
}

#[test]
fn requesting_semantic_verification_on_a_non_crate_workspace_fails_closed() {
    let repo = collab_fixture_repo("collab1-non-regression-verify");
    let base = run_base(&repo);
    let proposals = write_proposals(
        &repo,
        &[(
            "01-a.json",
            proposal_json(
                &base,
                "agent-a",
                "tune-helper",
                "two_functions.rs",
                &["helper"],
                PATCH_A,
            ),
        )],
    );
    let scratch = repo.with_extension("scratch");
    let head_before = git(&repo, &["rev-parse", "HEAD"]).trim().to_owned();
    let run = run_integrate(&repo, &proposals, &["--verify"], &scratch);

    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    assert_eq!(run.decision_kinds(), vec!["MANUAL_REQUIRED"]);
    assert!(run.accepted().is_empty());
    let reasons = run.reasons(0);
    assert!(
        reasons.contains("cannot infer a single crate root"),
        "fail-closed reason must name the missing crate root: {reasons}"
    );
    assert!(reasons.contains("fail-closed"), "reasons: {reasons}");
    assert_eq!(run.report["verification"]["mode"], "semantic");
    assert_no_unexpected_state_change(&repo, &head_before);
}
