//! `lower_for` / `compile_source_for`: only what the entry reaches must be
//! covered by the IR. Host I/O elsewhere (here `env::args` in `main`) does not
//! block an entry that never calls it; an entry that does reach an uncovered
//! construct is refused, naming the function; the strict `compile_source`
//! keeps refusing the whole program.

use ravum_lang_ir::lower::LowerError;
use ravum_lang_ir::{compile_source, compile_source_for, CompileError};
use ravum_lang_jit::{compile, DEFAULT_FUEL};

const SOURCE: &str = r#"
use std::env;
use std::io;
fn total(n: i64) -> i64 {
    let mut s: i64 = 0;
    for i in 0..n {
        s += helper(i);
    }
    s
}
fn helper(i: i64) -> i64 { i * 2 }
fn compute() -> i64 { total(10) }
fn argc() -> usize { env::args().len() }
fn uses_argc() -> i64 { argc() as i64 }
fn main() {
    let n = argc();
    io::println(&n.to_string());
}
"#;

fn unsupported(r: Result<ravum_lang_ir::Module, CompileError>) -> String {
    match r {
        Err(CompileError::Lower(LowerError::Unsupported(w))) => w,
        other => panic!("expected Unsupported, got {other:?}"),
    }
}

#[test]
fn an_entry_that_avoids_host_io_lowers_and_runs() {
    let ir = compile_source_for(SOURCE, &["compute"]).expect("compute lowers");
    let out = compile(&ir).expect("compile").run("compute", DEFAULT_FUEL);
    assert_eq!(
        out.render(),
        ravum_lang_ir::interp::run(&ir, "compute", 1_000_000).render()
    );
    assert!(out.render().contains("90"), "{}", out.render());
    // `main` and `argc` are stubs: they trap with the reason if run.
    let stubbed = compile(&ir).expect("compile").run("main", DEFAULT_FUEL);
    assert!(
        stubbed.render().contains("not available on this backend"),
        "{}",
        stubbed.render()
    );
}

#[test]
fn an_entry_that_reaches_host_io_is_refused_naming_the_function() {
    let w = unsupported(compile_source_for(SOURCE, &["uses_argc"]));
    assert!(
        w.contains("std::env::args") && w.contains("in `argc`"),
        "{w}"
    );
    let w = unsupported(compile_source_for(SOURCE, &["main"]));
    assert!(w.contains("std::env::args"), "{w}");
    // Several entries: one reaching host I/O is enough to refuse.
    let w = unsupported(compile_source_for(SOURCE, &["compute", "uses_argc"]));
    assert!(w.contains("in `argc`"), "{w}");
}

#[test]
fn the_strict_lowering_still_refuses_the_whole_program() {
    let w = unsupported(compile_source(SOURCE));
    assert!(w.contains("std::env::args"), "{w}");
}

#[test]
fn an_unknown_entry_is_an_error_not_an_empty_module() {
    match compile_source_for(SOURCE, &["nope"]) {
        Err(CompileError::Lower(LowerError::MissingFact(w))) => assert!(w.contains("nope")),
        other => panic!("{other:?}"),
    }
}
