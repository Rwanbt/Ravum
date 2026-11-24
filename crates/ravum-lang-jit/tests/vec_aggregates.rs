//! T14: `Vec` of structs and tuples (one handle per element, to a word
//! vector of its scalar leaves) agrees between the interpreter and the JIT.

use ravum_lang_ir::compile_source;
use ravum_lang_ir::interp::{run_with_output, DEFAULT_STEPS};
use ravum_lang_jit::{compile, DEFAULT_FUEL};

fn agree(source: &str) -> (String, String) {
    let ir = compile_source(source).expect("vec of aggregates lowers");
    let (interpreted, interpreted_output) = run_with_output(&ir, "main", DEFAULT_STEPS);
    let (native, native_output) = compile(&ir)
        .expect("vec of aggregates compiles")
        .run_with_output("main", DEFAULT_FUEL);
    assert_eq!(native.render(), interpreted.render());
    assert_eq!(native_output, interpreted_output);
    (native.render(), native_output.stdout)
}

const PROGRAM: &str = r#"use std::io;

struct Point { x: i32, y: i64 }
struct Named { name: String, at: Point, w: (u8, f64) }

fn sum(v: &Vec<Point>) -> i64 effects() {
    let mut s = 0i64;
    let mut i = 0usize;
    while i < v.len() {
        s = s + (v[i].x as i64) + v[i].y;
        i = i + 1usize;
    }
    return s;
}

fn main() -> () effects(io, alloc) {
    let mut v: Vec<Point> = Vec::new();
    v.push(Point { x: -3, y: 10 });
    v.push(Point { x: 7, y: 100 });
    v[0].x = v[0].x * 2;
    v[1] = Point { x: 1, y: 2 };
    let p = v[0];
    io::println(&p.x.to_string());
    io::println(&sum(&v).to_string());
    io::println(&v.len().to_string());
    let mut n: Vec<Named> = Vec::new();
    n.push(Named { name: String::from("ra"), at: Point { x: 4, y: 5 }, w: (9u8, 0.5f64) });
    n[0].name.push_str("vum");
    n[0].at.y = n[0].at.y + 1;
    n[0].w.1 = n[0].w.1 * 3.0f64;
    io::println(&n[0].name);
    io::println(&n[0].at.y.to_string());
    io::println(&n[0].w.1.to_string());
    io::println(&n[0].w.0.to_string());
    match v.pop() {
        Some(q) => io::println(&q.y.to_string()),
        None => io::println("none"),
    }
    let d = v.pop();
    match v.pop() {
        Some(_) => io::println("some"),
        None => io::println("empty"),
    }
}
"#;

#[test]
fn vec_of_structs_and_tuples_agrees_with_the_interpreter() {
    let (_, out) = agree(PROGRAM);
    assert_eq!(out, "-6\n7\n2\nravum\n6\n1.5\n9\n2\nempty\n");
}

#[test]
fn an_out_of_bounds_aggregate_element_panics_with_the_element_count() {
    let source = r#"use std::io;

struct P { a: i64, b: i64 }

fn main() -> () effects(io, alloc) {
    let mut v: Vec<P> = Vec::new();
    v.push(P { a: 1, b: 2 });
    io::println(&v[1].b.to_string());
}
"#;
    let (render, out) = agree(source);
    assert_eq!(out, "");
    assert!(
        render.contains("the len is 1 but the index is 1"),
        "{render}"
    );
}

#[test]
fn unsupported_vec_aggregate_forms_are_refused() {
    for (body, why) in [
        (
            "let mut v: Vec<B> = Vec::new(); v.push(B { z: [P { a: 1, b: 2 }] }); v[0].z[0].a = 5;",
            "an array of aggregates inside a `Vec` element",
        ),
        (
            "let mut v: Vec<C> = Vec::new(); v.push(C { z: [(1, 2)] }); let x: i64 = v[0].z[0].1; drop(x);",
            "an array of aggregates inside a `Vec` element",
        ),
    ] {
        let source = format!(
            "struct P {{ a: i64, b: i64 }}\nstruct B {{ z: [P; 1] }}\nstruct C {{ z: [(i64, i64); 1] }}\nfn bump(p: &mut P) -> () effects() {{ p.a = 3; }}\nfn main() -> () effects(alloc) {{ {body} }}\n"
        );
        let err = format!("{:?}", compile_source(&source).expect_err(body));
        assert!(err.contains(why), "{body}: {err}");
    }
}

const FOR_VEC: &str = r#"use std::io;

struct Point { x: i32, y: i64 }

fn main() -> () effects(io, alloc) {
    let mut v: Vec<i64> = Vec::new();
    for i in 0..6 {
        v.push((i * 10) as i64);
    }
    let mut sum = 0i64;
    for x in v {
        if x == 20 {
            continue;
        }
        if x == 50 {
            break;
        }
        sum = sum + x;
    }
    io::println(&sum.to_string());
    let mut words: Vec<String> = Vec::new();
    words.push(String::from("ra"));
    words.push(String::from("vum"));
    let mut all = String::new();
    for w in words {
        all.push_str(&w);
    }
    io::println(&all);
    let mut ps: Vec<Point> = Vec::new();
    ps.push(Point { x: 1, y: 2 });
    ps.push(Point { x: -3, y: 40 });
    for p in ps {
        io::println(&((p.x as i64) + p.y).to_string());
    }
    let empty: Vec<i64> = Vec::new();
    for x in empty {
        io::println("never");
    }
    io::println("end");
}
"#;

#[test]
fn for_over_an_owned_vec_agrees_with_the_interpreter() {
    let (_, out) = agree(FOR_VEC);
    assert_eq!(out, "80\nravum\n3\n37\nend\n");
}

#[test]
fn unwrap_agrees_and_panics_with_the_specified_text() {
    let ok = "use std::io;\nfn main() -> () effects(io, alloc, panic) {\n    let a: Option<i64> = Some(4);\n    let r: Result<i64, Error> = Ok(5);\n    io::println(&(a.unwrap() + r.unwrap()).to_string());\n}\n";
    assert_eq!(agree(ok).1, "9\n");
    for (body, text) in [
        ("let a: Option<i64> = None;\n    let b: i64 = a.unwrap();\n    drop(b);", "called Option::unwrap on None"),
        ("let a: Result<i64, Error> = Err(Error::new(\"boom\"));\n    let b: i64 = a.unwrap();\n    drop(b);", "called Result::unwrap on an Err value"),
    ] {
        let src = format!("fn main() -> () effects(alloc, panic) {{\n    {body}\n}}\n");
        let (render, out) = agree(&src);
        assert_eq!(out, "");
        assert!(render.contains(text), "{render}");
        assert!(!render.contains("boom"), "the Err content must not be formatted: {render}");
    }
}

#[test]
fn results_with_the_builtin_error_agree() {
    let src = r#"use std::io;

fn half(n: i64) -> Result<i64, Error> effects(alloc) {
    if n % 2 != 0 {
        return Err(Error::new("odd"));
    }
    return Ok(n / 2);
}

fn quarter(n: i64) -> Result<i64, Error> effects(alloc) {
    let h = half(n)?;
    return half(h);
}

fn main() -> () effects(io, alloc) {
    match quarter(12) {
        Ok(v) => io::println(&v.to_string()),
        Err(e) => io::println(&e.message),
    }
    match quarter(6) {
        Ok(v) => io::println(&v.to_string()),
        Err(e) => io::println(&e.message),
    }
}
"#;
    assert_eq!(agree(src).1, "3\nodd\n");
}

const SCALAR_REFS: &str = r#"use std::io;

fn bump(x: &mut i64, by: i64) -> () effects() {
    *x = *x + by;
}

fn set(f: &mut bool) -> () effects() {
    *f = true;
}

fn read(x: &i64) -> i64 effects() {
    return *x * 2;
}

fn main() -> () effects(io, alloc) {
    let mut n = 5i64;
    bump(&mut n, 3);
    bump(&mut n, -1);
    io::println(&n.to_string());
    io::println(&read(&n).to_string());
    let mut total = 0i64;
    for i in 0i64..4i64 {
        bump(&mut total, i);
    }
    io::println(&total.to_string());
    let mut flag = false;
    set(&mut flag);
    io::println(&flag.to_string());
}
"#;

#[test]
fn references_to_scalars_agree_with_the_interpreter() {
    let (_, out) = agree(SCALAR_REFS);
    assert_eq!(out, "7\n14\n6\ntrue\n");
}

#[test]
fn main_returning_err_reports_it_on_stderr_like_the_rustc_path() {
    let src = "use std::io;\nfn main() -> Result<(), Error> effects(io, alloc) {\n    io::println(\"before\");\n    return Err(Error::new(\"bad thing\"));\n}\n";
    let ir = compile_source(src).unwrap();
    let (_, out) = run_with_output(&ir, "main", DEFAULT_STEPS);
    let (_, nout) = compile(&ir).unwrap().run_with_output("main", DEFAULT_FUEL);
    assert_eq!(out, nout);
    assert_eq!(out.stdout, "before\n");
    assert_eq!(out.stderr, "error: bad thing\n");
    let ok = "use std::io;\nfn main() -> Result<(), Error> effects(io) {\n    io::println(\"fine\");\n    Ok(())\n}\n";
    let (_, out) = run_with_output(&compile_source(ok).unwrap(), "main", DEFAULT_STEPS);
    assert_eq!((out.stdout.as_str(), out.stderr.as_str()), ("fine\n", ""));
}

const ENUMS: &str = r#"use std::io;

enum Command {
    Add(String),
    Rename { from: String, to: String },
    Count(i64),
    Quit,
}

enum Maybe<T> {
    Just(T),
    Nothing,
}

fn describe(c: Command) -> String effects(alloc) {
    match c {
        Command::Add(name) => {
            let mut s = String::from("add ");
            s.push_str(&name);
            return s;
        }
        Command::Rename { from, to } => {
            let mut s = from;
            s.push_str(" -> ");
            s.push_str(&to);
            return s;
        }
        Command::Count(n) => return n.to_string(),
        Command::Quit => return String::from("quit"),
    }
}

fn or_zero(m: Maybe<i64>) -> i64 effects() {
    match m {
        Maybe::Just(v) => return v,
        Maybe::Nothing => return 0,
    }
}

fn text(m: Maybe<String>) -> String effects(alloc) {
    match m {
        Maybe::Just(s) => return s,
        Maybe::Nothing => return String::from("-"),
    }
}

fn main() -> () effects(io, alloc) {
    io::println(&describe(Command::Add(String::from("milk"))));
    io::println(&describe(Command::Rename { from: String::from("a"), to: String::from("b") }));
    io::println(&describe(Command::Count(-7)));
    io::println(&describe(Command::Quit));
    io::println(&(or_zero(Maybe::Just(41)) + or_zero(Maybe::Nothing) + 1).to_string());
    io::println(&text(Maybe::Just(String::from("ravum"))));
    io::println(&text(Maybe::Nothing));
}
"#;

#[test]
fn non_copy_and_generic_enums_agree_with_the_interpreter() {
    let (_, out) = agree(ENUMS);
    assert_eq!(out, "add milk\na -> b\n-7\nquit\n42\nravum\n-\n");
}

#[test]
fn a_vec_of_enums_agrees() {
    let src = r#"use std::io;

enum Shape {
    Dot,
    Line(i64),
    Box { w: i64, h: i64 },
}

fn area(s: Shape) -> i64 effects() {
    match s {
        Shape::Dot => return 0,
        Shape::Line(n) => return n,
        Shape::Box { w, h } => return w * h,
    }
}

fn main() -> () effects(io, alloc) {
    let mut v: Vec<Shape> = Vec::new();
    v.push(Shape::Line(4));
    v.push(Shape::Box { w: 3, h: 5 });
    v.push(Shape::Dot);
    v[0] = Shape::Box { w: 2, h: 2 };
    let mut total = 0i64;
    for s in v {
        total = total + area(s);
    }
    io::println(&total.to_string());
    let mut opts: Vec<Option<i64>> = Vec::new();
    opts.push(Some(3));
    opts.push(None);
    match opts.pop() {
        Some(Some(x)) => io::println(&x.to_string()),
        Some(None) => io::println("inner none"),
        None => io::println("empty"),
    }
    io::println(&opts[0].unwrap_or(0).to_string());
}
"#;
    assert_eq!(agree(src).1, "19\ninner none\n3\n");
}

#[test]
fn mutable_borrows_of_vec_struct_elements_write_back() {
    let src = r#"use std::io;

struct Counter { hits: i64, last: i64 }

impl Counter {
    fn hit(&mut self, v: i64) -> () effects() {
        self.hits = self.hits + 1;
        self.last = v;
    }
}

fn reset(c: &mut Counter, to: i64) -> i64 effects() {
    let before = c.hits;
    c.hits = to;
    return before;
}

fn main() -> () effects(io, alloc) {
    let mut v: Vec<Counter> = Vec::new();
    v.push(Counter { hits: 0, last: 0 });
    v.push(Counter { hits: 10, last: 0 });
    v[1].hit(7);
    v[1].hit(8);
    v[0].hit(1);
    let was = reset(&mut v[1], 100);
    io::println(&was.to_string());
    io::println(&v[0].hits.to_string());
    io::println(&v[1].hits.to_string());
    io::println(&v[1].last.to_string());
}
"#;
    assert_eq!(agree(src).1, "12\n1\n100\n8\n");
}

#[test]
fn cloning_a_vec_of_copy_structs_is_deep() {
    let src = r#"use std::io;

struct P { x: i64, y: i64 }

fn main() -> () effects(io, alloc) {
    let mut a: Vec<P> = Vec::new();
    a.push(P { x: 1, y: 2 });
    a.push(P { x: 3, y: 4 });
    let mut b: Vec<P> = a.clone();
    b[0].x = 100;
    b.push(P { x: 5, y: 6 });
    io::println(&a[0].x.to_string());
    io::println(&b[0].x.to_string());
    io::println(&a.len().to_string());
    io::println(&b.len().to_string());
    io::println(&b[1].y.to_string());
}
"#;
    assert_eq!(agree(src).1, "1\n100\n2\n3\n4\n");
}

#[test]
fn arrays_inside_vec_elements_agree() {
    let src = r#"use std::io;

struct Voice { taps: [f32; 3], gain: f32 }

fn main() -> () effects(io, alloc, panic) {
    let mut v: Vec<Voice> = Vec::new();
    v.push(Voice { taps: [0.5, 0.25, 0.125], gain: 2.0 });
    v.push(Voice { taps: [1.0, 0.0, 0.0], gain: 1.0 });
    v[0].taps[2] = 0.75;
    let mut sum: f32 = 0.0;
    for k in 0usize..3usize {
        sum = sum + v[0].taps[k] * v[0].gain;
    }
    io::println(&sum.to_string());
    io::println(&v[1].taps[0].to_string());
    let bad: usize = 3;
    io::println(&v[0].taps[bad].to_string());
}
"#;
    let (render, out) = agree(src);
    assert_eq!(out, "3\n1\n");
    // Same text as the rustc path (EXPR-7.6, case
    // types/l2-array-in-vec-element), through `Term::TrapBounds` (IR 1.5),
    // which prints nothing and so stays valid in real-time modules (INV-05).
    assert!(
        render.contains("index out of bounds: the len is 3 but the index is 3"),
        "{render}"
    );
}

/// Fail-closed (IR 1.5): the JIT and the hot-swap engine refuse a bounds
/// trap whose operands are out of range or not `u64`, before compiling.
#[test]
fn the_jit_refuses_a_malformed_bounds_trap() {
    use ravum_lang_ir::ir::{Term, Ty};
    let src = "fn gain(x: f32) -> f32 effects() {\n    let a: [f32; 2] = [x, x];\n    let i: usize = 2;\n    return a[i];\n}\n";
    let m = ravum_lang_ir::compile_source(src).unwrap();
    assert!(ravum_lang_jit::compile(&m).is_ok());
    let f = m.func_id("gain").unwrap() as usize;
    let bi = m.functions[f]
        .blocks
        .iter()
        .position(|b| matches!(b.term, Term::TrapBounds { .. }))
        .expect("a bounds trap");
    let Term::TrapBounds { len, .. } = m.functions[f].blocks[bi].term else {
        unreachable!()
    };
    let f32_local = m.functions[f]
        .locals
        .iter()
        .position(|t| *t == Ty::F32)
        .unwrap() as u32;
    for (l, i) in [(len, 7_777), (len, f32_local)] {
        let mut bad = m.clone();
        bad.functions[f].blocks[bi].term = Term::TrapBounds { len: l, index: i };
        assert!(
            ravum_lang_jit::compile(&bad).is_err(),
            "JIT compiled a malformed trap"
        );
        assert!(
            ravum_lang_jit::hot::HotEngine::start(&bad, "gain").is_err(),
            "hot engine started a malformed module"
        );
    }
}

/// OWN-6.9 / P1, documented limit L-OWN-020-1: a match on `&mut v[i]` of a
/// `Vec` of aggregates needs its lent copy written back on every exit of
/// the match (RFC-UI-LANG-1 §2.1); until that is implemented the IR refuses
/// it explicitly instead of approximating it. A shared `&v[i]` is accepted
/// and agrees with the rustc path.
#[test]
fn match_on_mut_vec_aggregate_element_is_refused_not_approximated() {
    let refused = "struct P { x: i64 }\nenum S { A(P), B }\nfn f(v: &mut Vec<S>) -> i64 {\n    match &mut v[0] {\n        S::A(p) => { p.x += 1; p.x }\n        S::B => 0,\n    }\n}\nfn check() -> i64 effects(alloc) {\n    let mut v: Vec<S> = Vec::new();\n    v.push(S::A(P { x: 1 }));\n    return f(&mut v);\n}\n";
    let err = format!("{:?}", compile_source(refused).unwrap_err());
    assert!(
        err.contains("`&mut` borrow of an aggregate `Vec` element outside a call"),
        "{err}"
    );
    let shared = "struct P { x: i64 }\nenum S { A(P), B }\nfn f(v: &Vec<S>) -> i64 {\n    match &v[0] {\n        S::A(p) => p.x,\n        S::B => 0,\n    }\n}\nfn check() -> i64 effects(alloc) {\n    let mut v: Vec<S> = Vec::new();\n    v.push(S::A(P { x: 41 }));\n    return f(&v) + 1;\n}\n";
    let m = compile_source(shared).unwrap();
    let (r, _) = run_with_output(&m, "check", DEFAULT_STEPS);
    assert_eq!(r.render(), "value i64 42");
    assert_eq!(compile(&m).unwrap().run("check", DEFAULT_FUEL).render(), "value i64 42");
}
