//! `ravum unit-check` — real cross-crate unit graph + chained rustc campaign.
//!
//! This is the CLI face of MVP-0c L1 (commit `cecfa77`): it discovers the
//! workspace unit graph through the public `cargo metadata` interface, runs the
//! conservative chained `--extern` compilation (`ravum-unit-graph::run_chain`,
//! including Cargo fallbacks for units whose independence cannot be established),
//! and reports unit statuses/diagnostics.
//!
//! Guarantees:
//! * **no user file is modified**: the analysis runs on a content-addressed copy
//!   of the workspace inside `--state` (the cargo invocations only see the copy);
//!   the original workspace is fingerprinted before and after, and a difference
//!   makes the command report `STALE` (exit 3) instead of trusting the result;
//! * fail-closed exit codes: `0` no compilation error in the campaign, `1` real
//!   compilation errors/diagnostics, `2` usage or graph invalid, `3` uncertain
//!   (cargo/rustc unavailable, sources changed during the analysis, stale).
//!
//! Exit codes are documented in `docs/mvp/MVP-0c-cli.md`; the JSON report is
//! versioned (`schema_version`).

use crate::commands::work_dir;
use ravum_unit_graph::{
    run_chain, sha256_hex, ChainOptions, ContextSpec, DiscoverError, GraphError, RunReport,
    UnitGraph,
};
use serde_json::json;
use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// No compilation error in the campaign (conservative Cargo fallbacks allowed).
const EXIT_OK: u8 = 0;
/// The campaign produced real compilation errors/diagnostics.
const EXIT_DIAGNOSTICS: u8 = 1;
/// Usage error, or the workspace/graph is invalid.
const EXIT_USAGE_OR_GRAPH: u8 = 2;
/// The analysis is uncertain: cargo/rustc unavailable, stale sources, campaign
/// could not run.
const EXIT_UNCERTAIN: u8 = 3;

const REPORT_SCHEMA_VERSION: u32 = 1;

pub(crate) const SKIPPED_DIRS: &[&str] = &[".git", "target", "node_modules", ".cache"];

/// Entry point for `ravum unit-check`.
pub fn unit_check(args: &[String]) -> u8 {
    if args
        .iter()
        .any(|argument| argument == "--help" || argument == "-h")
    {
        print_help();
        return EXIT_OK;
    }
    match run(args) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("unit-check: {}", error.message);
            error.code
        }
    }
}

#[derive(Debug)]
struct CommandError {
    code: u8,
    message: String,
}

impl CommandError {
    fn new(code: u8, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn usage(message: impl Into<String>) -> Self {
        Self::new(EXIT_USAGE_OR_GRAPH, message)
    }

    fn uncertain(message: impl Into<String>) -> Self {
        Self::new(EXIT_UNCERTAIN, message)
    }
}

type CommandResult<T> = Result<T, CommandError>;

#[derive(Debug, Default)]
struct Options {
    workspace: Option<String>,
    changed: Vec<String>,
    state: Option<PathBuf>,
    json: bool,
    out: Option<PathBuf>,
}

fn parse_args(args: &[String]) -> CommandResult<Options> {
    let mut options = Options::default();
    let mut iterator = args.iter();
    while let Some(argument) = iterator.next() {
        match argument.as_str() {
            "--json" => options.json = true,
            "--state" => {
                let value = iterator
                    .next()
                    .ok_or_else(|| CommandError::usage("--state requires a directory value"))?;
                options.state = Some(PathBuf::from(value));
            }
            "--out" => {
                let value = iterator
                    .next()
                    .ok_or_else(|| CommandError::usage("--out requires a file value"))?;
                options.out = Some(PathBuf::from(value));
            }
            "--changed" => {
                let value = iterator.next().ok_or_else(|| {
                    CommandError::usage("--changed requires a workspace-relative path value")
                })?;
                if value.trim().is_empty() {
                    return Err(CommandError::usage("--changed path is empty"));
                }
                options.changed.push(value.clone());
            }
            other if other.starts_with("--") => {
                return Err(CommandError::usage(format!("unknown option {other}")));
            }
            other => {
                if options.workspace.is_some() {
                    return Err(CommandError::usage(format!(
                        "unexpected extra argument {other:?} (one workspace only)"
                    )));
                }
                options.workspace = Some(other.to_owned());
            }
        }
    }
    Ok(options)
}

fn print_help() {
    println!(
        "ravum unit-check <workspace> [options]\n\
         \n\
         Real cross-crate unit campaign: cargo metadata -> unit graph -> chained\n\
         rustc --emit=metadata with conservative Cargo fallbacks.\n\
         The workspace is analysed on a copy inside --state; no user file is\n\
         modified.\n\
         \n\
         OPTIONS:\n\
         \x20 --changed <path>   workspace-relative path whose unit (and its transitive\n\
         \x20                   dependents) is the analysis scope; repeatable.\n\
         \x20                   Without it, every unit is in scope.\n\
         \x20 --state <dir>     scratch directory (default target/ravum/unit-check).\n\
         \x20 --json            print the versioned JSON report instead of the summary.\n\
         \x20 --out <file>      also write the printed report (JSON or text) to <file>.\n\
         \x20 --help            this help.\n\
         \n\
         EXIT CODES:\n\
         \x20 0  campaign valid, no compilation error (Cargo fallbacks allowed)\n\
         \x20 1  real compilation errors/diagnostics in at least one unit\n\
         \x20 2  usage error, invalid workspace or invalid unit graph\n\
         \x20 3  uncertain analysis: cargo/rustc unavailable, campaign failed to run,\n\
         \x20    or the workspace changed during the analysis (stale)\n"
    );
}

fn run(args: &[String]) -> CommandResult<u8> {
    let started = Instant::now();
    let options = parse_args(args)?;
    let workspace_argument = options.workspace.clone().ok_or_else(|| {
        CommandError::usage(
            "usage: ravum unit-check <workspace> [--changed <path>] [--state <dir>] [--json] [--out <file>]",
        )
    })?;
    let workspace = PathBuf::from(&workspace_argument);
    if !workspace.is_dir() {
        return Err(CommandError::usage(format!(
            "workspace is not a directory: {}",
            workspace.display()
        )));
    }
    let workspace = std::fs::canonicalize(&workspace)
        .map_err(|error| CommandError::usage(format!("cannot canonicalize workspace: {error}")))?;
    if !workspace.join("Cargo.toml").is_file() {
        return Err(CommandError::usage(format!(
            "workspace has no Cargo.toml: {}",
            workspace.display()
        )));
    }

    let state_dir = match options.state.clone() {
        Some(path) if path.is_absolute() => path,
        Some(path) => std::env::current_dir()
            .map_err(|error| CommandError::usage(format!("cannot resolve --state: {error}")))?
            .join(path),
        None => work_dir().join("unit-check"),
    };
    std::fs::create_dir_all(&state_dir).map_err(|error| {
        CommandError::usage(format!(
            "cannot create state directory {}: {error}",
            state_dir.display()
        ))
    })?;
    let state_dir = std::fs::canonicalize(&state_dir).unwrap_or(state_dir);

    // 1. Content-addressed copy: the cargo invocations never touch the user
    //    workspace. The fingerprint is recomputed after the campaign (STALE).
    let fingerprint = workspace_fingerprint(&workspace, &state_dir).map_err(|error| {
        CommandError::usage(format!(
            "cannot fingerprint workspace {}: {error}",
            workspace.display()
        ))
    })?;
    let copy_root = state_dir.join("workspaces").join(&fingerprint[..12]);
    if !copy_root.is_dir() {
        copy_workspace(&workspace, &copy_root, &state_dir).map_err(|error| {
            CommandError::usage(format!(
                "cannot copy workspace into {}: {error}",
                copy_root.display()
            ))
        })?;
    }

    // 2. Real unit graph discovery (fail-closed; no second Cargo.toml parser).
    let graph = UnitGraph::discover(&copy_root).map_err(|error| graph_error("discovery", error))?;

    // 3. Analysis scope: --changed units + transitive dependents, or every unit.
    let (changed_units, affected_units) = analysis_scope(&graph, &options.changed)?;

    // 4. Real chained campaign (targeted rustc + conservative Cargo fallback).
    let context = ContextSpec::from_rustc(Vec::new()).map_err(|error| {
        CommandError::uncertain(format!("toolchain context unavailable: {error}"))
    })?;
    let report = run_chain(
        &graph,
        &ChainOptions::new(state_dir.join("campaign"), context),
    )
    .map_err(|error| graph_error("campaign", error))?;

    // 5. STALE detection: the user workspace must not have changed while the
    //    analysis ran on its copy.
    let fingerprint_after = workspace_fingerprint(&workspace, &state_dir).map_err(|error| {
        CommandError::usage(format!(
            "cannot re-fingerprint workspace {}: {error}",
            workspace.display()
        ))
    })?;
    let workspace_unchanged = fingerprint_after == fingerprint;

    // 6. Report + exit code.
    let exit_code = if !workspace_unchanged || report.stale > 0 {
        EXIT_UNCERTAIN
    } else if report.errors > 0 {
        EXIT_DIAGNOSTICS
    } else {
        EXIT_OK
    };
    let payload = build_report(
        &workspace,
        &copy_root,
        &state_dir,
        &graph,
        &report,
        &options,
        &changed_units,
        &affected_units,
        workspace_unchanged,
        exit_code,
        started.elapsed().as_millis() as u64,
    );
    let rendered = if options.json {
        serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_owned())
    } else {
        render_human(&payload, exit_code)
    };
    if let Some(path) = &options.out {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|error| {
                    CommandError::usage(format!(
                        "cannot create output directory {}: {error}",
                        parent.display()
                    ))
                })?;
            }
        }
        std::fs::write(path, format!("{rendered}\n")).map_err(|error| {
            CommandError::usage(format!(
                "cannot write report to {}: {error}",
                path.display()
            ))
        })?;
    }
    println!("{rendered}");
    Ok(exit_code)
}

fn graph_error(stage: &str, error: GraphError) -> CommandError {
    match error.kind {
        DiscoverError::CargoMissing | DiscoverError::CargoFailed => {
            CommandError::uncertain(format!(
                "{stage} unavailable (cargo missing or failed): {}",
                error.message
            ))
        }
        DiscoverError::ManifestInvalid
        | DiscoverError::IncoherentGraph
        | DiscoverError::Cycle
        | DiscoverError::AmbiguousDependency => CommandError::usage(format!(
            "{stage}: invalid workspace/graph: {}",
            error.message
        )),
    }
}

/// Maps each `--changed` path to its unit and returns `(changed, affected)` in
/// topological order. Without `--changed`, every unit is in scope.
fn analysis_scope(
    graph: &UnitGraph,
    changed_paths: &[String],
) -> CommandResult<(Vec<String>, Vec<String>)> {
    if changed_paths.is_empty() {
        return Ok((graph.order().to_vec(), graph.order().to_vec()));
    }
    let mut changed: BTreeSet<String> = BTreeSet::new();
    for path in changed_paths {
        let unit = unit_for_changed_path(graph, path).ok_or_else(|| {
            CommandError::usage(format!(
                "--changed path {path:?} does not belong to any unit of the workspace"
            ))
        })?;
        changed.insert(unit.clone());
    }
    let names: Vec<&str> = changed.iter().map(String::as_str).collect();
    let mut affected: BTreeSet<String> = graph.dependents_closure(&names);
    affected.extend(changed.iter().cloned());
    let ordered_changed: Vec<String> = graph
        .order()
        .iter()
        .filter(|name| changed.contains(*name))
        .cloned()
        .collect();
    let ordered_affected: Vec<String> = graph
        .order()
        .iter()
        .filter(|name| affected.contains(*name))
        .cloned()
        .collect();
    Ok((ordered_changed, ordered_affected))
}

fn unit_for_changed_path<'graph>(graph: &'graph UnitGraph, path: &str) -> Option<&'graph String> {
    let normalized = path.replace('\\', "/");
    let normalized = normalized.trim_start_matches("./").trim_end_matches('/');
    graph
        .units()
        .iter()
        .find(|(name, unit)| {
            let relative = unit
                .root_dir
                .strip_prefix(graph.root())
                .map(|relative| relative.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            let relative = relative.trim_end_matches('/');
            let direct = (!relative.is_empty()
                && (normalized == relative || normalized.starts_with(&format!("{relative}/"))))
                || normalized == name.as_str();
            let by_file = unit.source_files.iter().any(|source| {
                source
                    .strip_prefix(graph.root())
                    .map(|relative| relative.to_string_lossy().replace('\\', "/"))
                    .map(|relative| relative == normalized)
                    .unwrap_or(false)
            });
            direct || by_file
        })
        .map(|(name, _)| name)
}

#[allow(clippy::too_many_arguments)]
fn build_report(
    workspace: &Path,
    copy_root: &Path,
    state_dir: &Path,
    graph: &UnitGraph,
    report: &RunReport,
    options: &Options,
    changed_units: &[String],
    affected_units: &[String],
    workspace_unchanged: bool,
    exit_code: u8,
    duration_ms: u64,
) -> serde_json::Value {
    let affected: BTreeSet<&String> = affected_units.iter().collect();
    let units: Vec<serde_json::Value> = graph
        .order()
        .iter()
        .filter_map(|name| {
            let unit = graph.unit(name)?;
            let outcome = report.unit(name);
            let status = outcome
                .map(|outcome| outcome.status.as_str().to_owned())
                .unwrap_or_else(|| "missing".to_owned());
            Some(json!({
                "unit": name,
                "package": unit.name,
                "crate_name": unit.crate_name,
                "kind": unit.kind.as_str(),
                "dir": unit
                    .root_dir
                    .strip_prefix(graph.root())
                    .map(|relative| relative.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default(),
                "status": status,
                "fallback_reason": outcome.and_then(|outcome| outcome.fallback_reason.clone()),
                "targeted_ok": outcome.map(|outcome| outcome.targeted_ok()).unwrap_or(false),
                "diagnostics": outcome.map(|outcome| outcome.diagnostics.clone()).unwrap_or_default(),
                "content_id": outcome.map(|outcome| outcome.content_id.clone()),
                "interface_id": outcome.and_then(|outcome| outcome.interface_id.clone()),
                "affected": affected.contains(name),
            }))
        })
        .collect();
    json!({
        "schema_version": REPORT_SCHEMA_VERSION,
        "command": "ravum unit-check",
        "generated_at_unix": ravum_protocol::now_unix(),
        "duration_ms": duration_ms,
        "workspace": display_path(workspace),
        "analysis_copy": display_path(copy_root),
        "state_dir": display_path(state_dir),
        "workspace_unchanged": workspace_unchanged,
        "exit_code": exit_code,
        "changed_units": changed_units,
        "affected_units": affected_units,
        "scope_rule": "changed units + transitive path dependents (or every unit without --changed)",
        "valid": report.valid,
        "errors": report.errors,
        "stale": report.stale,
        "fallback_units": report.fallback,
        "rustc_invocations": report.rustc_invocations(),
        "cargo_fallback_invocations": report.cargo_traces.len(),
        "cache_hits": report.cache_hits,
        "cache_misses": report.cache_misses,
        "toolchain_id": report.toolchain_id,
        "host_target": report.host_target,
        "target": report.target,
        "units": units,
        "diagnostics": {
            "compilation_errors": report.errors,
            "stale": report.stale,
            "fallback_units": report.fallback,
            "note": "a fallback unit is verified by cargo check; it is not a targeted --extern compilation",
        },
        "scope": {
            "changed": options.changed,
            "affected_units": affected_units,
        },
    })
}

fn render_human(payload: &serde_json::Value, exit_code: u8) -> String {
    let mut text = String::new();
    let workspace = payload["workspace"].as_str().unwrap_or("?");
    let units = payload["units"].as_array().cloned().unwrap_or_default();
    let affected = payload["affected_units"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .len();
    text.push_str(&format!(
        "unit-check workspace={workspace} units={} affected={affected} errors={} stale={} fallback={}\n",
        units.len(),
        payload["errors"].as_u64().unwrap_or(0),
        payload["stale"].as_u64().unwrap_or(0),
        payload["fallback_units"].as_u64().unwrap_or(0),
    ));
    text.push_str(&format!(
        "rustc_invocations={} cargo_fallbacks={} cache_hits={} duration_ms={} workspace_unchanged={}\n",
        payload["rustc_invocations"].as_u64().unwrap_or(0),
        payload["cargo_fallback_invocations"].as_u64().unwrap_or(0),
        payload["cache_hits"].as_u64().unwrap_or(0),
        payload["duration_ms"].as_u64().unwrap_or(0),
        payload["workspace_unchanged"].as_bool().unwrap_or(false),
    ));
    for unit in &units {
        let name = unit["unit"].as_str().unwrap_or("?");
        let status = unit["status"].as_str().unwrap_or("?");
        let fallback = unit["fallback_reason"]
            .as_str()
            .map(|reason| format!(" [fallback: {reason}]"))
            .unwrap_or_default();
        let affected = if unit["affected"].as_bool().unwrap_or(false) {
            " affected"
        } else {
            ""
        };
        text.push_str(&format!("  {name:<24} {status:<18}{fallback}{affected}\n"));
        if unit["status"].as_str() == Some("error") {
            for diagnostic in unit["diagnostics"].as_array().into_iter().flatten().take(3) {
                if let Some(line) = diagnostic.as_str() {
                    text.push_str(&format!("    {line}\n"));
                }
            }
        }
    }
    text.push_str(&format!("exit={exit_code}"));
    text
}

// ---------------------------------------------------------------------------
// Workspace copy + fingerprint (read-only with respect to the user workspace)
// ---------------------------------------------------------------------------

pub(crate) fn walk_files(root: &Path, excluded: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&directory)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();
        entries.sort();
        for path in entries {
            if path == excluded || path.starts_with(excluded) {
                continue;
            }
            let metadata = match std::fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            if metadata.file_type().is_symlink() {
                // Never follow symlinks: a copy must not escape the workspace.
                continue;
            }
            if metadata.is_dir() {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("");
                if SKIPPED_DIRS.contains(&name) {
                    continue;
                }
                stack.push(path);
            } else if metadata.is_file() {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

/// Windows canonical paths carry a `\\?\` verbatim prefix; reports keep the
/// user-facing form.
pub(crate) fn display_path(path: &Path) -> String {
    let text = path.display().to_string();
    match text.strip_prefix(r"\\?\") {
        Some(stripped) => stripped.to_owned(),
        None => text,
    }
}

pub(crate) fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"))
}

/// Content fingerprint of the user workspace: sorted `(relative path, sha256)`.
pub(crate) fn workspace_fingerprint(root: &Path, state_dir: &Path) -> io::Result<String> {
    let mut canonical = String::from("ravum-unit-check-workspace-v1\0");
    for path in walk_files(root, state_dir)? {
        let bytes = std::fs::read(&path)?;
        canonical.push_str(&relative_path(root, &path));
        canonical.push('\0');
        canonical.push_str(&sha256_hex(&bytes));
        canonical.push('\0');
    }
    Ok(sha256_hex(canonical.as_bytes()))
}

pub(crate) fn copy_workspace(root: &Path, destination: &Path, state_dir: &Path) -> io::Result<()> {
    if destination.is_dir() {
        return Ok(());
    }
    let temporary = destination.with_extension("tmp");
    if temporary.exists() {
        std::fs::remove_dir_all(&temporary)?;
    }
    for path in walk_files(root, state_dir)? {
        let relative = path
            .strip_prefix(root)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path outside workspace"))?;
        let target = temporary.join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&path, &target)?;
    }
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(&temporary, destination)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ravum_unit_graph::UnitStatus;

    #[test]
    fn parsing_requires_a_single_workspace_and_repeated_changed_paths() {
        let parsed = parse_args(&[
            "ws".to_owned(),
            "--changed".to_owned(),
            "core/src/lib.rs".to_owned(),
            "--changed".to_owned(),
            "api".to_owned(),
            "--json".to_owned(),
        ])
        .expect("parses");
        assert_eq!(parsed.workspace.as_deref(), Some("ws"));
        assert_eq!(parsed.changed, vec!["core/src/lib.rs", "api"]);
        assert!(parsed.json);
        assert!(parse_args(&["a".to_owned(), "b".to_owned()]).is_err());
        assert!(parse_args(&["--wat".to_owned()]).is_err());
        assert!(parse_args(&["ws".to_owned(), "--changed".to_owned()]).is_err());
    }

    #[test]
    fn statuses_are_reported_with_their_conservative_name() {
        assert_eq!(UnitStatus::TargetedOk.as_str(), "targeted_ok");
        assert_eq!(UnitStatus::FallbackPackage.as_str(), "fallback_package");
        assert_eq!(UnitStatus::Error.as_str(), "error");
        assert_eq!(UnitStatus::Stale.as_str(), "stale");
    }
}
