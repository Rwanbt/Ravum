//! Package/module loader for Ravum Core 1.0.
//!
//! OWNER: worker `cli` (this file only; the syntax/sema workers own the rest of
//! `ravum-lang-sema`).
//!
//! Responsibilities:
//! * read and strictly validate `ravum.toml` with a small internal TOML subset
//!   (no external `toml` crate);
//! * resolve the local dependency graph, refuse cycles/symlinks/absolute paths,
//!   and compute a deterministic content digest per package;
//! * maintain `ravum.lock` (created on `check`/`build`, `--locked` refuses drift);
//! * load `src/main.rv` (binary crate) and/or `src/lib.rv` (library crate),
//!   resolve `mod foo;` declarations (`src/foo.rv` then `src/foo/mod.rv`),
//!   expand them into inline [`Module`] values so the checker never sees a
//!   [`ItemKind::ModDecl`], and register every file in a [`SourceMap`];
//! * merge direct/transitive dependency roots as public modules named after the
//!   dependency, so `use util::helper;` can be resolved by the checker.
//!
//! ## Path policy (documented, fail-closed)
//!
//! * Package directories and all opened files are canonicalized.
//! * A dependency `path` must be relative, at most
//!   [`MAX_DEPENDENCY_PATH_DEPTH`] components, and must resolve inside the
//!   **parent directory** of the package: `../util` is legitimate
//!   (`spec/language/modules.md` Q-MOD-2), `../../outside` is an escape.
//!   Absolute paths, Windows prefixes, too-deep paths and escapes are
//!   `E-MOD-PATH-ESCAPE`; a missing dependency is `E-MOD-DEPENDENCY`.
//! * No component of a dependency path or of a module path may be a symbolic
//!   link (`E-MOD-PATH`); symlinks are inspected with `symlink_metadata` so
//!   they are never followed.
//! * Reserved Windows path components (`CON`, `PRN`, `AUX`, `NUL`, `COM1-9`,
//!   `LPT1-9`, trailing dot/space, with or without extension) are refused even
//!   on filesystems that accept them (`E-MOD-RESERVED-NAME`).
//! * A package with both `src/main.rv` and `src/lib.rv` is refused
//!   (`E-MOD-BOTH-ROOTS`); Core 1.0 has no silent root selection and
//!   `main.rv` cannot import the package library.
//! * Module files must stay under the package `src/` directory
//!   (`E-MOD-PATH-ESCAPE`).
//! * There is no `[workspace]` root in Core 1.0, so the loader does not impose a
//!   workspace-directory bound; only the rules above are enforced.
//! * File reads are bounded by [`MAX_FILE_SIZE`]; a package may register at most
//!   [`MAX_SOURCE_FILES`] source files (the parser reserves the high byte of a
//!   `NodeId` for the `FileId`).
//! * Dependency packages are only ever *read*; the loader writes `ravum.lock`
//!   in the root package (and nothing else).

use ravum_lang_syntax::ast::{Ident, Item, ItemKind, Module, NodeId, Program, Vis};
use ravum_lang_syntax::diag::{Diagnostic, DiagnosticBag};
use ravum_lang_syntax::parser::parse_file;
use ravum_lang_syntax::source::SourceMap;
use ravum_lang_syntax::span::{FileId, Span};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

/// The only package edition accepted by this loader (fail-closed).
pub const MANIFEST_EDITION: &str = "core-1.0";
/// Version of the `ravum.lock` format.
pub const LOCKFILE_VERSION: u64 = 1;
/// Lockfile file name at the package root.
pub const LOCKFILE_NAME: &str = "ravum.lock";
/// Manifest file name at the package root.
pub const MANIFEST_NAME: &str = "ravum.toml";
/// Maximum size of any single file the loader will read (8 MiB).
pub const MAX_FILE_SIZE: u64 = 8 * 1024 * 1024;
/// Maximum number of source files registered in one [`SourceMap`]. `NodeId`
/// carries the `FileId` in its high 32 bits (see `syntax::parser`), so the
/// bound is 2^32 - 1 in the type; the policy limit is 65 536 (it admits a
/// package of 1 000 modules plus its manifest and dependency manifests).
pub const MAX_SOURCE_FILES: usize = 65_536;
/// Maximum dependency-graph depth (root = 0), matching the path-escape policy.
pub const MAX_PACKAGE_DEPTH: usize = 8;
/// Maximum number of path components accepted in a dependency `path`.
pub const MAX_DEPENDENCY_PATH_DEPTH: usize = 8;
/// Maximum number of `.rv` files considered for a package digest.
pub const MAX_PACKAGE_FILES: usize = 4096;
/// Maximum size of an existing `ravum.lock` read for comparison.
pub const MAX_LOCKFILE_SIZE: u64 = 1024 * 1024;

/// File id used for synthetic (`NodeId`, `Span`) values created by the loader
/// for dependency wrapper modules. Real files use ids below
/// [`MAX_SOURCE_FILES`] (65 536), so `u32::MAX - 1` cannot collide with a parsed
/// file. The previous marker was the high byte `0xFE` of a 32-bit `NodeId`,
/// which *can* collide once 254 files are allowed.
const SYNTHETIC_FILE: u32 = u32::MAX - 1;

/// The effective source-file limit. Production uses [`MAX_SOURCE_FILES`]; the
/// unit tests lower it (thread-locally) so the refusal path and its diagnostic
/// can be exercised without creating 65 536 files.
fn source_file_limit() -> usize {
    #[cfg(test)]
    {
        if let Some(limit) = TEST_SOURCE_FILE_LIMIT.with(std::cell::Cell::get) {
            return limit;
        }
    }
    MAX_SOURCE_FILES
}

#[cfg(test)]
thread_local! {
    static TEST_SOURCE_FILE_LIMIT: std::cell::Cell<Option<usize>> =
        const { std::cell::Cell::new(None) };
}

// ---------------------------------------------------------------------------
// Public model
// ---------------------------------------------------------------------------

/// One `[dependencies]` entry, as written in the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyDecl {
    pub name: String,
    /// Relative path exactly as written.
    pub path: String,
    pub span: Span,
}

/// Strict, validated `ravum.toml` content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageManifest {
    pub name: String,
    pub version: String,
    pub edition: String,
    /// Sorted by dependency name (deterministic).
    pub dependencies: Vec<DependencyDecl>,
}

/// A resolved local dependency (transitive entries included).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    pub name: String,
    pub version: String,
    /// `sha256:<hex>` of the package content (all `.rv` under `src/` + manifest).
    pub digest: String,
}

/// Which crate root was selected for `program`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrateKind {
    /// `src/main.rv`
    Bin,
    /// `src/lib.rv`
    Lib,
    /// A standalone `.rv` file (no package, no manifest, no lockfile).
    File,
}

impl CrateKind {
    pub fn name(self) -> &'static str {
        match self {
            CrateKind::Bin => "bin",
            CrateKind::Lib => "lib",
            CrateKind::File => "file",
        }
    }
}

/// Result of the `ravum.lock` handling for a package load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockfileState {
    /// The lockfile was absent and has been written.
    Created,
    /// The lockfile was present and matches the resolved graph.
    Current,
    /// The lockfile was stale; a warning was emitted and the file rewritten.
    Stale,
    /// `--locked` was requested and the lockfile is absent.
    Missing,
    /// The lockfile is unreadable or uses an unknown format version.
    Invalid,
    /// Not applicable (single-file mode or the graph failed to resolve).
    NotApplicable,
}

impl LockfileState {
    pub fn name(self) -> &'static str {
        match self {
            LockfileState::Created => "created",
            LockfileState::Current => "current",
            LockfileState::Stale => "stale",
            LockfileState::Missing => "missing",
            LockfileState::Invalid => "invalid",
            LockfileState::NotApplicable => "not_applicable",
        }
    }
}

/// Options for [`load_package_with_options`].
#[derive(Debug, Clone, Default)]
pub struct LoadOptions {
    /// Refuse to create or rewrite `ravum.lock`; a missing or different lockfile
    /// is an error (exit code 1 in the CLI).
    pub locked: bool,
    /// In-memory source overrides, keyed by **canonicalized** file path.
    ///
    /// A file present here is read from the map instead of the filesystem. This
    /// is what lets the language server analyse an editor buffer that is not
    /// saved yet (`textDocument/didChange`) without ever writing the user's
    /// file. An override never creates a file, never changes a digest of the
    /// package on disk, and is ignored for paths the loader does not load.
    pub overrides: BTreeMap<PathBuf, String>,
}

/// Per-session parse cache (language server).
///
/// Keyed by `(content hash, FileId)`: a parse result embeds the `FileId` in every
/// `NodeId` and every diagnostic span, so it may only be reused for the same
/// `FileId`. An unchanged file whose `FileId` did not move is not parsed again.
/// The cache is bounded — it clears itself at [`PARSE_CACHE_LIMIT`] entries (each
/// entry holds a full AST), so a long editor session cannot grow without bound.
/// `hits`/`misses` are exposed for tests and diagnostics.
#[derive(Debug, Default)]
pub struct ParseCache {
    entries: BTreeMap<(u64, FileId), (Program, Vec<Diagnostic>)>,
    hits: u64,
    misses: u64,
}

/// Maximum number of parsed files kept per session.
pub const PARSE_CACHE_LIMIT: usize = 512;

impl ParseCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn hits(&self) -> u64 {
        self.hits
    }

    pub fn misses(&self) -> u64 {
        self.misses
    }

    fn get(&mut self, key: &(u64, FileId)) -> Option<(Program, Vec<Diagnostic>)> {
        match self.entries.get(key) {
            Some((program, diags)) => {
                self.hits += 1;
                Some((program.clone(), diags.clone()))
            }
            None => {
                self.misses += 1;
                None
            }
        }
    }

    fn insert(&mut self, key: (u64, FileId), program: Program, diags: Vec<Diagnostic>) {
        if self.entries.len() >= PARSE_CACHE_LIMIT {
            self.entries.clear();
        }
        self.entries.insert(key, (program, diags));
    }
}

/// FNV-1a 64 over the file text: deterministic, dependency-free cache key.
fn content_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Everything the CLI needs after a successful (or failed) load.
#[derive(Debug)]
pub struct LoadedPackage {
    /// Primary crate (`main.rv` when it exists, otherwise `lib.rv`). `None` when
    /// a loader, syntax or I/O error makes the program unusable.
    pub program: Option<Program>,
    /// Always `None`: Core 1.0 refuses a package with both `src/main.rv` and
    /// `src/lib.rv` (`E-MOD-BOTH-ROOTS`). Kept so the CLI/driver interface can
    /// evolve without another breaking change.
    pub lib_program: Option<Program>,
    pub sources: SourceMap,
    pub diags: DiagnosticBag,
    /// Canonical package directory (or the parent directory in single-file mode).
    pub root_dir: String,
    pub package_name: Option<String>,
    pub package_version: Option<String>,
    pub edition: Option<String>,
    pub crate_kind: Option<CrateKind>,
    /// Direct and transitive dependencies, sorted by name.
    pub dependencies: Vec<Dependency>,
    /// Aggregate `sha256:<hex>` of the resolved dependencies (empty digest when
    /// there is no dependency).
    pub dependencies_digest: String,
    pub lockfile_state: LockfileState,
    /// Wall-clock time spent in the parser while loading this package (all
    /// files, dependencies included). Additive measurement for `--timings`;
    /// zero when the package was not parsed.
    pub parse_time: Duration,
    /// Source files registered in `sources`, in load order (display paths).
    pub loaded_files: Vec<String>,
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// Load a package from a directory, or a standalone `.rv` file.
///
/// This is the frozen interface used by the CLI; [`load_package_with_options`]
/// adds `--locked` support.
pub fn load_package(path: &Path) -> LoadedPackage {
    load_package_with_options(path, &LoadOptions::default())
}

/// Load a package with explicit options (currently `--locked`).
pub fn load_package_with_options(path: &Path, options: &LoadOptions) -> LoadedPackage {
    let mut cache = ParseCache::default();
    load_package_with_parse_cache(path, options, &mut cache)
}

/// Load a package, reusing the parses stored in `cache` for unchanged files.
pub fn load_package_with_parse_cache(
    path: &Path,
    options: &LoadOptions,
    cache: &mut ParseCache,
) -> LoadedPackage {
    match fs::metadata(path) {
        Ok(meta) if meta.is_file() => load_single_file(path, options, cache),
        Ok(meta) if meta.is_dir() => load_package_dir(path, options, cache),
        Ok(_) => {
            let mut out = LoadedPackage::empty(path);
            err(
                &mut out.diags,
                "E-MOD-MANIFEST",
                Span::dummy(),
                format!("`{}` is neither a file nor a directory", path.display()),
            );
            out
        }
        Err(e) => {
            let mut out = LoadedPackage::empty(path);
            err(
                &mut out.diags,
                "E-IO",
                Span::dummy(),
                format!("cannot access `{}`: {e}", path.display()),
            );
            out
        }
    }
}

impl LoadedPackage {
    fn empty(path: &Path) -> Self {
        Self {
            program: None,
            lib_program: None,
            sources: SourceMap::new(),
            diags: DiagnosticBag::new(),
            root_dir: path.display().to_string(),
            package_name: None,
            package_version: None,
            edition: None,
            crate_kind: None,
            dependencies: Vec::new(),
            dependencies_digest: empty_dependencies_digest(),
            lockfile_state: LockfileState::NotApplicable,
            parse_time: Duration::ZERO,
            loaded_files: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Single-file mode
// ---------------------------------------------------------------------------

fn load_single_file(path: &Path, options: &LoadOptions, cache: &mut ParseCache) -> LoadedPackage {
    let mut out = LoadedPackage::empty(path);
    let canonical = match fs::canonicalize(path) {
        Ok(p) => p,
        Err(e) => {
            err(
                &mut out.diags,
                "E-IO",
                Span::dummy(),
                format!("cannot canonicalize `{}`: {e}", path.display()),
            );
            return out;
        }
    };
    let parent = canonical
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    out.root_dir = parent.display().to_string();
    out.package_name = canonical
        .file_stem()
        .map(|s| s.to_string_lossy().to_string());
    out.crate_kind = Some(CrateKind::File);

    if canonical.extension().and_then(|e| e.to_str()) != Some("rv") {
        err(
            &mut out.diags,
            "E-IO",
            Span::dummy(),
            format!(
                "`{}` is not a `.rv` file; pass a package directory instead",
                path.display()
            ),
        );
        return out;
    }

    let mut sources = SourceMap::new();
    let mut diags = DiagnosticBag::new();
    let mut file_ids: BTreeMap<PathBuf, FileId> = BTreeMap::new();
    let mut synthetic: u64 = ((SYNTHETIC_FILE as u64) << 32) | 1;
    let mut parse_time = Duration::ZERO;

    let program = {
        let mut ctx = CrateCtx {
            src_root: parent.clone(),
            display_base: parent.clone(),
            display_prefix: String::new(),
            sources: &mut sources,
            diags: &mut diags,
            file_ids: &mut file_ids,
            loaded: BTreeSet::new(),
            synthetic: &mut synthetic,
            parse_time: &mut parse_time,
            parse_cache: cache,
            overrides: &options.overrides,
        };
        load_crate_root(&mut ctx, &canonical).map(|module| Program {
            file: ctx.file_ids.get(&canonical).copied().unwrap_or(0),
            root: module,
        })
    };

    out.loaded_files = display_files(&sources);
    out.parse_time = parse_time;
    if !diags.has_errors() {
        out.program = program;
    }
    out.sources = sources;
    out.diags = diags;
    out
}

// ---------------------------------------------------------------------------
// Package mode
// ---------------------------------------------------------------------------

fn load_package_dir(input: &Path, options: &LoadOptions, cache: &mut ParseCache) -> LoadedPackage {
    let mut out = LoadedPackage::empty(input);
    let root = match fs::canonicalize(input) {
        Ok(p) => p,
        Err(e) => {
            err(
                &mut out.diags,
                "E-IO",
                Span::dummy(),
                format!("cannot canonicalize `{}`: {e}", input.display()),
            );
            return out;
        }
    };
    out.root_dir = root.display().to_string();

    let mut sources = SourceMap::new();
    let mut diags = DiagnosticBag::new();

    // 1. Manifest (registered first so its diagnostics carry a real location).
    let manifest_path = root.join(MANIFEST_NAME);
    if !manifest_path.is_file() {
        // Convenience required by `check --json` on a lone script: a directory
        // with exactly one top-level `.rv` file and no manifest is treated as
        // single-file mode. More than one is ambiguous and stays an error.
        match top_level_rv_files(&root) {
            Ok(mut files) if files.len() == 1 => {
                return load_single_file(&files.remove(0), options, cache);
            }
            Ok(files) if files.len() > 1 => {
                err(
                    &mut diags,
                    "E-MOD-MANIFEST",
                    Span::dummy(),
                    format!(
                        "no `{MANIFEST_NAME}` in `{}` and {} top-level `.rv` files; pass one \
                         file explicitly or run `ravum-lang init`",
                        root.display(),
                        files.len()
                    ),
                );
                let mut out = LoadedPackage::empty(input);
                out.diags = diags;
                return out;
            }
            _ => {}
        }
    }
    let manifest_text = match read_text_limited(
        &manifest_path,
        MAX_FILE_SIZE,
        "E-MOD-MANIFEST",
        &mut diags,
    ) {
        Some(t) => t,
        None => {
            if !diags.has_errors() {
                err(
                    &mut diags,
                    "E-MOD-MANIFEST",
                    Span::dummy(),
                    format!(
                        "no `{MANIFEST_NAME}` in `{}`; run `ravum-lang init` or pass a single `.rv` file",
                        root.display()
                    ),
                );
            }
            out.diags = diags;
            return out;
        }
    };
    let manifest_id = sources.add(MANIFEST_NAME, manifest_text.clone());
    let (manifest, manifest_diags) = parse_manifest(&manifest_text, manifest_id);
    diags.extend(manifest_diags);
    let Some(manifest) = manifest else {
        out.sources = sources;
        out.diags = diags;
        return out;
    };

    out.package_name = Some(manifest.name.clone());
    out.package_version = Some(manifest.version.clone());
    out.edition = Some(manifest.edition.clone());

    // 2. Dependency graph + digests.
    let mut graph = DepGraph::new(&root, manifest.clone());
    let mut stack: Vec<(String, PathBuf)> = vec![(manifest.name.clone(), root.clone())];
    collect_dependencies(
        &root,
        &manifest,
        0,
        &mut stack,
        &mut graph,
        &mut sources,
        &mut diags,
    );

    if !graph.failed {
        compute_digests(&mut graph, &mut diags);
    }

    let mut state = LockfileState::NotApplicable;
    if !graph.failed && !diags.has_errors() {
        state = update_lockfile(&root, &graph, options.locked, &mut diags);
    }

    if state != LockfileState::NotApplicable {
        out.lockfile_state = state;
        out.dependencies = graph
            .packages
            .iter()
            .skip(1)
            .map(|p| Dependency {
                name: p.manifest.name.clone(),
                version: p.manifest.version.clone(),
                digest: p.digest.clone(),
            })
            .collect();
        out.dependencies
            .sort_by(|a, b| a.name.cmp(&b.name).then(a.version.cmp(&b.version)));
        out.dependencies_digest = aggregate_dependencies_digest(&out.dependencies);
    }

    // 3. Crate roots. Errors above make the program unusable; skip parsing then.
    let mut parse_time = Duration::ZERO;
    if !diags.has_errors() {
        let (program, lib_program, kind) = load_crates(
            &root,
            &graph,
            &mut sources,
            &mut diags,
            &mut parse_time,
            cache,
            options,
        );
        // Module/parse errors are discovered while loading: never hand a
        // partially loaded program to the checker.
        if !diags.has_errors() {
            out.program = program;
        }
        out.lib_program = lib_program;
        out.crate_kind = kind;
    }

    out.loaded_files = display_files(&sources);
    out.parse_time = parse_time;
    out.sources = sources;
    out.diags = diags;
    out
}

// ---------------------------------------------------------------------------
// Manifest parsing: minimal TOML subset
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum TomlValue {
    Str(String),
    Table(Vec<(String, TomlValue)>),
}

#[derive(Debug)]
struct TomlSection {
    span: Span,
    keys: BTreeMap<String, (TomlValue, Span)>,
}

impl Default for TomlSection {
    fn default() -> Self {
        Self {
            span: Span::dummy(),
            keys: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Default)]
struct ParsedToml {
    sections: BTreeMap<String, TomlSection>,
}

/// Parse the strict TOML subset of `ravum.toml`:
/// `[section]` headers, `key = "string"` pairs and one-line
/// `key = { inner = "value", ... }` inline tables. Comments (`#`), blank lines
/// and single-line basic strings with the usual escapes are supported. Arrays,
/// multiline strings/tables, numbers and booleans are rejected.
fn parse_toml(text: &str, file: FileId) -> (ParsedToml, DiagnosticBag) {
    let mut diags = DiagnosticBag::new();
    let mut parsed = ParsedToml::default();
    let mut current = String::new();
    let mut seen_sections: BTreeSet<String> = BTreeSet::new();
    let mut offset = 0u32;
    let bytes = text.as_bytes();
    let mut line_no = 0u32;

    for raw_line in text.split_inclusive('\n') {
        line_no += 1;
        let line_start = offset;
        offset = offset.saturating_add(raw_line.len() as u32);
        let line = raw_line.trim_end_matches(['\n', '\r']);
        let body = strip_comment(line);
        let trimmed = body.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lead = body.len() - body.trim_start().len();
        let start = line_start.saturating_add(lead as u32);
        let end = start.saturating_add(trimmed.len() as u32);
        let line_span = Span::new(
            file,
            start.min(bytes.len() as u32),
            end.min(bytes.len() as u32),
        );

        if trimmed.starts_with('[') {
            if !trimmed.ends_with(']') || trimmed.len() < 2 {
                diags.push(Diagnostic::error(
                    "E-MOD-MANIFEST",
                    line_span,
                    format!("invalid section header on line {line_no}: `{trimmed}`"),
                ));
                continue;
            }
            let name = trimmed[1..trimmed.len() - 1].trim().to_string();
            if name.is_empty() || !name.chars().all(is_toml_bare_key_char) {
                diags.push(Diagnostic::error(
                    "E-MOD-MANIFEST",
                    line_span,
                    format!("invalid section name `{name}` on line {line_no}"),
                ));
                continue;
            }
            if !seen_sections.insert(name.clone()) {
                diags.push(Diagnostic::error(
                    "E-MOD-MANIFEST",
                    line_span,
                    format!("duplicate section `[{name}]` on line {line_no}"),
                ));
                continue;
            }
            parsed.sections.entry(name.clone()).or_default().span = line_span;
            current = name;
            continue;
        }

        let Some((key, value_text)) = split_key_value(trimmed) else {
            diags.push(Diagnostic::error(
                "E-MOD-MANIFEST",
                line_span,
                format!("expected `key = value` on line {line_no}"),
            ));
            continue;
        };
        let key = key.trim().to_string();
        if key.is_empty() || !key.chars().all(is_toml_bare_key_char) {
            diags.push(Diagnostic::error(
                "E-MOD-MANIFEST",
                line_span,
                format!("invalid key `{key}` on line {line_no}"),
            ));
            continue;
        }
        let value_text = value_text.trim();
        let value_start = start + (trimmed.len() - value_text.len()) as u32;
        let value_span = Span::new(
            file,
            value_start.min(bytes.len() as u32),
            (value_start as usize + value_text.len()).min(bytes.len()) as u32,
        );
        let Some(value) = parse_toml_value(value_text, file, value_start, &mut diags) else {
            continue;
        };
        let section = parsed.sections.entry(current.clone()).or_default();
        if section.keys.contains_key(&key) {
            diags.push(Diagnostic::error(
                "E-MOD-MANIFEST",
                line_span,
                format!("duplicate key `{key}` in section `[{current}]`"),
            ));
            continue;
        }
        section.keys.insert(key, (value, value_span));
    }

    (parsed, diags)
}

fn is_toml_bare_key_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.'
}

fn strip_comment(line: &str) -> &str {
    let mut in_string = false;
    let mut escaped = false;
    for (i, ch) in line.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
        } else if ch == '"' {
            in_string = true;
        } else if ch == '#' {
            return &line[..i];
        }
    }
    line
}

fn split_key_value(s: &str) -> Option<(&str, &str)> {
    let mut in_string = false;
    let mut escaped = false;
    for (i, ch) in s.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
        } else if ch == '"' {
            in_string = true;
        } else if ch == '=' {
            return Some((&s[..i], &s[i + 1..]));
        }
    }
    None
}

fn parse_toml_value(
    s: &str,
    file: FileId,
    base: u32,
    diags: &mut DiagnosticBag,
) -> Option<TomlValue> {
    if s.starts_with('"') {
        let (value, _) = parse_basic_string(s, file, base, diags)?;
        return Some(TomlValue::Str(value));
    }
    if s.starts_with('{') {
        if !s.ends_with('}') || s.len() < 2 {
            diags.push(Diagnostic::error(
                "E-MOD-MANIFEST",
                Span::new(file, base, base + s.len() as u32),
                "inline table must be closed with `}` on the same line",
            ));
            return None;
        }
        let inner = &s[1..s.len() - 1];
        let mut entries = Vec::new();
        if !inner.trim().is_empty() {
            for (idx, part) in split_top_level_commas(inner).into_iter().enumerate() {
                let part_start =
                    base + 1 + inner.find(part).unwrap_or(0) as u32 + if idx == 0 { 0 } else { 0 };
                let trimmed = part.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let Some((k, v)) = split_key_value(trimmed) else {
                    diags.push(Diagnostic::error(
                        "E-MOD-MANIFEST",
                        Span::new(file, part_start, part_start + part.len() as u32),
                        format!("invalid inline table entry `{trimmed}`"),
                    ));
                    return None;
                };
                let k = k.trim();
                let v = v.trim();
                if k.is_empty() || !k.chars().all(is_toml_bare_key_char) || !v.starts_with('"') {
                    diags.push(Diagnostic::error(
                        "E-MOD-MANIFEST",
                        Span::new(file, part_start, part_start + part.len() as u32),
                        format!("inline table entry `{trimmed}` must be `key = \"value\"`"),
                    ));
                    return None;
                }
                // `base` is only used for diagnostics; derive the value offset
                // from the original slice.
                let value_offset =
                    base + 1 + inner.find(part).unwrap_or(0) as u32 + (part.len() - v.len()) as u32;
                let (value, _) = parse_basic_string(v, file, value_offset, diags)?;
                if entries
                    .iter()
                    .any(|(existing, _): &(String, TomlValue)| existing == k)
                {
                    diags.push(Diagnostic::error(
                        "E-MOD-MANIFEST",
                        Span::new(file, part_start, part_start + part.len() as u32),
                        format!("duplicate inline table key `{k}`"),
                    ));
                    return None;
                }
                entries.push((k.to_string(), TomlValue::Str(value)));
            }
        }
        return Some(TomlValue::Table(entries));
    }
    diags.push(Diagnostic::error(
        "E-MOD-MANIFEST",
        Span::new(file, base, base + s.len() as u32),
        format!("unsupported TOML value `{s}`; only strings and inline tables are accepted"),
    ));
    None
}

fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    let mut start = 0usize;
    for (i, ch) in s.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
        } else if ch == '"' {
            in_string = true;
        } else if ch == ',' {
            out.push(&s[start..i]);
            start = i + 1;
        }
    }
    out.push(&s[start..]);
    out
}

/// Parse a single-line TOML basic string starting at `s[0] == '"'`.
fn parse_basic_string(
    s: &str,
    file: FileId,
    base: u32,
    diags: &mut DiagnosticBag,
) -> Option<(String, usize)> {
    debug_assert!(s.starts_with('"'));
    let mut value = String::new();
    let mut chars = s.char_indices().skip(1);
    while let Some((i, ch)) = chars.next() {
        match ch {
            '"' => return Some((value, i + 1)),
            '\\' => {
                let Some((_, esc)) = chars.next() else {
                    break;
                };
                match esc {
                    '"' => value.push('"'),
                    '\\' => value.push('\\'),
                    'n' => value.push('\n'),
                    't' => value.push('\t'),
                    'r' => value.push('\r'),
                    other => {
                        diags.push(Diagnostic::error(
                            "E-MOD-MANIFEST",
                            Span::new(file, base, base + s.len() as u32),
                            format!("unsupported escape sequence `\\{other}` in string"),
                        ));
                        return None;
                    }
                }
            }
            c if c.is_control() => {
                diags.push(Diagnostic::error(
                    "E-MOD-MANIFEST",
                    Span::new(file, base, base + s.len() as u32),
                    "control character in string",
                ));
                return None;
            }
            other => value.push(other),
        }
    }
    diags.push(Diagnostic::error(
        "E-MOD-MANIFEST",
        Span::new(file, base, base + s.len() as u32),
        "unterminated string",
    ));
    None
}

/// Validate a parsed manifest against `spec/language/modules.md` §5 and §8.
fn parse_manifest(text: &str, file: FileId) -> (Option<PackageManifest>, DiagnosticBag) {
    let (parsed, mut diags) = parse_toml(text, file);
    if diags.has_errors() {
        return (None, diags);
    }

    for (name, section) in &parsed.sections {
        match name.as_str() {
            "package" | "dependencies" => {}
            "build" | "scripts" => {
                diags.push(Diagnostic::error(
                    "E-MOD-BUILD-SCRIPT",
                    section.span,
                    format!(
                        "section `[{name}]` is not supported: Core 1.0 has no build scripts \
                         (spec/language/modules.md MOD-5.10)"
                    ),
                ));
            }
            other if other.starts_with("dependencies.") => {
                diags.push(Diagnostic::error(
                    "E-MOD-DEPENDENCY",
                    section.span,
                    format!(
                        "section `[{other}]` is not supported: declare dependencies as \
                         `name = {{ path = \"...\" }}` under `[dependencies]`"
                    ),
                ));
            }
            _ => {
                diags.push(Diagnostic::error(
                    "E-MOD-MANIFEST",
                    section.span,
                    format!("unknown section `[{name}]` (manifest is strict)"),
                ));
            }
        }
    }

    let package = parsed.sections.get("package");
    if package.is_none() {
        diags.push(Diagnostic::error(
            "E-MOD-MANIFEST",
            Span::new(file, 0, 0),
            "missing required `[package]` section",
        ));
        return (None, diags);
    }
    let package = package.expect("checked above");

    let mut name: Option<String> = None;
    let mut version: Option<String> = None;
    let mut edition: Option<String> = None;
    for (key, (value, span)) in &package.keys {
        match key.as_str() {
            "name" | "version" | "edition" => {
                let TomlValue::Str(text) = value else {
                    diags.push(Diagnostic::error(
                        "E-MOD-MANIFEST",
                        *span,
                        format!("`package.{key}` must be a string"),
                    ));
                    continue;
                };
                match key.as_str() {
                    "name" => name = Some(text.clone()),
                    "version" => version = Some(text.clone()),
                    "edition" => edition = Some(text.clone()),
                    _ => unreachable!(),
                }
            }
            other => diags.push(Diagnostic::error(
                "E-MOD-MANIFEST",
                *span,
                format!("unknown key `package.{other}` (Core 1.0 manifest is strict)"),
            )),
        }
    }

    let Some(name) = name else {
        diags.push(Diagnostic::error(
            "E-MOD-MANIFEST",
            package.span,
            "missing required `package.name`",
        ));
        return (None, diags);
    };
    if !is_valid_package_name(&name) {
        diags.push(Diagnostic::error(
            "E-MOD-MANIFEST",
            package.span,
            format!("invalid package name `{name}`: expected `[a-z][a-z0-9_-]*`"),
        ));
    }
    let Some(version) = version else {
        diags.push(Diagnostic::error(
            "E-MOD-MANIFEST",
            package.span,
            "missing required `package.version`",
        ));
        return (None, diags);
    };
    if !is_valid_version(&version) {
        diags.push(Diagnostic::error(
            "E-MOD-MANIFEST",
            package.span,
            format!(
                "invalid version `{version}`: expected `MAJOR.MINOR.PATCH` (decimal integers, \
                 no leading zeros, no pre-release)"
            ),
        ));
    }
    let Some(edition) = edition else {
        diags.push(Diagnostic::error(
            "E-MOD-MANIFEST",
            package.span,
            "missing required `package.edition`",
        ));
        return (None, diags);
    };
    if edition != MANIFEST_EDITION {
        diags.push(Diagnostic::error(
            "E-UNSUPPORTED-EDITION",
            package.span,
            format!(
                "unsupported edition `{edition}`: this toolchain accepts only \
                 `edition = \"{MANIFEST_EDITION}\"`"
            ),
        ));
    }

    let mut dependencies = Vec::new();
    if let Some(section) = parsed.sections.get("dependencies") {
        for (dep_name, (value, span)) in &section.keys {
            if !is_valid_package_name(dep_name) {
                diags.push(Diagnostic::error(
                    "E-MOD-DEPENDENCY",
                    *span,
                    format!("invalid dependency name `{dep_name}`: expected `[a-z][a-z0-9_-]*`"),
                ));
                continue;
            }
            let TomlValue::Table(entries) = value else {
                diags.push(Diagnostic::error(
                    "E-MOD-DEPENDENCY",
                    *span,
                    format!(
                        "dependency `{dep_name}` must be `{dep_name} = {{ path = \"...\" }}` \
                         (no registry, no version range, no build script)"
                    ),
                ));
                continue;
            };
            if entries.len() != 1 || entries[0].0 != "path" {
                diags.push(Diagnostic::error(
                    "E-MOD-DEPENDENCY",
                    *span,
                    format!(
                        "dependency `{dep_name}` must contain exactly one `path` key \
                         (local path dependencies only)"
                    ),
                ));
                continue;
            }
            let TomlValue::Str(path) = &entries[0].1 else {
                diags.push(Diagnostic::error(
                    "E-MOD-DEPENDENCY",
                    *span,
                    format!("dependency `{dep_name}`: `path` must be a string"),
                ));
                continue;
            };
            dependencies.push(DependencyDecl {
                name: dep_name.clone(),
                path: path.clone(),
                span: *span,
            });
        }
    }
    dependencies.sort_by(|a, b| a.name.cmp(&b.name));

    if diags.has_errors() {
        return (None, diags);
    }
    (
        Some(PackageManifest {
            name,
            version,
            edition,
            dependencies,
        }),
        diags,
    )
}

/// `[a-z][a-z0-9_-]*`
pub fn is_valid_package_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// `MAJOR.MINOR.PATCH`, decimal integers without leading zeros, no pre-release.
pub fn is_valid_version(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 3 {
        return false;
    }
    parts.iter().all(|part| {
        !part.is_empty()
            && part.bytes().all(|b| b.is_ascii_digit())
            && (part.len() == 1 || !part.starts_with('0'))
            && part.parse::<u64>().is_ok()
    })
}

fn is_module_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Windows-reserved path component: `CON`, `PRN`, `AUX`, `NUL`, `COM1..COM9`,
/// `LPT1..LPT9` with or without an extension, and any component with a trailing
/// dot or space (Win32 strips them, which breaks deterministic resolution).
/// Refused even on filesystems that accept them (Core 1.0 reproducibility).
pub fn is_reserved_windows_component(component: &str) -> bool {
    let trimmed = component.trim_end_matches(['.', ' ']);
    if trimmed.is_empty() || trimmed.len() != component.len() {
        // Empty component or trailing dot/space.
        return true;
    }
    let base = trimmed.split('.').next().unwrap_or("").to_ascii_uppercase();
    if matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    let bytes = base.as_bytes();
    bytes.len() == 4
        && (base.starts_with("COM") || base.starts_with("LPT"))
        && bytes[3].is_ascii_digit()
        && bytes[3] != b'0'
}

// ---------------------------------------------------------------------------
// Dependency graph
// ---------------------------------------------------------------------------

struct GraphPackage {
    dir: PathBuf,
    manifest: PackageManifest,
    digest: String,
}

struct DepGraph {
    packages: Vec<GraphPackage>,
    by_dir: BTreeMap<PathBuf, usize>,
    by_name: BTreeMap<String, usize>,
    failed: bool,
}

impl DepGraph {
    fn new(root: &Path, manifest: PackageManifest) -> Self {
        let mut graph = Self {
            packages: Vec::new(),
            by_dir: BTreeMap::new(),
            by_name: BTreeMap::new(),
            failed: false,
        };
        graph.by_dir.insert(root.to_path_buf(), 0);
        graph.by_name.insert(manifest.name.clone(), 0);
        graph.packages.push(GraphPackage {
            dir: root.to_path_buf(),
            manifest,
            digest: String::new(),
        });
        graph
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_dependencies(
    dir: &Path,
    manifest: &PackageManifest,
    depth: usize,
    stack: &mut Vec<(String, PathBuf)>,
    graph: &mut DepGraph,
    sources: &mut SourceMap,
    diags: &mut DiagnosticBag,
) {
    for dep in &manifest.dependencies {
        let Some(dep_dir) = resolve_dependency_dir(dir, dep, diags) else {
            graph.failed = true;
            continue;
        };
        if dep_dir == dir || stack.iter().any(|(_, p)| *p == dep_dir) {
            let position = stack.iter().position(|(_, p)| *p == dep_dir).unwrap_or(0);
            let mut chain: Vec<String> = stack[position..].iter().map(|(n, _)| n.clone()).collect();
            chain.push(dep.name.clone());
            diags.push(Diagnostic::error(
                "E-MOD-CYCLE",
                dep.span,
                format!("dependency cycle: {}", chain.join(" -> ")),
            ));
            graph.failed = true;
            continue;
        }
        if let Some(&index) = graph.by_dir.get(&dep_dir) {
            let known = &graph.packages[index].manifest;
            if known.name != dep.name {
                diags.push(Diagnostic::error(
                    "E-MOD-DEPENDENCY",
                    dep.span,
                    format!(
                        "dependency key `{}` resolves to package `{}`; the key must equal the \
                         target `package.name`",
                        dep.name, known.name
                    ),
                ));
                graph.failed = true;
            }
            continue;
        }
        if depth + 1 > MAX_PACKAGE_DEPTH {
            diags.push(Diagnostic::error(
                "E-MOD-PATH-ESCAPE",
                dep.span,
                format!(
                    "dependency graph deeper than {MAX_PACKAGE_DEPTH} packages; \
                     check for an accidental cycle or a generated tree"
                ),
            ));
            graph.failed = true;
            continue;
        }

        let target_manifest_path = dep_dir.join(MANIFEST_NAME);
        let Some(text) = read_text_limited(
            &target_manifest_path,
            MAX_FILE_SIZE,
            "E-MOD-MANIFEST",
            diags,
        ) else {
            graph.failed = true;
            continue;
        };
        let display = format!("{}/{}", dep.name, MANIFEST_NAME);
        let file_id = sources.add(display, text.clone());
        let (target, target_diags) = parse_manifest(&text, file_id);
        diags.extend(target_diags);
        let Some(target) = target else {
            graph.failed = true;
            continue;
        };
        if target.name != dep.name {
            diags.push(Diagnostic::error(
                "E-MOD-DEPENDENCY",
                dep.span,
                format!(
                    "dependency key `{}` does not match the target `package.name` (`{}`)",
                    dep.name, target.name
                ),
            ));
            graph.failed = true;
            continue;
        }
        if !is_module_identifier(&target.name) {
            diags.push(Diagnostic::error(
                "E-MOD-DEPENDENCY",
                dep.span,
                format!(
                    "dependency name `{}` is not usable as a module path in Core 1.0 \
                     (ASCII identifier required; `-` is not a module name character)",
                    target.name
                ),
            ));
            graph.failed = true;
            continue;
        }
        if is_reserved_windows_component(&target.name) {
            diags.push(Diagnostic::error(
                "E-MOD-RESERVED-NAME",
                dep.span,
                format!(
                    "dependency name `{}` is a reserved Windows path component; package names \
                     used as module names must be portable",
                    target.name
                ),
            ));
            graph.failed = true;
            continue;
        }
        match symlink_metadata_optional(&dep_dir.join("src").join("lib.rv")) {
            Ok(None) => {
                diags.push(Diagnostic::error(
                    "E-MOD-DEPENDENCY",
                    dep.span,
                    format!(
                        "dependency `{}` has no `src/lib.rv`; a binary package is not importable",
                        target.name
                    ),
                ));
                graph.failed = true;
                continue;
            }
            Ok(Some(meta)) if meta.file_type().is_symlink() => {
                diags.push(Diagnostic::error(
                    "E-MOD-PATH",
                    dep.span,
                    format!("dependency `{}` has a symlinked `src/lib.rv`", target.name),
                ));
                graph.failed = true;
                continue;
            }
            Ok(Some(meta)) if !meta.is_file() => {
                diags.push(Diagnostic::error(
                    "E-MOD-DEPENDENCY",
                    dep.span,
                    format!(
                        "dependency `{}`: `src/lib.rv` is not a regular file",
                        target.name
                    ),
                ));
                graph.failed = true;
                continue;
            }
            Ok(Some(_)) => {}
            Err(e) => {
                diags.push(Diagnostic::error(
                    "E-IO",
                    dep.span,
                    format!("cannot inspect dependency `{}`: {e}", target.name),
                ));
                graph.failed = true;
                continue;
            }
        }
        if graph.by_name.contains_key(&target.name) {
            diags.push(Diagnostic::error(
                "E-MOD-DUPLICATE",
                dep.span,
                format!(
                    "two different packages are named `{}`; Core 1.0 has no multi-version or \
                     alias resolution",
                    target.name
                ),
            ));
            graph.failed = true;
            continue;
        }

        let index = graph.packages.len();
        graph.packages.push(GraphPackage {
            dir: dep_dir.clone(),
            manifest: target.clone(),
            digest: String::new(),
        });
        graph.by_dir.insert(dep_dir.clone(), index);
        graph.by_name.insert(target.name.clone(), index);
        stack.push((target.name.clone(), dep_dir.clone()));
        collect_dependencies(&dep_dir, &target, depth + 1, stack, graph, sources, diags);
        stack.pop();
    }
}

/// Resolve a dependency `path` relative to `pkg_dir`, refusing absolute paths,
/// Windows prefixes and symbolic links. `..` is allowed (local path deps).
fn resolve_dependency_dir(
    pkg_dir: &Path,
    dep: &DependencyDecl,
    diags: &mut DiagnosticBag,
) -> Option<PathBuf> {
    let raw = dep.path.trim();
    if raw.is_empty() {
        diags.push(Diagnostic::error(
            "E-MOD-DEPENDENCY",
            dep.span,
            format!("dependency `{}` has an empty path", dep.name),
        ));
        return None;
    }
    if raw.contains('\0') {
        diags.push(Diagnostic::error(
            "E-MOD-PATH-ESCAPE",
            dep.span,
            format!("dependency `{}` path contains a NUL byte", dep.name),
        ));
        return None;
    }
    let path = Path::new(raw);
    if path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, Component::Prefix(_) | Component::RootDir))
    {
        diags.push(Diagnostic::error(
            "E-MOD-PATH-ESCAPE",
            dep.span,
            format!(
                "dependency `{}` path `{raw}` is absolute; only package-relative local paths \
                 are accepted",
                dep.name
            ),
        ));
        return None;
    }
    let depth = path
        .components()
        .filter(|component| matches!(component, Component::Normal(_) | Component::ParentDir))
        .count();
    if depth > MAX_DEPENDENCY_PATH_DEPTH {
        diags.push(Diagnostic::error(
            "E-MOD-PATH-ESCAPE",
            dep.span,
            format!(
                "dependency `{}` path `{raw}` has {depth} components; Core 1.0 accepts at most \
                 {MAX_DEPENDENCY_PATH_DEPTH}",
                dep.name
            ),
        ));
        return None;
    }

    // Path policy **before** any filesystem lookup: `..` is bounded by the
    // package parent directory. A lexical escape is `E-MOD-PATH-ESCAPE` even
    // when the target does not exist (conformance `security/dependency-path-escape`).
    let Some(allowed_parent) = pkg_dir.parent() else {
        diags.push(Diagnostic::error(
            "E-MOD-PATH-ESCAPE",
            dep.span,
            format!(
                "dependency `{}` path `{raw}`: the package has no parent directory to bound \
                 local dependencies",
                dep.name
            ),
        ));
        return None;
    };
    let mut normalized = pkg_dir.to_path_buf();
    let mut escaped = false;
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(name) => normalized.push(name),
            Component::ParentDir => match normalized.parent() {
                Some(parent) => normalized = parent.to_path_buf(),
                None => {
                    escaped = true;
                    break;
                }
            },
            Component::Prefix(_) | Component::RootDir => {
                escaped = true;
                break;
            }
        }
        if !normalized.starts_with(allowed_parent) {
            escaped = true;
            break;
        }
    }
    if escaped {
        diags.push(Diagnostic::error(
            "E-MOD-PATH-ESCAPE",
            dep.span,
            format!(
                "dependency `{}` path `{raw}` escapes the package parent directory; `..` is \
                 only allowed within the parent directory",
                dep.name
            ),
        ));
        return None;
    }

    let mut current = pkg_dir.to_path_buf();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match current.parent() {
                Some(parent) => current = parent.to_path_buf(),
                None => {
                    diags.push(Diagnostic::error(
                        "E-MOD-PATH-ESCAPE",
                        dep.span,
                        format!(
                            "dependency `{}` path `{raw}` escapes above the filesystem root",
                            dep.name
                        ),
                    ));
                    return None;
                }
            },
            Component::Normal(name) => {
                let component = name.to_string_lossy();
                if is_reserved_windows_component(&component) {
                    diags.push(Diagnostic::error(
                        "E-MOD-RESERVED-NAME",
                        dep.span,
                        format!(
                            "dependency `{}` path `{raw}` contains the reserved Windows \
                             component `{component}`",
                            dep.name
                        ),
                    ));
                    return None;
                }
                current.push(name);
                match symlink_metadata_optional(&current) {
                    Ok(Some(meta)) if meta.file_type().is_symlink() => {
                        diags.push(Diagnostic::error(
                            "E-MOD-PATH",
                            dep.span,
                            format!(
                                "dependency `{}` path `{raw}` traverses the symbolic link `{}`; \
                                 symlinks are refused for dependencies",
                                dep.name,
                                current.display()
                            ),
                        ));
                        return None;
                    }
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        diags.push(Diagnostic::error(
                            "E-MOD-DEPENDENCY",
                            dep.span,
                            format!(
                                "dependency `{}` path `{raw}` does not exist (`{}` is missing)",
                                dep.name,
                                current.display()
                            ),
                        ));
                        return None;
                    }
                    Err(e) => {
                        diags.push(Diagnostic::error(
                            "E-IO",
                            dep.span,
                            format!(
                                "dependency `{}`: cannot inspect `{}`: {e}",
                                dep.name,
                                current.display()
                            ),
                        ));
                        return None;
                    }
                }
            }
            Component::Prefix(_) | Component::RootDir => unreachable!("checked above"),
        }
    }

    let canonical = match fs::canonicalize(&current) {
        Ok(p) => p,
        Err(e) => {
            diags.push(Diagnostic::error(
                "E-MOD-DEPENDENCY",
                dep.span,
                format!(
                    "dependency `{}` path `{raw}` cannot be resolved (`{}`): {e}",
                    dep.name,
                    current.display()
                ),
            ));
            return None;
        }
    };
    // `..` is bounded by the parent directory of the package (Q-MOD-2):
    // siblings such as `../util` are legitimate, `../../outside` is an escape.
    let parent = pkg_dir.parent();
    if !parent
        .map(|allowed| canonical.starts_with(allowed))
        .unwrap_or(false)
    {
        diags.push(Diagnostic::error(
            "E-MOD-PATH-ESCAPE",
            dep.span,
            format!(
                "dependency `{}` path `{raw}` resolves to `{}`, outside the package parent \
                 directory; `..` is only allowed within the parent directory",
                dep.name,
                canonical.display()
            ),
        ));
        return None;
    }
    if !canonical.is_dir() {
        diags.push(Diagnostic::error(
            "E-MOD-DEPENDENCY",
            dep.span,
            format!(
                "dependency `{}` path `{raw}` is not a directory (`{}`)",
                dep.name,
                canonical.display()
            ),
        ));
        return None;
    }
    if !canonical.join(MANIFEST_NAME).is_file() {
        diags.push(Diagnostic::error(
            "E-MOD-DEPENDENCY",
            dep.span,
            format!(
                "dependency `{}` path `{raw}` does not contain a `{MANIFEST_NAME}` \
                 (`{}`)",
                dep.name,
                canonical.display()
            ),
        ));
        return None;
    }
    Some(canonical)
}

// ---------------------------------------------------------------------------
// Content digests
// ---------------------------------------------------------------------------

fn compute_digests(graph: &mut DepGraph, diags: &mut DiagnosticBag) {
    for package in &mut graph.packages {
        match package_digest(&package.dir, diags) {
            Some(digest) => package.digest = digest,
            None => graph.failed = true,
        }
    }
}

/// SHA-256 of the package content: every `.rv` file under `src/` (sorted by
/// relative path) plus `ravum.toml`, encoded as `path\0len\0bytes` with `\r\n`
/// normalised to `\n` (see `spec/language/modules.md` MOD-7.2 / Q-MOD-6).
fn package_digest(pkg_dir: &Path, diags: &mut DiagnosticBag) -> Option<String> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let manifest_path = pkg_dir.join(MANIFEST_NAME);
    if let Ok(Some(meta)) = symlink_metadata_optional(&manifest_path) {
        if meta.file_type().is_symlink() {
            err(
                diags,
                "E-MOD-PATH",
                Span::dummy(),
                format!(
                    "`{}` is a symbolic link; symlinks are refused for packages",
                    manifest_path.display()
                ),
            );
            return None;
        }
    }
    let manifest = match fs::read(&manifest_path) {
        Ok(bytes) => bytes,
        Err(e) => {
            diags.push(Diagnostic::error(
                "E-IO",
                Span::dummy(),
                format!("cannot read `{}`: {e}", manifest_path.display()),
            ));
            return None;
        }
    };
    if manifest.len() as u64 > MAX_FILE_SIZE {
        diags.push(Diagnostic::error(
            "E-LIMIT-FILE-SIZE",
            Span::dummy(),
            format!(
                "`{}` is larger than the {} byte limit",
                manifest_path.display(),
                MAX_FILE_SIZE
            ),
        ));
        return None;
    }
    files.push((MANIFEST_NAME.to_string(), normalize_newlines(&manifest)));

    let src = pkg_dir.join("src");
    if let Ok(Some(meta)) = symlink_metadata_optional(&src) {
        if meta.file_type().is_symlink() {
            err(
                diags,
                "E-MOD-PATH",
                Span::dummy(),
                format!(
                    "`{}` is a symbolic link; the package `src/` directory is never followed",
                    src.display()
                ),
            );
            return None;
        }
    }
    if symlink_metadata_optional(&src).ok().flatten().is_some() {
        let mut rv_files = Vec::new();
        if !collect_rv_files(&src, Path::new("src"), &mut rv_files, diags) {
            return None;
        }
        rv_files.sort();
        if rv_files.len() > MAX_PACKAGE_FILES {
            diags.push(Diagnostic::error(
                "E-LIMIT-FILES",
                Span::dummy(),
                format!(
                    "package `{}` contains more than {MAX_PACKAGE_FILES} `.rv` files",
                    pkg_dir.display()
                ),
            ));
            return None;
        }
        for rel in rv_files {
            let abs = pkg_dir.join(&rel);
            let data = match fs::read(&abs) {
                Ok(d) => d,
                Err(e) => {
                    diags.push(Diagnostic::error(
                        "E-IO",
                        Span::dummy(),
                        format!("cannot read `{}`: {e}", abs.display()),
                    ));
                    return None;
                }
            };
            if data.len() as u64 > MAX_FILE_SIZE {
                diags.push(Diagnostic::error(
                    "E-LIMIT-FILE-SIZE",
                    Span::dummy(),
                    format!(
                        "`{}` is larger than the {} byte limit",
                        abs.display(),
                        MAX_FILE_SIZE
                    ),
                ));
                return None;
            }
            files.push((path_to_slash(&rel), normalize_newlines(&data)));
        }
    }

    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut hasher = Sha256::new();
    for (path, bytes) in &files {
        hasher.update(path.as_bytes());
        hasher.update(&[0]);
        hasher.update(bytes.len().to_string().as_bytes());
        hasher.update(&[0]);
        hasher.update(bytes);
    }
    Some(format!("sha256:{}", hasher.finish_hex()))
}

fn collect_rv_files(
    dir: &Path,
    rel: &Path,
    out: &mut Vec<PathBuf>,
    diags: &mut DiagnosticBag,
) -> bool {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            diags.push(Diagnostic::error(
                "E-IO",
                Span::dummy(),
                format!("cannot list `{}`: {e}", dir.display()),
            ));
            return false;
        }
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name();
        let child_rel = rel.join(&name);
        if is_reserved_windows_component(&name.to_string_lossy()) {
            diags.push(Diagnostic::error(
                "E-MOD-RESERVED-NAME",
                Span::dummy(),
                format!(
                    "`{}` is a reserved Windows path component; package sources must be \
                     portable",
                    child_rel.display()
                ),
            ));
            return false;
        }
        let path = entry.path();
        let meta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                diags.push(Diagnostic::error(
                    "E-IO",
                    Span::dummy(),
                    format!("cannot inspect `{}`: {e}", path.display()),
                ));
                return false;
            }
        };
        if meta.file_type().is_symlink() {
            diags.push(Diagnostic::error(
                "E-MOD-PATH",
                Span::dummy(),
                format!(
                    "symbolic link `{}` under `src/` is refused by the package digest",
                    path.display()
                ),
            ));
            return false;
        }
        if meta.is_dir() {
            if !collect_rv_files(&path, &child_rel, out, diags) {
                return false;
            }
        } else if meta.is_file() && path.extension().and_then(|e| e.to_str()) == Some("rv") {
            out.push(child_rel);
        }
    }
    true
}

fn normalize_newlines(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            out.push(b'\n');
            i += 2;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

fn aggregate_dependencies_digest(deps: &[Dependency]) -> String {
    let mut hasher = Sha256::new();
    for dep in deps {
        hasher.update(dep.name.as_bytes());
        hasher.update(&[0]);
        hasher.update(dep.version.as_bytes());
        hasher.update(&[0]);
        hasher.update(dep.digest.as_bytes());
        hasher.update(b"\n");
    }
    format!("sha256:{}", hasher.finish_hex())
}

fn empty_dependencies_digest() -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"");
    format!("sha256:{}", hasher.finish_hex())
}

// ---------------------------------------------------------------------------
// Lockfile
// ---------------------------------------------------------------------------

fn update_lockfile(
    root: &Path,
    graph: &DepGraph,
    locked: bool,
    diags: &mut DiagnosticBag,
) -> LockfileState {
    let expected = lockfile_text(&graph.packages);
    let path = root.join(LOCKFILE_NAME);
    let existing = match fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            diags.push(Diagnostic::error(
                "E-MOD-PATH",
                Span::dummy(),
                format!(
                    "`{}` is a symbolic link; the lockfile is never followed",
                    path.display()
                ),
            ));
            return LockfileState::Invalid;
        }
        Ok(meta) if meta.is_file() => {
            match read_text_limited(&path, MAX_LOCKFILE_SIZE, "E-MOD-LOCK", diags) {
                Some(text) => Some(text),
                None => return LockfileState::Invalid,
            }
        }
        Ok(_) => {
            diags.push(Diagnostic::error(
                "E-MOD-LOCK",
                Span::dummy(),
                format!("`{}` is not a regular file", path.display()),
            ));
            return LockfileState::Invalid;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            diags.push(Diagnostic::error(
                "E-MOD-LOCK",
                Span::dummy(),
                format!("cannot inspect `{}`: {e}", path.display()),
            ));
            return LockfileState::Invalid;
        }
    };

    let Some(existing) = existing else {
        if locked {
            diags.push(Diagnostic::error(
                "E-MOD-MISSING",
                Span::dummy(),
                format!(
                    "`--locked` was requested but `{}` does not exist; run `ravum-lang check` \
                     without `--locked` to create it",
                    path.display()
                ),
            ));
            return LockfileState::Missing;
        }
        return match atomic_write(&path, expected.as_bytes()) {
            Ok(()) => LockfileState::Created,
            Err(e) => {
                diags.push(Diagnostic::error(
                    "E-MOD-LOCK",
                    Span::dummy(),
                    format!("cannot write `{}`: {e}", path.display()),
                ));
                LockfileState::Invalid
            }
        };
    };

    // Fail-closed on an unknown lockfile version, even in non-locked mode.
    match lockfile_version_of(&existing) {
        Some(LOCKFILE_VERSION) => {}
        Some(other) => {
            diags.push(Diagnostic::error(
                "E-MOD-LOCK",
                Span::dummy(),
                format!(
                    "`{}` uses lockfile_version {other}; this toolchain understands only \
                     {LOCKFILE_VERSION}",
                    path.display()
                ),
            ));
            return LockfileState::Invalid;
        }
        None => {
            diags.push(Diagnostic::error(
                "E-MOD-LOCK",
                Span::dummy(),
                format!(
                    "`{}` has no valid `lockfile_version` line; refusing to interpret it \
                     partially",
                    path.display()
                ),
            ));
            return LockfileState::Invalid;
        }
    }

    if existing == expected {
        return LockfileState::Current;
    }
    if locked {
        diags.push(Diagnostic::error(
            "E-MOD-LOCK",
            Span::dummy(),
            format!(
                "`--locked` was requested but `{}` is out of date with the resolved sources; \
                 run `ravum-lang check` to update it",
                path.display()
            ),
        ));
        return LockfileState::Stale;
    }
    // A stale lockfile is not an error and not a warning in the canonical
    // diagnostic registry: it is rewritten and reported through the command's
    // `lockfile` state / `lockfile_stale` JSON field. `--locked` above is the
    // strict mode.
    match atomic_write(&path, expected.as_bytes()) {
        Ok(()) => LockfileState::Stale,
        Err(e) => {
            diags.push(Diagnostic::error(
                "E-MOD-LOCK",
                Span::dummy(),
                format!("cannot update `{}`: {e}", path.display()),
            ));
            LockfileState::Invalid
        }
    }
}

fn lockfile_text(packages: &[GraphPackage]) -> String {
    let mut entries: Vec<&GraphPackage> = packages.iter().collect();
    entries.sort_by(|a, b| {
        a.manifest
            .name
            .cmp(&b.manifest.name)
            .then(a.manifest.version.cmp(&b.manifest.version))
    });
    let mut out = String::from(
        "# ravum.lock — generated by ravum-lang; do not edit.\n\
         # Format: lockfile_version 1 (spec/language/modules.md §7).\n\
         lockfile_version = 1\n",
    );
    for package in entries {
        out.push_str("\n[[package]]\n");
        out.push_str(&format!("name = \"{}\"\n", package.manifest.name));
        out.push_str(&format!("version = \"{}\"\n", package.manifest.version));
        out.push_str(&format!("digest = \"{}\"\n", package.digest));
    }
    out
}

fn lockfile_version_of(text: &str) -> Option<u64> {
    for line in text.lines() {
        let line = strip_comment(line).trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("lockfile_version") {
            let rest = rest.trim_start();
            return rest.strip_prefix('=')?.trim().parse::<u64>().ok();
        }
        // Any other content before the version is tolerated here; byte equality
        // with the canonical text is what decides staleness.
    }
    None
}

// ---------------------------------------------------------------------------
// Crate and module loading
// ---------------------------------------------------------------------------

struct CrateCtx<'a> {
    /// Canonical `src/` directory of the package (module escape bound).
    src_root: PathBuf,
    /// Canonical package directory used to render display paths.
    display_base: PathBuf,
    /// Prefix for display paths (`"util/"` for dependency sources).
    display_prefix: String,
    sources: &'a mut SourceMap,
    diags: &'a mut DiagnosticBag,
    file_ids: &'a mut BTreeMap<PathBuf, FileId>,
    // Files already included once in *this* crate (duplicate detection).
    loaded: BTreeSet<PathBuf>,
    synthetic: &'a mut u64,
    /// Accumulated wall-clock time spent in the parser (see `parse_time`).
    parse_time: &'a mut Duration,
    /// Per-session parse cache (see [`ParseCache`]).
    parse_cache: &'a mut ParseCache,
    /// Canonical path -> in-memory text (editor buffers not yet saved).
    overrides: &'a BTreeMap<PathBuf, String>,
}

fn load_crates(
    root: &Path,
    graph: &DepGraph,
    sources: &mut SourceMap,
    diags: &mut DiagnosticBag,
    parse_time: &mut Duration,
    parse_cache: &mut ParseCache,
    options: &LoadOptions,
) -> (Option<Program>, Option<Program>, Option<CrateKind>) {
    let src = root.join("src");
    let main_path = src.join("main.rv");
    let lib_path = src.join("lib.rv");
    let has_main = match regular_file(&main_path, diags) {
        Ok(v) => v,
        Err(()) => return (None, None, None),
    };
    let has_lib = match regular_file(&lib_path, diags) {
        Ok(v) => v,
        Err(()) => return (None, None, None),
    };
    if !has_main && !has_lib {
        err(
            diags,
            "E-MOD-MISSING",
            Span::dummy(),
            format!(
                "package `{}` has neither `src/main.rv` nor `src/lib.rv`",
                root.display()
            ),
        );
        return (None, None, None);
    }
    if has_main && has_lib {
        // Decision Core 1.0 (Lead): no silent choice between two roots. Also
        // documents that `main.rv` importing the package library is
        // UNSUPPORTED (E-UNSUPPORTED-SELF-LIB, checker-side) — keeping a single
        // root makes the rule unambiguous.
        err(
            diags,
            "E-MOD-BOTH-ROOTS",
            Span::dummy(),
            format!(
                "package `{}` contains both `src/main.rv` and `src/lib.rv`; Core 1.0 \
                 requires exactly one crate root (remove one of them)",
                root.display()
            ),
        );
        return (None, None, None);
    }

    let mut file_ids: BTreeMap<PathBuf, FileId> = BTreeMap::new();
    let mut synthetic: u64 = ((SYNTHETIC_FILE as u64) << 32) | 1;
    let (root_file, kind) = if has_main {
        (&main_path, CrateKind::Bin)
    } else {
        (&lib_path, CrateKind::Lib)
    };
    let program = load_crate_program(
        root_file,
        root,
        &src,
        graph,
        sources,
        diags,
        &mut file_ids,
        &mut synthetic,
        parse_time,
        parse_cache,
        options,
    );
    (program, None, Some(kind))
}

#[allow(clippy::too_many_arguments)]
fn load_crate_program(
    root_file: &Path,
    package_root: &Path,
    src_root: &Path,
    graph: &DepGraph,
    sources: &mut SourceMap,
    diags: &mut DiagnosticBag,
    file_ids: &mut BTreeMap<PathBuf, FileId>,
    synthetic: &mut u64,
    parse_time: &mut Duration,
    parse_cache: &mut ParseCache,
    options: &LoadOptions,
) -> Option<Program> {
    let mut ctx = CrateCtx {
        src_root: src_root.to_path_buf(),
        display_base: package_root.to_path_buf(),
        display_prefix: String::new(),
        sources,
        diags,
        file_ids,
        loaded: BTreeSet::new(),
        synthetic,
        parse_time,
        parse_cache,
        overrides: &options.overrides,
    };
    let module = load_crate_root(&mut ctx, root_file)?;
    let file = module.span.file;
    let mut program = Program { file, root: module };
    merge_dependencies(&mut program, graph, &mut ctx);
    Some(program)
}

fn regular_file(path: &Path, diags: &mut DiagnosticBag) -> Result<bool, ()> {
    match symlink_metadata_optional(path) {
        Ok(Some(meta)) if meta.file_type().is_symlink() => {
            diags.push(Diagnostic::error(
                "E-MOD-PATH",
                Span::dummy(),
                format!(
                    "`{}` is a symbolic link; crate roots and modules are never followed",
                    path.display()
                ),
            ));
            Err(())
        }
        Ok(Some(meta)) if meta.is_file() => Ok(true),
        Ok(Some(_)) => {
            diags.push(Diagnostic::error(
                "E-IO",
                Span::dummy(),
                format!("`{}` is not a regular file", path.display()),
            ));
            Err(())
        }
        Ok(None) => Ok(false),
        Err(e) => {
            diags.push(Diagnostic::error(
                "E-IO",
                Span::dummy(),
                format!("cannot inspect `{}`: {e}", path.display()),
            ));
            Err(())
        }
    }
}

fn load_crate_root(ctx: &mut CrateCtx<'_>, root_file: &Path) -> Option<Module> {
    if !check_lexical_no_symlinks(&ctx.src_root, root_file, ctx.diags) {
        return None;
    }
    let canonical = match fs::canonicalize(root_file) {
        Ok(p) => p,
        Err(e) => {
            err(
                ctx.diags,
                "E-IO",
                Span::dummy(),
                format!("cannot resolve `{}`: {e}", root_file.display()),
            );
            return None;
        }
    };
    if !path_is_under(&canonical, &ctx.src_root) {
        err(
            ctx.diags,
            "E-MOD-PATH-ESCAPE",
            Span::dummy(),
            format!(
                "crate root `{}` is outside the package `src/` directory",
                canonical.display()
            ),
        );
        return None;
    }
    if check_path_no_symlinks(&ctx.src_root, &canonical, ctx.diags).is_none() {
        return None;
    }
    ctx.loaded.insert(canonical.clone());
    load_file_module(ctx, &canonical, None)
}

fn load_file_module(
    ctx: &mut CrateCtx<'_>,
    canonical: &Path,
    name: Option<Ident>,
) -> Option<Module> {
    let file_id = ensure_source(ctx, canonical)?;
    let text = ctx.sources.file(file_id)?.text.clone();
    let key = (content_hash(text.as_bytes()), file_id);
    let (program, parsed_diags) = match ctx.parse_cache.get(&key) {
        Some((program, diags)) => (program, diags),
        None => {
            let parse_started = Instant::now();
            let parsed = parse_file(file_id, &text);
            *ctx.parse_time += parse_started.elapsed();
            let diags = parsed.diags.into_vec();
            ctx.parse_cache
                .insert(key, parsed.program.clone(), diags.clone());
            (parsed.program, diags)
        }
    };
    for diagnostic in parsed_diags {
        ctx.diags.push(diagnostic);
    }
    let mut module = program.root;
    module.name = name;
    let file_dir = canonical
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| ctx.src_root.clone());
    expand_module(ctx, &mut module, &file_dir, Path::new(""));
    Some(module)
}

fn expand_module(
    ctx: &mut CrateCtx<'_>,
    module: &mut Module,
    file_dir: &Path,
    inline_prefix: &Path,
) {
    let mut kept: Vec<Item> = Vec::with_capacity(module.items.len());
    for mut item in std::mem::take(&mut module.items) {
        match item.kind {
            ItemKind::Mod(mut inner) => {
                let inline_name = inner
                    .name
                    .as_ref()
                    .map(|ident| ident.name.clone())
                    .unwrap_or_default();
                let next_prefix = if inline_name.is_empty() {
                    inline_prefix.to_path_buf()
                } else {
                    inline_prefix.join(&inline_name)
                };
                expand_module(ctx, &mut inner, file_dir, &next_prefix);
                item.kind = ItemKind::Mod(inner);
                kept.push(item);
            }
            ItemKind::ModDecl(decl) => {
                if let Some(mut loaded) = resolve_module_file(ctx, &decl, file_dir, inline_prefix) {
                    loaded.name = Some(decl.name.clone());
                    item.kind = ItemKind::Mod(loaded);
                    kept.push(item);
                }
            }
            other => {
                item.kind = other;
                kept.push(item);
            }
        }
    }
    module.items = kept;
}

fn resolve_module_file(
    ctx: &mut CrateCtx<'_>,
    decl: &ravum_lang_syntax::ast::ModDecl,
    file_dir: &Path,
    inline_prefix: &Path,
) -> Option<Module> {
    let name = &decl.name.name;
    if is_reserved_windows_component(name) {
        err(
            ctx.diags,
            "E-MOD-RESERVED-NAME",
            decl.name.span,
            format!(
                "module `{name}` is a reserved Windows path component; module names must be \
                 portable (CON, PRN, AUX, NUL, COM1-9, LPT1-9, trailing dot/space)"
            ),
        );
        return None;
    }
    for component in inline_prefix.components() {
        if let Component::Normal(part) = component {
            let part = part.to_string_lossy();
            if is_reserved_windows_component(&part) {
                err(
                    ctx.diags,
                    "E-MOD-RESERVED-NAME",
                    decl.name.span,
                    format!(
                        "inline module path component `{part}` is a reserved Windows path \
                         component"
                    ),
                );
                return None;
            }
        }
    }
    let dir = file_dir.join(inline_prefix);
    let candidate_file = dir.join(format!("{name}.rv"));
    let candidate_dir = dir.join(name).join("mod.rv");

    let file_meta = match symlink_metadata_optional(&candidate_file) {
        Ok(meta) => meta,
        Err(e) => {
            err(
                ctx.diags,
                "E-IO",
                decl.name.span,
                format!("cannot inspect `{}`: {e}", candidate_file.display()),
            );
            return None;
        }
    };
    let dir_meta = match symlink_metadata_optional(&candidate_dir) {
        Ok(meta) => meta,
        Err(e) => {
            err(
                ctx.diags,
                "E-IO",
                decl.name.span,
                format!("cannot inspect `{}`: {e}", candidate_dir.display()),
            );
            return None;
        }
    };

    if file_meta
        .as_ref()
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
        || dir_meta
            .as_ref()
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
    {
        err(
            ctx.diags,
            "E-MOD-PATH",
            decl.name.span,
            format!(
                "module `{name}` resolves through a symbolic link; symlinks are refused for \
                 module resolution"
            ),
        );
        return None;
    }

    let is_file = file_meta.map(|m| m.is_file()).unwrap_or(false);
    let is_dir_file = dir_meta.map(|m| m.is_file()).unwrap_or(false);
    let candidate = match (is_file, is_dir_file) {
        (true, true) => {
            err(
                ctx.diags,
                "E-MOD-AMBIGUOUS",
                decl.name.span,
                format!(
                    "module `{name}` is ambiguous: both `{}` and `{}` exist",
                    candidate_file.display(),
                    candidate_dir.display()
                ),
            );
            return None;
        }
        (true, false) => candidate_file,
        (false, true) => candidate_dir,
        (false, false) => {
            err(
                ctx.diags,
                "E-MOD-MISSING",
                decl.name.span,
                format!(
                    "module `{name}` not found: neither `{}` nor `{}` exists",
                    candidate_file.display(),
                    candidate_dir.display()
                ),
            );
            return None;
        }
    };

    if !check_lexical_no_symlinks(&ctx.src_root, &candidate, ctx.diags) {
        return None;
    }
    let canonical = match fs::canonicalize(&candidate) {
        Ok(p) => p,
        Err(e) => {
            err(
                ctx.diags,
                "E-MOD-MISSING",
                decl.name.span,
                format!(
                    "cannot resolve module `{name}` (`{}`): {e}",
                    candidate.display()
                ),
            );
            return None;
        }
    };
    if !path_is_under(&canonical, &ctx.src_root) {
        err(
            ctx.diags,
            "E-MOD-PATH-ESCAPE",
            decl.name.span,
            format!(
                "module `{name}` resolves outside the package `src/` directory (`{}`)",
                canonical.display()
            ),
        );
        return None;
    }
    if check_path_no_symlinks(&ctx.src_root, &canonical, ctx.diags).is_none() {
        return None;
    }
    if ctx.loaded.contains(&canonical) {
        err(
            ctx.diags,
            "E-MOD-DUPLICATE",
            decl.name.span,
            format!(
                "module file `{}` is included more than once in this crate",
                display_relative(&canonical, &ctx.display_base)
            ),
        );
        return None;
    }
    ctx.loaded.insert(canonical.clone());
    load_file_module(ctx, &canonical, Some(decl.name.clone()))
}

fn ensure_source(ctx: &mut CrateCtx<'_>, canonical: &Path) -> Option<FileId> {
    if let Some(id) = ctx.file_ids.get(canonical) {
        return Some(*id);
    }
    if ctx.sources.len() >= source_file_limit() {
        let limit = source_file_limit();
        err(
            ctx.diags,
            "E-LIMIT-FILES",
            Span::dummy(),
            format!(
                "more than {limit} source files were loaded; the parser's NodeId \
                 reserves 32 bits for the FileId"
            ),
        );
        return None;
    }
    let text = match ctx.overrides.get(canonical) {
        // Editor buffer not saved yet: the overlay is analysed in place, never
        // written. The same size limit applies as for a real file so an
        // adversarial buffer cannot exhaust memory.
        Some(text) => {
            if text.len() as u64 > MAX_FILE_SIZE {
                err(
                    ctx.diags,
                    "E-LIMIT-FILE-SIZE",
                    Span::dummy(),
                    format!(
                        "`{}` is {} bytes, above the {} byte limit",
                        canonical.display(),
                        text.len(),
                        MAX_FILE_SIZE
                    ),
                );
                return None;
            }
            text.clone()
        }
        None => read_text_limited(canonical, MAX_FILE_SIZE, "E-IO", ctx.diags)?,
    };
    let display = format!(
        "{}{}",
        ctx.display_prefix,
        display_relative(canonical, &ctx.display_base)
    );
    let id = ctx.sources.add(display, text);
    ctx.file_ids.insert(canonical.to_path_buf(), id);
    Some(id)
}

/// Merge every resolved dependency (direct and transitive) into the crate root
/// as a public module named after the package, so `use util::helper;` resolves.
///
/// This is the encoding allowed by the frozen `sema::check(program, sources,
/// options)` interface: the checker only sees one `Program`. Cross-package
/// *visibility* rules (only declared dependencies are importable) remain the
/// checker's responsibility; the loader provides the definitions.
fn merge_dependencies(program: &mut Program, graph: &DepGraph, ctx: &mut CrateCtx<'_>) {
    let mut packages: Vec<&GraphPackage> = graph.packages.iter().skip(1).collect();
    packages.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));

    let local_names: BTreeSet<String> = program
        .root
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Mod(module) => module.name.as_ref().map(|n| n.name.clone()),
            _ => None,
        })
        .collect();

    for package in packages {
        let name = package.manifest.name.clone();
        if local_names.contains(&name) {
            err(
                ctx.diags,
                "E-MOD-DUPLICATE",
                Span::dummy(),
                format!(
                    "dependency `{name}` collides with a local module of the same name; \
                     rename one of them"
                ),
            );
            continue;
        }
        let lib_root = package.dir.join("src").join("lib.rv");
        let dep_src = package.dir.join("src");
        let mut dep_ctx = CrateCtx {
            src_root: dep_src.clone(),
            display_base: package.dir.clone(),
            display_prefix: format!("{name}/"),
            sources: &mut *ctx.sources,
            diags: &mut *ctx.diags,
            file_ids: &mut *ctx.file_ids,
            loaded: BTreeSet::new(),
            synthetic: &mut *ctx.synthetic,
            parse_time: &mut *ctx.parse_time,
            parse_cache: &mut *ctx.parse_cache,
            overrides: ctx.overrides,
        };
        let module = load_crate_root(&mut dep_ctx, &lib_root);
        if let Some(module) = module {
            let item_id = next_synthetic(dep_ctx.synthetic);
            let module_id = next_synthetic(dep_ctx.synthetic);
            let mut module = module;
            module.id = module_id;
            module.name = Some(Ident::new(name.clone(), Span::dummy()));
            program.root.items.push(Item {
                id: item_id,
                vis: Vis::Public(Span::dummy()),
                kind: ItemKind::Mod(module),
                span: Span::dummy(),
            });
        }
    }
}

fn next_synthetic(counter: &mut NodeId) -> NodeId {
    let id = *counter;
    *counter = counter.saturating_add(1);
    if (*counter >> 32) != u64::from(SYNTHETIC_FILE) {
        *counter = (u64::from(SYNTHETIC_FILE) << 32) | 1;
    }
    id
}

// ---------------------------------------------------------------------------
// Filesystem helpers
// ---------------------------------------------------------------------------

fn read_text_limited(
    path: &Path,
    limit: u64,
    code: &str,
    diags: &mut DiagnosticBag,
) -> Option<String> {
    match symlink_metadata_optional(path) {
        Ok(Some(meta)) if meta.file_type().is_symlink() => {
            err(
                diags,
                "E-MOD-PATH",
                Span::dummy(),
                format!(
                    "`{}` is a symbolic link; refusing to read it",
                    path.display()
                ),
            );
            return None;
        }
        Ok(Some(meta)) if meta.is_file() => {
            if meta.len() > limit {
                err(
                    diags,
                    "E-LIMIT-FILE-SIZE",
                    Span::dummy(),
                    format!(
                        "`{}` is {} bytes, above the {} byte limit",
                        path.display(),
                        meta.len(),
                        limit
                    ),
                );
                return None;
            }
        }
        Ok(Some(_)) => {
            err(
                diags,
                code,
                Span::dummy(),
                format!("`{}` is not a regular file", path.display()),
            );
            return None;
        }
        Ok(None) => {
            err(
                diags,
                code,
                Span::dummy(),
                format!("`{}` does not exist", path.display()),
            );
            return None;
        }
        Err(e) => {
            err(
                diags,
                code,
                Span::dummy(),
                format!("cannot inspect `{}`: {e}", path.display()),
            );
            return None;
        }
    }
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            err(
                diags,
                code,
                Span::dummy(),
                format!("cannot read `{}`: {e}", path.display()),
            );
            return None;
        }
    };
    match String::from_utf8(bytes) {
        Ok(text) => Some(text),
        Err(_) => {
            err(
                diags,
                "E-UNSUPPORTED-TOKEN",
                Span::dummy(),
                format!(
                    "`{}` is not valid UTF-8; Ravum Core 1.0 sources are UTF-8",
                    path.display()
                ),
            );
            None
        }
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "tmp".to_string());
    let tmp = parent.join(format!(".{file_name}.tmp-{}", std::process::id()));
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        let _ = file.sync_all();
    }
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

fn symlink_metadata_optional(path: &Path) -> std::io::Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(meta) => Ok(Some(meta)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn path_is_under(path: &Path, base: &Path) -> bool {
    path.strip_prefix(base).is_ok()
}

/// Walk the *lexical* path `target` (which must start with `base`) and refuse
/// any symbolic-link component. This is required in addition to canonical
/// escape checks: a symlink inside `src/` pointing to another `src/` directory
/// would otherwise be silently accepted.
fn check_lexical_no_symlinks(base: &Path, target: &Path, diags: &mut DiagnosticBag) -> bool {
    let Ok(relative) = target.strip_prefix(base) else {
        return true;
    };
    let mut current = base.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match symlink_metadata_optional(&current) {
            Ok(Some(meta)) if meta.file_type().is_symlink() => {
                err(
                    diags,
                    "E-MOD-PATH",
                    Span::dummy(),
                    format!(
                        "`{}` traverses the symbolic link `{}`; symlinks are refused",
                        target.display(),
                        current.display()
                    ),
                );
                return false;
            }
            Ok(_) => {}
            Err(e) => {
                err(
                    diags,
                    "E-IO",
                    Span::dummy(),
                    format!("cannot inspect `{}`: {e}", current.display()),
                );
                return false;
            }
        }
    }
    true
}

/// Walk from `base` to `target` (both canonical) and return an error if any
/// component in between is a symbolic link. Returns the target on success.
fn check_path_no_symlinks(base: &Path, target: &Path, diags: &mut DiagnosticBag) -> Option<()> {
    let Ok(relative) = target.strip_prefix(base) else {
        return Some(());
    };
    let mut current = base.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match symlink_metadata_optional(&current) {
            Ok(Some(meta)) if meta.file_type().is_symlink() => {
                err(
                    diags,
                    "E-MOD-PATH",
                    Span::dummy(),
                    format!(
                        "`{}` traverses the symbolic link `{}`; symlinks are refused",
                        target.display(),
                        current.display()
                    ),
                );
                return None;
            }
            Ok(_) => {}
            Err(e) => {
                err(
                    diags,
                    "E-IO",
                    Span::dummy(),
                    format!("cannot inspect `{}`: {e}", current.display()),
                );
                return None;
            }
        }
    }
    Some(())
}

fn display_relative(path: &Path, base: &Path) -> String {
    match path.strip_prefix(base) {
        Ok(rel) => path_to_slash(rel),
        Err(_) => path_to_slash(path),
    }
}

fn path_to_slash(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().to_string()),
            Component::CurDir => None,
            Component::ParentDir => Some("..".to_string()),
            Component::RootDir => Some(String::new()),
            Component::Prefix(prefix) => Some(prefix.as_os_str().to_string_lossy().to_string()),
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Top-level regular `.rv` files of a directory (symlinks ignored; they would be
/// refused by [`load_single_file`] anyway).
fn top_level_rv_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let meta = fs::symlink_metadata(&path)?;
        if meta.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("rv") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn display_files(sources: &SourceMap) -> Vec<String> {
    sources
        .files()
        .iter()
        .map(|file| file.path.clone())
        .collect()
}

fn err(diags: &mut DiagnosticBag, code: &str, span: Span, message: impl Into<String>) {
    diags.push(Diagnostic::error(code, span, message));
}

// ---------------------------------------------------------------------------
// SHA-256 (internal, dependency-free; tested against FIPS 180-4 vectors)
// ---------------------------------------------------------------------------

struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    buffered: usize,
    length: u64,
}

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

impl Sha256 {
    fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buffer: [0u8; 64],
            buffered: 0,
            length: 0,
        }
    }

    fn update(&mut self, mut data: &[u8]) {
        self.length = self.length.saturating_add(data.len() as u64);
        if self.buffered > 0 {
            let take = (64 - self.buffered).min(data.len());
            self.buffer[self.buffered..self.buffered + take].copy_from_slice(&data[..take]);
            self.buffered += take;
            data = &data[take..];
            if self.buffered == 64 {
                let block = self.buffer;
                self.compress(&block);
                self.buffered = 0;
            }
        }
        while data.len() >= 64 {
            let mut block = [0u8; 64];
            block.copy_from_slice(&data[..64]);
            self.compress(&block);
            data = &data[64..];
        }
        if !data.is_empty() {
            self.buffer[..data.len()].copy_from_slice(data);
            self.buffered = data.len();
        }
    }

    fn finish_hex(mut self) -> String {
        let bit_len = self.length.wrapping_mul(8);
        let mut padding = vec![0x80u8];
        let remainder = (self.length as usize + 1) % 64;
        let pad_len = if remainder <= 56 {
            56 - remainder
        } else {
            120 - remainder
        };
        padding.extend(std::iter::repeat(0u8).take(pad_len));
        padding.extend_from_slice(&bit_len.to_be_bytes());
        // `update` would count the padding in `length`; use a direct path.
        let saved_len = self.length;
        self.update(&padding);
        self.length = saved_len;
        debug_assert_eq!(self.buffered, 0);
        let mut out = String::with_capacity(64);
        for word in self.state {
            out.push_str(&format!("{word:08x}"));
        }
        out
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (i, chunk) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut a = self.state[0];
        let mut b = self.state[1];
        let mut c = self.state[2];
        let mut d = self.state[3];
        let mut e = self.state[4];
        let mut f = self.state[5];
        let mut g = self.state[6];
        let mut h = self.state[7];
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA256_K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }
}

/// Hex SHA-256 helper used by the CLI-level unit tests.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finish_hex()
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(tag: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let path = std::env::temp_dir().join(format!(
                "ravum-loader-{tag}-{}-{}-{nanos}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).expect("create temp dir");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }

        fn file(&self, rel: &str, content: &str) -> PathBuf {
            let path = self.path.join(rel);
            fs::create_dir_all(path.parent().unwrap()).expect("create parent");
            fs::write(&path, content).expect("write file");
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn manifest(name: &str, deps: &str) -> String {
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n\n{deps}"
        )
    }

    fn codes(diags: &DiagnosticBag) -> Vec<String> {
        diags.iter().map(|d| d.code.clone()).collect()
    }

    // --- SHA-256 ---------------------------------------------------------

    #[test]
    fn sha256_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"The quick brown fox jumps over the lazy dog"),
            "d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592"
        );
        assert_eq!(
            sha256_hex(&vec![b'a'; 1_000_000]),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
        // Block/padding boundaries.
        let mut expected = String::new();
        for len in [55usize, 56, 57, 63, 64, 65, 127, 128, 129] {
            let digest = sha256_hex(&vec![b'x'; len]);
            assert_eq!(digest.len(), 64);
            if len == 64 {
                expected = digest;
            }
        }
        assert!(!expected.is_empty());
    }

    // --- manifest --------------------------------------------------------

    #[test]
    fn manifest_accepts_minimal_package() {
        let src = "[package]\nname = \"hello\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n";
        let (manifest, diags) = parse_manifest(src, 0);
        assert!(!diags.has_errors(), "{:?}", codes(&diags));
        let manifest = manifest.expect("manifest");
        assert_eq!(manifest.name, "hello");
        assert_eq!(manifest.version, "0.1.0");
        assert_eq!(manifest.edition, "core-1.0");
        assert!(manifest.dependencies.is_empty());
    }

    #[test]
    fn manifest_accepts_path_dependency() {
        let src = manifest("app", "\n[dependencies]\nutil = { path = \"../util\" }\n");
        let (manifest, diags) = parse_manifest(&src, 0);
        assert!(!diags.has_errors(), "{:?}", codes(&diags));
        let manifest = manifest.expect("manifest");
        assert_eq!(manifest.dependencies.len(), 1);
        assert_eq!(manifest.dependencies[0].name, "util");
        assert_eq!(manifest.dependencies[0].path, "../util");
    }

    #[test]
    fn manifest_rejects_unknown_keys_and_sections() {
        let src = manifest(
            "app",
            "\n[dependencies]\nutil = { path = \"../util\", version = \"1\" }\n",
        );
        let (_, diags) = parse_manifest(&src, 0);
        assert!(codes(&diags).contains(&"E-MOD-DEPENDENCY".to_string()));

        let src = manifest("app", "\n[features]\ndefault = \"x\"\n");
        let (_, diags) = parse_manifest(&src, 0);
        assert!(codes(&diags).contains(&"E-MOD-MANIFEST".to_string()));

        let src = manifest("app", "\n[build]\nscript = \"build.sh\"\n");
        let (_, diags) = parse_manifest(&src, 0);
        assert!(codes(&diags).contains(&"E-MOD-BUILD-SCRIPT".to_string()));

        let src = "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\nlicense = \"MIT\"\n";
        let (_, diags) = parse_manifest(src, 0);
        assert!(codes(&diags).contains(&"E-MOD-MANIFEST".to_string()));
    }

    #[test]
    fn manifest_rejects_invalid_identity_fields() {
        let src = "[package]\nname = \"App\"\nversion = \"01.0.0\"\nedition = \"core-2.0\"\n";
        let (manifest, diags) = parse_manifest(src, 0);
        assert!(manifest.is_none());
        let codes = codes(&diags);
        assert!(codes.contains(&"E-MOD-MANIFEST".to_string()));
        assert!(codes.contains(&"E-UNSUPPORTED-EDITION".to_string()));
    }

    #[test]
    fn manifest_rejects_version_ranges_and_string_dependencies() {
        let src = manifest("app", "\n[dependencies]\nutil = \"^1.0.0\"\n");
        let (_, diags) = parse_manifest(&src, 0);
        assert!(codes(&diags).contains(&"E-MOD-DEPENDENCY".to_string()));
    }

    #[test]
    fn version_validation() {
        assert!(is_valid_version("0.0.0"));
        assert!(is_valid_version("1.2.3"));
        assert!(is_valid_version("10.20.30"));
        assert!(!is_valid_version("1.2"));
        assert!(!is_valid_version("1.2.3.4"));
        assert!(!is_valid_version("01.2.3"));
        assert!(!is_valid_version("1.2.3-alpha"));
        assert!(!is_valid_version("1.2.x"));
        assert!(!is_valid_version(""));
    }

    #[test]
    fn package_name_validation() {
        assert!(is_valid_package_name("hello"));
        assert!(is_valid_package_name("hello-world_2"));
        assert!(!is_valid_package_name("Hello"));
        assert!(!is_valid_package_name("2hello"));
        assert!(!is_valid_package_name("hello/world"));
        assert!(!is_valid_package_name(""));
    }

    #[test]
    fn reserved_windows_components_are_detected() {
        for name in [
            "CON", "con", "Con.rv", "PRN", "AUX", "NUL", "COM1", "LPT9", "aux.txt",
        ] {
            assert!(
                is_reserved_windows_component(name),
                "`{name}` must be reserved"
            );
        }
        for name in [
            "com0", "com10", "lpt0", "console", "foo.rv", "main", "aux_", "_con",
        ] {
            assert!(
                !is_reserved_windows_component(name),
                "`{name}` must not be reserved"
            );
        }
        assert!(is_reserved_windows_component("foo."));
        assert!(is_reserved_windows_component("foo "));
    }

    #[test]
    fn reserved_module_name_is_refused() {
        let temp = TempDir::new("reserved-module");
        temp.file("ravum.toml", &manifest("app", ""));
        temp.file("src/main.rv", "mod con;\nfn main() { }\n");
        let loaded = load_package(&temp.path);
        assert!(codes(&loaded.diags).contains(&"E-MOD-RESERVED-NAME".to_string()));
    }

    #[test]
    fn reserved_dependency_component_is_refused() {
        let temp = TempDir::new("reserved-dep");
        temp.file(
            "app/ravum.toml",
            &manifest("app", "\n[dependencies]\nutil = { path = \"../aux\" }\n"),
        );
        temp.file("app/src/main.rv", "fn main() { }\n");
        let loaded = load_package(&temp.path.join("app"));
        assert!(codes(&loaded.diags).contains(&"E-MOD-RESERVED-NAME".to_string()));
    }

    // --- package loading -------------------------------------------------

    #[test]
    fn package_loads_manifest_and_writes_lockfile() {
        let temp = TempDir::new("load");
        temp.file("ravum.toml", &manifest("app", ""));
        temp.file("src/main.rv", "fn main() { }\n");
        let loaded = load_package(&temp.path);
        assert_eq!(loaded.package_name.as_deref(), Some("app"));
        assert_eq!(loaded.package_version.as_deref(), Some("0.1.0"));
        assert_eq!(loaded.crate_kind, Some(CrateKind::Bin));
        assert_eq!(loaded.lockfile_state, LockfileState::Created);
        let lock = temp.path.join(LOCKFILE_NAME);
        assert!(lock.is_file());
        let text = fs::read_to_string(&lock).unwrap();
        assert!(text.contains("lockfile_version = 1"));
        assert!(text.contains("name = \"app\""));
        assert!(text.contains("digest = \"sha256:"));
    }

    #[test]
    fn lockfile_is_deterministic_across_runs() {
        let temp = TempDir::new("lock");
        temp.file("ravum.toml", &manifest("app", ""));
        temp.file("src/main.rv", "fn main() { }\n");
        let first = load_package(&temp.path);
        assert_eq!(first.lockfile_state, LockfileState::Created);
        let bytes_one = fs::read(temp.path.join(LOCKFILE_NAME)).unwrap();
        let second = load_package(&temp.path);
        assert_eq!(second.lockfile_state, LockfileState::Current);
        let bytes_two = fs::read(temp.path.join(LOCKFILE_NAME)).unwrap();
        assert_eq!(bytes_one, bytes_two);
    }

    #[test]
    fn lockfile_changes_when_dependency_changes() {
        let temp = TempDir::new("lockdep");
        temp.file("util/ravum.toml", &manifest("util", ""));
        temp.file("util/src/lib.rv", "pub fn helper() -> i32 { return 1; }\n");
        temp.file(
            "app/ravum.toml",
            &manifest("app", "\n[dependencies]\nutil = { path = \"../util\" }\n"),
        );
        temp.file("app/src/main.rv", "fn main() { }\n");
        let first = load_package(&temp.path.join("app"));
        assert_eq!(first.dependencies.len(), 1);
        let digest_one = first.dependencies_digest.clone();
        let lock_one = fs::read(temp.path.join("app").join(LOCKFILE_NAME)).unwrap();

        temp.file("util/src/lib.rv", "pub fn helper() -> i32 { return 2; }\n");
        let second = load_package(&temp.path.join("app"));
        assert_ne!(digest_one, second.dependencies_digest);
        let lock_two = fs::read(temp.path.join("app").join(LOCKFILE_NAME)).unwrap();
        assert_ne!(lock_one, lock_two);
    }

    #[test]
    fn locked_mode_refuses_missing_lockfile() {
        let temp = TempDir::new("locked");
        temp.file("ravum.toml", &manifest("app", ""));
        temp.file("src/main.rv", "fn main() { }\n");
        let loaded = load_package_with_options(
            &temp.path,
            &LoadOptions {
                locked: true,
                ..Default::default()
            },
        );
        assert_eq!(loaded.lockfile_state, LockfileState::Missing);
        assert!(codes(&loaded.diags).contains(&"E-MOD-MISSING".to_string()));
    }

    #[test]
    fn locked_mode_accepts_current_lockfile() {
        let temp = TempDir::new("locked-ok");
        temp.file("ravum.toml", &manifest("app", ""));
        temp.file("src/main.rv", "fn main() { }\n");
        let _ = load_package(&temp.path);
        let loaded = load_package_with_options(
            &temp.path,
            &LoadOptions {
                locked: true,
                ..Default::default()
            },
        );
        assert_eq!(loaded.lockfile_state, LockfileState::Current);
        assert!(!codes(&loaded.diags).contains(&"E-MOD-LOCK".to_string()));
    }

    #[test]
    fn override_is_analysed_instead_of_the_file_on_disk() {
        let temp = TempDir::new("override");
        temp.file("ravum.toml", &manifest("app", ""));
        temp.file("src/main.rv", "fn main() { }\n");
        let source = std::fs::canonicalize(temp.path.join("src").join("main.rv"))
            .expect("canonical source path");

        let buffered = "fn main() { let unsaved = 1; }\n";
        let options = LoadOptions {
            locked: false,
            overrides: std::iter::once((source.clone(), buffered.to_string())).collect(),
        };
        let loaded = load_package_with_options(&temp.path, &options);
        assert!(!loaded.diags.has_errors(), "{:?}", codes(&loaded.diags));
        let program = loaded.program.expect("program");
        let text = loaded
            .sources
            .file(program.file)
            .expect("root source")
            .text
            .clone();
        assert_eq!(
            text, buffered,
            "the loader must analyse the in-memory buffer"
        );
        assert_eq!(
            std::fs::read_to_string(&source).expect("read disk"),
            "fn main() { }\n",
            "an override must never write the user's file"
        );
    }

    #[test]
    fn override_does_not_change_the_package_digest_on_disk() {
        let temp = TempDir::new("override-digest");
        temp.file("ravum.toml", &manifest("app", ""));
        temp.file("src/main.rv", "fn main() { }\n");
        let source = std::fs::canonicalize(temp.path.join("src").join("main.rv"))
            .expect("canonical source path");
        let clean = load_package(&temp.path);
        let options = LoadOptions {
            locked: false,
            overrides: std::iter::once((source, "fn main() { let x = 2; }\n".to_string()))
                .collect(),
        };
        let overridden = load_package_with_options(&temp.path, &options);
        // A package with no dependency has the empty digest either way; the
        // point is that the override path adds no diagnostic and no extra file.
        assert_eq!(clean.dependencies_digest, overridden.dependencies_digest);
        assert_eq!(clean.loaded_files, overridden.loaded_files);
        assert!(
            !overridden.diags.has_errors(),
            "{:?}",
            codes(&overridden.diags)
        );
    }

    #[test]
    fn override_above_the_file_size_limit_is_refused() {
        let temp = TempDir::new("override-limit");
        temp.file("ravum.toml", &manifest("app", ""));
        temp.file("src/main.rv", "fn main() { }\n");
        let source = std::fs::canonicalize(temp.path.join("src").join("main.rv"))
            .expect("canonical source path");
        let huge = "x".repeat(MAX_FILE_SIZE as usize + 1);
        let options = LoadOptions {
            locked: false,
            overrides: std::iter::once((source, huge)).collect(),
        };
        let loaded = load_package_with_options(&temp.path, &options);
        assert!(codes(&loaded.diags).contains(&"E-LIMIT-FILE-SIZE".to_string()));
    }

    #[test]
    fn locked_mode_refuses_stale_lockfile() {
        let temp = TempDir::new("locked-stale");
        temp.file("ravum.toml", &manifest("app", ""));
        temp.file("src/main.rv", "fn main() { }\n");
        let _ = load_package(&temp.path);
        temp.file("src/main.rv", "fn main() { let x = 1; }\n");
        let loaded = load_package_with_options(
            &temp.path,
            &LoadOptions {
                locked: true,
                ..Default::default()
            },
        );
        assert_eq!(loaded.lockfile_state, LockfileState::Stale);
        assert!(codes(&loaded.diags).contains(&"E-MOD-LOCK".to_string()));
    }

    #[test]
    fn lockfile_unknown_version_is_rejected() {
        let temp = TempDir::new("lock-version");
        temp.file("ravum.toml", &manifest("app", ""));
        temp.file("src/main.rv", "fn main() { }\n");
        temp.file(
            LOCKFILE_NAME,
            "lockfile_version = 99\n\n[[package]]\nname = \"app\"\nversion = \"0.1.0\"\ndigest = \"sha256:00\"\n",
        );
        let loaded = load_package(&temp.path);
        assert_eq!(loaded.lockfile_state, LockfileState::Invalid);
        assert!(codes(&loaded.diags).contains(&"E-MOD-LOCK".to_string()));
    }

    #[test]
    fn missing_manifest_directory_is_rejected() {
        let temp = TempDir::new("no-manifest");
        temp.file("src/main.rv", "fn main() { }\n");
        let loaded = load_package(&temp.path);
        assert!(loaded.program.is_none());
        assert!(codes(&loaded.diags).contains(&"E-MOD-MANIFEST".to_string()));
    }

    // --- dependency graph security --------------------------------------

    fn app_with_dependency(dep_path: &str, dep_name: &str) -> TempDir {
        let temp = TempDir::new("depsec");
        temp.file(
            "app/ravum.toml",
            &manifest(
                "app",
                &format!("\n[dependencies]\n{dep_name} = {{ path = \"{dep_path}\" }}\n"),
            ),
        );
        temp.file("app/src/main.rv", "fn main() { }\n");
        temp
    }

    #[test]
    fn absolute_dependency_path_is_refused() {
        let temp = app_with_dependency("/tmp/util", "util");
        let loaded = load_package(&temp.path.join("app"));
        let codes = codes(&loaded.diags);
        assert!(
            codes.contains(&"E-MOD-PATH-ESCAPE".to_string())
                || codes.contains(&"E-MOD-DEPENDENCY".to_string()),
            "{codes:?}"
        );
    }

    #[test]
    fn dependency_path_above_package_parent_is_path_escape() {
        let temp = TempDir::new("dep-escape");
        temp.file("outside/ravum.toml", &manifest("outside", ""));
        temp.file("outside/src/lib.rv", "pub fn o() -> i32 { return 1; }\n");
        temp.file(
            "nested/app/ravum.toml",
            &manifest(
                "app",
                "\n[dependencies]\noutside = { path = \"../../outside\" }\n",
            ),
        );
        temp.file("nested/app/src/main.rv", "fn main() { }\n");
        let loaded = load_package(&temp.path().join("nested/app"));
        assert!(
            codes(&loaded.diags).contains(&"E-MOD-PATH-ESCAPE".to_string()),
            "codes: {:?}",
            codes(&loaded.diags)
        );
    }

    #[test]
    fn nonexistent_escape_is_path_escape_not_missing() {
        let temp = TempDir::new("dep-escape-missing");
        temp.file(
            "nested/app/ravum.toml",
            &manifest(
                "app",
                "\n[dependencies]\nutil = { path = \"../../outside\" }\n",
            ),
        );
        temp.file("nested/app/src/main.rv", "fn main() { }\n");
        // `../../outside` does not exist; the path policy must win over the
        // existence check.
        let loaded = load_package(&temp.path().join("nested/app"));
        let codes = codes(&loaded.diags);
        assert!(
            codes.contains(&"E-MOD-PATH-ESCAPE".to_string()),
            "codes: {codes:?}"
        );
        assert!(
            !codes.contains(&"E-MOD-DEPENDENCY".to_string()),
            "existence must not be reported before the path policy: {codes:?}"
        );
    }

    #[test]
    fn dependency_path_component_depth_is_bounded() {
        let temp = app_with_dependency("a/a/a/a/a/a/a/a/a", "util");
        let loaded = load_package(&temp.path().join("app"));
        assert!(
            codes(&loaded.diags).contains(&"E-MOD-PATH-ESCAPE".to_string()),
            "codes: {:?}",
            codes(&loaded.diags)
        );
    }

    #[test]
    fn windows_prefixed_dependency_path_is_refused() {
        let temp = app_with_dependency("C:\\\\tmp\\\\util", "util");
        let loaded = load_package(&temp.path.join("app"));
        let codes = codes(&loaded.diags);
        assert!(
            codes.contains(&"E-MOD-PATH-ESCAPE".to_string())
                || codes.contains(&"E-MOD-DEPENDENCY".to_string()),
            "{codes:?}"
        );
    }

    #[test]
    fn nonexistent_dependency_path_is_refused() {
        let temp = app_with_dependency("../nope", "util");
        let loaded = load_package(&temp.path.join("app"));
        assert!(codes(&loaded.diags).contains(&"E-MOD-DEPENDENCY".to_string()));
    }

    #[test]
    fn dependency_name_must_match_target_package() {
        let temp = TempDir::new("depname");
        temp.file("util/ravum.toml", &manifest("other", ""));
        temp.file("util/src/lib.rv", "pub fn helper() -> i32 { return 1; }\n");
        temp.file(
            "app/ravum.toml",
            &manifest("app", "\n[dependencies]\nutil = { path = \"../util\" }\n"),
        );
        temp.file("app/src/main.rv", "fn main() { }\n");
        let loaded = load_package(&temp.path.join("app"));
        assert!(codes(&loaded.diags).contains(&"E-MOD-DEPENDENCY".to_string()));
    }

    #[test]
    fn dependency_without_lib_is_refused() {
        let temp = TempDir::new("deplib");
        temp.file("util/ravum.toml", &manifest("util", ""));
        temp.file("util/src/main.rv", "fn main() { }\n");
        temp.file(
            "app/ravum.toml",
            &manifest("app", "\n[dependencies]\nutil = { path = \"../util\" }\n"),
        );
        temp.file("app/src/main.rv", "fn main() { }\n");
        let loaded = load_package(&temp.path.join("app"));
        assert!(codes(&loaded.diags).contains(&"E-MOD-DEPENDENCY".to_string()));
    }

    #[test]
    fn dependency_cycle_is_refused() {
        let temp = TempDir::new("depcycle");
        temp.file(
            "a/ravum.toml",
            &manifest("a", "\n[dependencies]\nb = { path = \"../b\" }\n"),
        );
        temp.file("a/src/lib.rv", "pub fn a() -> i32 { return 1; }\n");
        temp.file(
            "b/ravum.toml",
            &manifest("b", "\n[dependencies]\na = { path = \"../a\" }\n"),
        );
        temp.file("b/src/lib.rv", "pub fn b() -> i32 { return 2; }\n");
        let loaded = load_package(&temp.path.join("a"));
        assert!(codes(&loaded.diags).contains(&"E-MOD-CYCLE".to_string()));
    }

    #[test]
    fn two_packages_with_same_name_conflict() {
        let temp = TempDir::new("depconflict");
        // `a` and `b` both depend on different packages named `util`.
        temp.file(
            "a/ravum.toml",
            &manifest("a", "\n[dependencies]\nutil = { path = \"../util-a\" }\n"),
        );
        temp.file("a/src/lib.rv", "pub fn a() -> i32 { return 1; }\n");
        temp.file(
            "b/ravum.toml",
            &manifest("b", "\n[dependencies]\nutil = { path = \"../util-b\" }\n"),
        );
        temp.file("b/src/lib.rv", "pub fn b() -> i32 { return 2; }\n");
        temp.file("util-a/ravum.toml", &manifest("util", ""));
        temp.file("util-a/src/lib.rv", "pub fn ua() -> i32 { return 1; }\n");
        temp.file("util-b/ravum.toml", &manifest("util", ""));
        temp.file("util-b/src/lib.rv", "pub fn ub() -> i32 { return 2; }\n");
        temp.file(
            "app/ravum.toml",
            &manifest(
                "app",
                "\n[dependencies]\na = { path = \"../a\" }\nb = { path = \"../b\" }\n",
            ),
        );
        temp.file("app/src/main.rv", "fn main() { }\n");
        let loaded = load_package(&temp.path.join("app"));
        assert!(codes(&loaded.diags).contains(&"E-MOD-DUPLICATE".to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_dependency_is_refused() {
        let temp = TempDir::new("depsymlink");
        fs::create_dir_all(temp.path.join("real")).unwrap();
        temp.file("real/ravum.toml", &manifest("util", ""));
        temp.file("real/src/lib.rv", "pub fn u() -> i32 { return 1; }\n");
        std::os::unix::fs::symlink(temp.path.join("real"), temp.path.join("link"))
            .expect("create symlink");
        temp.file(
            "app/ravum.toml",
            &manifest("app", "\n[dependencies]\nutil = { path = \"../link\" }\n"),
        );
        temp.file("app/src/main.rv", "fn main() { }\n");
        let loaded = load_package(&temp.path.join("app"));
        assert!(codes(&loaded.diags).contains(&"E-MOD-PATH".to_string()));
    }

    #[test]
    fn directory_with_one_rv_file_uses_single_file_mode() {
        let temp = TempDir::new("lone-rv");
        temp.file("solo.rv", "fn main() { }\n");
        let loaded = load_package(&temp.path());
        assert_eq!(loaded.crate_kind, Some(CrateKind::File));
        assert_eq!(loaded.package_name.as_deref(), Some("solo"));
        assert_eq!(loaded.lockfile_state, LockfileState::NotApplicable);
    }

    #[test]
    fn directory_with_several_rv_files_without_manifest_is_ambiguous() {
        let temp = TempDir::new("two-rv");
        temp.file("a.rv", "fn main() { }\n");
        temp.file("b.rv", "fn main() { }\n");
        let loaded = load_package(&temp.path());
        assert!(codes(&loaded.diags).contains(&"E-MOD-MANIFEST".to_string()));
    }

    #[test]
    fn single_file_mode_loads_without_manifest() {
        let temp = TempDir::new("single");
        let file = temp.file("hello.rv", "fn main() { }\n");
        let loaded = load_package(&file);
        assert_eq!(loaded.crate_kind, Some(CrateKind::File));
        assert_eq!(loaded.package_name.as_deref(), Some("hello"));
        assert_eq!(loaded.lockfile_state, LockfileState::NotApplicable);
        assert_eq!(loaded.dependencies_digest, empty_dependencies_digest());
    }

    // --- module resolution (real parser) ---------------------------------

    fn has_mod_decl(module: &Module) -> bool {
        module.items.iter().any(|item| match &item.kind {
            ItemKind::Mod(inner) => has_mod_decl(inner),
            ItemKind::ModDecl(_) => true,
            _ => false,
        })
    }

    fn module_named<'a>(module: &'a Module, name: &str) -> Option<&'a Module> {
        module.items.iter().find_map(|item| match &item.kind {
            ItemKind::Mod(inner) if inner.name.as_ref().map(|n| n.name.as_str()) == Some(name) => {
                Some(inner)
            }
            _ => None,
        })
    }

    #[test]
    fn module_files_are_expanded_into_inline_modules() {
        let temp = TempDir::new("modules");
        temp.file("ravum.toml", &manifest("app", ""));
        temp.file(
            "src/main.rv",
            "mod util;\nmod geo {\n    mod point;\n}\nfn main() { }\n",
        );
        temp.file("src/util.rv", "pub fn helper() -> i32 { return 1; }\n");
        temp.file("src/geo/point.rv", "pub struct Point { pub x: i32 }\n");

        let loaded = load_package(&temp.path);
        assert!(
            !loaded.diags.has_errors(),
            "loader errors: {:?}",
            codes(&loaded.diags)
        );
        let program = loaded.program.expect("parsed program");
        assert!(
            !has_mod_decl(&program.root),
            "ModDecl leaked to the checker"
        );
        let util = module_named(&program.root, "util").expect("util module");
        assert!(matches!(
            util.items.first().map(|item| &item.kind),
            Some(ItemKind::Fn(_))
        ));
        let geo = module_named(&program.root, "geo").expect("inline geo module");
        assert!(module_named(geo, "point").is_some(), "nested file module");
        assert!(loaded
            .loaded_files
            .iter()
            .any(|path| path == "src/geo/point.rv"));
    }

    #[test]
    fn both_crate_roots_are_refused() {
        let temp = TempDir::new("both-roots");
        temp.file("ravum.toml", &manifest("app", ""));
        temp.file("src/main.rv", "fn main() { }\n");
        temp.file("src/lib.rv", "pub fn helper() -> i32 { return 1; }\n");
        let loaded = load_package(&temp.path);
        assert!(codes(&loaded.diags).contains(&"E-MOD-BOTH-ROOTS".to_string()));
        assert!(loaded.program.is_none());
        assert!(loaded.lib_program.is_none());
    }

    #[test]
    fn ambiguous_module_file_is_refused() {
        let temp = TempDir::new("ambiguous");
        temp.file("ravum.toml", &manifest("app", ""));
        temp.file("src/main.rv", "mod util;\nfn main() { }\n");
        temp.file("src/util.rv", "pub fn a() -> i32 { return 1; }\n");
        temp.file("src/util/mod.rv", "pub fn b() -> i32 { return 2; }\n");
        let loaded = load_package(&temp.path);
        assert!(codes(&loaded.diags).contains(&"E-MOD-AMBIGUOUS".to_string()));
        assert!(loaded.program.is_none());
    }

    #[test]
    fn missing_module_file_is_refused() {
        let temp = TempDir::new("missing-module");
        temp.file("ravum.toml", &manifest("app", ""));
        temp.file("src/main.rv", "mod absent;\nfn main() { }\n");
        let loaded = load_package(&temp.path);
        assert!(codes(&loaded.diags).contains(&"E-MOD-MISSING".to_string()));
    }

    #[test]
    fn duplicate_module_file_is_refused() {
        let temp = TempDir::new("duplicate-module");
        temp.file("ravum.toml", &manifest("app", ""));
        temp.file("src/main.rv", "mod a;\nmod b;\nfn main() { }\n");
        temp.file("src/a.rv", "mod shared;\npub fn a() -> i32 { return 1; }\n");
        temp.file("src/b.rv", "mod shared;\npub fn b() -> i32 { return 2; }\n");
        temp.file("src/shared.rv", "pub fn s() -> i32 { return 3; }\n");
        let loaded = load_package(&temp.path);
        assert!(codes(&loaded.diags).contains(&"E-MOD-DUPLICATE".to_string()));
    }

    #[test]
    fn module_cycle_surfaces_as_duplicate_file() {
        let temp = TempDir::new("module-cycle");
        temp.file("ravum.toml", &manifest("app", ""));
        temp.file("src/main.rv", "mod a;\nfn main() { }\n");
        temp.file("src/a.rv", "mod b;\npub fn a() -> i32 { return 1; }\n");
        temp.file("src/b.rv", "mod a;\npub fn b() -> i32 { return 2; }\n");
        let loaded = load_package(&temp.path);
        assert!(codes(&loaded.diags).contains(&"E-MOD-DUPLICATE".to_string()));
    }

    #[test]
    fn dependency_root_is_merged_as_public_module() {
        let temp = TempDir::new("dep-merge");
        temp.file("util/ravum.toml", &manifest("util", ""));
        temp.file("util/src/lib.rv", "pub fn helper() -> i32 { return 1; }\n");
        temp.file(
            "app/ravum.toml",
            &manifest("app", "\n[dependencies]\nutil = { path = \"../util\" }\n"),
        );
        temp.file("app/src/main.rv", "use util::helper;\nfn main() { }\n");
        let loaded = load_package(&temp.path.join("app"));
        assert!(
            !loaded.diags.has_errors(),
            "loader errors: {:?}",
            codes(&loaded.diags)
        );
        let program = loaded.program.expect("parsed program");
        let util = module_named(&program.root, "util").expect("dependency module");
        assert!(util
            .items
            .iter()
            .any(|item| matches!(&item.kind, ItemKind::Fn(_))));
        assert_eq!(loaded.dependencies.len(), 1);
        assert!(loaded.dependencies_digest.starts_with("sha256:"));
    }

    #[test]
    fn digest_encoding_is_stable_for_crlf() {
        let temp = TempDir::new("crlf");
        temp.file("ravum.toml", &manifest("app", ""));
        temp.file("src/main.rv", "fn main() { }\n");
        let lf = package_digest(&temp.path, &mut DiagnosticBag::new()).unwrap();
        temp.file("src/main.rv", "fn main() { }\r\n");
        let crlf = package_digest(&temp.path, &mut DiagnosticBag::new()).unwrap();
        assert_eq!(lf, crlf);
    }

    /// T4: the source-file ceiling is exercised without creating 65 536 files.
    /// The limit is lowered thread-locally, so no other test is affected.
    #[test]
    fn the_source_file_limit_is_refused_with_the_existing_diagnostic() {
        let temp = TempDir::new("file-limit");
        temp.file("ravum.toml", &manifest("file-limit", ""));
        temp.file("src/main.rv", "mod a;\nmod b;\nfn main() { }\n");
        temp.file("src/a.rv", "pub fn a() -> i32 { return 1; }\n");
        temp.file("src/b.rv", "pub fn b() -> i32 { return 2; }\n");
        TEST_SOURCE_FILE_LIMIT.with(|limit| limit.set(Some(3)));
        let loaded = load_package_with_options(
            &temp.path,
            &LoadOptions {
                locked: false,
                overrides: std::collections::BTreeMap::new(),
            },
        );
        TEST_SOURCE_FILE_LIMIT.with(|limit| limit.set(None));
        let found = codes(&loaded.diags);
        assert!(
            found.iter().any(|code| code == "E-LIMIT-FILES"),
            "expected E-LIMIT-FILES, got {found:?}"
        );
        let message = loaded
            .diags
            .iter()
            .find(|diag| diag.code == "E-LIMIT-FILES")
            .map(|diag| diag.message.clone())
            .unwrap_or_default();
        assert!(
            message.contains("more than 3 source files"),
            "the diagnostic must name the effective limit: {message}"
        );
    }
}
