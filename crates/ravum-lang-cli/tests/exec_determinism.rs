//! CONC-3.4, CONC-1.1: a program that reads no argument, variable, input or
//! file produces the same output on every run (one thread, no scheduler).

use std::fs;
use std::process::Command;

#[test]
fn a_closed_program_prints_the_same_bytes_on_every_run() {
    let root = std::env::temp_dir().join(format!("ravum-exec-determinism-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("ravum.toml"),
        "[package]\nname = \"det\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
    )
    .unwrap();
    // Collections, arithmetic, a loop and string building: every source of
    // order a runtime could vary (hashing, scheduling) would show here.
    fs::write(
        root.join("src/main.rv"),
        "use std::io;\nfn main() {\n    let mut v: Vec<i64> = Vec::new();\n    let mut i: i64 = 0;\n    \
         while i < 50 {\n        v.push((i * 7919) % 101);\n        i = i + 1;\n    }\n    \
         let mut s = String::new();\n    let mut k: usize = 0;\n    \
         while k < v.len() {\n        s.push_str(&v[k].to_string());\n        \
         s.push_str(\",\");\n        k = k + 1;\n    }\n    io::println(&s);\n}\n",
    )
    .unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
            .args(["run", "."])
            .current_dir(&root)
            .output()
            .expect("run ravum-lang")
    };
    let first = run();
    assert_eq!(
        first.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(!first.stdout.is_empty());
    for _ in 0..3 {
        let again = run();
        assert_eq!(again.status.code(), Some(0));
        assert_eq!(
            again.stdout, first.stdout,
            "the output changed between runs"
        );
    }
    let _ = fs::remove_dir_all(&root);
}
