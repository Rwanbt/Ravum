//! `Option::ok_or`, `Result::ok`, `Result::err` in the IR: the interpreter
//! and the JIT agree and print what the rustc path prints (`EXPECTED` from
//! `ravum-lang run`), with scalar, heap (`String`) and aggregate payloads,
//! both branches of each, and the argument of `ok_or` evaluated whatever the
//! tag.

use ravum_lang_ir::compile_source;
use ravum_lang_ir::interp::{run_with_output, DEFAULT_STEPS};
use ravum_lang_jit::{compile, DEFAULT_FUEL};

const SOURCE: &str = r#"
use std::io;
struct P { x: i64, name: String }
fn show(r: Result<i64, Error>) -> () effects(io, alloc) {
    match r {
        Ok(v) => io::println(&v.to_string()),
        Err(e) => io::println(e.message.as_str()),
    }
}
fn half(n: i64) -> Option<i64> {
    if n % 2 == 0 { Some(n / 2) } else { None }
}
fn main() -> () effects(io, alloc) {
    show(half(8).ok_or(Error::new("odd")));
    show(half(7).ok_or(Error::new("odd")));
    let none: Option<String> = None;
    match none.ok_or(Error::new("missing")) {
        Ok(s) => io::println(s.as_str()),
        Err(e) => io::println(e.message.as_str()),
    }
    let p: Option<P> = Some(P { x: 3, name: String::from("é") });
    match p.ok_or(Error::new("no p")) {
        Ok(q) => {
            io::println(&q.x.to_string());
            io::println(q.name.as_str());
        }
        Err(e) => io::println(e.message.as_str()),
    }
    let good: Result<String, i32> = Ok(String::from("yes"));
    let bad: Result<String, i32> = Err(-4);
    match good.ok() {
        Some(s) => io::println(s.as_str()),
        None => io::println("none"),
    }
    io::println(&bad.clone().ok().is_none().to_string());
    match bad.err() {
        Some(c) => io::println(&c.to_string()),
        None => io::println("none"),
    }
    let r2: Result<i64, String> = Ok(1);
    io::println(&r2.err().is_none().to_string());
}
"#;

const EXPECTED: &str = "4\nodd\nmissing\n3\né\nyes\ntrue\n-4\ntrue\n";

#[test]
fn option_and_result_conversions_agree_with_the_rustc_path() {
    let ir = compile_source(SOURCE).expect("conversions lower");
    let (interpreted, io_i) = run_with_output(&ir, "main", DEFAULT_STEPS);
    let (native, io_n) = compile(&ir)
        .expect("compile")
        .run_with_output("main", DEFAULT_FUEL);
    assert_eq!(native.render(), interpreted.render());
    assert_eq!(io_i.stdout, EXPECTED);
    assert_eq!(io_n.stdout, EXPECTED);
}
