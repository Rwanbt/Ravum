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
        },
    );
    if !loaded.diags.is_empty() {
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
    if !result.diags.is_empty() {
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
