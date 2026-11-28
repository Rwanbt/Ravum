//! `ravum-lang watch --backend jit` — continuous compilation on the native
//! development backend (gates T15/T16, V1).
//!
//! On every saved change of the watched `.rv` file:
//!
//! 1. parse + check the file (the checker is the authority, P0-C);
//! 2. lower it to IR v1.1 and hand it to the hot-swap engine, which compiles
//!    **only the functions whose code changed** (`ravum-lang-jit::hot`);
//! 3. activate the new version and run the entry once, printing its result.
//!
//! Fail-closed (INV-03): a file that does not parse, does not check, uses a
//! construct the IR does not cover, or changes an incompatible entry is
//! reported and **the previous version stays active**.
//!
//! Scope: a package whose `src/` holds module files (all of them, loaded like
//! the language server does: parse cache, stable file ids, incremental check
//! of the whole package; a binary package needs its `main`, as for `build`),
//! or a single-file program (`src/main.rv`, else `src/lib.rv`, or the file
//! given), checked as a library since the session runs an entry. Only what
//! the entry reaches must be covered by the IR (`lower_checked_for`). Entry: `process_sample` when the file declares a DSP block
//! (the block is driven with a deterministic test signal: no audio device),
//! otherwise `--entry NAME` (default `main`), a function without parameters;
//! what it prints with `io::print*` is shown before its result.
//! The default backend (`rustc`) remains the reference for builds.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use ravum_lang_ir::lower::LowerError;
use ravum_lang_ir::CompileError;
use ravum_lang_jit::hot::{HotEngine, RtRunner};
use ravum_lang_sema::loader::{load_package_with_parse_cache, LoadOptions, ParseCache};
use ravum_lang_sema::syntax::{parser, FileId, Severity, SourceMap, LANGUAGE_EDITION};
use ravum_lang_sema::{check, CheckOptions, Profile};

use crate::util;

const POLL: Duration = Duration::from_millis(100);
const DSP_ENTRY: &str = "process_sample";
/// Samples processed after each accepted version of a DSP block.
const DSP_SAMPLES: usize = 4096;

fn say(line: &str) {
    use std::io::Write;
    println!("{line}");
    let _ = std::io::stdout().flush();
}

fn source_file(path: &Path) -> Result<PathBuf, String> {
    if path.is_file() {
        return Ok(path.to_path_buf());
    }
    for rel in ["src/main.rv", "src/lib.rv"] {
        let p = path.join(rel);
        if p.is_file() {
            return Ok(p);
        }
    }
    Err(format!(
        "`{}` has no `src/main.rv` or `src/lib.rv` (the jit backend watches a single-file program)",
        path.display()
    ))
}

fn stamp(p: &Path) -> Option<(SystemTime, u64)> {
    let m = std::fs::metadata(p).ok()?;
    Some((m.modified().ok()?, m.len()))
}

/// Human diagnostics (`file:line:col: code: message`) for a rejected file.
fn diagnostics(file: &Path, src: &str) -> Vec<String> {
    // Same checking mode as `compile_source` (a library: the jit backend runs
    // an entry, not a binary `main`), shown under the real file name.
    const VIRTUAL: &str = "src/lib.rv";
    let mut sources = SourceMap::new();
    let id = sources.add(VIRTUAL, src);
    let parsed = parser::parse_file(id, src);
    let mut out: Vec<String> = Vec::new();
    let mut push = |sources: &SourceMap, d: &ravum_lang_sema::syntax::Diagnostic| {
        let at = sources
            .location(d.span)
            .replacen(VIRTUAL, &file.display().to_string(), 1);
        out.push(format!("{at}: {}: {}", d.code, d.message));
    };
    if !parsed.diags.is_empty() {
        for d in parsed.diags.iter() {
            push(&sources, d);
        }
        return out;
    }
    let options = CheckOptions {
        profile: Profile::Core,
        edition: LANGUAGE_EDITION.to_string(),
    };
    for d in check(&parsed.program, &sources, &options).diags.iter() {
        if d.severity == Severity::Error {
            push(&sources, d);
        }
    }
    out
}

/// Deterministic test signal for a DSP block (no audio device involved).
fn test_signal(n: usize) -> Vec<f32> {
    let mut s: u32 = 0x2545_f491;
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            (s % 2001) as f32 / 1000.0 - 1.0
        })
        .collect()
}

struct Session {
    engine: HotEngine,
    runner: RtRunner,
    entry: String,
    dsp: bool,
}

impl Session {
    fn exercise(&mut self) {
        if self.dsp {
            let x = test_signal(DSP_SAMPLES);
            let (mut peak, mut energy) = (0.0f32, 0.0f64);
            for v in &x {
                let y = self.runner.process_sample(*v);
                peak = peak.max(y.abs());
                energy += (y as f64) * (y as f64);
            }
            let rms = (energy / DSP_SAMPLES as f64).sqrt();
            say(&format!(
                "[ravum] {}: {DSP_SAMPLES} samples, peak {peak:.6}, rms {rms:.6} (state kept across versions)",
                self.entry
            ));
            let rolled = self
                .engine
                .mailbox()
                .rolled_back_generation
                .load(std::sync::atomic::Ordering::SeqCst);
            if rolled != 0 {
                say(&format!(
                    "[ravum] generation {rolled} trapped and was rolled back"
                ));
            }
        } else {
            let (outcome, out) = self.runner.run_nullary_with_output(self.engine.messages());
            // The program's own output, as it printed it, then the result.
            if !out.stdout.is_empty() {
                use std::io::Write;
                print!("{}", out.stdout);
                let _ = std::io::stdout().flush();
            }
            if !out.stderr.is_empty() {
                eprint!("{}", out.stderr);
            }
            say(&format!("[ravum] {}() -> {}", self.entry, outcome.render()));
        }
    }
}

/// T13: [`ravum_lang_ir::compile_source`], with the check done by
/// [`ravum_lang_incr::package::check_package`] (same virtual `src/lib.rv`,
/// same options), reusing the unchanged bodies of the previous text; the
/// facts are complete, so the IR is the same as a full check's.
fn compile_incremental(
    src: &str,
    entry: Option<&str>,
    state: &mut Option<ravum_lang_incr::package::PackageState>,
) -> Result<ravum_lang_ir::Module, CompileError> {
    let mut sources = SourceMap::new();
    let id = sources.add("src/lib.rv", src);
    let parsed = parser::parse_file(id, src);
    if !parsed.diags.is_empty() {
        *state = None;
        return Err(CompileError::Rejected(
            parsed.diags.iter().map(|d| d.code.clone()).collect(),
        ));
    }
    let options = CheckOptions {
        profile: Profile::Core,
        edition: LANGUAGE_EDITION.to_string(),
    };
    let (result, next, stats) = ravum_lang_incr::package::check_package(
        state.as_ref(),
        &parsed.program,
        &sources,
        &options,
    );
    *state = Some(next);
    say(&format!(
        "[ravum] check: {} bodies reused, {} checked{}",
        stats.reused_bodies,
        stats.checked_bodies,
        stats
            .full
            .map(|r| format!(" (full: {r})"))
            .unwrap_or_default()
    ));
    let errors: Vec<String> = result
        .diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.code.clone())
        .collect();
    match result.checked {
        Some(checked) if errors.is_empty() => lower_for_session(&checked, entry),
        _ => Err(CompileError::Rejected(errors)),
    }
}

/// The entries the session needs: the one it runs (`--entry`, else
/// `process_sample` when the root declares a DSP block, else `main`), plus
/// the root's reload-contract functions the engine calls itself (`init`,
/// `step`, `migrate`, HR-1). Only what they reach is lowered
/// (`lower_checked_for`).
fn entries_of(checked: &ravum_lang_sema::CheckedProgram, entry: Option<&str>) -> Vec<String> {
    let root: Vec<&str> = checked
        .program
        .root
        .items
        .iter()
        .filter_map(|i| match &i.kind {
            ravum_lang_sema::syntax::ast::ItemKind::Fn(f) => Some(f.name.name.as_str()),
            _ => None,
        })
        .collect();
    let main = match entry {
        Some(e) => e,
        None if root.contains(&DSP_ENTRY) => DSP_ENTRY,
        None => "main",
    };
    let mut out = vec![main.to_string()];
    for extra in ["init", "step", "migrate"] {
        if extra != main && root.contains(&extra) {
            out.push(extra.to_string());
        }
    }
    out
}

fn lower_for_session(
    checked: &ravum_lang_sema::CheckedProgram,
    entry: Option<&str>,
) -> Result<ravum_lang_ir::Module, CompileError> {
    let entries = entries_of(checked, entry);
    let refs: Vec<&str> = entries.iter().map(String::as_str).collect();
    ravum_lang_ir::lower_checked_for(checked, &refs)
}

/// A watched package: everything kept across edits so that an edit costs
/// what it changed (parses of untouched files, their `FileId`s, the checked
/// bodies).
struct PackageWatch {
    root: PathBuf,
    cache: ParseCache,
    file_ids: std::collections::BTreeMap<PathBuf, FileId>,
}

/// T13 on a package: load it (parse cache, stable file ids), check it
/// incrementally, lower it. A rejection carries the rendered diagnostics.
fn compile_package(
    w: &mut PackageWatch,
    entry: Option<&str>,
    state: &mut Option<ravum_lang_incr::package::PackageState>,
) -> Result<ravum_lang_ir::Module, (CompileError, Vec<String>)> {
    let options = LoadOptions {
        locked: false,
        overrides: Default::default(),
        file_ids: Some(w.file_ids.clone()),
    };
    let package = load_package_with_parse_cache(&w.root, &options, &mut w.cache);
    for (path, id) in &package.file_ids {
        w.file_ids.insert(path.clone(), *id);
    }
    let render =
        |sources: &SourceMap,
         ds: &mut dyn Iterator<Item = &ravum_lang_sema::syntax::Diagnostic>| {
            ds.filter(|d| d.severity == Severity::Error)
                .map(|d| format!("{}: {}: {}", sources.location(d.span), d.code, d.message))
                .collect::<Vec<String>>()
        };
    let rejected = |lines: Vec<String>| {
        let codes = lines.clone();
        (CompileError::Rejected(codes), lines)
    };
    let program = match &package.program {
        Some(p) if !package.diags.has_errors() => p,
        _ => {
            *state = None;
            return Err(rejected(render(
                &package.sources,
                &mut package.diags.iter(),
            )));
        }
    };
    let options = CheckOptions {
        profile: Profile::Core,
        edition: LANGUAGE_EDITION.to_string(),
    };
    let (result, next, stats) = ravum_lang_incr::package::check_package(
        state.as_ref(),
        program,
        &package.sources,
        &options,
    );
    *state = Some(next);
    say(&format!(
        "[ravum] check: {} bodies reused, {} checked{}",
        stats.reused_bodies,
        stats.checked_bodies,
        stats
            .full
            .map(|r| format!(" (full: {r})"))
            .unwrap_or_default()
    ));
    let errors = render(&package.sources, &mut result.diags.iter());
    match result.checked {
        Some(checked) if errors.is_empty() => {
            lower_for_session(&checked, entry).map_err(|e| (e, Vec::new()))
        }
        _ => Err(rejected(errors)),
    }
}

/// Whether `src/` of a package holds module files besides its root
/// (`main.rv`/`lib.rv`). A one-file package keeps the single-file mode, whose
/// root is checked as a library (the session runs an entry, not `main`).
fn has_modules(root: &Path) -> bool {
    let mut dirs = vec![root.join("src")];
    while let Some(d) = dirs.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
            } else if p.extension().is_some_and(|x| x == "rv")
                && !(d == root.join("src")
                    && matches!(
                        p.file_name().and_then(|n| n.to_str()),
                        Some("main.rv" | "lib.rv")
                    ))
            {
                return true;
            }
        }
    }
    false
}

/// What is watched: one file, or a whole package.
enum Target {
    File(PathBuf),
    Package(PackageWatch),
}

impl Target {
    fn describe(&self) -> String {
        match self {
            Target::File(f) => f.display().to_string(),
            Target::Package(w) => format!("package {}", w.root.display()),
        }
    }

    /// The source files whose changes trigger a rebuild.
    fn files(&self) -> Vec<PathBuf> {
        match self {
            Target::File(f) => vec![f.clone()],
            Target::Package(w) => {
                let mut files = vec![w.root.join("ravum.toml")];
                let mut dirs = vec![w.root.join("src")];
                while let Some(d) = dirs.pop() {
                    let Ok(rd) = std::fs::read_dir(&d) else {
                        continue;
                    };
                    for e in rd.flatten() {
                        let p = e.path();
                        if p.is_dir() {
                            dirs.push(p);
                        } else if p.extension().is_some_and(|x| x == "rv") {
                            files.push(p);
                        }
                    }
                }
                files.sort();
                files
            }
        }
    }

    /// Cheap per-poll view: each file and its (modification time, length).
    fn stamps(&self) -> Vec<(PathBuf, Option<(SystemTime, u64)>)> {
        self.files()
            .into_iter()
            .map(|p| {
                let st = stamp(&p);
                (p, st)
            })
            .collect()
    }

    /// The bytes of every watched file, read only once a stamp moved.
    fn contents(&self) -> Vec<(PathBuf, Option<Vec<u8>>)> {
        self.files()
            .into_iter()
            .map(|p| {
                let b = std::fs::read(&p).ok();
                (p, b)
            })
            .collect()
    }
}

/// One attempt: compile the current text; start or update the session.
/// Returns `true` when a new version became active.
fn attempt(
    target: &mut Target,
    entry_opt: Option<&str>,
    session: &mut Option<Session>,
    incr: &mut Option<ravum_lang_incr::package::PackageState>,
) -> bool {
    let started = Instant::now();
    let compiled = match target {
        Target::Package(w) => compile_package(w, entry_opt, incr),
        Target::File(file) => {
            let Ok(src) = std::fs::read_to_string(&*file) else {
                say(&format!(
                    "[ravum] cannot read {}; previous version kept",
                    file.display()
                ));
                return false;
            };
            compile_incremental(&src, entry_opt, incr).map_err(|e| {
                let lines = match e {
                    CompileError::Rejected(_) => diagnostics(file, &src),
                    _ => Vec::new(),
                };
                (e, lines)
            })
        }
    };
    let ir = match compiled {
        Ok(ir) => ir,
        Err((CompileError::Rejected(_), lines)) => {
            say("[ravum] rejected by the checker; previous version kept");
            for d in lines {
                say(&format!("  {d}"));
            }
            return false;
        }
        Err((CompileError::Lower(LowerError::Unsupported(what)), _)) => {
            say(&format!(
                "[ravum] not supported by the jit backend: {what}; previous version kept \
                 (use the default backend for this program)"
            ));
            return false;
        }
        Err((CompileError::Lower(e), _)) => {
            say(&format!(
                "[ravum] internal lowering error: {e}; previous version kept"
            ));
            return false;
        }
    };
    let front = started.elapsed();
    match session {
        None => {
            let dsp = ir.func_id(DSP_ENTRY).is_some() && entry_opt.is_none();
            let entry = if dsp {
                DSP_ENTRY
            } else {
                entry_opt.unwrap_or("main")
            };
            match HotEngine::start(&ir, entry) {
                Ok((engine, runner)) => {
                    say(&format!(
                        "[ravum] jit: {} functions compiled (front {:.1} ms, total {:.1} ms)",
                        ir.functions.len(),
                        front.as_secs_f64() * 1e3,
                        started.elapsed().as_secs_f64() * 1e3
                    ));
                    let mut s = Session {
                        engine,
                        runner,
                        entry: entry.to_string(),
                        dsp,
                    };
                    s.exercise();
                    *session = Some(s);
                    true
                }
                Err(r) => {
                    say(&format!("[ravum] refused: {r}"));
                    false
                }
            }
        }
        Some(s) => match s.engine.update(&ir) {
            Ok(report) => {
                s.runner.poll();
                if report.migration_pending {
                    // HR-1.7, editor side: the poll above took the state
                    // snapshot; convert it with `migrate`, then adopt.
                    if let Err(r) = s.engine.collect_migration() {
                        say(&format!("[ravum] refused: {r}; previous version kept"));
                        return false;
                    }
                    s.runner.poll();
                    say("[ravum] state migrated with `migrate` (HR-1.7)");
                }
                s.engine.collect();
                let names = if report.recompiled.is_empty() {
                    "none".to_string()
                } else {
                    report.recompiled.join(", ")
                };
                say(&format!(
                    "[ravum] jit: {} recompiled ({names}), {} reused (front {:.1} ms, jit {:.1} ms)",
                    report.recompiled.len(),
                    report.reused,
                    front.as_secs_f64() * 1e3,
                    report.compile_time.as_secs_f64() * 1e3
                ));
                s.exercise();
                true
            }
            Err(r) => {
                say(&format!("[ravum] refused: {r}; previous version kept"));
                false
            }
        },
    }
}

pub fn run(path: &Path, entry: Option<&str>, max_rebuilds: Option<u64>) -> i32 {
    let mut target = if path.is_dir() && path.join("ravum.toml").is_file() && has_modules(path) {
        Target::Package(PackageWatch {
            root: path.to_path_buf(),
            cache: ParseCache::default(),
            file_ids: Default::default(),
        })
    } else {
        match source_file(path) {
            Ok(f) => Target::File(f),
            Err(m) => {
                util::err(m);
                return util::EXIT_FAIL;
            }
        }
    };
    say(&format!(
        "[ravum] watching {} (backend: jit)",
        target.describe()
    ));
    let mut session: Option<Session> = None;
    // T13: the incremental check state, kept across edits.
    let mut incr: Option<ravum_lang_incr::package::PackageState> = None;
    // The bytes last checked: a new stamp with the same contents (a
    // duplicate file-system notification, a touch) is not a change and does
    // not count against `--max-rebuilds`.
    let mut last = target.stamps();
    let mut seen = target.contents();
    attempt(&mut target, entry, &mut session, &mut incr);
    if max_rebuilds == Some(0) {
        return util::EXIT_OK;
    }
    let mut changes: u64 = 0;
    loop {
        std::thread::sleep(POLL);
        let now = target.stamps();
        if now == last {
            continue;
        }
        last = now;
        let bytes = target.contents();
        if bytes == seen {
            continue;
        }
        seen = bytes;
        say("[ravum] change detected");
        attempt(&mut target, entry, &mut session, &mut incr);
        changes += 1;
        if max_rebuilds.is_some_and(|m| changes >= m) {
            return util::EXIT_OK;
        }
    }
}
