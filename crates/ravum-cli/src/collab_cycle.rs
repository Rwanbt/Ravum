//! `ravum collab-cycle` — composition root L3.4 / COLLAB-4.
//!
//! Problème visé: un changement de code accepté par le coordinateur collaboratif
//! ne doit déclencher que les analyses daemon *nécessaires*, en partageant les
//! identités (contenu exact) et l'invalidation (graphe d'unités), sans maintenir
//! un second moteur de snapshots.
//!
//! Ce module est un **composition root**: il assemble `ravum-workspace-coordinator`
//! (décisions + snapshots avant/après + journal) et `ravum-daemon` (identité de
//! contenu + cache de verdicts) sans modifier ni coupler ces bibliothèques.
//!
//! Pipeline d'un cycle:
//!
//! 1. charger le workspace (fichiers `.rs` + `Cargo.toml`) en [`SourceSnapshot`];
//! 2. découvrir les unités de compilation (crate = `Cargo.toml` le plus proche) et
//!    le graphe de dépendances `path` déclarées dans `[dependencies]`;
//! 3. soumettre les propositions au coordinateur (mode textuel, sans vérification
//!    sémantique) et conserver décisions, `AcceptedChange` et journal;
//! 4. pour chaque changement accepté, calculer les unités affectées = unités
//!    modifiées + dépendants transitifs, et n'analyser **que celles-là**;
//! 5. vérifier la correspondance d'identité par **contenu** (hash SHA-256 des
//!    fichiers du snapshot coordinateur vs hashs que le daemon identifie) et
//!    démontrer l'adressage par contenu par un second passage identique => hits;
//! 6. écrire un rapport JSON (identités, cache, timings, unités analysées/sautées)
//!    et retourner un code de sortie fail-closed.
//!
//! Limites assumées (voir `docs/mvp/MVP-0b-collab-4.md`):
//! * le graphe ne lit que les dépendances `path` de `[dependencies]` (ni
//!   `dev-dependencies`, ni `build-dependencies`, ni versions crates.io);
//! * l'analyse daemon reste mono-unité (`--emit=metadata` d'une lib, sans lier les
//!   dépendances): un dépendant est *programmé* par le graphe, son contenu est
//!   envoyé tel quel, et le cache d'identité de contenu décide s'il faut ou non
//!   relancer `rustc`;
//! * l'historique d'intégration (journal + `AcceptedChange`) n'est **pas** un
//!   historique d'activation native: aucun artefact n'est construit ni chargé ici.

use crate::commands::{flag, flag_u64, has, positional, work_dir};
use ravum_collab_oracle::UnitGraphOracle;
use ravum_daemon::{AnalysisRequest, BuildContext, Daemon, DaemonConfig};
use ravum_protocol::{ChangeProposal, DecisionKind, COLLAB_SCHEMA_VERSION};
use ravum_snapshot::SourceSnapshot;
use ravum_workspace_coordinator::{Coordinator, EvaluationProtocol};
use serde::Serialize;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const REPORT_SCHEMA_VERSION: u32 = 1;
const MAX_PASSES: u64 = 8;

/// Entry point for `ravum collab-cycle`.
pub fn collab_cycle(args: &[String]) -> u8 {
    match run(args) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("collab-cycle: {}", error.message);
            error.code
        }
    }
}

// ---------------------------------------------------------------------------
// Errors with explicit exit codes
// ---------------------------------------------------------------------------

/// Exit codes of the command (documented in `docs/mvp/MVP-0b-collab-4.md`).
const EXIT_OK: u8 = 0;
const EXIT_UNEXPECTED_DECISION: u8 = 1;
const EXIT_USAGE_OR_GRAPH: u8 = 2;
const EXIT_ANALYSIS_FAILED: u8 = 3;
const EXIT_IDENTITY_VIOLATION: u8 = 4;

#[derive(Debug)]
struct CycleError {
    code: u8,
    message: String,
}

impl CycleError {
    fn new(code: u8, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn usage(message: impl Into<String>) -> Self {
        Self::new(EXIT_USAGE_OR_GRAPH, message)
    }

    fn graph(message: impl Into<String>) -> Self {
        Self::new(EXIT_USAGE_OR_GRAPH, message)
    }
}

type CycleResult<T> = Result<T, CycleError>;

/// One invariant violation collected during a cycle. The report keeps them all;
/// the exit code is derived from the highest severity.
#[derive(Debug, Clone, Serialize)]
struct Violation {
    kind: String,
    message: String,
}

impl Violation {
    fn new(kind: &str, message: impl Into<String>) -> Self {
        Self {
            kind: kind.to_owned(),
            message: message.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Unit graph (crate = nearest Cargo.toml, edges = [dependencies] path)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Unit {
    /// Workspace-relative directory of the unit; `""` is the workspace root.
    dir: String,
    package_name: String,
    /// `BuildContext::crate_name`: package name with `-` replaced by `_`.
    crate_name: String,
    edition: String,
    /// Analysis root, workspace-relative (`<dir>/src/lib.rs`).
    root: String,
    /// Every workspace-relative file of the unit: `Cargo.toml` + `.rs` files.
    files: Vec<String>,
    /// Unit directories this unit depends on (declared `path` dependencies).
    dependencies: BTreeSet<String>,
    /// Unit directories that depend on this unit (reverse edges).
    dependents: BTreeSet<String>,
}

/// A workspace laid out as independent crates, with a minimal `path`-dependency
/// graph. Discovery is fail-closed: an unreadable manifest, an unresolvable path
/// dependency, a cycle or a `.rs` file outside every crate is an error, never a
/// silent "analyse something else".
#[derive(Debug, Clone)]
struct UnitGraph {
    units: BTreeMap<String, Unit>,
    /// Topological order (dependencies before dependents).
    order: Vec<String>,
}

#[derive(Debug, Clone)]
struct AffectedUnits {
    /// Units that own at least one changed file.
    direct: BTreeSet<String>,
    /// `direct` plus every transitive dependent.
    affected: BTreeSet<String>,
}

impl UnitGraph {
    /// Discovers the units of `snapshot` and validates the dependency graph.
    fn discover(snapshot: &SourceSnapshot) -> CycleResult<Self> {
        let manifest_paths: Vec<String> = snapshot
            .paths()
            .filter(|path| is_manifest_path(path))
            .map(str::to_owned)
            .collect();
        if manifest_paths.is_empty() {
            return Err(CycleError::graph(
                "aucun Cargo.toml dans le workspace: impossible de définir une unité de compilation",
            ));
        }

        let mut manifests: BTreeSet<String> = BTreeSet::new();
        let mut parsed_manifests: BTreeMap<String, ParsedManifest> = BTreeMap::new();
        let mut units: BTreeMap<String, Unit> = BTreeMap::new();
        for manifest_path in &manifest_paths {
            let dir = parent_dir(manifest_path);
            let content = snapshot.content_of(manifest_path).ok_or_else(|| {
                CycleError::graph(format!("Cargo.toml absent du snapshot: {manifest_path}"))
            })?;
            let parsed = parse_manifest(manifest_path, content)?;
            manifests.insert(dir.clone());
            let crate_name = sanitize_crate_name(&parsed.package_name);
            if crate_name.is_empty() {
                return Err(CycleError::graph(format!(
                    "{manifest_path}: [package] name n'est pas un identifiant Rust utilisable"
                )));
            }
            units.insert(
                dir.clone(),
                Unit {
                    dir: dir.clone(),
                    package_name: parsed.package_name.clone(),
                    crate_name,
                    edition: parsed.edition.clone(),
                    root: String::new(),
                    files: Vec::new(),
                    dependencies: BTreeSet::new(),
                    dependents: BTreeSet::new(),
                },
            );
            parsed_manifests.insert(dir, parsed);
        }

        // Resolve `path` dependencies now that every manifest directory is known.
        for (dir, parsed) in &parsed_manifests {
            let manifest_path = join_dir(dir, "Cargo.toml");
            for (dependency_name, raw_path) in &parsed.dependencies {
                let Some(raw_path) = raw_path else {
                    return Err(CycleError::graph(format!(
                        "{manifest_path}: la dépendance {dependency_name:?} n'est pas une dépendance `path`; \
                         le graphe COLLAB-4 ne sait pas la résoudre (fail-closed)"
                    )));
                };
                let resolved = resolve_relative_dir(dir, raw_path).map_err(|reason| {
                    CycleError::graph(format!(
                        "{manifest_path}: dépendance {dependency_name:?} path={raw_path:?} invalide: {reason}"
                    ))
                })?;
                if !manifests.contains(&resolved) {
                    return Err(CycleError::graph(format!(
                        "{manifest_path}: dépendance {dependency_name:?} path={raw_path:?} introuvable \
                         (aucun Cargo.toml à la cible {:?}); analyse refusée plutôt que devinée",
                        display_dir(&resolved)
                    )));
                }
                if resolved == *dir {
                    return Err(CycleError::graph(format!(
                        "{manifest_path}: la crate dépend d'elle-même (path={raw_path:?})"
                    )));
                }
                units
                    .get_mut(dir)
                    .expect("unit inserted above")
                    .dependencies
                    .insert(resolved.clone());
                units
                    .get_mut(&resolved)
                    .expect("dependency unit exists")
                    .dependents
                    .insert(dir.to_owned());
            }
        }

        // Attach files to their nearest manifest.
        for path in snapshot.paths() {
            if is_manifest_path(path) {
                let dir = parent_dir(path);
                let unit = units.get_mut(&dir).ok_or_else(|| {
                    CycleError::graph(format!(
                        "Cargo.toml non découvert (incohérence interne): {path}"
                    ))
                })?;
                unit.files.push(path.to_owned());
                continue;
            }
            if !path.ends_with(".rs") {
                continue;
            }
            let dir = nearest_manifest_dir(&manifests, path).ok_or_else(|| {
                CycleError::graph(format!(
                    "fichier .rs hors de toute crate (aucun Cargo.toml ancêtre): {path}; \
                     refus fail-closed plutôt qu'une unité inventée"
                ))
            })?;
            units
                .get_mut(&dir)
                .expect("nearest manifest is a discovered unit")
                .files
                .push(path.to_owned());
        }

        // Root file: the daemon analyses one library unit per request.
        for unit in units.values_mut() {
            unit.files.sort();
            let lib_root = join_dir(&unit.dir, "src/lib.rs");
            let flat_lib_root = join_dir(&unit.dir, "lib.rs");
            let root = if unit.files.iter().any(|file| file == &lib_root) {
                lib_root
            } else if unit.files.iter().any(|file| file == &flat_lib_root) {
                flat_lib_root
            } else {
                return Err(CycleError::graph(format!(
                    "unité {:?} (dossier {:?}): racine d'analyse introuvable; le daemon COLLAB-4 \
                     n'analyse que des bibliothèques (`src/lib.rs` ou `lib.rs`), pas des binaires",
                    unit.package_name,
                    display_dir(&unit.dir)
                )));
            };
            unit.root = root;
        }

        // Crate names become `--crate-name`: they must be valid and unique.
        let mut by_crate_name: BTreeMap<String, String> = BTreeMap::new();
        for unit in units.values() {
            if let Some(other) = by_crate_name.insert(unit.crate_name.clone(), unit.dir.clone()) {
                return Err(CycleError::graph(format!(
                    "deux unités ont le même nom de crate {:?} (dossiers {:?} et {:?})",
                    unit.crate_name,
                    display_dir(&other),
                    display_dir(&unit.dir)
                )));
            }
        }

        let order = topological_order(&units)?;
        Ok(Self { units, order })
    }

    fn unit(&self, dir: &str) -> Option<&Unit> {
        self.units.get(dir)
    }

    /// Units owning `changed_paths`, plus every transitive dependent.
    fn affected(&self, changed_paths: &[String]) -> CycleResult<AffectedUnits> {
        let mut direct: BTreeSet<String> = BTreeSet::new();
        for path in changed_paths {
            let dir = self.unit_of_path(path)?;
            direct.insert(dir);
        }
        let mut affected = direct.clone();
        let mut stack: Vec<String> = direct.iter().cloned().collect();
        while let Some(dir) = stack.pop() {
            let unit = self.units.get(&dir).ok_or_else(|| {
                CycleError::graph(format!(
                    "unité inconnue dans le graphe: {:?}",
                    display_dir(&dir)
                ))
            })?;
            for dependent in unit.dependents.clone() {
                if affected.insert(dependent.clone()) {
                    stack.push(dependent);
                }
            }
        }
        Ok(AffectedUnits { direct, affected })
    }

    /// The unit that owns `path` (nearest `Cargo.toml` ancestor).
    fn unit_of_path(&self, path: &str) -> CycleResult<String> {
        let manifests: BTreeSet<String> = self.units.keys().cloned().collect();
        nearest_manifest_dir(&manifests, path).ok_or_else(|| {
            CycleError::graph(format!(
                "chemin modifié hors de toute crate (aucun Cargo.toml ancêtre): {path}"
            ))
        })
    }

    /// Units of `subset` in dependency order (dependencies first).
    fn ordered_subset(&self, subset: &BTreeSet<String>) -> Vec<String> {
        self.order
            .iter()
            .filter(|dir| subset.contains(*dir))
            .cloned()
            .collect()
    }

    fn skipped(&self, affected: &BTreeSet<String>) -> Vec<String> {
        self.order
            .iter()
            .filter(|dir| !affected.contains(*dir))
            .cloned()
            .collect()
    }
}

fn is_manifest_path(path: &str) -> bool {
    path == "Cargo.toml" || path.ends_with("/Cargo.toml")
}

fn parent_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(index) => path[..index].to_owned(),
        None => String::new(),
    }
}

fn display_dir(dir: &str) -> &str {
    if dir.is_empty() {
        "."
    } else {
        dir
    }
}

fn join_dir(dir: &str, relative: &str) -> String {
    if dir.is_empty() {
        relative.to_owned()
    } else {
        format!("{dir}/{relative}")
    }
}

fn nearest_manifest_dir(manifests: &BTreeSet<String>, path: &str) -> Option<String> {
    let mut current = parent_dir(path);
    loop {
        if manifests.contains(&current) {
            return Some(current);
        }
        if current.is_empty() {
            return None;
        }
        current = parent_dir(&current);
    }
}

/// Normalizes `base/raw` into a workspace-relative directory. Rejects absolute
/// paths, `..` escapes and empty results left dangling.
fn resolve_relative_dir(base: &str, raw: &str) -> Result<String, String> {
    let raw = raw.replace('\\', "/");
    if raw.starts_with('/') || raw.contains(':') {
        return Err("chemin absolu interdit (le graphe reste dans le workspace)".to_owned());
    }
    let mut parts: Vec<String> = if base.is_empty() {
        Vec::new()
    } else {
        base.split('/').map(str::to_owned).collect()
    };
    for part in raw.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err("le chemin sort du workspace".to_owned());
                }
            }
            other => parts.push(other.to_owned()),
        }
    }
    Ok(parts.join("/"))
}

/// Minimal TOML subset reader for `Cargo.toml`: `[package]` (`name`, `edition`)
/// and `[dependencies]`/`[dependencies.<name>]` (`path`). Everything else is
/// ignored on purpose: this is a dependency graph, not a TOML parser.
#[derive(Debug, Clone, Default)]
struct ParsedManifest {
    package_name: String,
    edition: String,
    /// Declared dependencies as `(name, Some(path))` or `(name, None)`.
    dependencies: Vec<(String, Option<String>)>,
}

fn parse_manifest(path: &str, content: &str) -> CycleResult<ParsedManifest> {
    let mut parsed = ParsedManifest {
        edition: "2015".to_owned(),
        ..ParsedManifest::default()
    };
    let mut section = String::new();
    let mut inline_dependencies: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut table_dependencies: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();

    for (line_index, raw) in content.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            if !line.ends_with(']') || line.len() < 2 {
                return Err(CycleError::graph(format!(
                    "{path}:{}: en-tête de section TOML illisible: {line:?}",
                    line_index + 1
                )));
            }
            section = line[1..line.len() - 1].trim().to_owned();
            continue;
        }
        let known_section = section == "package"
            || section == "dependencies"
            || section.starts_with("dependencies.");
        if !known_section {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(CycleError::graph(format!(
                "{path}:{}: ligne illisible dans [{section}]: {line:?}",
                line_index + 1
            )));
        };
        let key = key.trim();
        let value = value.trim();
        if section == "package" {
            match key {
                "name" => parsed.package_name = unquote(path, line_index, value)?,
                "edition" => parsed.edition = unquote(path, line_index, value)?,
                _ => {}
            }
        } else if section == "dependencies" {
            if key.is_empty() {
                return Err(CycleError::graph(format!(
                    "{path}:{}: dépendance sans nom",
                    line_index + 1
                )));
            }
            if !inline_dependencies.contains_key(key) {
                order.push(key.to_owned());
            }
            inline_dependencies.insert(key.to_owned(), inline_path(path, line_index, value)?);
        } else {
            // `[dependencies.<name>] path = "..."`
            let dependency_name = section["dependencies.".len()..].trim().to_owned();
            if dependency_name.contains('"') || dependency_name.is_empty() {
                return Err(CycleError::graph(format!(
                    "{path}:{}: nom de dépendance de table non supporté: {dependency_name:?}",
                    line_index + 1
                )));
            }
            if !inline_dependencies.contains_key(&dependency_name)
                && !table_dependencies.contains_key(&dependency_name)
            {
                order.push(dependency_name.clone());
            }
            if key == "path" {
                let resolved = unquote(path, line_index, value)?;
                table_dependencies.insert(dependency_name, Some(resolved));
            } else if key == "version" {
                table_dependencies.entry(dependency_name).or_insert(None);
            }
        }
    }

    if parsed.package_name.is_empty() {
        return Err(CycleError::graph(format!(
            "{path}: aucun [package] name lisible: le Cargo.toml est illisible pour le graphe \
             (fail-closed, aucune analyse au hasard)"
        )));
    }
    for name in order {
        let from_table = table_dependencies.remove(&name);
        let from_inline = inline_dependencies.remove(&name);
        let path = match (from_inline, from_table) {
            (Some(Some(inline)), Some(Some(table))) if inline != table => {
                return Err(CycleError::graph(format!(
                    "{path}: dépendance {name:?} déclarée deux fois avec des chemins différents"
                )));
            }
            (Some(Some(path)), _) | (_, Some(Some(path))) => Some(path),
            _ => None,
        };
        parsed.dependencies.push((name, path));
    }
    Ok(parsed)
}

fn unquote(path: &str, line_index: usize, value: &str) -> CycleResult<String> {
    let trimmed = value.trim();
    let Some(inner) = trimmed
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    else {
        return Err(CycleError::graph(format!(
            "{path}:{}: valeur TOML non supportée (chaîne attendue): {value:?}",
            line_index + 1
        )));
    };
    if inner.contains('\\') || inner.contains('"') {
        return Err(CycleError::graph(format!(
            "{path}:{}: échappements TOML non supportés dans {value:?}",
            line_index + 1
        )));
    }
    Ok(inner.to_owned())
}

/// Extracts `path = "..."` from an inline dependency table, `None` for a plain
/// version string or an inline table without `path`.
fn inline_path(path: &str, line_index: usize, value: &str) -> CycleResult<Option<String>> {
    let trimmed = value.trim();
    if trimmed.starts_with('"') {
        unquote(path, line_index, trimmed)?;
        return Ok(None);
    }
    if !trimmed.starts_with('{') {
        return Err(CycleError::graph(format!(
            "{path}:{}: forme de dépendance non supportée (table inline attendue): {value:?}",
            line_index + 1
        )));
    }
    if !trimmed.ends_with('}') {
        // Multi-line inline tables are legal TOML but out of scope here.
        return Err(CycleError::graph(format!(
            "{path}:{}: table inline multi-ligne non supportée: {value:?}",
            line_index + 1
        )));
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    for entry in inner.split(',') {
        let Some((key, raw_value)) = entry.split_once('=') else {
            return Err(CycleError::graph(format!(
                "{path}:{}: entrée de table inline illisible: {entry:?}",
                line_index + 1
            )));
        };
        if key.trim() == "path" {
            return Ok(Some(unquote(path, line_index, raw_value.trim())?));
        }
    }
    Ok(None)
}

fn sanitize_crate_name(package_name: &str) -> String {
    let mut name = String::with_capacity(package_name.len());
    for character in package_name.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            name.push(character);
        } else if character == '-' || character == '.' {
            name.push('_');
        } else {
            return String::new();
        }
    }
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
        return String::new();
    }
    name
}

/// DFS topological sort with explicit cycle detection.
fn topological_order(units: &BTreeMap<String, Unit>) -> CycleResult<Vec<String>> {
    fn visit(
        dir: &str,
        units: &BTreeMap<String, Unit>,
        state: &mut BTreeMap<String, u8>,
        output: &mut Vec<String>,
    ) -> Result<(), String> {
        match state.get(dir).copied().unwrap_or(0) {
            2 => return Ok(()),
            1 => {
                return Err(format!(
                    "cycle de dépendances path autour de {:?}",
                    display_dir(dir)
                ))
            }
            _ => {}
        }
        state.insert(dir.to_owned(), 1);
        let unit = units
            .get(dir)
            .ok_or_else(|| format!("unité inconnue dans le graphe: {dir:?}"))?;
        for dependency in &unit.dependencies {
            visit(dependency, units, state, output)?;
        }
        state.insert(dir.to_owned(), 2);
        output.push(dir.to_owned());
        Ok(())
    }

    let mut state: BTreeMap<String, u8> = BTreeMap::new();
    let mut output = Vec::with_capacity(units.len());
    for dir in units.keys() {
        visit(dir, units, &mut state, &mut output).map_err(CycleError::graph)?;
    }
    Ok(output)
}

// ---------------------------------------------------------------------------
// Proposals
// ---------------------------------------------------------------------------

/// Deterministic built-in scenario: two agents modify two different functions of
/// `crate_a`, both against the same base snapshot (same expected hashes).
fn builtin_proposals(snapshot: &SourceSnapshot) -> CycleResult<Vec<ChangeProposal>> {
    let path = "crate_a/src/lib.rs";
    let content = snapshot.content_of(path).ok_or_else(|| {
        CycleError::graph(format!(
            "scénario intégré: fichier {path} absent du workspace chargé"
        ))
    })?;
    let base_hash = snapshot
        .file_hash(path)
        .ok_or_else(|| CycleError::graph(format!("scénario intégré: hash de {path} indisponible")))?
        .to_owned();
    let base_id = snapshot.id().clone();
    let patch_a = replacement_patch(path, content, "    x * 2", "    x * 3")?;
    let patch_b = replacement_patch(path, content, "    x + 1", "    x + 10")?;
    let expected = [(path.to_owned(), base_hash)];
    Ok(vec![
        ChangeProposal {
            schema_version: COLLAB_SCHEMA_VERSION,
            agent_id: "agent-a".to_owned(),
            task_id: "tune-double".to_owned(),
            base_snapshot_id: base_id.clone(),
            target_paths: vec![path.to_owned()],
            target_symbols: vec!["double".to_owned()],
            expected_hashes: expected.iter().cloned().collect(),
            declared_dependencies: Vec::new(),
            impacted_contracts: Vec::new(),
            unified_patch: patch_a,
            note: Some("agent-a: change uniquement double(), base = snapshot initial".to_owned()),
        },
        ChangeProposal {
            schema_version: COLLAB_SCHEMA_VERSION,
            agent_id: "agent-b".to_owned(),
            task_id: "tune-shift".to_owned(),
            base_snapshot_id: base_id,
            target_paths: vec![path.to_owned()],
            target_symbols: vec!["shift".to_owned()],
            expected_hashes: expected.into_iter().collect(),
            declared_dependencies: Vec::new(),
            impacted_contracts: Vec::new(),
            unified_patch: patch_b,
            note: Some("agent-b: change uniquement shift(), même base que agent-a".to_owned()),
        },
    ])
}

/// Builds an exact unified hunk that replaces one unique line, with one line of
/// context on each side. Line numbers are derived from the real content, so the
/// built-in scenario cannot drift silently if the fixture changes.
fn replacement_patch(
    path: &str,
    content: &str,
    old_line: &str,
    new_line: &str,
) -> CycleResult<String> {
    let lines: Vec<&str> = content.lines().collect();
    let matches: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| **line == old_line)
        .map(|(index, _)| index)
        .collect();
    if matches.len() != 1 {
        return Err(CycleError::graph(format!(
            "scénario intégré: la ligne {old_line:?} doit être unique dans {path} (occurrences: {})",
            matches.len()
        )));
    }
    let index = matches[0];
    let first = index.saturating_sub(1);
    let last = if index + 1 < lines.len() {
        index + 1
    } else {
        index
    };
    let mut hunk = Vec::new();
    let mut old_len = 0usize;
    let mut new_len = 0usize;
    for line in &lines[first..index] {
        hunk.push(format!(" {line}"));
        old_len += 1;
        new_len += 1;
    }
    hunk.push(format!("-{}", lines[index]));
    old_len += 1;
    hunk.push(format!("+{new_line}"));
    new_len += 1;
    for line in &lines[index + 1..=last] {
        hunk.push(format!(" {line}"));
        old_len += 1;
        new_len += 1;
    }
    Ok(format!(
        "+++ b/{path}\n@@ -{},{old_len} +{},{new_len} @@\n{}\n",
        first + 1,
        first + 1,
        hunk.join("\n")
    ))
}

/// Loads every `*.json` proposal of `directory` in deterministic file-name order.
fn load_proposals(directory: &Path) -> CycleResult<Vec<ChangeProposal>> {
    let entries = std::fs::read_dir(directory).map_err(|error| {
        CycleError::usage(format!(
            "impossible de lire le dossier de propositions {}: {error}",
            directory.display()
        ))
    })?;
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().map(|ext| ext == "json").unwrap_or(false))
        .collect();
    paths.sort();
    if paths.is_empty() {
        return Err(CycleError::usage(format!(
            "aucune proposition *.json dans {}",
            directory.display()
        )));
    }
    let mut proposals = Vec::with_capacity(paths.len());
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for path in &paths {
        let text = std::fs::read_to_string(path).map_err(|error| {
            CycleError::usage(format!("impossible de lire {}: {error}", path.display()))
        })?;
        let proposal: ChangeProposal = serde_json::from_str(&text).map_err(|error| {
            CycleError::usage(format!(
                "{} n'est pas une ChangeProposal JSON valide: {error}",
                path.display()
            ))
        })?;
        if !seen.insert(proposal.proposal_id()) {
            return Err(CycleError::usage(format!(
                "proposition dupliquée: {} (dossier {})",
                proposal.proposal_id(),
                directory.display()
            )));
        }
        proposals.push(proposal);
    }
    Ok(proposals)
}

// ---------------------------------------------------------------------------
// Analysis records + report
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
struct FileMapping {
    path: String,
    coordinator_sha256: Option<String>,
    daemon_sha256: String,
    equal: bool,
}

#[derive(Debug, Clone, Serialize)]
struct AnalysisRecord {
    pass: u32,
    revision: u64,
    change_proposal_id: String,
    change_journal_seq: u64,
    unit: String,
    unit_dir: String,
    role: String,
    changed_paths: Vec<String>,
    coordinator_snapshot_before_id: String,
    coordinator_snapshot_after_id: String,
    daemon_snapshot_id: String,
    daemon_context_id: String,
    daemon_status: String,
    daemon_success: bool,
    daemon_error_count: usize,
    daemon_message: Option<String>,
    cache_kind: String,
    cache_hit: bool,
    timings: serde_json::Value,
    file_content_mapping: Vec<FileMapping>,
    content_identity_verified: bool,
    identity_note: String,
    daemon_report: serde_json::Value,
}

/// Unit scope of one change across all passes: expected `(pass, proposal, unit)`
/// analyses. Built while running so the scope invariant can be validated from
/// observed data instead of from the code path that produced it.
type ScopeKey = (u32, String, String);

fn scope_violations(expected: &BTreeSet<ScopeKey>, actual: &BTreeSet<ScopeKey>) -> Vec<String> {
    let mut violations = Vec::new();
    for (pass, proposal, unit) in actual.difference(expected) {
        violations.push(format!(
            "unité {unit:?} analysée alors qu'elle n'est pas affectée par {proposal} (passage {pass})"
        ));
    }
    for (pass, proposal, unit) in expected.difference(actual) {
        violations.push(format!(
            "unité affectée {unit:?} non analysée pour {proposal} (passage {pass})"
        ));
    }
    violations
}

fn sanitize_label(raw: &str) -> String {
    let mut label: String = raw
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '/' | '.') {
                character
            } else {
                '-'
            }
        })
        .collect();
    if label.len() > 200 {
        label.truncate(200);
    }
    if label.is_empty() {
        label.push_str("collab4");
    }
    label
}

fn short_id(id: &str) -> String {
    id.chars().take(12).collect()
}

// ---------------------------------------------------------------------------
// Command
// ---------------------------------------------------------------------------

fn run(args: &[String]) -> CycleResult<u8> {
    let started = std::time::Instant::now();
    let Some(workspace_argument) = positional(args, 0) else {
        return Err(CycleError::usage(
            "usage: ravum collab-cycle <workspace> [--proposals <dir>] [--daemon-work <dir>] \
             [--journal <path>] [--out <path>] [--passes N] [--json] \
             [--protocol v1|v2] [--oracle unit-graph --oracle-state <dir>]",
        ));
    };
    let workspace = PathBuf::from(&workspace_argument);
    if !workspace.is_dir() {
        return Err(CycleError::usage(format!(
            "workspace introuvable ou non répertoire: {}",
            workspace.display()
        )));
    }
    let proposals_directory = flag(args, "--proposals").map(PathBuf::from);
    let daemon_work = flag(args, "--daemon-work")
        .map(PathBuf::from)
        .unwrap_or_else(|| work_dir().join("collab4-daemon"));
    let journal_base = flag(args, "--journal")
        .map(PathBuf::from)
        .unwrap_or_else(|| work_dir().join("collab4").join("journal.jsonl"));
    let out_path = flag(args, "--out").map(PathBuf::from);
    let json_output = has(args, "--json");
    let passes = flag_u64(args, "--passes", 1).clamp(1, MAX_PASSES) as u32;

    // MVP-0c L5.1: opt-in versioned protocol and injected L1 oracle. Defaults
    // (no flags) are unchanged: v1 textual compatibility, no oracle.
    let protocol = match flag(args, "--protocol").as_deref() {
        None | Some("v1") => EvaluationProtocol::V1CollabCompat,
        Some("v2") => EvaluationProtocol::V2SemanticStatuses,
        Some(other) => {
            return Err(CycleError::usage(format!(
                "--protocol attendu: v1 ou v2, reçu {other:?}"
            )));
        }
    };
    let oracle_kind = flag(args, "--oracle");
    let oracle_state = flag(args, "--oracle-state").map(PathBuf::from);
    let oracle_enabled = match oracle_kind.as_deref() {
        None | Some("none") => {
            if oracle_state.is_some() {
                return Err(CycleError::usage(
                    "--oracle-state exige --oracle unit-graph",
                ));
            }
            false
        }
        Some("unit-graph") => {
            if !protocol.is_v2() {
                return Err(CycleError::usage(
                    "--oracle unit-graph exige --protocol v2 (l'oracle n'est pas consulté en mode v1)",
                ));
            }
            true
        }
        Some(other) => {
            return Err(CycleError::usage(format!(
                "--oracle attendu: unit-graph ou none, reçu {other:?}"
            )));
        }
    };
    let oracle_state = oracle_state.unwrap_or_else(|| work_dir().join("collab-oracle"));

    // 1. Workspace -> SourceSnapshot (single content engine: ravum-snapshot).
    let base_snapshot = SourceSnapshot::from_directory(&workspace, |path| {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        name == "Cargo.toml" || path.extension().map(|ext| ext == "rs").unwrap_or(false)
    })
    .map_err(|error| {
        CycleError::usage(format!(
            "impossible de charger le workspace {}: {error}",
            workspace.display()
        ))
    })?;
    if base_snapshot.file_count() == 0 {
        return Err(CycleError::usage(format!(
            "workspace vide: aucun fichier .rs/Cargo.toml sous {}",
            workspace.display()
        )));
    }

    // 2. Unit graph (crates + path dependencies), fail-closed.
    let graph = UnitGraph::discover(&base_snapshot)?;

    // 3. Proposals.
    let proposals = match &proposals_directory {
        Some(directory) => load_proposals(directory)?,
        None => builtin_proposals(&base_snapshot)?,
    };
    let mode = match &proposals_directory {
        Some(directory) => format!("proposals:{}", directory.display()),
        None => "builtin:two-agents-two-functions".to_owned(),
    };

    // 4. One daemon for the whole cycle: its snapshots/cache persist in
    //    `--daemon-work`, which is what makes a second process a disk-cache hit.
    let daemon = Daemon::open(DaemonConfig {
        work_dir: daemon_work.clone(),
        ..DaemonConfig::default()
    })
    .map_err(|error| CycleError::usage(format!("ouverture du daemon impossible: {error}")))?;
    let toolchain = daemon.toolchain().clone();

    let mut violations: Vec<Violation> = Vec::new();
    let mut analyses: Vec<AnalysisRecord> = Vec::new();
    let mut decisions: Vec<serde_json::Value> = Vec::new();
    let mut accepted_changes: Vec<serde_json::Value> = Vec::new();
    let mut passes_report: Vec<serde_json::Value> = Vec::new();
    let mut journal_paths: Vec<String> = Vec::new();
    let mut revision: u64 = 0;
    let mut expected_scope: BTreeSet<ScopeKey> = BTreeSet::new();
    let mut actual_scope: BTreeSet<ScopeKey> = BTreeSet::new();
    let mut change_scopes: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut content_identities: BTreeMap<(String, String, String), String> = BTreeMap::new();
    let mut analyzed_units_global: BTreeSet<String> = BTreeSet::new();
    let mut skipped_units_global: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for pass in 1..=passes {
        let pass_started = std::time::Instant::now();
        let journal_path = journal_path_for(&journal_base, pass);
        let mut coordinator =
            Coordinator::new(base_snapshot.clone()).with_evaluation_protocol(protocol);
        if oracle_enabled {
            coordinator = coordinator
                .with_semantic_oracle(Arc::new(UnitGraphOracle::new(oracle_state.clone())));
        }
        coordinator = coordinator.with_journal(&journal_path).map_err(|error| {
            CycleError::usage(format!(
                "journal de décisions inutilisable {}: {error}",
                journal_path.display()
            ))
        })?;
        let stats_before = daemon.stats();
        let mut pass_decisions = 0u64;
        let mut pass_accepted = 0u64;

        for proposal in &proposals {
            let decision = coordinator.submit(proposal);
            pass_decisions += 1;
            let evaluation = coordinator.evaluations().last().cloned();
            decisions.push(json!({
                "pass": pass,
                "proposal_id": decision.proposal_id,
                "decision": decision.decision,
                "reasons": decision.reasons,
                "base_snapshot_id": decision.base_snapshot_id,
                "resulting_snapshot_id": decision.resulting_snapshot_id,
                "journal_seq": coordinator.journal().last().map(|entry| entry.seq),
                "evaluation_level": evaluation.as_ref().map(|record| record.level.as_str()),
                "oracle_blocking": evaluation
                    .as_ref()
                    .and_then(|record| record.oracle.as_ref())
                    .map(|report| report.has_blocking()),
                "evaluation": evaluation,
            }));
            if decision.decision != DecisionKind::AutoSafe {
                violations.push(Violation::new(
                    "unexpected_decision",
                    format!(
                        "passage {pass}: proposition {} -> {} (AUTO_SAFE attendu): {}",
                        decision.proposal_id,
                        decision.decision.as_str(),
                        decision.reasons.join(" | ")
                    ),
                ));
                continue;
            }
            pass_accepted += 1;
            let accepted = coordinator
                .accepted_change_for(&decision.proposal_id)
                .cloned()
                .ok_or_else(|| {
                    CycleError::new(
                        EXIT_IDENTITY_VIOLATION,
                        format!(
                            "incohérence coordinateur: décision AUTO_SAFE sans AcceptedChange pour {}",
                            decision.proposal_id
                        ),
                    )
                })?;
            accepted_changes.push(json!({"pass": pass, "record": accepted}));
            let changed_paths = accepted.paths.clone();
            let affected = graph.affected(&changed_paths)?;
            let scope_units: BTreeSet<String> = affected.affected.clone();
            match change_scopes.get(&accepted.proposal_id) {
                Some(previous) if previous != &scope_units => {
                    violations.push(Violation::new(
                        "identity",
                        format!(
                            "périmètre d'analyse instable pour {} entre passages: {:?} vs {:?}",
                            accepted.proposal_id, previous, scope_units
                        ),
                    ));
                }
                None => {
                    change_scopes.insert(accepted.proposal_id.clone(), scope_units);
                }
                _ => {}
            }
            for dir in graph.skipped(&affected.affected) {
                skipped_units_global
                    .entry(dir)
                    .or_default()
                    .insert(accepted.proposal_id.clone());
            }

            for dir in graph.ordered_subset(&affected.affected) {
                let unit = graph.unit(&dir).cloned().ok_or_else(|| {
                    CycleError::graph(format!("unité inconnue: {:?}", display_dir(&dir)))
                })?;
                let role = if affected.direct.contains(&dir) {
                    "changed"
                } else {
                    "dependent"
                };
                expected_scope.insert((pass, accepted.proposal_id.clone(), unit.dir.clone()));
                revision += 1;
                let label = sanitize_label(&format!(
                    "collab4-p{pass}-{}-{}",
                    unit.crate_name, accepted.journal_seq
                ));
                let request = build_request(&unit, coordinator.current(), revision, label)?;
                let identity = request.identity(&toolchain).map_err(|error| {
                    CycleError::new(
                        EXIT_USAGE_OR_GRAPH,
                        format!(
                            "requête daemon invalide pour {:?}: {error}",
                            unit.package_name
                        ),
                    )
                })?;
                let report = match daemon.analyze(&request) {
                    Ok(report) => report,
                    Err(error) => {
                        return Err(CycleError::new(
                            EXIT_ANALYSIS_FAILED,
                            format!(
                                "analyse daemon impossible pour {:?} (passage {pass}): {error}",
                                unit.package_name
                            ),
                        ));
                    }
                };

                // Identity: the report must be exactly the request identity we
                // computed from the submitted bytes.
                if report.snapshot_id != identity.snapshot_id {
                    violations.push(Violation::new(
                        "identity",
                        format!(
                            "identité daemon divergente pour {:?}: annoncée {}, calculée {}",
                            unit.package_name, report.snapshot_id, identity.snapshot_id
                        ),
                    ));
                }
                if report.context_id != identity.context_id {
                    violations.push(Violation::new(
                        "identity",
                        format!(
                            "contexte daemon divergent pour {:?}: annoncé {}, calculé {}",
                            unit.package_name, report.context_id, identity.context_id
                        ),
                    ));
                }

                // Content mapping: coordinator file hashes vs daemon file hashes.
                let snapshot_now = coordinator.current();
                let mut mappings = Vec::with_capacity(identity.files.len());
                let mut all_equal = identity.files.len() == unit.files.len();
                for file in &identity.files {
                    let coordinator_hash = snapshot_now.file_hash(&file.path).map(str::to_owned);
                    let equal = coordinator_hash.as_deref() == Some(file.sha256.as_str());
                    if !equal {
                        all_equal = false;
                    }
                    mappings.push(FileMapping {
                        path: file.path.clone(),
                        coordinator_sha256: coordinator_hash,
                        daemon_sha256: file.sha256.clone(),
                        equal,
                    });
                }
                if !all_equal {
                    violations.push(Violation::new(
                        "identity",
                        format!(
                            "correspondance de contenu non démontrée pour {:?} (passage {pass}): \
                             {} fichier(s) mappés",
                            unit.package_name,
                            identity.files.len()
                        ),
                    ));
                }
                let snapshot_after = snapshot_now.id().as_str().to_owned();
                if snapshot_after != accepted.snapshot_after.as_str() {
                    violations.push(Violation::new(
                        "identity",
                        format!(
                            "snapshot coordinateur courant {} différent du snapshot_after {} \
                             enregistré pour {}",
                            snapshot_after, accepted.snapshot_after, accepted.proposal_id
                        ),
                    ));
                }

                // The declared content must actually have been analysed fresh.
                if !report.is_current() {
                    violations.push(Violation::new(
                        "analysis_failed",
                        format!(
                            "analyse non courante ({}) pour {:?} (passage {pass}): {}",
                            report.status.as_str(),
                            unit.package_name,
                            report.message.clone().unwrap_or_default()
                        ),
                    ));
                }
                if !report.success || report.error_count() > 0 {
                    violations.push(Violation::new(
                        "analysis_failed",
                        format!(
                            "analyse en échec pour {:?} (passage {pass}): {} erreur(s), statut {}",
                            unit.package_name,
                            report.error_count(),
                            report.status.as_str()
                        ),
                    ));
                }

                // A replay of identical content must be served by the content
                // cache: no compiler run, and the same daemon identity.
                if pass > 1 && !report.cache.hit {
                    violations.push(Violation::new(
                        "identity",
                        format!(
                            "passage {pass} identique au passage 1 mais aucune hit de cache pour {:?}",
                            unit.package_name
                        ),
                    ));
                }
                let content_key = (
                    accepted.proposal_id.clone(),
                    unit.dir.clone(),
                    snapshot_after.clone(),
                );
                match content_identities.get(&content_key) {
                    Some(previous) if previous != &report.snapshot_id => {
                        violations.push(Violation::new(
                            "identity",
                            format!(
                                "même contenu coordonné mais identités daemon différentes pour \
                                 {:?}: {} vs {}",
                                unit.package_name, previous, report.snapshot_id
                            ),
                        ));
                    }
                    Some(_) => {}
                    None => {
                        content_identities.insert(content_key, report.snapshot_id.clone());
                    }
                }

                actual_scope.insert((pass, accepted.proposal_id.clone(), unit.dir.clone()));
                analyzed_units_global.insert(unit.dir.clone());
                analyses.push(AnalysisRecord {
                    pass,
                    revision,
                    change_proposal_id: accepted.proposal_id.clone(),
                    change_journal_seq: accepted.journal_seq,
                    unit: unit.package_name.clone(),
                    unit_dir: unit.dir.clone(),
                    role: role.to_owned(),
                    changed_paths: changed_paths.clone(),
                    coordinator_snapshot_before_id: accepted.snapshot_before.as_str().to_owned(),
                    coordinator_snapshot_after_id: snapshot_after.clone(),
                    daemon_snapshot_id: report.snapshot_id.clone(),
                    daemon_context_id: report.context_id.clone(),
                    daemon_status: report.status.as_str().to_owned(),
                    daemon_success: report.success,
                    daemon_error_count: report.error_count(),
                    daemon_message: report.message.clone(),
                    cache_kind: report.cache.kind.as_str().to_owned(),
                    cache_hit: report.cache.hit,
                    timings: json!(report.timings),
                    file_content_mapping: mappings,
                    content_identity_verified: all_equal,
                    identity_note: "les identifiants de snapshot coordinateur et daemon ne sont \
                                    pas égaux par construction (le daemon hache racine + contexte \
                                    + toolchain + fichiers de l'unité; le coordinateur hache tous \
                                    les fichiers du workspace): la correspondance est vérifiée \
                                    fichier par fichier par hash SHA-256, pas revendiquée."
                        .to_owned(),
                    daemon_report: report.to_json(),
                });
            }
        }

        // Journal: durable, append-only, one line per decision.
        let entries = coordinator.journal();
        if entries.len() as u64 != pass_decisions {
            violations.push(Violation::new(
                "journal",
                format!(
                    "journal en mémoire incohérent (passage {pass}): {} entrées pour {} décisions",
                    entries.len(),
                    pass_decisions
                ),
            ));
        }
        for (index, entry) in entries.iter().enumerate() {
            if entry.seq != index as u64 + 1 {
                violations.push(Violation::new(
                    "journal",
                    format!(
                        "séquence de journal non monotone (passage {pass}): {} à l'index {index}",
                        entry.seq
                    ),
                ));
            }
        }
        if let Some(error) = coordinator.journal_error() {
            violations.push(Violation::new(
                "journal",
                format!("écriture du journal en échec (passage {pass}): {error}"),
            ));
        }
        let durable_lines = count_lines(&journal_path).map_err(|error| {
            CycleError::usage(format!(
                "journal illisible {}: {error}",
                journal_path.display()
            ))
        })?;
        if durable_lines != entries.len() as u64 {
            violations.push(Violation::new(
                "journal",
                format!(
                    "journal durable incohérent (passage {pass}): {} ligne(s) sur disque pour {} décision(s)",
                    durable_lines,
                    entries.len()
                ),
            ));
        }
        journal_paths.push(journal_path.display().to_string());

        let stats_after = daemon.stats();
        let pass_analyses: Vec<&AnalysisRecord> = analyses
            .iter()
            .filter(|analysis| analysis.pass == pass)
            .collect();
        let hits = pass_analyses
            .iter()
            .filter(|analysis| analysis.cache_hit)
            .count() as u64;
        passes_report.push(json!({
            "pass": pass,
            "decisions": pass_decisions,
            "accepted": pass_accepted,
            "analyses": pass_analyses.len() as u64,
            "cache_hits": hits,
            "cache_misses": pass_analyses.len() as u64 - hits,
            "rustc_runs": stats_after.rustc_runs.saturating_sub(stats_before.rustc_runs),
            "journal_path": journal_path.display().to_string(),
            "journal_entries": entries.len() as u64,
            "journal_durable_lines": durable_lines,
            "duration_ms": pass_started.elapsed().as_millis() as u64,
        }));
    }

    // Scope invariant, computed from observed records only.
    for message in scope_violations(&expected_scope, &actual_scope) {
        violations.push(Violation::new("identity", message));
    }

    let skipped_units: Vec<serde_json::Value> = graph
        .order
        .iter()
        .filter(|dir| !analyzed_units_global.contains(*dir))
        .map(|dir| {
            let unit = graph.unit(dir).expect("graph unit");
            json!({
                "unit": unit.package_name,
                "dir": display_dir(dir),
                "reason": "ni modifiée ni dépendante (transitive) d'une unité modifiée: jamais analysée",
                "changes_that_skipped_it": skipped_units_global.get(dir).cloned().unwrap_or_default(),
            })
        })
        .collect();

    let daemon_stats = daemon.stats();
    let units_report: Vec<serde_json::Value> = graph
        .order
        .iter()
        .map(|dir| {
            let unit = graph.unit(dir).expect("graph unit");
            json!({
                "unit": unit.package_name,
                "dir": display_dir(dir),
                "crate_name": unit.crate_name,
                "edition": unit.edition,
                "root": unit.root,
                "files": unit.files,
                "dependencies": unit
                    .dependencies
                    .iter()
                    .map(|dependency| graph.unit(dependency).map(|unit| unit.package_name.clone()).unwrap_or_else(|| dependency.clone()))
                    .collect::<Vec<_>>(),
                "dependents": unit
                    .dependents
                    .iter()
                    .map(|dependent| graph.unit(dependent).map(|unit| unit.package_name.clone()).unwrap_or_else(|| dependent.clone()))
                    .collect::<Vec<_>>(),
            })
        })
        .collect();

    let mut exit_code = EXIT_OK;
    if violations
        .iter()
        .any(|violation| violation.kind == "identity" || violation.kind == "journal")
    {
        exit_code = EXIT_IDENTITY_VIOLATION;
    } else if violations
        .iter()
        .any(|violation| violation.kind == "analysis_failed")
    {
        exit_code = EXIT_ANALYSIS_FAILED;
    } else if violations
        .iter()
        .any(|violation| violation.kind == "unexpected_decision")
    {
        exit_code = EXIT_UNEXPECTED_DECISION;
    }

    let report = json!({
        "schema_version": REPORT_SCHEMA_VERSION,
        "command": "ravum collab-cycle",
        "generated_at_unix": ravum_protocol::now_unix(),
        "duration_ms": started.elapsed().as_millis() as u64,
        "workspace": workspace.display().to_string(),
        "mode": mode,
        "protocol": protocol.as_str(),
        "oracle": {
            "kind": if oracle_enabled { "unit-graph" } else { "none" },
            "state_dir": if oracle_enabled { Some(oracle_state.display().to_string()) } else { None },
            "note": "l'oracle L1 n'est consulté qu'en protocole v2 et seulement s'il est demandé; son indisponibilité est fail-closed (MANUAL_REQUIRED)",
        },
        "passes_requested": passes,
        "environment": {
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "toolchain": toolchain,
            "daemon_version": daemon_stats_as_version(&analyses),
            "machine_note": "poste de référence chargé pendant la campagne: campagnes de tests sérialisées (--test-threads=1), timings mono-exécution non représentatifs",
        },
        "workspace_snapshot": {
            "base_snapshot_id": base_snapshot.id().as_str(),
            "file_count": base_snapshot.file_count(),
            "files": base_snapshot.paths().collect::<Vec<_>>(),
        },
        "unit_graph": {
            "units": units_report,
            "topological_order": graph.order,
        },
        "decisions": decisions,
        "accepted_changes": accepted_changes,
        "analyses": analyses,
        "skipped_units": skipped_units,
        "passes": passes_report,
        "journal_paths": journal_paths,
        "daemon": {
            "work_dir": daemon_work.display().to_string(),
            "stats": daemon_stats,
            "cache_hit_rate": daemon_stats.cache_hit_rate(),
        },
        "identity": {
            "content_mapping_rule": "hash SHA-256 fichier par fichier: snapshot coordinateur vs identité daemon calculée depuis les octets soumis",
            "coordinator_vs_daemon_snapshot_ids": "jamais égaux par construction; aucune égalité revendiquée sans vérification de contenu",
            "replay_rule": "un second passage identique doit être servi par le cache daemon (hit) sans exécution rustc",
        },
        "violations": violations,
        "exit_code": exit_code,
    });

    if let Some(path) = &out_path {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|error| {
                    CycleError::usage(format!(
                        "impossible de créer le dossier du rapport {}: {error}",
                        parent.display()
                    ))
                })?;
            }
        }
        let text = serde_json::to_string_pretty(&report).map_err(|error| {
            CycleError::usage(format!("sérialisation du rapport impossible: {error}"))
        })?;
        std::fs::write(path, format!("{text}\n")).map_err(|error| {
            CycleError::usage(format!(
                "écriture du rapport {} impossible: {error}",
                path.display()
            ))
        })?;
    }

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_owned())
        );
    } else {
        print_human_summary(&report, &analyses, exit_code);
    }
    Ok(exit_code)
}

fn daemon_stats_as_version(analyses: &[AnalysisRecord]) -> String {
    analyses
        .iter()
        .find_map(|analysis| {
            analysis
                .daemon_report
                .get("daemon_version")
                .and_then(|value| value.as_str())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "unknown".to_owned())
}

/// Journal path of one pass: the requested path for pass 1, a suffixed path for
/// additional passes so no durable decision history is truncated.
fn journal_path_for(base: &Path, pass: u32) -> PathBuf {
    if pass == 1 {
        return base.to_path_buf();
    }
    let stem = base
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "journal".to_owned());
    let extension = base
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()))
        .unwrap_or_default();
    let file = format!("{stem}.pass{pass}{extension}");
    match base.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(file),
        _ => PathBuf::from(file),
    }
}

fn count_lines(path: &Path) -> std::io::Result<u64> {
    let text = std::fs::read_to_string(path)?;
    Ok(text.lines().count() as u64)
}

fn build_request(
    unit: &Unit,
    snapshot: &SourceSnapshot,
    revision: u64,
    label: String,
) -> CycleResult<AnalysisRequest> {
    let mut files = Vec::with_capacity(unit.files.len());
    for path in &unit.files {
        let content = snapshot.content_of(path).ok_or_else(|| {
            CycleError::new(
                EXIT_IDENTITY_VIOLATION,
                format!(
                    "fichier {path} déclaré par l'unité {:?} absent du snapshot courant",
                    unit.package_name
                ),
            )
        })?;
        files.push(ravum_daemon::SourceFile {
            path: path.clone(),
            content: content.to_owned(),
        });
    }
    Ok(AnalysisRequest {
        revision,
        label,
        root: unit.root.clone(),
        files,
        context: BuildContext {
            edition: unit.edition.clone(),
            crate_name: unit.crate_name.clone(),
            crate_type: "lib".to_owned(),
            flags: Vec::new(),
        },
        priority: ravum_daemon::Priority::Normal,
    })
}

fn print_human_summary(report: &serde_json::Value, analyses: &[AnalysisRecord], exit_code: u8) {
    let workspace = report["workspace"].as_str().unwrap_or("?");
    println!(
        "collab-cycle workspace={workspace} mode={}",
        report["mode"].as_str().unwrap_or("?")
    );
    println!(
        "protocol={} oracle={} oracle_state={}",
        report["protocol"].as_str().unwrap_or("?"),
        report["oracle"]["kind"].as_str().unwrap_or("none"),
        report["oracle"]["state_dir"].as_str().unwrap_or("-")
    );
    let units = report["unit_graph"]["units"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let names: Vec<String> = units
        .iter()
        .map(|unit| {
            let name = unit["unit"].as_str().unwrap_or("?");
            let dependencies = unit["dependencies"]
                .as_array()
                .map(|dependencies| {
                    dependencies
                        .iter()
                        .filter_map(|value| value.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_default();
            if dependencies.is_empty() {
                name.to_owned()
            } else {
                format!("{name} -> [{dependencies}]")
            }
        })
        .collect();
    println!("units: {}", names.join(", "));
    println!(
        "workspace snapshot {} ({} fichiers)",
        short_id(
            report["workspace_snapshot"]["base_snapshot_id"]
                .as_str()
                .unwrap_or("?")
        ),
        report["workspace_snapshot"]["file_count"]
            .as_u64()
            .unwrap_or(0)
    );

    for pass in report["passes"].as_array().cloned().unwrap_or_default() {
        println!(
            "passage {}: décisions={} acceptées={} analyses={} hits={} misses={} rustc_runs={}",
            pass["pass"].as_u64().unwrap_or(0),
            pass["decisions"].as_u64().unwrap_or(0),
            pass["accepted"].as_u64().unwrap_or(0),
            pass["analyses"].as_u64().unwrap_or(0),
            pass["cache_hits"].as_u64().unwrap_or(0),
            pass["cache_misses"].as_u64().unwrap_or(0),
            pass["rustc_runs"].as_u64().unwrap_or(0),
        );
    }

    for analysis in analyses {
        println!(
            "  [p{}] {:<14} role={:<9} cache={:<6} status={} errors={} analysis={}ms total={}ms daemon={}",
            analysis.pass,
            analysis.unit,
            analysis.role,
            analysis.cache_kind,
            analysis.daemon_status,
            analysis.daemon_error_count,
            analysis
                .timings
                .get("analysis_ms")
                .and_then(|value| value.as_u64())
                .unwrap_or(0),
            analysis
                .timings
                .get("total_ms")
                .and_then(|value| value.as_u64())
                .unwrap_or(0),
            short_id(&analysis.daemon_snapshot_id),
        );
        println!(
            "        coordinateur {} -> {}  contenu vérifié={}",
            short_id(&analysis.coordinator_snapshot_before_id),
            short_id(&analysis.coordinator_snapshot_after_id),
            analysis.content_identity_verified,
        );
    }

    let skipped = report["skipped_units"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if skipped.is_empty() {
        println!("unités sautées: aucune");
    } else {
        let names: Vec<String> = skipped
            .iter()
            .filter_map(|unit| unit["unit"].as_str())
            .map(str::to_owned)
            .collect();
        println!("unités sautées: {}", names.join(", "));
    }

    let stats = &report["daemon"]["stats"];
    println!(
        "daemon: analyses={} hits_memory={} hits_disk={} misses={} rustc_runs={}",
        stats["requests"].as_u64().unwrap_or(0),
        stats["cache_hits_memory"].as_u64().unwrap_or(0),
        stats["cache_hits_disk"].as_u64().unwrap_or(0),
        stats["cache_misses"].as_u64().unwrap_or(0),
        stats["rustc_runs"].as_u64().unwrap_or(0),
    );
    let violations = report["violations"].as_array().cloned().unwrap_or_default();
    if violations.is_empty() {
        println!("invariants: OK");
    } else {
        for violation in &violations {
            println!(
                "VIOLATION [{}] {}",
                violation["kind"].as_str().unwrap_or("?"),
                violation["message"].as_str().unwrap_or("?")
            );
        }
    }
    println!("exit={exit_code}");
}

// ---------------------------------------------------------------------------
// Unit tests (no rustc, no daemon: the graph and the scenario only)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_workspace() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/collab4/workspace")
    }

    fn fixture_snapshot() -> SourceSnapshot {
        let workspace = fixture_workspace();
        SourceSnapshot::from_directory(&workspace, |path| {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            name == "Cargo.toml" || path.extension().map(|ext| ext == "rs").unwrap_or(false)
        })
        .expect("fixture workspace")
    }

    fn snapshot(files: &[(&str, &str)]) -> SourceSnapshot {
        SourceSnapshot::new(
            files
                .iter()
                .map(|(path, content)| ((*path).to_owned(), (*content).to_owned())),
        )
    }

    #[test]
    fn graph_discovers_units_and_path_dependent_edges() {
        let graph = UnitGraph::discover(&fixture_snapshot()).expect("graph");
        assert_eq!(graph.units.len(), 3, "crate_a, crate_b, crate_c");
        let crate_a = graph.unit("crate_a").expect("crate_a");
        let crate_b = graph.unit("crate_b").expect("crate_b");
        let crate_c = graph.unit("crate_c").expect("crate_c");
        assert_eq!(crate_a.root, "crate_a/src/lib.rs");
        assert_eq!(crate_a.dependencies.len(), 0);
        assert_eq!(crate_c.dependencies.len(), 0);
        assert_eq!(
            crate_b.dependencies.iter().collect::<Vec<_>>(),
            vec![&"crate_a".to_owned()],
            "crate_b must depend on crate_a by path"
        );
        assert!(crate_a.dependents.contains("crate_b"));
        assert!(!crate_c.dependents.contains("crate_b"));
        let order = &graph.order;
        assert!(
            order.iter().position(|dir| dir == "crate_a")
                < order.iter().position(|dir| dir == "crate_b"),
            "dependencies must come before dependents: {order:?}"
        );
    }

    #[test]
    fn affected_closure_is_direct_changed_plus_transitive_dependents() {
        let graph = UnitGraph::discover(&snapshot(&[
            ("a/Cargo.toml", "[package]\nname = \"a\"\nedition = \"2021\"\n[workspace]\n"),
            ("a/src/lib.rs", "pub fn a() {}\n"),
            ("b/Cargo.toml", "[package]\nname = \"b\"\nedition = \"2021\"\n[workspace]\n\n[dependencies]\na = { path = \"../a\" }\n"),
            ("b/src/lib.rs", "pub fn b() {}\n"),
            ("c/Cargo.toml", "[package]\nname = \"c\"\nedition = \"2021\"\n[workspace]\n\n[dependencies]\nb = { path = \"../b\" }\n"),
            ("c/src/lib.rs", "pub fn c() {}\n"),
            ("d/Cargo.toml", "[package]\nname = \"d\"\nedition = \"2021\"\n[workspace]\n"),
            ("d/src/lib.rs", "pub fn d() {}\n"),
        ]))
        .expect("graph");
        let affected = graph
            .affected(&["a/src/lib.rs".to_owned()])
            .expect("affected");
        assert_eq!(
            affected.direct.iter().collect::<Vec<_>>(),
            vec![&"a".to_owned()]
        );
        assert_eq!(
            affected.affected.iter().collect::<Vec<_>>(),
            vec![&"a".to_owned(), &"b".to_owned(), &"c".to_owned()],
            "transitive dependents must be scheduled"
        );
        assert_eq!(graph.skipped(&affected.affected), vec!["d".to_owned()]);
    }

    #[test]
    fn unreadable_manifest_fails_closed() {
        let error = UnitGraph::discover(&snapshot(&[
            ("a/Cargo.toml", "[package]\nedition = \"2021\"\n"),
            ("a/src/lib.rs", "pub fn a() {}\n"),
        ]))
        .expect_err("a Cargo.toml without [package] name must be refused");
        assert_eq!(error.code, EXIT_USAGE_OR_GRAPH);
        assert!(
            error.message.contains("aucun [package] name lisible"),
            "message: {}",
            error.message
        );

        let error = UnitGraph::discover(&snapshot(&[
            (
                "a/Cargo.toml",
                "[package]\nname = \"a\"\n[dependencies\nbroken\n",
            ),
            ("a/src/lib.rs", "pub fn a() {}\n"),
        ]))
        .expect_err("a malformed section header must be refused");
        assert_eq!(error.code, EXIT_USAGE_OR_GRAPH);
    }

    #[test]
    fn missing_path_dependency_fails_closed() {
        let error = UnitGraph::discover(&snapshot(&[
            ("a/Cargo.toml", "[package]\nname = \"a\"\n[workspace]\n"),
            ("a/src/lib.rs", "pub fn a() {}\n"),
            ("b/Cargo.toml", "[package]\nname = \"b\"\n[workspace]\n\n[dependencies]\na = { path = \"../missing\" }\n"),
            ("b/src/lib.rs", "pub fn b() {}\n"),
        ]))
        .expect_err("unresolvable path dependency must be refused");
        assert_eq!(error.code, EXIT_USAGE_OR_GRAPH);
        assert!(
            error.message.contains("introuvable"),
            "message: {}",
            error.message
        );
    }

    #[test]
    fn rust_file_outside_every_crate_fails_closed() {
        let error = UnitGraph::discover(&snapshot(&[
            ("a/Cargo.toml", "[package]\nname = \"a\"\n[workspace]\n"),
            ("a/src/lib.rs", "pub fn a() {}\n"),
            ("stray.rs", "pub fn stray() {}\n"),
        ]))
        .expect_err("a .rs file outside every crate cannot be attributed to a unit");
        assert_eq!(error.code, EXIT_USAGE_OR_GRAPH);
        assert!(error.message.contains("hors de toute crate"));
    }

    #[test]
    fn dependency_cycle_fails_closed() {
        let error = UnitGraph::discover(&snapshot(&[
            (
                "a/Cargo.toml",
                "[package]\nname = \"a\"\n[workspace]\n\n[dependencies]\nb = { path = \"../b\" }\n",
            ),
            ("a/src/lib.rs", "pub fn a() {}\n"),
            (
                "b/Cargo.toml",
                "[package]\nname = \"b\"\n[workspace]\n\n[dependencies]\na = { path = \"../a\" }\n",
            ),
            ("b/src/lib.rs", "pub fn b() {}\n"),
        ]))
        .expect_err("a path-dependency cycle must be refused");
        assert!(
            error.message.contains("cycle"),
            "message: {}",
            error.message
        );
    }

    #[test]
    fn builtin_scenario_is_auto_safe_and_targets_only_crate_a_plus_dependent() {
        let base = fixture_snapshot();
        let graph = UnitGraph::discover(&base).expect("graph");
        let proposals = builtin_proposals(&base).expect("proposals");
        let mut coordinator = Coordinator::new(base);
        for proposal in &proposals {
            let decision = coordinator.submit(proposal);
            assert_eq!(
                decision.decision,
                DecisionKind::AutoSafe,
                "{}: {:?}",
                proposal.proposal_id(),
                decision.reasons
            );
        }
        let content = coordinator
            .current()
            .content_of("crate_a/src/lib.rs")
            .expect("integrated content");
        assert!(content.contains("x * 3"), "agent-a intent preserved");
        assert!(content.contains("x + 10"), "agent-b intent present");

        let accepted = coordinator.accepted_changes();
        assert_eq!(accepted.len(), 2);
        let mut affected_for_change = Vec::new();
        for change in accepted {
            let affected = graph.affected(&change.paths).expect("affected");
            affected_for_change.push(affected.affected.clone());
        }
        assert_eq!(
            affected_for_change[0].iter().collect::<Vec<_>>(),
            vec![&"crate_a".to_owned(), &"crate_b".to_owned()],
            "crate_a and its dependent crate_b are affected, crate_c is not"
        );
        assert_eq!(affected_for_change[0], affected_for_change[1]);
        assert_eq!(
            graph.skipped(&affected_for_change[1]),
            vec!["crate_c".to_owned()]
        );
    }

    #[test]
    fn replacement_patch_applies_exactly_and_rejects_ambiguous_markers() {
        let content = fixture_snapshot()
            .content_of("crate_a/src/lib.rs")
            .expect("crate_a")
            .to_owned();
        let patch = replacement_patch("crate_a/src/lib.rs", &content, "    x * 2", "    x * 3")
            .expect("patch");
        let files = ravum_workspace_coordinator::parse(&patch).expect("patch parses");
        let applied =
            ravum_workspace_coordinator::apply(&files[0], &content).expect("patch applies");
        assert!(applied.contains("x * 3"));
        assert!(applied.contains("x + 1"), "the other function is untouched");
        let error = replacement_patch("crate_a/src/lib.rs", &content, "pub fn", "pub fn")
            .expect_err("ambiguous marker must be refused");
        assert_eq!(error.code, EXIT_USAGE_OR_GRAPH);
    }

    #[test]
    fn scope_validation_detects_unaffected_analysis_and_missing_analysis() {
        let mut expected: BTreeSet<ScopeKey> = BTreeSet::new();
        expected.insert((1, "agent-a/t".to_owned(), "crate_a".to_owned()));
        expected.insert((1, "agent-a/t".to_owned(), "crate_b".to_owned()));
        let mut actual = expected.clone();
        assert!(scope_violations(&expected, &actual).is_empty());

        actual.insert((1, "agent-a/t".to_owned(), "crate_c".to_owned()));
        let violations = scope_violations(&expected, &actual);
        assert_eq!(violations.len(), 1);
        assert!(
            violations[0].contains("crate_c") && violations[0].contains("pas affectée"),
            "{violations:?}"
        );

        actual.remove(&(1, "agent-a/t".to_owned(), "crate_b".to_owned()));
        let violations = scope_violations(&expected, &actual);
        assert!(
            violations
                .iter()
                .any(|message| message.contains("non analysée")),
            "{violations:?}"
        );
    }

    #[test]
    fn manifest_parser_reads_package_edition_and_table_dependencies() {
        let parsed = parse_manifest(
            "Cargo.toml",
            "[package]\nname = \"some-crate\"\nedition = \"2021\"\n\n[workspace]\n\n[dependencies]\nserde = \"1\"\n\n[dependencies.other]\npath = \"../other\"\n",
        )
        .expect("manifest");
        assert_eq!(parsed.package_name, "some-crate");
        assert_eq!(parsed.edition, "2021");
        assert_eq!(sanitize_crate_name(&parsed.package_name), "some_crate");
        assert_eq!(
            parsed.dependencies,
            vec![
                ("serde".to_owned(), None),
                ("other".to_owned(), Some("../other".to_owned()))
            ]
        );
    }
}
