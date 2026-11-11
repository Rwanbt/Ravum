//! Case discovery and strict `case.json` validation.
//!
//! The corpus layout is `conformance/cases/<taxonomy>/<case>/case.json`. Any
//! deviation is reported as an *invalid case* (a FAIL when running), never
//! skipped: a malformed case must not disappear from the report.

use crate::json::Json;
use crate::util::{is_safe_relative_path, looks_absolute};
use std::fs;
use std::path::{Path, PathBuf};

/// Closed list of taxonomies. The order is the report order.
pub const TAXONOMIES: &[&str] = &[
    "lexer",
    "parser",
    "resolve",
    "types",
    "ownership",
    "effects",
    "run",
    "unsupported",
    "modules",
    "packages",
    "fmt",
    "cli",
    "security",
];

pub fn taxonomy_index(taxonomy: &str) -> usize {
    TAXONOMIES
        .iter()
        .position(|candidate| *candidate == taxonomy)
        .unwrap_or(TAXONOMIES.len())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    CompilePass,
    CompileFail,
    RunPass,
    RunFail,
    FmtIdempotent,
    Cli,
}

impl Mode {
    pub fn parse(text: &str) -> Option<Mode> {
        match text {
            "compile-pass" => Some(Mode::CompilePass),
            "compile-fail" => Some(Mode::CompileFail),
            "run-pass" => Some(Mode::RunPass),
            "run-fail" => Some(Mode::RunFail),
            "fmt-idempotent" => Some(Mode::FmtIdempotent),
            "cli" => Some(Mode::Cli),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Mode::CompilePass => "compile-pass",
            Mode::CompileFail => "compile-fail",
            Mode::RunPass => "run-pass",
            Mode::RunFail => "run-fail",
            Mode::FmtIdempotent => "fmt-idempotent",
            Mode::Cli => "cli",
        }
    }

    /// True when a *passing* case of this mode exercised a buildable program.
    ///
    /// This is the perimeter of the `check ⇒ build` contract: `compile-pass`
    /// and `fmt-idempotent` gain an explicit `ravum-lang build`, while
    /// `run-pass`/`run-fail` already compile and execute the program. A
    /// `compile-fail` program is invalid by construction (it must not build)
    /// and a `cli` case drives arbitrary commands, so neither is a build
    /// witness.
    pub fn builds_program(&self) -> bool {
        match self {
            Mode::CompilePass | Mode::RunPass | Mode::RunFail | Mode::FmtIdempotent => true,
            Mode::CompileFail | Mode::Cli => false,
        }
    }

    /// Modelled exit code when `case.json` does not override it explicitly.
    pub fn default_exit_code(&self) -> Option<i32> {
        match self {
            Mode::CompilePass => Some(0),
            Mode::CompileFail => Some(1),
            Mode::RunPass => Some(0),
            Mode::RunFail => Some(1),
            Mode::FmtIdempotent => None,
            Mode::Cli => Some(0),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixtureKind {
    SymlinkDir,
    SymlinkFile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fixture {
    pub kind: FixtureKind,
    /// Link path, relative to the isolated case copy.
    pub path: String,
    /// Link target; may contain `..` but must stay relative.
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedFile {
    pub path: String,
    pub contains: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CaseSpec {
    pub id: String,
    pub mode: Mode,
    /// Package root under test, relative to the case directory. `"."` = root.
    pub project: String,
    pub expect_codes: Vec<String>,
    pub args: Vec<String>,
    pub stdin: String,
    pub stdout_contains: Option<String>,
    pub stderr_contains: Option<String>,
    pub exit_code: Option<i32>,
    pub skip_on: Vec<String>,
    pub unsupported: bool,
    pub notes: String,
    /// Files that must exist after the run (relative to the project root).
    pub expect_files: Vec<ExpectedFile>,
    pub expect_json_schema_version: Option<i64>,
    pub timeout_ms: Option<u64>,
    pub fixtures: Vec<Fixture>,
}

#[derive(Debug, Clone)]
pub struct CaseEntry {
    pub id: String,
    pub taxonomy: String,
    /// Repository-relative path (`lexer/foo`), used by `--filter` and reports.
    pub rel_dir: String,
    pub dir: PathBuf,
    /// Best-effort mode for invalid entries; valid entries carry `spec.mode`.
    pub mode: Mode,
    pub spec: Result<CaseSpec, String>,
}

impl CaseEntry {
    pub fn mode(&self) -> Mode {
        match &self.spec {
            Ok(spec) => spec.mode,
            Err(_) => self.mode,
        }
    }
}

const KNOWN_FIELDS: &[&str] = &[
    "id",
    "mode",
    "project",
    "expect_codes",
    "args",
    "stdin",
    "stdout_contains",
    "stderr_contains",
    "exit_code",
    "skip_on",
    "unsupported",
    "notes",
    "expect_files",
    "expect_file_contains",
    "expect_json_schema_version",
    "timeout_ms",
    "fixtures",
];

const SKIP_OSES: &[&str] = &["windows", "linux", "macos"];

/// Discovers cases below `cases_root`. Returns every directory as an entry,
/// including malformed ones (as `Err`), so nothing is silently hidden.
pub fn discover(cases_root: &Path, filter: Option<&str>) -> Result<Vec<CaseEntry>, String> {
    if !cases_root.is_dir() {
        return Err(format!(
            "cases directory not found: {}",
            cases_root.display()
        ));
    }
    let mut entries = Vec::new();
    let taxonomy_dirs = read_dir_sorted(cases_root)
        .map_err(|error| format!("cannot read {}: {error}", cases_root.display()))?;
    for taxonomy_item in taxonomy_dirs {
        let taxonomy = taxonomy_item.name.clone();
        if !taxonomy_item.is_dir {
            push_invalid(
                &mut entries,
                taxonomy.clone(),
                taxonomy.clone(),
                taxonomy_item.path,
                Mode::CompilePass,
                format!(
                    "unexpected file `{}` at the corpus root (only taxonomy directories are allowed)",
                    taxonomy_item.name
                ),
            );
            continue;
        }
        if !TAXONOMIES.contains(&taxonomy.as_str()) {
            push_invalid(
                &mut entries,
                taxonomy.clone(),
                taxonomy.clone(),
                taxonomy_item.path,
                Mode::CompilePass,
                format!(
                    "unknown taxonomy `{taxonomy}`; expected one of: {}",
                    TAXONOMIES.join(", ")
                ),
            );
            continue;
        }
        let case_dirs = match read_dir_sorted(&taxonomy_item.path) {
            Ok(items) => items,
            Err(error) => {
                push_invalid(
                    &mut entries,
                    taxonomy.clone(),
                    taxonomy.clone(),
                    taxonomy_item.path,
                    Mode::CompilePass,
                    format!("cannot read taxonomy directory: {error}"),
                );
                continue;
            }
        };
        for case_item in case_dirs {
            let rel_dir = format!("{taxonomy}/{}", case_item.name);
            if !case_item.is_dir {
                if matches_filter(filter, &rel_dir, &rel_dir) {
                    push_invalid(
                        &mut entries,
                        rel_dir.clone(),
                        taxonomy.clone(),
                        case_item.path,
                        Mode::CompilePass,
                        "unexpected file inside a taxonomy directory (a case is a directory containing case.json)".to_string(),
                    );
                }
                continue;
            }
            let entry = load_entry(&taxonomy, &rel_dir, &case_item.path);
            if matches_filter(filter, &entry.rel_dir, &entry.id) {
                entries.push(entry);
            }
        }
    }
    mark_duplicate_ids(&mut entries);
    entries.sort_by(|a, b| {
        taxonomy_index(&a.taxonomy)
            .cmp(&taxonomy_index(&b.taxonomy))
            .then_with(|| a.rel_dir.cmp(&b.rel_dir))
    });
    Ok(entries)
}

fn push_invalid(
    entries: &mut Vec<CaseEntry>,
    id: String,
    taxonomy: String,
    dir: PathBuf,
    mode: Mode,
    message: String,
) {
    entries.push(CaseEntry {
        id,
        taxonomy,
        rel_dir: String::new(),
        dir,
        mode,
        spec: Err(message),
    });
}

fn matches_filter(filter: Option<&str>, rel_dir: &str, id: &str) -> bool {
    match filter {
        None => true,
        Some(needle) => rel_dir.contains(needle) || id.contains(needle),
    }
}

fn load_entry(taxonomy: &str, rel_dir: &str, dir: &Path) -> CaseEntry {
    let case_file = dir.join("case.json");
    let fallback_mode = Mode::CompilePass;
    let text = match fs::read_to_string(&case_file) {
        Ok(text) => text,
        Err(error) => {
            return CaseEntry {
                id: rel_dir.to_string(),
                taxonomy: taxonomy.to_string(),
                rel_dir: rel_dir.to_string(),
                dir: dir.to_path_buf(),
                mode: fallback_mode,
                spec: Err(format!("cannot read case.json: {error}")),
            }
        }
    };
    let json = match Json::parse(&text) {
        Ok(json) => json,
        Err(error) => {
            return CaseEntry {
                id: rel_dir.to_string(),
                taxonomy: taxonomy.to_string(),
                rel_dir: rel_dir.to_string(),
                dir: dir.to_path_buf(),
                mode: fallback_mode,
                spec: Err(format!("invalid JSON in case.json: {error}")),
            }
        }
    };
    let Some(entries) = json.as_object() else {
        return CaseEntry {
            id: rel_dir.to_string(),
            taxonomy: taxonomy.to_string(),
            rel_dir: rel_dir.to_string(),
            dir: dir.to_path_buf(),
            mode: fallback_mode,
            spec: Err("case.json must contain a JSON object".to_string()),
        };
    };
    let id_hint = json
        .get("id")
        .and_then(Json::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(rel_dir)
        .to_string();
    let mode_hint = json
        .get("mode")
        .and_then(Json::as_str)
        .and_then(Mode::parse)
        .unwrap_or(fallback_mode);
    match parse_spec(entries) {
        Ok(spec) => CaseEntry {
            id: spec.id.clone(),
            taxonomy: taxonomy.to_string(),
            rel_dir: rel_dir.to_string(),
            dir: dir.to_path_buf(),
            mode: spec.mode,
            spec: Ok(spec),
        },
        Err(error) => CaseEntry {
            id: id_hint,
            taxonomy: taxonomy.to_string(),
            rel_dir: rel_dir.to_string(),
            dir: dir.to_path_buf(),
            mode: mode_hint,
            spec: Err(error),
        },
    }
}

/// Rejects duplicate keys in the `case.json` object (a duplicate would make
/// the effective value depend on the reader, which is not deterministic).
fn check_no_duplicate_keys(entries: &[(String, Json)]) -> Result<(), String> {
    let mut seen: Vec<&str> = Vec::new();
    for (key, _) in entries {
        if seen.contains(&key.as_str()) {
            return Err(format!("duplicate key `{key}` in case.json"));
        }
        seen.push(key);
    }
    Ok(())
}

fn check_known_fields(entries: &[(String, Json)]) -> Result<(), String> {
    for (key, _) in entries {
        if !KNOWN_FIELDS.contains(&key.as_str()) {
            return Err(format!(
                "unknown field `{key}` in case.json (known fields: {})",
                KNOWN_FIELDS.join(", ")
            ));
        }
    }
    Ok(())
}

fn parse_spec(entries: &[(String, Json)]) -> Result<CaseSpec, String> {
    let object = Json::Object(entries.to_vec());
    check_no_duplicate_keys(entries)?;
    check_known_fields(entries)?;

    let id = require_string(&object, "id")?;
    if id.len() > 200 {
        return Err(format!("id is too long ({} bytes, max 200)", id.len()));
    }
    if id.chars().any(|ch| ch.is_control()) {
        return Err("id must not contain control characters".to_string());
    }

    let mode_text = require_string(&object, "mode")?;
    let mode = Mode::parse(&mode_text).ok_or_else(|| {
        format!(
            "unknown mode `{mode_text}`; expected one of: compile-pass, compile-fail, run-pass, run-fail, fmt-idempotent, cli"
        )
    })?;

    let project = optional_string(&object, "project")?.unwrap_or_else(|| ".".to_string());
    if !is_safe_relative_path(&project) {
        return Err(format!(
            "project must be a safe relative path inside the case (got `{project}`)"
        ));
    }

    let expect_codes = string_array(&object, "expect_codes")?;
    for code in &expect_codes {
        if !is_valid_code(code) {
            return Err(format!(
                "invalid diagnostic code `{code}` (expected E-SEGMENT[-SEGMENT...] or \
                 W-SEGMENT[-SEGMENT...], uppercase)"
            ));
        }
    }

    let args = string_array(&object, "args")?;
    if mode == Mode::Cli {
        if args.is_empty() {
            return Err("cli mode requires a non-empty `args` array".to_string());
        }
        for arg in &args {
            if looks_absolute(arg) {
                return Err(format!(
                    "cli argument `{arg}` looks absolute; cli cases may only touch the isolated copy"
                ));
            }
        }
    }

    let stdin = optional_string(&object, "stdin")?.unwrap_or_default();
    let stdout_contains = optional_string(&object, "stdout_contains")?;
    let stderr_contains = optional_string(&object, "stderr_contains")?;
    let notes = optional_string(&object, "notes")?.unwrap_or_default();

    let exit_code = match object.get("exit_code") {
        None => None,
        Some(value) => {
            let code = value
                .as_i64()
                .ok_or_else(|| "exit_code must be an integer".to_string())?;
            if !(0..=255).contains(&code) {
                return Err(format!("exit_code {code} is outside 0..=255"));
            }
            Some(code as i32)
        }
    };
    match (mode, exit_code) {
        (Mode::CompilePass, Some(code)) if code != 0 => {
            return Err(format!(
                "compile-pass always expects exit code 0, but exit_code is {code}"
            ))
        }
        (Mode::CompileFail, Some(code)) if code != 1 => {
            return Err(format!(
                "compile-fail always expects exit code 1, but exit_code is {code}"
            ))
        }
        (Mode::FmtIdempotent, Some(_)) => {
            return Err(
                "fmt-idempotent checks the exit codes of its own steps; exit_code is not allowed"
                    .to_string(),
            )
        }
        _ => {}
    }

    let skip_on = string_array(&object, "skip_on")?;
    for os in &skip_on {
        if !SKIP_OSES.contains(&os.as_str()) {
            return Err(format!(
                "invalid skip_on value `{os}`; expected one of: {}",
                SKIP_OSES.join(", ")
            ));
        }
    }

    let unsupported = match object.get("unsupported") {
        None => false,
        Some(value) => value
            .as_bool()
            .ok_or_else(|| "unsupported must be a boolean".to_string())?,
    };
    if unsupported {
        if mode != Mode::CompileFail {
            return Err(
                "unsupported: true only documents compile-fail cases for excluded features"
                    .to_string(),
            );
        }
        if !expect_codes
            .iter()
            .any(|code| code.starts_with("E-UNSUPPORTED-"))
        {
            return Err(
                "unsupported: true requires at least one E-UNSUPPORTED-* code in expect_codes"
                    .to_string(),
            );
        }
    }

    if mode != Mode::CompileFail && !expect_codes.is_empty() {
        return Err(
            "expect_codes is only meaningful for compile-fail (runtime errors have no diagnostic code)"
                .to_string(),
        );
    }
    if mode == Mode::CompileFail && expect_codes.is_empty() {
        return Err(
            "compile-fail requires at least one expected diagnostic code: refusing to accept any error is not a conformance check"
                .to_string(),
        );
    }
    if mode == Mode::RunFail
        && stderr_contains
            .as_deref()
            .map(str::is_empty)
            .unwrap_or(true)
        && stdout_contains
            .as_deref()
            .map(str::is_empty)
            .unwrap_or(true)
    {
        return Err(
            "run-fail must verify the failure message with `stderr_contains` (e.g. \"panic:\" or \"error:\") in addition to the exit code"
                .to_string(),
        );
    }

    let expect_files = parse_expected_files(&object)?;
    let expect_json_schema_version = match object.get("expect_json_schema_version") {
        None => None,
        Some(value) => Some(
            value
                .as_i64()
                .ok_or_else(|| "expect_json_schema_version must be an integer".to_string())?,
        ),
    };
    let timeout_ms = match object.get("timeout_ms") {
        None => None,
        Some(value) => {
            let timeout = value
                .as_i64()
                .ok_or_else(|| "timeout_ms must be an integer".to_string())?;
            if !(1..=3_600_000).contains(&timeout) {
                return Err(format!("timeout_ms {timeout} is outside 1..=3600000"));
            }
            Some(timeout as u64)
        }
    };
    let fixtures = parse_fixtures(&object)?;

    Ok(CaseSpec {
        id,
        mode,
        project,
        expect_codes,
        args,
        stdin,
        stdout_contains,
        stderr_contains,
        exit_code,
        skip_on,
        unsupported,
        notes,
        expect_files,
        expect_json_schema_version,
        timeout_ms,
        fixtures,
    })
}

fn parse_expected_files(object: &Json) -> Result<Vec<ExpectedFile>, String> {
    let mut files = Vec::new();
    if let Some(list) = object.get("expect_files") {
        for value in list
            .as_array()
            .ok_or_else(|| "expect_files must be an array of strings".to_string())?
        {
            let path = value
                .as_str()
                .ok_or_else(|| "expect_files must contain strings".to_string())?;
            if !is_safe_relative_path(path) {
                return Err(format!(
                    "expect_files path `{path}` must be a safe relative path"
                ));
            }
            files.push(ExpectedFile {
                path: path.to_string(),
                contains: None,
            });
        }
    }
    if let Some(list) = object.get("expect_file_contains") {
        for value in list.as_array().ok_or_else(|| {
            "expect_file_contains must be an array of {path, contains} objects".to_string()
        })? {
            let entries = value
                .as_object()
                .ok_or_else(|| "expect_file_contains entries must be objects".to_string())?;
            for (key, _) in entries {
                if key != "path" && key != "contains" {
                    return Err(format!("unknown key `{key}` in expect_file_contains entry"));
                }
            }
            let path = value
                .get("path")
                .and_then(Json::as_str)
                .ok_or_else(|| "expect_file_contains entry requires a string `path`".to_string())?;
            let contains = value
                .get("contains")
                .and_then(Json::as_str)
                .ok_or_else(|| {
                    "expect_file_contains entry requires a string `contains`".to_string()
                })?;
            if !is_safe_relative_path(path) {
                return Err(format!(
                    "expect_file_contains path `{path}` must be a safe relative path"
                ));
            }
            files.push(ExpectedFile {
                path: path.to_string(),
                contains: Some(contains.to_string()),
            });
        }
    }
    Ok(files)
}

fn parse_fixtures(object: &Json) -> Result<Vec<Fixture>, String> {
    let Some(value) = object.get("fixtures") else {
        return Ok(Vec::new());
    };
    let list = value
        .as_array()
        .ok_or_else(|| "fixtures must be an array".to_string())?;
    if list.len() > 8 {
        return Err("fixtures is limited to 8 entries".to_string());
    }
    let mut fixtures = Vec::new();
    for entry in list {
        let fields = entry
            .as_object()
            .ok_or_else(|| "fixtures entries must be objects".to_string())?;
        for (key, _) in fields {
            if !matches!(key.as_str(), "kind" | "path" | "target") {
                return Err(format!("unknown key `{key}` in fixtures entry"));
            }
        }
        let kind_text = entry
            .get("kind")
            .and_then(Json::as_str)
            .ok_or_else(|| "fixtures entry requires a string `kind`".to_string())?;
        let kind = match kind_text {
            "symlink-dir" => FixtureKind::SymlinkDir,
            "symlink-file" => FixtureKind::SymlinkFile,
            other => {
                return Err(format!(
                    "unknown fixture kind `{other}`; expected symlink-dir or symlink-file"
                ))
            }
        };
        let path = entry
            .get("path")
            .and_then(Json::as_str)
            .ok_or_else(|| "fixtures entry requires a string `path`".to_string())?;
        if !is_safe_relative_path(path) {
            return Err(format!(
                "fixtures path `{path}` must be a safe relative path inside the case"
            ));
        }
        let target = entry
            .get("target")
            .and_then(Json::as_str)
            .ok_or_else(|| "fixtures entry requires a string `target`".to_string())?;
        if target.is_empty() || target.starts_with('/') || target.starts_with('\\') {
            return Err(format!(
                "fixtures target `{target}` must be a relative path"
            ));
        }
        if looks_absolute(target) {
            return Err(format!("fixtures target `{target}` must not be absolute"));
        }
        fixtures.push(Fixture {
            kind,
            path: path.to_string(),
            target: target.to_string(),
        });
    }
    Ok(fixtures)
}

/// A canonical diagnostic code: `E-SEGMENT[-SEGMENT...]` for an error or
/// `W-SEGMENT[-SEGMENT...]` for a warning.
///
/// DIAG-1.6 closes the warning list to `W-RES-UNUSED-IMPORT` and
/// `W-OWN-UNUSED-VARIABLE`; both are legitimate codes, so the harness must
/// accept `W-*` exactly like `E-*` (it used to reject them, which forced the
/// `W-*` cases to skip schema validation).
pub(crate) fn is_valid_code(code: &str) -> bool {
    let mut segments = code.split('-');
    if !matches!(segments.next(), Some("E") | Some("W")) {
        return false;
    }
    let rest: Vec<&str> = segments.collect();
    !rest.is_empty()
        && rest.iter().all(|segment| {
            !segment.is_empty()
                && segment
                    .chars()
                    .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit())
        })
}

fn require_string(object: &Json, key: &str) -> Result<String, String> {
    object
        .get(key)
        .and_then(Json::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("missing required string field `{key}`"))
}

fn optional_string(object: &Json, key: &str) -> Result<Option<String>, String> {
    match object.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_str()
            .map(|text| Some(text.to_string()))
            .ok_or_else(|| format!("field `{key}` must be a string")),
    }
}

fn string_array(object: &Json, key: &str) -> Result<Vec<String>, String> {
    let Some(value) = object.get(key) else {
        return Ok(Vec::new());
    };
    let items = value
        .as_array()
        .ok_or_else(|| format!("field `{key}` must be an array of strings"))?;
    let mut out = Vec::new();
    for item in items {
        out.push(
            item.as_str()
                .ok_or_else(|| format!("field `{key}` must contain only strings"))?
                .to_string(),
        );
    }
    Ok(out)
}

fn mark_duplicate_ids(entries: &mut [CaseEntry]) {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for entry in entries.iter() {
        match counts.iter_mut().find(|(id, _)| *id == entry.id) {
            Some((_, count)) => *count += 1,
            None => counts.push((entry.id.clone(), 1)),
        }
    }
    for entry in entries.iter_mut() {
        if let Some((_, count)) = counts.iter().find(|(id, _)| *id == entry.id) {
            if *count > 1 {
                let message = format!(
                    "duplicate case id `{}` ({count} cases share it); ids must be unique",
                    entry.id
                );
                entry.spec = Err(message);
            }
        }
    }
}

struct DirItem {
    name: String,
    path: PathBuf,
    is_dir: bool,
}

fn read_dir_sorted(dir: &Path) -> std::io::Result<Vec<DirItem>> {
    let mut items = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        items.push(DirItem {
            name: entry.file_name().to_string_lossy().into_owned(),
            path: entry.path(),
            is_dir: file_type.is_dir(),
        });
    }
    items.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<CaseSpec, String> {
        let json = Json::parse(text).unwrap();
        parse_spec(json.as_object().unwrap())
    }

    #[test]
    fn build_perimeter_covers_every_building_mode() {
        // The `check ⇒ build` contract applies to the modes that produce a
        // buildable program; `compile-fail` and `cli` are check-only by
        // construction and must never be counted as build witnesses.
        for mode in [
            Mode::CompilePass,
            Mode::RunPass,
            Mode::RunFail,
            Mode::FmtIdempotent,
        ] {
            assert!(mode.builds_program(), "{}", mode.as_str());
        }
        for mode in [Mode::CompileFail, Mode::Cli] {
            assert!(!mode.builds_program(), "{}", mode.as_str());
        }
    }

    #[test]
    fn minimal_case_defaults() {
        let spec = parse(r#"{"id":"lexer/empty","mode":"compile-pass"}"#).unwrap();
        assert_eq!(spec.mode, Mode::CompilePass);
        assert_eq!(spec.project, ".");
        assert!(spec.expect_codes.is_empty());
        assert_eq!(spec.exit_code, None);
        assert!(!spec.unsupported);
    }

    #[test]
    fn full_case_parses() {
        let spec = parse(
            r#"{
                "id": "run/calc",
                "mode": "run-pass",
                "project": "app",
                "args": ["5", "+", "3"],
                "stdin": "x",
                "stdout_contains": "8",
                "stderr_contains": "",
                "exit_code": 0,
                "skip_on": ["windows"],
                "notes": "n",
                "expect_files": ["out.txt"],
                "expect_file_contains": [{"path": "out.txt", "contains": "DONE"}],
                "expect_json_schema_version": 1,
                "timeout_ms": 5000
            }"#,
        )
        .unwrap();
        assert_eq!(spec.mode, Mode::RunPass);
        assert_eq!(spec.project, "app");
        assert_eq!(spec.args.len(), 3);
        assert_eq!(spec.expect_files.len(), 2);
        assert_eq!(spec.timeout_ms, Some(5000));
    }

    #[test]
    fn rejects_unknown_or_duplicate_fields() {
        assert!(
            parse(r#"{"id":"a","mode":"cli","args":["x"],"stdout_contain":"y"}"#)
                .unwrap_err()
                .contains("unknown field")
        );
        assert!(parse(r#"{"id":"a","id":"b","mode":"cli","args":["x"]}"#)
            .unwrap_err()
            .contains("duplicate key"));
    }

    #[test]
    fn rejects_bad_mode_code_and_project() {
        assert!(parse(r#"{"id":"a","mode":"nope"}"#).is_err());
        assert!(
            parse(r#"{"id":"a","mode":"compile-fail","expect_codes":["E-lex-x"]}"#)
                .unwrap_err()
                .contains("invalid diagnostic code")
        );
        assert!(parse(r#"{"id":"a","mode":"compile-pass","project":"../x"}"#).is_err());
        assert!(parse(r#"{"id":"a","mode":"compile-pass","project":"C:\\x"}"#).is_err());
    }

    #[test]
    fn enforces_mode_specific_exit_codes_and_fields() {
        assert!(parse(r#"{"id":"a","mode":"compile-pass","exit_code":1}"#).is_err());
        assert!(parse(r#"{"id":"a","mode":"compile-fail","exit_code":2}"#).is_err());
        assert!(parse(r#"{"id":"a","mode":"fmt-idempotent","exit_code":0}"#).is_err());
        assert!(parse(r#"{"id":"a","mode":"run-pass","expect_codes":["E-LEX-CHAR"]}"#).is_err());
        assert!(parse(r#"{"id":"a","mode":"cli"}"#).is_err());
        assert!(parse(r#"{"id":"a","mode":"cli","args":["C:\\outside"]}"#).is_err());
    }

    #[test]
    fn unsupported_marker_is_consistent() {
        let ok = parse(
            r#"{"id":"a","mode":"compile-fail","expect_codes":["E-UNSUPPORTED-TRAIT"],"unsupported":true}"#,
        );
        assert!(ok.is_ok());
        assert!(parse(
            r#"{"id":"a","mode":"compile-fail","expect_codes":["E-TYP-MISMATCH"],"unsupported":true}"#
        )
        .is_err());
        assert!(
            parse(r#"{"id":"a","mode":"compile-pass","expect_codes":[],"unsupported":true}"#)
                .is_err()
        );
    }

    #[test]
    fn fixtures_and_expected_files_are_validated() {
        assert!(parse(
            r#"{"id":"a","mode":"compile-fail","expect_codes":["E-MOD-SYMLINK"],"fixtures":[{"kind":"symlink-dir","path":"util","target":"real"}]}"#
        )
        .is_ok());
        assert!(parse(
            r#"{"id":"a","mode":"compile-fail","expect_codes":["E-MOD-SYMLINK"],"fixtures":[{"kind":"symlink-dir","path":"../util","target":"real"}]}"#
        )
        .is_err());
        assert!(parse(
            r#"{"id":"a","mode":"compile-fail","expect_codes":["E-MOD-SYMLINK"],"fixtures":[{"kind":"symlink-dir","path":"util","target":"C:\\x"}]}"#
        )
        .is_err());
        assert!(parse(r#"{"id":"a","mode":"compile-pass","expect_files":["../x"]}"#).is_err());
    }

    #[test]
    fn duplicate_ids_are_all_marked_invalid() {
        let mut entries = vec![
            CaseEntry {
                id: "x".into(),
                taxonomy: "lexer".into(),
                rel_dir: "lexer/a".into(),
                dir: PathBuf::from("a"),
                mode: Mode::CompilePass,
                spec: Ok(CaseSpec {
                    id: "x".into(),
                    mode: Mode::CompilePass,
                    project: ".".into(),
                    expect_codes: Vec::new(),
                    args: Vec::new(),
                    stdin: String::new(),
                    stdout_contains: None,
                    stderr_contains: None,
                    exit_code: None,
                    skip_on: Vec::new(),
                    unsupported: false,
                    notes: String::new(),
                    expect_files: Vec::new(),
                    expect_json_schema_version: None,
                    timeout_ms: None,
                    fixtures: Vec::new(),
                }),
            },
            CaseEntry {
                id: "x".into(),
                taxonomy: "lexer".into(),
                rel_dir: "lexer/b".into(),
                dir: PathBuf::from("b"),
                mode: Mode::CompilePass,
                spec: Ok(CaseSpec {
                    id: "x".into(),
                    mode: Mode::CompilePass,
                    project: ".".into(),
                    expect_codes: Vec::new(),
                    args: Vec::new(),
                    stdin: String::new(),
                    stdout_contains: None,
                    stderr_contains: None,
                    exit_code: None,
                    skip_on: Vec::new(),
                    unsupported: false,
                    notes: String::new(),
                    expect_files: Vec::new(),
                    expect_json_schema_version: None,
                    timeout_ms: None,
                    fixtures: Vec::new(),
                }),
            },
        ];
        mark_duplicate_ids(&mut entries);
        assert!(entries.iter().all(|entry| entry.spec.is_err()));
    }

    #[test]
    fn valid_codes() {
        for code in [
            "E-LEX-CHAR",
            "E-TYP-LITERAL-RANGE",
            "E-UNSUPPORTED-IDENT",
            "E-MOD-DEP-CONFLICT",
            // DIAG-1.6 closes the warning list on these two codes; they are as
            // canonical as an `E-*` code and must be accepted by the harness.
            "W-RES-UNUSED-IMPORT",
            "W-OWN-UNUSED-VARIABLE",
        ] {
            assert!(is_valid_code(code), "{code}");
        }
        for code in [
            "",
            "E",
            "E-",
            "E-lower",
            "W",
            "W-",
            "W-lower",
            "X-LEX-CHAR",
            "E--X",
            "E-LEX-CHAR-",
        ] {
            assert!(!is_valid_code(code), "{code}");
        }
    }
}
