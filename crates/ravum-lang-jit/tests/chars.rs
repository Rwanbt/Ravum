//! IR v1.9: `char` (its code point in the IR). The interpreter and the JIT
//! agree, and print exactly what the rustc path prints for the same program
//! (`EXPECTED` was produced by `ravum-lang run`): literals with escapes and
//! the largest scalar, `as u32`, `match` on chars, comparisons, `String::push`,
//! `to_string` through a reference, `chars()`, a struct field, `Option<char>`
//! and `Map<char, _>` in key order.

use ravum_lang_ir::compile_source;
use ravum_lang_ir::interp::{run_with_output, DEFAULT_STEPS};
use ravum_lang_jit::{compile, DEFAULT_FUEL};

const SOURCE: &str = r#"
use std::io;
struct Glyph { c: char, n: u32 }
fn kind(c: char) -> i32 {
    match c {
        'a' => 1,
        'é' => 2,
        '😀' => 3,
        '\n' => 4,
        _ => 0,
    }
}
fn show(c: &char) -> () effects(io, alloc) {
    io::println(&c.to_string());
}
fn main() -> () effects(io, alloc, panic) {
    let max = '\u{10FFFF}';
    io::println(&(max as u32).to_string());
    io::println(&('\0' as u32).to_string());
    io::println(&kind('a').to_string());
    io::println(&kind('é').to_string());
    io::println(&kind('😀').to_string());
    io::println(&kind('\n').to_string());
    io::println(&kind('z').to_string());
    io::println(&('a' < 'b').to_string());
    io::println(&('é' > 'z').to_string());
    io::println(&('a' == 'a').to_string());
    let mut s = String::new();
    s.push('a');
    s.push('é');
    s.push('😀');
    s.push('\t');
    io::println(&s.len().to_string());
    io::println(&s.chars().len().to_string());
    let cs = String::from("xé😀").chars();
    let mut back = String::new();
    for i in 0usize..cs.len() {
        back.push(cs[i]);
        show(&cs[i]);
    }
    io::println(back.as_str());
    let g = Glyph { c: '€', n: 3 };
    io::println(&g.c.to_string());
    io::println(&(g.c as u32 + g.n).to_string());
    let o: Option<char> = Some('ß');
    match o {
        Some(c) => io::println(&c.to_string()),
        None => io::println("none"),
    }
    let mut m: Map<char, i64> = Map::new();
    let _a = m.insert('b', 2i64);
    let _b = m.insert('a', 1i64);
    let _c = m.insert('é', 3i64);
    for (k, v) in &m {
        io::print(&k.to_string());
        io::println(&v.to_string());
    }
}
"#;

const EXPECTED: &str =
    "1114111\n0\n1\n2\n3\n4\n0\ntrue\ntrue\ntrue\n8\n4\nx\né\n😀\nxé😀\n€\n8367\nß\na1\nb2\né3\n";

#[test]
fn chars_agree_with_the_rustc_path() {
    let ir = compile_source(SOURCE).expect("chars lower");
    let (interpreted, io_i) = run_with_output(&ir, "main", DEFAULT_STEPS);
    let (native, io_n) = compile(&ir)
        .expect("chars compile")
        .run_with_output("main", DEFAULT_FUEL);
    assert_eq!(native.render(), interpreted.render());
    assert_eq!(io_i.stdout, EXPECTED);
    assert_eq!(io_n.stdout, EXPECTED);
}

/// Negative: a heap word that is not a Unicode scalar (a surrogate, or above
/// U+10FFFF) is a fault, never a replacement character.
#[test]
fn a_non_scalar_word_is_a_fault() {
    use ravum_lang_ir::heap::{Heap, HeapStop};
    use ravum_lang_ir::ir::HeapOp;
    let mut h = Heap::default();
    let s = h.text("x").unwrap();
    for bad in [0xD800u64, 0x11_0000, u64::MAX] {
        assert!(
            matches!(h.op(HeapOp::StrPushChar, s, bad), Err(HeapStop::Fault(_))),
            "{bad:#x}"
        );
        assert!(
            matches!(h.op(HeapOp::StrOfChar, bad, 0), Err(HeapStop::Fault(_))),
            "{bad:#x}"
        );
    }
    assert_eq!(h.str(s).unwrap(), "x", "nothing was appended");
}
