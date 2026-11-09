//! Conservative workspace unit graph, discovered through the public
//! `cargo metadata --no-deps` interface (never a second `Cargo.toml` parser).
//!
//! Any uncertainty is explicit: discovery failures are typed, and each unit can
//! report a [`FallbackReason`] that forces the Cargo oracle instead of inventing
//! an independence between crates.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Typed failure of the workspace model. There is no "best effort" graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphError {
    pub kind: DiscoverError,
    pub message: String,
}

impl GraphError {
    pub fn new(kind: DiscoverError, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn is_unavailable(&self) -> bool {
        matches!(
            self.kind,
            DiscoverError::CargoMissing | DiscoverError::CargoFailed
        )
    }
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for GraphError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoverError {
    /// `cargo` could not be spawned: the whole analysis must fall back.
    CargoMissing,
    /// `cargo metadata` exited non-zero: the whole analysis must fall back.
    CargoFailed,
    /// The metadata JSON is malformed or lacks a required field.
    ManifestInvalid,
    /// A path dependency does not resolve to a workspace member, or a member is
    /// missing from the metadata: independence cannot be established.
    IncoherentGraph,
    /// The dependency graph contains a cycle.
    Cycle,
    /// A dependency version/target cannot be matched to a member deterministically.
    AmbiguousDependency,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitKind {
    Lib,
    Bin,
    Other,
}

impl UnitKind {
    pub fn as_str(self) -> &'static str {
        match self {
            UnitKind::Lib => "lib",
            UnitKind::Bin => "bin",
            UnitKind::Other => "other",
        }
    }
}

/// One declared dependency of a unit, as far as the graph needs to know.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dependency {
    /// Name used by the dependent in `extern crate` / paths (rename-aware).
    pub extern_name: String,
    /// Package name from the metadata.
    pub package: String,
    /// `None` for a non-path (registry/git) dependency.
    pub path: Option<PathBuf>,
    /// `true` when the dependency is not a `path` dependency.
    pub external: bool,
    /// `true` when the dependency is (or targets) a proc-macro.
    pub proc_macro: bool,
    /// Features requested on the dependency (non-empty means not enumerable here).
    pub features: Vec<String>,
    pub optional: bool,
}

/// One compilable unit of the workspace (one package, one lib/bin target).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Unit {
    pub name: String,
    pub crate_name: String,
    pub manifest_path: PathBuf,
    pub root_dir: PathBuf,
    pub edition: String,
    pub kind: UnitKind,
    pub src_path: PathBuf,
    pub declared_features: Vec<String>,
    pub build_script: bool,
    pub dependencies: Vec<Dependency>,
    pub dependents: Vec<String>,
    /// Every source file that contributes to the unit's content identity
    /// (`Cargo.toml`, `build.rs`, all `.rs` below `root_dir`).
    pub source_files: Vec<PathBuf>,
}

/// Semi-conservative fallback reason attached to a unit before any compilation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackReason {
    /// The unit is not a library target (binaries are checked by Cargo).
    BinaryTarget,
    /// A build script runs before compilation: its output is not modelled.
    BuildScript,
    /// The package declares features: the activated cfg set is not enumerable.
    FeaturesDeclared,
    /// At least one dependency comes from a registry/git source.
    ExternalDependency,
    /// A dependency is a proc-macro.
    ProcMacroDependency,
    /// The unit itself is a proc-macro.
    ProcMacroTarget,
    /// The sources use `include!`: file set not statically enumerable here.
    IncludeMacro,
    /// The sources use `env!`/`option_env!`: build environment not modelled.
    EnvMacro,
    /// The requested target differs from the host target.
    TargetMismatch,
    /// A direct dependency could not be compiled as a targeted unit.
    DependencyNotTargeted,
}

impl FallbackReason {
    pub fn as_str(self) -> &'static str {
        match self {
            FallbackReason::BinaryTarget => "binary_target",
            FallbackReason::BuildScript => "build_script",
            FallbackReason::FeaturesDeclared => "features_declared",
            FallbackReason::ExternalDependency => "external_dependency",
            FallbackReason::ProcMacroDependency => "proc_macro_dependency",
            FallbackReason::ProcMacroTarget => "proc_macro_target",
            FallbackReason::IncludeMacro => "include_macro",
            FallbackReason::EnvMacro => "env_macro",
            FallbackReason::TargetMismatch => "target_mismatch",
            FallbackReason::DependencyNotTargeted => "dependency_not_targeted",
        }
    }
}

/// The workspace unit graph. Members are ordered topologically (dependencies
/// first) and deterministically (name order for ties).
#[derive(Debug, Clone)]
pub struct UnitGraph {
    root: PathBuf,
    units: BTreeMap<String, Unit>,
    order: Vec<String>,
}

impl UnitGraph {
    /// Discovers the workspace at `root` with `cargo metadata --no-deps`.
    pub fn discover(root: &Path) -> Result<Self, GraphError> {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let canonical_root = std::fs::canonicalize(root).map_err(|error| {
            GraphError::new(
                DiscoverError::ManifestInvalid,
                format!(
                    "workspace root {} is not accessible: {error}",
                    root.display()
                ),
            )
        })?;
        let output = Command::new(&cargo)
            .arg("metadata")
            .arg("--no-deps")
            .arg("--format-version")
            .arg("1")
            .arg("--offline")
            .arg("--manifest-path")
            .arg(canonical_root.join("Cargo.toml"))
            .output()
            .map_err(|error| {
                GraphError::new(
                    DiscoverError::CargoMissing,
                    format!("cannot spawn `cargo metadata`: {error}"),
                )
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GraphError::new(
                DiscoverError::CargoFailed,
                format!(
                    "`cargo metadata --no-deps` failed with {}: {}",
                    output.status,
                    stderr.trim()
                ),
            ));
        }
        let metadata: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|error| {
                GraphError::new(
                    DiscoverError::ManifestInvalid,
                    format!("cargo metadata returned invalid JSON: {error}"),
                )
            })?;
        Self::from_metadata(&canonical_root, &metadata)
    }

    /// Builds the graph from an already parsed `cargo metadata` value (used by
    /// tests to exercise conservative classifications without recomputing it).
    pub fn from_metadata(root: &Path, metadata: &serde_json::Value) -> Result<Self, GraphError> {
        let packages = metadata["packages"].as_array().ok_or_else(|| {
            GraphError::new(
                DiscoverError::ManifestInvalid,
                "cargo metadata: `packages` array is missing",
            )
        })?;
        let members: BTreeSet<String> = metadata["workspace_members"]
            .as_array()
            .ok_or_else(|| {
                GraphError::new(
                    DiscoverError::ManifestInvalid,
                    "cargo metadata: `workspace_members` array is missing",
                )
            })?
            .iter()
            .filter_map(|value| value.as_str().map(str::to_owned))
            .collect();
        let mut packages_by_id: BTreeMap<&str, &serde_json::Value> = BTreeMap::new();
        for package in packages {
            if let Some(id) = package["id"].as_str() {
                packages_by_id.insert(id, package);
            }
        }

        // Package name -> package id, and proc-macro package ids.
        let mut name_to_id: BTreeMap<String, String> = BTreeMap::new();
        let mut proc_macro_ids: BTreeSet<String> = BTreeSet::new();
        for id in &members {
            let package = packages_by_id.get(id.as_str()).ok_or_else(|| {
                GraphError::new(
                    DiscoverError::IncoherentGraph,
                    format!("workspace member {id} is absent from `packages`"),
                )
            })?;
            let name = package["name"]
                .as_str()
                .ok_or_else(|| {
                    GraphError::new(
                        DiscoverError::ManifestInvalid,
                        format!("workspace member {id} has no name"),
                    )
                })?
                .to_owned();
            name_to_id.insert(name, id.clone());
            if package["targets"]
                .as_array()
                .map(|targets| {
                    targets.iter().any(|target| {
                        target["kind"]
                            .as_array()
                            .map(|kinds| {
                                kinds.iter().any(|kind| kind.as_str() == Some("proc-macro"))
                            })
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false)
            {
                proc_macro_ids.insert(id.clone());
            }
        }

        let mut units: BTreeMap<String, Unit> = BTreeMap::new();
        for (name, id) in &name_to_id {
            let package = packages_by_id
                .get(id.as_str())
                .copied()
                .expect("checked above");
            let manifest_path =
                PathBuf::from(package["manifest_path"].as_str().ok_or_else(|| {
                    GraphError::new(
                        DiscoverError::ManifestInvalid,
                        format!("package {name} has no manifest_path"),
                    )
                })?);
            let root_dir = manifest_path
                .parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| {
                    GraphError::new(
                        DiscoverError::ManifestInvalid,
                        format!("package {name} has an unusable manifest_path"),
                    )
                })?;
            let targets = package["targets"].as_array().cloned().unwrap_or_default();
            let lib_target = targets.iter().find(|target| {
                target["kind"]
                    .as_array()
                    .map(|kinds| kinds.iter().any(|kind| kind.as_str() == Some("lib")))
                    .unwrap_or(false)
            });
            let bin_target = targets.iter().find(|target| {
                target["kind"]
                    .as_array()
                    .map(|kinds| kinds.iter().any(|kind| kind.as_str() == Some("bin")))
                    .unwrap_or(false)
            });
            let (kind, target) = match (lib_target, bin_target) {
                (Some(target), _) => (UnitKind::Lib, target.clone()),
                (None, Some(target)) => (UnitKind::Bin, target.clone()),
                (None, None) => {
                    let Some(target) = targets.first() else {
                        continue;
                    };
                    (UnitKind::Other, target.clone())
                }
            };
            let src_path = PathBuf::from(target["src_path"].as_str().ok_or_else(|| {
                GraphError::new(
                    DiscoverError::ManifestInvalid,
                    format!("package {name} has a target without src_path"),
                )
            })?);
            let crate_name = target["name"]
                .as_str()
                .map(|name| name.replace('-', "_"))
                .ok_or_else(|| {
                    GraphError::new(
                        DiscoverError::ManifestInvalid,
                        format!("package {name} has a target without name"),
                    )
                })?;
            let edition = target["edition"]
                .as_str()
                .or_else(|| package["edition"].as_str())
                .unwrap_or("2021")
                .to_owned();
            let declared_features: Vec<String> = package["features"]
                .as_object()
                .map(|features| {
                    let mut names: Vec<String> = features.keys().cloned().collect();
                    names.sort();
                    names.retain(|name| name != "default");
                    names
                })
                .unwrap_or_default();
            let build_script = targets.iter().any(|target| {
                target["kind"]
                    .as_array()
                    .map(|kinds| {
                        kinds
                            .iter()
                            .any(|kind| kind.as_str() == Some("custom-build"))
                    })
                    .unwrap_or(false)
            }) || root_dir.join("build.rs").is_file();

            let mut dependencies = Vec::new();
            for dependency in package["dependencies"]
                .as_array()
                .cloned()
                .unwrap_or_default()
            {
                let kind = dependency["kind"].as_str().unwrap_or("normal");
                if kind != "normal" {
                    // build/dev dependencies do not contribute to the lib rmeta chain.
                    continue;
                }
                let package_name = dependency["name"].as_str().unwrap_or_default().to_owned();
                if package_name.is_empty() {
                    return Err(GraphError::new(
                        DiscoverError::ManifestInvalid,
                        format!("package {name}: dependency without name"),
                    ));
                }
                let extern_name = dependency["rename"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| package_name.replace('-', "_"));
                let path = dependency["path"].as_str().map(PathBuf::from);
                let dep_id = name_to_id.get(&package_name);
                let proc_macro = dep_id
                    .map(|id| proc_macro_ids.contains(id))
                    .unwrap_or(false);
                let features: Vec<String> = dependency["features"]
                    .as_array()
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(|value| value.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default();
                let optional = dependency["optional"].as_bool().unwrap_or(false);
                dependencies.push(Dependency {
                    extern_name,
                    package: package_name,
                    external: path.is_none(),
                    path: path.clone(),
                    proc_macro,
                    features,
                    optional,
                });
            }
            dependencies.sort_by(|a, b| a.extern_name.cmp(&b.extern_name));
            // Filled after every unit root is known: attribution must stop at
            // nested manifests/members (nearest-manifest rule, no double count).
            let source_files = Vec::new();
            units.insert(
                name.clone(),
                Unit {
                    name: name.clone(),
                    crate_name,
                    manifest_path,
                    root_dir,
                    edition,
                    kind,
                    src_path,
                    declared_features,
                    build_script,
                    dependencies,
                    dependents: Vec::new(),
                    source_files,
                },
            );
        }

        // Attribute every source file to exactly one package (nearest manifest):
        // a root package with `[workspace]` must not own its members' files.
        let all_roots: BTreeSet<PathBuf> =
            units.values().map(|unit| unit.root_dir.clone()).collect();
        for unit in units.values_mut() {
            unit.source_files =
                collect_source_files(&unit.root_dir, &all_roots).map_err(|error| {
                    GraphError::new(
                        DiscoverError::ManifestInvalid,
                        format!("cannot enumerate sources of {}: {error}", unit.name),
                    )
                })?;
        }

        // Validate path dependencies and fill reverse edges.
        let dir_to_name: BTreeMap<PathBuf, String> = units
            .values()
            .map(|unit| {
                let canonical =
                    std::fs::canonicalize(&unit.root_dir).unwrap_or_else(|_| unit.root_dir.clone());
                (canonical, unit.name.clone())
            })
            .collect();
        for unit in units.values() {
            for dependency in &unit.dependencies {
                if let Some(path) = &dependency.path {
                    let resolved = std::fs::canonicalize(path).map_err(|error| {
                        GraphError::new(
                            DiscoverError::IncoherentGraph,
                            format!(
                                "{}: path dependency {:?} cannot be resolved: {error}",
                                unit.manifest_path.display(),
                                dependency.package
                            ),
                        )
                    })?;
                    let Some(dep_name) = dir_to_name.get(&resolved) else {
                        return Err(GraphError::new(
                            DiscoverError::IncoherentGraph,
                            format!(
                                "{}: path dependency {:?} points outside the workspace members",
                                unit.manifest_path.display(),
                                dependency.package
                            ),
                        ));
                    };
                    if dep_name == &unit.name {
                        return Err(GraphError::new(
                            DiscoverError::Cycle,
                            format!("{}: depends on itself", unit.name),
                        ));
                    }
                    let _ = dep_name;
                } else if dependency.package.is_empty() {
                    return Err(GraphError::new(
                        DiscoverError::IncoherentGraph,
                        format!("{}: malformed dependency", unit.name),
                    ));
                }
            }
        }
        let mut reverse: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for unit in units.values() {
            for dependency in &unit.dependencies {
                if let Some(path) = &dependency.path {
                    if let Ok(resolved) = std::fs::canonicalize(path) {
                        if let Some(dep) = dir_to_name.get(&resolved) {
                            reverse
                                .entry(dep.clone())
                                .or_default()
                                .push(unit.name.clone());
                        }
                    }
                }
            }
        }
        for (name, mut dependents) in reverse {
            dependents.sort();
            dependents.dedup();
            if let Some(unit) = units.get_mut(&name) {
                unit.dependents = dependents;
            }
        }

        let order = topological_order(&units)?;
        Ok(Self {
            root: root.to_path_buf(),
            units,
            order,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn units(&self) -> &BTreeMap<String, Unit> {
        &self.units
    }

    pub fn unit(&self, name: &str) -> Option<&Unit> {
        self.units.get(name)
    }

    /// Topological order (dependencies before dependents).
    pub fn order(&self) -> &[String] {
        &self.order
    }

    /// Every transitive dependent of the given units (never includes the units
    /// themselves unless reachable through a cycle, which discovery rejects).
    pub fn dependents_closure(&self, changed: &[&str]) -> BTreeSet<String> {
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut stack: Vec<String> = changed.iter().map(|name| (*name).to_owned()).collect();
        while let Some(name) = stack.pop() {
            if let Some(unit) = self.units.get(&name) {
                for dependent in &unit.dependents {
                    if seen.insert(dependent.clone()) {
                        stack.push(dependent.clone());
                    }
                }
            }
        }
        seen
    }

    /// Static conservative fallback reason for a unit under `target`.
    ///
    /// A dependency that could not be compiled as a targeted unit is reported by
    /// the runner as [`FallbackReason::DependencyNotTargeted`], not here.
    pub fn fallback_reason(
        &self,
        unit: &Unit,
        host_target: &str,
        requested_target: Option<&str>,
    ) -> Option<FallbackReason> {
        if let Some(target) = requested_target {
            if target != host_target {
                return Some(FallbackReason::TargetMismatch);
            }
        }
        if unit.kind != UnitKind::Lib {
            return Some(FallbackReason::BinaryTarget);
        }
        if unit.build_script {
            return Some(FallbackReason::BuildScript);
        }
        if !unit.declared_features.is_empty() {
            return Some(FallbackReason::FeaturesDeclared);
        }
        if let Some(reason) = proc_macro_reason(unit) {
            return Some(reason);
        }
        if unit
            .dependencies
            .iter()
            .any(|dependency| dependency.external)
        {
            return Some(FallbackReason::ExternalDependency);
        }
        if unit
            .dependencies
            .iter()
            .any(|dependency| dependency.optional)
        {
            return Some(FallbackReason::FeaturesDeclared);
        }
        if unit
            .dependencies
            .iter()
            .any(|dependency| !dependency.features.is_empty())
        {
            return Some(FallbackReason::FeaturesDeclared);
        }
        if unit
            .dependencies
            .iter()
            .any(|dependency| dependency.proc_macro)
        {
            return Some(FallbackReason::ProcMacroDependency);
        }
        if source_uses_macro(unit, "include!") {
            return Some(FallbackReason::IncludeMacro);
        }
        if source_uses_macro(unit, "env!") || source_uses_macro(unit, "option_env!") {
            return Some(FallbackReason::EnvMacro);
        }
        None
    }
}

fn proc_macro_reason(unit: &Unit) -> Option<FallbackReason> {
    // The unit itself is not a proc-macro: metadata only gives lib/bin/other here,
    // proc-macro units are detected as `Other`/lib targets with a proc-macro kind.
    // Conservatively treat any unit whose only targets are proc-macro as fallback.
    if unit.kind == UnitKind::Other {
        return Some(FallbackReason::ProcMacroTarget);
    }
    None
}

fn source_uses_macro(unit: &Unit, needle: &str) -> bool {
    for path in &unit.source_files {
        if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(path) {
            if content.contains(needle) {
                return true;
            }
        }
    }
    false
}

/// Enumerates the files of one package, stopping at every nested manifest or
/// workspace member: files belong to the nearest package (never to a parent root
/// package too). `target`, VCS and daemon state directories are skipped.
fn collect_source_files(
    root_dir: &Path,
    all_roots: &BTreeSet<PathBuf>,
) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut stack = vec![root_dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(&current)?.flatten().collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                if matches!(name.as_str(), "target" | ".git" | ".ravum" | "node_modules") {
                    continue;
                }
                if path != *root_dir
                    && (all_roots.contains(&path) || path.join("Cargo.toml").is_file())
                {
                    // A nested package/member owns its own files.
                    continue;
                }
                stack.push(path);
                continue;
            }
            let extension = path.extension().and_then(|value| value.to_str());
            if matches!(extension, Some("rs")) || name == "Cargo.toml" || name == "build.rs" {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

fn topological_order(units: &BTreeMap<String, Unit>) -> Result<Vec<String>, GraphError> {
    let mut state: BTreeMap<&str, u8> = BTreeMap::new(); // 0 unvisited, 1 visiting, 2 done
    let mut order: Vec<String> = Vec::new();

    fn visit<'a>(
        name: &'a str,
        units: &'a BTreeMap<String, Unit>,
        state: &mut BTreeMap<&'a str, u8>,
        order: &mut Vec<String>,
    ) -> Result<(), GraphError> {
        match state.get(name).copied().unwrap_or(0) {
            2 => return Ok(()),
            1 => {
                return Err(GraphError::new(
                    DiscoverError::Cycle,
                    format!("dependency cycle involving {name}"),
                ))
            }
            _ => {}
        }
        state.insert(name, 1);
        let unit = units.get(name).ok_or_else(|| {
            GraphError::new(
                DiscoverError::IncoherentGraph,
                format!("unit {name} disappeared from the graph"),
            )
        })?;
        for dependency in &unit.dependencies {
            if dependency.path.is_some()
                && !dependency.external
                && units.contains_key(&dependency.package)
            {
                visit(&dependency.package, units, state, order)?;
            }
        }
        state.insert(name, 2);
        order.push(name.to_owned());
        Ok(())
    }

    // Deterministic entry order; dependencies are emitted before their dependents.
    let names: Vec<String> = units.keys().cloned().collect();
    for name in &names {
        visit(name, units, &mut state, &mut order)?;
    }
    Ok(order)
}
