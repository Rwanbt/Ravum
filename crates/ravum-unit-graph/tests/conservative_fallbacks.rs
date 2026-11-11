//! Replis conservateurs pour les inconnues Cargo : cibles multiples, types de
//! crate non-lib, proc-macros, build scripts, configuration Cargo, macros à jeu
//! de fichiers non énumérable, sources illisibles.
//!
//! Deux niveaux de preuve :
//! * décisions pures via `UnitGraph::from_metadata` (JSON synthétique, déterministe,
//!   sans `cargo`) ;
//! * un workspace réel lib+bin compilé par `run_chain`, qui prouve que le repli
//!   atteint bien l'oracle Cargo et ne réutilise aucun artefact ciblé périmé.

use ravum_unit_graph::{
    run_chain, ChainOptions, ContextSpec, FallbackReason, UnitGraph, UnitKind, UnitStatus,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn tests_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ravum-tests")
}

fn workspace(name: &str) -> PathBuf {
    let root = tests_root().join(format!("unit-graph-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write manifest");
    std::fs::write(root.join("src/lib.rs"), "pub fn f() -> u32 { 1 }\n").expect("write lib");
    root
}

fn target(root: &Path, kind: &str, crate_types: &[&str]) -> Value {
    json!({
        "kind": [kind],
        "src_path": root.join("src/lib.rs").to_string_lossy(),
        "name": "demo",
        "edition": "2021",
        "crate_types": crate_types,
    })
}

fn metadata(root: &Path, targets: Vec<Value>, features: Value, dependencies: Value) -> Value {
    json!({
        "packages": [{
            "id": "demo 0.1.0 (path+file:///demo)",
            "name": "demo",
            "manifest_path": root.join("Cargo.toml").to_string_lossy(),
            "features": features,
            "dependencies": dependencies,
            "targets": targets,
        }],
        "workspace_members": ["demo 0.1.0 (path+file:///demo)"],
    })
}

fn single_metadata(root: &Path, targets: Vec<Value>) -> Value {
    metadata(root, targets, json!({}), json!([]))
}

fn reason_of(graph: &UnitGraph, unit: &str, host: &str) -> Option<FallbackReason> {
    graph.fallback_reason(graph.unit(unit).expect("unit in graph"), host, None)
}

const HOST: &str = "x86_64-pc-windows-msvc";

#[test]
fn a_plain_lib_stays_targeted() {
    let root = workspace("plain");
    let graph = UnitGraph::from_metadata(
        &root,
        &single_metadata(&root, vec![target(&root, "lib", &["lib"])]),
    )
    .expect("graph");
    let unit = graph.unit("demo").expect("unit");
    assert_eq!(unit.kind, UnitKind::Lib);
    assert_eq!(unit.target_count, 1);
    assert!(!unit.proc_macro);
    assert_eq!(reason_of(&graph, "demo", HOST), None);
}

#[test]
fn multiple_targets_force_a_cargo_fallback() {
    let root = workspace("multi-target");
    let graph = UnitGraph::from_metadata(
        &root,
        &single_metadata(
            &root,
            vec![
                target(&root, "lib", &["lib"]),
                target(&root, "bin", &["bin"]),
            ],
        ),
    )
    .expect("graph");
    assert_eq!(graph.unit("demo").expect("unit").target_count, 2);
    assert_eq!(
        reason_of(&graph, "demo", HOST),
        Some(FallbackReason::MultipleTargets),
        "one rustc invocation would silently ignore the binary target"
    );
}

#[test]
fn integration_tests_and_examples_do_not_force_a_fallback() {
    let root = workspace("extra-targets");
    let graph = UnitGraph::from_metadata(
        &root,
        &single_metadata(
            &root,
            vec![
                target(&root, "lib", &["lib"]),
                target(&root, "test", &["test"]),
                target(&root, "example", &["example"]),
                target(&root, "bench", &["bench"]),
            ],
        ),
    )
    .expect("graph");
    let unit = graph.unit("demo").expect("unit");
    assert_eq!(
        unit.target_count, 1,
        "only lib/bin/proc-macro targets count: cargo check -p does not build tests/examples"
    );
    assert_eq!(
        reason_of(&graph, "demo", HOST),
        None,
        "over-conservatism here would reduce every real library to the Cargo oracle"
    );
}

#[test]
fn a_non_lib_crate_type_forces_a_cargo_fallback() {
    let root = workspace("cdylib");
    let graph = UnitGraph::from_metadata(
        &root,
        &single_metadata(&root, vec![target(&root, "lib", &["cdylib", "rlib"])]),
    )
    .expect("graph");
    assert_eq!(
        reason_of(&graph, "demo", HOST),
        Some(FallbackReason::NonLibCrateType),
        "the runner only builds metadata with --crate-type lib"
    );
}

#[test]
fn a_proc_macro_target_forces_a_cargo_fallback() {
    let root = workspace("proc-macro");
    let graph = UnitGraph::from_metadata(
        &root,
        &single_metadata(&root, vec![target(&root, "proc-macro", &["proc-macro"])]),
    )
    .expect("graph");
    assert!(graph.unit("demo").expect("unit").proc_macro);
    assert_eq!(
        reason_of(&graph, "demo", HOST),
        Some(FallbackReason::ProcMacroTarget)
    );
}

#[test]
fn a_build_script_forces_a_cargo_fallback() {
    let root = workspace("build-script");
    std::fs::write(root.join("build.rs"), "fn main() {}\n").expect("write build.rs");
    let graph = UnitGraph::from_metadata(
        &root,
        &single_metadata(&root, vec![target(&root, "lib", &["lib"])]),
    )
    .expect("graph");
    assert!(graph.unit("demo").expect("unit").build_script);
    assert_eq!(
        reason_of(&graph, "demo", HOST),
        Some(FallbackReason::BuildScript),
        "OUT_DIR/build-script output is not modelled"
    );
}

#[test]
fn a_cargo_config_file_forces_a_cargo_fallback() {
    let root = workspace("build-config");
    std::fs::create_dir_all(root.join(".cargo")).expect("create .cargo");
    std::fs::write(
        root.join(".cargo/config.toml"),
        "[build]\nrustflags = [\"--cfg\", \"ravum_custom\"]\n",
    )
    .expect("write cargo config");
    let graph = UnitGraph::from_metadata(
        &root,
        &single_metadata(&root, vec![target(&root, "lib", &["lib"])]),
    )
    .expect("graph");
    assert_eq!(
        reason_of(&graph, "demo", HOST),
        Some(FallbackReason::BuildConfig),
        "a targeted rustc does not read .cargo/config.toml"
    );
}

#[test]
fn a_path_attribute_forces_a_cargo_fallback() {
    let root = workspace("path-attribute");
    std::fs::write(
        root.join("src/lib.rs"),
        "#[path = \"other.rs\"]\nmod other;\npub fn f() -> u32 { 1 }\n",
    )
    .expect("write lib");
    let graph = UnitGraph::from_metadata(
        &root,
        &single_metadata(&root, vec![target(&root, "lib", &["lib"])]),
    )
    .expect("graph");
    assert_eq!(
        reason_of(&graph, "demo", HOST),
        Some(FallbackReason::PathAttribute),
        "the module file set can leave the enumerated sources"
    );
}

#[test]
fn include_macros_force_a_cargo_fallback() {
    let root = workspace("include-macro");
    std::fs::write(
        root.join("src/lib.rs"),
        "const DATA: &[u8] = include_bytes!(\"data.bin\");\npub fn f() -> u32 { 1 }\n",
    )
    .expect("write lib");
    let graph = UnitGraph::from_metadata(
        &root,
        &single_metadata(&root, vec![target(&root, "lib", &["lib"])]),
    )
    .expect("graph");
    assert_eq!(
        reason_of(&graph, "demo", HOST),
        Some(FallbackReason::IncludeMacro)
    );
}

#[test]
fn env_macros_force_a_cargo_fallback() {
    let root = workspace("env-macro");
    std::fs::write(
        root.join("src/lib.rs"),
        "pub fn f() -> &'static str { env!(\"RAVUM_TEST_ENV\") }\n",
    )
    .expect("write lib");
    let graph = UnitGraph::from_metadata(
        &root,
        &single_metadata(&root, vec![target(&root, "lib", &["lib"])]),
    )
    .expect("graph");
    assert_eq!(
        reason_of(&graph, "demo", HOST),
        Some(FallbackReason::EnvMacro)
    );
}

#[test]
fn an_unreadable_source_forces_a_cargo_fallback() {
    let root = workspace("unreadable");
    let graph = UnitGraph::from_metadata(
        &root,
        &single_metadata(&root, vec![target(&root, "lib", &["lib"])]),
    )
    .expect("graph");
    // The graph enumerated the file; removing it must make the scan conservative,
    // never silently "no macro found".
    std::fs::remove_file(root.join("src/lib.rs")).expect("remove source");
    assert_eq!(
        reason_of(&graph, "demo", HOST),
        Some(FallbackReason::SourceUnreadable)
    );
}

#[test]
fn an_optional_dependency_forces_a_fallback_even_with_resolved_features() {
    let root = workspace("optional-dep");
    let mut metadata = metadata(
        &root,
        vec![target(&root, "lib", &["lib"])],
        json!({"extra": []}),
        json!([{
            "name": "demo-util",
            "kind": "normal",
            "optional": true,
            "features": [],
            "path": root.join("util").to_string_lossy(),
        }]),
    );
    // A resolved activation set, so the check reaches the optional-edge branch and
    // not the "activation unknown" branch.
    metadata["resolve"] = json!({
        "nodes": [{
            "id": "demo 0.1.0 (path+file:///demo)",
            "features": []
        }]
    });
    // The optional path dependency must also be a workspace member for discovery to
    // stay coherent; make it a second package.
    metadata["packages"]
        .as_array_mut()
        .expect("packages")
        .push(json!({
            "id": "demo-util 0.1.0 (path+file:///demo-util)",
            "name": "demo-util",
            "manifest_path": root.join("util/Cargo.toml").to_string_lossy(),
            "features": {},
            "dependencies": [],
            "targets": [{
                "kind": ["lib"],
                "src_path": root.join("util/src/lib.rs").to_string_lossy(),
                "name": "demo_util",
                "edition": "2021",
                "crate_types": ["lib"],
            }],
        }));
    metadata["workspace_members"]
        .as_array_mut()
        .expect("members")
        .push(json!("demo-util 0.1.0 (path+file:///demo-util)"));
    std::fs::create_dir_all(root.join("util/src")).expect("create util");
    std::fs::write(
        root.join("util/Cargo.toml"),
        "[package]\nname = \"demo-util\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write util manifest");
    std::fs::write(root.join("util/src/lib.rs"), "pub fn u() {}\n").expect("write util lib");

    let graph = UnitGraph::from_metadata(&root, &metadata).expect("graph");
    let unit = graph.unit("demo").expect("unit");
    assert!(unit.features_resolved, "the resolve section is present");
    assert_eq!(
        reason_of(&graph, "demo", HOST),
        Some(FallbackReason::OptionalDependency),
        "an optional edge the targeted chain cannot filter forces the Cargo oracle"
    );
}

// ---------------------------------------------------------------------------
// Bout en bout : un vrai workspace lib+bin passe par l'oracle Cargo.
// ---------------------------------------------------------------------------

#[test]
fn a_real_multi_target_workspace_falls_back_to_cargo_without_reusing_artifacts() {
    let root = tests_root().join(format!("unit-graph-real-multi-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("Cargo.toml"),
        concat!(
            "[package]\n",
            "name = \"demo\"\n",
            "version = \"0.1.0\"\n",
            "edition = \"2021\"\n\n",
            "[lib]\n",
            "name = \"demo\"\n",
            "path = \"src/lib.rs\"\n\n",
            "[[bin]]\n",
            "name = \"demo-bin\"\n",
            "path = \"src/main.rs\"\n\n",
            // Standalone: do not be captured by the Ravum workspace that contains
            // `target/ravum-tests`.
            "[workspace]\n",
        ),
    )
    .expect("write manifest");
    std::fs::write(root.join("src/lib.rs"), "pub fn f() -> u32 { 1 }\n").expect("write lib");
    std::fs::write(
        root.join("src/main.rs"),
        "fn main() { println!(\"{}\", demo::f()); }\n",
    )
    .expect("write bin");

    let context = ContextSpec::from_rustc(Vec::new()).expect("local rustc -vV");
    let graph = UnitGraph::discover(&root).expect("discover");
    let unit = graph.unit("demo").expect("unit");
    assert_eq!(unit.target_count, 2);
    let state = root.join("target/ravum-state");
    let report =
        run_chain(&graph, &ChainOptions::new(state.clone(), context.clone())).expect("campaign");
    let outcome = report.unit("demo").expect("demo outcome");
    assert_eq!(outcome.status, UnitStatus::FallbackPackage);
    assert_eq!(outcome.fallback_reason.as_deref(), Some("multiple_targets"));
    assert!(
        outcome.cache_key.is_empty(),
        "a Cargo fallback never carries a targeted cache key"
    );
    assert!(
        report.rustc_traces.is_empty(),
        "no targeted artifact may be produced for the fallback unit"
    );

    // A second run goes through Cargo again: no stale targeted verdict is reused.
    let second = run_chain(&graph, &ChainOptions::new(state, context)).expect("second campaign");
    let second_outcome = second.unit("demo").expect("demo outcome");
    assert_eq!(second_outcome.status, UnitStatus::FallbackPackage);
    assert!(!second_outcome.hit);
    assert!(second.cache_hits == 0);
    let _ = std::fs::remove_dir_all(&root);
}
