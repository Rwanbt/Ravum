//! IR v1.10: exact `i128`/`u128` arithmetic, casts and fail-closed validation.
//! The interpreter, Cranelift JIT and independent reference machine must
//! render the same value or the same trap, including every 128-bit boundary.

use ravum_lang_ir::compile_source;
use ravum_lang_ir::interp::{run, run_dsp, DEFAULT_STEPS};
use ravum_lang_jit::{compile, DEFAULT_FUEL};

fn values(ty: &str) -> Vec<u128> {
    match ty {
        "i128" => vec![
            i128::MIN as u128,
            i128::MIN as u128 + 1,
            u128::MAX,
            u128::MAX - 1,
            (1u128 << 64) + 1,
            1,
            0,
            i128::MAX as u128 - 1,
            i128::MAX as u128,
        ],
        "u128" => vec![
            0,
            1,
            (1u128 << 64) - 1,
            1u128 << 64,
            (1u128 << 64) + 1,
            1u128 << 127,
            (1u128 << 127) + 1,
            u128::MAX - 1,
            u128::MAX,
        ],
        _ => unreachable!(),
    }
}

fn source_value(ty: &str, bits: u128) -> String {
    if ty == "i128" {
        (bits as i128).to_string()
    } else {
        bits.to_string()
    }
}

fn generated() -> Vec<String> {
    let mut out = Vec::new();
    for ty in ["i128", "u128"] {
        let vals = values(ty);
        for a in &vals {
            for b in &vals {
                let (a, b) = (source_value(ty, *a), source_value(ty, *b));
                for op in ["+", "-", "*", "/", "%", "&", "|", "^"] {
                    out.push(format!(
                        "fn check() -> {ty} effects() {{ let a: {ty} = {a}; let b: {ty} = {b}; return a {op} b; }}\n"
                    ));
                }
                for method in [
                    "wrapping_add",
                    "wrapping_sub",
                    "wrapping_mul",
                    "saturating_add",
                    "saturating_sub",
                    "saturating_mul",
                ] {
                    out.push(format!(
                        "fn check() -> {ty} effects() {{ let a: {ty} = {a}; let b: {ty} = {b}; return a.{method}(b); }}\n"
                    ));
                }
                for method in [
                    "checked_add",
                    "checked_sub",
                    "checked_mul",
                    "checked_div",
                    "checked_rem",
                ] {
                    out.push(format!(
                        "fn check() -> {ty} effects() {{ let a: {ty} = {a}; let b: {ty} = {b}; match a.{method}(b) {{ Some(v) => return v, None => return 0{ty} }} }}\n"
                    ));
                }
                for op in ["==", "!=", "<", "<=", ">", ">="] {
                    out.push(format!(
                        "fn check() -> bool effects() {{ let a: {ty} = {a}; let b: {ty} = {b}; return a {op} b; }}\n"
                    ));
                }
            }
            let a = source_value(ty, *a);
            for method in ["abs", "wrapping_neg", "saturating_neg", "checked_neg"] {
                if ty != "i128" {
                    continue;
                }
                if method == "checked_neg" {
                    out.push(format!(
                        "fn check() -> {ty} effects() {{ let a: {ty} = {a}; match a.{method}() {{ Some(v) => return v, None => return 0{ty} }} }}\n"
                    ));
                } else {
                    out.push(format!(
                        "fn check() -> {ty} effects() {{ let a: {ty} = {a}; return a.{method}(); }}\n"
                    ));
                }
            }
            out.push(format!(
                "fn check() -> {ty} effects() {{ let a: {ty} = {a}; return !a; }}\n"
            ));
            for amount in [0u32, 1, 63, 64, 65, 127, 128, 129] {
                for op in ["<<", ">>"] {
                    out.push(format!(
                        "fn check() -> {ty} effects() {{ let a: {ty} = {a}; let n: u32 = {amount}; return a {op} n; }}\n"
                    ));
                }
            }
        }
        let targets = [
            "i8", "u8", "i16", "u16", "i32", "u32", "i64", "u64", "i128", "u128", "f32", "f64",
        ];
        for a in &vals {
            let a = source_value(ty, *a);
            for to in targets {
                out.push(format!(
                    "fn check() -> {to} effects() {{ let a: {ty} = {a}; return a as {to}; }}\n"
                ));
            }
        }
    }
    let floats_f64 = [
        "0.0",
        "-0.0",
        "2.5",
        "-2.5",
        "9223372036854775808.0",
        "170141183460469231731687303715884105728.0",
        "-170141183460469231731687303715884105728.0",
        "170141183460469231731687303715884105728.0 * 2.0",
        "1.0e300",
        "-1.0e300",
        "0.0 / 0.0",
        "1.0 / 0.0",
        "-1.0 / 0.0",
    ];
    let floats_f32 = [
        "0.0",
        "-0.0",
        "2.5",
        "-2.5",
        "9223372036854775808.0",
        "170141183460469231731687303715884105728.0",
        "-170141183460469231731687303715884105728.0",
        "3.4028234663852886e38",
        "-3.4028234663852886e38",
        "0.0 / 0.0",
        "1.0 / 0.0",
        "-1.0 / 0.0",
    ];
    for (from, floats) in [
        ("f32", floats_f32.as_slice()),
        ("f64", floats_f64.as_slice()),
    ] {
        for x in floats {
            for to in ["i128", "u128"] {
                out.push(format!(
                    "fn check() -> {to} effects() {{ let x: {from} = {x}; return x as {to}; }}\n"
                ));
            }
        }
    }
    out
}

#[test]
fn wide_integers_agree_with_interpreter_jit_and_reference_machine() {
    let programs = generated();
    assert!(programs.len() >= 2_000, "generator lost power");
    let mut failures = Vec::new();
    let mut traps = 0;
    for src in programs {
        let ir = compile_source(&src).unwrap_or_else(|e| panic!("not lowered: {e:?}\n{src}"));
        let interpreted = run(&ir, "check", DEFAULT_STEPS).render();
        let native = compile(&ir)
            .unwrap_or_else(|e| panic!("JIT refused: {e:?}\n{src}"))
            .run("check", DEFAULT_FUEL)
            .render();
        let machine = ravum_core_machine::run(
            &src,
            "check",
            ravum_core_machine::SPEC_VERSION,
            ravum_core_machine::DEFAULT_BUDGET,
        )
        .render();
        traps += interpreted.starts_with("panic") as usize;
        if native != interpreted || native != machine {
            failures.push(format!(
                "JIT `{native}` / interpreter `{interpreted}` / machine `{machine}`\n{src}"
            ));
        }
    }
    eprintln!(
        "IR v1.10 wide differential: {} programs, {traps} traps",
        generated().len()
    );
    assert!(
        traps >= 100,
        "overflow/division traps not exercised: {traps}"
    );
    assert!(
        failures.is_empty(),
        "{} divergences:\n{}",
        failures.len(),
        failures
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n---\n")
    );
}

#[test]
fn wide_heap_words_and_mistyped_constants_are_refused() {
    for src in [
        "fn check() -> i32 effects(alloc) { let mut v: Vec<i128> = Vec::new(); v.push(1i128); return 0; }\n",
        "fn check() -> i32 effects(alloc) { let mut v: Vec<u128> = Vec::new(); v.push(1u128); return 0; }\n",
        "fn check() -> i32 effects(alloc) { let mut m: Map<i128, i64> = Map::new(); let _old = m.insert(1i128, 2i64); return 0; }\n",
        "fn check() -> i32 effects(alloc) { let mut m: Map<i64, u128> = Map::new(); let _old = m.insert(1i64, 2u128); return 0; }\n",
    ] {
        match compile_source(src) {
            Err(ravum_lang_ir::CompileError::Lower(ravum_lang_ir::lower::LowerError::Unsupported(
                detail,
            ))) => assert!(detail.contains("128-bit heap word"), "{detail}"),
            other => panic!("expected an explicit heap-word refusal for\n{src}\ngot {other:?}"),
        }
    }

    use ravum_lang_ir::ir::{Const, HeapOp, Inst, PrintArg, Stream, Ty};
    let mut ir = compile_source("fn check() -> i128 effects() { return 1i128; }\n").unwrap();
    ir.validate().unwrap();
    let original = ir.clone();
    let dst = match &ir.functions[0].blocks[0].insts[0] {
        Inst::Const { dst, .. } => *dst,
        _ => unreachable!(),
    };
    ir.functions[0].blocks[0].insts[0] = Inst::Const {
        dst,
        value: Const::Int {
            ty: Ty::I128,
            value: 1,
        },
    };
    assert!(
        ir.validate().is_err(),
        "non-canonical wide constant accepted"
    );
    assert!(ravum_lang_ir::Module::from_json(&ir.to_json()).is_err());

    let mut bad_heap = original.clone();
    bad_heap.functions[0].blocks[0].insts[0] = Inst::Heap {
        dst: Some(dst),
        op: HeapOp::VecLen,
        a: Some(dst),
        b: None,
    };
    assert!(bad_heap.validate().is_err(), "wide heap word accepted");
    assert!(ravum_lang_ir::Module::from_json(&bad_heap.to_json()).is_err());
    assert!(run(&bad_heap, "check", DEFAULT_STEPS)
        .render()
        .contains("invalid IR"));
    assert!(compile(&bad_heap).is_err(), "JIT accepted a wide heap word");

    assert!(ravum_lang_ir::interp::to_word(ravum_lang_ir::interp::Val::U128(1)).is_err());
    for ty in [Ty::I128, Ty::U128] {
        assert!(ravum_lang_ir::interp::from_word(ty, 1).is_err());
    }
    let unit = original.functions[0]
        .locals
        .iter()
        .position(|t| *t == Ty::Unit)
        .unwrap() as u32;
    let obj = original.functions[0].locals.len() as u32;
    let word = obj + 1;
    let flag = obj + 2;
    let cases = [
        (
            "VecSet handle",
            Inst::VecSet {
                h: dst,
                i: word,
                x: word,
            },
        ),
        (
            "VecPop handle",
            Inst::VecPop {
                present: flag,
                value: word,
                h: dst,
            },
        ),
        (
            "PrintObj",
            Inst::Print {
                stream: Stream::Stdout,
                arg: PrintArg::Obj(dst),
            },
        ),
        (
            "HeapText",
            Inst::HeapText {
                dst,
                text: "x".into(),
            },
        ),
        (
            "HeapOfValue",
            Inst::HeapOfValue {
                dst,
                src: dst,
                ty: Ty::I128,
            },
        ),
        (
            "Unit to wide cast",
            Inst::Cast {
                dst,
                from: Ty::Unit,
                to: Ty::I128,
                src: unit,
            },
        ),
        (
            "wide to Unit cast",
            Inst::Cast {
                dst: unit,
                from: Ty::I128,
                to: Ty::Unit,
                src: dst,
            },
        ),
    ];
    for (name, inst) in cases {
        let mut bad = original.clone();
        bad.functions[0].locals.extend([Ty::Obj, Ty::U64, Ty::Bool]);
        bad.functions[0].blocks[0].insts[0] = inst;
        assert!(bad.validate().is_err(), "{name}: validation accepted it");
        assert!(
            ravum_lang_ir::Module::from_json(&bad.to_json()).is_err(),
            "{name}: JSON accepted it"
        );
        assert!(
            run(&bad, "check", DEFAULT_STEPS)
                .render()
                .contains("invalid IR"),
            "{name}: interpreter ran it"
        );
        assert!(compile(&bad).is_err(), "{name}: JIT accepted it");
    }
}

#[test]
fn a_wide_dsp_state_is_refused_until_the_host_buffer_is_wide_aligned() {
    let ir = compile_source(
        "struct State { x: i128 }\nfn init() -> State effects() { return State { x: 1i128 }; }\nfn process_sample(state: &mut State, x: f32) -> f32 effects() { state.x = state.x.wrapping_add(1); return x; }\n",
    )
    .unwrap();
    match ravum_lang_jit::hot::HotEngine::start(&ir, "process_sample") {
        Err(ravum_lang_jit::hot::Refusal::Invalid(detail)) => {
            assert!(detail.contains("8-byte alignment"), "{detail}");
        }
        _ => panic!("expected an explicit state-alignment refusal"),
    }
    match run_dsp(&ir, Some("init"), "process_sample", &[], DEFAULT_STEPS) {
        Err(ravum_lang_ir::interp::RunOutcome::Fault(detail)) => {
            assert!(detail.contains("8-byte alignment"), "{detail}");
        }
        _ => panic!("interpreter accepted a wide state without aligned host storage"),
    }
}

#[test]
fn a_wide_nullary_function_hot_swaps_and_rejects_a_changed_signature() {
    let v1 = compile_source("fn value() -> i128 effects() { return 1i128; }\n").unwrap();
    let (mut engine, mut runner) = ravum_lang_jit::hot::HotEngine::start(&v1, "value").unwrap();
    assert_eq!(runner.run_nullary(&[]).render(), "value i128 1");

    let v2 = compile_source("fn value() -> i128 effects() { return 2i128; }\n").unwrap();
    let report = engine.update(&v2).unwrap();
    assert_eq!(report.recompiled, vec!["value".to_string()]);
    assert_eq!(runner.run_nullary(&[]).render(), "value i128 1");
    assert!(runner.poll());
    assert_eq!(runner.run_nullary(&[]).render(), "value i128 2");

    let v3 = compile_source("fn value() -> u128 effects() { return 3u128; }\n").unwrap();
    assert!(matches!(
        engine.update(&v3),
        Err(ravum_lang_jit::hot::Refusal::EntrySignature(_))
    ));
    assert!(!runner.poll());
    assert_eq!(runner.run_nullary(&[]).render(), "value i128 2");
}

#[test]
fn a_forged_alignment_update_with_the_same_digest_is_refused() {
    let ir = compile_source(
        "struct State { x: i64 }\nfn init() -> State effects() { return State { x: 1i64 }; }\nfn process_sample(state: &mut State, x: f32) -> f32 effects() { state.x = state.x + 1; return x; }\n",
    )
    .unwrap();
    let (mut engine, _runner) =
        ravum_lang_jit::hot::HotEngine::start(&ir, "process_sample").unwrap();
    let mut forged = ir.clone();
    forged.state.as_mut().unwrap().align = 16;
    match engine.update(&forged) {
        Err(ravum_lang_jit::hot::Refusal::Invalid(detail)) => {
            assert!(detail.contains("8-byte alignment"), "{detail}");
        }
        _ => panic!("forged state alignment bypassed the host-buffer check"),
    }
}
