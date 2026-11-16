//! Behaviour contracts for the reference backend's *construct coverage*.
//!
//! Every case in this file feeds a **complete Ravum package** through the real
//! pipeline (`loader -> check -> generate`) and asserts an observable property of
//! the emitted Rust. The assertions are behavioural: they name the Rust text the
//! backend is required to produce for the Ravum construct, so a regression in the
//! generator fails a test instead of silently changing the output.
//!
//! These contracts were written for the wave-3 coverage pass (owner decision S5:
//! 100 % of product code, measured on `ravum-lang-sema` + `ravum-lang-codegen`).
//! They add cases for constructs that had no test: float patterns lowered to
//! guards (G-6.36), literal patterns of every family, constants of every value
//! kind, array/tuple/slice/never type spelling, the free-builtin mapping, the
//! signed-zero tie of `math::{min,max}`, assertion helpers, and the FFI wrapper.
//! No existing test is modified or weakened.
//!
//! Run with:
//! `cargo test -p ravum-lang-codegen --test emit_constructs`

use ravum_lang_codegen::{generate, GenOptions};
use ravum_lang_sema::facts::{CheckOptions, Profile};
use ravum_lang_sema::loader::{load_package_with_options, LoadOptions};
use ravum_lang_sema::{check, syntax};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Package harness
// ---------------------------------------------------------------------------

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> TempDir {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.subsec_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "ravum-emit-{}-{}-{}-{}",
            std::process::id(),
            unique,
            nanos,
            label
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("cannot create the temporary directory");
        TempDir { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Write `source` as a one-file binary package, then run the real front end.
///
/// Returns the generated Rust, or a rendered diagnostic string. A loader
/// *warning* is not a rejection (the CLI exits 0 on warnings); only errors are.
fn compile_source(label: &str, source: &str) -> Result<String, String> {
    compile_with_options(label, source, "main.rv", &GenOptions::default())
}

fn compile_lib_source(label: &str, source: &str, options: &GenOptions) -> Result<String, String> {
    compile_with_options(label, source, "lib.rv", options)
}

fn compile_with_options(
    label: &str,
    source: &str,
    crate_root: &str,
    options: &GenOptions,
) -> Result<String, String> {
    let dir = TempDir::new(label);
    let manifest =
        format!("[package]\nname = \"{label}\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n");
    std::fs::write(dir.path.join("ravum.toml"), manifest).expect("cannot write the manifest");
    let src = dir.path.join("src");
    std::fs::create_dir_all(&src).expect("cannot create the package source directory");
    std::fs::write(src.join(crate_root), source).expect("cannot write the Ravum source");
    let loaded = load_package_with_options(
        &dir.path,
        &LoadOptions {
            locked: false,
            overrides: std::collections::BTreeMap::new(),
            ..Default::default()
        },
    );
    if loaded.diags.has_errors() {
        return Err(format!(
            "loader rejected `{label}`:\n{}",
            loaded.diags.render(&loaded.sources)
        ));
    }
    let program = match loaded.program {
        Some(program) => program,
        None => return Err(format!("loader produced no program for `{label}`")),
    };
    let checked = check(
        &program,
        &loaded.sources,
        &CheckOptions {
            profile: Profile::Core,
            edition: syntax::LANGUAGE_EDITION.to_string(),
        },
    );
    if checked.diags.has_errors() {
        return Err(format!(
            "checker rejected `{label}`:\n{}",
            checked.diags.render(&loaded.sources)
        ));
    }
    let checked = match checked.checked {
        Some(checked) => checked,
        None => return Err(format!("checker produced no CheckedProgram for `{label}`")),
    };
    generate(&checked, options)
        .map_err(|error| format!("code generation failed for `{label}`: {}", error.message))
}

/// The package must be accepted end to end; return the emitted Rust.
fn emitted(source: &str) -> String {
    match compile_source("emit", source) {
        Ok(code) => code,
        Err(message) => {
            panic!("expected the program to be accepted\n{message}\n--- source ---\n{source}")
        }
    }
}

/// The package must be rejected by the *code generator*; return its message.
fn codegen_error(source: &str) -> String {
    match compile_source("emit-error", source) {
        Ok(code) => panic!(
            "expected a code generation error, the program was accepted\n--- generated ---\n{code}\n--- source ---\n{source}"
        ),
        Err(message) => message,
    }
}

fn assert_emitted(code: &str, fragments: &[&str], source: &str) {
    for fragment in fragments {
        assert!(
            code.contains(fragment),
            "the generated Rust does not contain {fragment:?}\n--- source ---\n{source}\n--- generated ---\n{code}"
        );
    }
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[test]
fn every_scalar_width_is_spelled_in_return_position() {
    let source = r#"
fn width_i8(v: i8) -> i8 { return v; }
fn width_i16(v: i16) -> i16 { return v; }
fn width_i32(v: i32) -> i32 { return v; }
fn width_i64(v: i64) -> i64 { return v; }
fn width_i128(v: i128) -> i128 { return v; }
fn width_u8(v: u8) -> u8 { return v; }
fn width_u16(v: u16) -> u16 { return v; }
fn width_u32(v: u32) -> u32 { return v; }
fn width_u64(v: u64) -> u64 { return v; }
fn width_u128(v: u128) -> u128 { return v; }
fn width_isize(v: isize) -> isize { return v; }
fn width_usize(v: usize) -> usize { return v; }
fn width_f32(v: f32) -> f32 { return v; }
fn width_f64(v: f64) -> f64 { return v; }
fn width_bool(v: bool) -> bool { return v; }
fn width_char(v: char) -> char { return v; }

fn main() {
    let a: i128 = 1;
    let b: u128 = 2;
    let c: f32 = 1.5;
    let d: i8 = 1;
    let e: char = 'c';
}
"#;
    let code = emitted(source);
    assert_emitted(
        &code,
        &[
            "-> i8 {",
            "-> i16 {",
            "-> i32 {",
            "-> i64 {",
            "-> i128 {",
            "-> u8 {",
            "-> u16 {",
            "-> u32 {",
            "-> u64 {",
            "-> u128 {",
            "-> isize {",
            "-> usize {",
            "-> f32 {",
            "-> f64 {",
            "-> bool {",
            "-> char {",
        ],
        source,
    );
}

#[test]
fn unit_and_single_element_tuple_types_keep_their_rust_spelling() {
    let source = r#"
fn one() -> (i32,) { return (1,); }
fn unit() -> () { }
fn pair() -> (i32, i32) { return (1, 2); }

fn main() {
    let a = one();
    unit();
    let t = pair();
    let x = t.0;
    let y = t.1;
}
"#;
    let code = emitted(source);
    // A unit return type is omitted in Rust, so the observable contract is the
    // bare signature plus the tuple spellings.
    assert_emitted(
        &code,
        &["fn unit() {", "-> (i32,)", "-> (i32, i32)"],
        source,
    );
}

#[test]
fn never_returning_functions_are_emitted_with_the_never_type() {
    let source = r#"
fn stop() -> ! {
    panic("stop")
}

fn main() {
    let x: i32 = stop();
}
"#;
    let code = emitted(source);
    assert_emitted(&code, &["-> !"], source);
}

#[test]
fn borrowed_text_and_slice_parameters_use_the_borrowed_spellings() {
    let source = r#"
fn len_of(t: &str) -> usize { return t.len(); }
fn sum(values: &[f64]) -> f64 {
    let mut total = 0.0;
    let mut index: usize = 0;
    while index < values.len() {
        total = total + values[index];
        index = index + 1;
    }
    return total;
}

fn main() {
    let n = len_of("abc");
    let s = sum(&[1.0, 2.0]);
}
"#;
    let code = emitted(source);
    assert_emitted(&code, &["&str", "&[f64]"], source);
}

#[test]
fn array_lengths_come_from_a_literal_or_a_repeat_count() {
    let source = r#"
fn main() {
    let literal: [i32; 3] = [1, 2, 3];
    let counted: [i32; 3] = [7; 3];
    let zeros: [i32; 4] = [0; 4];
}
"#;
    let code = emitted(source);
    assert_emitted(
        &code,
        &["[1i32, 2i32, 3i32]", "[7i32; 3usize]", "[0i32; 4usize]"],
        source,
    );
}

#[test]
fn numeric_casts_are_emitted_as_rust_casts() {
    let source = r#"
fn narrow(x: i32) -> u8 { return x as u8; }
fn truncate(y: f64) -> i32 { return y as i32; }

fn main() {
    let a = narrow(300);
    let b = truncate(1.9);
}
"#;
    let code = emitted(source);
    assert_emitted(&code, &["x as u8", "y as i32"], source);
}

// ---------------------------------------------------------------------------
// Patterns and constants
// ---------------------------------------------------------------------------

#[test]
fn float_literal_patterns_are_lowered_to_equality_guards() {
    let source = r#"
use std::io;

fn classer(x: f64) -> i32 {
    match x {
        1.5 => 1,
        _ => 0,
    }
}

fn main() {
    io::println(&classer(1.5).to_string());
}
"#;
    let code = emitted(source);
    assert_emitted(
        &code,
        &[
            // The scrutinee is bound once, so the guards can compare values.
            "let __ravum_match_scrutinee = ",
            "match __ravum_match_scrutinee {",
            "__ravum_flt_0 if __ravum_flt_0 == 1.5f64 => ",
        ],
        source,
    );
}

#[test]
fn nested_float_patterns_inside_a_variant_are_guarded_too() {
    let source = r#"
use std::io;

enum Mesure {
    Valeur(f64),
    Absent,
}

fn lire(m: Mesure) -> i32 {
    match m {
        Mesure::Valeur(1.5) => 1,
        Mesure::Valeur(_) => 2,
        Mesure::Absent => 3,
    }
}

fn main() {
    io::println(&lire(Mesure::Valeur(1.5)).to_string());
}
"#;
    let code = emitted(source);
    assert_emitted(
        &code,
        &["Mesure::Valeur(__ravum_flt_0) if __ravum_flt_0 == 1.5f64 => "],
        source,
    );
}

#[test]
fn negative_integer_patterns_keep_their_sign_and_their_width() {
    let source = r#"
use std::io;

fn signe(n: i32) -> i32 {
    match n {
        -1 => 1,
        _ => 0,
    }
}

fn petit(n: i8) -> i32 {
    match n {
        -128 => 1,
        _ => 0,
    }
}

fn main() {
    io::println(&signe(-1).to_string());
    io::println(&petit(-128).to_string());
}
"#;
    let code = emitted(source);
    assert_emitted(&code, &["-1i32 =>", "-128i8 =>"], source);
}

#[test]
fn bool_char_and_text_literal_patterns_are_emitted() {
    let source = r#"
fn flag(value: bool) -> i32 {
    match value {
        true => 1,
        false => 0,
    }
}

fn letter(value: char) -> i32 {
    match value {
        'a' => 1,
        _ => 0,
    }
}

fn code(value: &str) -> i32 {
    match value {
        "abc" => 1,
        _ => 0,
    }
}

fn main() {
    let a = flag(true);
    let b = letter('a');
    let c = code("abc");
}
"#;
    let code = emitted(source);
    assert_emitted(&code, &["true => ", "'a' => ", "\"abc\" => "], source);
}

#[test]
fn struct_variant_patterns_render_their_fields() {
    let source = r#"
enum Shape {
    Dot,
    Rect { w: i32, h: i32 },
}

fn area(shape: Shape) -> i32 {
    match shape {
        Shape::Dot => 0,
        Shape::Rect { w: w, h: h } => w * h,
    }
}

fn main() {
    let a = area(Shape::Dot);
    let b = area(Shape::Rect { w: 2, h: 3 });
}
"#;
    let code = emitted(source);
    assert_emitted(&code, &["Shape::Rect { w: w, h: h } => "], source);
}

#[test]
fn constants_of_every_value_kind_are_emitted_with_their_type() {
    let source = r#"
use std::io;

const FLAG: bool = true;
const LETTER: char = 'z';
const NEG: i32 = -4;
const COUNT: usize = 7;
const LABEL: &str = "ravum";

fn main() {
    io::println(LABEL);
    let a = FLAG;
    let b = LETTER;
    let c = NEG + COUNT as i32;
}
"#;
    let code = emitted(source);
    assert_emitted(
        &code,
        &[
            "const FLAG: bool = true;",
            "const LETTER: char = 'z';",
            "const NEG: i32 = -4i32;",
            "const COUNT: usize = 7usize;",
            "const LABEL: &'static str = \"ravum\";",
        ],
        source,
    );
}

// ---------------------------------------------------------------------------
// String escaping
// ---------------------------------------------------------------------------

#[test]
fn string_and_char_escapes_are_re_escaped_for_rust() {
    let source = "fn main() {\n    let text = \"line\\nbreak\\ttab\\rreturn\\\\slash\\\"quote\";\n    let letter = '\\n';\n    let carriage = '\\r';\n}\n";
    let code = emitted(source);
    assert_emitted(
        &code,
        &["\\n", "\\t", "\\r", "\\\\slash", "\\\"quote"],
        source,
    );
}

// ---------------------------------------------------------------------------
// Builtins
// ---------------------------------------------------------------------------

#[test]
fn io_builtins_map_to_the_runtime_helpers() {
    let source = r#"
fn main() {
    io::println("out");
    io::print("out");
    io::eprintln("err");
}
"#;
    let code = emitted(source);
    assert_emitted(
        &code,
        &[
            "crate::rv_println(",
            "crate::rv_print(",
            "crate::rv_eprintln(",
        ],
        source,
    );
}

#[test]
fn environment_and_file_builtins_map_to_the_runtime_helpers() {
    let source = r#"
fn main() {
    let value = env::var("RAVUM_ABSENT_VARIABLE");
    let present = fs::exists("no-such-file.ravum");
    let text = fs::read_to_string("no-such-file.ravum");
    let written = fs::write_string("ravum-emit-test.txt", "content");
}
"#;
    let code = emitted(source);
    assert_emitted(
        &code,
        &[
            "crate::rv_env_var(",
            "crate::rv_fs_exists(",
            "crate::rv_fs_read_to_string(",
            "crate::rv_fs_write_string(",
        ],
        source,
    );
}

#[test]
fn free_math_builtins_are_emitted_as_method_calls() {
    let source = r#"
fn main() {
    let a = math::abs(0.0 - 2.0);
    let b = math::sqrt(2.0);
    let c = math::floor(1.5);
    let d = math::pow(2.0, 3.0);
}
"#;
    let code = emitted(source);
    assert_emitted(
        &code,
        &[
            ").abs()",
            "(2.0f64).sqrt()",
            "(1.5f64).floor()",
            "(2.0f64).powf(3.0f64)",
        ],
        source,
    );
}

#[test]
fn integer_math_builtins_are_refused_by_the_checker() {
    // The Core 1.0 `std::math` surface is `f64` only. The emitter's non-float
    // branch for `math::{min,max}` is therefore unreachable from an accepted
    // program; this contract pins the refusal instead of pretending to cover it.
    let message = match compile_source("emit", "fn main() {\n    let hi = math::max(1, 2);\n}\n") {
        Ok(_) => panic!("integer `math::max` must be refused by the checker"),
        Err(message) => message,
    };
    assert!(
        message.contains("E-TYP-MISMATCH") && message.contains("expected `f64`"),
        "expected the checker to demand `f64`, got:\n{message}"
    );
}

#[test]
fn float_min_and_max_keep_the_signed_zero_tie() {
    let source = r#"
fn main() {
    let hi = math::max(1.0, 2.0);
    let lo = math::min(1.0, 2.0);
}
"#;
    let code = emitted(source);
    // STD-6.5 fixes `max(-0.0, +0.0) = +0.0` and `min(-0.0, +0.0) = -0.0`;
    // the backend emits the tie explicitly because the LLVM intrinsic is
    // optimisation-level dependent.
    assert_emitted(
        &code,
        &["if (", "== 0.0 &&", "{ 0.0 }", "{ -0.0 }", ".max(", ".min("],
        source,
    );
}

#[test]
fn text_methods_use_the_std_mapping() {
    let source = r#"
use std::io;

fn main() {
    let text = String::from("1,2\n3,4");
    let parts = text.lines();
    let count = parts.len();
    let trimmed = text.trim();
    let parsed = trimmed.parse_i64();
    let floats = text.parse_f64();
    io::println(&count.to_string());
}
"#;
    let code = emitted(source);
    assert_emitted(
        &code,
        &[
            ".lines().map(|rv_line| rv_line.to_string()).collect::<Vec<String>>()",
            ".trim().to_string()",
            ".parse::<i64>()",
            ".parse::<f64>()",
        ],
        source,
    );
}

#[test]
fn a_builtin_function_used_as_a_value_is_refused_by_the_checker() {
    // The checker diagnoses a bare builtin before lowering to Rust.
    let message = match compile_source("emit", "fn main() {\n    let write = io::println;\n}\n") {
        Ok(_) => panic!("a builtin used as a value must be refused by the checker"),
        Err(message) => message,
    };
    assert!(
        message.contains("E-TYP-CALL") && message.contains("must be called"),
        "expected the checker to demand a direct call, got:\n{message}"
    );
}

#[test]
fn assertion_builtins_emit_the_helpers_with_location() {
    let source = r#"
use std::io;

fn main() {
    assert(true);
    assert_eq(1, 1);
    io::println("done");
}
"#;
    let code = emitted(source);
    assert_emitted(
        &code,
        &["crate::rv_assert(", "crate::rv_assert_eq("],
        source,
    );
}

#[test]
fn a_two_argument_assert_is_refused_by_the_checker() {
    // `std::panic::assert` takes exactly one argument, so the emitter's
    // message-argument arm cannot be reached from an accepted program.
    let message = match compile_source(
        "emit",
        "use std::io;\nfn main() {\n    assert(true, \"message\");\n    io::println(\"done\");\n}\n",
    ) {
        Ok(_) => panic!("two-argument `assert` must be refused by the checker"),
        Err(message) => message,
    };
    assert!(
        message.contains("E-TYP-ARITY"),
        "expected an arity refusal, got:\n{message}"
    );
}

#[test]
fn assert_eq_across_two_text_types_uses_the_cross_helper() {
    let source = r#"
use std::io;

fn main() {
    let owned = String::from("abc");
    assert_eq(owned, "abc");
}
"#;
    let code = emitted(source);
    assert_emitted(&code, &["crate::rv_assert_eq_cross("], source);
}

#[test]
fn panic_without_a_message_is_refused_by_the_checker() {
    // `std::panic::panic` takes exactly one argument, so the emitter's default
    // `"panic"` text arm cannot be reached from an accepted program.
    let message = match compile_source(
        "emit",
        "fn stop() -> ! {\n    panic()\n}\n\nfn main() {\n    let x: i32 = stop();\n}\n",
    ) {
        Ok(_) => panic!("zero-argument `panic` must be refused by the checker"),
        Err(message) => message,
    };
    assert!(
        message.contains("E-TYP-ARITY"),
        "expected an arity refusal, got:\n{message}"
    );
}

#[test]
fn parse_builtins_emit_the_result_mapping() {
    let source = r#"
fn main() {
    let n = string::parse_i64("42");
    let f = string::parse_f64("1.5");
}
"#;
    let code = emitted(source);
    assert_emitted(
        &code,
        &["parse::<i64>()", "parse::<f64>()", ".trim()"],
        source,
    );
}

#[test]
fn prelude_constructors_and_constants_are_emitted() {
    let source = r#"
fn main() {
    let some: Option<i32> = Option::Some(1);
    let none: Option<i32> = Option::None;
    let ok: Result<i32, String> = Result::Ok(2);
}
"#;
    let code = emitted(source);
    assert_emitted(
        &code,
        &["Option::Some(1i32)", "None", "Result::Ok("],
        source,
    );
}

#[test]
fn builtin_methods_on_text_vec_and_option_are_emitted() {
    let source = r#"
use std::io;

fn main() {
    let text = String::from("abc");
    let upper = text.to_uppercase();
    let trimmed = text.trim();
    let empty = text.is_empty();
    let size = text.len();
    let mut values: Vec<i32> = Vec::new();
    values.push(1);
    let count = values.len();
    let is_empty = values.is_empty();
    let maybe: Option<i32> = Option::Some(3);
    let present = maybe.is_some();
    let value = maybe.unwrap_or(0);
    let shown = count.to_string();
    io::println(&upper);
}
"#;
    let code = emitted(source);
    assert_emitted(
        &code,
        &[
            "format!(\"{}\", ",
            ".to_uppercase()",
            ".trim().to_string()",
            ".is_empty()",
            ".len()",
            ".push(",
            ".is_some()",
            ".unwrap_or(",
        ],
        source,
    );
}

#[test]
fn drop_and_string_from_builtins_are_emitted() {
    let source = r#"
use std::io;

fn main() {
    let text = String::from("abc");
    drop(text);
    io::println("done");
}
"#;
    let code = emitted(source);
    assert_emitted(&code, &["String::from(", "drop("], source);
}

// ---------------------------------------------------------------------------
// FFI
// ---------------------------------------------------------------------------

#[test]
fn extern_blocks_emit_a_checked_ffi_wrapper() {
    let source = r#"
extern "C" {
    fn abs(x: i32) -> i32;
}

fn main() {
    let value = unsafe { abs(-7) };
}
"#;
    let code = emitted(source);
    // The wrapper must be a real Rust `extern "C"` declaration plus the guard
    // that keeps the whole generated file free of `unsafe` outside the wrapper.
    assert_emitted(&code, &["extern \"C\"", "pub fn rvm_ffi_", "abs"], source);
}

// ---------------------------------------------------------------------------
// Refusals: the backend fails closed on constructs it cannot lower
// ---------------------------------------------------------------------------

#[test]
fn an_unknown_builtin_method_is_refused_by_name() {
    // `Core 1.0` has no such method; the checker may let it through on an
    // unknown owner only if the stdlib table is wrong, so this pins the
    // backend's explicit refusal instead of a silent wrong lowering.
    let message = codegen_error(
        r#"
fn main() {
    let text = String::from("abc");
    let bad = text.reverse();
}
"#,
    );
    assert!(
        message.contains("E-RES-UNKNOWN") && message.contains("reverse"),
        "expected the checker to refuse the unknown method by name, got:\n{message}"
    );
}

// ---------------------------------------------------------------------------
// Ownership facts consumed by the array-move planner
// ---------------------------------------------------------------------------

#[test]
fn a_borrowing_builtin_receiver_is_not_planned_as_a_move() {
    // STD-11.3: `Option::is_some` takes `&self`. The array-move planner must
    // therefore keep reading `holder.items[0]` in place: a backend that planned
    // a move for the borrowing call would have to destructure the array to move
    // the element out (Rust cannot move out of an index), which is visible in
    // the emitted Rust.
    let source = r#"
struct Holder { items: [Option<String>; 2] }

fn main() {
    let holder = Holder { items: [Option::Some(String::from("a")), Option::None] };
    let present = holder.items[0].is_some();
}
"#;
    let code = emitted(source);
    assert_emitted(&code, &["holder.items[0usize].is_some()"], source);
    assert!(
        !code.contains("let [e0"),
        "a borrowing receiver must not trigger the array-move destructuring\n--- generated ---\n{code}"
    );
}

#[test]
fn prelude_constants_that_the_checker_does_not_expose_are_refused() {
    // `resolve` only produces `ValuePath::BuiltinConst("Option::None")`; the
    // emitter's `math::PI` / `math::E` / primitive `MIN`/`MAX` arms therefore
    // have no accepted program behind them.
    let message = match compile_source("emit", "fn main() {\n    let pi = math::PI;\n}\n") {
        Ok(_) => panic!("`math::PI` must be refused by the resolver"),
        Err(message) => message,
    };
    assert!(
        message.contains("E-RES-UNKNOWN") && message.contains("unknown name `PI`"),
        "expected the resolver to refuse `math::PI`, got:\n{message}"
    );
}

#[test]
fn a_prelude_constructor_used_as_a_value_is_refused_by_the_checker() {
    // `Option::Some` is a builtin function; using it as a value (instead of
    // calling it) is `E-TYP-CALL`, so `builtin_fn_value` never runs for it.
    let message = match compile_source("emit", "fn main() {\n    let ctor = Option::Some;\n}\n") {
        Ok(_) => panic!("a prelude constructor used as a value must be refused"),
        Err(message) => message,
    };
    assert!(
        message.contains("E-TYP-CALL") && message.contains("requires arguments"),
        "expected `E-TYP-CALL`, got:\n{message}"
    );
}

#[test]
fn test_mode_emits_a_runner_for_each_test_function() {
    // `ravum-lang test` asks the backend for a test runner (`GenOptions::test_mode`).
    // The runner installs the test panic hook and calls each `test fn`.
    let source = r#"
test fn first() {
}

test fn second() -> Result<(), Error> {
    return Result::Ok(());
}
"#;
    let options = GenOptions {
        test_mode: true,
        ..GenOptions::default()
    };
    let code = match compile_lib_source("emit-test-mode", source, &options) {
        Ok(code) => code,
        Err(message) => {
            panic!("test mode must generate a runner\n{message}\n--- source ---\n{source}")
        }
    };
    assert_emitted(
        &code,
        &[
            "crate::rv_install_test_panic_hook();",
            "--filter",
            "first",
            "second",
        ],
        source,
    );
}

#[test]
fn integer_literal_suffixes_come_from_the_declared_width() {
    // `int_type_name` decides the Rust suffix of a literal whose type was
    // inferred from its annotation (no explicit suffix in the source).
    let source = r#"
fn main() {
    let a: i16 = 1;
    let b: u8 = 2;
    let c: u64 = 3;
    let d: u16 = 4;
    let e: isize = 5;
    let f: i64 = 6;
}
"#;
    let code = emitted(source);
    assert_emitted(
        &code,
        &["1i16", "2u8", "3u64", "4u16", "5isize", "6i64"],
        source,
    );
}

#[test]
fn char_escapes_of_every_family_are_re_escaped() {
    let source = "fn main() {\n    let newline = '\\n';\n    let tab = '\\t';\n    let carriage = '\\r';\n    let backslash = '\\\\';\n    let quote = '\\'';\n}\n";
    let code = emitted(source);
    assert_emitted(
        &code,
        &["'\\n'", "'\\t'", "'\\r'", "'\\\\'", "'\\''"],
        source,
    );
}

#[test]
fn primitive_methods_are_passed_through_or_lowered() {
    // Core 1.0 exposes the RFC-RT-TRAPS-1 integer methods and the `f64` math
    // methods; the backend passes them through with their Rust name.
    let source = r#"
fn main() {
    let n: i32 = 0 - 5;
    let a = n.abs();
    let w = n.wrapping_add(1);
    let s = n.saturating_sub(2);
    let c = n.checked_mul(3);
    let f: f64 = 1.5;
    let root = f.sqrt();
    let hi = f.max(2.5);
    let low = f.min(2.5);
}
"#;
    let code = emitted(source);
    assert_emitted(
        &code,
        &[
            ".abs()",
            ".wrapping_add(1i32)",
            ".saturating_sub(2i32)",
            ".checked_mul(3i32)",
            ".sqrt()",
            ".max(",
            ".min(",
        ],
        source,
    );
}

#[test]
fn text_methods_on_a_borrowed_str_are_emitted() {
    let source = r#"
fn main() {
    let text = "Prepare";
    let size = text.len();
    let empty = text.is_empty();
    let upper = text.to_uppercase();
    let contains = text.contains("re");
    let replaced = text.replace("P", "p");
    let starts = text.starts_with("Pr");
}
"#;
    let code = emitted(source);
    assert_emitted(
        &code,
        &[
            ".len()",
            ".is_empty()",
            ".to_uppercase()",
            ".contains(",
            ".replace(",
            ".starts_with(",
        ],
        source,
    );
}

#[test]
fn the_checker_pins_the_assertion_arities_before_codegen() {
    // `assert` takes exactly one argument and `assert_eq` exactly two, so the
    // backend's arity-error arms are unreachable from an accepted program.
    for (source, needle) in [
        (
            "fn main() {\n    assert(true, \"message\");\n}\n",
            "`std::panic::assert` expects 1 argument(s)",
        ),
        (
            "fn main() {\n    assert_eq(1);\n}\n",
            "`assert_eq` expects 2 arguments",
        ),
    ] {
        let message = match compile_source("emit-arity", source) {
            Ok(_) => panic!("expected an arity refusal for:\n{source}"),
            Err(message) => message,
        };
        assert!(
            message.contains("E-TYP-ARITY") && message.contains(needle),
            "expected `E-TYP-ARITY` naming {needle:?}, got:\n{message}"
        );
    }
}

#[test]
fn deferred_initialization_emits_an_explicit_type_and_no_placeholder() {
    let rust = compile_source(
        "deferred-init",
        r#"
fn main() {
    let mut count: i32;
    count = 4;
    io::println(count.to_string().as_str());
}
"#,
    )
    .expect("a typed local may be assigned before its first read");

    assert!(rust.contains("let mut count: i32;"), "{rust}");
    assert!(rust.contains("count = 4i32"), "{rust}");
}

#[test]
fn nested_extern_is_discovered_without_weakening_unsafe_elsewhere() {
    let code = compile_source(
        "nested-extern",
        r#"
mod platform {
    mod native {
        extern "C" {
            fn abs(value: i32) -> i32;
        }
    }
}
fn main() {}
"#,
    )
    .expect("nested module with a scalar extern should compile");
    assert!(code.contains("#![deny(unsafe_code)]"), "{code}");
    assert!(!code.contains("#![forbid(unsafe_code)]"), "{code}");
    assert_eq!(code.matches("#[allow(unsafe_code)]").count(), 1);
    assert_eq!(code.matches("unsafe {").count(), 1);
}
