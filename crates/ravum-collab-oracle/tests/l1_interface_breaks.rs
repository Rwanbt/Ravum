//! Real L1 interface-break detection on `fixtures/multicrate`.
//!
//! Every scenario runs the actual conservative campaign of `ravum-unit-graph`
//! (2× `cargo metadata`, chained `--emit=metadata` rustc invocations, Cargo
//! fallbacks), and the assertions are on real compilation outcomes — not on a
//! simulated fake. The only test double in the whole crate is the declared
//! verifier of `end_to_end_v2.rs`.

mod support;

use ravum_collab_oracle::UnitGraphOracle;
use ravum_workspace_coordinator::{
    OracleFindingKind, OracleSeverity, OracleVerdict, SemanticOracle,
};
use support::*;

#[test]
fn a_compatible_body_only_change_is_clear_when_the_dependents_are_declared() {
    let oracle = UnitGraphOracle::new(state_dir("l1-compatible"));
    let verdict = oracle.evaluate(&body_only_request(CORE_DEPENDENTS));

    assert!(
        matches!(verdict, OracleVerdict::Clear { .. }),
        "a body-only change with all dependents declared must be clear: {verdict:?}"
    );
    let cost = oracle.last_cost().expect("cost recorded");
    assert_eq!(cost.changed_units, vec!["core".to_owned()]);
    assert!(cost.affected_units.is_empty());
    assert!(cost.rustc_invocations > 0);
    assert_eq!(cost.cargo_metadata_invocations, 2);
    assert!(
        cost.cargo_fallback_invocations > 0,
        "the fixture has fallback units"
    );
    assert_eq!(cost.before_errors, 0);
    assert_eq!(cost.after_errors, 0);
}

#[test]
fn an_undeclared_compatible_change_only_produces_a_non_blocking_warning() {
    let oracle = UnitGraphOracle::new(state_dir("l1-undeclared"));
    let verdict = oracle.evaluate(&body_only_request(&[]));

    let blocking = blocking(&verdict);
    assert!(
        blocking.is_empty(),
        "a change that still compiles must not be blocking: {blocking:?}"
    );
    assert!(
        has_kind(&verdict, OracleFindingKind::UndeclaredDependency),
        "the graph must report undeclared impacted dependents: {verdict:?}"
    );
    let finding = findings(&verdict)
        .into_iter()
        .find(|finding| finding.kind == OracleFindingKind::UndeclaredDependency)
        .expect("undeclared finding");
    assert_eq!(finding.severity, OracleSeverity::Warning);
    for dependent in CORE_DEPENDENTS {
        assert!(
            finding.symbols.iter().any(|symbol| symbol == dependent),
            "finding must list {dependent}: {finding:?}"
        );
    }
    assert!(finding.evidence_id.len() == 64);
}

#[test]
fn a_public_signature_break_used_by_api_is_a_blocking_interface_break() {
    let oracle = UnitGraphOracle::new(state_dir("l1-interface-break"));
    let verdict = oracle.evaluate(&interface_break_request());

    let blocking = blocking(&verdict);
    assert!(
        !blocking.is_empty(),
        "a signature break used by api must be blocking: {verdict:?}"
    );
    let interface = findings(&verdict)
        .into_iter()
        .filter(|finding| finding.kind == OracleFindingKind::InterfaceBreak)
        .find(|finding| finding.message.starts_with("interface break:"))
        .expect("a graph interface-break finding with affected units");
    assert_eq!(interface.severity, OracleSeverity::Blocking);
    assert!(
        interface.symbols.iter().any(|symbol| symbol == "api"),
        "api must be listed as affected: {interface:?}"
    );
    assert!(
        interface.message.contains("interface break") && interface.message.contains("api"),
        "message must explain the break: {}",
        interface.message
    );

    let cost = oracle.last_cost().expect("cost recorded");
    assert_eq!(cost.changed_units, vec!["core".to_owned()]);
    assert!(
        cost.changed_interface_units.contains(&"core".to_owned()),
        "the interface identity of core must have changed: {cost:?}"
    );
    assert!(
        cost.affected_units.iter().any(|unit| unit == "api"),
        "api must be in the attributed affected units: {cost:?}"
    );
    assert!(cost.after_errors > 0);
    assert_eq!(cost.before_errors, 0, "the baseline must compile");
}

#[test]
fn a_symbol_removal_used_by_api_is_a_blocking_interface_break() {
    let before = fixture_snapshot();
    let after = apply_patch(&before, CORE_SYMBOL_REMOVAL);
    let request = request(
        before,
        after,
        CORE_SYMBOL_REMOVAL,
        &[CORE_PATH],
        &["api", "consumer", "application"],
    );
    let oracle = UnitGraphOracle::new(state_dir("l1-symbol-removal"));
    let verdict = oracle.evaluate(&request);

    assert!(
        has_kind(&verdict, OracleFindingKind::InterfaceBreak),
        "removing core::add used by api must be an interface break: {verdict:?}"
    );
    assert!(
        blocking(&verdict)
            .iter()
            .any(|finding| finding.kind == OracleFindingKind::InterfaceBreak),
        "the interface break must be blocking: {verdict:?}"
    );
    let cost = oracle.last_cost().expect("cost recorded");
    assert!(
        cost.affected_units.iter().any(|unit| unit == "api"),
        "api must be attributed: {cost:?}"
    );
}

#[test]
fn a_changed_unit_compiled_through_a_cargo_fallback_is_unavailable_never_clear() {
    let before = fixture_snapshot();
    let after = apply_patch(&before, BUILDSCRIPT_FALLBACK_CHANGE);
    let request = request(
        before,
        after,
        BUILDSCRIPT_FALLBACK_CHANGE,
        &[BUILDSCRIPT_PATH],
        &[],
    );
    let oracle = UnitGraphOracle::new(state_dir("l1-fallback-uncertain"));
    let verdict = oracle.evaluate(&request);

    let reason = unavailable_reason(&verdict).unwrap_or_else(|| {
        panic!("a changed unit behind a Cargo fallback must be Unavailable: {verdict:?}")
    });
    assert!(
        reason.contains("fallback") && reason.contains("interface identity"),
        "reason must explain the fallback uncertainty: {reason}"
    );
    assert!(
        !matches!(verdict, OracleVerdict::Clear { .. }),
        "uncertainty must never be reported as Clear"
    );
}

#[test]
fn a_changed_feature_unit_is_verifiable_when_its_activation_set_is_resolved() {
    // MVP-0d: the activation set of `features` is resolved and passed to rustc,
    // so this unit no longer hides behind a Cargo fallback. The change compiles
    // and touches no public interface, so the verdict must not be Unavailable.
    let before = fixture_snapshot();
    let after = apply_patch(&before, FEATURES_BODY_CHANGE);
    let request = request(before, after, FEATURES_BODY_CHANGE, &[FEATURES_PATH], &[]);
    let oracle = UnitGraphOracle::new(state_dir("l1-features-resolved"));
    let verdict = oracle.evaluate(&request);

    assert!(
        unavailable_reason(&verdict).is_none(),
        "a resolved feature unit must be verifiable: {verdict:?}"
    );
    assert!(
        !matches!(verdict, OracleVerdict::Unavailable { .. }),
        "no fallback uncertainty should remain for this unit: {verdict:?}"
    );
    assert!(
        blocking(&verdict).is_empty(),
        "a body change that still compiles must not be blocking: {verdict:?}"
    );
}

#[test]
fn a_toolchain_mismatch_is_unavailable_never_clear() {
    let oracle = UnitGraphOracle::new(state_dir("l1-toolchain-mismatch")).with_expected_toolchain(
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    let verdict = oracle.evaluate(&body_only_request(CORE_DEPENDENTS));
    let reason = unavailable_reason(&verdict)
        .unwrap_or_else(|| panic!("a toolchain mismatch must be Unavailable: {verdict:?}"));
    assert!(reason.contains("toolchain mismatch"), "reason: {reason}");
}

#[test]
fn a_lockfile_change_is_a_blocking_finding_without_any_campaign_claim() {
    let mut request = body_only_request(CORE_DEPENDENTS);
    request.changed_paths.push("Cargo.lock".to_owned());
    let oracle = UnitGraphOracle::new(state_dir("l1-lockfile"));
    let verdict = oracle.evaluate(&request);

    assert!(
        has_kind(&verdict, OracleFindingKind::LockfileChange),
        "a lockfile change must be reported: {verdict:?}"
    );
    assert!(
        blocking(&verdict)
            .iter()
            .any(|finding| finding.kind == OracleFindingKind::LockfileChange),
        "the lockfile finding must be blocking: {verdict:?}"
    );
    let cost = oracle.last_cost().expect("cost recorded");
    assert_eq!(
        cost.cargo_metadata_invocations, 0,
        "no L1 campaign is claimed for a lockfile change"
    );
}
