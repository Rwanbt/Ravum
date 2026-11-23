//! IR v1.2 `io::print*`: native output = interpreter output, byte for byte,
//! for every scalar type and the hard float cases; a real-time entry that
//! prints is refused (INV-05).

use ravum_lang_ir::compile_source;
use ravum_lang_ir::interp::{run_with_output, DEFAULT_STEPS};
use ravum_lang_jit::hot::{HotEngine, Refusal};
use ravum_lang_jit::{compile, DEFAULT_FUEL};

/// One program printing `value` (an expression of type `ty`) both ways.
fn program(ty: &str, value: &str) -> String {
    format!(
        "use std::io;\nfn check() -> i64 effects(io, alloc) {{\n    let z: f64 = 0.0;\n    let v: {ty} = {value};\n    io::print(\"v=\");\n    io::println(&v.to_string());\n    io::eprintln(v.to_string().as_str());\n    return 1;\n}}\n"
    )
}

pub fn cases() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (ty, vals) in [
        ("i8", vec!["-128", "127", "0", "-1"]),
        ("u8", vec!["0", "255"]),
        ("i16", vec!["-32768", "32767"]),
        ("u16", vec!["65535"]),
        ("i32", vec!["-2147483648", "2147483647", "42"]),
        ("u32", vec!["4294967295"]),
        ("i64", vec!["-9223372036854775808", "9223372036854775807"]),
        ("u64", vec!["18446744073709551615", "0"]),
        ("isize", vec!["-9223372036854775808"]),
        ("usize", vec!["18446744073709551615"]),
        ("bool", vec!["true", "false", "1 < 2"]),
        (
            "f64",
            vec![
                "0.1",
                "1.0",
                "-0.0",
                "0.0",
                "1.5e300",
                "1e21",
                "1e-7",
                "123456789.125",
                "1.0 / z",
                "-1.0 / z",
                "z / z",
                "0.30000000000000004",
                "2.5",
            ],
        ),
        (
            "f32",
            vec!["0.1", "16777217.0", "-0.0", "3.4028235e38", "1e-7"],
        ),
    ] {
        for v in vals {
            out.push((format!("{ty} {v}"), program(ty, v)));
        }
    }
    out
}

#[test]
fn native_output_is_identical_to_the_interpreter() {
    let mut n = 0;
    for (name, src) in cases() {
        let ir = compile_source(&src).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let (ri, oi) = run_with_output(&ir, "check", DEFAULT_STEPS);
        let (rn, on) = compile(&ir).unwrap().run_with_output("check", DEFAULT_FUEL);
        assert_eq!(rn.render(), ri.render(), "{name}");
        assert_eq!(on, oi, "{name}");
        assert!(
            on.stdout.starts_with("v=") && on.stdout.ends_with('\n'),
            "{name}"
        );
        assert_eq!(on.stdout[2..], on.stderr, "{name}: stdout and stderr texts");
        n += 1;
    }
    assert!(n >= 40, "{n}");
}

#[test]
fn output_before_a_trap_is_kept_and_order_is_program_order() {
    let src = "use std::io;\nfn check() -> i64 effects(io, alloc) {\n    let mut i: i64 = 0;\n    while i < 3 {\n        io::println(&i.to_string());\n        i = i + 1;\n    }\n    let d: i64 = 0;\n    io::println(\"before\");\n    return 1 / d;\n}\n";
    let ir = compile_source(src).unwrap();
    let (ri, oi) = run_with_output(&ir, "check", DEFAULT_STEPS);
    let (rn, on) = compile(&ir).unwrap().run_with_output("check", DEFAULT_FUEL);
    assert_eq!(rn.render(), "panic: division by zero");
    assert_eq!(rn.render(), ri.render());
    assert_eq!(on.stdout, "0\n1\n2\nbefore\n");
    assert_eq!(on, oi);
}

#[test]
fn a_real_time_entry_that_prints_is_refused() {
    // Negative (INV-05): no I/O may run on the audio thread.
    let src =
        "use std::io;\nfn gain(x: f32) -> f32 effects(io, alloc) {\n    io::println(\"x\");\n    return x;\n}\n";
    let ir = compile_source(src).unwrap();
    match HotEngine::start(&ir, "gain") {
        Err(Refusal::Invalid(m)) => assert!(m.contains("INV-05"), "{m}"),
        Err(other) => panic!("expected Invalid, got {other:?}"),
        Ok(_) => panic!("expected a refusal"),
    }
}

#[test]
fn string_methods_outside_the_ir_are_unsupported() {
    // IR v1.3 prints any heap string; `to_uppercase` is still not lowered.
    let src = "use std::io;\nfn check() -> i64 effects(io, alloc) {\n    let n: i64 = 3;\n    let s = n.to_string();\n    io::println(&s.to_uppercase());\n    return 0;\n}\n";
    match compile_source(src) {
        Err(ravum_lang_ir::CompileError::Lower(ravum_lang_ir::lower::LowerError::Unsupported(
            _,
        ))) => {}
        other => panic!("expected Unsupported, got {other:?}"),
    }
}

#[test]
fn a_real_time_entry_that_allocates_is_refused() {
    // Negative (INV-05): no heap allocation on the audio thread.
    let src = "fn gain(x: f32) -> f32 effects(alloc) {\n    let v: Vec<f32> = Vec::new();\n    drop(v);\n    return x;\n}\n";
    let ir = compile_source(src).unwrap();
    match HotEngine::start(&ir, "gain") {
        Err(Refusal::Invalid(m)) => assert!(m.contains("heap allocation"), "{m}"),
        Err(other) => panic!("expected Invalid, got {other:?}"),
        Ok(_) => panic!("expected a refusal"),
    }
}

#[test]
fn the_heap_limit_is_a_fault_not_a_crash() {
    // The string doubles 30 times: past the 256 MiB limit long before.
    let src = "fn check() -> i64 effects(alloc) {\n    let mut s = String::from(\"xxxxxxxxxxxxxxxxxxxx\");\n    let mut k: i64 = 0;\n    while k < 30 {\n        let c = s.clone();\n        s.push_str(c.as_str());\n        k = k + 1;\n    }\n    return 1;\n}\n";
    let ir = compile_source(src).unwrap();
    let native = compile(&ir).unwrap().run("check", DEFAULT_FUEL).render();
    assert_eq!(native, "fault: heap limit");
    let (interp, _) = run_with_output(&ir, "check", DEFAULT_STEPS * 10);
    assert_eq!(interp.render(), "fault: heap limit");
}

#[test]
fn a_string_building_entry_is_hot_swapped() {
    let v1 = "use std::io;\nfn label() -> String effects(alloc) {\n    return String::from(\"v1\");\n}\n\nfn main() -> () effects(io, alloc) {\n    let mut s = label();\n    s.push_str(\"-ok\");\n    io::println(&s);\n}\n";
    let (mut engine, mut rt) = HotEngine::start(&compile_source(v1).unwrap(), "main").unwrap();
    let (r, out) = rt.run_nullary_with_output(engine.messages());
    assert_eq!(r.render(), "value () ()");
    assert_eq!(out.stdout, "v1-ok\n");
    let v2 = v1.replace("\"v1\"", "\"v2\"");
    let rep = engine.update(&compile_source(&v2).unwrap()).unwrap();
    assert_eq!(rep.recompiled, vec!["label".to_string()]);
    assert!(rt.poll());
    let (_, out) = rt.run_nullary_with_output(engine.messages());
    assert_eq!(out.stdout, "v2-ok\n");
}
