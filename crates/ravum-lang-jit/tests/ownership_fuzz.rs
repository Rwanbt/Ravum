//! U1 exit criterion (docs/ui/PLAN.md): **extended ownership fuzzing** of the
//! RFC-UI-LANG-1 features — bounded borrows in `match` on references (OWN-6.8
//! to 6.12), `for` over `&v` / `&mut v` (OWN-6.13), `Map` views (STD-20),
//! structural clone (OWN-6.14), text primitives (STD-9.13 to 9.19).
//!
//! Unlike `differential_oracle.rs` (well-owned programs by construction),
//! this generator assembles statements **at random, legal or not**: borrows
//! held across conflicting writes, moves followed by uses, pushes during an
//! iteration, writes through views, early exits. The checker decides.
//!
//! Properties, for every generated program:
//! 1. **soundness**: if Ravum accepts it, the generated Rust compiles (rustc's
//!    borrow checker agrees) — a rustc error is a hole in Ravum's checker;
//! 2. **parity**: if Ravum accepts it, the IR path lowers it, and the rustc
//!    binary, the IR interpreter and the JIT print the same stdout and panic
//!    with the same text (or all exit normally);
//! 3. **determinism**: two `check` runs give the same verdict.
//!
//! A refusal by Ravum is allowed (the checker is conservative); the rate is
//! printed, never hidden.
//!
//! Modes: fast (`cargo test`, 3 seeds x 12 programs, needs `rustc` on the
//! path, else skipped and said so) and long (`#[ignore]`, 3 seeds x 200,
//! `RAVUM_OWN_FUZZ_PROGRAMS` to override):
//! `cargo test -p ravum-lang-jit --test ownership_fuzz -- --ignored --nocapture`.
//! On a failure, the program is written to `target/own-fuzz-failures/`.

use ravum_lang_codegen::{generate, GenOptions};
use ravum_lang_ir::interp::{run_with_output, DEFAULT_STEPS};
use ravum_lang_jit::{compile, DEFAULT_FUEL};
use ravum_lang_sema::facts::{CheckOptions, Profile};
use ravum_lang_sema::loader::{load_package_with_options, LoadOptions};
use ravum_lang_sema::{check, syntax};
use std::path::{Path, PathBuf};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const PRELUDE: &str = r#"use std::io;
use std::string;
struct Item { a: i64, s: String }
enum Shape { Dot, Box(i64, i64) }
fn total(v: &Vec<Item>) -> i64 effects() {
    let mut t: i64 = 0;
    for x in v { t += x.a; }
    return t;
}
fn show(b: &Vec<Shape>) -> () effects(io, alloc) {
    for x in b {
        match x {
            Shape::Box(w, h) => { io::println(&(*w).to_string()); io::println(&(*h).to_string()); }
            Shape::Dot => io::println("dot"),
        }
    }
}
fn bump(v: &mut Vec<Item>, k: i64) -> i64 effects() {
    for x in v {
        x.a += k;
        if x.a > 50 { return x.a; }
    }
    return 0;
}
"#;

/// Statement templates. `{k}` is a small literal, `{i}` an index (0..=2,
/// so out of bounds sometimes), `{t}` a short text. Many are illegal on
/// purpose (marked `// bad:` for the reader; the checker is not told).
const STMTS: &[&str] = &[
    "for x in &mut v { x.a += {k}; }",
    "for x in &v { if x.a > {k} { io::println(x.s.as_str()); } }",
    "for x in &mut v { if x.a > {k} { break; } x.a += 1; }",
    "for x in &mut v { if x.a == {k} { continue; } x.s.push_str(\"{t}\"); }",
    "for x in &mut v { v.push(Item { a: {k}, s: String::from(\"{t}\") }); } // bad: push during iteration",
    "for x in &v { v[0].a = {k}; } // bad: write during a shared iteration",
    "for x in &mut v { let y = x; } // bad?: a bounded borrow moved into a let",
    "match &mut b[{i}] { Shape::Box(w, h) => { *w += {k}; *h = *w; } Shape::Dot => {} }",
    "match &mut b[{i}] { Shape::Box(w, _) => { b.push(Shape::Dot); *w += 1; } Shape::Dot => {} } // bad: push during a view",
    "match &b[{i}] { Shape::Box(w, _) => { io::println(&(*w).to_string()); } Shape::Dot => { io::println(\"dot\"); } }",
    "match &mut b[{i}] { Shape::Box(w, _) => { if *w > {k} { return; } *w += 2; } Shape::Dot => {} }",
    "for x in &mut v { match &mut b[0] { Shape::Box(w, _) => { *w += x.a; } Shape::Dot => {} } }",
    "match m.get(&String::from(\"k\")) { Some(x) => { io::println(&(*x).to_string()); } None => { io::println(\"none\"); } }",
    "match m.get_mut(&String::from(\"k\")) { Some(x) => { *x += {k}; } None => {} }",
    "match m.get_mut(&String::from(\"k\")) { Some(x) => { let _o = m.insert(String::from(\"z\"), 1); *x += 1; } None => {} } // bad: insert during a view",
    "let _o = m.insert(String::from(\"{t}\"), {k});",
    "for (key, x) in &mut m { *x += {k}; io::println(key.as_str()); }",
    "for (key, x) in &m { let _o = m.insert(String::from(\"w\"), *x); } // bad: insert during iteration",
    "let c = v.clone(); io::println(&total(&c).to_string());",
    "let c = b.clone(); show(&c);",
    "let w = v; v = w;",
    "let w = v; io::println(&total(&w).to_string()); // bad: v used later after the move",
    "let r = bump(&mut v, {k}); io::println(&r.to_string());",
    "let parts = s.split(\",\"); for p in &parts { io::println(p.as_str()); }",
    "match s.find(\",\") { Some(i) => { s = s.substring(0, i); } None => { s.push_str(\",{t}\"); } }",
    "match s.strip_prefix(\"{t}\") { Some(r) => { s = r; } None => {} }",
    "let cs = s.chars(); io::println(&cs.len().to_string());",
    "let mut k2: i64 = 0; while k2 < 2 { for x in &mut v { x.a += k2; } k2 += 1; }",
    "for x in &v { s.push_str(x.s.as_str()); }",
    "for x in &v { s = x.s; } // bad: move out of a borrow",
];

fn instantiate(t: &str, rng: &mut Rng) -> String {
    let texts = ["a", "k", "xy", "é", ""];
    let mut out = t.to_string();
    while out.contains("{k}") {
        out = out.replacen("{k}", &(rng.below(8) as i64).to_string(), 1);
    }
    while out.contains("{i}") {
        out = out.replacen("{i}", &rng.below(3).to_string(), 1);
    }
    while out.contains("{t}") {
        out = out.replacen("{t}", texts[rng.below(texts.len())], 1);
    }
    out
}

fn program(seed: u64) -> String {
    let mut rng = Rng(seed | 1);
    let n = 2 + rng.below(5);
    let mut body = String::new();
    for _ in 0..n {
        let t = STMTS[rng.below(STMTS.len())];
        body.push_str("    ");
        body.push_str(&instantiate(t, &mut rng));
        body.push('\n');
    }
    format!(
        "{PRELUDE}fn main() -> () effects(io, alloc) {{
    let mut v: Vec<Item> = Vec::new();
    v.push(Item {{ a: 1, s: String::from(\"x\") }});
    v.push(Item {{ a: 2, s: String::from(\"y\") }});
    let mut b: Vec<Shape> = Vec::new();
    b.push(Shape::Box(1, 1));
    b.push(Shape::Dot);
    let mut m: Map<String, i64> = Map::new();
    let _o = m.insert(String::from(\"k\"), 3);
    let mut s = String::from(\"a,b,c\");
{body}    io::println(&total(&v).to_string());
    show(&b);
    for (key, x) in &m {{ io::print(key.as_str()); io::print(\"=\"); io::println(&(*x).to_string()); }}
    io::println(s.as_str());
    io::println(string::join(&s.split(\",\"), \"|\").as_str());
}}
"
    )
}

// ------------------------------------------------------------------ paths

struct Tmp(PathBuf);
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tmp(label: &str) -> Tmp {
    let p = std::env::temp_dir().join(format!("ravum-ownfuzz-{}-{label}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(p.join("src")).unwrap();
    Tmp(p)
}

/// `Ok(rust)` when Ravum accepts; `Err(codes)` when it refuses.
fn ravum_check(src: &str, label: &str) -> Result<String, String> {
    let d = tmp(label);
    std::fs::write(
        d.0.join("ravum.toml"),
        format!("[package]\nname = \"{label}\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n"),
    )
    .unwrap();
    std::fs::write(d.0.join("src/main.rv"), src).unwrap();
    let loaded = load_package_with_options(
        &d.0,
        &LoadOptions {
            locked: false,
            ..Default::default()
        },
    );
    if loaded.diags.has_errors() {
        return Err(loaded.diags.render(&loaded.sources));
    }
    let program = loaded.program.expect("loader produced no program");
    let r = check(
        &program,
        &loaded.sources,
        &CheckOptions {
            profile: Profile::Core,
            edition: syntax::LANGUAGE_EDITION.to_string(),
        },
    );
    if r.diags.has_errors() {
        return Err(r.diags.render(&loaded.sources));
    }
    let checked = r.checked.expect("no CheckedProgram");
    generate(&checked, &GenOptions::default()).map_err(|e| format!("CODEGEN: {}", e.message))
}

fn rustc_run(rust: &str, label: &str) -> Result<(String, String, i32), String> {
    let d = tmp(&format!("{label}-rs"));
    let src = d.0.join("p.rs");
    let bin = d.0.join(if cfg!(windows) { "p.exe" } else { "p" });
    std::fs::write(&src, rust).unwrap();
    let rustc = std::env::var("RAVUM_RUSTC").unwrap_or_else(|_| "rustc".into());
    let o = std::process::Command::new(rustc)
        .args([
            "--edition",
            "2021",
            "-C",
            "overflow-checks=on",
            "-C",
            "opt-level=0",
            "--crate-name",
            "own_fuzz",
            "-o",
        ])
        .arg(&bin)
        .arg(&src)
        .output()
        .expect("cannot run rustc");
    if !o.status.success() {
        return Err(String::from_utf8_lossy(&o.stderr).into_owned());
    }
    let r = std::process::Command::new(&bin)
        .output()
        .expect("cannot run the binary");
    Ok((
        String::from_utf8_lossy(&r.stdout).into_owned(),
        String::from_utf8_lossy(&r.stderr).into_owned(),
        r.status.code().unwrap_or(-1),
    ))
}

/// The panic text of a rustc-path run (`panic: <text>` on stderr), if any.
fn panic_text(stderr: &str) -> Option<String> {
    stderr
        .lines()
        .find_map(|l| l.strip_prefix("panic: ").map(str::to_string))
}

#[derive(Default, Debug)]
struct Tally {
    accepted: usize,
    refused: usize,
    failures: Vec<String>,
}

fn save_failure(seed: u64, src: &str, why: &str) -> String {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/own-fuzz-failures");
    let _ = std::fs::create_dir_all(&dir);
    let f = dir.join(format!("seed-{seed:016x}.rv"));
    let _ = std::fs::write(&f, format!("// {why}\n{src}"));
    format!("seed {seed:#018x}: {why} ({})", f.display())
}

fn one(seed: u64, t: &mut Tally) {
    let src = program(seed);
    let label = format!("s{seed:016x}");
    let first = ravum_check(&src, &label);
    let again = ravum_check(&src, &format!("{label}b"));
    if first.is_ok() != again.is_ok() {
        t.failures
            .push(save_failure(seed, &src, "non-deterministic check verdict"));
        return;
    }
    let rust = match first {
        Err(e) if e.starts_with("CODEGEN:") => {
            t.failures.push(save_failure(seed, &src, &e));
            return;
        }
        Err(_) => {
            t.refused += 1;
            return;
        }
        Ok(r) => r,
    };
    t.accepted += 1;
    // 1. Soundness: rustc's borrow checker must agree.
    let (out, err, _code) = match rustc_run(&rust, &label) {
        Ok(x) => x,
        Err(e) => {
            let head: String = e.lines().take(6).collect::<Vec<_>>().join(" | ");
            t.failures.push(save_failure(
                seed,
                &src,
                &format!("SOUNDNESS: accepted by Ravum, rejected by rustc: {head}"),
            ));
            return;
        }
    };
    let b_panic = panic_text(&err);
    // 2. Parity: the IR path lowers it and agrees.
    let ir = match ravum_lang_ir::compile_source(&src) {
        Ok(ir) => ir,
        Err(e) => {
            t.failures.push(save_failure(
                seed,
                &src,
                &format!("PARITY: accepted, but the IR path refuses it: {e:?}"),
            ));
            return;
        }
    };
    let (iv, io) = run_with_output(&ir, "main", DEFAULT_STEPS);
    let (jv, jo) = compile(&ir)
        .expect("JIT")
        .run_with_output("main", DEFAULT_FUEL);
    if io.stdout != jo.stdout || iv.render() != jv.render() {
        t.failures.push(save_failure(
            seed,
            &src,
            "PARITY: interpreter and JIT disagree",
        ));
        return;
    }
    if io.stdout != out {
        t.failures.push(save_failure(
            seed,
            &src,
            &format!(
                "PARITY: stdout differs\n// rustc: {out:?}\n// IR:    {:?}",
                io.stdout
            ),
        ));
        return;
    }
    let c_render = iv.render();
    let c_panicked = c_render.contains("panic") || c_render.contains("trap");
    match (&b_panic, c_panicked) {
        (Some(text), true) if !c_render.contains(text.as_str()) => {
            t.failures.push(save_failure(
                seed,
                &src,
                &format!("PARITY: panic text differs: rustc {text:?}, IR {c_render:?}"),
            ));
        }
        (Some(_), false) | (None, true) => {
            t.failures.push(save_failure(
                seed,
                &src,
                &format!("PARITY: one path panicked: rustc {b_panic:?}, IR {c_render:?}"),
            ));
        }
        _ => {}
    }
}

fn campaign(programs: usize) {
    if std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .map(|o| !o.status.success())
        .unwrap_or(true)
    {
        eprintln!("ownership_fuzz: rustc not found — NOT_RUN (never a PASS)");
        return;
    }
    let mut t = Tally::default();
    for base in [0x5EED_0001u64, 0x5EED_0002, 0x5EED_0003] {
        for i in 0..programs as u64 {
            one(
                base.wrapping_mul(0x9E37_79B9)
                    .wrapping_add(i * 0x1_0000_0001),
                &mut t,
            );
        }
    }
    eprintln!(
        "ownership_fuzz: {} programs, {} accepted (all checked), {} refused by the checker, {} failure(s)",
        3 * programs,
        t.accepted,
        t.refused,
        t.failures.len()
    );
    assert!(
        t.accepted > 0,
        "the generator produced no accepted program: it tests nothing"
    );
    assert!(
        t.refused > 0,
        "the generator produced no refused program: it does not exercise refusals"
    );
    assert!(
        t.failures.is_empty(),
        "ownership fuzz failures:\n{}",
        t.failures.join("\n")
    );
}

#[test]
fn ownership_fuzz_fast() {
    campaign(12);
}

#[test]
#[ignore]
fn ownership_fuzz_long() {
    campaign(
        std::env::var("RAVUM_OWN_FUZZ_PROGRAMS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(200),
    );
}
