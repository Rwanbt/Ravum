//! Modules in the IR: inline modules (and the file modules the loader
//! expands into them) are flattened; homonyms in different modules stay
//! distinct, `super::`/`crate::` paths resolve as the checker decided, a
//! `main` inside a module is not the entry point, and methods of a struct
//! declared in a module work. `EXPECTED` is the rustc path's output.

use ravum_lang_ir::compile_source;
use ravum_lang_ir::interp::{run_with_output, DEFAULT_STEPS};
use ravum_lang_jit::{compile, DEFAULT_FUEL};

const SOURCE: &str = r#"
use std::io;
mod a {
    pub struct P { pub x: i64 }
    pub fn f() -> i64 { 1 }
    pub fn main() -> i64 { 100 }
    pub mod b {
        pub fn f() -> i64 { 2 }
        pub fn g() -> i64 { super::f() + crate::f() + 10 }
    }
    impl P {
        pub fn get(&self) -> i64 { self.x + f() }
    }
}
fn f() -> i64 { 3 }
fn main() {
    io::println(&a::f().to_string());
    io::println(&a::b::f().to_string());
    io::println(&a::b::g().to_string());
    io::println(&f().to_string());
    io::println(&a::main().to_string());
    let p = a::P { x: 5 };
    io::println(&p.get().to_string());
}
"#;

const EXPECTED: &str = "1\n2\n14\n3\n100\n6\n";

#[test]
fn modules_agree_with_the_rustc_path() {
    let ir = compile_source(SOURCE).expect("modules lower");
    let (interpreted, io_i) = run_with_output(&ir, "main", DEFAULT_STEPS);
    let (native, io_n) = compile(&ir)
        .expect("compile")
        .run_with_output("main", DEFAULT_FUEL);
    assert_eq!(native.render(), interpreted.render());
    assert_eq!(io_i.stdout, EXPECTED);
    assert_eq!(io_n.stdout, EXPECTED);
}
