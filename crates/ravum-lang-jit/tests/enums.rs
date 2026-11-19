//! IR v1.2 oracle: `Copy` enums, `Option<T>`, `match` and `int::checked_*` —
//! native code vs the IR interpreter vs the independent reference machine.

use ravum_lang_ir::compile_source;
use ravum_lang_ir::interp::{run, DEFAULT_STEPS};
use ravum_lang_jit::{compile, DEFAULT_FUEL};

/// Runs every program on the three engines (two when `machine` is false: the
/// reference machine has no struct variants); returns (compared, traps).
fn judge(programs: &[String]) -> (usize, usize) {
    judge_with(programs, true)
}

fn judge_with(programs: &[String], with_machine: bool) -> (usize, usize) {
    let (mut compared, mut traps) = (0, 0);
    let mut failures = Vec::new();
    for src in programs {
        let ir = match compile_source(src) {
            Ok(ir) => ir,
            Err(e) => {
                failures.push(format!("not lowered ({e:?}):\n{src}"));
                continue;
            }
        };
        let interp = run(&ir, "check", DEFAULT_STEPS).render();
        let native = compile(&ir).unwrap().run("check", DEFAULT_FUEL).render();
        let machine = if with_machine {
            ravum_core_machine::run(
                src,
                "check",
                ravum_core_machine::SPEC_VERSION,
                ravum_core_machine::DEFAULT_BUDGET,
            )
            .render()
        } else {
            interp.clone()
        };
        compared += 1;
        traps += interp.starts_with("panic") as usize;
        if native != interp || native != machine {
            failures.push(format!(
                "native `{native}`, interpreter `{interp}`, machine `{machine}` for\n{src}"
            ));
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
    (compared, traps)
}

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

#[test]
fn checked_methods_agree_on_every_boundary() {
    let mut programs = Vec::new();
    let types = [
        "i8", "i16", "i32", "i64", "isize", "u8", "u16", "u32", "u64", "usize",
    ];
    for t in types {
        let (lo, hi) = bounds(t);
        let mut vals = vec![lo, lo + 1, -1, 0, 1, 2, 3, hi / 2 + 1, hi - 1, hi];
        vals.retain(|v| (lo..=hi).contains(v));
        vals.sort();
        vals.dedup();
        for a in &vals {
            if lo < 0 {
                programs.push(format!(
                    "fn check() -> i64 effects() {{\n    let miss: i64 = -77;\n    let a: {t} = {a};\n    return match a.checked_neg() {{\n        Some(v) => v as i64,\n        None => miss,\n    }};\n}}\n"
                ));
            }
            for b in &vals {
                for m in ["add", "sub", "mul", "div", "rem"] {
                    programs.push(format!(
                        "fn check() -> i64 effects() {{\n    let miss: i64 = -77;\n    let a: {t} = {a};\n    let b: {t} = {b};\n    let r: Option<{t}> = a.checked_{m}(b);\n    return match r {{\n        Some(v) => v as i64,\n        None => miss,\n    }};\n}}\n"
                    ));
                }
            }
        }
    }
    let (n, traps) = judge(&programs);
    eprintln!("IR v1.2 checked_*: {n} programs, native = interpreter = machine");
    assert!(n >= 3_700, "generator lost power: {n}");
    assert_eq!(traps, 0, "checked_* must never trap");
}

fn shapes(named: bool) -> String {
    let src = "enum Shape {\n    Empty,\n    Dot(i8),\n    Circle(f64),\n    Rect { w: i32, h: i32 },\n    Pair(u8, i64),\n}\n\n\
struct Tagged {\n    s: Shape,\n    k: i16,\n}\n\n\
fn make(n: i32) -> Shape effects() {\n    if n % 5 == 0 {\n        return Shape::Empty;\n    }\n    if n % 5 == 1 {\n        return Shape::Dot((n % 100) as i8);\n    }\n    if n % 5 == 2 {\n        return Shape::Circle((n as f64) * 0.5);\n    }\n    if n % 5 == 3 {\n        return Shape::Rect { h: n, w: n * 2 };\n    }\n    return Shape::Pair(n as u8, (n as i64) * 1000);\n}\n\n\
fn area(s: Shape) -> i64 effects() {\n    return match s {\n        Shape::Empty => 0,\n        Shape::Dot(1) => 100,\n        Shape::Dot(d) => d as i64,\n        Shape::Circle(r) => (r * r) as i64,\n        Shape::Rect { w, h: 7 } => (w as i64) + 7000,\n        Shape::Rect { w, h } => (w as i64) * (h as i64),\n        Shape::Pair(0, x) => x,\n        Shape::Pair(a, x) => (a as i64) + x,\n    };\n}\n\n\
fn first_dot(a: Option<Shape>, b: Option<Shape>) -> Option<i8> effects() {\n    return match (a, b) {\n        (Some(Shape::Dot(x)), _) => Some(x),\n        (_, Some(Shape::Dot(y))) => Some(y),\n        _ => None,\n    };\n}\n\n";
    if named {
        src.to_string()
    } else {
        // Tuple form of `Rect`, for the reference machine.
        src.replace("Rect { w: i32, h: i32 }", "Rect(i32, i32)")
            .replace("Shape::Rect { h: n, w: n * 2 }", "Shape::Rect(n * 2, n)")
            .replace("Shape::Rect { w, h: 7 }", "Shape::Rect(w, 7)")
            .replace("Shape::Rect { w, h }", "Shape::Rect(w, h)")
            .replace(
                "Shape::Rect { w: 2147483647, h: 2 }",
                "Shape::Rect(2147483647, 2)",
            )
    }
}

fn shape_program(named: bool, x: i32, y: i32, z: i32) -> String {
    let shapes = shapes(named);
    format!(
        "{shapes}fn check() -> i64 effects() {{\n    let mut t = Tagged {{ s: make({x}), k: {z} }};\n    let copy = t.s;\n    t.s = make({y});\n    let mut acc: i64 = area(copy) * 3 + area(t.s);\n    let o = first_dot(Some(make({x})), Some(t.s));\n    acc = acc + match o {{\n        Some(d) => (d as i64) * 11,\n        None => -1,\n    }};\n    let arr: [Shape; 3] = [make({z}), Shape::Empty, make({x} + {y})];\n    let two: usize = 3;\n    for i in 0..two {{\n        acc = acc * 2 + area(arr[i]);\n    }}\n    let flag: bool = acc > 0;\n    acc = acc + match flag {{\n        true => 1,\n        false => 2,\n    }};\n    return acc + match t.k {{\n        0 => 5,\n        -1 => 6,\n        _ => (t.k as i64),\n    }};\n}}\n"
    )
}

#[test]
fn enums_and_match_agree_with_the_reference_machine() {
    for named in [false, true] {
        enums_and_match(named);
    }
}

fn enums_and_match(named: bool) {
    let shapes = shapes(named);
    let mut programs = Vec::new();
    for x in [
        0, 1, 2, 3, 4, 6, 7, 11, 13, 21, 26, 99, 255, 256, 1000, -3, -8,
    ] {
        for y in [0, 1, 2, 3, 4, 5, 8, 12, 17] {
            for z in [0, -1, 3, 7] {
                programs.push(shape_program(named, x, y, z));
            }
        }
    }
    // A trap while building a payload (`n * 2` overflows in `make`), and in
    // a payload expression.
    programs.push(format!(
        "{shapes}fn check() -> i64 effects() {{\n    return area(make(2147483643));\n}}\n"
    ));
    programs.push(format!(
        "{shapes}fn check() -> i64 effects() {{\n    let z: i8 = 0;\n    let s = Shape::Dot(7 / z);\n    return area(s);\n}}\n"
    ));
    let (n, traps) = judge_with(&programs, !named);
    eprintln!(
        "IR v1.2 enums/match (struct variants: {named}): {n} programs, {traps} traps, native = interpreter{}",
        if named { "" } else { " = machine" }
    );
    assert!(n >= 600 && traps >= 2, "generator lost power: {n}/{traps}");
}

#[test]
fn a_match_binding_is_a_copy_not_an_alias() {
    let shapes = shapes(false);
    let src = format!(
        "{shapes}fn check() -> i64 effects() {{\n    let mut a: Option<Tagged> = Some(Tagged {{ s: Shape::Dot(5), k: 1 }});\n    let r = match a {{\n        Some(t) => t,\n        None => Tagged {{ s: Shape::Empty, k: 0 }},\n    }};\n    a = None;\n    let mut x: i64 = area(r.s);\n    x = x + match a {{\n        Some(_) => 1000,\n        None => 1,\n    }};\n    return x;\n}}\n"
    );
    let (n, _) = judge(&[src]);
    assert_eq!(n, 1);
}

#[test]
fn option_methods_agree_with_the_reference_machine() {
    let mut programs = Vec::new();
    for v in ["Some(7)", "None", "Some(-9223372036854775808)", "Some(0)"] {
        for d in ["3", "-1"] {
            programs.push(format!(
                "fn check() -> i64 effects() {{\n    let a: Option<i64> = {v};\n    let d: i64 = {d};\n    let k: i64 = 1000;\n    let mut r: i64 = a.unwrap_or(d);\n    if a.is_some() {{\n        r = r + k;\n    }}\n    if a.is_none() {{\n        r = r - k;\n    }}\n    return r;\n}}\n"
            ));
        }
    }
    // Aggregate payload: `unwrap_or` copies the tuple.
    programs.push("fn check() -> i64 effects() {\n    let a: Option<(i32, i64)> = Some((4, 5));\n    let b: Option<(i32, i64)> = None;\n    let d0: (i32, i64) = (0, 0);\n    let d1: (i32, i64) = (1, 2);\n    let x = a.unwrap_or(d0);\n    let y = b.unwrap_or(d1);\n    let h: i64 = 100;\n    let t: i64 = 10;\n    return (x.0 as i64) * h + x.1 * t + (y.0 as i64) + y.1;\n}\n".to_string());
    let (n, _) = judge(&programs);
    assert_eq!(n, 9);
}
