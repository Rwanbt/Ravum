//! T14: generic structs are laid out per instance (monomorphization) and
//! agree between the interpreter and the JIT.

use ravum_lang_ir::compile_source;
use ravum_lang_ir::interp::{run_with_output, DEFAULT_STEPS};
use ravum_lang_jit::{compile, DEFAULT_FUEL};

fn agree(source: &str) -> String {
    let ir = compile_source(source).expect("generic struct lowers");
    let (interpreted, interpreted_output) = run_with_output(&ir, "main", DEFAULT_STEPS);
    let (native, native_output) = compile(&ir)
        .expect("generic struct compiles")
        .run_with_output("main", DEFAULT_FUEL);
    assert_eq!(native.render(), interpreted.render());
    assert_eq!(native_output, interpreted_output);
    native_output.stdout
}

#[test]
fn generic_structs_agree_with_the_interpreter() {
    for (a, b) in [(-3i64, 7i64), (0, 0), (i64::MAX, i64::MIN)] {
        let source = format!(
            r#"use std::io;

struct Pair<T> {{ a: T, b: T }}
struct Tagged<T, U> {{ tag: U, value: T }}

fn swap<T>(p: Pair<T>) -> Pair<T> effects() {{
    return Pair {{ a: p.b, b: p.a }};
}}

fn main() -> () effects(io, alloc) {{
    let p = Pair {{ a: {a}i64, b: {b}i64 }};
    let q = swap(p);
    io::println(&q.a.to_string());
    io::println(&q.b.to_string());
    let f = Pair {{ a: 1.5f64, b: 2.5f64 }};
    io::println(&swap(f).a.to_string());
    let t = Tagged {{ tag: 9u8, value: Pair {{ a: true, b: false }} }};
    io::println(&t.tag.to_string());
    io::println(&t.value.b.to_string());
}}
"#
        );
        assert_eq!(agree(&source), format!("{b}\n{a}\n2.5\n9\nfalse\n"));
    }
}

#[test]
fn a_generic_struct_holding_a_heap_object_agrees() {
    let source = r#"use std::io;

struct Boxed<T> { inner: T, count: i64 }

fn main() -> () effects(io, alloc) {
    let mut b = Boxed { inner: String::from("ra"), count: 1 };
    b.inner.push_str("vum");
    b.count = b.count + 1;
    io::println(&b.inner);
    io::println(&b.count.to_string());
}
"#;
    assert_eq!(agree(source), "ravum\n2\n");
}

#[test]
fn an_instance_the_ir_cannot_lay_out_is_still_refused() {
    // `u128` is outside the IR subset: the instance `Holder<u128>` is
    // refused explicitly, not approximated (the same struct at `i64` lowers).
    let source = r#"
struct Holder<T> { item: T }

fn main() -> () effects() {
    let h = Holder { item: 5u128 };
}
"#;
    match compile_source(source) {
        Err(ravum_lang_ir::CompileError::Lower(ravum_lang_ir::lower::LowerError::Unsupported(
            detail,
        ))) => assert_eq!(detail, "type `u128`"),
        other => panic!("expected an explicit refusal, got {other:?}"),
    }
}
