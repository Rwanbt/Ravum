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

/// RFC-CONTRACTS-1 on both paths: the same program, a violated `ensures`
/// after two good calls; same output and same violation message.
#[test]
fn contracts_behave_the_same_on_rustc_and_jit() {
    let src = "struct P {\n    x: i64,\n    y: i64,\n}\n\nfn div(a: i64, b: i64) -> i64 effects()\n    requires b != 0\n    ensures result * b + a % b == a\n{\n    return a / b;\n}\n\nfn grow(mut x: i64, p: P) -> i64 effects()\n    requires x >= 0\n    ensures result > x && p.y == 2\n{\n    x = x + 100;\n    if x > 110 {\n        return x - 120;\n    }\n    return x + p.x;\n}\n\nfn shift(mut x: i64) -> i64 effects()\n    ensures result == x + 1\n{\n    x = x + 5;\n    return x - 4;\n}\n\nfn main() -> () effects(io, alloc) {\n    let requires: i64 = 3;\n    io::println(&shift(10).to_string());\n    io::println(&div(7, 2).to_string());\n    io::println(&div(-7, requires).to_string());\n    io::println(&grow(1, P { x: 5, y: 2 }).to_string());\n    io::println(&grow(30, P { x: 5, y: 2 }).to_string());\n}\n";
    let dir = std::env::temp_dir().join(format!("rv-jit-contracts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("ravum.toml"),
        "[package]\nname = \"jitcontracts\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/main.rv"), src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
        .arg("run")
        .arg(&dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(!out.status.success(), "the last call violates `ensures`");
    let msg = "contract violated: ensures result > x && p.y == 2";
    assert!(stderr.contains(msg), "{stderr}");
    let ir = compile_source(src).unwrap();
    let (r, jit) = compile(&ir).unwrap().run_with_output("main", DEFAULT_FUEL);
    assert_eq!(r.render(), format!("panic: {msg}"));
    assert_eq!(jit.stdout, stdout, "output before the violation");
    // `shift`: `ensures` reads `x` as it was on entry (CON-3), not 15.
    assert_eq!(stdout, "11\n3\n-2\n106\n");
    let _ = std::fs::remove_dir_all(&dir);
}

/// IR v1.3 run heap (`String`, `Vec<T>`): same output, byte for byte, and
/// the same out-of-bounds panic text as the rustc build.
#[test]
fn strings_and_vectors_match_the_rustc_build() {
    let src = "fn greet(name: &String) -> String effects(alloc) {\n    let mut s = String::new();\n    s.push_str(\"hello \");\n    s.push_str(name.as_str());\n    return s;\n}\n\nfn fill(v: &mut Vec<i64>, n: i64) -> () effects(alloc) {\n    let mut i: i64 = 0;\n    while i < n {\n        v.push(i * i);\n        i = i + 1;\n    }\n}\n\nfn total(v: &Vec<i64>) -> i64 effects() {\n    let mut s: i64 = 0;\n    let mut i: usize = 0;\n    while i < v.len() {\n        s = s + v[i];\n        i = i + 1;\n    }\n    return s;\n}\n\nfn main() -> () effects(io, alloc) {\n    let who = String::from(\"ravum\");\n    let g = greet(&who);\n    io::println(&g);\n    let mut c = g.clone();\n    c.push_str(\"!\");\n    io::println(&c);\n    io::println(&g);\n    io::println(&g.len().to_string());\n    io::println(&(g.as_str() == c.as_str()).to_string());\n    io::println(&(g.as_str() != c.as_str()).to_string());\n    io::println(&(who.as_str() == \"ravum\").to_string());\n    let e = String::new();\n    io::println(&e.is_empty().to_string());\n    let mut v: Vec<i64> = Vec::new();\n    fill(&mut v, 6);\n    io::println(&v.len().to_string());\n    io::println(&total(&v).to_string());\n    v[2] = 100;\n    let w = v.clone();\n    v[0] = -5;\n    io::println(&v[0].to_string());\n    io::println(&w[0].to_string());\n    io::println(&w[2].to_string());\n    let mut f: Vec<f64> = Vec::with_capacity(4);\n    f.push(0.1);\n    f.push(2.5);\n    io::println(&f[0].to_string());\n    let mut words: Vec<String> = Vec::new();\n    words.push(String::from(\"a\"));\n    words.push(g.clone());\n    io::println(&words[1]);\n    let mut k: i64 = 0;\n    while !v.is_empty() {\n        let top = match v.pop() {\n            Some(x) => x,\n            None => 0,\n        };\n        k = k + top;\n    }\n    io::println(&k.to_string());\n    let none = match v.pop() {\n        Some(x) => x,\n        None => -1,\n    };\n    io::println(&none.to_string());\n    let seven: usize = 7;\n    io::println(&w[seven].to_string());\n}\n";
    let dir = std::env::temp_dir().join(format!("rv-jit-heap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("ravum.toml"),
        "[package]\nname = \"jitheap\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/main.rv"), src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
        .arg("run")
        .arg(&dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(!out.status.success(), "the last access is out of bounds");
    let msg = "index out of bounds: the len is 6 but the index is 7";
    assert!(stderr.contains(msg), "{stderr}");
    let ir = compile_source(src).unwrap();
    let (r, jit) = compile(&ir).unwrap().run_with_output("main", DEFAULT_FUEL);
    assert_eq!(r.render(), format!("panic: {msg}"));
    assert_eq!(jit.stdout, stdout);
    // Same on the interpreter.
    let (ri, oi) =
        ravum_lang_ir::interp::run_with_output(&ir, "main", ravum_lang_ir::interp::DEFAULT_STEPS);
    assert_eq!(ri.render(), r.render());
    assert_eq!(oi.stdout, stdout);
    assert_eq!(stdout.lines().count(), 17, "{stdout}");
    let _ = std::fs::remove_dir_all(&dir);
}
