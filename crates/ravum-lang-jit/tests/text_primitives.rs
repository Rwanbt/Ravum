//! RFC-UI-LANG-1 P4 (STD-7.7, STD-9.13..STD-9.19): the text primitives agree
//! between the interpreter and the JIT, edge cases included, and panic with
//! the texts of the rustc path's prelude (`rv_text_*`).

use ravum_lang_ir::compile_source;
use ravum_lang_ir::interp::{run_with_output, DEFAULT_STEPS};
use ravum_lang_jit::{compile, DEFAULT_FUEL};

fn agree(source: &str) -> (String, String) {
    let ir = compile_source(source).expect("text primitives lower");
    let (interpreted, interpreted_output) = run_with_output(&ir, "main", DEFAULT_STEPS);
    let (native, native_output) = compile(&ir)
        .expect("text primitives compile")
        .run_with_output("main", DEFAULT_FUEL);
    assert_eq!(native.render(), interpreted.render());
    assert_eq!(native_output, interpreted_output);
    (native.render(), native_output.stdout)
}

const EDGES: &str = r#"use std::io;
use std::string;
fn show(o: Option<usize>) -> () effects(io, alloc) {
    match o {
        Some(i) => io::println(&i.to_string()),
        None => io::println("none"),
    }
}
fn main() -> () effects(io, alloc, panic) {
    let s = String::from("aé😀b");
    show(s.find(""));
    show(s.find("😀"));
    show(s.find("z"));
    show(String::from("").find(""));
    let v = String::from(",a,").split(",");
    io::println(&v.len().to_string());
    io::println(string::join(&v, "|").as_str());
    let e: Vec<String> = Vec::new();
    io::println(&string::join(&e, ",").len().to_string());
    let one = String::from("x").split(";");
    io::println(string::join(&one, ";").as_str());
    match s.strip_prefix("") {
        Some(r) => io::println(r.as_str()),
        None => io::println("none"),
    }
    match s.strip_suffix("😀b") {
        Some(r) => io::println(r.as_str()),
        None => io::println("none"),
    }
    match s.strip_prefix("é") {
        Some(r) => io::println(r.as_str()),
        None => io::println("none"),
    }
    io::println(&s.chars().len().to_string());
    io::println(&s.len().to_string());
    io::println(&s.is_char_boundary(0).to_string());
    io::println(&s.is_char_boundary(2).to_string());
    io::println(&s.is_char_boundary(8).to_string());
    io::println(&s.is_char_boundary(9).to_string());
    io::println(s.substring(1, 3).as_str());
    io::println(&s.substring(8, 8).len().to_string());
    io::println(&s.starts_with("aé").to_string());
    io::println(&s.ends_with("a").to_string());
}
"#;

#[test]
fn edge_cases_agree_with_the_interpreter() {
    let (render, out) = agree(EDGES);
    assert!(!render.contains("panic"), "{render}");
    assert_eq!(
        out,
        "0\n3\nnone\n0\n3\n|a|\n0\nx\naé😀b\naé\nnone\n4\n8\ntrue\nfalse\ntrue\nfalse\né\n0\ntrue\nfalse\n"
    );
}

#[test]
fn text_primitives_panic_with_the_specified_texts() {
    for (body, text) in [
        ("let v = String::from(\"abc\").split(\"\");\n    drop(v);", "empty separator"),
        ("let t = String::from(\"abc\").substring(2, 1);\n    drop(t);", "substring out of range"),
        ("let t = String::from(\"abc\").substring(0, 4);\n    drop(t);", "substring out of range"),
        (
            "let t = String::from(\"é\").substring(1, 2);\n    drop(t);",
            "substring not on a char boundary",
        ),
        (
            "let t = String::from(\"é\").substring(0, 1);\n    drop(t);",
            "substring not on a char boundary",
        ),
    ] {
        let src = format!("fn main() -> () effects(alloc, panic) {{\n    {body}\n}}\n");
        let (render, out) = agree(&src);
        assert_eq!(out, "");
        assert!(render.contains(text), "{body}: {render}");
    }
}
