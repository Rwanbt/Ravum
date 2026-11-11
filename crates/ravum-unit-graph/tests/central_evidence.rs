//! Central L1 evidence: real traces for the cross-crate signature change.
//!
//! Running `cargo test -p ravum-unit-graph --test central_evidence` always builds
//! the evidence from real `rustc`/`cargo` invocations (nothing is mocked: the
//! fixture is copied to a temporary workspace, edited, and compiled by the local
//! rustc 1.95.0).
//!
//! Writing the evidence file into the repository is opt-in so that a normal test
//! run never dirties the working tree (timings and interface ids depend on the
//! run): set `RAVUM_WRITE_EVIDENCE=1` to regenerate
//! `evidence/mvp0c-cross-crate-2026-11-09.json`; otherwise the same JSON is
//! written under `target/ravum-evidence/`.

use ravum_unit_graph::{run_chain, ChainOptions, ContextSpec, RunReport, UnitGraph, UnitStatus};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture_root() -> PathBuf {
    repo_root().join("fixtures/multicrate")
}

fn unique(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "ravum-mvp0c-{prefix}-{}-{nanos}",
        std::process::id()
    ))
}

fn copy_dir(source: &Path, target: &Path) {
    std::fs::create_dir_all(target).expect("create target dir");
    for entry in std::fs::read_dir(source).expect("read fixture").flatten() {
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

fn workspace(name: &str) -> PathBuf {
    let root = unique(name);
    let _ = std::fs::remove_dir_all(&root);
    copy_dir(&fixture_root(), &root);
    root
}

fn context() -> ContextSpec {
    ContextSpec::from_rustc(Vec::new()).expect("rustc -vV")
}

fn edit(workspace: &Path, relative: &str, old: &str, new: &str) {
    let path = workspace.join(relative);
    let content = std::fs::read_to_string(&path).expect("read fixture source");
    assert!(
        content.contains(old),
        "{} does not contain {old:?}",
        path.display()
    );
    std::fs::write(&path, content.replacen(old, new, 1)).expect("write fixture source");
}

fn campaign(workspace: &Path, state: &Path) -> RunReport {
    let graph = UnitGraph::discover(workspace).expect("discover workspace");
    run_chain(&graph, &ChainOptions::new(state.to_path_buf(), context())).expect("run chain")
}

fn rustc_version() -> String {
    let output = Command::new("rustc")
        .arg("-vV")
        .output()
        .expect("rustc -vV");
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn spawn_calibration() -> Vec<u64> {
    let mut samples = Vec::new();
    for _ in 0..3 {
        let started = Instant::now();
        let _ = Command::new("rustc").arg("-vV").output();
        samples.push(started.elapsed().as_millis() as u64);
    }
    samples
}

fn unit_status(report: &RunReport, unit: &str) -> Value {
    let outcome = report.unit(unit);
    json!({
        "unit": unit,
        "status": outcome.map(|outcome| outcome.status.as_str()),
        "hit": outcome.map(|outcome| outcome.hit),
        "fallback_reason": outcome.and_then(|outcome| outcome.fallback_reason.clone()),
        "diagnostics": outcome.map(|outcome| outcome.diagnostics.clone()).unwrap_or_default(),
        "interface_id": outcome.and_then(|outcome| outcome.interface_id.clone()),
        "content_id": outcome.map(|outcome| outcome.content_id.clone()),
        "cache_key": outcome.map(|outcome| outcome.cache_key.clone()),
    })
}

fn traces(report: &RunReport) -> Value {
    json!({
        "rustc": report.rustc_traces.iter().map(|trace| json!({
            "unit": trace.unit,
            "args": trace.args,
            "exit_code": trace.exit_code,
            "duration_ms": trace.duration_ms,
            "rmeta_sha256": trace.rmeta_sha256,
            "stderr_head": trace.stderr.lines().take(6).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "cargo": report.cargo_traces.iter().map(|trace| json!({
            "unit": trace.unit,
            "args": trace.args,
            "exit_code": trace.exit_code,
            "duration_ms": trace.duration_ms,
        })).collect::<Vec<_>>(),
    })
}

fn summary(report: &RunReport) -> Value {
    json!({
        "valid": report.valid,
        "cache_hits": report.cache_hits,
        "errors": report.errors,
        "stale": report.stale,
        "fallback": report.fallback,
        "rustc_invocations": report.rustc_traces.len(),
        "cargo_invocations": report.cargo_traces.len(),
        "units": report.units.iter().map(|outcome| json!({
            "unit": outcome.unit,
            "kind": outcome.kind,
            "status": outcome.status.as_str(),
            "hit": outcome.hit,
            "fallback_reason": outcome.fallback_reason,
            "interface_id": outcome.interface_id,
            "diagnostics": outcome.diagnostics,
        })).collect::<Vec<_>>(),
    })
}

#[test]
fn central_case_and_evidence() {
    let generated_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let calibration = spawn_calibration();
    let version = rustc_version();
    let host_target = context().host_target;
    let mut evidence = json!({
        "schema_version": 1,
        "kind": "mvp0c-cross-crate-2026-11-09",
        "generated_unix": generated_unix,
        "generated_iso": "2026-11-09 (local)",
        "toolchain": {"release": "1.95.0", "host": host_target, "rustc_vV": version},
        "machine": {
            "os": "Windows 11",
            "logical_processors": std::thread::available_parallelism().map(|value| value.get()).unwrap_or(0),
            "load_calibration_rustc_vV_ms": calibration,
        },
        "commands": [
            "cargo test -p ravum-unit-graph",
            "cargo test -p ravum-unit-graph --test central_evidence",
            "cargo run -q -p ravum-unit-graph --example central_case (n/a: evidence is generated by the test)",
        ],
        "fixture": "fixtures/multicrate (core -> api -> consumer -> application, plus features and buildscript)",
        "limits": [
            "one local toolchain (rustc 1.95.0 x86_64-pc-windows-msvc) and one target (host)",
            "a bounded campaign on a tiny fixture, not a large real workspace",
            "the targeted chain compiles lib targets only; binaries, features, build scripts and external dependencies deliberately fall back to Cargo",
            "no claim of whole-program correctness: statuses are observations, never PROVED",
            "the .rmeta determinism is measured on this toolchain only; on a non-deterministic toolchain the conservative strategy is to revalidate dependents (documented)",
        ],
    });

    // --- Scenario 0: determinism of the interface artifacts ------------------
    let determinism_workspace = workspace("determinism");
    let first = campaign(
        &determinism_workspace,
        &determinism_workspace.join("state-a"),
    );
    let second = campaign(
        &determinism_workspace,
        &determinism_workspace.join("state-b"),
    );
    let mut determinism = Vec::new();
    let mut deterministic = true;
    for unit in ["core", "api", "consumer"] {
        let a = first
            .unit(unit)
            .and_then(|outcome| outcome.interface_id.clone());
        let b = second
            .unit(unit)
            .and_then(|outcome| outcome.interface_id.clone());
        deterministic &= a.is_some() && a == b;
        determinism.push(json!({"unit": unit, "first_interface_id": a, "second_interface_id": b}));
    }
    assert!(
        deterministic,
        "the .rmeta of the targeted units must be deterministic on this toolchain: {determinism:?}"
    );
    evidence["determinism"] = json!({
        "identical_compilations_same_sha256": deterministic,
        "units": determinism,
        "conservative_rule_if_nondeterministic": "dependents are revalidated instead of HIT when interface ids differ",
    });
    let _ = std::fs::remove_dir_all(&determinism_workspace);

    // --- Scenario 1: baseline (everything reachable) ------------------------
    let baseline_workspace = workspace("baseline");
    let baseline = campaign(&baseline_workspace, &baseline_workspace.join("state"));
    assert!(baseline.valid, "baseline must be valid");
    assert_eq!(
        baseline.unit("core").map(|outcome| outcome.status),
        Some(UnitStatus::TargetedOk)
    );
    assert_eq!(
        baseline.unit("core").map(|outcome| outcome.status),
        Some(UnitStatus::TargetedOk)
    );
    evidence["baseline"] = json!({
        "summary": summary(&baseline),
        "unit_core": unit_status(&baseline, "core"),
        "unit_api": unit_status(&baseline, "api"),
        "unit_consumer": unit_status(&baseline, "consumer"),
        "unit_application": unit_status(&baseline, "application"),
        "unit_features": unit_status(&baseline, "features"),
        "unit_buildscript": unit_status(&baseline, "buildscript"),
        "traces": traces(&baseline),
    });

    // --- Scenario 2: central breaking signature change ----------------------
    let breaking_workspace = workspace("breaking");
    let breaking_state = breaking_workspace.join("state");
    let before = campaign(&breaking_workspace, &breaking_state);
    let api_content_before = before
        .unit("api")
        .map(|outcome| outcome.content_id.clone())
        .expect("api content");
    edit(
        &breaking_workspace,
        "core/src/lib.rs",
        "pub fn scale(x: f64) -> f64 {\n    x * 2.0\n}",
        "pub fn scale(x: f64, factor: f64) -> f64 {\n    x * factor\n}",
    );
    edit(
        &breaking_workspace,
        "core/src/lib.rs",
        "scale(x) * internal_factor()",
        "scale(x, 2.0) * internal_factor()",
    );
    let breaking = campaign(&breaking_workspace, &breaking_state);
    assert!(
        !breaking.valid,
        "the breaking change must invalidate the campaign"
    );
    assert_eq!(
        breaking.unit("core").map(|outcome| outcome.status),
        Some(UnitStatus::TargetedOk)
    );
    assert_eq!(
        breaking.unit("api").map(|outcome| outcome.status),
        Some(UnitStatus::Error)
    );
    assert_eq!(
        breaking.unit("api").map(|outcome| &outcome.content_id),
        Some(&api_content_before),
        "api sources are unchanged: the error must come from the core interface"
    );
    assert!(
        breaking
            .rustc_traces
            .iter()
            .any(|trace| trace.unit == "api"),
        "rustc must be invoked for api after the signature change"
    );
    assert!(
        breaking
            .cargo_traces
            .iter()
            .any(|trace| trace.unit == "consumer" || trace.unit == "application"),
        "the downstream units must be rechecked through the Cargo oracle"
    );
    let api_diagnostics = breaking
        .unit("api")
        .map(|outcome| outcome.diagnostics.clone())
        .unwrap_or_default();
    assert!(
        api_diagnostics.iter().any(|line| line.contains("error")),
        "the break diagnostic must be present: {api_diagnostics:?}"
    );
    evidence["central_breaking_signature_change"] = json!({
        "change": "core::scale(f64) -> core::scale(f64, f64); core internal call site updated",
        "expected": "core TargetedOk, api ERROR with real diagnostic, downstream rechecked via Cargo, no stale HIT",
        "summary_before": summary(&before),
        "summary_after": summary(&breaking),
        "unit_core_after": unit_status(&breaking, "core"),
        "unit_api_after": unit_status(&breaking, "api"),
        "unit_consumer_after": unit_status(&breaking, "consumer"),
        "unit_application_after": unit_status(&breaking, "application"),
        "api_content_id_unchanged": breaking.unit("api").map(|outcome| &outcome.content_id) == Some(&api_content_before),
        "api_hit": breaking.unit("api").map(|outcome| outcome.hit),
        "api_diagnostics": api_diagnostics,
        "traces_after": traces(&breaking),
    });

    // --- Scenario 3: compatible interface addition (success path) -----------
    let compatible_workspace = workspace("compatible");
    let compatible_state = compatible_workspace.join("state");
    let before = campaign(&compatible_workspace, &compatible_state);
    assert!(before.valid);
    edit(
        &compatible_workspace,
        "core/src/lib.rs",
        "/// Adds two values (used by the \"symbol removal\" negative case).",
        "pub fn cube(x: f64) -> f64 {\n    x * x * x\n}\n\n/// Adds two values (used by the \"symbol removal\" negative case).",
    );
    let compatible = campaign(&compatible_workspace, &compatible_state);
    assert!(compatible.valid, "compatible addition must stay valid");
    for unit in ["core", "api", "consumer"] {
        assert_eq!(
            compatible.unit(unit).map(|outcome| outcome.status),
            Some(UnitStatus::TargetedOk),
            "{unit} must be revalidated successfully"
        );
        assert_eq!(
            compatible.unit(unit).map(|outcome| outcome.hit),
            Some(false),
            "{unit} must not be a stale HIT after a core interface change"
        );
    }
    assert!(compatible
        .rustc_traces
        .iter()
        .any(|trace| trace.unit == "api"));
    assert!(compatible
        .rustc_traces
        .iter()
        .any(|trace| trace.unit == "consumer"));
    evidence["compatible_interface_addition"] = json!({
        "change": "core gains a new public function `cube`; existing call sites unchanged",
        "expected": "core, api and consumer really revalidated (not HIT), valid campaign",
        "summary_after": summary(&compatible),
        "unit_core_after": unit_status(&compatible, "core"),
        "unit_api_after": unit_status(&compatible, "api"),
        "unit_consumer_after": unit_status(&compatible, "consumer"),
        "unit_application_after": unit_status(&compatible, "application"),
        "traces_after": traces(&compatible),
    });

    // --- 16-case table (from the mandatory negative suite) ------------------
    evidence["negative_cases"] = json!([
        {"id": 1, "case": "corps privé sans impact d'interface", "test": "case_01_private_body_change_keeps_dependents_valid"},
        {"id": 2, "case": "signature publique", "test": "case_02_public_signature_change_revalidates_dependents"},
        {"id": 3, "case": "suppression de symbole", "test": "case_03_symbol_removal_is_detected_in_dependents"},
        {"id": 4, "case": "type partagé", "test": "case_04_shared_type_change_is_detected"},
        {"id": 5, "case": "implémentation de trait", "test": "case_05_trait_impl_change_revalidates_without_breaking"},
        {"id": 6, "case": "feature", "test": "case_06_feature_package_falls_back_to_cargo"},
        {"id": 7, "case": "build script", "test": "case_07_build_script_falls_back_to_cargo"},
        {"id": 8, "case": "dépendance externe", "test": "case_08_external_dependency_falls_back_to_cargo"},
        {"id": 9, "case": "contrat", "test": "case_09_contract_change_is_treated_conservatively"},
        {"id": 10, "case": "configuration cible", "test": "case_10_foreign_target_forces_full_fallback"},
        {"id": 11, "case": "dépendance transitive", "test": "case_11_transitive_dependency_is_invalidated"},
        {"id": 12, "case": "erreur uniquement dans un dépendant", "test": "case_12_dependent_only_error_is_detected_without_source_change"},
        {"id": 13, "case": "modification pendant la vérification (STALE)", "test": "case_13_modification_during_verification_is_stale"},
        {"id": 14, "case": "réutilisation d'artefact périmé (refus)", "test": "case_14_stale_artifact_is_never_reused"},
        {"id": 15, "case": "changement de version de compilateur", "test": "case_15_toolchain_change_invalidates_the_cache"},
        {"id": 16, "case": "conflit entre deux propositions (MANUAL_REQUIRED)", "test": "case_16_conflicting_proposals_require_manual_decision"}
    ]);

    let text = serde_json::to_string_pretty(&evidence).expect("serialize evidence");
    let write_repository = std::env::var("RAVUM_WRITE_EVIDENCE").as_deref() == Ok("1");
    let evidence_dir = if write_repository {
        repo_root().join("evidence")
    } else {
        repo_root().join("target").join("ravum-evidence")
    };
    std::fs::create_dir_all(&evidence_dir).expect("evidence dir");
    let path = evidence_dir.join("mvp0c-cross-crate-2026-11-09.json");
    std::fs::write(&path, format!("{text}\n")).expect("write evidence");
    if write_repository {
        println!("evidence written (repository): {}", path.display());
    } else {
        println!(
            "evidence written (ignored dir; set RAVUM_WRITE_EVIDENCE=1 to write the repository copy): {}",
            path.display()
        );
    }
}
