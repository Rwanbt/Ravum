//! Real `SemanticOracle` adapter backed by the L1 unit graph (MVP-0c L3.4).
//!
//! The coordinator (`ravum-workspace-coordinator`) defines the
//! [`SemanticOracle`] injection point and never depends on an analyser. The L1
//! crate (`ravum-unit-graph`) owns the conservative cross-crate unit graph and the
//! chained `--extern` compilation. This crate is the **only** place where the two
//! meet, so neither of them depends on the other (no cycle):
//!
//! ```text
//! ravum-workspace-coordinator  <--  ravum-collab-oracle  -->  ravum-unit-graph
//! ```
//!
//! What one evaluation does:
//!
//! 1. it runs the existing textual oracle ([`TextualSemanticsOracle`]) so the
//!    contract findings (public surface, ABI, removed assertions) are preserved;
//! 2. it materialises the proposal's `snapshot_before` and `snapshot_after` into
//!    content-addressed scratch directories (reused for the same snapshot pair);
//! 3. it discovers the unit graph for both sides with
//!    `cargo metadata --no-deps --offline` (2 Cargo invocations) and runs the
//!    conservative chained compilation twice (`run_chain`, with Cargo fallbacks);
//! 4. it compares, per unit, the **content identity** (sources) and the
//!    **interface identity** (SHA-256 of the `.rmeta` rustc produced) before/after;
//! 5. an unchanged dependent that fails to compile while the unit it depends on
//!    changed is an `INTERFACE_BREAK` blocking finding listing the affected
//!    units — this is the concrete L1 experiment, and it is a real compilation
//!    failure, not a heuristic on signatures;
//! 6. changed units that still compile produce an `UNDECLARED_DEPENDENCY` warning
//!    when the proposal does not declare the impacted dependents;
//! 7. **any uncertainty yields `Unavailable`, never `Clear`**: missing cargo or
//!    rustc, `cargo metadata` failure, cycle/incoherent graph, a changed unit
//!    compiled through a conservative Cargo fallback (interface identity not
//!    observable), a changed unit that does not compile, a structural workspace
//!    change, sources changing during verification, or an unexplained failure of
//!    an unchanged unit.
//!
//! Limits: the adapter observes one local toolchain, one small fixture-style
//! workspace, and a bounded compilation campaign. It never claims `PROVED` and
//! cannot prove general correctness; a successful campaign is a reproducible
//! observation, not a proof. Cargo fallbacks are used by the L1 runner for units
//! whose independence cannot be established (binaries, features, build scripts,
//! external deps): their compile verdict is authoritative, but their interface
//! identity is not observable, hence the fail-closed rule above when a *changed*
//! unit falls back.

use ravum_snapshot::SourceSnapshot;
use ravum_unit_graph::{
    run_chain, ChainOptions, ContextSpec, RunReport, Unit, UnitGraph, UnitStatus,
};
use ravum_workspace_coordinator::{
    OracleFinding, OracleFindingKind, OracleRequest, OracleSeverity, OracleVerdict, SemanticOracle,
    TextualSemanticsOracle,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

/// Schema of the serialisable cost/observation record.
pub const ORACLE_SCHEMA_VERSION: u32 = 1;

/// One measured evaluation of the oracle (also used for the cost evidence).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OracleCost {
    pub schema_version: u32,
    pub elapsed_ms: u64,
    /// Real `rustc` invocations of the two chained campaigns.
    pub rustc_invocations: usize,
    /// `cargo check -p ...` fallback invocations of the two campaigns.
    pub cargo_fallback_invocations: usize,
    /// `cargo metadata --no-deps` invocations (before + after graph).
    pub cargo_metadata_invocations: usize,
    pub before_errors: usize,
    pub after_errors: usize,
    pub fallback_units: usize,
    pub cache_hits: usize,
    pub changed_units: Vec<String>,
    pub changed_interface_units: Vec<String>,
    pub affected_units: Vec<String>,
}

impl OracleCost {
    fn failed(elapsed_ms: u64) -> Self {
        Self {
            schema_version: ORACLE_SCHEMA_VERSION,
            elapsed_ms,
            rustc_invocations: 0,
            cargo_fallback_invocations: 0,
            cargo_metadata_invocations: 0,
            before_errors: 0,
            after_errors: 0,
            fallback_units: 0,
            cache_hits: 0,
            changed_units: Vec::new(),
            changed_interface_units: Vec::new(),
            affected_units: Vec::new(),
        }
    }
}

/// L1-backed semantic oracle.
///
/// `state_root` must be an exclusive scratch directory (tests and the future
/// daemon wiring give it distinct roots). The same snapshot pair reuses the same
/// content-addressed subdirectories, which makes a second evaluation of the same
/// proposal cheap (verdict archive + Cargo target dir reuse) without changing the
/// verdict.
pub struct UnitGraphOracle {
    state_root: PathBuf,
    flags: Vec<String>,
    expected_toolchain_id: Option<String>,
    sequence: AtomicU64,
    last_cost: Mutex<Option<OracleCost>>,
}

impl UnitGraphOracle {
    pub fn new(state_root: impl Into<PathBuf>) -> Self {
        Self {
            state_root: state_root.into(),
            flags: Vec::new(),
            expected_toolchain_id: None,
            sequence: AtomicU64::new(0),
            last_cost: Mutex::new(None),
        }
    }

    /// Extra rustc flags for the compilation context (validated by L1). Default:
    /// none.
    pub fn with_flags(mut self, flags: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.flags = flags.into_iter().map(Into::into).collect();
        self
    }

    /// Refuse to analyse when the local toolchain does not match `toolchain_id`.
    /// Used to exercise the "toolchain different ⇒ Unavailable" rule.
    pub fn with_expected_toolchain(mut self, toolchain_id: impl Into<String>) -> Self {
        self.expected_toolchain_id = Some(toolchain_id.into());
        self
    }

    pub fn state_root(&self) -> &Path {
        &self.state_root
    }

    /// Cost/observation of the last evaluation (also on failure).
    pub fn last_cost(&self) -> Option<OracleCost> {
        self.last_cost.lock().ok().and_then(|guard| guard.clone())
    }

    fn record(&self, cost: OracleCost) {
        if let Ok(mut guard) = self.last_cost.lock() {
            *guard = Some(cost);
        }
    }

    fn preflight(&self) -> Result<ContextSpec, String> {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let version = std::process::Command::new(&cargo)
            .arg("--version")
            .output()
            .map_err(|error| {
                format!(
                    "cargo is unavailable at {:?}: {error} (fail-closed: no interface analysis is claimed)",
                    cargo
                )
            })?;
        if !version.status.success() {
            return Err(format!(
                "cargo --version failed with {}: {}",
                version.status,
                String::from_utf8_lossy(&version.stderr).trim()
            ));
        }
        let context = ContextSpec::from_rustc(self.flags.clone())
            .map_err(|error| format!("toolchain context is unavailable: {error} (fail-closed)"))?;
        if let Some(expected) = &self.expected_toolchain_id {
            if &context.toolchain_id != expected {
                return Err(format!(
                    "toolchain mismatch: expected context {}, local toolchain is {}",
                    expected, context.toolchain_id
                ));
            }
        }
        Ok(context)
    }
}

impl SemanticOracle for UnitGraphOracle {
    fn name(&self) -> &'static str {
        "unit-graph-l1"
    }

    fn evaluate(&self, request: &OracleRequest) -> OracleVerdict {
        let started = Instant::now();
        // 1. Preserve the textual contract findings (public surface, ABI, removed
        //    assertions). An unavailable textual oracle is already fail-closed.
        let (textual_findings, textual_proof) = match TextualSemanticsOracle.evaluate(request) {
            OracleVerdict::Unavailable { reason } => {
                self.record(OracleCost::failed(elapsed_ms(started)));
                return OracleVerdict::Unavailable {
                    reason: format!("textual oracle unavailable: {reason}"),
                };
            }
            OracleVerdict::Findings(findings) => (findings, None),
            OracleVerdict::Clear { bounded_proof } => (Vec::new(), bounded_proof),
        };

        // 2. A lockfile change can alter the dependency graph itself: the L1
        //    campaign would not attribute the resulting failure honestly. Refuse
        //    early with the structured finding (never Clear).
        if let Some(path) = request.changed_paths.iter().find(|path| is_lockfile(path)) {
            let mut findings = textual_findings;
            findings.push(OracleFinding::new(
                OracleFindingKind::LockfileChange,
                OracleSeverity::Blocking,
                format!(
                    "lockfile {path:?} changed: the dependency graph can change without any public interface changing; a compilation campaign cannot attribute the effects, human arbitration required"
                ),
            ));
            self.record(OracleCost::failed(elapsed_ms(started)));
            return OracleVerdict::Findings(findings);
        }

        match self.analyze(request, started) {
            Ok(analysis) => {
                let cost = analysis.cost.clone();
                self.record(cost);
                let mut findings = textual_findings;
                findings.extend(analysis.findings);
                if findings.is_empty() {
                    OracleVerdict::Clear {
                        bounded_proof: textual_proof,
                    }
                } else {
                    OracleVerdict::Findings(findings)
                }
            }
            Err(reason) => {
                self.record(OracleCost::failed(elapsed_ms(started)));
                OracleVerdict::Unavailable { reason }
            }
        }
    }
}

struct Analysis {
    findings: Vec<OracleFinding>,
    cost: OracleCost,
}

impl UnitGraphOracle {
    fn analyze(&self, request: &OracleRequest, started: Instant) -> Result<Analysis, String> {
        let context = self.preflight()?;
        let campaign_root = self.state_root.join(format!(
            "{}-{}",
            request.snapshot_before.id().short(),
            request.snapshot_after.id().short()
        ));
        let _sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        let before_dir = campaign_root.join("before");
        let after_dir = campaign_root.join("after");
        materialize(&request.snapshot_before, &before_dir)?;
        materialize(&request.snapshot_after, &after_dir)?;

        // 3. Both graphs are discovered with `cargo metadata --no-deps --offline`
        //    (2 Cargo invocations, counted in the cost).
        let before_graph = UnitGraph::discover(&before_dir).map_err(|error| {
            format!("cannot discover the before unit graph: {error} (fail-closed)")
        })?;
        let after_graph = UnitGraph::discover(&after_dir).map_err(|error| {
            format!("cannot discover the after unit graph: {error} (fail-closed)")
        })?;

        let before_names: BTreeSet<&String> = before_graph.units().keys().collect();
        let after_names: BTreeSet<&String> = after_graph.units().keys().collect();
        if before_names != after_names {
            let added: Vec<&String> = after_names.difference(&before_names).copied().collect();
            let removed: Vec<&String> = before_names.difference(&after_names).copied().collect();
            return Err(format!(
                "workspace structure changed (added units {added:?}, removed units {removed:?}): cross-version interface comparison is not modelled (fail-closed)"
            ));
        }

        // 4. Two real chained campaigns (targeted rustc + conservative Cargo
        //    fallbacks for units whose independence cannot be established).
        let before_report = run_chain(
            &before_graph,
            &ChainOptions::new(campaign_root.join("state-before"), context.clone()),
        )
        .map_err(|error| format!("before compilation campaign failed to run: {error}"))?;
        let mut after_report = run_chain(
            &after_graph,
            &ChainOptions::new(campaign_root.join("state-after"), context.clone()),
        )
        .map_err(|error| format!("after compilation campaign failed to run: {error}"))?;

        if before_report.toolchain_id != context.toolchain_id
            || after_report.toolchain_id != context.toolchain_id
            || before_report.host_target != after_report.host_target
        {
            return Err(
                "toolchain identity changed between the two campaigns (fail-closed)".to_owned(),
            );
        }
        if before_report.stale > 0 || after_report.stale > 0 {
            return Err(format!(
                "sources changed during verification (before stale={}, after stale={}); verdict discarded (fail-closed)",
                before_report.stale, after_report.stale
            ));
        }
        if before_report.errors > 0 {
            // Say *which* unit failed and why: a fail-closed message that only
            // counts errors cannot be acted on (it cost a full investigation to
            // find the failing unit from a fresh clone).
            let first = before_report
                .units
                .iter()
                .find(|unit| unit.status == UnitStatus::Error)
                .map(|unit| {
                    format!(
                        "unit {:?}: {}",
                        unit.unit,
                        first_diagnostic(&before_report, &unit.unit)
                            .or_else(|| unit.fallback_reason.clone())
                            .unwrap_or_else(|| "<no diagnostic>".to_owned())
                    )
                })
                .unwrap_or_else(|| "<no failing unit recorded>".to_owned());
            return Err(format!(
                "the baseline snapshot does not compile ({} error(s)); interface attribution is impossible (fail-closed). First failing {first}",
                before_report.errors
            ));
        }

        // 5. Changed units: content identity of the unit sources, plus changed
        //    paths that belong to a unit (defence in depth: the content identity is
        //    what the L1 runner actually compiled).
        let mut changed_units: BTreeSet<String> = BTreeSet::new();
        for name in after_graph.order() {
            let before = before_report
                .unit(name)
                .map(|outcome| outcome.content_id.as_str());
            let after = after_report
                .unit(name)
                .map(|outcome| outcome.content_id.as_str());
            if before != after || before.is_none() || after.is_none() {
                changed_units.insert(name.clone());
            }
        }
        for changed_path in &request.changed_paths {
            if let Some(unit) = unit_for_path(&after_graph, changed_path) {
                changed_units.insert(unit.name.clone());
            }
        }
        if changed_units.is_empty()
            && request
                .changed_paths
                .iter()
                .any(|path| path.ends_with("Cargo.toml"))
        {
            return Err(
                "a Cargo.toml changed without changing any unit content identity; the structural effect on the graph is not modelled (fail-closed)"
                    .to_owned(),
            );
        }
        let changed_units: Vec<String> = changed_units.into_iter().collect();

        // 6. A changed unit must have an observable interface identity: a changed
        //    unit compiled through a conservative Cargo fallback cannot be compared.
        let mut changed_interface_units: Vec<String> = Vec::new();
        for name in &changed_units {
            let before = before_report.unit(name);
            let after = after_report.unit(name);
            let (Some(before), Some(after)) = (before, after) else {
                return Err(format!(
                    "changed unit {name:?} is missing from one of the two campaigns (fail-closed)"
                ));
            };
            if after.status == UnitStatus::Error {
                return Err(format!(
                    "changed unit {name:?} does not compile after the patch ({}); interface compatibility cannot be established (fail-closed). First diagnostic: {}",
                    after.fallback_reason.as_deref().unwrap_or("targeted rustc"),
                    first_diagnostic(&after_report, name).unwrap_or_else(|| "<none>".to_owned())
                ));
            }
            match (&before.interface_id, &after.interface_id) {
                (Some(before_id), Some(after_id)) => {
                    if before_id != after_id {
                        changed_interface_units.push(name.clone());
                    }
                }
                _ => {
                    return Err(format!(
                        "changed unit {name:?} was analysed through a conservative Cargo fallback ({}); its interface identity is not observable, so no interface verdict can be claimed (fail-closed)",
                        after
                            .fallback_reason
                            .clone()
                            .or_else(|| before.fallback_reason.clone())
                            .unwrap_or_else(|| "unknown".to_owned())
                    ));
                }
            }
        }

        // 7. Attribute after-campaign failures: an unchanged dependent that fails
        //    while one of its transitive dependencies changed is an interface break.
        //    An *unattributable* failure is retried once with a fresh state directory
        //    (a transient Cargo/rustc hiccup under machine load must not turn into a
        //    durable claim), then reported Unavailable if it persists.
        let mut retried = false;
        let affected_units: BTreeSet<String> = loop {
            match attribute_failures(&after_report, &after_graph, &changed_units) {
                Ok(attributed) => break attributed,
                Err(AttributionIssue::Unattributed(_)) if !retried => {
                    retried = true;
                    after_report = run_chain(
                        &after_graph,
                        &ChainOptions::new(
                            campaign_root.join("state-after-retry"),
                            context.clone(),
                        ),
                    )
                    .map_err(|error| {
                        format!("retried after compilation campaign failed to run: {error}")
                    })?;
                    if after_report.stale > 0 {
                        return Err(format!(
                            "sources changed during the retried verification (stale={}); verdict discarded (fail-closed)",
                            after_report.stale
                        ));
                    }
                    if after_report.toolchain_id != context.toolchain_id {
                        return Err(
                            "toolchain identity changed between campaigns (fail-closed)".to_owned()
                        );
                    }
                }
                Err(AttributionIssue::Unattributed(message)) => {
                    return Err(format!("{message} (observed twice; fail-closed)"));
                }
                Err(AttributionIssue::Fatal(message)) => return Err(message),
            }
        };

        // 8. Undeclared dependents: a changed unit with impacted dependents that the
        //    proposal did not declare. Non-blocking (the code still compiles), but
        //    journaled so revalidation scope is explicit.
        let mut undeclared: BTreeSet<(String, String)> = BTreeSet::new();
        if affected_units.is_empty() {
            for changed_unit in &changed_units {
                for dependent in after_graph.dependents_closure(&[changed_unit.as_str()]) {
                    if changed_units.contains(&dependent) {
                        continue;
                    }
                    let Some(unit) = after_graph.unit(&dependent) else {
                        continue;
                    };
                    let relative = relative_unit_dir(&after_graph, unit);
                    if !declared_matches(&request.declared_dependencies, &relative, &dependent) {
                        undeclared.insert((dependent, changed_unit.clone()));
                    }
                }
            }
        }

        let mut findings = Vec::new();
        if !affected_units.is_empty() {
            let affected: Vec<String> = affected_units.iter().cloned().collect();
            let paths: Vec<String> = changed_units
                .iter()
                .filter_map(|name| {
                    after_graph
                        .unit(name)
                        .map(|unit| relative_unit_dir(&after_graph, unit))
                })
                .collect();
            let symbols: Vec<String> = affected.clone();
            let diagnostic = affected
                .iter()
                .find_map(|name| first_diagnostic(&after_report, name))
                .unwrap_or_else(|| "<no diagnostic>".to_owned());
            findings.push(
                OracleFinding::new(
                    OracleFindingKind::InterfaceBreak,
                    OracleSeverity::Blocking,
                    format!(
                        "interface break: dependent unit(s) {} fail to compile while their own sources are unchanged; changed unit(s): {}. First compiler diagnostic: {diagnostic}",
                        affected.join(", "),
                        changed_units.join(", ")
                    ),
                )
                .with_paths(paths)
                .with_symbols(symbols),
            );
        }
        if affected_units.is_empty() && !undeclared.is_empty() {
            let pairs: Vec<String> = undeclared
                .iter()
                .map(|(dependent, changed)| format!("{changed} -> {dependent}"))
                .collect();
            let paths: Vec<String> = undeclared
                .iter()
                .filter_map(|(dependent, _)| {
                    after_graph
                        .unit(dependent)
                        .map(|unit| relative_unit_dir(&after_graph, unit))
                })
                .collect();
            let symbols: Vec<String> = undeclared
                .iter()
                .map(|(dependent, _)| dependent.clone())
                .collect();
            findings.push(
                OracleFinding::new(
                    OracleFindingKind::UndeclaredDependency,
                    OracleSeverity::Warning,
                    format!(
                        "changed unit(s) have impacted dependents that the proposal did not declare: {}; revalidation scope is explicit (non-blocking because the full campaign compiles)",
                        pairs.join(", ")
                    ),
                )
                .with_paths(paths)
                .with_symbols(symbols),
            );
        }

        let cost = OracleCost {
            schema_version: ORACLE_SCHEMA_VERSION,
            elapsed_ms: elapsed_ms(started),
            rustc_invocations: before_report.rustc_invocations() + after_report.rustc_invocations(),
            cargo_fallback_invocations: before_report.cargo_traces.len()
                + after_report.cargo_traces.len(),
            cargo_metadata_invocations: 2,
            before_errors: before_report.errors,
            after_errors: after_report.errors,
            fallback_units: before_report.fallback + after_report.fallback,
            cache_hits: before_report.cache_hits + after_report.cache_hits,
            changed_units,
            changed_interface_units,
            affected_units: affected_units.into_iter().collect(),
        };
        Ok(Analysis { findings, cost })
    }
}

enum AttributionIssue {
    /// The failure cannot be explained by the patch; worth one retry.
    Unattributed(String),
    /// The failure is real and attributable (or the patch itself is broken).
    Fatal(String),
}

fn attribute_failures(
    report: &RunReport,
    graph: &UnitGraph,
    changed_units: &[String],
) -> Result<BTreeSet<String>, AttributionIssue> {
    let mut affected: BTreeSet<String> = BTreeSet::new();
    for outcome in &report.units {
        if outcome.status != UnitStatus::Error {
            continue;
        }
        if changed_units.contains(&outcome.unit) {
            return Err(AttributionIssue::Fatal(format!(
                "changed unit {:?} does not compile after the patch; interface compatibility cannot be established (fail-closed). First diagnostic: {}",
                outcome.unit,
                first_diagnostic(report, &outcome.unit).unwrap_or_else(|| "<none>".to_owned())
            )));
        }
        let dependencies = dependency_closure(graph, &outcome.unit);
        if dependencies
            .iter()
            .any(|dependency| changed_units.contains(dependency))
        {
            affected.insert(outcome.unit.clone());
        } else {
            return Err(AttributionIssue::Unattributed(format!(
                "unchanged unit {:?} fails to compile without any changed transitive dependency; the failure cannot be attributed to the patch (fail-closed). First diagnostic: {}",
                outcome.unit,
                first_diagnostic(report, &outcome.unit).unwrap_or_else(|| "<none>".to_owned())
            )));
        }
    }
    Ok(affected)
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis() as u64
}

fn materialize(snapshot: &SourceSnapshot, dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir)
        .map_err(|error| format!("cannot create scratch dir {}: {error}", dir.display()))?;
    snapshot.materialize_to(dir).map_err(|error| {
        format!(
            "cannot materialize snapshot into {}: {error}",
            dir.display()
        )
    })
}

/// Lockfiles / generated dependency manifests are handled before any campaign.
fn is_lockfile(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    let name = normalized.rsplit('/').next().unwrap_or(&normalized);
    name == "Cargo.lock" || name == "Cargo.toml.orig" || name.ends_with(".lock")
}

/// Unit whose root directory contains `changed_path` (relative workspace path).
fn unit_for_path<'graph>(graph: &'graph UnitGraph, changed_path: &str) -> Option<&'graph Unit> {
    let normalized = changed_path.replace('\\', "/");
    graph.units().values().find(|unit| {
        let relative = relative_unit_dir(graph, unit);
        if relative.is_empty() {
            return true;
        }
        normalized == relative
            || normalized.starts_with(&format!("{relative}/"))
            || unit.source_files.iter().any(|file| {
                file.strip_prefix(graph.root())
                    .map(|rel| rel.to_string_lossy().replace('\\', "/"))
                    .map(|rel| rel == normalized)
                    .unwrap_or(false)
            })
    })
}

/// Unit root directory relative to the workspace root, with forward slashes and no
/// trailing slash (empty for a unit at the workspace root).
fn relative_unit_dir(graph: &UnitGraph, unit: &Unit) -> String {
    unit.root_dir
        .strip_prefix(graph.root())
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| String::new())
        .trim_end_matches('/')
        .to_owned()
}

/// Declared dependency matching: unit name, unit directory, or a path inside it.
fn declared_matches(declared: &[String], unit_relative: &str, unit_name: &str) -> bool {
    declared.iter().any(|entry| {
        let entry = entry.trim().replace('\\', "/");
        let entry = entry.trim_start_matches("./").trim_end_matches('/');
        if entry.is_empty() {
            return false;
        }
        if unit_relative.is_empty() {
            return true;
        }
        entry == unit_name
            || entry == unit_relative
            || entry.starts_with(&format!("{unit_relative}/"))
    })
}

/// Transitive dependencies of `unit` that are members of the graph.
fn dependency_closure(graph: &UnitGraph, unit_name: &str) -> BTreeSet<String> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut stack: Vec<String> = graph
        .unit(unit_name)
        .map(|unit| {
            unit.dependencies
                .iter()
                .filter(|dependency| !dependency.external)
                .map(|dependency| dependency.package.clone())
                .collect()
        })
        .unwrap_or_default();
    while let Some(name) = stack.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        if let Some(unit) = graph.unit(&name) {
            for dependency in &unit.dependencies {
                if !dependency.external && !seen.contains(&dependency.package) {
                    stack.push(dependency.package.clone());
                }
            }
        }
    }
    seen
}

fn first_diagnostic(report: &RunReport, unit: &str) -> Option<String> {
    report
        .unit(unit)
        .and_then(|outcome| outcome.diagnostics.first())
        .map(|line| {
            let mut line = line.trim().to_owned();
            if line.len() > 240 {
                line.truncate(240);
                line.push('…');
            }
            line
        })
}

/// Diagnostic counters by unit, useful for evidence and debugging.
pub fn unit_statuses(report: &RunReport) -> BTreeMap<String, (String, Option<String>)> {
    report
        .units
        .iter()
        .map(|outcome| {
            (
                outcome.unit.clone(),
                (
                    outcome.status.as_str().to_owned(),
                    outcome.fallback_reason.clone(),
                ),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lockfiles_are_recognised() {
        assert!(is_lockfile("Cargo.lock"));
        assert!(is_lockfile("sub/Cargo.lock"));
        assert!(is_lockfile("poetry.lock"));
        assert!(!is_lockfile("Cargo.toml"));
        assert!(!is_lockfile("src/lib.rs"));
    }

    #[test]
    fn declared_matching_accepts_unit_name_or_directory() {
        let declared = vec!["api".to_owned(), "consumer/src".to_owned()];
        assert!(declared_matches(&declared, "api", "api"));
        assert!(declared_matches(&declared, "consumer", "consumer"));
        assert!(!declared_matches(&declared, "application", "application"));
        assert!(!declared_matches(&[], "api", "api"));
    }
}
