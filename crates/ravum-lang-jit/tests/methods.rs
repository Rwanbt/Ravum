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
        // `checked_*` is covered since enums/`match` (IR v1.2); `max` is not.
        ("let a: f64 = 2.5;", "a.max(1.0) as i64"),
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

/// Hard `f64` inputs for `math::*`: ties (STD-6.3), the 2^52 boundary where
/// every value is an integer, signed zeros, infinities and NaN (built at run
/// time from `z = 0.0`).
const MATH_VALUES: &[&str] = &[
    "0.5",
    "-0.5",
    "1.5",
    "-1.5",
    "2.5",
    "-2.5",
    "0.49999999999999994",
    "-0.49999999999999994",
    "4503599627370495.5",
    "-4503599627370495.5",
    "4503599627370497.0",
    "1.0e300",
    "0.0",
    "-0.0",
    "-7.25",
    "2.0",
    "1.0 / z",
    "-1.0 / z",
    "z / z",
];

#[test]
fn math_functions_agree_with_interpreter_and_reference_machine() {
    let mut n = 0;
    let mut failures = Vec::new();
    let mut check = |src: String| {
        let ir = compile_source(&src).unwrap_or_else(|e| panic!("{e:?}\n{src}"));
        let interp = run(&ir, "check", DEFAULT_STEPS).render();
        let native = compile(&ir).unwrap().run("check", DEFAULT_FUEL).render();
        let m = ravum_core_machine::run(
            &src,
            "check",
            ravum_core_machine::SPEC_VERSION,
            ravum_core_machine::DEFAULT_BUDGET,
        )
        .render();
        // Bits matter: -0 vs +0 is the point of STD-6.5; the render shows
        // the sign of zero, and `1.0 / r` separates them further below.
        if native != interp || native != m {
            failures.push(format!(
                "native `{native}` interp `{interp}` machine `{m}`\n{src}"
            ));
        }
        n += 1;
    };
    for x in MATH_VALUES {
        for f in ["abs", "sqrt", "floor", "ceil", "round"] {
            check(format!(
                "fn check() -> f64 effects() {{\n    let z: f64 = 0.0;\n    let x: f64 = {x};\n    let r = math::{f}(x);\n    return r;\n}}\n"
            ));
        }
        for y in MATH_VALUES {
            for f in ["min", "max"] {
                check(format!(
                    "fn check() -> f64 effects() {{\n    let z: f64 = 0.0;\n    let r = math::{f}({x}, {y});\n    let one: f64 = 1.0;\n    return one / r;\n}}\n"
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n---\n")
    );
    eprintln!("IR v1.2 math::*: {n} programs, native = interpreter = machine");
    assert!(n >= 800, "{n}");
}

#[test]
fn validate_refuses_integer_only_and_float_only_ops_on_the_wrong_type() {
    use ravum_lang_ir::ir::{BinOp, Inst};
    // Forged IR: `OverflowsAdd` on f64, `MathMin` on i64 — never produced by
    // the lowering, must never reach a backend.
    let src = "fn check() -> i64 effects() {\n    let a: i64 = 1;\n    let b: i64 = 2;\n    return a + b;\n}\n";
    let ir = compile_source(src).unwrap();
    for (op, ty) in [
        (BinOp::OverflowsAdd, ravum_lang_ir::ir::Ty::F64),
        (BinOp::MathMin, ravum_lang_ir::ir::Ty::I64),
        (BinOp::WrappingMul, ravum_lang_ir::ir::Ty::F64),
    ] {
        let mut m = ir.clone();
        m.functions[0].blocks[0].insts.push(Inst::Bin {
            dst: 0,
            op,
            ty,
            lhs: 0,
            rhs: 0,
        });
        assert!(m.validate().is_err(), "{op:?} on {ty:?} accepted");
    }
}
