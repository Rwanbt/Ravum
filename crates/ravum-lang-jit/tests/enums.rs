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

pub const ACC: &str = "struct Acc {\n    total: i64,\n    n: i32,\n}\n\nimpl Acc {\n    fn new(start: i64) -> Acc effects() {\n        return Acc { total: start, n: 0 };\n    }\n    fn add(&mut self, x: i64) -> () effects() {\n        self.total = self.total + x;\n        self.n = self.n + 1;\n    }\n    fn mean(&self) -> i64 effects() {\n        return self.total / (self.n as i64);\n    }\n    fn doubled(self) -> Acc effects() {\n        let mut c = self;\n        c.total = c.total * 2;\n        return c;\n    }\n    fn bump_all(&mut self, k: i64) -> () effects() {\n        self.add(k);\n        self.add(k);\n    }\n}\n\nstruct Pair {\n    a: Acc,\n    b: Acc,\n}\n\nimpl Pair {\n    fn new() -> Pair effects() {\n        return Pair { a: Acc::new(0), b: Acc::new(100) };\n    }\n    fn mean(&self) -> i64 effects() {\n        return self.a.mean() + self.b.mean();\n    }\n}\n\n";

#[test]
fn user_methods_agree_with_the_reference_machine() {
    let mut programs = Vec::new();
    for x in [0i64, 1, 5, -7, 9_223_372_036_854_775_000] {
        for y in [1i64, 3, -2] {
            programs.push(format!(
                "{ACC}fn check() -> i64 effects() {{\n    let mut a = Acc::new({x});\n    a.add({y});\n    a.add(9);\n    let b = a.doubled();\n    a.bump_all({y});\n    let mut p = Pair::new();\n    p.a.add({y});\n    p.b.add(4);\n    let k: i64 = 1000;\n    return a.mean() * k + b.total + p.mean();\n}}\n"
            ));
        }
    }
    // A method whose receiver has no element yet: division by zero inside it.
    programs.push(format!(
        "{ACC}fn check() -> i64 effects() {{\n    let a = Acc::new(3);\n    return a.mean();\n}}\n"
    ));
    // The reference machine has no `impl`: native = interpreter here; the
    // independent oracle for methods is the rustc build
    // (`ravum-lang-cli/tests/jit_print_rustc.rs`).
    let (n, traps) = judge_with(&programs, false);
    eprintln!("IR v1.2 user methods: {n} programs, {traps} traps, native = interpreter");
    assert_eq!(n, 16);
    assert!(traps >= 2, "traps: {traps}");
}

#[test]
fn consts_agree_with_the_reference_machine() {
    let consts = "const LIMIT: i64 = 9223372036854775807;\nconst LOW: i8 = -128;\nconst STEP: u8 = 255;\nconst ON: bool = true;\nconst K: i32 = -5;\n\n";
    let mut programs = Vec::new();
    for x in [-5i32, 0, 7] {
        programs.push(format!(
            "{consts}fn check() -> i64 effects() {{\n    let x: i32 = {x};\n    let mut r: i64 = match x {{\n        K => 1,\n        0 => 2,\n        _ => 3,\n    }};\n    if ON {{\n        r = r + (LOW as i64) + (STEP as i64);\n    }}\n    return r + (LIMIT - LIMIT) + (K as i64);\n}}\n"
        ));
    }
    // Overflow through a constant at the boundary.
    programs.push(format!(
        "{consts}fn check() -> i64 effects() {{\n    let one: i64 = 1;\n    return LIMIT + one;\n}}\n"
    ));
    // `K =>` is a binding (G-6.38/G-6.41), so every `x` takes the first arm:
    // r = 1, then 1 - 128 + 255 = 128, minus K (-5): 123 for all three.
    let (n, traps) = judge(&programs);
    assert_eq!((n, traps), (4, 1));
    for p in &programs[..3] {
        let ir = compile_source(p).unwrap();
        assert_eq!(run(&ir, "check", DEFAULT_STEPS).render(), "value i64 123");
    }

    // A path is a constant pattern (EXPR-5.8); the reference machine has
    // none, so the expected values are written by hand.
    for (x, want) in [(-5, 1), (0, 2), (7, 3)] {
        let src = format!(
            "{consts}fn check() -> i64 effects() {{\n    let x: i32 = {x};\n    return match x {{\n        crate::K => 1,\n        0 => 2,\n        _ => 3,\n    }};\n}}\n"
        );
        let (n, _) = judge_with(std::slice::from_ref(&src), false);
        assert_eq!(n, 1);
        let ir = compile_source(&src).unwrap();
        assert_eq!(
            run(&ir, "check", DEFAULT_STEPS).render(),
            format!("value i64 {want}")
        );
    }
}

const PARSE: &str = "fn digit(c: i64) -> Result<i64, i32> effects() {\n    if c >= 0 && c <= 9 {\n        return Ok(c);\n    }\n    let code: i32 = 7;\n    return Err(code);\n}\n\nfn two(a: i64, b: i64) -> Result<i64, i32> effects() {\n    let x = digit(a)?;\n    let y = digit(b)?;\n    let ten: i64 = 10;\n    return Ok(x * ten + y);\n}\n\nfn first(o: Option<i64>) -> Option<i64> effects() {\n    let v = o?;\n    let one: i64 = 1;\n    return Some(v + one);\n}\n\n";

#[test]
fn result_and_try_agree_with_the_reference_machine() {
    let mut programs = Vec::new();
    for (a, b) in [(1, 2), (9, 0), (12, 3), (4, -1), (0, 0)] {
        programs.push(format!(
            "{PARSE}fn check() -> i64 effects() {{\n    let miss: i64 = -1;\n    let r = two({a}, {b});\n    let mut out: i64 = match r {{\n        Ok(v) => v,\n        Err(e) => (e as i64) * miss,\n    }};\n    if r.is_ok() {{\n        out = out + 1000;\n    }}\n    if r.is_err() {{\n        out = out - 1000;\n    }}\n    let d: i64 = 5;\n    return out + r.unwrap_or(d);\n}}\n"
        ));
    }
    for v in ["Some(41)", "None"] {
        programs.push(format!(
            "{PARSE}fn check() -> i64 effects() {{\n    let z: i64 = 0;\n    let o: Option<i64> = {v};\n    return first(o).unwrap_or(z);\n}}\n"
        ));
    }
    // The reference machine has no `?`: native = interpreter, and each
    // result equals the value computed by hand.
    let (n, traps) = judge_with(&programs, false);
    assert_eq!((n, traps), (7, 0));
    let want = [1024, 1180, -1002, -1002, 1000, 42, 0];
    for (p, w) in programs.iter().zip(want) {
        let ir = compile_source(p).unwrap();
        assert_eq!(
            run(&ir, "check", DEFAULT_STEPS).render(),
            format!("value i64 {w}")
        );
    }
}

#[test]
fn generic_functions_agree_with_the_reference_machine() {
    let generic = "fn pick<T>(c: bool, a: T, b: T) -> T effects() {\n    if c {\n        return a;\n    }\n    return b;\n}\n\nfn swap<A, B>(p: (A, B)) -> (B, A) effects() {\n    return (p.1, p.0);\n}\n\nfn wrap<T>(x: T) -> Option<T> effects() {\n    return Some(x);\n}\n\nfn depth<T>(x: T, n: i64) -> T effects() {\n    if n <= 0 {\n        return x;\n    }\n    let one: i64 = 1;\n    return depth(x, n - one);\n}\n\n";
    let mut programs = Vec::new();
    for c in ["true", "false"] {
        programs.push(format!(
            "{generic}fn check() -> i64 effects() {{\n    let a: i64 = 3;\n    let b: i64 = 4;\n    let x = pick({c}, a, b);\n    let f: f64 = pick({c}, 1.5, 2.5);\n    let s = swap((x, f > 2.0));\n    let w = wrap(s);\n    let n: i64 = 5;\n    let seven: u8 = 7;\n    let d: u8 = depth(seven, n);\n    let z: i64 = 0;\n    let k: i64 = 100;\n    let r = match w {{\n        Some(t) => if t.0 {{ k }} else {{ z }},\n        None => z,\n    }};\n    return x + r + (d as i64) + (f as i64);\n}}\n"
        ));
    }
    // The reference machine has no generics: native = interpreter, and the
    // values computed by hand.
    let (n, _) = judge_with(&programs, false);
    assert_eq!(n, 2);
    for (p, w) in programs.iter().zip([11, 113]) {
        let ir = compile_source(p).unwrap();
        assert_eq!(
            run(&ir, "check", DEFAULT_STEPS).render(),
            format!("value i64 {w}")
        );
    }
    // Each distinct set of type arguments is one instance.
    let ir = compile_source(&programs[0]).unwrap();
    let names: Vec<&str> = ir.functions.iter().map(|f| f.name.as_str()).collect();
    assert!(
        names.contains(&"pick<i64>") && names.contains(&"pick<f64>"),
        "{names:?}"
    );
    assert!(
        names.iter().all(|n| *n != "pick"),
        "no uninstantiated generic: {names:?}"
    );
}
