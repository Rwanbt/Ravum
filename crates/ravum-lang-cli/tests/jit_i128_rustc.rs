//! IR v1.10 positive oracle: the same wide-integer program run through
//! `ravum-lang run` (rustc), the IR interpreter and Cranelift prints the same
//! bytes. Covers calls, references, fixed aggregates, shifts and casts.

use std::process::Command;

use ravum_lang_ir::compile_source;
use ravum_lang_ir::interp::{run_with_output, DEFAULT_STEPS};
use ravum_lang_jit::{compile, DEFAULT_FUEL};

const SOURCE: &str = r#"use std::io;
struct Wide { signed: i128, unsigned: u128 }
fn mix(a: i128, b: u128, c: i64, d: f64) -> i128 effects() {
    let x = a.wrapping_add(b as i128);
    let y = (c as i128).wrapping_mul(3);
    return x.wrapping_add(y).wrapping_add(d as i128);
}
fn read(x: &i128) -> i128 effects() { return *x; }
fn make() -> Wide effects() { return Wide { signed: -9i128, unsigned: 11u128 }; }
fn many(a: i128, b: i128, c: i128, d: u128, e: f64, f: i32) -> i128 effects() {
    return a.wrapping_add(b).wrapping_add(c).wrapping_add(d as i128).wrapping_add(e as i128).wrapping_add(f as i128);
}
fn pick(x: Option<i128>) -> i128 effects() {
    match x { Some(v) => return v, None => return 0i128 }
}
fn main() -> () effects(io, alloc) {
    let min: i128 = -170141183460469231731687303715884105728;
    let max: i128 = 170141183460469231731687303715884105727;
    let umax: u128 = 340282366920938463463374607431768211455;
    io::println(&min.to_string());
    io::println(&max.to_string());
    io::println(&umax.to_string());
    io::println(&min.wrapping_add(1).to_string());
    io::println(&max.saturating_add(1).to_string());
    io::println(&umax.wrapping_add(1).to_string());
    io::println(&umax.saturating_add(1).to_string());
    io::println(&(min >> 127).to_string());
    io::println(&(umax >> 127).to_string());
    io::println(&(umax << 64).to_string());
    io::println(&mix(-5i128, 7u128, 4i64, 2.75).to_string());
    io::println(&many(1i128, 2i128, 3i128, 4u128, 5.75, 6i32).to_string());
    io::println(&read(&max).to_string());
    let w = Wide { signed: min, unsigned: umax };
    let made = make();
    io::println(&made.signed.to_string());
    io::println(&made.unsigned.to_string());
    let a: [i128; 3] = [min, max, 9i128];
    io::println(&(w.signed + a[1] + a[2]).wrapping_add(1).to_string());
    io::println(&w.unsigned.to_string());
    io::println(&pick(Some(41i128)).to_string());
    let big: f64 = 18446744073709551616.0;
    io::println(&(big as u128).to_string());
    io::println(&(max as f64).to_string());
    io::println(&(umax as f32).to_string());
}
"#;

#[test]
fn wide_integer_output_is_byte_identical_on_rustc_interpreter_and_jit() {
    let dir = std::env::temp_dir().join(format!("rv-jit-i128-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("ravum.toml"),
        "[package]\nname = \"jiti128\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/main.rv"), SOURCE).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
        .arg("run")
        .arg(&dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "rustc path failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let reference = String::from_utf8(out.stdout).unwrap();

    let ir = compile_source(SOURCE).unwrap();
    let (interpreted, interpreter_output) = run_with_output(&ir, "main", DEFAULT_STEPS);
    let (native, native_output) = compile(&ir).unwrap().run_with_output("main", DEFAULT_FUEL);
    assert_eq!(native.render(), "value () ()");
    assert_eq!(interpreted.render(), native.render());
    assert_eq!(interpreter_output, native_output);
    assert_eq!(native_output.stdout, reference);
    assert_eq!(reference.lines().count(), 21, "{reference}");
    let _ = std::fs::remove_dir_all(&dir);
}
