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
            "let v: Vec<P> = Vec::new(); let w = v.clone();",
            "`clone` of a `Vec` of aggregates",
        ),
        (
            "let mut v: Vec<P> = Vec::new(); v.push(P { a: 1, b: 2 }); bump(&mut v[0]);",
            "`&mut` borrow of an aggregate `Vec` element",
        ),
        (
            "let mut v: Vec<Option<i64>> = Vec::new(); v.push(None);",
            "`Vec` of aggregates holding an enum",
        ),
    ] {
        let source = format!(
            "struct P {{ a: i64, b: i64 }}\nfn bump(p: &mut P) -> () effects() {{ p.a = 3; }}\nfn main() -> () effects(alloc) {{ {body} }}\n"
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
