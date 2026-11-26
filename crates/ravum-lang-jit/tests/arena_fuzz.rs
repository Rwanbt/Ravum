//! RFC-UI-LANG-2 P2: differential fuzz of `Arena<T>` / `Handle<T>`. Random
//! **legal** programs (inserts, removals of possibly stale handles, handles
//! of another arena, `get` / `get_mut` in a `match`, iteration over `&a` and
//! `&mut a` with `break`, clones, handles as `Map` keys, aggregate elements
//! with a `String`) run on the three engines: rustc (path B, the oracle),
//! the IR interpreter and the JIT. Their outputs and verdicts must be equal.
//!
//! Fast mode (`cargo test`): 2 seeds x 10 programs. Long mode (`--ignored`):
//! 3 seeds x 100. Failures are written to `target/arena-fuzz-failures/`.

use ravum_lang_codegen::{generate, GenOptions};
use ravum_lang_ir::compile_source;
use ravum_lang_ir::interp::{run_with_output, DEFAULT_STEPS};
use ravum_lang_jit::{compile, DEFAULT_FUEL};
use ravum_lang_sema::facts::{CheckOptions, Profile};
use ravum_lang_sema::loader::{load_package_with_options, LoadOptions};
use ravum_lang_sema::{check, syntax};
use std::path::PathBuf;

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
struct Item { a: i64, s: String }
fn say(b: bool) -> () effects(io) { if b { io::println("t"); } else { io::println("f"); } }
fn main() -> () effects(io, alloc, panic) {
    let mut a: Arena<i64> = Arena::new();
    let mut b: Arena<i64> = Arena::new();
    let mut c: Arena<Item> = Arena::new();
    let mut hs: Vec<Handle<i64>> = Vec::new();
    let mut hc: Vec<Handle<Item>> = Vec::new();
    let mut m: Map<Handle<i64>, i64> = Map::new();
    hs.push(a.insert(1i64));
    hc.push(c.insert(Item { a: 1, s: String::from("i") }));
"#;

/// `$K` is a random index, `$N` a small value. Every template is legal.
const STMTS: &[&str] = &[
    "    hs.push(a.insert($Ni64));\n",
    "    hs.push(a.insert($Ni64));\n",
    "    hs.push(b.insert($Ni64));\n",
    "    if hs.len() > 0 { let h = hs[$K % hs.len()]; match a.remove(h) { Some(x) => io::println(&x.to_string()), None => io::println(\"-\") } }\n",
    "    if hs.len() > 0 { let h = hs[$K % hs.len()]; match a.remove(h) { Some(x) => io::println(&x.to_string()), None => io::println(\"-\") } }\n",
    "    if hs.len() > 0 { let h = hs[$K % hs.len()]; match a.get(h) { Some(x) => io::println(&x.to_string()), None => io::println(\"none\") } }\n",
    "    if hs.len() > 0 { let h = hs[$K % hs.len()]; match a.get_mut(h) { Some(x) => { *x = *x + $N; } None => {} } }\n",
    "    if hs.len() > 0 { say(a.contains(hs[$K % hs.len()])); say(b.contains(hs[$K % hs.len()])); }\n",
    "    for (h, x) in &a { io::println(&x.to_string()); drop(h); }\n",
    "    for (h, x) in &mut a { *x = *x - $N; drop(h); }\n",
    "    for (h, x) in &mut a { *x = *x + 1; if *x > $N { break; } drop(h); }\n",
    "    { let d = a.clone(); io::println(&d.len().to_string()); if hs.len() > 0 { say(d.contains(hs[$K % hs.len()])); } }\n",
    "    if hs.len() > 0 { let _o = m.insert(hs[$K % hs.len()], $Ni64); }\n",
    "    for (h, v) in &m { io::println(&v.to_string()); say(a.contains(*h)); }\n",
    "    hc.push(c.insert(Item { a: $N, s: String::from(\"s$N\") }));\n",
    "    if hc.len() > 0 { let h = hc[$K % hc.len()]; match c.get_mut(h) { Some(it) => { it.a = it.a + 1; it.s.push_str(\"!\"); } None => {} } }\n",
    "    if hc.len() > 0 { let h = hc[$K % hc.len()]; match c.remove(h) { Some(it) => io::println(it.s.as_str()), None => io::println(\"-\") } }\n",
    "    for (h, it) in &c { io::println(it.s.as_str()); io::println(&it.a.to_string()); drop(h); }\n",
    "    for (h, it) in &mut c { it.s.push_str(\"+\"); drop(h); }\n",
    "    { let e = c.clone(); for (h, it) in &e { io::println(it.s.as_str()); drop(h); } }\n",
    "    io::println(&a.len().to_string()); say(a.is_empty());\n",
    "    if hs.len() > 1 { say(hs[0] == hs[hs.len() - 1]); say(hs[0] != hs[$K % hs.len()]); }\n",
];

fn program(seed: u64) -> String {
    let mut rng = Rng(seed | 1);
    let mut src = String::from(PRELUDE);
    for _ in 0..(10 + rng.below(16)) {
        let t = STMTS[rng.below(STMTS.len())];
        let k = rng.below(7).to_string();
        let n = rng.below(9).to_string();
        src.push_str(&t.replace("$K", &k).replace("$N", &n));
    }
    src.push_str("    for (h, x) in &a { io::println(&x.to_string()); drop(h); }\n}\n");
    src
}

struct Tmp(PathBuf);
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn path_b(src: &str, label: &str) -> String {
    let d =
        Tmp(std::env::temp_dir().join(format!("ravum-arenafuzz-{}-{label}", std::process::id())));
    let _ = std::fs::remove_dir_all(&d.0);
    std::fs::create_dir_all(d.0.join("src")).unwrap();
    std::fs::write(
        d.0.join("ravum.toml"),
        format!("[package]\nname = \"af{label}\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n"),
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
    assert!(
        !loaded.diags.has_errors(),
        "{}",
        loaded.diags.render(&loaded.sources)
    );
    let program = loaded.program.unwrap();
    let r = check(
        &program,
        &loaded.sources,
        &CheckOptions {
            profile: Profile::Core,
            edition: syntax::LANGUAGE_EDITION.to_string(),
        },
    );
    assert!(
        !r.diags.has_errors(),
        "generator produced a refused program:\n{}\n{src}",
        r.diags.render(&loaded.sources)
    );
    let rust = generate(&r.checked.unwrap(), &GenOptions::default()).expect("codegen");
    let file = d.0.join("p.rs");
    let bin = d.0.join(if cfg!(windows) { "p.exe" } else { "p" });
    std::fs::write(&file, rust).unwrap();
    let rustc = std::env::var("RAVUM_RUSTC").unwrap_or_else(|_| "rustc".into());
    let o = std::process::Command::new(rustc)
        .args([
            "--edition",
            "2021",
            "-C",
            "overflow-checks=on",
            "-A",
            "warnings",
            "-o",
        ])
        .arg(&bin)
        .arg(&file)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let r = std::process::Command::new(&bin).output().unwrap();
    let stderr = String::from_utf8_lossy(&r.stderr);
    let mut out = String::from_utf8_lossy(&r.stdout).into_owned();
    if let Some(p) = stderr.lines().find(|l| l.starts_with("panic: ")) {
        out.push_str(p);
    }
    out
}

fn one(seed: u64) -> Result<(), String> {
    let src = program(seed);
    let label = format!("{seed:x}");
    let want = path_b(&src, &label);
    let ir = compile_source(&src).map_err(|e| format!("IR refused: {e:?}\n{src}"))?;
    let (i, io) = run_with_output(&ir, "main", DEFAULT_STEPS);
    let (n, no) = compile(&ir)
        .map_err(|e| format!("JIT: {e:?}"))?
        .run_with_output("main", DEFAULT_FUEL);
    let with_panic = |out: &str, render: &str| {
        let mut s = out.to_string();
        if let Some(p) = render.strip_prefix("panic: ") {
            s.push_str("panic: ");
            s.push_str(p);
        }
        s
    };
    let gi = with_panic(&io.stdout, &i.render());
    let gn = with_panic(&no.stdout, &n.render());
    if gi != want || gn != want {
        let dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/arena-fuzz-failures");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(format!("{label}.rv")), &src);
        return Err(format!(
            "seed {label}: B {want:?}\ninterp {gi:?}\njit {gn:?}\n{src}"
        ));
    }
    Ok(())
}

fn campaign(seeds: &[u64], programs: u64) {
    let mut failures = Vec::new();
    let mut n = 0;
    for s in seeds {
        for k in 0..programs {
            if let Err(e) = one(s.wrapping_mul(1_000_003).wrapping_add(k)) {
                failures.push(e);
            }
            n += 1;
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {n} programs disagree:\n{}",
        failures.len(),
        failures.join("\n---\n")
    );
    eprintln!("arena fuzz: {n} programs, B = interpreter = JIT");
}

#[test]
fn arena_fuzz_fast() {
    campaign(&[1, 2], 10);
}

#[test]
#[ignore]
fn arena_fuzz_long() {
    campaign(&[11, 12, 13], 100);
}
