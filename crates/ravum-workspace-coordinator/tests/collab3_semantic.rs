//! COLLAB-3 — the ten semantic-conflict cases of the mandate, exercised through
//! the real `ravum-collab` binary with the explicit semantic opt-in.
//!
//! A textually clean merge is never `AUTO_SAFE` by default in semantic mode: it
//! must pass the required checks, and when a required check cannot run the
//! decision is `MANUAL_REQUIRED` (fail-closed), with reasons and evidence ids in
//! the durable journal.

mod support;

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use support::*;

struct Case {
    repo: PathBuf,
    base: Value,
    scratch: PathBuf,
}

fn fixture_case(name: &str, fixture_dir: &str) -> Case {
    let repo = init_repo(
        name,
        &[(
            "src/lib.rs",
            fixture(&format!("semantic/{fixture_dir}/src/lib.rs")),
        )],
    );
    let base = run_base(&repo);
    let scratch = repo.with_extension("scratch");
    Case {
        repo,
        base,
        scratch,
    }
}

fn proposal(case: &Case, agent: &str, task: &str, symbols: &[&str], patch_file: &str) -> Value {
    proposal_json(
        &case.base,
        agent,
        task,
        "src/lib.rs",
        symbols,
        &fixture(&format!("semantic/{patch_file}")),
    )
}

fn single(case: &Case, agent: &str, task: &str, symbols: &[&str], patch_file: &str) -> PathBuf {
    write_proposals(
        &case.repo,
        &[("01.json", proposal(case, agent, task, symbols, patch_file))],
    )
}

fn integrate(case: &Case, proposals: &Path, extra: &[&str]) -> Run {
    run_integrate(&case.repo, proposals, extra, &case.scratch)
}

fn head(repo: &Path) -> String {
    git(repo, &["rev-parse", "HEAD"]).trim().to_owned()
}

fn assert_refused_without_state_change(case: &Case, before: &str, run: &Run, expected_kind: &str) {
    assert_eq!(
        run.code, 0,
        "stderr: {}\nstdout: {}",
        run.stderr, run.stdout
    );
    assert_eq!(
        run.decision_kinds(),
        vec![expected_kind.to_owned()],
        "decisions: {:?}",
        run.decisions()
    );
    assert!(
        run.accepted().is_empty(),
        "a refusal must not be recorded as accepted"
    );
    assert_eq!(
        git(&case.repo, &["rev-parse", "HEAD"]).trim(),
        before,
        "a refusal must not publish"
    );
    assert_eq!(git(&case.repo, &["status", "--porcelain"]).trim(), "");
}

// ---------------------------------------------------------------------------
// 1. same function modified twice
// ---------------------------------------------------------------------------

#[test]
fn case01_same_function_twice_is_a_symbol_conflict() {
    let case = fixture_case("collab3-same-function", "same_function");
    let proposals = write_proposals(
        &case.repo,
        &[
            (
                "01-a.json",
                proposal(
                    &case,
                    "agent-a",
                    "alpha",
                    &["alpha"],
                    "same_function/a.patch",
                ),
            ),
            (
                "02-b.json",
                proposal(
                    &case,
                    "agent-b",
                    "alpha",
                    &["alpha"],
                    "same_function/b.patch",
                ),
            ),
        ],
    );
    let run = integrate(&case, &proposals, &["--verify"]);
    assert_eq!(run.decision_kinds(), vec!["AUTO_SAFE", "CONFLICT"]);
    assert_eq!(run.summary("auto_safe"), 1);
    assert_eq!(run.summary("conflict"), 1);
    assert!(
        run.reasons(1).contains("alpha"),
        "reason: {}",
        run.reasons(1)
    );
    assert_eq!(run.accepted().len(), 1);
}

// ---------------------------------------------------------------------------
// 2. public signature changed
// ---------------------------------------------------------------------------

#[test]
fn case02_public_signature_change_requires_human_arbitration() {
    let case = fixture_case("collab3-public-signature", "public_signature");
    let proposals = single(
        &case,
        "agent-a",
        "area",
        &["area"],
        "public_signature/a.patch",
    );
    let before = head(&case.repo);
    let run = integrate(&case, &proposals, &["--verify"]);
    assert_refused_without_state_change(&case, &before, &run, "MANUAL_REQUIRED");
    let reasons = run.reasons(0);
    assert!(reasons.contains("public API"), "reasons: {reasons}");
    assert!(
        reasons.contains("semantic policy evidence"),
        "the refusal must carry journaled evidence: {reasons}"
    );
    assert!(
        reasons.contains("semantic policy evidence"),
        "the policy verdict must carry evidence: {reasons}"
    );
}

// ---------------------------------------------------------------------------
// 3. caller became incompatible
// ---------------------------------------------------------------------------

#[test]
fn case03_incompatible_caller_is_rejected_by_the_real_compiler() {
    let case = fixture_case("collab3-caller", "caller_incompatible");
    let proposals = single(
        &case,
        "agent-a",
        "scale",
        &["scale"],
        "caller_incompatible/a.patch",
    );
    let before = head(&case.repo);
    let run = integrate(&case, &proposals, &["--verify"]);
    assert_refused_without_state_change(&case, &before, &run, "REJECT");
    let reasons = run.reasons(0);
    assert!(
        reasons.contains("E0061"),
        "the compiler diagnostic code must be journaled: {reasons}"
    );
    assert!(reasons.contains("does not compile"), "reasons: {reasons}");
    assert!(reasons.contains("evidence"), "reasons: {reasons}");
}

// ---------------------------------------------------------------------------
// 4. weakened contract
// ---------------------------------------------------------------------------

#[test]
fn case04_a_removed_contract_assertion_requires_arbitration() {
    let case = fixture_case("collab3-weakened", "weakened_contract");
    let proposals = single(
        &case,
        "agent-a",
        "divide",
        &["checked_divide"],
        "weakened_contract/a.patch",
    );
    let before = head(&case.repo);
    let run = integrate(&case, &proposals, &["--verify"]);
    assert_refused_without_state_change(&case, &before, &run, "MANUAL_REQUIRED");
    let reasons = run.reasons(0);
    assert!(reasons.contains("contract assertion"), "reasons: {reasons}");
    assert!(reasons.contains("human arbitration"), "reasons: {reasons}");
    assert!(
        reasons.contains("semantic policy evidence"),
        "reasons: {reasons}"
    );

    // A declared impacted contract is refused by the pre-existing INV-02 rule.
    let mut declared = proposal(
        &case,
        "agent-a",
        "divide",
        &["checked_divide"],
        "weakened_contract/a.patch",
    );
    declared["impacted_contracts"] = json!(["checked_divide: divisor != 0"]);
    let proposals = write_proposals(&case.repo, &[("01.json", declared)]);
    let run = integrate(&case, &proposals, &["--verify"]);
    assert_refused_without_state_change(&case, &before, &run, "MANUAL_REQUIRED");
    assert!(
        run.reasons(0).contains("impacted contract"),
        "reasons: {}",
        run.reasons(0)
    );
}

// ---------------------------------------------------------------------------
// 5. undeclared dependency
// ---------------------------------------------------------------------------

#[test]
fn case05_an_undeclared_dependency_on_a_modified_symbol_is_refused() {
    let case = fixture_case("collab3-dependency", "undeclared_dependency");
    let proposals = write_proposals(
        &case.repo,
        &[
            (
                "01-a.json",
                proposal(
                    &case,
                    "agent-a",
                    "helper",
                    &["helper"],
                    "undeclared_dependency/a.patch",
                ),
            ),
            (
                "02-b.json",
                proposal(
                    &case,
                    "agent-b",
                    "compose",
                    &["compose"],
                    "undeclared_dependency/b.patch",
                ),
            ),
        ],
    );
    let run = integrate(&case, &proposals, &["--verify"]);
    assert_eq!(run.decision_kinds(), vec!["AUTO_SAFE", "MANUAL_REQUIRED"]);
    assert_eq!(run.summary("auto_safe"), 1);
    assert_eq!(run.summary("manual_required"), 1);
    let reasons = run.reasons(1);
    assert!(
        reasons.contains("undeclared semantic dependency"),
        "reasons: {reasons}"
    );
    assert!(reasons.contains("helper"), "reasons: {reasons}");
    assert_eq!(run.accepted().len(), 1);

    // Positive control: the same second change without the cross-symbol
    // reference is verified and accepted (the heuristic is not a blanket veto).
    let control = fixture_case("collab3-dependency-control", "undeclared_dependency");
    let constant_patch = "+++ b/src/lib.rs\n@@ -7,3 +7,3 @@\n fn compose(x: u32) -> u32 {\n-    helper(x) * 2\n+    9\n }\n";
    let proposals = write_proposals(
        &control.repo,
        &[
            (
                "01-a.json",
                proposal(
                    &control,
                    "agent-a",
                    "helper",
                    &["helper"],
                    "undeclared_dependency/a.patch",
                ),
            ),
            (
                "02-b.json",
                proposal_json(
                    &control.base,
                    "agent-b",
                    "compose",
                    "src/lib.rs",
                    &["compose"],
                    constant_patch,
                ),
            ),
        ],
    );
    let run = integrate(&control, &proposals, &["--verify"]);
    assert_eq!(
        run.decision_kinds(),
        vec!["AUTO_SAFE", "AUTO_SAFE"],
        "reasons: {}",
        run.reasons(1)
    );
    assert_eq!(run.accepted().len(), 2);
}

// ---------------------------------------------------------------------------
// 6. ambiguous import introduced by the merge
// ---------------------------------------------------------------------------

#[test]
fn case06_an_ambiguous_glob_import_is_rejected_by_compilation() {
    let case = fixture_case("collab3-ambiguous-import", "ambiguous_import_glob");
    let proposals = write_proposals(
        &case.repo,
        &[
            (
                "01-a.json",
                proposal(
                    &case,
                    "agent-a",
                    "glob-a",
                    &["r"],
                    "ambiguous_import_glob/a.patch",
                ),
            ),
            (
                "02-b.json",
                proposal(
                    &case,
                    "agent-b",
                    "glob-b",
                    &["t"],
                    "ambiguous_import_glob/b.patch",
                ),
            ),
        ],
    );
    let run = integrate(&case, &proposals, &["--verify", "--commit"]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    assert_eq!(run.decision_kinds(), vec!["AUTO_SAFE", "REJECT"]);
    let reasons = run.reasons(1);
    assert!(
        reasons.contains("E0659"),
        "the ambiguity code must be journaled: {reasons}"
    );
    assert!(reasons.contains("evidence"), "reasons: {reasons}");
    assert_eq!(run.accepted().len(), 1);

    // Only A's import and call are published; B's second glob is not.
    let content = git(&case.repo, &["show", "HEAD:src/lib.rs"]);
    assert!(content.contains("use a::*;"), "HEAD:\n{content}");
    assert!(!content.contains("use b::*;"), "HEAD:\n{content}");
    assert_eq!(
        git(&case.repo, &["rev-list", "--count", "HEAD"]).trim(),
        "2"
    );
}

// ---------------------------------------------------------------------------
// 7. ABI change
// ---------------------------------------------------------------------------

#[test]
fn case07_an_abi_change_requires_human_arbitration() {
    let case = fixture_case("collab3-abi", "abi_change");
    let proposals = single(
        &case,
        "agent-a",
        "entry",
        &["abi_entry"],
        "abi_change/a.patch",
    );
    let before = head(&case.repo);
    let run = integrate(&case, &proposals, &["--verify"]);
    assert_refused_without_state_change(&case, &before, &run, "MANUAL_REQUIRED");
    let reasons = run.reasons(0);
    assert!(
        reasons.contains("ABI surface changed"),
        "reasons: {reasons}"
    );
    assert!(
        reasons.contains("semantic policy evidence"),
        "reasons: {reasons}"
    );
    assert!(
        reasons.contains("cannot be proven by a local compile"),
        "reasons: {reasons}"
    );
}

// ---------------------------------------------------------------------------
// 8. lockfile modification
// ---------------------------------------------------------------------------

#[test]
fn case08_a_lockfile_modification_is_never_auto_integrated() {
    let repo = init_repo(
        "collab3-lockfile",
        &[
            ("src/lib.rs", fixture("semantic/lockfile/src/lib.rs")),
            ("Cargo.lock", fixture("semantic/lockfile/Cargo.lock")),
        ],
    );
    let base = run_base(&repo);
    let proposal = proposal_json(
        &base,
        "agent-a",
        "lock",
        "Cargo.lock",
        &[],
        &fixture("semantic/lockfile/lock.patch"),
    );
    let proposals = write_proposals(&repo, &[("01.json", proposal)]);
    let scratch = repo.with_extension("scratch");
    let before = head(&repo);
    let run = run_integrate(&repo, &proposals, &["--verify"], &scratch);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    assert_eq!(run.decision_kinds(), vec!["MANUAL_REQUIRED"]);
    assert!(run.accepted().is_empty());
    assert!(
        run.reasons(0).contains("generated or a lockfile"),
        "reasons: {}",
        run.reasons(0)
    );
    assert_eq!(head(&repo), before);
    assert_eq!(git(&repo, &["status", "--porcelain"]).trim(), "");
}

// ---------------------------------------------------------------------------
// 9. functional regression in code that still compiles
// ---------------------------------------------------------------------------

#[test]
fn case09_a_functional_regression_that_compiles_is_caught_by_test_execution() {
    let case = fixture_case("collab3-regression", "functional_regression");
    let proposals = single(
        &case,
        "agent-a",
        "double",
        &["double"],
        "functional_regression/a.patch",
    );

    // (a) required check unavailable -> explicit MANUAL_REQUIRED, never AUTO_SAFE.
    let before = head(&case.repo);
    let run = integrate(&case, &proposals, &["--verify"]);
    assert_refused_without_state_change(&case, &before, &run, "MANUAL_REQUIRED");
    assert!(
        run.reasons(0).contains("test execution is disabled"),
        "reasons: {}",
        run.reasons(0)
    );
    assert!(
        run.reasons(0).contains("fail-closed"),
        "reasons: {}",
        run.reasons(0)
    );

    // (b) with test execution enabled, the regression is rejected with proof.
    let run = integrate(&case, &proposals, &["--verify-tests"]);
    assert_refused_without_state_change(&case, &before, &run, "REJECT");
    let reasons = run.reasons(0);
    assert!(reasons.contains("test harness"), "reasons: {reasons}");
    assert!(reasons.contains("failed"), "reasons: {reasons}");
    assert!(reasons.contains("evidence"), "reasons: {reasons}");
}

// ---------------------------------------------------------------------------
// 10. agent working on a stale snapshot
// ---------------------------------------------------------------------------

#[test]
fn case10_a_stale_base_snapshot_requires_revalidation() {
    let case = fixture_case("collab3-stale", "two_agents_same_file");
    let mut stale = proposal(
        &case,
        "agent-a",
        "alpha",
        &["alpha"],
        "two_agents_same_file/a.patch",
    );
    stale["base_snapshot_id"] = json!("an-older-base");
    stale["expected_hashes"] = json!({});
    let proposals = write_proposals(&case.repo, &[("01.json", stale)]);
    let before = head(&case.repo);
    let run = integrate(&case, &proposals, &["--verify"]);
    assert_refused_without_state_change(&case, &before, &run, "REBASE_REVALIDATE");
    assert!(
        run.reasons(0).contains("stale"),
        "reasons: {}",
        run.reasons(0)
    );
}
