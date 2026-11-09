//! Chained compilation with fresh `--extern`, conservative Cargo fallback and
//! explicit statuses. There is no `PROVED` status and no "assume independent".

use crate::archive::{ArtifactStore, ExternArtifact, VerdictArchive, VerdictEntry};
use crate::graph::{FallbackReason, GraphError, Unit, UnitGraph, UnitKind};
use crate::identity::{
    content_id, context_id, externs_digest, sha256_hex, verdict_key, ContextSpec,
};
use crate::UnitStatus;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// One real `rustc` invocation, with the exact bytes it produced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RustcTrace {
    pub unit: String,
    pub args: Vec<String>,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub rmeta_sha256: Option<String>,
}

/// One `cargo check` fallback invocation (the oracle).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CargoTrace {
    pub unit: String,
    pub args: Vec<String>,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
}

/// Outcome of one unit for one campaign.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitOutcome {
    pub unit: String,
    pub kind: String,
    pub status: UnitStatus,
    /// `true` only when the verdict was reused *and* its artifact bytes matched.
    pub hit: bool,
    pub content_id: String,
    pub context_id: String,
    pub externs: Vec<ExternArtifact>,
    pub externs_digest: String,
    pub interface_id: Option<String>,
    pub fallback_reason: Option<String>,
    pub diagnostics: Vec<String>,
    pub cache_key: String,
}

impl UnitOutcome {
    pub fn targeted_ok(&self) -> bool {
        self.status == UnitStatus::TargetedOk
    }
}

/// Everything needed to run one analysis campaign.
pub struct ChainOptions {
    pub state_dir: PathBuf,
    /// Validated compilation context (toolchain identity, target, flags).
    pub context: ContextSpec,
    /// Optional hook called with the unit name right after its compilation:
    /// used by the STALE test to modify a source file during verification.
    pub after_compile_hook: Option<std::sync::Arc<dyn Fn(&str) + Send + Sync>>,
}

impl ChainOptions {
    pub fn new(state_dir: PathBuf, context: ContextSpec) -> Self {
        Self {
            state_dir,
            context,
            after_compile_hook: None,
        }
    }

    pub fn with_after_compile_hook(
        mut self,
        hook: std::sync::Arc<dyn Fn(&str) + Send + Sync>,
    ) -> Self {
        self.after_compile_hook = Some(hook);
        self
    }
}

/// Result of one campaign. The report is serializable as-is for evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunReport {
    pub schema_version: u32,
    /// `true` when no unit is `ERROR` or `STALE`. A conservative fallback does
    /// not make the campaign invalid; it is reported, not hidden.
    pub valid: bool,
    pub units: Vec<UnitOutcome>,
    pub rustc_traces: Vec<RustcTrace>,
    pub cargo_traces: Vec<CargoTrace>,
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub errors: usize,
    pub stale: usize,
    pub fallback: usize,
    pub toolchain_id: String,
    pub host_target: String,
    pub target: Option<String>,
    pub state_dir: String,
}

impl RunReport {
    pub fn unit(&self, name: &str) -> Option<&UnitOutcome> {
        self.units.iter().find(|outcome| outcome.unit == name)
    }

    pub fn rustc_invocations(&self) -> usize {
        self.rustc_traces.len()
    }
}

/// Runs one full campaign: topological order, fresh `--extern` chain, Cargo
/// fallback whenever any independence would be invented.
pub fn run_chain(graph: &UnitGraph, options: &ChainOptions) -> Result<RunReport, GraphError> {
    options.context.validate().map_err(|message| {
        GraphError::new(crate::graph::DiscoverError::ManifestInvalid, message)
    })?;
    if options.context.target.as_deref() != Some(options.context.host_target.as_str())
        && options.context.target.is_some()
    {
        // Non-host target: no unit can be compiled with the host toolchain as a
        // targeted lib here; the whole campaign is a conservative fallback.
        return run_full_fallback(graph, options);
    }

    let artifacts = ArtifactStore::new(options.state_dir.join("artifacts")).map_err(|error| {
        GraphError::new(
            crate::graph::DiscoverError::ManifestInvalid,
            format!("cannot create artifact store: {error}"),
        )
    })?;
    let mut archive = VerdictArchive::open(options.state_dir.join("cache").join("verdicts.json"));
    let rustc: PathBuf = std::env::var_os("RAVUM_RUSTC")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("rustc"));
    std::fs::create_dir_all(options.state_dir.join("out")).map_err(|error| {
        GraphError::new(
            crate::graph::DiscoverError::ManifestInvalid,
            format!("cannot create state/out: {error}"),
        )
    })?;
    let deps_dir = options.state_dir.join("deps");
    std::fs::create_dir_all(&deps_dir).map_err(|error| {
        GraphError::new(
            crate::graph::DiscoverError::ManifestInvalid,
            format!("cannot create state/deps: {error}"),
        )
    })?;

    let mut outcomes: BTreeMap<String, UnitOutcome> = BTreeMap::new();
    let mut rustc_traces = Vec::new();
    let mut cargo_traces = Vec::new();

    for unit_name in graph.order() {
        let unit = match graph.unit(unit_name) {
            Some(unit) => unit,
            None => continue,
        };
        let files = load_unit_files(graph, unit)?;
        let content = content_id(&files);
        let ctx = context_id(&options.context, unit);
        let mut fallback = graph.fallback_reason(
            unit,
            &options.context.host_target,
            options.context.target.as_deref(),
        );
        let mut externs: Vec<ExternArtifact> = Vec::new();
        if fallback.is_none() && unit.kind == UnitKind::Lib {
            for dependency in &unit.dependencies {
                if dependency.external {
                    fallback = Some(FallbackReason::ExternalDependency);
                    break;
                }
                if dependency.proc_macro {
                    fallback = Some(FallbackReason::ProcMacroDependency);
                    break;
                }
                let dep_outcome = outcomes.get(&dependency.package);
                match dep_outcome {
                    Some(outcome)
                        if outcome.status == UnitStatus::TargetedOk
                            && outcome.interface_id.is_some() =>
                    {
                        let crate_name = graph
                            .unit(&dependency.package)
                            .map(|dep| dep.crate_name.clone())
                            .unwrap_or_else(|| dependency.package.replace('-', "_"));
                        externs.push(ExternArtifact {
                            name: dependency.extern_name.clone(),
                            crate_name,
                            interface_id: outcome.interface_id.clone().unwrap_or_default(),
                        });
                    }
                    _ => {
                        fallback = Some(FallbackReason::DependencyNotTargeted);
                        break;
                    }
                }
            }
        }
        let externs = {
            let mut externs = externs;
            externs.sort_by(|a, b| a.name.cmp(&b.name));
            externs
        };
        let externs_hash = externs_digest(&externs);

        if let Some(reason) = fallback {
            let outcome = cargo_fallback(
                graph,
                unit,
                reason,
                content,
                ctx,
                externs_hash,
                &options.state_dir,
                &mut cargo_traces,
            );
            outcomes.insert(unit.name.clone(), outcome);
            continue;
        }

        let key = verdict_key(&content, &ctx, &externs_hash, &options.context.toolchain_id);
        // A HIT requires the exact key *and* verified artifact bytes.
        if let Some(entry) = archive.get(&key).cloned() {
            let artifact_ok = match (&entry.interface_id, &entry.rmeta_sha256) {
                (Some(interface_id), Some(rmeta_sha)) if interface_id == rmeta_sha => {
                    artifacts.load_verified(rmeta_sha).is_some()
                }
                _ => false,
            };
            if entry.status == UnitStatus::TargetedOk && artifact_ok {
                if let Some(interface_id) = entry.interface_id.clone() {
                    archive.note_hit(&key, now_unix());
                    outcomes.insert(
                        unit.name.clone(),
                        UnitOutcome {
                            unit: unit.name.clone(),
                            kind: unit.kind.as_str().to_owned(),
                            status: UnitStatus::TargetedOk,
                            hit: true,
                            content_id: content,
                            context_id: ctx,
                            externs,
                            externs_digest: externs_hash,
                            interface_id: Some(interface_id),
                            fallback_reason: None,
                            diagnostics: Vec::new(),
                            cache_key: key,
                        },
                    );
                    continue;
                }
            }
        }

        // Real chained compilation with fresh `--extern` artifacts.
        let out_dir = options.state_dir.join("out").join(&unit.name);
        std::fs::create_dir_all(&out_dir).map_err(|error| {
            GraphError::new(
                crate::graph::DiscoverError::ManifestInvalid,
                format!("cannot create out dir for {}: {error}", unit.name),
            )
        })?;
        let out_file = out_dir.join(format!("lib{}.rmeta", unit.crate_name));
        let mut args: Vec<String> = vec![
            "--crate-name".to_owned(),
            unit.crate_name.clone(),
            "--edition".to_owned(),
            unit.edition.clone(),
            "--crate-type".to_owned(),
            "lib".to_owned(),
            "--emit=metadata".to_owned(),
            "-o".to_owned(),
            out_file.display().to_string(),
        ];
        for extern_artifact in &externs {
            let artifact_path = match artifacts.load_verified(&extern_artifact.interface_id) {
                Some(bytes) => {
                    let deps_path = deps_dir.join(format!("lib{}.rmeta", extern_artifact.name));
                    if std::fs::read(&deps_path).ok().as_deref() != Some(bytes.as_slice()) {
                        std::fs::write(&deps_path, &bytes).map_err(|error| {
                            GraphError::new(
                                crate::graph::DiscoverError::ManifestInvalid,
                                format!(
                                    "cannot materialize extern {}: {error}",
                                    extern_artifact.name
                                ),
                            )
                        })?;
                    }
                    deps_path
                }
                None => {
                    // The dependency artifact vanished or was tampered with: no
                    // targeted compilation is possible, fall back conservatively.
                    let outcome = cargo_fallback(
                        graph,
                        unit,
                        FallbackReason::DependencyNotTargeted,
                        content.clone(),
                        ctx.clone(),
                        externs_hash.clone(),
                        &options.state_dir,
                        &mut cargo_traces,
                    );
                    outcomes.insert(unit.name.clone(), outcome);
                    break;
                }
            };
            args.push("--extern".to_owned());
            args.push(format!(
                "{}={}",
                extern_artifact.name,
                artifact_path.display()
            ));
        }
        if outcomes.contains_key(&unit.name) {
            continue;
        }
        args.push("-L".to_owned());
        args.push(format!("dependency={}", deps_dir.display()));
        for flag in &options.context.flags {
            args.push(flag.clone());
        }
        args.push(unit.src_path.display().to_string());

        let started = Instant::now();
        let output = Command::new(&rustc)
            .args(&args)
            .env_remove("RUSTFLAGS")
            .env_remove("RUSTC_WRAPPER")
            .output()
            .map_err(|error| {
                GraphError::new(
                    crate::graph::DiscoverError::CargoMissing,
                    format!("cannot spawn rustc: {error}"),
                )
            })?;
        let duration_ms = started.elapsed().as_millis() as u64;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if let Some(hook) = &options.after_compile_hook {
            hook(&unit.name);
        }
        let success = output.status.success() && out_file.is_file();
        let rmeta_sha = if success {
            std::fs::read(&out_file).ok().map(|bytes| {
                let sha = sha256_hex(&bytes);
                let _ = artifacts.store(&bytes);
                sha
            })
        } else {
            None
        };
        let trace = RustcTrace {
            unit: unit.name.clone(),
            args: args.clone(),
            stdout: stdout.clone(),
            stderr: stderr.clone(),
            exit_code: output.status.code(),
            duration_ms,
            rmeta_sha256: rmeta_sha.clone(),
        };
        rustc_traces.push(trace);

        // STALE: the sources must not have changed between hashing and the end of
        // the compilation. Recompute from disk; a mismatch discards the verdict.
        let reloaded = load_unit_files(graph, unit)?;
        let reloaded_content = content_id(&reloaded);
        let mut status = if success {
            UnitStatus::TargetedOk
        } else {
            UnitStatus::Error
        };
        let mut diagnostics = collect_diagnostics(&stderr);
        if reloaded_content != content {
            status = UnitStatus::Stale;
            diagnostics.push(format!(
                "STALE: {} changed during verification (content_id differs); verdict discarded",
                unit.name
            ));
        }
        let interface_id = if status == UnitStatus::TargetedOk {
            rmeta_sha.clone()
        } else {
            None
        };
        if status == UnitStatus::TargetedOk {
            if let Some(rmeta_sha) = &rmeta_sha {
                archive.insert(VerdictEntry {
                    key: key.clone(),
                    unit: unit.name.clone(),
                    status,
                    content_id: content.clone(),
                    context_id: ctx.clone(),
                    externs_digest: externs_hash.clone(),
                    toolchain_id: options.context.toolchain_id.clone(),
                    interface_id: Some(rmeta_sha.clone()),
                    rmeta_sha256: Some(rmeta_sha.clone()),
                    diagnostics_digest: sha256_hex(diagnostics.join("\n").as_bytes()),
                    at_unix: now_unix(),
                    hits: 0,
                });
            }
        }
        outcomes.insert(
            unit.name.clone(),
            UnitOutcome {
                unit: unit.name.clone(),
                kind: unit.kind.as_str().to_owned(),
                status,
                hit: false,
                content_id: content,
                context_id: ctx,
                externs,
                externs_digest: externs_hash,
                interface_id,
                fallback_reason: None,
                diagnostics,
                cache_key: key,
            },
        );
    }

    if let Err(error) = archive.save() {
        // A cache write failure must not fabricate a valid campaign: the results
        // above stay valid for this run, but no verdict is reused next time.
        eprintln!("ravum-unit-graph: cannot save verdict archive: {error}");
    }
    let units: Vec<UnitOutcome> = graph
        .order()
        .iter()
        .filter_map(|name| outcomes.get(name).cloned())
        .collect();
    let errors = units
        .iter()
        .filter(|outcome| outcome.status == UnitStatus::Error)
        .count();
    let stale = units
        .iter()
        .filter(|outcome| outcome.status == UnitStatus::Stale)
        .count();
    let fallback = units
        .iter()
        .filter(|outcome| {
            matches!(
                outcome.status,
                UnitStatus::FallbackPackage | UnitStatus::FallbackFull
            )
        })
        .count();
    let cache_hits = units.iter().filter(|outcome| outcome.hit).count();
    Ok(RunReport {
        schema_version: crate::REPORT_SCHEMA_VERSION,
        valid: errors == 0 && stale == 0,
        units,
        rustc_traces,
        cargo_traces,
        cache_hits,
        cache_misses: outcomes.len().saturating_sub(cache_hits),
        errors,
        stale,
        fallback,
        toolchain_id: options.context.toolchain_id.clone(),
        host_target: options.context.host_target.clone(),
        target: options.context.target.clone(),
        state_dir: options.state_dir.display().to_string(),
    })
}

/// Whole-campaign fallback (e.g. non-host target): every unit goes through Cargo.
fn run_full_fallback(graph: &UnitGraph, options: &ChainOptions) -> Result<RunReport, GraphError> {
    let mut outcomes = Vec::new();
    let mut cargo_traces = Vec::new();
    for unit_name in graph.order() {
        let unit = graph.unit(unit_name).expect("order names are units");
        let files = load_unit_files(graph, unit)?;
        let content = content_id(&files);
        let ctx = context_id(&options.context, unit);
        let reason = if unit.kind != UnitKind::Lib {
            FallbackReason::BinaryTarget
        } else {
            FallbackReason::TargetMismatch
        };
        let mut outcome = cargo_fallback(
            graph,
            unit,
            reason,
            content,
            ctx,
            externs_digest(&[]),
            &options.state_dir,
            &mut cargo_traces,
        );
        if outcome.status != UnitStatus::Error {
            outcome.status = UnitStatus::FallbackFull;
        }
        outcomes.push(outcome);
    }
    let errors = outcomes
        .iter()
        .filter(|outcome| outcome.status == UnitStatus::Error)
        .count();
    Ok(RunReport {
        schema_version: crate::REPORT_SCHEMA_VERSION,
        valid: errors == 0,
        units: outcomes,
        rustc_traces: Vec::new(),
        cargo_traces,
        cache_hits: 0,
        cache_misses: 0,
        errors,
        stale: 0,
        fallback: 0,
        toolchain_id: options.context.toolchain_id.clone(),
        host_target: options.context.host_target.clone(),
        target: options.context.target.clone(),
        state_dir: options.state_dir.display().to_string(),
    })
}

#[allow(clippy::too_many_arguments)]
fn cargo_fallback(
    graph: &UnitGraph,
    unit: &Unit,
    reason: FallbackReason,
    content: String,
    ctx: String,
    externs_hash: String,
    state_dir: &Path,
    traces: &mut Vec<CargoTrace>,
) -> UnitOutcome {
    let target_dir = state_dir.join("cargo-target");
    let args: Vec<String> = vec![
        "check".to_owned(),
        "-p".to_owned(),
        unit.name.clone(),
        "--manifest-path".to_owned(),
        graph.root().join("Cargo.toml").display().to_string(),
        "--offline".to_owned(),
        "--target-dir".to_owned(),
        target_dir.display().to_string(),
    ];
    if !unit.declared_features.is_empty() {
        // No feature selection is modelled: plain `cargo check -p`, the oracle.
    }
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let started = Instant::now();
    let output = match Command::new(cargo)
        .args(&args)
        .env_remove("RUSTFLAGS")
        .env_remove("RUSTC_WRAPPER")
        .output()
    {
        Ok(output) => output,
        Err(error) => {
            traces.push(CargoTrace {
                unit: unit.name.clone(),
                args,
                stdout: String::new(),
                stderr: format!("cannot spawn cargo: {error}"),
                exit_code: None,
                duration_ms: started.elapsed().as_millis() as u64,
            });
            return UnitOutcome {
                unit: unit.name.clone(),
                kind: unit.kind.as_str().to_owned(),
                status: UnitStatus::Error,
                hit: false,
                content_id: content,
                context_id: ctx,
                externs: Vec::new(),
                externs_digest: externs_hash,
                interface_id: None,
                fallback_reason: Some(reason.as_str().to_owned()),
                diagnostics: vec![format!("cannot spawn cargo: {error}")],
                cache_key: String::new(),
            };
        }
    };
    let duration_ms = started.elapsed().as_millis() as u64;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let success = output.status.success();
    traces.push(CargoTrace {
        unit: unit.name.clone(),
        args,
        stdout,
        stderr: stderr.clone(),
        exit_code: output.status.code(),
        duration_ms,
    });
    let diagnostics = collect_diagnostics(&stderr);
    let status = if success {
        UnitStatus::FallbackPackage
    } else {
        UnitStatus::Error
    };
    UnitOutcome {
        unit: unit.name.clone(),
        kind: unit.kind.as_str().to_owned(),
        status,
        hit: false,
        content_id: content,
        context_id: ctx,
        externs: Vec::new(),
        externs_digest: externs_hash,
        interface_id: None,
        fallback_reason: Some(reason.as_str().to_owned()),
        diagnostics,
        cache_key: String::new(),
    }
}

fn load_unit_files(graph: &UnitGraph, unit: &Unit) -> Result<Vec<(String, Vec<u8>)>, GraphError> {
    let mut files = Vec::new();
    for path in &unit.source_files {
        let bytes = std::fs::read(path).map_err(|error| {
            GraphError::new(
                crate::graph::DiscoverError::ManifestInvalid,
                format!("cannot read {}: {error}", path.display()),
            )
        })?;
        let relative = path
            .strip_prefix(graph.root())
            .map(|relative| relative.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"));
        files.push((relative, bytes));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files)
}

fn collect_diagnostics(stderr: &str) -> Vec<String> {
    stderr
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with("error")
                || trimmed.starts_with("warning")
                || trimmed.contains("error[")
        })
        .map(|line| line.trim_end().to_owned())
        .take(64)
        .collect()
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
