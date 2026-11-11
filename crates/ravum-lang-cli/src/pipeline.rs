//! Shared frontend/build pipeline used by every command.
//!
//! The CLI never invents facts: it loads the package, runs the frozen
//! `sema::check` entry point, and hands the resulting `CheckedProgram` to
//! `codegen::generate`. A stage that is still a stub reports its own internal
//! diagnostic and the command exits 1 — no false success.
//!
//! # Build identity, isolation and publication (P0-B)
//!
//! A build is described by a deterministic [`BuildIdentity`]: the content digest
//! of the analyzed source snapshot (in-memory texts, never mtimes), the
//! dependencies digest, the crate name/kind, the profile options, the target,
//! the test mode, the generator version and the rustc version. Same inputs give
//! the same identity; any differing input gives a different one.
//!
//! Intermediate files live under `<package>/.ravum/build/<identity>/`, so two
//! builds of different identities never share an intermediate `.rs` or an output
//! path. The directory component is a deliberately short 16-hex digest prefix and
//! the only nested component is `out/<pid>-<counter>/`, because the MSVC linker
//! (`link.exe`, spawned by rustc) fails with a spurious
//! `LNK1181: cannot open input file '.obj'` once the paths handed to it grow long
//! or carry a `\\?\` verbatim prefix. No path given to rustc is ever verbatim.
//! Inside one identity the generated source is written atomically (its bytes are
//! deterministic, so concurrent writers agree) while each invocation compiles into
//! its own output directory: two processes building the same package at the same
//! time cannot truncate each other's output.
//!
//! The user-facing paths stay the documented ones: `<package>/.ravum/gen/main.rs`
//! (or `tests.rs`), `<package>/.ravum/bin/<name>[.exe|.wasm|]` and
//! `<package>/.ravum/lib/<name>.rlib`. Those are *publication* targets only: the
//! artifact is renamed into place (`std::fs::rename`: `rename(2)` on Unix,
//! `MoveFileExW(MOVEFILE_REPLACE_EXISTING)` on Windows) after rustc exited 0 and
//! after the package was re-loaded and its snapshot digest compared with the one
//! this build was generated from. A failed, interrupted or stale build therefore
//! leaves the previously published artifact untouched. Relying on `fs::rename`
//! means the publication is atomic only inside one volume and cannot replace a
//! running Windows executable; both limits are reported as errors rather than
//! papered over with a copy. All paths handed to rustc are plain (no `\\?\`
//! verbatim prefix), because the MSVC linker mis-parses those.
//!
//! Next to each published artifact a manifest `<artifact>.build.json` records the
//! identity, the source digest, the options, the toolchain and a timestamp, so a
//! caller can verify that an artifact matches the sources it claims. The manifest
//! is published after the artifact; an interruption between the two leaves a
//! *verifiably* stale manifest (which is what the source-digest comparison is
//! for), never a manifest claiming an artifact that does not exist.

use ravum_lang_codegen::{generate, GenOptions, GENERATOR_VERSION};
use ravum_lang_sema::facts::{CheckOptions, CheckedProgram};
use ravum_lang_sema::loader::{load_package_with_options, CrateKind, LoadOptions};
use ravum_lang_sema::{check, LoadedPackage, Profile};
use ravum_lang_syntax::ast::{ItemKind, Module, Program};
use ravum_lang_syntax::diag::DiagnosticBag;
use ravum_lang_syntax::source::SourceMap;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::util;

/// Result of loading + checking one package (both crate roots when present).
pub struct Frontend {
    /// The path the caller asked to analyse (package directory or single file).
    /// Kept so the publication guard can run the *same* load again.
    pub target: PathBuf,
    pub pkg: LoadedPackage,
    pub checked: Option<CheckedProgram>,
    /// Checked `src/lib.rv` when both crate roots exist. Kept for `doc`/future
    /// commands; build/test use the primary crate (`main.rv` when present).
    #[allow(dead_code)]
    pub lib_checked: Option<CheckedProgram>,
    pub diags: DiagnosticBag,
}

impl Frontend {
    pub fn success(&self) -> bool {
        !self.diags.has_errors()
    }

    pub fn root(&self) -> PathBuf {
        PathBuf::from(&self.pkg.root_dir)
    }

    pub fn error_count(&self) -> usize {
        self.diags.error_count()
    }

    pub fn warning_count(&self) -> usize {
        self.diags.warning_count()
    }
}

/// Load a package and run the checker on every crate root it contains.
pub fn analyze(path: &Path, profile: Profile, locked: bool) -> Frontend {
    let mut pkg = load_package_with_options(
        path,
        &LoadOptions {
            locked,
            overrides: std::collections::BTreeMap::new(),
        },
    );
    let mut diags = std::mem::take(&mut pkg.diags);
    let options = CheckOptions {
        profile,
        edition: ravum_lang_syntax::LANGUAGE_EDITION.to_string(),
    };
    let mut checked = None;
    let mut lib_checked = None;

    if !diags.has_errors() {
        if let Some(program) = &pkg.program {
            let result = check(program, &pkg.sources, &options);
            diags.extend(result.diags);
            checked = result.checked;
        }
        if let Some(program) = &pkg.lib_program {
            let result = check(program, &pkg.sources, &options);
            diags.extend(result.diags);
            lib_checked = result.checked;
        }
    }

    diags.sort();
    Frontend {
        target: path.to_path_buf(),
        pkg,
        checked,
        lib_checked,
        diags,
    }
}

pub fn render_diags(frontend: &Frontend) -> String {
    frontend.diags.render(&frontend.pkg.sources)
}

// ---------------------------------------------------------------------------
// Summary helpers
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Copy)]
pub struct Counts {
    pub items: usize,
    pub functions: usize,
    pub tests: usize,
}

pub fn program_counts(program: &Program) -> Counts {
    let mut counts = Counts::default();
    count_module(&program.root, &mut counts);
    counts
}

fn count_module(module: &Module, counts: &mut Counts) {
    for item in &module.items {
        counts.items += 1;
        match &item.kind {
            ItemKind::Fn(function) => {
                counts.functions += 1;
                if function.is_test {
                    counts.tests += 1;
                }
            }
            ItemKind::Impl(implementation) => {
                for member in &implementation.items {
                    counts.items += 1;
                    if let ItemKind::Fn(function) = &member.kind {
                        counts.functions += 1;
                        if function.is_test {
                            counts.tests += 1;
                        }
                    }
                }
            }
            ItemKind::Mod(inner) => count_module(inner, counts),
            _ => {}
        }
    }
}

pub fn rv_file_count(frontend: &Frontend) -> usize {
    frontend
        .pkg
        .loaded_files
        .iter()
        .filter(|path| path.ends_with(".rv"))
        .count()
}

pub fn counts_for(frontend: &Frontend) -> Counts {
    if let Some(checked) = &frontend.checked {
        program_counts(&checked.program)
    } else if let Some(program) = &frontend.pkg.program {
        program_counts(program)
    } else {
        Counts::default()
    }
}

#[derive(Debug, Clone)]
pub struct FnEffects {
    pub name: String,
    pub path: String,
    pub effects: Vec<String>,
    pub declared: Option<Vec<String>>,
    pub is_test: bool,
}

pub fn function_effects(checked: &CheckedProgram) -> Vec<FnEffects> {
    let mut out: Vec<FnEffects> = checked
        .facts
        .fn_sigs
        .values()
        .map(|signature| FnEffects {
            name: signature.name.clone(),
            path: signature.rust_path.clone(),
            effects: signature
                .effects
                .names()
                .into_iter()
                .map(str::to_string)
                .collect(),
            declared: signature
                .declared_effects
                .as_ref()
                .map(|set| set.names().into_iter().map(str::to_string).collect()),
            is_test: signature.is_test,
        })
        .collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

pub fn crate_name(frontend: &Frontend) -> String {
    frontend
        .pkg
        .crate_kind
        .map(|kind| kind.name().to_string())
        .unwrap_or_else(|| "none".to_string())
}

pub fn lockfile_name(frontend: &Frontend) -> String {
    frontend.pkg.lockfile_state.name().to_string()
}

/// Summary shared by `check` and `lint` (text and JSON use the same data).
pub fn base_summary(frontend: &Frontend, profile: Profile) -> serde_json::Value {
    let counts = counts_for(frontend);
    let effects = frontend
        .checked
        .as_ref()
        .map(function_effects)
        .unwrap_or_default()
        .into_iter()
        .map(|effect| {
            json!({
                "name": effect.name,
                "path": effect.path,
                "effects": effect.effects,
                "declared": effect.declared,
                "is_test": effect.is_test,
            })
        })
        .collect::<Vec<_>>();
    let dependencies = frontend
        .pkg
        .dependencies
        .iter()
        .map(|dependency| {
            json!({
                "name": dependency.name,
                "version": dependency.version,
                "digest": dependency.digest,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "package": frontend.pkg.package_name,
        "crate": frontend.pkg.crate_kind.map(|kind| kind.name()),
        "profile": profile.name(),
        "files": rv_file_count(frontend),
        "items": counts.items,
        "functions": counts.functions,
        "tests": counts.tests,
        "errors": frontend.error_count(),
        "warnings": frontend.warning_count(),
        "effects": effects,
        "dependencies": dependencies,
        "dependencies_digest": frontend.pkg.dependencies_digest,
        "lockfile": lockfile_name(frontend),
    })
}

pub fn text_summary(frontend: &Frontend, profile: Profile) -> String {
    let counts = counts_for(frontend);
    format!(
        "package {} ({}, profile {}): {} file(s), {} item(s), {} function(s), {} test(s), \
         {} error(s), {} warning(s)",
        frontend.pkg.package_name.as_deref().unwrap_or("<none>"),
        crate_name(frontend),
        profile.name(),
        rv_file_count(frontend),
        counts.items,
        counts.functions,
        counts.tests,
        frontend.error_count(),
        frontend.warning_count(),
    )
}

// ---------------------------------------------------------------------------
// Code generation and rustc
// ---------------------------------------------------------------------------

/// Compilation target selected by `--target`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BuildTarget {
    /// Host binary (default).
    #[default]
    Native,
    /// WebAssembly with WASI preview 1, executed through Node.js.
    Wasm32WasiP1,
}

impl BuildTarget {
    /// Only the canonical triple is accepted; anything else is a CLI usage
    /// error (exit 2) so a typo never silently builds for the host.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "wasm32-wasip1" => Some(BuildTarget::Wasm32WasiP1),
            _ => None,
        }
    }

    pub fn rustc_triple(self) -> Option<&'static str> {
        match self {
            BuildTarget::Native => None,
            BuildTarget::Wasm32WasiP1 => Some("wasm32-wasip1"),
        }
    }

    pub fn is_wasm(self) -> bool {
        matches!(self, BuildTarget::Wasm32WasiP1)
    }
}

#[derive(Debug)]
pub struct BuildOptions {
    pub release: bool,
    pub test_mode: bool,
    pub emit_rust: Option<PathBuf>,
    pub target: BuildTarget,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            release: false,
            test_mode: false,
            emit_rust: None,
            target: BuildTarget::Native,
        }
    }
}

/// Version of the build-identity formula. Bump it whenever the inputs or their
/// encoding change: every existing `.ravum/build/<identity>/` directory then
/// becomes unreachable instead of being silently reused for the new formula.
pub const BUILD_IDENTITY_VERSION: u32 = 1;

/// Deterministic description of one build (P0-B).
///
/// Two builds with the same identity must produce the same artifact; any
/// differing input listed in [`BuildIdentity::text`] must produce a different
/// identity.
#[derive(Debug, Clone)]
pub struct BuildIdentity {
    /// `sha256:<hex>` over [`BuildIdentity::text`].
    pub digest: String,
    /// `sha256:<hex>` of the analyzed source snapshot (paths + contents).
    pub source_digest: String,
    /// `sha256:<hex>` of the resolved dependencies (from the loader).
    pub dependencies_digest: String,
    pub crate_name: String,
    /// `bin`, `lib` or `tests`.
    pub crate_kind: &'static str,
    pub test_mode: bool,
    pub release: bool,
    /// `3` for `--release`, `0` otherwise.
    pub opt_level: &'static str,
    /// `host` or the rustc triple.
    pub target: &'static str,
    /// `rustc --version`, or `unknown` when the toolchain is absent.
    pub rustc: String,
    /// `ravum-lang-codegen <version>`.
    pub generator: String,
}

impl BuildIdentity {
    /// Directory name under `<package>/.ravum/build/`: the first 16 hex digits of
    /// the digest.
    ///
    /// The full 64-hex digest would buy nothing on disk and would push the rustc
    /// `-o`/input paths closer to Windows' `MAX_PATH` in deep package directories
    /// (that overflow is what produced spurious `link.exe` `LNK1181` failures for
    /// `%TEMP%` packages). The full digest is recorded in `build` output and in the
    /// artifact manifest, so a shortened directory can never be mistaken for the
    /// identity itself.
    pub fn directory(&self) -> String {
        self.digest
            .strip_prefix("sha256:")
            .unwrap_or(&self.digest)
            .chars()
            .filter(|ch| ch.is_ascii_hexdigit())
            .take(16)
            .collect()
    }

    /// Canonical, labeled text the identity digest is computed over. Every line
    /// is a distinct input, so no two differing builds can share a digest by
    /// concatenation ambiguity.
    fn text(&self) -> String {
        let mut text = String::new();
        text.push_str("ravum-build-identity\n");
        text.push_str(&format!("version: {BUILD_IDENTITY_VERSION}\n"));
        text.push_str(&format!("source-digest: {}\n", self.source_digest));
        text.push_str(&format!(
            "dependencies-digest: {}\n",
            self.dependencies_digest
        ));
        text.push_str(&format!("crate-name: {}\n", self.crate_name));
        text.push_str(&format!("crate-kind: {}\n", self.crate_kind));
        text.push_str(&format!("test-mode: {}\n", self.test_mode));
        text.push_str(&format!("release: {}\n", self.release));
        text.push_str(&format!("opt-level: {}\n", self.opt_level));
        text.push_str("overflow-checks: on\n");
        text.push_str("debuginfo: 2\n");
        text.push_str(&format!("target: {}\n", self.target));
        text.push_str(&format!("rustc: {}\n", self.rustc));
        text.push_str(&format!("generator: {}\n", self.generator));
        text
    }
}

/// Content digest of the *analyzed* source snapshot: the in-memory texts the
/// checker saw (files are sorted by path so the digest is order-independent),
/// never mtimes and never the state of the disk.
pub fn source_snapshot_digest(sources: &SourceMap) -> String {
    let entries = sources
        .files()
        .iter()
        .map(|file| (file.path.as_str(), file.text.as_str()));
    snapshot_digest(entries)
}

fn snapshot_digest<'a>(files: impl Iterator<Item = (&'a str, &'a str)>) -> String {
    let mut entries: Vec<(&str, &str)> = files.collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    let mut text = String::new();
    for (path, content) in entries {
        text.push_str(path);
        text.push('\u{0}');
        text.push_str(&ravum_lang_sema::loader::sha256_hex(content.as_bytes()));
        text.push('\n');
    }
    format!(
        "sha256:{}",
        ravum_lang_sema::loader::sha256_hex(text.as_bytes())
    )
}

/// Re-run the loader on the same target and digest the snapshot it produces now.
///
/// The loader records *display* paths relative to each package root (a local
/// dependency's manifest shows up as `util/ravum.toml`, which is not resolvable
/// from the dependent package), so the analysed file list cannot simply be
/// re-read from disk. Re-loading answers the publication question exactly — "do
/// the sources still produce the snapshot this artifact was generated from?" —
/// and covers every analysed file, including local path dependencies and the
/// package manifest. It parses but does not check, so the cost is a second parse
/// of the package, paid once per build.
fn current_snapshot_digest(frontend: &Frontend) -> Result<String, String> {
    let reloaded = load_package_with_options(
        &frontend.target,
        &LoadOptions {
            locked: false,
            overrides: std::collections::BTreeMap::new(),
        },
    );
    if reloaded.diags.has_errors() {
        return Err(format!(
            "re-loading `{}` produced {} error(s), so the artifact can no longer be \
             matched to the sources",
            frontend.target.display(),
            reloaded.diags.error_count()
        ));
    }
    Ok(source_snapshot_digest(&reloaded.sources))
}

/// Compute the deterministic identity of a build (`options` + `frontend`).
pub fn build_identity(frontend: &Frontend, options: &BuildOptions) -> BuildIdentity {
    let crate_name = util::rust_crate_name(frontend.pkg.package_name.as_deref());
    let is_library = !options.test_mode && frontend.pkg.crate_kind == Some(CrateKind::Lib);
    let crate_kind = if options.test_mode {
        "tests"
    } else if is_library {
        "lib"
    } else {
        "bin"
    };
    let identity = BuildIdentity {
        digest: String::new(),
        source_digest: source_snapshot_digest(&frontend.pkg.sources),
        dependencies_digest: frontend.pkg.dependencies_digest.clone(),
        crate_name,
        crate_kind,
        test_mode: options.test_mode,
        release: options.release,
        opt_level: if options.release { "3" } else { "0" },
        target: options.target.rustc_triple().unwrap_or("host"),
        rustc: rustc_version().unwrap_or_else(|| "unknown".to_string()),
        generator: format!("ravum-lang-codegen {GENERATOR_VERSION}"),
    };
    let digest = format!(
        "sha256:{}",
        ravum_lang_sema::loader::sha256_hex(identity.text().as_bytes())
    );
    BuildIdentity { digest, ..identity }
}

#[derive(Debug)]
pub struct Artifacts {
    /// Published artifact (`<package>/.ravum/bin/<name>[.exe|.wasm]`, or
    /// `<package>/.ravum/lib/<name>.rlib`) — unchanged, documented path.
    pub binary: PathBuf,
    /// Generated Rust: `<package>/.ravum/gen/{main,tests}.rs`, or the explicit
    /// `--emit-rust` path when one was given.
    pub generated: PathBuf,
    pub target: BuildTarget,
    /// Deterministic identity of this build.
    pub identity: BuildIdentity,
    /// `<artifact>.build.json` next to the published artifact.
    pub manifest: PathBuf,
    /// Isolated intermediate directory, `.ravum/build/<identity-16-hex>/`.
    pub build_dir: PathBuf,
}

#[derive(Debug)]
pub enum BuildError {
    NoProgram,
    Codegen(String),
    Write(String),
    ToolMissing(String),
    CompileFailed(String),
    Cache(String),
    Unsupported(String),
    /// The sources changed while rustc was running: the result is stale and was
    /// not published.
    Stale(String),
}

impl BuildError {
    pub fn exit_code(&self) -> i32 {
        match self {
            BuildError::ToolMissing(_) => util::EXIT_TOOL,
            _ => util::EXIT_FAIL,
        }
    }

    pub fn message(&self) -> String {
        match self {
            BuildError::NoProgram => {
                "no checked program: fix the diagnostics before building".to_string()
            }
            BuildError::Codegen(message) => format!("code generation failed: {message}"),
            BuildError::Write(message) => message.clone(),
            BuildError::ToolMissing(tool) => format!(
                "`{tool}` was not found; install a Rust toolchain (or set RAVUM_RUSTC) to \
                 build and run Ravum programs"
            ),
            BuildError::CompileFailed(output) => {
                format!("rustc failed:\n{output}")
            }
            BuildError::Cache(message) => message.clone(),
            BuildError::Unsupported(message) => message.clone(),
            BuildError::Stale(message) => format!("stale build rejected: {message}"),
        }
    }

    pub fn print(&self) {
        util::err(self.message());
    }
}

pub fn rustc_command() -> String {
    std::env::var("RAVUM_RUSTC").unwrap_or_else(|_| "rustc".to_string())
}

pub fn rustc_version() -> Option<String> {
    let rustc = rustc_command();
    let output = Command::new(&rustc).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Node.js executable used to run `wasm32-wasip1` programs (`RAVUM_NODE` overrides).
pub fn node_command() -> String {
    std::env::var("RAVUM_NODE").unwrap_or_else(|_| "node".to_string())
}

pub fn node_version() -> Option<String> {
    let node = node_command();
    let output = Command::new(&node).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// File name of the published artifact (also used for the manifest).
fn artifact_file_name(
    frontend: &Frontend,
    options: &BuildOptions,
    crate_name: &str,
    is_library: bool,
) -> String {
    if options.target.is_wasm() {
        if options.test_mode {
            format!("{crate_name}-tests.wasm")
        } else {
            format!("{crate_name}.wasm")
        }
    } else if options.test_mode {
        format!("{crate_name}-tests{}", exe_suffix())
    } else if is_library {
        format!("{crate_name}.rlib")
    } else {
        util::executable_name(frontend.pkg.package_name.as_deref())
    }
}

pub fn compile_program(
    frontend: &Frontend,
    options: &BuildOptions,
) -> Result<Artifacts, BuildError> {
    let checked = frontend.checked.as_ref().ok_or(BuildError::NoProgram)?;
    let root = frontend.root();
    let cache = util::ensure_cache_dir(&root).map_err(BuildError::Cache)?;
    let crate_name = util::rust_crate_name(frontend.pkg.package_name.as_deref());
    let is_library = !options.test_mode && frontend.pkg.crate_kind == Some(CrateKind::Lib);
    if options.target.is_wasm() && is_library {
        return Err(BuildError::Unsupported(
            "`--target wasm32-wasip1` is only supported for binary crates; \
             library packages build for the host"
                .to_string(),
        ));
    }

    let identity = build_identity(frontend, options);

    // Isolated compilation directory, keyed by identity. Concurrent builds of
    // *different* identities can never touch each other's files; concurrent
    // builds of the *same* identity get their own short `out/<pid>-<n>/`
    // directory, because rustc writes `-o` non-atomically. Every added path
    // component is kept short on purpose: Windows' MSVC linker chokes when the
    // full path approaches `MAX_PATH`.
    let identity_dir = cache.join("build").join(identity.directory());
    let gen_dir = identity_dir.join("gen");
    let out_dir =
        identity_dir
            .join("out")
            .join(format!("{}-{}", std::process::id(), next_invocation_id()));
    for dir in [&gen_dir, &out_dir] {
        std::fs::create_dir_all(dir).map_err(|error| {
            BuildError::Write(format!("cannot create `{}`: {error}", dir.display()))
        })?;
    }

    let gen_options = GenOptions {
        source_comments: true,
        test_mode: options.test_mode,
        crate_name: crate_name.clone(),
    };
    let rust =
        generate(checked, &gen_options).map_err(|error| BuildError::Codegen(error.message))?;

    // Intermediate generated Rust: inside the identity directory. Its bytes are
    // deterministic for the identity, so two concurrent writers agree and the
    // atomic write keeps readers on complete files.
    let generated_file = if options.test_mode {
        "tests.rs"
    } else {
        "main.rs"
    };
    let intermediate = gen_dir.join(generated_file);
    util::write_bytes_atomic(&intermediate, rust.as_bytes()).map_err(BuildError::Write)?;

    let file_name = artifact_file_name(frontend, options, &crate_name, is_library);
    let stable_dir = if is_library {
        cache.join("lib")
    } else {
        cache.join("bin")
    };
    let stable_artifact = stable_dir.join(&file_name);
    let intermediate_artifact = out_dir.join(&file_name);

    let rustc = rustc_command();
    let mut command = Command::new(&rustc);
    command
        .arg("--edition")
        .arg("2021")
        .arg("-C")
        .arg("overflow-checks=on")
        .arg("-C")
        .arg("debuginfo=2")
        .arg("-C")
        .arg(if options.release {
            "opt-level=3"
        } else {
            "opt-level=0"
        })
        .arg("--crate-name")
        .arg(&crate_name);
    if let Some(triple) = options.target.rustc_triple() {
        command.arg("--target").arg(triple);
    }
    if is_library {
        command.arg("--crate-type").arg("lib");
    }
    // Never hand a Windows verbatim (`\\?\`) path to rustc: it forwards the
    // output/object paths to `link.exe`, which fails with
    // "LNK1181: cannot open input file '.obj'" on some path shapes. The cache
    // directory already comes back non-verbatim; this is the explicit guarantee.
    command
        .arg("-o")
        .arg(util::strip_verbatim(&intermediate_artifact))
        .arg(util::strip_verbatim(&intermediate));

    match command.output() {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            let mut text = String::new();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            let stdout = String::from_utf8_lossy(&output.stdout);
            if !stdout.trim().is_empty() {
                text.push_str(&stdout);
            }
            return Err(BuildError::CompileFailed(text));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(BuildError::ToolMissing(rustc));
        }
        Err(error) => {
            return Err(BuildError::CompileFailed(format!(
                "cannot execute `{rustc}`: {error}"
            )));
        }
    }

    // Stale guard: the artifact is only valid for the snapshot it was generated
    // from. Re-load the package now and compare the snapshot digest; if it
    // changed while rustc ran, refuse to publish anything.
    let current_digest =
        current_snapshot_digest(frontend).map_err(|message| BuildError::Stale(message))?;
    if current_digest != identity.source_digest {
        return Err(BuildError::Stale(format!(
            "the sources changed during the build (analysed {}, now {}); nothing was \
             published and the previous artifact is untouched",
            identity.source_digest, current_digest
        )));
    }

    // Publication: atomic rename of the artifact, then of the generated text we
    // keep for the documented `.ravum/gen/` path, then the manifest. Paths handed
    // to rustc/link are plain (no Windows `\\?\` verbatim prefix), which the MSVC
    // linker mis-parses.
    util::publish_rename(&intermediate_artifact, &stable_artifact).map_err(BuildError::Write)?;
    let stable_generated = cache.join("gen").join(generated_file);
    util::write_bytes_atomic(&stable_generated, rust.as_bytes()).map_err(BuildError::Write)?;
    let generated = match &options.emit_rust {
        Some(path) => {
            util::write_bytes_atomic(path, rust.as_bytes()).map_err(BuildError::Write)?;
            path.clone()
        }
        None => stable_generated,
    };
    let manifest = stable_artifact.with_file_name(format!("{file_name}.build.json"));
    let manifest_json = build_manifest(&identity, options, &crate_name, is_library, &file_name);
    util::write_bytes_atomic(&manifest, manifest_json.as_bytes()).map_err(BuildError::Write)?;

    Ok(Artifacts {
        binary: stable_artifact,
        generated,
        target: options.target,
        identity,
        manifest,
        build_dir: identity_dir,
    })
}

/// JSON manifest published next to the artifact, so a caller can check that the
/// artifact matches the sources and options it claims.
fn build_manifest(
    identity: &BuildIdentity,
    options: &BuildOptions,
    crate_name: &str,
    is_library: bool,
    file_name: &str,
) -> String {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0);
    let manifest = json!({
        "schema": 1,
        "identity": identity.digest,
        "source_digest": identity.source_digest,
        "dependencies_digest": identity.dependencies_digest,
        "artifact": file_name,
        "crate_name": crate_name,
        "crate_kind": identity.crate_kind,
        "test_mode": identity.test_mode,
        "release": options.release,
        "opt_level": identity.opt_level,
        "overflow_checks": "on",
        "debuginfo": "2",
        "target": identity.target,
        "generator": identity.generator,
        "rustc": identity.rustc,
        "library": is_library,
        "built_at_unix_ms": now_ms,
    });
    let mut text =
        serde_json::to_string_pretty(&manifest).unwrap_or_else(|_| "{\"schema\":1}".to_string());
    text.push('\n');
    text
}

/// Monotonic per-process invocation counter, used to give each `compile_program`
/// call its own output directory even inside one process.
fn next_invocation_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn exe_suffix() -> &'static str {
    if cfg!(windows) {
        ".exe"
    } else {
        ""
    }
}

// ---------------------------------------------------------------------------
// Facts dump (`build --emit-ir`, explicitly not a stable IR)
// ---------------------------------------------------------------------------

pub fn ir_dump(frontend: &Frontend) -> String {
    let mut out = String::new();
    out.push_str("// ravum-lang facts dump — unstable, NOT a stable IR (Core 1.0 draft)\n");
    out.push_str(&format!(
        "package: {}\n",
        frontend.pkg.package_name.as_deref().unwrap_or("<none>")
    ));
    out.push_str(&format!("crate: {}\n", crate_name(frontend)));
    let Some(checked) = &frontend.checked else {
        out.push_str("no checked program\n");
        return out;
    };
    let facts = &checked.facts;

    out.push_str("functions:\n");
    let mut functions: Vec<_> = facts.fn_sigs.values().collect();
    functions.sort_by(|a, b| a.rust_path.cmp(&b.rust_path));
    for signature in functions {
        let params = signature
            .params
            .iter()
            .map(|param| format!("{}: {}", param.name, param.ty))
            .collect::<Vec<_>>()
            .join(", ");
        let declared = match &signature.declared_effects {
            Some(set) => format!(" declared({})", set.display()),
            None => String::new(),
        };
        out.push_str(&format!(
            "  {}({}) -> {} effects: {}{}{}\n",
            signature.rust_path,
            params,
            signature.ret,
            signature.effects.display(),
            declared,
            if signature.is_test { " [test]" } else { "" }
        ));
    }

    out.push_str("structs:\n");
    let mut structs: Vec<_> = facts.structs.values().collect();
    structs.sort_by(|a, b| a.rust_path.cmp(&b.rust_path));
    for definition in structs {
        let fields = definition
            .fields
            .iter()
            .map(|field| {
                format!(
                    "{}: {}{}",
                    field.name,
                    field.ty,
                    if field.public { "" } else { " (private)" }
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!(
            "  {}{} {{ {} }}{}\n",
            definition.rust_path,
            generic_suffix(&definition.generics),
            fields,
            if definition.is_copy { " [Copy]" } else { "" }
        ));
    }

    out.push_str("enums:\n");
    let mut enums: Vec<_> = facts.enums.values().collect();
    enums.sort_by(|a, b| a.rust_path.cmp(&b.rust_path));
    for definition in enums {
        let variants = definition
            .variants
            .iter()
            .map(|variant| variant.name.clone())
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!(
            "  {}{} {{ {} }}\n",
            definition.rust_path,
            generic_suffix(&definition.generics),
            variants
        ));
    }

    out.push_str("consts:\n");
    let mut consts: Vec<_> = facts.consts.values().collect();
    consts.sort_by(|a, b| a.rust_path.cmp(&b.rust_path));
    for definition in consts {
        out.push_str(&format!(
            "  {}: {} = {:?}\n",
            definition.rust_path, definition.ty, definition.value
        ));
    }

    out.push_str("dependencies:\n");
    for dependency in &frontend.pkg.dependencies {
        out.push_str(&format!(
            "  {} {} {}\n",
            dependency.name, dependency.version, dependency.digest
        ));
    }
    out
}

fn generic_suffix(generics: &[String]) -> String {
    if generics.is_empty() {
        String::new()
    } else {
        format!("<{}>", generics.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ravum_lang_syntax::source::SourceMap;

    struct Scratch {
        path: PathBuf,
    }

    impl Scratch {
        fn new(tag: &str) -> Scratch {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0);
            let path = std::env::temp_dir().join(format!(
                "ravum-pipeline-{tag}-{}-{nanos}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).expect("create scratch dir");
            Scratch { path }
        }

        fn package(&self, name: &str, source: &str) -> PathBuf {
            let dir = self.path.join(name);
            std::fs::create_dir_all(dir.join("src")).expect("create package");
            std::fs::write(
                dir.join("ravum.toml"),
                format!(
                    "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n"
                ),
            )
            .expect("write manifest");
            std::fs::write(dir.join("src/main.rv"), source).expect("write source");
            dir
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn snapshot(files: &[(&str, &str)]) -> SourceMap {
        let mut sources = SourceMap::new();
        for (path, text) in files {
            sources.add(*path, *text);
        }
        sources
    }

    #[test]
    fn snapshot_digest_is_deterministic_and_order_independent() {
        let first = snapshot(&[("src/main.rv", "fn main() {}\n")]);
        let second = snapshot(&[("src/main.rv", "fn main() {}\n")]);
        assert_eq!(
            source_snapshot_digest(&first),
            source_snapshot_digest(&second)
        );
        assert!(source_snapshot_digest(&first).starts_with("sha256:"));

        // Adding a file, or changing a byte, changes the digest.
        let more = snapshot(&[
            ("src/main.rv", "fn main() {}\n"),
            ("src/lib.rv", "fn helper() {}\n"),
        ]);
        assert_ne!(
            source_snapshot_digest(&first),
            source_snapshot_digest(&more)
        );
        let changed = snapshot(&[("src/main.rv", "fn main() { }\n")]);
        assert_ne!(
            source_snapshot_digest(&first),
            source_snapshot_digest(&changed)
        );
    }

    #[test]
    fn identity_is_stable_and_rejects_every_differing_input() {
        if rustc_version().is_none() {
            eprintln!("SKIP: rustc is not available");
            return;
        }
        let scratch = Scratch::new("identity");
        let dir = scratch.package(
            "identity-demo",
            "fn main() {\n    io::println(\"hi\");\n}\n",
        );
        let frontend = analyze(&dir, Profile::Core, false);
        assert!(frontend.success(), "the test package must check");

        let debug = BuildOptions::default();
        let first = build_identity(&frontend, &debug);
        let second = build_identity(&frontend, &debug);
        assert_eq!(first.digest, second.digest, "same inputs, same identity");
        assert_eq!(first.source_digest, second.source_digest);
        assert!(
            first.directory().chars().all(|ch| ch.is_ascii_hexdigit()),
            "the identity directory is plain hex: {}",
            first.directory()
        );
        assert_eq!(
            first.directory().len(),
            16,
            "a short hex prefix keeps rustc/link paths away from MAX_PATH"
        );

        // Every documented identity input must influence the digest.
        let release = BuildOptions {
            release: true,
            ..BuildOptions::default()
        };
        assert_ne!(first.digest, build_identity(&frontend, &release).digest);
        let tests = BuildOptions {
            test_mode: true,
            ..BuildOptions::default()
        };
        assert_ne!(first.digest, build_identity(&frontend, &tests).digest);
        let wasm = BuildOptions {
            target: BuildTarget::Wasm32WasiP1,
            ..BuildOptions::default()
        };
        assert_ne!(first.digest, build_identity(&frontend, &wasm).digest);

        // A changed source snapshot changes the identity.
        let changed_frontend = {
            std::fs::write(
                dir.join("src/main.rv"),
                "fn main() {\n    io::println(\"bye\");\n}\n",
            )
            .expect("rewrite source");
            analyze(&dir, Profile::Core, false)
        };
        assert_ne!(
            first.digest,
            build_identity(&changed_frontend, &debug).digest
        );
        assert_ne!(
            first.source_digest,
            changed_frontend.pkg.sources.files()[0].text
        );
    }

    #[test]
    fn publication_guard_detects_a_source_change() {
        let scratch = Scratch::new("guard");
        let dir = scratch.package("guard-demo", "fn main() {}\n");
        let frontend = analyze(&dir, Profile::Core, false);
        let before = current_snapshot_digest(&frontend).expect("re-load");
        assert_eq!(before, source_snapshot_digest(&frontend.pkg.sources));

        std::fs::write(dir.join("src/main.rv"), "fn main() { let x = 1; }\n")
            .expect("rewrite source");
        let after = current_snapshot_digest(&frontend).expect("re-load");
        assert_ne!(
            before, after,
            "the guard must see a source changed after the analysis"
        );

        // A deleted source cannot be verified: the guard must fail closed, not
        // report the old snapshot as current.
        std::fs::remove_file(dir.join("src/main.rv")).expect("delete source");
        assert!(
            current_snapshot_digest(&frontend).is_err(),
            "a missing source must be an explicit stale failure"
        );
    }

    #[test]
    fn local_dependency_sources_are_part_of_the_snapshot() {
        // The loader records a dependency manifest as `util/ravum.toml`, which is
        // not resolvable from the dependent root; the guard must therefore
        // re-load instead of re-reading. This test pins the re-load digest.
        let scratch = Scratch::new("dependency");
        let app = scratch.path.join("app");
        let util = scratch.path.join("util");
        std::fs::create_dir_all(app.join("src")).expect("create app");
        std::fs::create_dir_all(util.join("src")).expect("create util");
        std::fs::write(
            app.join("ravum.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n\n\
             [dependencies]\nutil = { path = \"../util\" }\n",
        )
        .expect("write app manifest");
        std::fs::write(
            util.join("ravum.toml"),
            "[package]\nname = \"util\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
        )
        .expect("write util manifest");
        std::fs::write(
            util.join("src/lib.rv"),
            "pub fn answer() -> i32 {\n    return 42;\n}\n",
        )
        .expect("write dependency");
        std::fs::write(
            app.join("src/main.rv"),
            "fn main() {\n    let n = util::answer();\n}\n",
        )
        .expect("write app");

        let frontend = analyze(&app, Profile::Core, false);
        assert!(
            frontend.success(),
            "the dependent package must check:\n{}",
            frontend.diags.render(&frontend.pkg.sources)
        );
        let digest = current_snapshot_digest(&frontend).expect("re-load the package");
        assert_eq!(
            digest,
            source_snapshot_digest(&frontend.pkg.sources),
            "the guard must be able to re-read a local dependency"
        );

        // Changing the dependency must be detected as a stale snapshot.
        std::fs::write(
            util.join("src/lib.rv"),
            "pub fn answer() -> i32 {\n    return 43;\n}\n",
        )
        .expect("rewrite dependency");
        assert_ne!(
            digest,
            current_snapshot_digest(&frontend).expect("re-load the package")
        );
    }
}
