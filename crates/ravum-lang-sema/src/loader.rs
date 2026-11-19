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

/// One `[native]` entry (RFC-E11-SQLITE NAT-1): a pre-built native library
/// the package links. `path` is relative to the package; `{os}` and `{arch}`
/// are replaced by the host platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeDecl {
    pub name: String,
    pub kind: NativeKind,
    pub path: String,
    pub lib: String,
    /// `(platform, hex)`: `""` for the plain `sha256` key, `"<os>-<arch>"`
    /// for a `sha256-<os>-<arch>` key.
    pub sha256: Vec<(String, String)>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeKind {
    Static,
    Dylib,
}

impl NativeKind {
    pub fn name(self) -> &'static str {
        match self {
            NativeKind::Static => "static",
            NativeKind::Dylib => "dylib",
        }
    }
}

/// A native library resolved on disk, its digest checked, and used by at
/// least one `extern "C" from "<name>"` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeLib {
    pub name: String,
    /// The package whose `[native]` section declares it.
    pub package: String,
    pub kind: NativeKind,
    /// Absolute directory passed to `rustc -L native=`.
    pub dir: PathBuf,
    pub lib: String,
    /// `sha256:<hex>` of the library file.
    pub digest: String,
}

/// Strict, validated `ravum.toml` content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageManifest {
    pub name: String,
    pub version: String,
    pub edition: String,
    /// Sorted by dependency name (deterministic).
    pub dependencies: Vec<DependencyDecl>,
    /// `[native]` entries, sorted by name.
    pub native: Vec<NativeDecl>,
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
    /// Append-only session table: canonical path -> `FileId` (T5).
    ///
    /// A language-server session reuses a file's `FileId` across reloads, even
    /// when a file is added or removed in between: every `NodeId` embeds the
    /// `FileId`, so a shifted id would invalidate the parse cache and renumber
    /// the AST of files the user did not touch. The loader reuses the id found
    /// here and assigns `max(id) + 1` to a path it has not seen yet; the
    /// resulting table is returned in [`LoadedPackage::file_ids`] and must be
    /// kept for the next load. `None` is the CLI: dense ids in load order.
    pub file_ids: Option<BTreeMap<PathBuf, FileId>>,
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
    /// Native libraries the program links (RFC-E11-SQLITE NAT-1, NAT-2).
    /// Their digests are folded into `dependencies_digest`, so they are part
    /// of the build identity.
    pub natives: Vec<NativeLib>,
    pub lockfile_state: LockfileState,
    /// Wall-clock time spent in the parser while loading this package (all
    /// files, dependencies included). Additive measurement for `--timings`;
    /// zero when the package was not parsed.
    pub parse_time: Duration,
    /// Source files registered in `sources`, in load order (display paths).
    pub loaded_files: Vec<String>,
    /// Canonical path -> `FileId` for every source registered by this load,
    /// `ravum.toml` included, plus the entries of
    /// [`LoadOptions::file_ids`] the load started from. Empty when the load
    /// failed before registering anything. A language-server session merges
    /// this into its table after every analysis.
    pub file_ids: BTreeMap<PathBuf, FileId>,
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
    let mut out = load_unrelativized(path, options, cache);
    // A directory is its own base; a file, or a path that cannot be read, is
    // shown relative to its parent so that its own name stays in the message.
    let base = match fs::metadata(path) {
        Ok(meta) if meta.is_dir() => path.to_path_buf(),
        _ => path.parent().unwrap_or(path).to_path_buf(),
    };
    let relativize = paths::Relativizer::new(&base);
    out.diags.rewrite_text(|text| relativize.apply(text));
    out
}

fn load_unrelativized(path: &Path, options: &LoadOptions, cache: &mut ParseCache) -> LoadedPackage {
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
            natives: Vec::new(),
            lockfile_state: LockfileState::NotApplicable,
            parse_time: Duration::ZERO,
            loaded_files: Vec::new(),
            file_ids: BTreeMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Single-file mode
// ---------------------------------------------------------------------------

mod crates;
mod digest;
mod files;
mod fsutil;
mod graph;
mod lockfile;
mod native;
mod paths;
mod sha256;
mod toml;
mod validate;

use crates::*;
use digest::*;
use files::*;
use fsutil::*;
use graph::*;
use lockfile::*;
use native::*;
pub use sha256::sha256_hex;
use sha256::Sha256;
use toml::*;
use validate::*;

#[cfg(test)]
mod tests;
