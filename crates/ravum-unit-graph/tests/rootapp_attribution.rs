//! MVP-0c L1.4 — regression: a workspace root package must not aggregate the
//! sources of its nested members in its `content_id`.
//!
//! Fixture: `fixtures/unit-graph-rootapp` (`root_app` = `[package]` + `[workspace]`
//! at the same directory, with a nested member `core`). All runs are real: the
//! fixture is copied to a temp directory and compiled by the local rustc.

use ravum_unit_graph::{run_chain, ChainOptions, ContextSpec, RunReport, UnitGraph};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("fixtures/unit-graph-rootapp")
}

fn unique(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "ravum-unit-graph-rootapp-{name}-{}-{nanos}",
        std::process::id()
    ))
}

fn copy_dir(source: &Path, target: &Path) {
    std::fs::create_dir_all(target).expect("create target");
    for entry in std::fs::read_dir(source).expect("read fixture").flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if name == "target" || name == "Cargo.lock" {
            continue;
        }
        let destination = target.join(&name);
        if path.is_dir() {
            copy_dir(&path, &destination);
        } else {
            std::fs::copy(&path, &destination).expect("copy fixture file");
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
    static CONTEXT: OnceLock<ContextSpec> = OnceLock::new();
    CONTEXT
        .get_or_init(|| ContextSpec::from_rustc(Vec::new()).expect("rustc -vV"))
        .clone()
}

fn run(workspace: &Path, state: &Path) -> RunReport {
    let graph = UnitGraph::discover(workspace).expect("discover root-app workspace");
    run_chain(&graph, &ChainOptions::new(state.to_path_buf(), context())).expect("run chain")
}

fn edit(workspace: &Path, relative: &str, old: &str, new: &str) {
    let path = workspace.join(relative);
    let content = std::fs::read_to_string(&path).expect("read source");
    assert!(content.contains(old), "{} lacks {old:?}", path.display());
    std::fs::write(&path, content.replacen(old, new, 1)).expect("write source");
}

/// Strips the Windows verbatim prefix, exactly like the graph does: comparison
/// must be about the same file, not about the same spelling of its path.
fn normalize(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy().to_string();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        return PathBuf::from(rest.to_string());
    }
    path
}

fn rel(workspace: &Path, path: &Path) -> String {
    let canonical = std::fs::canonicalize(workspace)
        .map(normalize)
        .unwrap_or_else(|_| workspace.to_path_buf());
    for base in [workspace, canonical.as_path()] {
        if let Ok(relative) = path.strip_prefix(base) {
            return relative.to_string_lossy().replace('\\', "/");
        }
    }
    // A short (8.3) `%TEMP%` name on a CI runner does not match the long canonical
    // path discovery reports: compare canonical forms before giving up.
    if let Ok(file_canonical) = std::fs::canonicalize(path) {
        let file_canonical = normalize(file_canonical);
        for base in [workspace, canonical.as_path()] {
            if let Ok(relative) = file_canonical.strip_prefix(base) {
                return relative.to_string_lossy().replace('\\', "/");
            }
        }
    }
    path.to_string_lossy().replace('\\', "/")
}

fn unit_files(workspace: &Path, graph: &UnitGraph, unit: &str) -> Vec<String> {
    graph
        .unit(unit)
        .expect("unit exists")
        .source_files
        .iter()
        .map(|path| rel(workspace, path))
        .collect()
}

fn content_id(report: &RunReport, unit: &str) -> String {
    report
        .unit(unit)
        .map(|outcome| outcome.content_id.clone())
        .unwrap_or_default()
}

#[test]
fn root_package_does_not_aggregate_member_sources() {
    let workspace = workspace("attribution");
    let graph = UnitGraph::discover(&workspace).expect("discover");
    let root_files = unit_files(&workspace, &graph, "root_app");
    let core_files = unit_files(&workspace, &graph, "core");
    println!("root_app files: {root_files:?}");
    println!("core files: {core_files:?}");

    assert!(root_files.contains(&"Cargo.toml".to_owned()));
    assert!(root_files.contains(&"src/lib.rs".to_owned()));
    assert!(
        !root_files.iter().any(|path| path.starts_with("core/")),
        "root package must not own member sources: {root_files:?}"
    );
    assert!(core_files.contains(&"core/Cargo.toml".to_owned()));
    assert!(core_files.contains(&"core/src/lib.rs".to_owned()));
    assert!(
        !core_files
            .iter()
            .any(|path| path == "Cargo.toml" || path == "src/lib.rs"),
        "member must not own root-package sources: {core_files:?}"
    );
    // No double attribution.
    for file in &root_files {
        assert!(!core_files.contains(file), "{file} attributed twice");
    }
    // The workspace root manifest belongs to the root package exactly once.
    let owners = graph
        .units()
        .values()
        .filter(|unit| {
            unit.source_files
                .iter()
                .any(|path| rel(&workspace, path) == "Cargo.toml")
        })
        .map(|unit| unit.name.clone())
        .collect::<Vec<_>>();
    assert_eq!(owners, vec!["root_app".to_owned()]);
    let _ = std::fs::remove_dir_all(&workspace);
}

#[test]
fn member_change_keeps_root_content_id_and_root_change_keeps_member_content_id() {
    let workspace = workspace("regression");
    let state = workspace.join("state");

    let baseline = run(&workspace, &state);
    assert!(baseline.valid, "baseline must be valid: {baseline:?}");
    let root_baseline = content_id(&baseline, "root_app");
    let core_baseline = content_id(&baseline, "core");
    let mut evidence = json!({
        "baseline": {
            "root_app_content_id": root_baseline,
            "core_content_id": core_baseline,
            "root_app_files": unit_files(&workspace, &UnitGraph::discover(&workspace).unwrap(), "root_app"),
            "core_files": unit_files(&workspace, &UnitGraph::discover(&workspace).unwrap(), "core"),
            "root_app_status": baseline.unit("root_app").map(|unit| unit.status.as_str()),
            "core_status": baseline.unit("core").map(|unit| unit.status.as_str()),
        },
        "steps": Vec::<Value>::new(),
    });
    let mut steps: Vec<Value> = Vec::new();

    // 1) Member private-body change: member content changes, root content must not.
    edit(
        &workspace,
        "core/src/lib.rs",
        "fn internal_offset() -> f64 {\n    0.0\n}",
        "fn internal_offset() -> f64 {\n    0.5\n}",
    );
    let member_changed = run(&workspace, &state);
    let root_after_member = content_id(&member_changed, "root_app");
    let core_after_member = content_id(&member_changed, "core");
    assert_ne!(
        core_after_member, core_baseline,
        "member content must change"
    );
    assert_eq!(
        root_after_member, root_baseline,
        "root content_id must NOT include member sources"
    );
    steps.push(json!({
        "step": "member-private-body-change",
        "root_app_content_id": root_after_member,
        "core_content_id": core_after_member,
        "root_app_hit": member_changed.unit("root_app").map(|unit| unit.hit),
        "root_app_status": member_changed.unit("root_app").map(|unit| unit.status.as_str()),
        "core_hit": member_changed.unit("core").map(|unit| unit.hit),
        "core_status": member_changed.unit("core").map(|unit| unit.status.as_str()),
    }));

    // 2) Member interface change: root content stays minimal, but as a *dependent*
    //    it must be really revalidated (the dependency interface changed).
    edit(
        &workspace,
        "core/src/lib.rs",
        "pub fn shifted(x: f64) -> f64 {",
        "pub fn cube(x: f64) -> f64 { x * x * x }\n\npub fn shifted(x: f64) -> f64 {",
    );
    let interface_changed = run(&workspace, &state);
    assert_eq!(
        content_id(&interface_changed, "root_app"),
        root_baseline,
        "root content stays independent from member sources"
    );
    steps.push(json!({
        "step": "member-interface-change",
        "root_app_content_id": content_id(&interface_changed, "root_app"),
        "core_content_id": content_id(&interface_changed, "core"),
        "root_app_hit": interface_changed.unit("root_app").map(|unit| unit.hit),
        "root_app_status": interface_changed.unit("root_app").map(|unit| unit.status.as_str()),
        "core_status": interface_changed.unit("core").map(|unit| unit.status.as_str()),
    }));

    // 3) Root change: root content changes, member content must not.
    edit(
        &workspace,
        "src/lib.rs",
        "pub fn version() -> &'static str {\n    \"root-v1\"\n}",
        "pub fn version() -> &'static str {\n    \"root-v2\"\n}",
    );
    let root_changed = run(&workspace, &state);
    assert_ne!(
        content_id(&root_changed, "root_app"),
        root_baseline,
        "root content must change when the root package changes"
    );
    assert_eq!(
        content_id(&root_changed, "core"),
        content_id(&interface_changed, "core"),
        "member content must not change when the root package changes"
    );
    steps.push(json!({
        "step": "root-change",
        "root_app_content_id": content_id(&root_changed, "root_app"),
        "core_content_id": content_id(&root_changed, "core"),
        "root_app_hit": root_changed.unit("root_app").map(|unit| unit.hit),
        "root_app_status": root_changed.unit("root_app").map(|unit| unit.status.as_str()),
        "core_hit": root_changed.unit("core").map(|unit| unit.hit),
        "core_status": root_changed.unit("core").map(|unit| unit.status.as_str()),
    }));
    evidence["steps"] = Value::Array(steps);
    evidence["final"] = json!({
        "root_app_files": unit_files(&workspace, &UnitGraph::discover(&workspace).unwrap(), "root_app"),
        "core_files": unit_files(&workspace, &UnitGraph::discover(&workspace).unwrap(), "core"),
    });
    evidence["reproduction_before_fix"] = json!({
        "command": "cargo test -p ravum-unit-graph --test rootapp_attribution -- --test-threads=1 root_package_does_not_aggregate_member_sources",
        "observed_root_app_files": ["Cargo.toml", "core/Cargo.toml", "core/src/lib.rs", "src/lib.rs"],
        "assertion_failed": "root package must not own member sources: [\"Cargo.toml\", \"core/Cargo.toml\", \"core/src/lib.rs\", \"src/lib.rs\"]",
        "cause": "UnitGraph::collect_source_files walked root_dir recursively and did not stop at nested manifests/members",
    });
    evidence["commands"] = json!([
        "cargo test -p ravum-unit-graph --test rootapp_attribution -- --test-threads=1 --nocapture",
        "cargo test -p ravum-unit-graph -- --test-threads=1",
    ]);
    evidence["valid"] = json!(true);
    evidence["limits"] = json!([
        "tiny fixture, one toolchain; body-only member change may revalidate the dependent only if rustc's rmeta changes (observed status is recorded, not assumed)",
        "files outside every unit root remain ignored (pre-existing behaviour); every file below a package root is now attributed exactly once",
        "no PROVED: these are bounded observations",
    ]);
    // Repository evidence is opt-in: a plain `cargo test --workspace` must leave
    // the git tree clean (same convention as `central_evidence`).
    let write_repository = std::env::var("RAVUM_WRITE_EVIDENCE").as_deref() == Ok("1");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let evidence_dir = if write_repository {
        root.join("evidence")
    } else {
        root.join("target").join("ravum-evidence")
    };
    std::fs::create_dir_all(&evidence_dir).expect("evidence dir");
    let evidence_path = evidence_dir.join("mvp0c-unit-graph-rootapp-2026-11-09.json");
    std::fs::write(
        &evidence_path,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&evidence).expect("evidence")
        ),
    )
    .expect("write evidence");
    println!(
        "unit-graph rootapp evidence written ({}) (set RAVUM_WRITE_EVIDENCE=1 for the repository copy)",
        evidence_path.display()
    );
    let _ = std::fs::remove_dir_all(&workspace);
    // Silence the unused-function warning in some build configurations.
    let _ = rel;
}
