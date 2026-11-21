//! MACH-5.3: traps are the same at every optimisation level. The same
//! program built with `build` (opt-level 0) and `build --release` (opt-level
//! 3) panics with the same message and exit code on overflow, division by
//! zero and out-of-bounds indexing.

use std::fs;
use std::process::Command;

fn build(dir: &std::path::Path, release: bool) -> String {
    let mut args = vec!["build", "."];
    if release {
        args.push("--release");
    }
    let out = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
        .args(&args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("built: "))
        .unwrap()
        .to_string()
}

#[test]
fn traps_are_identical_with_and_without_optimisation() {
    let cases = [
        ("overflow", "fn main() -> () effects(panic, io, alloc) {\n    let args: Vec<String> = env::args();\n    let mut x: i32 = 2147483600;\n    x = x + (args.len() as i32) * 100;\n    io::println(&x.to_string());\n}\n"),
        ("divzero", "fn main() -> () effects(panic, io, alloc) {\n    let args: Vec<String> = env::args();\n    let d: i64 = (args.len() as i64) - 1;\n    io::println(&(10 / d).to_string());\n}\n"),
        ("index", "fn main() -> () effects(panic, io, alloc) {\n    let args: Vec<String> = env::args();\n    let v: Vec<i64> = Vec::new();\n    io::println(&v[args.len()].to_string());\n}\n"),
    ];
    for (name, body) in cases {
        let dir = std::env::temp_dir().join(format!("ravum-trap-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(
            dir.join("ravum.toml"),
            format!(
                "[package]\nname = \"trap-{name}\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n"
            ),
        )
        .unwrap();
        fs::write(
            dir.join("src/main.rv"),
            format!("use std::io;\nuse std::env;\n{body}"),
        )
        .unwrap();
        let mut seen = Vec::new();
        for release in [false, true] {
            let artifact = build(&dir, release);
            let out = Command::new(&artifact).output().unwrap();
            let err = String::from_utf8_lossy(&out.stderr).into_owned();
            assert_eq!(
                out.status.code(),
                Some(1),
                "{name} release={release}: {err}"
            );
            assert!(err.contains("panic:"), "{name} release={release}: {err}");
            seen.push((out.stdout, err));
        }
        assert_eq!(
            seen[0], seen[1],
            "{name}: the trap differs between opt-level 0 and 3"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
