//! Shared frontend/build pipeline used by every command.
//!
//! The CLI never invents facts: it loads the package, runs the frozen
//! `sema::check` entry point, and hands the resulting `CheckedProgram` to
//! `codegen::generate`. A stage that is still a stub reports its own internal
//! diagnostic and the command exits 1 — no false success.

use ravum_lang_codegen::{generate, GenOptions};
use ravum_lang_sema::facts::{CheckOptions, CheckedProgram};
use ravum_lang_sema::loader::{load_package_with_options, CrateKind, LoadOptions};
use ravum_lang_sema::{check, LoadedPackage, Profile};
use ravum_lang_syntax::ast::{ItemKind, Module, Program};
use ravum_lang_syntax::diag::DiagnosticBag;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::util;

/// Result of loading + checking one package (both crate roots when present).
pub struct Frontend {
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

#[derive(Debug)]
pub struct Artifacts {
    pub binary: PathBuf,
    pub generated: PathBuf,
    pub target: BuildTarget,
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

pub fn compile_program(
    frontend: &Frontend,
    options: &BuildOptions,
) -> Result<Artifacts, BuildError> {
    let checked = frontend.checked.as_ref().ok_or(BuildError::NoProgram)?;
    let root = frontend.root();
    let cache = util::ensure_cache_dir(&root).map_err(BuildError::Cache)?;
    let crate_name = util::rust_crate_name(frontend.pkg.package_name.as_deref());

    let generated = match &options.emit_rust {
        Some(path) => path.clone(),
        None => cache.join("gen").join(if options.test_mode {
            "tests.rs"
        } else {
            "main.rs"
        }),
    };

    let gen_options = GenOptions {
        source_comments: true,
        test_mode: options.test_mode,
        crate_name: crate_name.clone(),
    };
    let rust =
        generate(checked, &gen_options).map_err(|error| BuildError::Codegen(error.message))?;
    util::write_bytes_atomic(&generated, rust.as_bytes()).map_err(BuildError::Write)?;

    let is_library = !options.test_mode && frontend.pkg.crate_kind == Some(CrateKind::Lib);
    if options.target.is_wasm() && is_library {
        return Err(BuildError::Unsupported(
            "`--target wasm32-wasip1` is only supported for binary crates; \
             library packages build for the host"
                .to_string(),
        ));
    }
    let output_path = if options.target.is_wasm() {
        cache.join("bin").join(if options.test_mode {
            format!("{crate_name}-tests.wasm")
        } else {
            format!("{crate_name}.wasm")
        })
    } else if options.test_mode {
        cache
            .join("bin")
            .join(format!("{crate_name}-tests{}", exe_suffix()))
    } else if is_library {
        cache.join("lib").join(format!("{crate_name}.rlib"))
    } else {
        cache
            .join("bin")
            .join(util::executable_name(frontend.pkg.package_name.as_deref()))
    };
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            BuildError::Write(format!("cannot create `{}`: {error}", parent.display()))
        })?;
    }

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
    command.arg("-o").arg(&output_path).arg(&generated);

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

    Ok(Artifacts {
        binary: output_path,
        generated,
        target: options.target,
    })
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
