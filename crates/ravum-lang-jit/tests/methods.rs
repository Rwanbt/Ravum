//! IR v1.2 oracle: scalar builtin methods (`abs`, `wrapping_*`,
//! `saturating_*`, `f64::{abs, sqrt, floor, ceil}`) — native code vs the IR
//! interpreter vs the independent reference machine, at every boundary value
//! of every integer type.

use ravum_lang_ir::compile_source;
use ravum_lang_ir::interp::{run, DEFAULT_STEPS};
use ravum_lang_jit::{compile, DEFAULT_FUEL};

fn bounds(t: &str) -> (i128, i128) {
    match t {
        "i8" => (i8::MIN as i128, i8::MAX as i128),
        "i16" => (i16::MIN as i128, i16::MAX as i128),
        "i32" => (i32::MIN as i128, i32::MAX as i128),
        "i64" | "isize" => (i64::MIN as i128, i64::MAX as i128),
        "u8" => (0, u8::MAX as i128),
        "u16" => (0, u16::MAX as i128),
        "u32" => (0, u32::MAX as i128),
        _ => (0, u64::MAX as i128),
    }
}

fn values(t: &str) -> Vec<i128> {
    let (lo, hi) = bounds(t);
    let mut v = vec![
        lo,
        lo + 1,
        -3,
        -1,
        0,
        1,
        2,
        7,
        hi / 2,
        hi / 2 + 1,
        hi - 1,
        hi,
    ];
    if lo < 0 {
        v.push(lo / 2);
        v.push(lo / 2 - 1);
    }
    v.retain(|x| (lo..=hi).contains(x));
    v.sort();
    v.dedup();
    v
}

fn programs() -> Vec<String> {
    let mut out = Vec::new();
    let types = [
        "i8", "i16", "i32", "i64", "isize", "u8", "u16", "u32", "u64", "usize",
    ];
    let binary = [
        "wrapping_add",
        "wrapping_sub",
        "wrapping_mul",
        "saturating_add",
        "saturating_sub",
        "saturating_mul",
    ];
    for t in types {
        let signed = bounds(t).0 < 0;
        let vals = values(t);
        for a in &vals {
            if signed {
                for m in ["abs", "wrapping_neg", "saturating_neg"] {
                    out.push(format!(
                        "fn check() -> i64 effects() {{\n    let a: {t} = {a};\n    let r: {t} = a.{m}();\n    return r as i64;\n}}\n"
                    ));
                }
            }
            for b in &vals {
                for m in binary {
                    out.push(format!(
                        "fn check() -> i64 effects() {{\n    let a: {t} = {a};\n    let b: {t} = {b};\n    let r: {t} = a.{m}(b);\n    return r as i64;\n}}\n"
                    ));
                }
            }
        }
    }
    // f64: ordinary values, signed zeros, ties, infinities and NaN (built at
    // run time, so no constant folding is involved).
    let exprs = [
        "2.5",
        "-2.5",
        "0.0",
        "-0.0",
        "16.0",
        "2.0",
        "-1.5",
        "0.49999999999999994",
        "1.0 / z",
        "-1.0 / z",
        "z / z",
        "-7.25",
    ];
    for x in exprs {
        for m in ["abs", "sqrt", "floor", "ceil"] {
            out.push(format!(
                "fn check() -> f64 effects() {{\n    let z: f64 = 0.0;\n    let x: f64 = {x};\n    return x.{m}();\n}}\n"
            ));
        }
    }
    out
}

#[test]
fn scalar_methods_agree_with_interpreter_and_reference_machine() {
    let (mut compared, mut traps, mut machine_agreed) = (0, 0, 0);
    let mut failures = Vec::new();
    let mut inconclusive = Vec::new();
    for src in programs() {
        let ir = match compile_source(&src) {
            Ok(ir) => ir,
            Err(e) => {
                failures.push(format!("not lowered ({e:?}):\n{src}"));
                continue;
            }
        };
        let interp = run(&ir, "check", DEFAULT_STEPS).render();
        let native = compile(&ir).unwrap().run("check", DEFAULT_FUEL).render();
        compared += 1;
        traps += interp.starts_with("panic") as usize;
        if native != interp {
            failures.push(format!(
                "native `{native}` != interpreter `{interp}` for\n{src}"
            ));
        }
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
            inconclusive.push(m);
        }
    }
    eprintln!(
        "IR v1.2 methods: {compared} compared, {traps} traps, {machine_agreed} identical to the machine, {} inconclusive {:?}",
        inconclusive.len(),
        inconclusive.first()
    );
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures
            .iter()
            .take(10000)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n---\n")
    );
    // Power: every program reached the independent oracle, and the
    // `abs(MIN)` traps were exercised.
    assert!(compared >= 1_500, "generator lost power: {compared}");
    assert_eq!(
        inconclusive.len(),
        0,
        "machine could not judge some programs"
    );
    assert_eq!(machine_agreed, compared);
    assert!(traps >= 5, "abs(MIN) traps not exercised: {traps}");
}

#[test]
fn unsupported_methods_are_refused_not_approximated() {
    for (decl, call) in [
        (
            "let a: i32 = 3;",
            "match a.checked_add(1) { Some(v) => v as i64, None => 0 }",
        ),
        ("let a: f64 = 2.5;", "a.round() as i64"),
        ("let a: f64 = 2.5;", "a.min(1.0) as i64"),
        ("let a: f64 = 2.5;", "a.powf(2.0) as i64"),
    ] {
        let src = format!("fn check() -> i64 effects() {{\n    {decl}\n    return {call};\n}}\n");
        match compile_source(&src) {
            Err(ravum_lang_ir::CompileError::Lower(
                ravum_lang_ir::lower::LowerError::Unsupported(_),
            )) => {}
            other => panic!("expected Unsupported for `{call}`, got {other:?}"),
        }
    }
}

#[test]
fn validate_refuses_a_method_operation_on_the_wrong_type() {
    // A forged IR (not produced by the lowering) must not reach a backend.
    let src = "fn check() -> f64 effects() {\n    let x: f64 = 2.0;\n    return x.sqrt();\n}\n";
    let ir = compile_source(src).unwrap();
    let json = ir.to_json().replace("\"F64\"", "\"I64\"");
    let forged = ravum_lang_ir::ir::Module::from_json(&json);
    assert!(
        forged.is_err() || forged.unwrap().validate().is_err(),
        "Sqrt on an integer accepted"
    );
}
