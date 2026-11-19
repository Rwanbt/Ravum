//! IR v1.2 `io::print*` on the jit backend vs the reference build path:
//! the same program run by `ravum-lang run` (generated Rust compiled by
//! rustc) and by the in-process native backend prints the same bytes.

use std::process::Command;

use ravum_lang_ir::compile_source;
use ravum_lang_jit::{compile, DEFAULT_FUEL};

fn program() -> String {
    let mut body = String::from("    let z: f64 = 0.0;\n");
    let cases: &[(&str, &[&str])] = &[
        ("i8", &["-128", "127", "0"]),
        ("u8", &["255"]),
        ("i16", &["-32768"]),
        ("u16", &["65535"]),
        ("i32", &["-2147483648", "2147483647"]),
        ("u32", &["4294967295"]),
        ("i64", &["-9223372036854775808", "9223372036854775807"]),
        ("u64", &["18446744073709551615"]),
        ("usize", &["18446744073709551615"]),
        ("bool", &["true", "false"]),
        (
            "f64",
            &[
                "0.1",
                "1.0",
                "-0.0",
                "1.5e300",
                "1e21",
                "1e-7",
                "123456789.125",
                "1.0 / z",
                "-1.0 / z",
                "z / z",
                "0.30000000000000004",
            ],
        ),
        (
            "f32",
            &["0.1", "16777217.0", "-0.0", "3.4028235e38", "1e-7"],
        ),
    ];
    for (i, (ty, vals)) in cases.iter().enumerate() {
        for (j, v) in vals.iter().enumerate() {
            body.push_str(&format!(
                "    let v{i}_{j}: {ty} = {v};\n    io::print(\"{ty}:\");\n    io::println(&v{i}_{j}.to_string());\n"
            ));
        }
    }
    body.push_str("    io::println(v0_0.to_string().as_str());\n    io::println(\"done\");\n");
    format!("fn main() -> () effects(io, alloc) {{\n{body}}}\n")
}

#[test]
fn jit_output_is_byte_identical_to_the_rustc_build() {
    let src = program();
    let dir = std::env::temp_dir().join(format!("rv-jit-print-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("ravum.toml"),
        "[package]\nname = \"jitprint\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/main.rv"), &src).unwrap();
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

    let ir = compile_source(&src).unwrap();
    let (r, jit) = compile(&ir).unwrap().run_with_output("main", DEFAULT_FUEL);
    assert_eq!(r.render(), "value () ()");
    assert!(reference.ends_with("done\n"), "{reference}");
    assert_eq!(
        jit.stdout, reference,
        "jit vs rustc output\n--- jit\n{}\n--- rustc\n{reference}",
        jit.stdout
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Same independent check for user methods (`impl`, `self`, `&self`,
/// `&mut self`, associated functions, methods calling methods, aggregate
/// returns): the reference machine has no `impl`, rustc is the oracle.
#[test]
fn jit_user_methods_match_the_rustc_build() {
    let acc = "struct Acc {\n    total: i64,\n    n: i32,\n}\n\nimpl Acc {\n    fn new(start: i64) -> Acc effects() {\n        return Acc { total: start, n: 0 };\n    }\n    fn add(&mut self, x: i64) -> () effects() {\n        self.total = self.total + x;\n        self.n = self.n + 1;\n    }\n    fn mean(&self) -> i64 effects() {\n        return self.total / (self.n as i64);\n    }\n    fn doubled(self) -> Acc effects() {\n        let mut c = self;\n        c.total = c.total * 2;\n        return c;\n    }\n    fn bump_all(&mut self, k: i64) -> () effects() {\n        self.add(k);\n        self.add(k);\n    }\n}\n\nstruct Pair {\n    a: Acc,\n    b: Acc,\n}\n\nimpl Pair {\n    fn new() -> Pair effects() {\n        return Pair { a: Acc::new(0), b: Acc::new(100) };\n    }\n    fn mean(&self) -> i64 effects() {\n        return self.a.mean() + self.b.mean();\n    }\n}\n\nfn case(x: i64, y: i64) -> i64 effects() {\n    let mut a = Acc::new(x);\n    a.add(y);\n    a.add(9);\n    let b = a.doubled();\n    a.bump_all(y);\n    let mut p = Pair::new();\n    p.a.add(y);\n    p.b.add(4);\n    let k: i64 = 1000;\n    return a.mean() * k + b.total + p.mean();\n}\n\n";
    let mut main = String::from("fn main() -> () effects(io, alloc) {\n");
    for x in [0i64, 1, 5, -7, 123_456_789] {
        for y in [1i64, 3, -2] {
            main.push_str(&format!("    io::println(&case({x}, {y}).to_string());\n"));
        }
    }
    main.push_str("}\n");
    let src = format!("{acc}{main}");
    let dir = std::env::temp_dir().join(format!("rv-jit-methods-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("ravum.toml"),
        "[package]\nname = \"jitmethods\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/main.rv"), &src).unwrap();
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
    assert_eq!(reference.lines().count(), 15, "{reference}");
    let ir = compile_source(&src).unwrap();
    let (r, jit) = compile(&ir).unwrap().run_with_output("main", DEFAULT_FUEL);
    assert_eq!(r.render(), "value () ()");
    assert_eq!(jit.stdout, reference);
    let _ = std::fs::remove_dir_all(&dir);
}
