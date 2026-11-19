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
