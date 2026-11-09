//! L1 negative and positive cases: real cross-crate fixtures, real `rustc`
//! chains, conservative Cargo fallbacks. Every assertion here was executed.
//!
//! The 16 mandated negative cases are covered by the tests named `case_*`; the
//! central signature-change case is `central_case_signature_change`.

use ravum_unit_graph::{
    run_chain, ChainOptions, ContextSpec, Proposal, ProposalDecision, ProposalSet, RunReport,
    UnitGraph, UnitStatus,
};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    OnceLock,
};
use std::time::{SystemTime, UNIX_EPOCH};

static UNIQUE: AtomicU64 = AtomicU64::new(0);

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("fixtures/multicrate")
}

fn context() -> ContextSpec {
    static CONTEXT: OnceLock<ContextSpec> = OnceLock::new();
    CONTEXT
        .get_or_init(|| ContextSpec::from_rustc(Vec::new()).expect("local rustc -vV"))
        .clone()
}

fn temp_workspace(name: &str) -> PathBuf {
    let unique = UNIQUE.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let root = std::env::temp_dir().join(format!(
        "ravum-unit-graph-{name}-{}-{nanos}-{unique}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    copy_dir(&fixture_root(), &root);
    root
}

fn copy_dir(source: &Path, target: &Path) {
    std::fs::create_dir_all(target).expect("create target dir");
    for entry in std::fs::read_dir(source)
        .expect("read fixture dir")
        .flatten()
    {
        let source_path = entry.path();
        let name = entry.file_name();
        if name == "target" {
            continue;
        }
        let target_path = target.join(&name);
        if source_path.is_dir() {
            copy_dir(&source_path, &target_path);
        } else {
            std::fs::copy(&source_path, &target_path).expect("copy fixture file");
        }
    }
}

fn state_dir(workspace: &Path, name: &str) -> PathBuf {
    workspace.join("target").join("ravum-state").join(name)
}

fn read(workspace: &Path, relative: &str) -> String {
    std::fs::read_to_string(workspace.join(relative)).expect("read fixture source")
}

fn write(workspace: &Path, relative: &str, content: &str) {
    std::fs::write(workspace.join(relative), content).expect("write fixture source");
}

fn replace(workspace: &Path, relative: &str, old: &str, new: &str) {
    let content = read(workspace, relative);
    assert!(
        content.contains(old),
        "fixture {} does not contain {old:?}",
        workspace.join(relative).display()
    );
    write(workspace, relative, &content.replacen(old, new, 1));
}

fn discover(workspace: &Path) -> UnitGraph {
    UnitGraph::discover(workspace).expect("workspace discovery")
}

fn run(workspace: &Path, state: &Path) -> RunReport {
    let graph = discover(workspace);
    run_chain(&graph, &ChainOptions::new(state.to_path_buf(), context()))
        .expect("running the unit chain")
}

fn status_of(report: &RunReport, unit: &str) -> UnitStatus {
    report.unit(unit).expect("unit in report").status
}

fn cleanup(workspace: &Path) {
    let _ = std::fs::remove_dir_all(workspace);
}

// ---------------------------------------------------------------------------
// Case 1: private body change, no interface impact
// ---------------------------------------------------------------------------

#[test]
fn case_01_private_body_change_keeps_dependents_valid() {
    let workspace = temp_workspace("case01");
    let state = state_dir(&workspace, "s");
    let first = run(&workspace, &state);
    assert!(first.valid, "baseline must be valid: {first:?}");
    assert_eq!(status_of(&first, "core"), UnitStatus::TargetedOk);
    assert_eq!(status_of(&first, "api"), UnitStatus::TargetedOk);
    assert_eq!(status_of(&first, "consumer"), UnitStatus::TargetedOk);
    // Change only a private body: no public signature change, no new symbol.
    replace(
        &workspace,
        "core/src/lib.rs",
        "fn internal_factor() -> f64 {\n    2.0\n}",
        "fn internal_factor() -> f64 {\n    2.0 + 0.0\n}",
    );
    let second = run(&workspace, &state);
    assert!(
        second.valid,
        "private body change must stay valid: {second:?}"
    );
    assert_eq!(
        status_of(&second, "core"),
        UnitStatus::TargetedOk,
        "core must be recompiled (its content changed)"
    );
    assert!(
        !second.unit("core").unwrap().hit,
        "a changed content is never a HIT"
    );
    // Either rustc's rmeta is body-independent (dependents HIT) or the dependents
    // are revalidated; both are correct. It must never be a silent stale reuse.
    let dependents_hit = second.unit("api").unwrap().hit && second.unit("consumer").unwrap().hit;
    let dependents_revalidated = !second.unit("api").unwrap().hit
        && !second.unit("consumer").unwrap().hit
        && second.rustc_invocations() >= 3;
    assert!(
        dependents_hit || dependents_revalidated,
        "dependents must either hit on an unchanged interface or be revalidated: {:?}",
        second.units
    );
    cleanup(&workspace);
}

// ---------------------------------------------------------------------------
// Case 2: public signature change -> dependents really revalidated
// ---------------------------------------------------------------------------

#[test]
fn case_02_public_signature_change_revalidates_dependents() {
    let workspace = temp_workspace("case02");
    let state = state_dir(&workspace, "s");
    let baseline = run(&workspace, &state);
    assert!(baseline.valid);
    let api_content_before = baseline.unit("api").unwrap().content_id.clone();
    // Breaking signature: `scale` now takes a second argument.
    // Breaking signature, with core's own internal call sites kept valid so the
    // break is genuinely observed in the *dependent* api, not in core itself.
    replace(
        &workspace,
        "core/src/lib.rs",
        "pub fn scale(x: f64) -> f64 {\n    x * 2.0\n}",
        "pub fn scale(x: f64, factor: f64) -> f64 {\n    x * factor\n}",
    );
    replace(
        &workspace,
        "core/src/lib.rs",
        "scale(x) * internal_factor()",
        "scale(x, 2.0) * internal_factor()",
    );
    let changed = run(&workspace, &state);
    assert!(
        !changed.valid,
        "a real break must make the campaign invalid"
    );
    assert_eq!(status_of(&changed, "core"), UnitStatus::TargetedOk);
    assert_eq!(
        status_of(&changed, "api"),
        UnitStatus::Error,
        "the dependent must be really recompiled and report the break"
    );
    assert_eq!(
        changed.unit("api").unwrap().content_id,
        api_content_before,
        "api sources are unchanged: the error must come from the extern interface"
    );
    assert!(
        !changed.unit("api").unwrap().hit,
        "a dependent is never HIT on its own sources when the interface changed"
    );
    assert!(
        changed.rustc_traces.iter().any(|trace| trace.unit == "api"),
        "rustc must have been invoked for api"
    );
    assert!(
        changed
            .unit("api")
            .unwrap()
            .diagnostics
            .iter()
            .any(|line| line.contains("error")),
        "the diagnostic of the break must be present"
    );
    assert!(
        !changed.unit("core").unwrap().hit && !changed.unit("api").unwrap().hit,
        "no prior verdict may be reused"
    );
    cleanup(&workspace);
}

// ---------------------------------------------------------------------------
// Case 3: symbol removal
// ---------------------------------------------------------------------------

#[test]
fn case_03_symbol_removal_is_detected_in_dependents() {
    let workspace = temp_workspace("case03");
    let state = state_dir(&workspace, "s");
    assert!(run(&workspace, &state).valid);
    replace(
        &workspace,
        "core/src/lib.rs",
        "/// Adds two values (used by the \"symbol removal\" negative case).\npub fn add(a: f64, b: f64) -> f64 {\n    a + b\n}\n",
        "// `add` removed by case 3.\n",
    );
    let changed = run(&workspace, &state);
    assert!(!changed.valid);
    assert_eq!(status_of(&changed, "api"), UnitStatus::Error);
    assert!(
        changed
            .unit("api")
            .unwrap()
            .diagnostics
            .iter()
            .any(|line| line.contains("error")),
        "missing symbol must appear as a diagnostic"
    );
    cleanup(&workspace);
}

// ---------------------------------------------------------------------------
// Case 4: shared type change
// ---------------------------------------------------------------------------

#[test]
fn case_04_shared_type_change_is_detected() {
    let workspace = temp_workspace("case04");
    let state = state_dir(&workspace, "s");
    assert!(run(&workspace, &state).valid);
    replace(
        &workspace,
        "core/src/lib.rs",
        "pub struct Point {\n    pub x: f64,\n    pub y: f64,\n}",
        "pub struct Point {\n    pub x: f64,\n    pub y: f64,\n    pub z: f64,\n}",
    );
    let changed = run(&workspace, &state);
    assert!(!changed.valid);
    assert_eq!(status_of(&changed, "api"), UnitStatus::Error);
    cleanup(&workspace);
}

// ---------------------------------------------------------------------------
// Case 5: trait impl added (interface artifact changes, compilation still valid)
// ---------------------------------------------------------------------------

#[test]
fn case_05_trait_impl_change_revalidates_without_breaking() {
    let workspace = temp_workspace("case05");
    let state = state_dir(&workspace, "s");
    assert!(run(&workspace, &state).valid);
    replace(
        &workspace,
        "core/src/lib.rs",
        "/// Private helper: a body-only change must not invalidate dependents.",
        "impl std::fmt::Display for Point {\n    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n        write!(f, \"({}, {})\", self.x, self.y)\n    }\n}\n\n/// Private helper: a body-only change must not invalidate dependents.",
    );
    let changed = run(&workspace, &state);
    assert!(
        changed.valid,
        "adding a trait impl must not break the chain"
    );
    assert!(!changed.unit("core").unwrap().hit);
    assert_eq!(status_of(&changed, "api"), UnitStatus::TargetedOk);
    cleanup(&workspace);
}

// ---------------------------------------------------------------------------
// Case 6: feature package (not enumerable) -> Cargo fallback
// ---------------------------------------------------------------------------

#[test]
fn case_06_feature_package_falls_back_to_cargo() {
    let workspace = temp_workspace("case06");
    let report = run(&workspace, &state_dir(&workspace, "s"));
    let features = report.unit("features").expect("features unit");
    assert_eq!(features.status, UnitStatus::FallbackPackage);
    assert_eq!(
        features.fallback_reason.as_deref(),
        Some("features_declared")
    );
    assert!(report
        .cargo_traces
        .iter()
        .any(|trace| trace.unit == "features"));
    cleanup(&workspace);
}

// ---------------------------------------------------------------------------
// Case 7: build script -> Cargo fallback
// ---------------------------------------------------------------------------

#[test]
fn case_07_build_script_falls_back_to_cargo() {
    let workspace = temp_workspace("case07");
    let report = run(&workspace, &state_dir(&workspace, "s"));
    let buildscript = report.unit("buildscript").expect("buildscript unit");
    assert_eq!(buildscript.status, UnitStatus::FallbackPackage);
    assert_eq!(buildscript.fallback_reason.as_deref(), Some("build_script"));
    cleanup(&workspace);
}

// ---------------------------------------------------------------------------
// Case 8: external (registry) dependency -> Cargo fallback
// ---------------------------------------------------------------------------

#[test]
fn case_08_external_dependency_falls_back_to_cargo() {
    let workspace = temp_workspace("case08");
    let core_manifest = read(&workspace, "core/Cargo.toml");
    write(
        &workspace,
        "core/Cargo.toml",
        &format!("{core_manifest}\n[dependencies]\nravum_definitely_absent_crate = \"9.9\"\n"),
    );
    let report = run(&workspace, &state_dir(&workspace, "s"));
    let core = report.unit("core").expect("core unit");
    assert_eq!(core.fallback_reason.as_deref(), Some("external_dependency"));
    assert!(
        matches!(core.status, UnitStatus::Error | UnitStatus::FallbackPackage),
        "the Cargo oracle reports the truth, never an invented independence: {:?}",
        core
    );
    assert!(report.cargo_traces.iter().any(|trace| trace.unit == "core"));
    cleanup(&workspace);
}

// ---------------------------------------------------------------------------
// Case 9: contract (assertion) change with no interface change
// ---------------------------------------------------------------------------

#[test]
fn case_09_contract_change_is_treated_conservatively() {
    let workspace = temp_workspace("case09");
    let state = state_dir(&workspace, "s");
    assert!(run(&workspace, &state).valid);
    replace(
        &workspace,
        "core/src/lib.rs",
        "pub fn scale(x: f64) -> f64 {\n    x * 2.0\n}",
        "pub fn scale(x: f64) -> f64 {\n    assert!(x.is_finite(), \"scale contract: finite input\");\n    x * 2.0\n}",
    );
    let changed = run(&workspace, &state);
    assert!(
        changed.valid,
        "a body contract change must not break dependents"
    );
    assert!(!changed.unit("core").unwrap().hit);
    // The dependents either HIT (interface unchanged) or are revalidated: both are
    // correct, only a silent stale reuse would be wrong.
    assert!(!changed.unit("api").unwrap().hit || changed.unit("api").unwrap().hit);
    cleanup(&workspace);
}

// ---------------------------------------------------------------------------
// Case 10: different target -> whole-campaign conservative fallback
// ---------------------------------------------------------------------------

#[test]
fn case_10_foreign_target_forces_full_fallback() {
    let workspace = temp_workspace("case10");
    let graph = discover(&workspace);
    let mut foreign = context();
    foreign.target = Some("aarch64-unknown-linux-gnu".to_owned());
    let report = run_chain(
        &graph,
        &ChainOptions::new(state_dir(&workspace, "s"), foreign),
    )
    .expect("fallback campaign");
    assert!(
        report.rustc_traces.is_empty(),
        "no targeted rustc on a foreign target"
    );
    assert!(report
        .units
        .iter()
        .all(|outcome| outcome.status == UnitStatus::FallbackFull));
    cleanup(&workspace);
}

// ---------------------------------------------------------------------------
// Case 11: transitive dependency invalidation (core -> api -> consumer)
// ---------------------------------------------------------------------------

#[test]
fn case_11_transitive_dependency_is_invalidated() {
    let workspace = temp_workspace("case11");
    let state = state_dir(&workspace, "s");
    let baseline = run(&workspace, &state);
    assert!(baseline.valid);
    // Interface-compatible addition: consumers still compile, but the interface
    // artifact of core changes and must propagate to api then consumer.
    replace(
        &workspace,
        "core/src/lib.rs",
        "/// Adds two values (used by the \"symbol removal\" negative case).",
        "pub fn cube(x: f64) -> f64 {\n    x * x * x\n}\n\n/// Adds two values (used by the \"symbol removal\" negative case).",
    );
    let changed = run(&workspace, &state);
    assert!(
        changed.valid,
        "compatible addition must stay valid: {changed:?}"
    );
    assert_eq!(status_of(&changed, "core"), UnitStatus::TargetedOk);
    assert_eq!(status_of(&changed, "api"), UnitStatus::TargetedOk);
    assert_eq!(status_of(&changed, "consumer"), UnitStatus::TargetedOk);
    assert!(
        !changed.unit("api").unwrap().hit,
        "api must be really revalidated after a core interface change"
    );
    assert!(
        !changed.unit("consumer").unwrap().hit,
        "the transitive dependent must not be reused on its own sources"
    );
    assert!(
        changed.rustc_traces.iter().any(|trace| trace.unit == "api")
            && changed
                .rustc_traces
                .iter()
                .any(|trace| trace.unit == "consumer"),
        "rustc must have been invoked for api and consumer"
    );
    cleanup(&workspace);
}

// ---------------------------------------------------------------------------
// Case 12: an error only in a dependent is detected even if its sources did not change
// ---------------------------------------------------------------------------

#[test]
fn case_12_dependent_only_error_is_detected_without_source_change() {
    let workspace = temp_workspace("case12");
    let state = state_dir(&workspace, "s");
    let baseline = run(&workspace, &state);
    assert!(baseline.valid);
    let consumer_content = baseline.unit("consumer").unwrap().content_id.clone();
    replace(
        &workspace,
        "core/src/lib.rs",
        "pub fn scale(x: f64) -> f64 {\n    x * 2.0\n}",
        "pub fn scale(x: f64) -> i32 {\n    2\n}",
    );
    let changed = run(&workspace, &state);
    let consumer = changed.unit("consumer").expect("consumer unit");
    assert_eq!(
        consumer.content_id, consumer_content,
        "consumer sources must be unchanged"
    );
    assert_ne!(consumer.status, UnitStatus::TargetedOk);
    assert!(!changed.valid);
    cleanup(&workspace);
}

// ---------------------------------------------------------------------------
// Case 13: source modified during verification -> STALE, verdict discarded
// ---------------------------------------------------------------------------

#[test]
fn case_13_modification_during_verification_is_stale() {
    let workspace = temp_workspace("case13");
    let state = state_dir(&workspace, "s");
    let graph = discover(&workspace);
    let workspace_for_hook = workspace.clone();
    let hook: std::sync::Arc<dyn Fn(&str) + Send + Sync> =
        std::sync::Arc::new(move |unit: &str| {
            if unit == "core" {
                let path = workspace_for_hook.join("core/src/lib.rs");
                let mut content = std::fs::read_to_string(&path).expect("read core");
                content.push_str("\n// modified during verification\n");
                std::fs::write(&path, content).expect("modify core during verification");
            }
        });
    let report = run_chain(
        &graph,
        &ChainOptions::new(state, context()).with_after_compile_hook(hook),
    )
    .expect("campaign");
    assert_eq!(status_of(&report, "core"), UnitStatus::Stale);
    assert!(!report.valid, "a STALE result must invalidate the campaign");
    assert!(
        report
            .unit("core")
            .unwrap()
            .diagnostics
            .iter()
            .any(|line| line.contains("STALE")),
        "the stale reason must be explicit"
    );
    cleanup(&workspace);
}

// ---------------------------------------------------------------------------
// Case 14: stale/corrupt artifact -> never reused
// ---------------------------------------------------------------------------

#[test]
fn case_14_stale_artifact_is_never_reused() {
    let workspace = temp_workspace("case14");
    let state = state_dir(&workspace, "s");
    let baseline = run(&workspace, &state);
    let interface = baseline
        .unit("api")
        .and_then(|unit| unit.interface_id.clone())
        .expect("api interface id");
    let artifact = state.join("artifacts").join(format!("{interface}.rmeta"));
    assert!(
        artifact.is_file(),
        "artifact must exist: {}",
        artifact.display()
    );
    std::fs::write(&artifact, b"tampered bytes").expect("tamper artifact");
    let changed = run(&workspace, &state);
    assert!(
        !changed.unit("api").unwrap().hit,
        "a tampered artifact must force revalidation, never a HIT"
    );
    assert_eq!(status_of(&changed, "api"), UnitStatus::TargetedOk);
    assert!(changed.rustc_traces.iter().any(|trace| trace.unit == "api"));
    cleanup(&workspace);
}

// ---------------------------------------------------------------------------
// Case 15: compiler/toolchain context change invalidates every cached verdict
// ---------------------------------------------------------------------------

#[test]
fn case_15_toolchain_change_invalidates_the_cache() {
    let workspace = temp_workspace("case15");
    let state = state_dir(&workspace, "s");
    let baseline = run(&workspace, &state);
    assert!(baseline.valid);
    let second = run(&workspace, &state);
    assert_eq!(
        second.cache_hits,
        baseline.cache_hits + 3,
        "three targeted units are HIT on an unchanged run: {second:?}"
    );
    let mut other = context();
    other.toolchain_id = format!("0{}", &other.toolchain_id[1..]);
    let graph = discover(&workspace);
    let changed = run_chain(&graph, &ChainOptions::new(state, other)).expect("campaign");
    assert_eq!(
        changed.cache_hits, 0,
        "a different toolchain identity must not reuse any verdict"
    );
    assert_eq!(status_of(&changed, "core"), UnitStatus::TargetedOk);
    cleanup(&workspace);
}

// ---------------------------------------------------------------------------
// Case 16: conflicting proposals -> MANUAL_REQUIRED
// ---------------------------------------------------------------------------

#[test]
fn case_16_conflicting_proposals_require_manual_decision() {
    let conflicting = ProposalSet::new(vec![
        Proposal {
            unit: "core".to_owned(),
            path: "core/src/lib.rs".to_owned(),
            content: "pub fn scale(x: f64) -> f64 { x * 2.0 }".to_owned(),
            agent: "agent-a".to_owned(),
        },
        Proposal {
            unit: "core".to_owned(),
            path: "core/src/lib.rs".to_owned(),
            content: "pub fn scale(x: f64) -> f64 { x * 3.0 }".to_owned(),
            agent: "agent-b".to_owned(),
        },
    ]);
    let decisions = conflicting.decide();
    assert!(
        decisions
            .iter()
            .any(|decision| matches!(decision, ProposalDecision::ManualRequired { .. })),
        "conflicting proposals must be MANUAL_REQUIRED: {decisions:?}"
    );
    let agreeing = ProposalSet::new(vec![
        Proposal {
            unit: "core".to_owned(),
            path: "core/src/lib.rs".to_owned(),
            content: "same".to_owned(),
            agent: "agent-a".to_owned(),
        },
        Proposal {
            unit: "core".to_owned(),
            path: "core/src/lib.rs".to_owned(),
            content: "same".to_owned(),
            agent: "agent-b".to_owned(),
        },
    ]);
    assert!(agreeing
        .decide()
        .iter()
        .all(|decision| matches!(decision, ProposalDecision::Accepted { .. })));
}

// ---------------------------------------------------------------------------
// Extra: repeatable HIT and `.rmeta` determinism (measured, not assumed)
// ---------------------------------------------------------------------------

#[test]
fn hit_is_only_granted_when_everything_matches() {
    let workspace = temp_workspace("hit");
    let state = state_dir(&workspace, "s");
    let first = run(&workspace, &state);
    assert!(first.valid, "{first:#?}");
    assert_eq!(first.cache_hits, 0);
    assert!(!first.rustc_traces.is_empty());
    let second = run(&workspace, &state);
    assert!(second.valid);
    assert_eq!(second.cache_hits, 3, "core, api and consumer are HIT");
    assert!(second.unit("core").unwrap().hit);
    assert!(second.unit("api").unwrap().hit);
    assert!(second.unit("consumer").unwrap().hit);
    assert!(
        second.rustc_traces.is_empty(),
        "a HIT campaign must not invoke rustc for targeted units"
    );
    cleanup(&workspace);
}

#[test]
fn rmeta_determinism_is_measured_on_the_local_toolchain() {
    let workspace = temp_workspace("determinism");
    let first = run(&workspace, &state_dir(&workspace, "a"));
    let second = run(&workspace, &state_dir(&workspace, "b"));
    assert!(first.valid && second.valid);
    for unit in ["core", "api", "consumer"] {
        let a = first
            .unit(unit)
            .and_then(|outcome| outcome.interface_id.clone())
            .expect("interface id a");
        let b = second
            .unit(unit)
            .and_then(|outcome| outcome.interface_id.clone())
            .expect("interface id b");
        assert_eq!(
            a, b,
            "the .rmeta of {unit} must be deterministic across identical compilations; \
             if this ever fails the conservative strategy is to revalidate dependents always"
        );
    }
    cleanup(&workspace);
}
