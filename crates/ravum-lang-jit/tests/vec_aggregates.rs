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
