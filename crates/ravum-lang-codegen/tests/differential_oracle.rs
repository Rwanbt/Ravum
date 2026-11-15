//! Differential oracle for the reference backend (B): `check` accepted =>
//! the generated Rust must compile and run.
//!
//! The generator is **deterministic** (`seed -> program`) and produces programs
//! that are **well typed by construction**: every statement template is typed,
//! every non-`Copy` value is consumed at most once, and every call/builtin used
//! here is part of Core 1.0. Randomness only picks statements, labels and small
//! literals; it never assembles an ill-typed expression.
//!
//! Properties measured for each generated program:
//!
//! 1. if `check` accepts the program, the generated Rust **compiles** with the
//!    same `rustc` flags as the CLI (`--edition 2021`, `overflow-checks=on`,
//!    `debuginfo=2`, `opt-level=0`);
//! 2. when the program is built and run, its **stdout and exit code are
//!    identical in debug and release** (`opt-level=3`), on a sampled subset
//!    (`RAVUM_DIFF_RELEASE_EVERY`, default 10) to bound the rustc cost;
//! 3. the same seed produces the **same bytes**, and two `check` runs produce
//!    the same diagnostics.
//!
//! On a divergence the program is **reduced** (statements removed one by one
//! while the divergence persists) and written to
//! `target/diff-divergences/`; each divergence must become a conformance case
//! and a line in `docs/qualification/T2-FINDINGS.md`. The generator must never
//! be narrowed to hide a divergence.
//!
//! Modes:
//! * fast (always runs, `cargo test`): `3 seeds x 200 programs`;
//! * long (`#[ignore]`): `3 seeds x 10 000 programs`, overridable with
//!   `RAVUM_DIFF_PROGRAMS`. Run explicitly:
//!   `cargo test -p ravum-lang-codegen --test differential_oracle -- --ignored --nocapture`.

use ravum_lang_codegen::{generate, GenOptions};
use ravum_lang_sema::facts::{CheckOptions, Profile};
use ravum_lang_sema::loader::{load_package_with_options, LoadOptions};
use ravum_lang_sema::{check, syntax};
use std::path::{Path, PathBuf};
use std::time::Instant;

// ---------------------------------------------------------------------------
// Deterministic generator
// ---------------------------------------------------------------------------

struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 {
                0x9E37_79B9_7F4A_7C15
            } else {
                seed
            },
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: usize) -> usize {
        if bound <= 1 {
            0
        } else {
            (self.next_u64() % bound as u64) as usize
        }
    }

    fn literal(&mut self) -> i32 {
        (self.below(9) + 1) as i32
    }
}

/// Fixed declarations. They are emitted for every program (unused items are
/// warnings, not errors, and the CLI-generated file allows them).
const HEADER: &str = r#"use std::io;

struct Pair { a: i32, b: String }
enum Shape { Dot, Line(i32), Rect { w: i32, h: i32 } }

impl Pair {
    fn a_value(&self) -> i32 { return self.a; }
    fn take_b(self) -> String { return self.b; }
    fn set_a(&mut self, value: i32) { self.a = value; }
}

fn make_pair(a: i32, b: String) -> Pair { return Pair { a: a, b: b }; }
fn make_text(n: i32) -> String { return n.to_string(); }
fn len_of(text: &str) -> usize { return text.len(); }
fn incr(value: &mut i32) { *value = *value + 1; }
fn shape_area(shape: Shape) -> i32 {
    match shape {
        Shape::Dot => 0,
        Shape::Line(n) => n,
        Shape::Rect { w: w, h: h } => w * h,
    }
}
fn make_vec() -> Vec<i32> {
    let mut v: Vec<i32> = Vec::new();
    v.push(1);
    v.push(2);
    return v;
}
fn maybe(flag: bool) -> Option<i32> {
    if flag { return Option::Some(3); }
    return Option::None;
}
fn maybe_text(flag: bool) -> Option<String> {
    if flag { return Option::Some(String::from("yes")); }
    return Option::None;
}
struct Holder { p: Pair, n: i32 }
fn make_holder(n: i32) -> Holder {
    return Holder { p: make_pair(n, String::from("h")), n: n };
}
enum Text { Empty, Full(String) }
fn make_text_enum(flag: bool) -> Text {
    if flag { return Text::Full(String::from("t")); }
    return Text::Empty;
}
fn checked(n: i32) -> Result<i32, Error> {
    if n < 0 { return Result::Err(Error::new("negative")); }
    return Result::Ok(n);
}
fn doubled(n: i32) -> Result<i32, Error> {
    let value = checked(n)?;
    return Result::Ok(value * 2);
}
fn first_of(xs: &[i32]) -> i32 {
    return xs[0];
}

"#;

/// Statement templates. `$id` is a unique statement id, `$lit` / `$lit2` are
/// small integer literals. Every template is well typed and leaves `acc: i32`
/// usable; non-`Copy` values are moved at most once.
const TEMPLATES: &[&str] = &[
    // 0: literal
    "let n$id: i32 = $lit;\n    acc = acc + n$id;",
    // 1: String -> borrow -> len
    "let s$id = make_text($lit);\n    let l$id = len_of(s$id.as_str());\n    acc = acc + l$id as i32;",
    // 2: move a String binding
    "let s$id = make_text($lit);\n    let m$id = s$id;\n    let l$id = m$id.len();\n    acc = acc + l$id as i32;",
    // 3: struct field copy
    "let p$id = make_pair($lit, String::from(\"p$id\"));\n    let a$id = p$id.a;\n    acc = acc + a$id;",
    // 4: struct field move (partial move)
    "let p$id = make_pair($lit, String::from(\"q$id\"));\n    let b$id = p$id.b;\n    acc = acc + b$id.len() as i32;",
    // 5: tuple copy + bool
    "let t$id = ($lit, true);\n    let x$id = t$id.0;\n    if t$id.1 { acc = acc + x$id; }",
    // 6: array index copy
    "let a$id = [1, 2, 3];\n    acc = acc + a$id[1];",
    // 7: array element move
    "let a$id = [String::from(\"x$id\"), String::from(\"y$id\")];\n    let e$id = a$id[1];\n    acc = acc + e$id.len() as i32;",
    // 8: if/else expression
    "let v$id = if acc > 0 { acc } else { 0 };\n    acc = acc + v$id;",
    // 9: enum payload call
    "let sh$id = Shape::Line($lit);\n    acc = acc + shape_area(sh$id);",
    // 10: match with struct-like payload bindings
    "let sh$id = Shape::Rect { w: 2, h: 3 };\n    let ar$id = match sh$id { Shape::Dot => 0, Shape::Line(n) => n, Shape::Rect { w: w, h: h } => w * h };\n    acc = acc + ar$id;",
    // 11: while
    "let mut i$id: i32 = 0;\n    while i$id < 3 { acc = acc + i$id; i$id += 1; }",
    // 12: for
    "for i$id in 0..3 { acc = acc + i$id; }",
    // 13: loop/break
    "loop { acc = acc + 1; break; }",
    // 14: method &self
    "let p$id = make_pair($lit, String::from(\"m$id\"));\n    acc = acc + p$id.a_value();",
    // 15: method &mut self
    "let mut p$id = make_pair($lit, String::from(\"n$id\"));\n    p$id.set_a($lit2);\n    acc = acc + p$id.a;",
    // 16: method self (consumes)
    "let p$id = make_pair($lit, String::from(\"o$id\"));\n    let b$id = p$id.take_b();\n    acc = acc + b$id.len() as i32;",
    // 17: &mut argument
    "let mut v$id: i32 = acc;\n    incr(&mut v$id);\n    acc = acc + v$id;",
    // 18: Option + match
    "let o$id = maybe(acc > 0);\n    let v$id = match o$id { Option::Some(x) => x, Option::None => 0 };\n    acc = acc + v$id;",
    // 19: Vec index loop
    "let v$id = make_vec();\n    let mut j$id: usize = 0;\n    while j$id < v$id.len() { acc = acc + v$id[j$id] as i32; j$id += 1; }",
    // 20: shadowing
    "let r$id = acc + 1;\n    let r$id = r$id + 1;\n    acc = acc + r$id;",
    // 21: nested block
    "{ let z$id = acc + 2; acc = acc + z$id; }",
    // 22: nested arrays
    "let n$id = [[1, 2], [3, 4]];\n    acc = acc + n$id[1][0];",
    // 23: partial move in one if branch
    "let p$id = make_pair(1, String::from(\"r$id\"));\n    if acc > 0 { let b$id = p$id.b; acc = acc + b$id.len() as i32; } else { acc = acc + p$id.a; }",
    // 24: tuple element move
    "let t$id = (String::from(\"u$id\"), $lit);\n    let s$id = t$id.0;\n    acc = acc + s$id.len() as i32;",
    // 25: match on unit variant
    "let sh$id = Shape::Dot;\n    let ar$id = match sh$id { Shape::Dot => 0, Shape::Line(n) => n, Shape::Rect { w: w, h: h } => w * h };\n    acc = acc + ar$id;",
    // 26: struct literal with fields out of declaration order
    "let p$id = Pair { b: String::from(\"s$id\"), a: $lit };\n    acc = acc + p$id.a;",
    // 27: projection of a temporary struct
    "acc = acc + make_pair(acc, String::from(\"t$id\")).a;",
    // 28: array of temporaries, move one element
    "let a$id = [make_text($lit), make_text($lit2)];\n    let e$id = a$id[0];\n    acc = acc + e$id.len() as i32;",
    // 29: struct with both fields consumed in order
    "let p$id = make_pair($lit, make_text($lit2));\n    let b$id = p$id.b;\n    acc = acc + b$id.len() as i32;",
    // 30: Result + match
    "let r$id = checked($lit);\n    let v$id = match r$id { Result::Ok(x) => x, Result::Err(e) => 0 };\n    acc = acc + v$id;",
    // 31: `?` propagation through a wrapper
    "let w$id = doubled($lit);\n    let v$id = match w$id { Result::Ok(x) => x, Result::Err(e) => 0 };\n    acc = acc + v$id;",
    // 32: float arithmetic and cast
    "let f$id = $lit as f64 / 2.0;\n    acc = acc + f$id as i32;",
    // 33: array-to-slice coercion at an argument
    "let a$id = [4, 5, 6];\n    acc = acc + first_of(&a$id);",
    // 34: two element moves from the same array (historical P0-A shape)
    "let a$id = [String::from(\"m$id\"), String::from(\"n$id\")];\n    let x$id = a$id[0];\n    let y$id = a$id[1];\n    acc = acc + x$id.len() as i32 + y$id.len() as i32;",
    // 35: match binding moves an Option<String> payload
    "let o$id = maybe_text(acc > 0);\n    let v$id = match o$id { Option::Some(text) => text.len() as i32, Option::None => 0 };\n    acc = acc + v$id;",
    // 36: nested struct field move, then another field of the outer struct
    "let h$id = make_holder($lit);\n    let p$id = h$id.p;\n    acc = acc + p$id.a + h$id.n;",
    // 37: element move from an array of non-Copy structs
    "let a$id = [make_pair(1, String::from(\"i$id\")), make_pair(2, String::from(\"j$id\"))];\n    let p$id = a$id[0];\n    acc = acc + p$id.a;",
    // 38: if/else expression producing an owned String, then moved
    "let s$id = if acc > 0 { make_text(1) } else { make_text(2) };\n    let n$id = s$id.len();\n    acc = acc + n$id as i32;",
    // 39: enum payload move through a match binding
    "let e$id = make_text_enum(acc > 0);\n    let n$id = match e$id { Text::Empty => 0, Text::Full(s) => s.len() as i32 };\n    acc = acc + n$id;",
    // 40: &self method on a struct temporary
    "acc = acc + make_pair(1, String::from(\"k$id\")).a_value();",
    // 41: shadowing a String inside a nested block, then dropping it
    "{ let s$id = make_text(1); let n$id = s$id.len(); acc = acc + n$id as i32; }",
];

struct Generated {
    statements: Vec<String>,
}

impl Generated {
    fn render(&self) -> String {
        let mut out = String::from(HEADER);
        out.push_str("fn main() {\n    let mut acc: i32 = 10;\n    ");
        for statement in &self.statements {
            out.push_str(&statement.replace('\n', "\n    "));
            out.push('\n');
            out.push_str("    ");
        }
        out.push_str("io::println(&acc.to_string());\n}\n");
        out
    }
}

fn instantiate(template: &str, id: usize, lit: i32, lit2: i32) -> String {
    template
        .replace("$id", &id.to_string())
        .replace("$lit2", &lit2.to_string())
        .replace("$lit", &lit.to_string())
}

fn generate_program(seed: u64, statement_count: usize) -> Generated {
    let mut rng = Rng::new(seed);
    let mut statements = Vec::with_capacity(statement_count);
    for index in 0..statement_count {
        let kind = rng.below(TEMPLATES.len());
        let lit = rng.literal();
        let lit2 = rng.literal();
        statements.push(instantiate(TEMPLATES[kind], index, lit, lit2));
    }
    Generated { statements }
}

// ---------------------------------------------------------------------------
// Evaluation: check -> codegen -> rustc -> run
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Verdict {
    CheckRejected,
    CodegenError(String),
    BuildFailed(String),
    BuildReleaseFailed(String),
    RuntimePanic(String),
    OutputMismatch { debug: String, release: String },
    Ok,
}

impl Verdict {
    fn diverges(&self) -> bool {
        !matches!(self, Verdict::CheckRejected | Verdict::Ok)
    }

    fn label(&self) -> &'static str {
        match self {
            Verdict::CheckRejected => "check_rejected",
            Verdict::CodegenError(_) => "codegen_error",
            Verdict::BuildFailed(_) => "build_failed",
            Verdict::BuildReleaseFailed(_) => "build_release_failed",
            Verdict::RuntimePanic(_) => "runtime_panic",
            Verdict::OutputMismatch { .. } => "output_mismatch",
            Verdict::Ok => "ok",
        }
    }
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> TempDir {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.subsec_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "ravum-diff-{}-{}-{}-{}",
            std::process::id(),
            unique,
            nanos,
            label
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("cannot create the temporary directory");
        TempDir { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn rustc_available() -> bool {
    std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn rustc_command() -> String {
    std::env::var("RAVUM_RUSTC").unwrap_or_else(|_| "rustc".to_string())
}

struct Compiled {
    _dir: TempDir,
    binary: PathBuf,
}

fn compile_rust(code: &str, label: &str, release: bool) -> Result<Compiled, String> {
    let dir = TempDir::new(label);
    let source = dir.path.join("program.rs");
    std::fs::write(&source, code).expect("cannot write the generated Rust");
    let binary = dir.path.join(if cfg!(windows) {
        "program.exe"
    } else {
        "program"
    });
    let mut command = std::process::Command::new(rustc_command());
    command
        .arg("--edition")
        .arg("2021")
        .arg("-C")
        .arg("overflow-checks=on")
        .arg("-C")
        .arg("debuginfo=2")
        .arg("-C")
        .arg(if release {
            "opt-level=3"
        } else {
            "opt-level=0"
        })
        .arg("--crate-name")
        .arg("diff_oracle_program")
        .arg("-o")
        .arg(&binary)
        .arg(&source);
    let output = command.output().expect("cannot run rustc");
    if output.status.success() {
        Ok(Compiled { _dir: dir, binary })
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

fn run_binary(binary: &Path) -> (String, String, i32) {
    let output = std::process::Command::new(binary)
        .output()
        .expect("cannot run the compiled program");
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    )
}

fn check_program(source: &str, label: &str) -> Result<Option<String>, String> {
    let dir = TempDir::new(label);
    let manifest =
        format!("[package]\nname = \"{label}\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n");
    std::fs::write(dir.path.join("ravum.toml"), manifest).expect("cannot write the manifest");
    let src = dir.path.join("src");
    std::fs::create_dir_all(&src).expect("cannot create the package source directory");
    std::fs::write(src.join("main.rv"), source).expect("cannot write the Ravum source");
    let loaded = load_package_with_options(
        &dir.path,
        &LoadOptions {
            locked: false,
            overrides: std::collections::BTreeMap::new(),
            ..Default::default()
        },
    );
    // `check` accepts a program with warnings (the CLI exits 0): only errors
    // make it a rejection. A warning must never be counted as a refusal.
    if loaded.diags.has_errors() {
        return Err(loaded.diags.render(&loaded.sources));
    }
    let program = match loaded.program {
        Some(program) => program,
        None => return Err("loader produced no program".to_string()),
    };
    let result = check(
        &program,
        &loaded.sources,
        &CheckOptions {
            profile: Profile::Core,
            edition: syntax::LANGUAGE_EDITION.to_string(),
        },
    );
    if result.diags.has_errors() {
        return Err(format!(
            "rejected:\n{}",
            result.diags.render(&loaded.sources)
        ));
    }
    let checked = result
        .checked
        .ok_or_else(|| "checker produced no CheckedProgram".to_string())?;
    let rust = generate(&checked, &GenOptions::default())
        .map_err(|error| format!("codegen error: {}", error.message))?;
    Ok(Some(rust))
}

/// Evaluate one program. `release_every` controls how often the release build
/// is executed (0 disables property 2 entirely).
fn evaluate(seed: u64, index: usize, source: &str, release_every: usize) -> Verdict {
    let label = format!("s{seed:016x}-i{index}");
    let rust = match check_program(source, &label) {
        Ok(Some(rust)) => rust,
        Ok(None) => return Verdict::CheckRejected,
        Err(message) if message.starts_with("codegen error:") => {
            return Verdict::CodegenError(message)
        }
        Err(message) => {
            if std::env::var("RAVUM_DIFF_DEBUG").is_ok_and(|value| !value.is_empty()) {
                eprintln!("check/loader rejection for {label}:\n{message}");
            }
            return Verdict::CheckRejected;
        }
    };
    let debug = match compile_rust(&rust, &label, false) {
        Ok(compiled) => compiled,
        Err(stderr) => return Verdict::BuildFailed(stderr),
    };
    let (debug_out, debug_err, debug_code) = run_binary(&debug.binary);
    if debug_code != 0 {
        return Verdict::RuntimePanic(format!(
            "debug exit {debug_code}; stdout: {debug_out:?}; stderr: {debug_err}"
        ));
    }
    if release_every > 0 && index % release_every == 0 {
        let release = match compile_rust(&rust, &format!("{label}-release"), true) {
            Ok(compiled) => compiled,
            Err(stderr) => return Verdict::BuildReleaseFailed(stderr),
        };
        let (release_out, release_err, release_code) = run_binary(&release.binary);
        if release_code != 0 {
            return Verdict::RuntimePanic(format!(
                "release exit {release_code}; stdout: {release_out:?}; stderr: {release_err}"
            ));
        }
        if debug_out != release_out || debug_code != release_code {
            return Verdict::OutputMismatch {
                debug: format!("exit {debug_code}: {debug_out:?}"),
                release: format!("exit {release_code}: {release_out:?}"),
            };
        }
    }
    Verdict::Ok
}

// ---------------------------------------------------------------------------
// Reducer
// ---------------------------------------------------------------------------

fn reduce(program: &mut Generated, release_every: usize) -> usize {
    let mut removed = 0;
    loop {
        let mut progress = false;
        for index in (0..program.statements.len()).rev() {
            let statement = program.statements.remove(index);
            let divergent = evaluate(0, 0, &program.render(), release_every).diverges();
            if divergent {
                progress = true;
                removed += 1;
            } else {
                program.statements.insert(index, statement);
            }
        }
        if !progress {
            return removed;
        }
    }
}

fn write_divergence(seed: u64, index: usize, verdict: &Verdict, program: &Generated) -> PathBuf {
    let dir = PathBuf::from("target").join("diff-divergences");
    std::fs::create_dir_all(&dir).expect("cannot create target/diff-divergences");
    let path = dir.join(format!("{}-{seed:016x}-{index}.rv", verdict.label()));
    std::fs::write(&path, program.render()).expect("cannot write the reduced divergence");
    path
}

// ---------------------------------------------------------------------------
// Runs
// ---------------------------------------------------------------------------

struct Stats {
    seed: u64,
    generated: usize,
    accepted: usize,
    rejected: usize,
    divergences: usize,
    elapsed_ms: u128,
}

fn run_seed(
    seed: u64,
    programs: usize,
    statements: usize,
    release_every: usize,
    collect: bool,
) -> Stats {
    let started = Instant::now();
    let mut stats = Stats {
        seed,
        generated: 0,
        accepted: 0,
        rejected: 0,
        divergences: 0,
        elapsed_ms: 0,
    };
    for index in 0..programs {
        let program = generate_program(
            seed ^ (index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15),
            statements,
        );
        // Property 3a: the same seed must produce the same bytes.
        let again = generate_program(
            seed ^ (index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15),
            statements,
        );
        assert_eq!(
            program.render(),
            again.render(),
            "generator is not deterministic for seed {seed:016x} index {index}"
        );
        let source = program.render();
        stats.generated += 1;
        // Property 3b: two independent `check` (and codegen) runs must agree.
        let first = check_program(&source, "determinism-a");
        let second = check_program(&source, "determinism-b");
        assert_eq!(
            format!("{first:?}"),
            format!("{second:?}"),
            "check/codegen is not deterministic for seed {seed:016x} index {index}"
        );
        let verdict = evaluate(seed, index, &source, release_every);
        match verdict {
            Verdict::CheckRejected => stats.rejected += 1,
            Verdict::Ok => stats.accepted += 1,
            other => {
                stats.accepted += 1;
                stats.divergences += 1;
                let mut reduced = program;
                let removed = reduce(&mut reduced, release_every);
                let path = write_divergence(seed, index, &other, &reduced);
                eprintln!(
                    "DIVERGENCE seed=0x{seed:016x} index={index} kind={} reduced={removed} statements at {}",
                    other.label(),
                    path.display()
                );
                if let Verdict::BuildFailed(stderr) | Verdict::BuildReleaseFailed(stderr) = &other {
                    let short: String = stderr.lines().take(12).collect::<Vec<_>>().join("\n");
                    eprintln!("{short}");
                }
                if !collect {
                    panic!(
                        "differential oracle found a divergence: seed=0x{seed:016x} index={index} kind={}",
                        other.label()
                    );
                }
            }
        }
    }
    stats.elapsed_ms = started.elapsed().as_millis();
    stats
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(default)
}

fn run_mode(label: &str, programs: usize, collect: bool) {
    if !rustc_available() {
        eprintln!("SKIP {label}: rustc not available");
        return;
    }
    let release_every = env_usize("RAVUM_DIFF_RELEASE_EVERY", 10);
    let statements = env_usize("RAVUM_DIFF_STATEMENTS", 8);
    let seeds = [
        0x0052_4156_554D_0001,
        0x0052_4156_554D_0002,
        0x0052_4156_554D_0003,
    ];
    let mut total_generated = 0;
    let mut total_accepted = 0;
    let mut total_rejected = 0;
    let mut total_divergences = 0;
    let mut worst_rate = f64::INFINITY;
    for seed in seeds {
        let stats = run_seed(seed, programs, statements, release_every, collect);
        let rate = stats.accepted as f64 / stats.generated.max(1) as f64 * 100.0;
        worst_rate = worst_rate.min(rate);
        eprintln!(
            "[diff:{label}] seed=0x{:016x} generated={} accepted={} rejected={} divergences={} acceptance={rate:.1}% elapsed_ms={}",
            stats.seed, stats.generated, stats.accepted, stats.rejected, stats.divergences, stats.elapsed_ms
        );
        total_generated += stats.generated;
        total_accepted += stats.accepted;
        total_rejected += stats.rejected;
        total_divergences += stats.divergences;
    }
    eprintln!(
        "[diff:{label}] TOTAL generated={total_generated} accepted={total_accepted} \
         rejected={total_rejected} divergences={total_divergences} \
         min_acceptance={worst_rate:.1}% release_every={release_every} statements={statements}"
    );
    assert_eq!(
        total_divergences, 0,
        "{label}: {total_divergences} divergence(s) found; see target/diff-divergences/ and docs/qualification/T2-FINDINGS.md"
    );
    // Guard against a generator that stopped producing accepted programs: a
    // vacuous oracle would be green for the wrong reason.
    assert!(
        total_rejected * 2 < total_generated,
        "{label}: less than half of the generated programs are accepted by check \
         ({} rejected / {} generated); the generator is not well typed by construction",
        total_rejected,
        total_generated
    );
}

// ---------------------------------------------------------------------------
// Second generator: type-directed, with an ownership environment (additive)
//
// The 42-template generator above stays untouched. This generator *composes*:
// an environment holds every binding with its type, mutability and ownership
// state (init / moved / partially moved / uninitialised), expressions are
// drawn among the rules that PRODUCE the wanted type, statements mutate the
// environment, and a branch join takes the INTERSECTION of the branch states
// (a value moved in one branch is considered moved afterwards). 10 % of the
// programs carry a deliberate fault and must be refused by `check` with an
// `E-OWN-*` code; the others must stay accepted, build and run.
// ---------------------------------------------------------------------------

use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ty {
    I32,
    Bool,
    Usize,
    Str,
    VecI32,
    Pair,
    Holder,
    Shape,
    Text,
    Tup(TupTy),
    Arr(ArrTy),
    Opt(OptTy),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TupTy {
    IB,
    BI,
    SI,
    IS,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ArrTy {
    I32x2,
    I32x3,
    Strx2,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum OptTy {
    I32,
    Str,
}

impl Ty {
    fn is_copy(self) -> bool {
        matches!(self, Ty::I32 | Ty::Bool | Ty::Usize)
    }

    fn name(self) -> &'static str {
        match self {
            Ty::I32 => "i32",
            Ty::Bool => "bool",
            Ty::Usize => "usize",
            Ty::Str => "String",
            Ty::VecI32 => "Vec<i32>",
            Ty::Pair => "Pair",
            Ty::Holder => "Holder",
            Ty::Shape => "Shape",
            Ty::Text => "Text",
            Ty::Tup(TupTy::IB) => "(i32, bool)",
            Ty::Tup(TupTy::BI) => "(bool, i32)",
            Ty::Tup(TupTy::SI) => "(String, i32)",
            Ty::Tup(TupTy::IS) => "(i32, String)",
            Ty::Arr(ArrTy::I32x2) => "[i32; 2]",
            Ty::Arr(ArrTy::I32x3) => "[i32; 3]",
            Ty::Arr(ArrTy::Strx2) => "[String; 2]",
            Ty::Opt(OptTy::I32) => "Option<i32>",
            Ty::Opt(OptTy::Str) => "Option<String>",
        }
    }

    /// `(element type, length)` for the array types the generator produces.
    fn elem(self) -> Option<(Ty, u8)> {
        match self {
            Ty::Arr(ArrTy::I32x2) => Some((Ty::I32, 2)),
            Ty::Arr(ArrTy::I32x3) => Some((Ty::I32, 3)),
            Ty::Arr(ArrTy::Strx2) => Some((Ty::Str, 2)),
            _ => None,
        }
    }
}

const TYPED_TYPES: [Ty; 17] = [
    Ty::I32,
    Ty::Bool,
    Ty::Str,
    Ty::Tup(TupTy::IB),
    Ty::Tup(TupTy::BI),
    Ty::Tup(TupTy::SI),
    Ty::Tup(TupTy::IS),
    Ty::Arr(ArrTy::I32x2),
    Ty::Arr(ArrTy::I32x3),
    Ty::Arr(ArrTy::Strx2),
    Ty::VecI32,
    Ty::Opt(OptTy::I32),
    Ty::Opt(OptTy::Str),
    Ty::Pair,
    Ty::Holder,
    Ty::Shape,
    Ty::Text,
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Step {
    Field(&'static str),
    Index(u8),
}

#[derive(Clone, PartialEq, Eq, Debug)]
enum Ow {
    Init,
    Moved,
    Part(Vec<Step>),
    Uninit,
}

#[derive(Clone)]
struct Bind {
    name: String,
    ty: Ty,
    ow: Ow,
    mutable: bool,
    /// `loop_epoch` at declaration time: a non-`Copy` binding may only be moved
    /// inside a loop when it was declared in that same loop body.
    epoch: usize,
}

#[derive(Clone)]
struct TEnv {
    frames: Vec<Vec<Bind>>,
}

fn is_prefix(prefix: &[Step], full: &[Step]) -> bool {
    prefix.len() <= full.len() && prefix.iter().zip(full.iter()).all(|(a, b)| a == b)
}

impl TEnv {
    fn new() -> TEnv {
        TEnv {
            frames: vec![Vec::new()],
        }
    }

    fn push_frame(&mut self) {
        self.frames.push(Vec::new());
    }

    fn pop_frame(&mut self) {
        self.frames.pop();
    }

    fn add(&mut self, name: &str, ty: Ty, mutable: bool, epoch: usize) {
        self.frames.last_mut().expect("a frame").push(Bind {
            name: name.to_string(),
            ty,
            ow: Ow::Init,
            mutable,
            epoch,
        });
    }

    fn find(&self, name: &str) -> Option<(usize, usize)> {
        for (frame_index, frame) in self.frames.iter().enumerate().rev() {
            for (bind_index, bind) in frame.iter().enumerate().rev() {
                if bind.name == name {
                    return Some((frame_index, bind_index));
                }
            }
        }
        None
    }

    fn set_ow(&mut self, name: &str, ow: Ow) {
        if let Some((frame_index, bind_index)) = self.find(name) {
            self.frames[frame_index][bind_index].ow = ow;
        }
    }

    /// Innermost binding per name, in declaration order.
    fn bindings(&self) -> Vec<Bind> {
        let mut map: BTreeMap<&str, &Bind> = BTreeMap::new();
        for frame in &self.frames {
            for bind in frame {
                map.insert(bind.name.as_str(), bind);
            }
        }
        map.into_values().cloned().collect()
    }

    /// Whether `path` (empty = the whole binding) may be used: the binding is
    /// initialised and no moved sub-place intersects the requested place.
    fn can_use(&self, name: &str, path: &[Step]) -> bool {
        match self.find(name).map(|(f, b)| &self.frames[f][b].ow) {
            Some(Ow::Init) => true,
            Some(Ow::Part(moved)) => !moved.iter().any(|step| {
                is_prefix(std::slice::from_ref(step), path)
                    || is_prefix(path, std::slice::from_ref(step))
            }),
            _ => false,
        }
    }

    fn mark_move(&mut self, name: &str, path: &[Step]) {
        if let Some((frame_index, bind_index)) = self.find(name) {
            let bind = &mut self.frames[frame_index][bind_index];
            match &mut bind.ow {
                Ow::Init => {
                    bind.ow = if path.is_empty() {
                        Ow::Moved
                    } else {
                        Ow::Part(path.to_vec())
                    };
                }
                Ow::Part(moved) => {
                    if path.is_empty() {
                        bind.ow = Ow::Moved;
                    } else {
                        for step in path {
                            if !moved.contains(step) {
                                moved.push(*step);
                            }
                        }
                    }
                }
                Ow::Moved | Ow::Uninit => {}
            }
        }
    }
}

fn merge_ow(base: &Ow, arms: &[TEnv], frame_index: usize, bind_index: usize) -> Ow {
    if arms
        .iter()
        .any(|arm| matches!(arm.frames[frame_index][bind_index].ow, Ow::Moved))
    {
        return Ow::Moved;
    }
    if arms
        .iter()
        .any(|arm| matches!(arm.frames[frame_index][bind_index].ow, Ow::Uninit))
    {
        return Ow::Uninit;
    }
    let mut paths: Vec<Step> = Vec::new();
    if let Ow::Part(moved) = base {
        paths.extend(moved.iter().copied());
    }
    for arm in arms {
        if let Ow::Part(moved) = &arm.frames[frame_index][bind_index].ow {
            for step in moved {
                if !paths.contains(step) {
                    paths.push(*step);
                }
            }
        }
    }
    if paths.is_empty() {
        Ow::Init
    } else {
        Ow::Part(paths)
    }
}

/// Junction: a value moved in one branch is considered moved afterwards.
fn merge_env(base: &TEnv, arms: &[TEnv]) -> TEnv {
    for arm in arms {
        assert_eq!(
            arm.frames.len(),
            base.frames.len(),
            "branch environments must have the same frame depth"
        );
    }
    let mut out = base.clone();
    for (frame_index, frame) in out.frames.iter_mut().enumerate() {
        for (bind_index, bind) in frame.iter_mut().enumerate() {
            bind.ow = merge_ow(
                &base.frames[frame_index][bind_index].ow,
                arms,
                frame_index,
                bind_index,
            );
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fault {
    UseAfterMove,
    DoubleMove,
    Uninit,
}

impl Fault {
    fn from_index(index: usize) -> Fault {
        match index % 3 {
            0 => Fault::UseAfterMove,
            1 => Fault::DoubleMove,
            _ => Fault::Uninit,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Fault::UseAfterMove => "use_after_move",
            Fault::DoubleMove => "double_move",
            Fault::Uninit => "use_of_uninit",
        }
    }
}

#[derive(Default)]
struct GenStats {
    rules: BTreeMap<&'static str, usize>,
    expr_nodes: usize,
    depth_sum: usize,
    max_depth: usize,
    statements: usize,
}

impl GenStats {
    fn merge(&mut self, other: GenStats) {
        for (rule, count) in other.rules {
            *self.rules.entry(rule).or_insert(0) += count;
        }
        self.expr_nodes += other.expr_nodes;
        self.depth_sum += other.depth_sum;
        self.max_depth = self.max_depth.max(other.max_depth);
        self.statements += other.statements;
    }

    fn average_depth(&self) -> f64 {
        if self.expr_nodes == 0 {
            0.0
        } else {
            self.depth_sum as f64 / self.expr_nodes as f64
        }
    }
}

struct TypedProgram {
    statements: Vec<String>,
    fault_statements: Vec<String>,
    fault: Option<Fault>,
}

impl TypedProgram {
    fn render(&self) -> String {
        let mut out = String::from(HEADER);
        out.push_str("fn main() {\n    let mut acc: i32 = 10;\n");
        for statement in &self.statements {
            out.push_str("    ");
            out.push_str(statement);
            out.push('\n');
        }
        for statement in &self.fault_statements {
            out.push_str("    ");
            out.push_str(statement);
            out.push('\n');
        }
        out.push_str("    io::println(&acc.to_string());\n}\n");
        out
    }
}

struct TypedGen {
    rng: Rng,
    name_counter: usize,
    loop_epoch: usize,
    loop_var: Option<String>,
    loop_bound: u8,
    no_move: bool,
}

impl TypedGen {
    fn new(seed: u64) -> TypedGen {
        TypedGen {
            rng: Rng::new(seed),
            name_counter: 0,
            loop_epoch: 0,
            loop_var: None,
            loop_bound: 0,
            no_move: false,
        }
    }

    fn fresh(&mut self) -> String {
        self.name_counter += 1;
        format!("v{}", self.name_counter)
    }

    fn lit(&mut self) -> i32 {
        (self.rng.below(8) + 1) as i32
    }

    fn pick(&mut self, options: &[&'static str]) -> &'static str {
        options[self.rng.below(options.len())]
    }

    fn rule(&mut self, stats: &mut GenStats, name: &'static str) {
        *stats.rules.entry(name).or_insert(0) += 1;
    }

    fn can_move_binding(&self, bind: &Bind) -> bool {
        if bind.ty.is_copy() {
            return true;
        }
        if self.no_move {
            return false;
        }
        self.loop_epoch == 0 || bind.epoch >= self.loop_epoch
    }

    // -- candidate enumeration ------------------------------------------------

    fn live_names(&self, env: &TEnv, ty: Ty) -> Vec<String> {
        env.bindings()
            .into_iter()
            .filter(|bind| {
                bind.ty == ty && env.can_use(&bind.name, &[]) && self.can_move_binding(bind)
            })
            .map(|bind| bind.name)
            .collect()
    }

    fn field_names(&self, env: &TEnv, root: Ty, field: &'static str, field_ty: Ty) -> Vec<String> {
        let path = [Step::Field(field)];
        env.bindings()
            .into_iter()
            .filter(|bind| {
                bind.ty == root
                    && env.can_use(&bind.name, &path)
                    && (field_ty.is_copy() || self.can_move_binding(bind))
            })
            .map(|bind| bind.name)
            .collect()
    }

    fn index_names(&self, env: &TEnv, root: Ty, index: u8, elem: Ty) -> Vec<String> {
        let path = [Step::Index(index)];
        env.bindings()
            .into_iter()
            .filter(|bind| {
                bind.ty == root
                    && bind
                        .ty
                        .elem()
                        .is_some_and(|(element, length)| element == elem && index < length)
                    && env.can_use(&bind.name, &path)
                    && (elem.is_copy() || self.can_move_binding(bind))
            })
            .map(|bind| bind.name)
            .collect()
    }

    fn loop_array_names(&self, env: &TEnv) -> Vec<String> {
        let bound = self.loop_bound;
        env.bindings()
            .into_iter()
            .filter(|bind| {
                bind.ty
                    .elem()
                    .is_some_and(|(element, length)| element == Ty::I32 && length >= bound)
                    && env.can_use(&bind.name, &[])
            })
            .map(|bind| bind.name)
            .collect()
    }

    fn choose(&mut self, names: &[String]) -> String {
        names[self.rng.below(names.len())].clone()
    }

    fn take_live(&mut self, env: &mut TEnv, ty: Ty, stats: &mut GenStats) -> String {
        let names = self.live_names(env, ty);
        let name = self.choose(&names);
        if !ty.is_copy() {
            env.mark_move(&name, &[]);
        }
        self.rule(stats, "expr:var");
        name
    }

    // -- leaves ---------------------------------------------------------------

    fn leaf(&mut self, ty: Ty, stats: &mut GenStats) -> String {
        match ty {
            Ty::I32 => {
                self.rule(stats, "expr:leaf:i32");
                format!("{}", self.lit())
            }
            Ty::Bool => {
                self.rule(stats, "expr:leaf:bool");
                if self.rng.below(2) == 0 {
                    "true".to_string()
                } else {
                    "false".to_string()
                }
            }
            Ty::Usize => {
                self.rule(stats, "expr:leaf:usize");
                format!("{}usize", self.rng.below(3) + 1)
            }
            Ty::Str => {
                self.rule(stats, "expr:leaf:String");
                let n = self.lit();
                format!("String::from(\"s{n}\")")
            }
            Ty::VecI32 => {
                self.rule(stats, "expr:leaf:Vec");
                "make_vec()".to_string()
            }
            Ty::Pair => {
                self.rule(stats, "expr:leaf:Pair");
                let a = self.lit();
                let n = self.lit();
                format!("make_pair({a}, String::from(\"p{n}\"))")
            }
            Ty::Holder => {
                self.rule(stats, "expr:leaf:Holder");
                let n = self.lit();
                format!("make_holder({n})")
            }
            Ty::Shape => self.leaf_shape(stats),
            Ty::Text => self.leaf_text(stats),
            Ty::Tup(TupTy::IB) => {
                self.rule(stats, "expr:leaf:tuple");
                let n = self.lit();
                format!("({n}, true)")
            }
            Ty::Tup(TupTy::BI) => {
                self.rule(stats, "expr:leaf:tuple");
                let n = self.lit();
                format!("(false, {n})")
            }
            Ty::Tup(TupTy::SI) => {
                self.rule(stats, "expr:leaf:tuple");
                let n = self.lit();
                format!("(String::from(\"t{n}\"), {n})")
            }
            Ty::Tup(TupTy::IS) => {
                self.rule(stats, "expr:leaf:tuple");
                let n = self.lit();
                format!("({n}, String::from(\"u{n}\"))")
            }
            Ty::Arr(ArrTy::I32x2) => {
                self.rule(stats, "expr:leaf:array");
                let a = self.lit();
                let b = self.lit();
                format!("[{a}, {b}]")
            }
            Ty::Arr(ArrTy::I32x3) => {
                self.rule(stats, "expr:leaf:array");
                let a = self.lit();
                let b = self.lit();
                let c = self.lit();
                format!("[{a}, {b}, {c}]")
            }
            Ty::Arr(ArrTy::Strx2) => {
                self.rule(stats, "expr:leaf:array");
                let n = self.lit();
                format!("[String::from(\"a{n}\"), String::from(\"b{n}\")]")
            }
            Ty::Opt(OptTy::I32) => {
                self.rule(stats, "expr:leaf:option");
                let n = self.lit();
                format!("Option::Some({n})")
            }
            Ty::Opt(OptTy::Str) => {
                self.rule(stats, "expr:leaf:option");
                let n = self.lit();
                format!("Option::Some(String::from(\"o{n}\"))")
            }
        }
    }

    fn leaf_shape(&mut self, stats: &mut GenStats) -> String {
        self.rule(stats, "expr:leaf:Shape");
        match self.rng.below(3) {
            0 => "Shape::Dot".to_string(),
            1 => {
                let n = self.lit();
                format!("Shape::Line({n})")
            }
            _ => {
                let w = self.lit();
                let h = self.lit();
                format!("Shape::Rect {{ w: {w}, h: {h} }}")
            }
        }
    }

    fn leaf_text(&mut self, stats: &mut GenStats) -> String {
        self.rule(stats, "expr:leaf:Text");
        if self.rng.below(2) == 0 {
            "Text::Empty".to_string()
        } else {
            let n = self.lit();
            format!("Text::Full(String::from(\"f{n}\"))")
        }
    }

    // -- expressions ----------------------------------------------------------

    fn expr(&mut self, ty: Ty, env: &mut TEnv, depth: usize, stats: &mut GenStats) -> String {
        stats.expr_nodes += 1;
        stats.depth_sum += depth;
        stats.max_depth = stats.max_depth.max(depth);
        if depth == 0 {
            return self.leaf(ty, stats);
        }
        match ty {
            Ty::I32 => self.expr_i32(env, depth, stats),
            Ty::Bool => self.expr_bool(env, depth, stats),
            Ty::Usize => self.expr_usize(env, depth, stats),
            Ty::Str => self.expr_str(env, depth, stats),
            Ty::VecI32 => self.expr_vec(env, depth, stats),
            Ty::Pair => self.expr_pair(env, depth, stats),
            Ty::Holder => self.expr_holder(env, depth, stats),
            Ty::Shape => self.expr_shape(env, depth, stats),
            Ty::Text => self.expr_text(env, depth, stats),
            Ty::Tup(kind) => self.expr_tup(kind, env, depth, stats),
            Ty::Arr(kind) => self.expr_arr(kind, env, depth, stats),
            Ty::Opt(kind) => self.expr_opt(kind, env, depth, stats),
        }
    }

    fn expr_i32(&mut self, env: &mut TEnv, depth: usize, stats: &mut GenStats) -> String {
        let mut options: Vec<&'static str> = vec!["leaf", "if", "block", "add", "match"];
        if !self.live_names(env, Ty::I32).is_empty() {
            options.push("var");
        }
        if !self.field_names(env, Ty::Pair, "a", Ty::I32).is_empty() {
            options.push("pair.a");
        }
        if !self.field_names(env, Ty::Holder, "n", Ty::I32).is_empty() {
            options.push("holder.n");
        }
        if !self
            .index_names(env, Ty::Tup(TupTy::IB), 0, Ty::I32)
            .is_empty()
            || !self
                .index_names(env, Ty::Tup(TupTy::BI), 1, Ty::I32)
                .is_empty()
            || !self
                .index_names(env, Ty::Tup(TupTy::SI), 1, Ty::I32)
                .is_empty()
            || !self
                .index_names(env, Ty::Tup(TupTy::IS), 0, Ty::I32)
                .is_empty()
        {
            options.push("tuple.i32");
        }
        if !self
            .index_names(env, Ty::Arr(ArrTy::I32x2), 0, Ty::I32)
            .is_empty()
            || !self
                .index_names(env, Ty::Arr(ArrTy::I32x2), 1, Ty::I32)
                .is_empty()
            || !self
                .index_names(env, Ty::Arr(ArrTy::I32x3), 2, Ty::I32)
                .is_empty()
        {
            options.push("array.i32");
        }
        if self.loop_var.is_some() && !self.loop_array_names(env).is_empty() {
            options.push("array.loop");
        }
        if !self.live_names(env, Ty::Shape).is_empty() {
            options.push("call.shape_area");
        }
        if !self.live_names(env, Ty::Str).is_empty() {
            options.push("call.len_of");
        }
        if !self.live_names(env, Ty::Pair).is_empty() {
            options.push("call.a_value");
        }
        if !self.live_names(env, Ty::VecI32).is_empty() {
            options.push("call.vec_len");
        }
        if !self.live_names(env, Ty::Arr(ArrTy::I32x2)).is_empty() {
            options.push("call.first_of");
        }
        match self.pick(&options) {
            "leaf" => self.leaf(Ty::I32, stats),
            "if" => self.expr_if(Ty::I32, env, depth, stats),
            "block" => self.expr_block(Ty::I32, env, depth, stats),
            "match" => self.expr_match(Ty::I32, env, depth, stats),
            "add" => {
                self.rule(stats, "expr:bin_add");
                let left = self.expr(Ty::I32, env, depth - 1, stats);
                let right = self.expr(Ty::I32, env, depth - 1, stats);
                format!("({left} + {right})")
            }
            "var" => self.take_live(env, Ty::I32, stats),
            "pair.a" => {
                let names = self.field_names(env, Ty::Pair, "a", Ty::I32);
                let name = self.choose(&names);
                self.rule(stats, "expr:proj:pair.a");
                format!("{name}.a")
            }
            "holder.n" => {
                let names = self.field_names(env, Ty::Holder, "n", Ty::I32);
                let name = self.choose(&names);
                self.rule(stats, "expr:proj:holder.n");
                format!("{name}.n")
            }
            "tuple.i32" => self.proj_tup_i32(env, stats),
            "array.i32" => self.proj_array_copy(env, stats),
            "array.loop" => {
                let names = self.loop_array_names(env);
                let name = self.choose(&names);
                let index = self.loop_var.clone().expect("a loop variable");
                self.rule(stats, "expr:proj:array.loop");
                format!("{name}[{index}]")
            }
            "call.shape_area" => {
                let shape = self.expr(Ty::Shape, env, depth - 1, stats);
                self.rule(stats, "expr:call:shape_area");
                format!("shape_area({shape})")
            }
            "call.len_of" => {
                let names = self.live_names(env, Ty::Str);
                let name = self.choose(&names);
                self.rule(stats, "expr:call:len_of");
                format!("(len_of({name}.as_str()) as i32)")
            }
            "call.a_value" => {
                let names = self.live_names(env, Ty::Pair);
                let name = self.choose(&names);
                self.rule(stats, "expr:call:a_value");
                format!("{name}.a_value()")
            }
            "call.vec_len" => {
                let names = self.live_names(env, Ty::VecI32);
                let name = self.choose(&names);
                self.rule(stats, "expr:call:vec_len");
                format!("({name}.len() as i32)")
            }
            "call.first_of" => {
                let names = self.live_names(env, Ty::Arr(ArrTy::I32x2));
                let name = self.choose(&names);
                self.rule(stats, "expr:call:first_of");
                format!("first_of(&{name})")
            }
            other => unreachable!("unknown i32 rule {other}"),
        }
    }

    fn proj_tup_i32(&mut self, env: &mut TEnv, stats: &mut GenStats) -> String {
        let mut candidates: Vec<(String, u8)> = Vec::new();
        for bind in env.bindings() {
            if let Ty::Tup(kind) = bind.ty {
                let index = match kind {
                    TupTy::IB | TupTy::IS => 0u8,
                    TupTy::BI | TupTy::SI => 1u8,
                };
                if matches!(kind, TupTy::IB | TupTy::IS | TupTy::BI | TupTy::SI)
                    && env.can_use(&bind.name, &[Step::Index(index)])
                {
                    candidates.push((bind.name, index));
                }
            }
        }
        let (name, index) = candidates[self.rng.below(candidates.len())].clone();
        self.rule(stats, "expr:proj:tuple.i32");
        format!("{name}.{index}")
    }

    fn proj_array_copy(&mut self, env: &mut TEnv, stats: &mut GenStats) -> String {
        let mut candidates: Vec<(String, u8)> = Vec::new();
        for bind in env.bindings() {
            if let Some((element, length)) = bind.ty.elem() {
                if element == Ty::I32 {
                    for index in 0..length {
                        if env.can_use(&bind.name, &[Step::Index(index)]) {
                            candidates.push((bind.name.clone(), index));
                        }
                    }
                }
            }
        }
        let (name, index) = candidates[self.rng.below(candidates.len())].clone();
        self.rule(stats, "expr:proj:array.i32");
        format!("{name}[{index}]")
    }

    fn proj_str(&mut self, env: &mut TEnv, stats: &mut GenStats) -> Option<String> {
        let mut candidates: Vec<(String, String, Step)> = Vec::new();
        for bind in env.bindings() {
            match bind.ty {
                Ty::Pair => {
                    if env.can_use(&bind.name, &[Step::Field("b")]) && self.can_move_binding(&bind)
                    {
                        candidates.push((
                            bind.name.clone(),
                            format!("{}.b", bind.name),
                            Step::Field("b"),
                        ));
                    }
                }
                Ty::Tup(TupTy::SI) => {
                    if env.can_use(&bind.name, &[Step::Index(0)]) && self.can_move_binding(&bind) {
                        candidates.push((
                            bind.name.clone(),
                            format!("{}.0", bind.name),
                            Step::Index(0),
                        ));
                    }
                }
                Ty::Tup(TupTy::IS) => {
                    if env.can_use(&bind.name, &[Step::Index(1)]) && self.can_move_binding(&bind) {
                        candidates.push((
                            bind.name.clone(),
                            format!("{}.1", bind.name),
                            Step::Index(1),
                        ));
                    }
                }
                Ty::Arr(ArrTy::Strx2) => {
                    for index in 0..2u8 {
                        if env.can_use(&bind.name, &[Step::Index(index)])
                            && self.can_move_binding(&bind)
                        {
                            candidates.push((
                                bind.name.clone(),
                                format!("{}[{index}]", bind.name),
                                Step::Index(index),
                            ));
                        }
                    }
                }
                _ => {}
            }
        }
        if candidates.is_empty() {
            return None;
        }
        let (name, text, step) = candidates[self.rng.below(candidates.len())].clone();
        env.mark_move(&name, &[step]);
        self.rule(stats, "expr:proj:string");
        Some(text)
    }

    fn expr_bool(&mut self, env: &mut TEnv, depth: usize, stats: &mut GenStats) -> String {
        let mut options: Vec<&'static str> = vec!["leaf", "if", "block", "cmp", "match"];
        if !self.live_names(env, Ty::Bool).is_empty() {
            options.push("var");
        }
        if !self
            .index_names(env, Ty::Tup(TupTy::IB), 1, Ty::Bool)
            .is_empty()
            || !self
                .index_names(env, Ty::Tup(TupTy::BI), 0, Ty::Bool)
                .is_empty()
        {
            options.push("tuple.bool");
        }
        if !self.live_names(env, Ty::VecI32).is_empty() {
            options.push("cmp.len");
        }
        match self.pick(&options) {
            "leaf" => self.leaf(Ty::Bool, stats),
            "if" => self.expr_if(Ty::Bool, env, depth, stats),
            "block" => self.expr_block(Ty::Bool, env, depth, stats),
            "match" => self.expr_match(Ty::Bool, env, depth, stats),
            "var" => self.take_live(env, Ty::Bool, stats),
            "cmp" => {
                self.rule(stats, "expr:cmp_i32");
                let left = self.expr(Ty::I32, env, depth - 1, stats);
                let right = self.expr(Ty::I32, env, depth - 1, stats);
                let operator = ["<", ">", "==", "!="][self.rng.below(4)];
                format!("({left} {operator} {right})")
            }
            "cmp.len" => {
                let names = self.live_names(env, Ty::VecI32);
                let name = self.choose(&names);
                let bound = self.rng.below(3) + 1;
                self.rule(stats, "expr:cmp:len");
                format!("({name}.len() > {bound}usize)")
            }
            "tuple.bool" => {
                let mut candidates: Vec<(String, u8)> = Vec::new();
                for bind in env.bindings() {
                    if let Ty::Tup(kind) = bind.ty {
                        let index = match kind {
                            TupTy::IB => 1u8,
                            TupTy::BI => 0u8,
                            _ => continue,
                        };
                        if env.can_use(&bind.name, &[Step::Index(index)]) {
                            candidates.push((bind.name, index));
                        }
                    }
                }
                let (name, index) = candidates[self.rng.below(candidates.len())].clone();
                self.rule(stats, "expr:proj:tuple.bool");
                format!("{name}.{index}")
            }
            other => unreachable!("unknown bool rule {other}"),
        }
    }

    fn expr_usize(&mut self, env: &mut TEnv, depth: usize, stats: &mut GenStats) -> String {
        let _ = depth;
        if !self.live_names(env, Ty::Usize).is_empty() && self.rng.below(2) == 0 {
            self.take_live(env, Ty::Usize, stats)
        } else {
            self.leaf(Ty::Usize, stats)
        }
    }

    fn expr_str(&mut self, env: &mut TEnv, depth: usize, stats: &mut GenStats) -> String {
        let mut options: Vec<&'static str> = vec!["leaf", "if", "block", "match"];
        if !self.live_names(env, Ty::Str).is_empty() {
            options.push("var");
        }
        if self.proj_str_available(env) {
            options.push("proj");
        }
        if !self.live_names(env, Ty::Pair).is_empty() {
            options.push("call.take_b");
        }
        options.push("call.make_text");
        match self.pick(&options) {
            "leaf" => self.leaf(Ty::Str, stats),
            "if" => self.expr_if(Ty::Str, env, depth, stats),
            "block" => self.expr_block(Ty::Str, env, depth, stats),
            "match" => self.expr_match(Ty::Str, env, depth, stats),
            "var" => self.take_live(env, Ty::Str, stats),
            "proj" => self.proj_str(env, stats).expect("a string projection"),
            "call.take_b" => {
                let names = self.live_names(env, Ty::Pair);
                let name = self.choose(&names);
                env.mark_move(&name, &[]);
                self.rule(stats, "expr:call:take_b");
                format!("{name}.take_b()")
            }
            "call.make_text" => {
                let value = self.expr(Ty::I32, env, depth - 1, stats);
                self.rule(stats, "expr:call:make_text");
                format!("make_text({value})")
            }
            other => unreachable!("unknown String rule {other}"),
        }
    }

    fn proj_str_available(&self, env: &TEnv) -> bool {
        env.bindings().into_iter().any(|bind| match bind.ty {
            Ty::Pair => {
                self.can_move_binding(&bind) && env.can_use(&bind.name, &[Step::Field("b")])
            }
            Ty::Tup(TupTy::SI) => {
                self.can_move_binding(&bind) && env.can_use(&bind.name, &[Step::Index(0)])
            }
            Ty::Tup(TupTy::IS) => {
                self.can_move_binding(&bind) && env.can_use(&bind.name, &[Step::Index(1)])
            }
            Ty::Arr(ArrTy::Strx2) => {
                self.can_move_binding(&bind)
                    && env.can_use(&bind.name, &[Step::Index(0)])
                        == env.can_use(&bind.name, &[Step::Index(1)])
                    && env.can_use(&bind.name, &[])
            }
            _ => false,
        })
    }

    fn expr_vec(&mut self, env: &mut TEnv, depth: usize, stats: &mut GenStats) -> String {
        let mut options: Vec<&'static str> = vec!["leaf", "if", "block"];
        if !self.live_names(env, Ty::VecI32).is_empty() {
            options.push("var");
        }
        match self.pick(&options) {
            "leaf" => self.leaf(Ty::VecI32, stats),
            "if" => self.expr_if(Ty::VecI32, env, depth, stats),
            "block" => self.expr_block(Ty::VecI32, env, depth, stats),
            _ => self.take_live(env, Ty::VecI32, stats),
        }
    }

    fn expr_pair(&mut self, env: &mut TEnv, depth: usize, stats: &mut GenStats) -> String {
        let mut options: Vec<&'static str> = vec!["leaf", "if", "block", "new"];
        if !self.live_names(env, Ty::Pair).is_empty() {
            options.push("var");
        }
        match self.pick(&options) {
            "leaf" => self.leaf(Ty::Pair, stats),
            "if" => self.expr_if(Ty::Pair, env, depth, stats),
            "block" => self.expr_block(Ty::Pair, env, depth, stats),
            "var" => self.take_live(env, Ty::Pair, stats),
            _ => {
                let a = self.expr(Ty::I32, env, depth - 1, stats);
                let b = self.expr(Ty::Str, env, depth - 1, stats);
                self.rule(stats, "expr:struct:pair");
                format!("Pair {{ a: {a}, b: {b} }}")
            }
        }
    }

    fn expr_holder(&mut self, env: &mut TEnv, depth: usize, stats: &mut GenStats) -> String {
        let mut options: Vec<&'static str> = vec!["leaf", "if", "block", "new"];
        if !self.live_names(env, Ty::Holder).is_empty() {
            options.push("var");
        }
        match self.pick(&options) {
            "leaf" => self.leaf(Ty::Holder, stats),
            "if" => self.expr_if(Ty::Holder, env, depth, stats),
            "block" => self.expr_block(Ty::Holder, env, depth, stats),
            "var" => self.take_live(env, Ty::Holder, stats),
            _ => {
                let pair = self.expr(Ty::Pair, env, depth - 1, stats);
                let n = self.expr(Ty::I32, env, depth - 1, stats);
                self.rule(stats, "expr:struct:holder");
                format!("Holder {{ p: {pair}, n: {n} }}")
            }
        }
    }

    fn expr_shape(&mut self, env: &mut TEnv, depth: usize, stats: &mut GenStats) -> String {
        let mut options: Vec<&'static str> = vec!["leaf", "if", "block", "line", "rect"];
        if !self.live_names(env, Ty::Shape).is_empty() {
            options.push("var");
        }
        match self.pick(&options) {
            "leaf" => self.leaf(Ty::Shape, stats),
            "if" => self.expr_if(Ty::Shape, env, depth, stats),
            "block" => self.expr_block(Ty::Shape, env, depth, stats),
            "var" => self.take_live(env, Ty::Shape, stats),
            "line" => {
                let n = self.expr(Ty::I32, env, depth - 1, stats);
                self.rule(stats, "expr:enum:shape.line");
                format!("Shape::Line({n})")
            }
            _ => {
                let w = self.expr(Ty::I32, env, depth - 1, stats);
                let h = self.expr(Ty::I32, env, depth - 1, stats);
                self.rule(stats, "expr:enum:shape.rect");
                format!("Shape::Rect {{ w: {w}, h: {h} }}")
            }
        }
    }

    fn expr_text(&mut self, env: &mut TEnv, depth: usize, stats: &mut GenStats) -> String {
        let mut options: Vec<&'static str> = vec!["leaf", "if", "block", "full"];
        if !self.live_names(env, Ty::Text).is_empty() {
            options.push("var");
        }
        match self.pick(&options) {
            "leaf" => self.leaf(Ty::Text, stats),
            "if" => self.expr_if(Ty::Text, env, depth, stats),
            "block" => self.expr_block(Ty::Text, env, depth, stats),
            "var" => self.take_live(env, Ty::Text, stats),
            _ => {
                let text = self.expr(Ty::Str, env, depth - 1, stats);
                self.rule(stats, "expr:enum:text.full");
                format!("Text::Full({text})")
            }
        }
    }

    fn expr_tup(
        &mut self,
        kind: TupTy,
        env: &mut TEnv,
        depth: usize,
        stats: &mut GenStats,
    ) -> String {
        let ty = Ty::Tup(kind);
        let mut options: Vec<&'static str> = vec!["leaf", "if", "block", "new"];
        if !self.live_names(env, ty).is_empty() {
            options.push("var");
        }
        match self.pick(&options) {
            "leaf" => self.leaf(ty, stats),
            "if" => self.expr_if(ty, env, depth, stats),
            "block" => self.expr_block(ty, env, depth, stats),
            "var" => self.take_live(env, ty, stats),
            _ => {
                let (first, second) = match kind {
                    TupTy::IB => (
                        self.expr(Ty::I32, env, depth - 1, stats),
                        self.expr(Ty::Bool, env, depth - 1, stats),
                    ),
                    TupTy::BI => (
                        self.expr(Ty::Bool, env, depth - 1, stats),
                        self.expr(Ty::I32, env, depth - 1, stats),
                    ),
                    TupTy::SI => (
                        self.expr(Ty::Str, env, depth - 1, stats),
                        self.expr(Ty::I32, env, depth - 1, stats),
                    ),
                    TupTy::IS => (
                        self.expr(Ty::I32, env, depth - 1, stats),
                        self.expr(Ty::Str, env, depth - 1, stats),
                    ),
                };
                self.rule(stats, "expr:tuple:new");
                format!("({first}, {second})")
            }
        }
    }

    fn expr_arr(
        &mut self,
        kind: ArrTy,
        env: &mut TEnv,
        depth: usize,
        stats: &mut GenStats,
    ) -> String {
        let ty = Ty::Arr(kind);
        let mut options: Vec<&'static str> = vec!["leaf", "if", "block", "new"];
        if !self.live_names(env, ty).is_empty() {
            options.push("var");
        }
        match self.pick(&options) {
            "leaf" => self.leaf(ty, stats),
            "if" => self.expr_if(ty, env, depth, stats),
            "block" => self.expr_block(ty, env, depth, stats),
            "var" => self.take_live(env, ty, stats),
            _ => {
                let (element, length) = ty.elem().expect("an array type");
                let mut parts = Vec::new();
                for _ in 0..length {
                    parts.push(self.expr(element, env, depth - 1, stats));
                }
                self.rule(stats, "expr:array:new");
                format!("[{}]", parts.join(", "))
            }
        }
    }

    fn expr_opt(
        &mut self,
        kind: OptTy,
        env: &mut TEnv,
        depth: usize,
        stats: &mut GenStats,
    ) -> String {
        let ty = Ty::Opt(kind);
        let mut options: Vec<&'static str> = vec!["leaf", "if", "block", "some"];
        if !self.live_names(env, ty).is_empty() {
            options.push("var");
        }
        match kind {
            OptTy::I32 => options.push("maybe"),
            OptTy::Str => options.push("maybe_text"),
        }
        match self.pick(&options) {
            "leaf" => self.leaf(ty, stats),
            "if" => self.expr_if(ty, env, depth, stats),
            "block" => self.expr_block(ty, env, depth, stats),
            "var" => self.take_live(env, ty, stats),
            "maybe" => {
                let flag = self.expr(Ty::Bool, env, depth - 1, stats);
                self.rule(stats, "expr:opt:maybe");
                format!("maybe({flag})")
            }
            "maybe_text" => {
                let flag = self.expr(Ty::Bool, env, depth - 1, stats);
                self.rule(stats, "expr:opt:maybe_text");
                format!("maybe_text({flag})")
            }
            _ => {
                let payload = match kind {
                    OptTy::I32 => self.expr(Ty::I32, env, depth - 1, stats),
                    OptTy::Str => self.expr(Ty::Str, env, depth - 1, stats),
                };
                self.rule(stats, "expr:opt:some");
                format!("Option::Some({payload})")
            }
        }
    }

    fn expr_if(&mut self, ty: Ty, env: &mut TEnv, depth: usize, stats: &mut GenStats) -> String {
        self.rule(stats, "expr:if");
        let cond = self.cond(env, depth - 1, stats);
        let base = env.clone();
        let mut then_env = base.clone();
        let then_value = self.expr(ty, &mut then_env, depth - 1, stats);
        let mut else_env = base.clone();
        let else_value = self.expr(ty, &mut else_env, depth - 1, stats);
        *env = merge_env(&base, &[then_env, else_env]);
        format!("if {cond} {{ {then_value} }} else {{ {else_value} }}")
    }

    fn expr_block(&mut self, ty: Ty, env: &mut TEnv, depth: usize, stats: &mut GenStats) -> String {
        self.rule(stats, "expr:block");
        let mut inner = env.clone();
        inner.push_frame();
        // Only a `let` here: a block-like statement (`while`, `if`, `match`)
        // followed by an expression is parsed as a call on its unit value.
        let statement = self.stmt_let(&mut inner, depth.saturating_sub(1), stats);
        let value = self.expr(ty, &mut inner, depth - 1, stats);
        inner.pop_frame();
        *env = inner;
        format!("{{ {statement} {value} }}")
    }

    /// `match` expression: exactly one arm per variant, with payload bindings.
    fn expr_match(
        &mut self,
        target: Ty,
        env: &mut TEnv,
        depth: usize,
        stats: &mut GenStats,
    ) -> String {
        self.rule(stats, "expr:match");
        let scrutinee_ty = match target {
            Ty::I32 => {
                if self.rng.below(2) == 0 {
                    Ty::Shape
                } else {
                    Ty::Opt(OptTy::I32)
                }
            }
            Ty::Str => {
                if self.rng.below(2) == 0 {
                    Ty::Text
                } else {
                    Ty::Opt(OptTy::Str)
                }
            }
            _ => Ty::Opt(OptTy::I32),
        };
        let scrutinee = self.expr(scrutinee_ty, env, depth - 1, stats);
        let base = env.clone();
        let mut arms: Vec<(String, Vec<(String, Ty)>)> = Vec::new();
        match scrutinee_ty {
            Ty::Shape => {
                let n = self.fresh();
                let w = self.fresh();
                let h = self.fresh();
                arms.push(("Shape::Dot".to_string(), Vec::new()));
                arms.push((format!("Shape::Line({n})"), vec![(n, Ty::I32)]));
                arms.push((
                    format!("Shape::Rect {{ w: {w}, h: {h} }}"),
                    vec![(w, Ty::I32), (h, Ty::I32)],
                ));
            }
            Ty::Opt(OptTy::I32) => {
                let x = self.fresh();
                arms.push((format!("Option::Some({x})"), vec![(x, Ty::I32)]));
                arms.push(("Option::None".to_string(), Vec::new()));
            }
            Ty::Opt(OptTy::Str) => {
                let s = self.fresh();
                arms.push((format!("Option::Some({s})"), vec![(s, Ty::Str)]));
                arms.push(("Option::None".to_string(), Vec::new()));
            }
            Ty::Text => {
                let s = self.fresh();
                arms.push(("Text::Empty".to_string(), Vec::new()));
                arms.push((format!("Text::Full({s})"), vec![(s, Ty::Str)]));
            }
            _ => unreachable!("unsupported match scrutinee {scrutinee_ty:?}"),
        }
        let mut rendered: Vec<String> = Vec::new();
        let mut arm_envs: Vec<TEnv> = Vec::new();
        for (pattern, bindings) in arms {
            let mut arm_env = base.clone();
            arm_env.push_frame();
            for (name, ty) in &bindings {
                arm_env.add(name, *ty, false, 0);
            }
            let body = self.expr(target, &mut arm_env, depth - 1, stats);
            arm_env.pop_frame();
            rendered.push(format!("{pattern} => {body}"));
            arm_envs.push(arm_env);
        }
        *env = merge_env(&base, &arm_envs);
        // A struct literal in scrutinee position would be parsed as the match
        // block (`match Shape::Rect { w: 3, h: 8 } { .. }`): parenthesise it.
        format!("match ({scrutinee}) {{ {} }}", rendered.join(", "))
    }

    fn cond(&mut self, env: &mut TEnv, depth: usize, stats: &mut GenStats) -> String {
        let mut options: Vec<&'static str> = vec!["literal", "cmp"];
        if !self.live_names(env, Ty::Bool).is_empty() {
            options.push("var");
        }
        match self.pick(&options) {
            "literal" => {
                self.rule(stats, "cond:literal");
                if self.rng.below(2) == 0 {
                    "true".to_string()
                } else {
                    "false".to_string()
                }
            }
            "var" => self.take_live(env, Ty::Bool, stats),
            _ => {
                self.rule(stats, "cond:cmp");
                let left = self.expr(Ty::I32, env, depth, stats);
                let right = self.expr(Ty::I32, env, depth, stats);
                let operator = ["<", ">", "==", "!="][self.rng.below(4)];
                format!("({left} {operator} {right})")
            }
        }
    }

    // -- statements -----------------------------------------------------------

    fn stmt(&mut self, env: &mut TEnv, depth: usize, stats: &mut GenStats) -> String {
        if depth == 0 {
            return if self.rng.below(100) < 60 {
                self.stmt_let(env, 0, stats)
            } else {
                self.stmt_assign(env, 0, stats)
            };
        }
        let roll = self.rng.below(100);
        if roll < 24 {
            self.stmt_let(env, depth, stats)
        } else if roll < 34 {
            self.stmt_shadow(env, depth, stats)
        } else if roll < 46 {
            self.stmt_assign(env, depth, stats)
        } else if roll < 56 {
            self.stmt_if(env, depth, stats, true)
        } else if roll < 64 {
            self.stmt_if(env, depth, stats, false)
        } else if roll < 74 {
            self.stmt_match(env, depth, stats)
        } else if roll < 82 {
            self.stmt_block(env, depth, stats)
        } else if roll < 91 {
            self.stmt_for(env, depth, stats)
        } else {
            self.stmt_while(env, depth, stats)
        }
    }

    fn stmt_let(&mut self, env: &mut TEnv, depth: usize, stats: &mut GenStats) -> String {
        self.rule(stats, "stmt:let");
        stats.statements += 1;
        let ty = TYPED_TYPES[self.rng.below(TYPED_TYPES.len())];
        let name = self.fresh();
        let mutable = self.rng.below(2) == 0;
        let value = self.expr(ty, env, depth, stats);
        let keyword = if mutable { "let mut" } else { "let" };
        let text = format!("{keyword} {name}: {} = {value};", ty.name());
        env.add(&name, ty, mutable, self.loop_epoch);
        text
    }

    fn stmt_shadow(&mut self, env: &mut TEnv, depth: usize, stats: &mut GenStats) -> String {
        self.rule(stats, "stmt:shadow");
        stats.statements += 1;
        let names: Vec<String> = env
            .bindings()
            .into_iter()
            .map(|bind| bind.name)
            .filter(|name| name != "acc")
            .collect();
        if names.is_empty() {
            return self.stmt_let(env, depth, stats);
        }
        let name = self.choose(&names);
        let ty = TYPED_TYPES[self.rng.below(TYPED_TYPES.len())];
        let value = self.expr(ty, env, depth, stats);
        let text = format!("let {name}: {} = {value};", ty.name());
        env.add(&name, ty, false, self.loop_epoch);
        text
    }

    fn stmt_assign(&mut self, env: &mut TEnv, depth: usize, stats: &mut GenStats) -> String {
        self.rule(stats, "stmt:assign");
        stats.statements += 1;
        let candidates: Vec<(String, Ty)> = env
            .bindings()
            .into_iter()
            .filter(|bind| bind.mutable)
            .map(|bind| (bind.name, bind.ty))
            .collect();
        if candidates.is_empty() {
            return self.stmt_let(env, depth, stats);
        }
        let (name, ty) = candidates[self.rng.below(candidates.len())].clone();
        let mut value = self.expr(ty, env, depth, stats);
        if value == name {
            value = self.leaf(ty, stats);
        }
        env.set_ow(&name, Ow::Init);
        format!("{name} = {value};")
    }

    fn stmt_if(
        &mut self,
        env: &mut TEnv,
        depth: usize,
        stats: &mut GenStats,
        with_else: bool,
    ) -> String {
        self.rule(
            stats,
            if with_else {
                "stmt:if"
            } else {
                "stmt:if_no_else"
            },
        );
        stats.statements += 1;
        let cond = self.cond(env, depth - 1, stats);
        let base = env.clone();
        let (then_text, then_env) = self.branch(base.clone(), depth - 1, stats);
        if with_else {
            let (else_text, else_env) = self.branch(base.clone(), depth - 1, stats);
            *env = merge_env(&base, &[then_env, else_env]);
            format!("if {cond} {{ {then_text} }} else {{ {else_text} }}")
        } else {
            *env = merge_env(&base, &[then_env, base.clone()]);
            format!("if {cond} {{ {then_text} }}")
        }
    }

    fn branch(&mut self, mut env: TEnv, depth: usize, stats: &mut GenStats) -> (String, TEnv) {
        env.push_frame();
        let count = self.rng.below(3);
        let mut parts: Vec<String> = Vec::new();
        for _ in 0..count {
            parts.push(self.stmt(&mut env, depth, stats));
        }
        env.pop_frame();
        (parts.join(" "), env)
    }

    fn stmt_match(&mut self, env: &mut TEnv, depth: usize, stats: &mut GenStats) -> String {
        self.rule(stats, "stmt:match");
        stats.statements += 1;
        let target = [Ty::I32, Ty::Str, Ty::Bool][self.rng.below(3)];
        let name = self.fresh();
        let value = self.expr_match(target, env, depth, stats);
        let text = format!("let {name}: {} = {value};", target.name());
        env.add(&name, target, false, self.loop_epoch);
        text
    }

    fn stmt_block(&mut self, env: &mut TEnv, depth: usize, stats: &mut GenStats) -> String {
        self.rule(stats, "stmt:block");
        stats.statements += 1;
        env.push_frame();
        let count = 1 + self.rng.below(2);
        let mut parts: Vec<String> = Vec::new();
        for _ in 0..count {
            parts.push(self.stmt(env, depth - 1, stats));
        }
        env.pop_frame();
        format!("{{ {} }}", parts.join(" "))
    }

    fn stmt_for(&mut self, env: &mut TEnv, depth: usize, stats: &mut GenStats) -> String {
        self.rule(stats, "stmt:for");
        stats.statements += 1;
        let bound = if self.rng.below(2) == 0 { 2u8 } else { 3u8 };
        let name = self.fresh();
        let saved_var = self.loop_var.replace(name.clone());
        let saved_bound = self.loop_bound;
        let saved_epoch = self.loop_epoch;
        let saved_no_move = self.no_move;
        self.loop_epoch += 1;
        self.loop_bound = bound;
        self.no_move = true;
        env.push_frame();
        env.add(&name, Ty::Usize, false, self.loop_epoch);
        let body = self.loop_body(env, depth, stats);
        env.pop_frame();
        self.loop_var = saved_var;
        self.loop_bound = saved_bound;
        self.loop_epoch = saved_epoch;
        self.no_move = saved_no_move;
        format!("for {name} in 0usize..{bound}usize {{ {body} }}")
    }

    fn stmt_while(&mut self, env: &mut TEnv, depth: usize, stats: &mut GenStats) -> String {
        self.rule(stats, "stmt:while");
        stats.statements += 1;
        let bound = 3u8;
        let counter = self.fresh();
        env.add(&counter, Ty::I32, true, self.loop_epoch);
        let saved_epoch = self.loop_epoch;
        let saved_no_move = self.no_move;
        self.loop_epoch += 1;
        self.no_move = true;
        env.push_frame();
        let body = self.loop_body(env, depth, stats);
        env.pop_frame();
        self.loop_epoch = saved_epoch;
        self.no_move = saved_no_move;
        format!("let mut {counter}: i32 = 0; while {counter} < {bound} {{ {body} {counter} = {counter} + 1; }}")
    }

    fn loop_body(&mut self, env: &mut TEnv, depth: usize, stats: &mut GenStats) -> String {
        let count = 1 + self.rng.below(2);
        let mut parts: Vec<String> = Vec::new();
        for _ in 0..count {
            parts.push(self.loop_stmt(env, depth, stats));
        }
        parts.join(" ")
    }

    fn loop_stmt(&mut self, env: &mut TEnv, depth: usize, stats: &mut GenStats) -> String {
        self.rule(stats, "stmt:loop");
        match self.rng.below(3) {
            0 => {
                let value = self.expr(Ty::I32, env, depth, stats);
                format!("acc = acc + {value};")
            }
            1 => {
                let name = self.fresh();
                let value = self.expr(Ty::I32, env, depth, stats);
                let text = format!("let {name}: i32 = {value};");
                env.add(&name, Ty::I32, false, self.loop_epoch);
                text
            }
            _ => {
                let arrays = self.loop_array_names(env);
                if let (Some(index), false) = (self.loop_var.clone(), arrays.is_empty()) {
                    let array = self.choose(&arrays);
                    format!("acc = acc + {array}[{index}];")
                } else {
                    let value = self.expr(Ty::I32, env, depth, stats);
                    format!("acc = acc + {value};")
                }
            }
        }
    }

    // -- deliberate faults ----------------------------------------------------

    fn fault_statements(&mut self, fault: Fault, stats: &mut GenStats) -> Vec<String> {
        self.rule(stats, "fault");
        let id = self.name_counter;
        self.name_counter += 1;
        match fault {
            Fault::UseAfterMove => vec![
                format!("let f{id}a: String = make_text(1);"),
                format!("let f{id}b: String = f{id}a;"),
                format!("let f{id}c = f{id}a.len();"),
            ],
            Fault::DoubleMove => vec![
                format!("let f{id}a: String = make_text(1);"),
                format!("let f{id}b: String = f{id}a;"),
                format!("let f{id}c: String = f{id}a;"),
            ],
            Fault::Uninit => vec![
                format!("let mut f{id}a: String;"),
                format!("let f{id}b = f{id}a.len();"),
            ],
        }
    }
}

fn generate_typed_program(
    seed: u64,
    statement_count: usize,
    forced_fault: Option<Fault>,
) -> (TypedProgram, GenStats) {
    let mut generator = TypedGen::new(seed);
    let mut stats = GenStats::default();
    let fault = forced_fault.or_else(|| {
        if generator.rng.below(10) == 0 {
            Some(Fault::from_index(generator.rng.below(3)))
        } else {
            None
        }
    });
    let mut env = TEnv::new();
    env.add("acc", Ty::I32, true, 0);
    let mut statements = Vec::with_capacity(statement_count);
    for _ in 0..statement_count {
        // Four is the depth bound of `gen_expr`; the environment model is
        // exercised by composition, not by unbounded nesting.
        statements.push(generator.stmt(&mut env, 4, &mut stats));
    }
    let fault_statements = match fault {
        Some(kind) => generator.fault_statements(kind, &mut stats),
        None => Vec::new(),
    };
    (
        TypedProgram {
            statements,
            fault_statements,
            fault,
        },
        stats,
    )
}

#[derive(Default)]
struct TypedTotals {
    generated: usize,
    faults: usize,
    fault_rejected: usize,
    fault_wrong: usize,
    accepted: usize,
    rejected: usize,
    divergences: usize,
}

impl TypedTotals {
    fn merge(&mut self, other: TypedTotals) {
        self.generated += other.generated;
        self.faults += other.faults;
        self.fault_rejected += other.fault_rejected;
        self.fault_wrong += other.fault_wrong;
        self.accepted += other.accepted;
        self.rejected += other.rejected;
        self.divergences += other.divergences;
    }

    fn fault_free(&self) -> usize {
        self.accepted + self.rejected + self.divergences
    }

    fn acceptance(&self) -> f64 {
        if self.fault_free() == 0 {
            0.0
        } else {
            self.accepted as f64 / self.fault_free() as f64 * 100.0
        }
    }
}

fn write_typed_divergence(seed: u64, index: usize, verdict: &Verdict, source: &str) -> PathBuf {
    let dir = PathBuf::from("target").join("diff-divergences");
    std::fs::create_dir_all(&dir).expect("cannot create target/diff-divergences");
    let path = dir.join(format!("typed-{}-{seed:016x}-{index}.rv", verdict.label()));
    std::fs::write(&path, source).expect("cannot write the typed divergence");
    path
}

fn run_typed_seed(
    seed: u64,
    programs: usize,
    statements: usize,
    release_every: usize,
    collect: bool,
) -> (TypedTotals, GenStats, u128) {
    let started = Instant::now();
    let mut totals = TypedTotals::default();
    let mut stats = GenStats::default();
    for index in 0..programs {
        let program_seed = seed ^ (index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let (program, program_stats) = generate_typed_program(program_seed, statements, None);
        let again = generate_typed_program(program_seed, statements, None).0;
        assert_eq!(
            program.render(),
            again.render(),
            "typed generator is not deterministic for seed {seed:016x} index {index}"
        );
        stats.merge(program_stats);
        totals.generated += 1;
        let source = program.render();
        if let Some(fault) = program.fault {
            totals.faults += 1;
            match check_program(&source, "typed-fault") {
                Err(message) if message.contains("E-OWN-") => totals.fault_rejected += 1,
                Err(message) => {
                    totals.fault_wrong += 1;
                    eprintln!(
                        "TYPED FAULT WRONG CODE ({}) seed=0x{seed:016x} index={index}: {}",
                        fault.label(),
                        message.lines().take(4).collect::<Vec<_>>().join(" | ")
                    );
                }
                Ok(_) => {
                    totals.fault_wrong += 1;
                    eprintln!(
                        "TYPED FAULT ACCEPTED ({}) seed=0x{seed:016x} index={index}",
                        fault.label()
                    );
                }
            }
            continue;
        }
        match evaluate(seed, index, &source, release_every) {
            Verdict::Ok => totals.accepted += 1,
            Verdict::CheckRejected => {
                totals.rejected += 1;
                if std::env::var("RAVUM_TYPED_DUMP").is_ok() {
                    let dir = PathBuf::from("target").join("typed-rejected");
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = std::fs::write(dir.join(format!("s{seed:016x}-i{index}.rv")), &source);
                }
            }
            other => {
                totals.divergences += 1;
                let path = write_typed_divergence(seed, index, &other, &source);
                eprintln!(
                    "TYPED DIVERGENCE seed=0x{seed:016x} index={index} kind={} program at {}",
                    other.label(),
                    path.display()
                );
                if let Verdict::BuildFailed(stderr) | Verdict::BuildReleaseFailed(stderr) = &other {
                    let short: String = stderr.lines().take(10).collect::<Vec<_>>().join("\n");
                    eprintln!("{short}");
                }
                if !collect {
                    panic!(
                        "typed differential oracle found a divergence: seed=0x{seed:016x} index={index} kind={}",
                        other.label()
                    );
                }
            }
        }
    }
    (totals, stats, started.elapsed().as_millis())
}

fn run_typed_mode(label: &str, programs: usize, release_every: usize, collect: bool, strict: bool) {
    if !rustc_available() {
        eprintln!("SKIP typed {label}: rustc not available");
        return;
    }
    let statements = env_usize("RAVUM_TYPED_STATEMENTS", 6);
    let seeds = [
        0x0052_4156_554D_1001,
        0x0052_4156_554D_1002,
        0x0052_4156_554D_1003,
    ];
    let mut total = TypedTotals::default();
    let mut stats = GenStats::default();
    for seed in seeds {
        let (totals, seed_stats, elapsed) =
            run_typed_seed(seed, programs, statements, release_every, collect);
        eprintln!(
            "[typed:{label}] seed=0x{:016x} generated={} faults={} fault_rejected={} accepted={} rejected={} divergences={} acceptance={:.1}% rules_fired={} elapsed_ms={elapsed}",
            seed,
            totals.generated,
            totals.faults,
            totals.fault_rejected,
            totals.accepted,
            totals.rejected,
            totals.divergences,
            totals.acceptance(),
            seed_stats.rules.len(),
        );
        total.merge(totals);
        stats.merge(stats_clone(&seed_stats));
    }
    eprintln!(
        "[typed:{label}] TOTAL generated={} faults={} fault_rejected={} accepted={} rejected={} divergences={} acceptance={:.1}% rules_fired={} statements_generated={} average_expr_depth={:.2} max_expr_depth={}",
        total.generated,
        total.faults,
        total.fault_rejected,
        total.accepted,
        total.rejected,
        total.divergences,
        total.acceptance(),
        stats.rules.len(),
        stats.statements,
        stats.average_depth(),
        stats.max_depth,
    );
    for (rule, count) in &stats.rules {
        eprintln!("[typed:{label}] rule {rule} = {count}");
    }
    assert_eq!(
        total.fault_wrong, 0,
        "{label}: {} deliberate fault(s) were not refused with an E-OWN-* code",
        total.fault_wrong
    );
    assert_eq!(
        total.fault_rejected, total.faults,
        "{label}: only {} of {} deliberate faults were refused",
        total.fault_rejected, total.faults
    );
    if strict {
        assert_eq!(
            total.divergences, 0,
            "{label}: {} divergence(s) found; see target/diff-divergences/",
            total.divergences
        );
        assert!(
            total.acceptance() >= 95.0,
            "{label}: acceptance of fault-free programs is {:.1}% (< 95 %); the environment model is wrong",
            total.acceptance()
        );
    }
}

// `GenStats` is not `Clone`; the seed line already printed the per-seed form
// count, and the aggregate only needs the counts.
fn stats_clone(stats: &GenStats) -> GenStats {
    GenStats {
        rules: stats
            .rules
            .iter()
            .map(|(rule, count)| (*rule, *count))
            .collect(),
        expr_nodes: stats.expr_nodes,
        depth_sum: stats.depth_sum,
        max_depth: stats.max_depth,
        statements: stats.statements,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn differential_oracle_fast() {
    run_mode("fast", env_usize("RAVUM_DIFF_PROGRAMS", 200), false);
}

#[test]
#[ignore = "long differential run: 3 seeds x 10 000 programs (RAVUM_DIFF_PROGRAMS overrides)"]
fn differential_oracle_long() {
    run_mode("long", env_usize("RAVUM_DIFF_PROGRAMS", 10_000), true);
}

/// Non-vacuity of the rustc half of the oracle: a check-accepted program plus
/// deliberately invalid Rust must be detected as a build failure.
#[test]
fn oracle_detects_a_deliberately_invalid_generated_file() {
    if !rustc_available() {
        eprintln!("SKIP oracle_detects_a_deliberately_invalid_generated_file: rustc not available");
        return;
    }
    let invalid = "fn main() { let value: i32 = \"text\"; }\n";
    assert!(
        compile_rust(invalid, "oracle-self-check", false).is_err(),
        "the oracle's rustc step must reject invalid Rust"
    );
}

/// Non-vacuity of the generator: distinct seeds must produce distinct programs
/// (otherwise the campaign would measure the same one program N times).
#[test]
fn generator_produces_distinct_programs_for_distinct_seeds() {
    let first = generate_program(1, 8).render();
    let second = generate_program(2, 8).render();
    assert_ne!(first, second, "two seeds produced the same program");
}

/// The typed generator must compose (it is not a template list): two seeds
/// differ, and the same seed produces the same bytes.
#[test]
fn typed_generator_is_deterministic_and_seed_sensitive() {
    let (first, _) = generate_typed_program(11, 6, None);
    let (again, _) = generate_typed_program(11, 6, None);
    let (other, _) = generate_typed_program(12, 6, None);
    assert_eq!(
        first.render(),
        again.render(),
        "typed generator is not deterministic"
    );
    assert_ne!(
        first.render(),
        other.render(),
        "two typed seeds produced the same program"
    );
    assert!(first.render().contains("fn main()"));
}

/// The three deliberate faults must be refused by `check` with an `E-OWN-*`
/// code, never silently accepted.
#[test]
fn typed_faults_are_refused_with_ownership_codes() {
    for fault in [Fault::UseAfterMove, Fault::DoubleMove, Fault::Uninit] {
        let (program, _) = generate_typed_program(0x1234_5678, 5, Some(fault));
        assert_eq!(program.fault, Some(fault));
        let source = program.render();
        match check_program(&source, "typed-fault-kind") {
            Err(message) => {
                assert!(
                    message.contains("E-OWN-"),
                    "fault {:?} must be refused with an E-OWN-* code, got: {message}",
                    fault
                );
                let first = message
                    .lines()
                    .find(|line| line.contains("E-OWN-"))
                    .unwrap_or("");
                eprintln!("[typed] fault {:?} => {}", fault, first.trim());
            }
            Ok(_) => panic!("fault {:?} was accepted by check", fault),
        }
    }
}

#[test]
fn typed_oracle_fast() {
    run_typed_mode(
        "fast",
        env_usize("RAVUM_TYPED_PROGRAMS", 40),
        0,
        false,
        true,
    );
}

#[test]
#[ignore = "long typed run: 3 seeds x 2 000 programs (RAVUM_TYPED_PROGRAMS overrides)"]
fn typed_oracle_long() {
    run_typed_mode(
        "long",
        env_usize("RAVUM_TYPED_PROGRAMS", 2_000),
        env_usize("RAVUM_TYPED_RELEASE_EVERY", 10),
        true,
        true,
    );
}
