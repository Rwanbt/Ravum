//! T15 differential oracle: native code (Cranelift JIT) vs the IR reference
//! interpreter vs the independent reference machine, on the machine corpus
//! and on generated arithmetic programs.

use std::path::Path;

use ravum_lang_ir::compile_source;
use ravum_lang_ir::interp::{run, DEFAULT_STEPS};
use ravum_lang_jit::{compile, DEFAULT_FUEL};

fn machine_corpus() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../ravum-core-machine/corpus");
    let mut out: Vec<(String, String)> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "rv"))
        .map(|p| {
            (
                p.file_stem().unwrap().to_string_lossy().into_owned(),
                std::fs::read_to_string(&p).unwrap(),
            )
        })
        .collect();
    out.sort();
    out
}

#[test]
fn native_code_agrees_with_interpreter_and_reference_machine() {
    let mut compared = 0;
    let mut failures = Vec::new();
    for (name, src) in machine_corpus() {
        let ir = match compile_source(&src) {
            Ok(ir) => ir,
            Err(_)
                if matches!(
                    name.as_str(),
                    "x-libm-refused" | "x-or-pattern-refused" | "x-range-pattern-refused"
                ) =>
            {
                continue
            }
            Err(err) => {
                failures.push(format!("{name}: unexpected frontend refusal: {err:?}"));
                continue;
            }
        };
        let expected = src
            .lines()
            .next()
            .unwrap()
            .strip_prefix("// oracle: ")
            .unwrap()
            .to_string();
        let interp = run(&ir, "check", DEFAULT_STEPS).render();
        let native = match compile(&ir) {
            Ok(c) => c.run("check", DEFAULT_FUEL).render(),
            Err(e) => {
                failures.push(format!("{name}: JIT failed: {e}"));
                continue;
            }
        };
        compared += 1;
        if native != interp {
            failures.push(format!(
                "{name}: native `{native}` != interpreter `{interp}`"
            ));
        }
        if expected.starts_with("diverge") && native != "diverge" {
            failures.push(format!(
                "{name}: expected divergence, native gave `{native}`"
            ));
        } else if !expected.starts_with("diverge")
            && !expected.starts_with("fault")
            && native != expected
        {
            failures.push(format!("{name}: native `{native}` != machine `{expected}`"));
        }
        if expected.starts_with("fault") && native.starts_with("value") {
            failures.push(format!(
                "{name}: machine faults, native produced `{native}`"
            ));
        }
    }
    eprintln!("T15 corpus: {compared} programs compared");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(compared >= 25);
}

/// Deterministic generator of small arithmetic programs over every integer
/// type, covering overflow, division and shift traps and casts.
fn generated() -> Vec<String> {
    let tys = [
        "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "isize", "usize",
    ];
    let ops = ["+", "-", "*", "/", "%", "&", "|", "^", "<<", ">>"];
    let mut seed: u64 = 0x5eed_1234_abcd_0001;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut out = Vec::new();
    for ty in tys {
        let signed = ty.starts_with('i');
        let max: i128 = match ty {
            "i8" => 127,
            "u8" => 255,
            "i16" => 32767,
            "u16" => 65535,
            "i32" => i32::MAX as i128,
            "u32" => u32::MAX as i128,
            "i64" | "isize" => i64::MAX as i128,
            _ => u64::MAX as i128,
        };
        let pick = |r: u64| -> i128 {
            let edges: [i128; 6] = [0, 1, 2, max, max - 1, max / 3];
            let e = edges[(r % 6) as usize];
            if signed && r.is_multiple_of(7) {
                -e - 1
            } else if signed && r.is_multiple_of(5) {
                -e
            } else {
                e
            }
        };
        for op in ops {
            for _ in 0..6 {
                let (a, b) = (pick(next()), pick(next()));
                let b = if matches!(op, "<<" | ">>") {
                    (next() % 70) as i128
                } else {
                    b
                };
                let rhs_ty = if matches!(op, "<<" | ">>") { "u32" } else { ty };
                out.push(format!(
                    "fn f(a: {ty}, b: {rhs_ty}) -> {ty} effects() {{\n    return a {op} b;\n}}\n\nfn check() -> {ty} effects() {{\n    let x: {ty} = {a};\n    let y: {rhs_ty} = {b};\n    return f(x, y);\n}}\n"
                ));
            }
            // Casts to every other type and to floats.
            let a = pick(next());
            for to in tys.iter().chain(["f32", "f64"].iter()) {
                out.push(format!(
                    "fn check() -> {to} effects() {{\n    let x: {ty} = {a};\n    return x as {to};\n}}\n"
                ));
            }
        }
    }
    for (v, to) in [
        ("1.0e300", "i32"),
        ("-1.0e300", "u8"),
        ("2.5", "u16"),
        ("-2.5", "i8"),
        ("4294967296.0", "u32"),
    ] {
        out.push(format!(
            "fn check() -> {to} effects() {{\n    let z: f64 = {v};\n    return z as {to};\n}}\n"
        ));
    }
    out
}

#[test]
fn native_code_agrees_with_interpreter_on_generated_programs() {
    let mut compared = 0;
    let mut panics = 0;
    let mut failures = Vec::new();
    for src in generated() {
        let Ok(ir) = compile_source(&src) else {
            continue;
        };
        let interp = run(&ir, "check", DEFAULT_STEPS).render();
        let native = compile(&ir).unwrap().run("check", DEFAULT_FUEL).render();
        compared += 1;
        panics += interp.starts_with("panic") as usize;
        if native != interp {
            failures.push(format!(
                "native `{native}` != interpreter `{interp}` for\n{src}"
            ));
        }
    }
    eprintln!("T15 generated: {compared} programs compared, {panics} trap");
    assert!(
        failures.is_empty(),
        "{} divergences:\n{}",
        failures.len(),
        failures.join("\n---\n")
    );
    assert!(
        compared >= 500 && panics >= 50,
        "generator lost power: {compared}/{panics}"
    );
}

#[test]
fn deep_recursion_and_endless_loops_are_reported_not_crashes() {
    let rec = "fn f(n: i64) -> i64 effects() {\n    return f(n + 1);\n}\n\nfn check() -> i64 effects() {\n    return f(0);\n}\n";
    let ir = compile_source(rec).unwrap();
    assert_eq!(
        compile(&ir).unwrap().run("check", DEFAULT_FUEL).render(),
        "fault: call depth limit"
    );
    let spin = "fn check() -> i64 effects() {\n    let mut i: i64 = 0;\n    while i >= 0 {\n        i = i * 1;\n    }\n    return i;\n}\n";
    let ir = compile_source(spin).unwrap();
    assert_eq!(
        compile(&ir).unwrap().run("check", 10_000).render(),
        "diverge"
    );
}

/// Synthetic program used by the build-time comparison (`scripts` in the T15
/// report): `n` pure functions chained by calls in runs of 100 (so the call
/// depth stays under the limit), plus `check` and `main`.
pub fn synthetic(n: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        let callee = if i % 100 == 0 {
            "x + 1".to_string()
        } else {
            format!("f{}(x % 1000) + 1", i - 1)
        };
        s.push_str(&format!(
            "fn f{i}(x: i64) -> i64 effects() {{\n    let mut acc: i64 = 0;\n    let lim: i64 = 4;\n    for k in 0..lim {{\n        acc = acc + k * 3 - (x % 7);\n    }}\n    if acc > 100 {{\n        acc = acc / 2;\n    }}\n    return acc + {callee};\n}}\n\n"
        ));
    }
    s.push_str(&format!("fn check() -> i64 effects() {{\n    return f{}(5);\n}}\n\nfn main() {{\n    let r = check();\n}}\n", n - 1));
    s
}

#[test]
#[ignore]
fn jit_build_time() {
    // cargo test -p ravum-lang-jit --release --test differential jit_build_time -- --ignored --nocapture
    let sizes: Vec<usize> = match std::env::var("RV_N") {
        Ok(v) => vec![v.parse().unwrap()],
        Err(_) => vec![100, 800],
    };
    for n in sizes {
        let src = synthetic(n);
        let out = std::env::temp_dir().join(format!("rv-jit-synth-{n}.rv"));
        std::fs::write(&out, &src).unwrap();
        let mut best = (u128::MAX, u128::MAX);
        let mut result = String::new();
        for _ in 0..5 {
            let t0 = std::time::Instant::now();
            let ir = compile_source(&src).expect("synthetic program lowers");
            let t1 = std::time::Instant::now();
            let c = compile(&ir).unwrap();
            let t2 = std::time::Instant::now();
            result = c.run("check", DEFAULT_FUEL).render();
            best.0 = best.0.min((t1 - t0).as_micros());
            best.1 = best.1.min((t2 - t1).as_micros());
        }
        assert_eq!(
            result,
            run(&compile_source(&src).unwrap(), "check", DEFAULT_STEPS).render()
        );
        eprintln!(
            "n={n} lines={} front(parse+check+lower)={}us jit={}us result={result} source={}",
            src.lines().count(),
            best.0,
            best.1,
            out.display()
        );
    }
}

#[test]
#[ignore]
fn ir_size_probe() {
    let ir = compile_source(&synthetic(3)).unwrap();
    for f in &ir.functions {
        let insts: usize = f.blocks.iter().map(|b| b.insts.len()).sum();
        eprintln!(
            "{} locals={} blocks={} insts={}",
            f.name,
            f.locals.len(),
            f.blocks.len(),
            insts
        );
    }
}

/// IR v1.1: aggregates in memory. Nested structs, arrays of structs, by-value
/// arguments (must be copies), aggregate returns (sret), `&mut` arguments,
/// bounds and overflow traps.
fn aggregate_programs(by_ref: bool) -> Vec<String> {
    let mut seed: u64 = 0xa66e_6a7e_0000_0011;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut out = Vec::new();
    for _ in 0..300 {
        let (x, y, z) = (
            (next() % 200) as i32 - 100,
            (next() % 200) as i32 - 100,
            (next() % 200) as i32 - 100,
        );
        let n = [0u8, 1, 200, 254, 255][(next() % 5) as usize];
        let i = next() % 3; // 2 is out of bounds for `k: [V; 2]`
        let j = next() % 4; // 3 is out of bounds for `c: [i16; 3]`
        let (bump, call) = if by_ref {
            (
                "fn bump(w: &mut W, i: usize, j: usize) -> () effects() {\n    w.k[i].a += 1;\n    w.k[i].c[j] = w.k[i].c[j] * 2;\n    w.n = w.n + 1;\n}\n\n".to_string(),
                format!("bump(&mut w, {i}, {j});"),
            )
        } else {
            (
                "fn bump(w: W, i: usize, j: usize) -> W effects() {\n    let mut r = w;\n    r.k[i].a += 1;\n    r.k[i].c[j] = r.k[i].c[j] * 2;\n    r.n = r.n + 1;\n    return r;\n}\n\n".to_string(),
                format!("w = bump(w, {i}, {j});"),
            )
        };
        out.push(format!(
            "struct V {{\n    a: i32,\n    b: f64,\n    c: [i16; 3],\n    d: bool,\n}}\n\nstruct W {{\n    v: V,\n    k: [V; 2],\n    n: u8,\n}}\n\n\
fn mk(x: i32) -> V effects() {{\n    return V {{ a: x, b: (x as f64) * 0.5, c: [x as i16, 2, 3], d: x > 0 }};\n}}\n\n\
{bump}\
fn take(v: V) -> i32 effects() {{\n    let mut u = v;\n    u.a = 100;\n    return u.a + (u.c[1] as i32);\n}}\n\n\
fn swap_first(w: W) -> W effects() {{\n    let mut r = w;\n    let t = r.k[0];\n    r.k[0] = r.k[1];\n    r.k[1] = t;\n    return r;\n}}\n\n\
fn check() -> i64 effects() {{\n    let mut w = W {{ v: mk({x}), k: [mk({y}), mk({z})], n: {n} }};\n    let before = take(w.v);\n    {call}\n    let s = swap_first(w);\n    let mut acc: i64 = (before as i64) + (w.n as i64);\n    let two: usize = 2;\n    for q in 0..two {{\n        acc = acc * 3 + (s.k[q].a as i64) + (s.k[q].c[0] as i64) + (w.k[q].c[2] as i64);\n        if s.k[q].d {{\n            acc = acc + 7;\n        }}\n    }}\n    if w.v.b > 0.0 {{\n        acc = acc + 1;\n    }}\n    return acc + (w.v.a as i64);\n}}\n"
        ));
    }
    out
}

#[test]
fn native_code_agrees_with_interpreter_on_aggregate_programs() {
    let (mut compared, mut traps, mut lowered_fail) = (0, 0, 0);
    let mut kinds = std::collections::BTreeMap::new();
    let mut failures = Vec::new();
    let mut machine_agreed = 0;
    let mut machine_other = Vec::new();
    let programs: Vec<(bool, String)> = aggregate_programs(true)
        .into_iter()
        .map(|p| (true, p))
        .chain(aggregate_programs(false).into_iter().map(|p| (false, p)))
        .collect();
    for (by_ref, src) in programs {
        let ir = match compile_source(&src) {
            Ok(ir) => ir,
            Err(e) => {
                lowered_fail += 1;
                if lowered_fail == 1 {
                    eprintln!("first rejection: {e:?}");
                }
                continue;
            }
        };
        let interp = run(&ir, "check", DEFAULT_STEPS).render();
        let native = compile(&ir).unwrap().run("check", DEFAULT_FUEL).render();
        compared += 1;
        if interp.starts_with("panic") {
            traps += 1;
            *kinds.entry(interp.clone()).or_insert(0) += 1;
        }
        if native != interp {
            failures.push(format!(
                "native `{native}` != interpreter `{interp}` for\n{src}"
            ));
        }
        if !by_ref {
            // Independent oracle: the reference machine shares no code with
            // the compiler (MACH-1.3).
            let m = ravum_core_machine::run(
                &src,
                "check",
                ravum_core_machine::SPEC_VERSION,
                ravum_core_machine::DEFAULT_BUDGET,
            )
            .render();
            if m.starts_with("value") || m.starts_with("panic") {
                if m == native {
                    machine_agreed += 1;
                } else {
                    failures.push(format!("native `{native}` != machine `{m}` for\n{src}"));
                }
            } else {
                machine_other.push(m);
            }
        }
    }
    eprintln!(
        "  vs reference machine: {machine_agreed} identical, {} inconclusive {:?}",
        machine_other.len(),
        machine_other.first()
    );
    eprintln!("T15/IR1.1 aggregates: {compared} compared, {traps} traps {kinds:?}, {lowered_fail} not lowered");
    assert_eq!(lowered_fail, 0);
    assert!(
        failures.is_empty(),
        "{} divergences:\n{}",
        failures.len(),
        failures.join("\n---\n")
    );
    assert!(
        compared >= 600 && traps >= 100 && kinds.len() >= 2,
        "generator lost power"
    );
    assert!(
        machine_agreed >= 250,
        "independent oracle compared too little: {machine_agreed}"
    );
}
