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
        "fn check() -> i64 effects(io, alloc) {{\n    let z: f64 = 0.0;\n    let v: {ty} = {value};\n    io::print(\"v=\");\n    io::println(&v.to_string());\n    io::eprintln(v.to_string().as_str());\n    return 1;\n}}\n"
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
    let src = "fn check() -> i64 effects(io, alloc) {\n    let mut i: i64 = 0;\n    while i < 3 {\n        io::println(&i.to_string());\n        i = i + 1;\n    }\n    let d: i64 = 0;\n    io::println(\"before\");\n    return 1 / d;\n}\n";
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
        "fn gain(x: f32) -> f32 effects(io, alloc) {\n    io::println(\"x\");\n    return x;\n}\n";
    let ir = compile_source(src).unwrap();
    match HotEngine::start(&ir, "gain") {
        Err(Refusal::Invalid(m)) => assert!(m.contains("INV-05"), "{m}"),
        Err(other) => panic!("expected Invalid, got {other:?}"),
        Ok(_) => panic!("expected a refusal"),
    }
}

#[test]
fn strings_other_than_literals_and_scalar_text_are_unsupported() {
    let src = "fn check() -> i64 effects(io, alloc) {\n    let n: i64 = 3;\n    let s = n.to_string();\n    io::println(&s);\n    return 0;\n}\n";
    match compile_source(src) {
        Err(ravum_lang_ir::CompileError::Lower(ravum_lang_ir::lower::LowerError::Unsupported(
            _,
        ))) => {}
        other => panic!("expected Unsupported, got {other:?}"),
    }
}
