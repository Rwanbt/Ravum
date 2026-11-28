//! T14 oracle: IR v1 against the independent reference machine
//! (`ravum-core-machine`, which shares no code with the compiler).
//!
//! Every corpus program of the machine is compiled to IR. When lowering
//! succeeds, the IR interpreter must give **exactly** the machine's rendered
//! outcome; when it fails, the failure must be an explicit `Unsupported` or a
//! checker rejection — never a silent wrong value. The JSON round trip must be
//! lossless.

use std::path::Path;

use ravum_lang_ir::interp::{run, DEFAULT_STEPS};
use ravum_lang_ir::lower::LowerError;
use ravum_lang_ir::{compile_source, CompileError, Module};

pub fn machine_corpus() -> Vec<(String, String)> {
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
fn ir_interpreter_agrees_with_the_reference_machine() {
    let corpus = machine_corpus();
    assert!(corpus.len() >= 50);
    let (mut agreed, mut unsupported, mut rejected) = (0, Vec::new(), Vec::new());
    let mut failures = Vec::new();
    for (name, src) in &corpus {
        let expected = src
            .lines()
            .next()
            .unwrap()
            .strip_prefix("// oracle: ")
            .unwrap()
            .to_string();
        match compile_source(src) {
            Ok(m) => {
                let got = run(&m, "check", DEFAULT_STEPS).render();
                if expected.starts_with("diverge") {
                    if got != "diverge" {
                        failures.push(format!("{name}: expected divergence, IR gave `{got}`"));
                    }
                } else if expected.starts_with("fault") {
                    if got.starts_with("value") {
                        failures.push(format!("{name}: machine `{expected}`, IR produced `{got}`"));
                    }
                } else if got != expected {
                    // Note: EXPR-7.6 asks for `index out of bounds: the len is N
                    // but the index is i`; the machine and IR v1.1 both give the
                    // short form (see the T14 report).
                    failures.push(format!("{name}: expected `{expected}`, IR gave `{got}`"));
                } else {
                    agreed += 1;
                }
            }
            Err(CompileError::Lower(LowerError::Unsupported(what))) => {
                if !matches!(name.as_str(), "o3-u128-max" | "x-libm-refused") {
                    failures.push(format!("{name}: unexpected lowering refusal: {what}"));
                }
                unsupported.push(format!("{name} ({what})"))
            }
            Err(CompileError::Lower(e)) => failures.push(format!("{name}: lowering bug: {e}")),
            Err(CompileError::Rejected(codes)) => {
                if !matches!(
                    name.as_str(),
                    "x-or-pattern-refused" | "x-range-pattern-refused"
                ) {
                    failures.push(format!("{name}: unexpected checker refusal: {codes:?}"));
                }
                rejected.push(format!("{name} {codes:?}"));
            }
        }
    }
    eprintln!(
        "T14 oracle: {} programs, {agreed} agree, {} unsupported, {} rejected by the checker",
        corpus.len(),
        unsupported.len(),
        rejected.len()
    );
    eprintln!("  unsupported: {}", unsupported.join(", "));
    eprintln!("  rejected: {}", rejected.join(", "));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        agreed >= 25,
        "IR v1 covers too little of the corpus: {agreed}"
    );
}

#[test]
fn json_round_trip_is_lossless_and_versioned() {
    let mut n = 0;
    for (_, src) in machine_corpus() {
        if let Ok(m) = compile_source(&src) {
            let json = m.to_json();
            assert_eq!(Module::from_json(&json).unwrap(), m);
            n += 1;
        }
    }
    assert!(n > 0);
    // Negative: an unknown version is refused (fail-closed).
    let m = compile_source("fn check() -> i32 effects() {\n    return 1;\n}\n").unwrap();
    let bad = m
        .to_json()
        .replace(ravum_lang_ir::IR_VERSION, "ravum-ir-999");
    assert!(Module::from_json(&bad).is_err());
    // Negative: an out-of-range local is refused by validation.
    let mut broken = m.clone();
    broken.functions[0].blocks[0]
        .insts
        .push(ravum_lang_ir::ir::Inst::Copy { dst: 999, src: 0 });
    assert!(broken.validate().is_err());
}

/// Fail-closed for `Term::TrapBounds` (IR 1.5): each operand must be an
/// in-range local of type `u64`; the validator, the interpreter and the JIT
/// all refuse a module that breaks this, and never index with it.
#[test]
fn a_malformed_bounds_trap_is_refused_by_every_backend() {
    use ravum_lang_ir::ir::Term;
    let m = compile_source(
        "fn check(i: usize) -> i64 effects() {\n    let a: [i64; 3] = [1, 2, 3];\n    return a[i];\n}\n\nfn entry() -> i64 effects() {\n    return check(5);\n}\n",
    )
    .unwrap();
    m.validate().unwrap();
    let f = m.func_id("check").unwrap() as usize;
    let (bi, len, index) = m.functions[f]
        .blocks
        .iter()
        .enumerate()
        .find_map(|(i, b)| match b.term {
            Term::TrapBounds { len, index } => Some((i, len, index)),
            _ => None,
        })
        .expect("the lowering emits a bounds trap");
    // Positive: the well-formed module stops with the spec message.
    let ok = ravum_lang_ir::interp::run(&m, "entry", 1_000_000).render();
    assert!(ok.contains("the len is 3 but the index is 5"), "{ok}");
    let wrong_type = m.functions[f]
        .locals
        .iter()
        .position(|t| *t != ravum_lang_ir::ir::Ty::U64)
        .expect("a non-u64 local") as u32;
    for (name, l, i) in [
        ("len out of range", 9_999, index),
        ("index out of range", len, 9_999),
        ("len not u64", wrong_type, index),
        ("index not u64", len, wrong_type),
    ] {
        let mut bad = m.clone();
        bad.functions[f].blocks[bi].term = Term::TrapBounds { len: l, index: i };
        assert!(bad.validate().is_err(), "{name}: validate accepted it");
        let run = ravum_lang_ir::interp::run(&bad, "entry", 1_000_000);
        assert!(
            run.render().contains("invalid IR"),
            "{name}: interpreter ran it: {}",
            run.render()
        );
        assert!(
            Module::from_json(&bad.to_json()).is_err(),
            "{name}: from_json accepted it"
        );
    }
}

#[test]
fn unsupported_constructs_are_refused_not_approximated() {
    for src in [
        // Generic enums, `Vec`s of enums and `&mut v[i]` are lowered since
        // 2026-11-23 (tests in ravum-lang-jit/tests/vec_aggregates.rs), and
        // `char` since IR v1.9 (ravum-lang-jit/tests/chars.rs); a string
        // literal pattern is not.
        "fn check() -> i32 effects(alloc) {\n    let s = String::from(\"a\");\n    match s.as_str() {\n        \"a\" => 1,\n        _ => 0,\n    }\n}\n",
        // IR v1.2 covers `wrapping_*`/`abs`; `round` (ties away from zero)
        // and `min` (NaN rule) still are not.
        "fn check() -> f64 effects() {\n    let x: f64 = 2.5;\n    return x.round();\n}\n",
        "fn check() -> f64 effects() {\n    let a: f64 = -5.0;\n    return a.min(1.0);\n}\n",
        // `&mut i32` parameters are lowered since 2026-11-23 (positive test
        // `references_to_scalars_agree_with_the_interpreter`); `i128` is not.
        "fn check() -> i32 effects() {\n    let x: i128 = 1;\n    return 0;\n}\n",
    ] {
        match compile_source(src) {
            Err(CompileError::Lower(LowerError::Unsupported(_))) => {}
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }
}

#[test]
fn corpus_limits_are_explicit_lowering_refusals() {
    for (fixture, reason) in [("o3-u128-max", "u128"), ("x-libm-refused", "sin")] {
        let corpus = machine_corpus();
        let (_, source) = corpus.iter().find(|(name, _)| name == fixture).unwrap();
        match compile_source(source) {
            Err(CompileError::Lower(LowerError::Unsupported(detail))) => {
                assert!(detail.contains(reason), "{fixture}: {detail}");
            }
            other => panic!("{fixture}: expected an explicit refusal, got {other:?}"),
        }
    }
}
