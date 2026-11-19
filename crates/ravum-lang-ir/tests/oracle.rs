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
                // A machine `diverge`/`fault` is inconclusive: the IR must not
                // turn it into a value.
                if expected.starts_with("diverge") || expected.starts_with("fault") {
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
                unsupported.push(format!("{name} ({what})"))
            }
            Err(CompileError::Lower(e)) => failures.push(format!("{name}: lowering bug: {e}")),
            Err(CompileError::Rejected(codes)) => rejected.push(format!("{name} {codes:?}")),
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

#[test]
fn unsupported_constructs_are_refused_not_approximated() {
    for src in [
        "enum E {\n    A,\n    B,\n}\nfn check() -> i32 effects() {\n    return 0;\n}\n",
        // IR v1.2 covers `wrapping_*`/`abs`; `round` (ties away from zero)
        // and `min` (NaN rule) still are not.
        "fn check() -> f64 effects() {\n    let x: f64 = 2.5;\n    return x.round();\n}\n",
        "fn check() -> f64 effects() {\n    let a: f64 = -5.0;\n    return a.min(1.0);\n}\n",
        "fn f(r: &mut i32) -> i32 effects() {\n    return 0;\n}\nfn check() -> i32 effects() {\n    return 0;\n}\n",
    ] {
        match compile_source(src) {
            Err(CompileError::Lower(LowerError::Unsupported(_))) => {}
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }
}
