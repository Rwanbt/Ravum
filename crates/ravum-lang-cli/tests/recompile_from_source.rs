//! COMPAT-4.5: compiled output is always rebuilt from the source; no binary
//! left by an earlier build is reused once the source has changed.

use std::fs;
use std::process::Command;

fn run(root: &std::path::Path) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
        .args(["run", "."])
        .current_dir(root)
        .output()
        .expect("run ravum-lang");
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("UTF-8 stdout")
}

#[test]
fn a_changed_source_is_never_served_from_an_earlier_build() {
    let root = std::env::temp_dir().join(format!("ravum-recompile-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("ravum.toml"),
        "[package]\nname = \"recompile\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/main.rv"),
        "fn main() {\n    io::println(\"first\");\n}\n",
    )
    .unwrap();
    assert!(run(&root).contains("first\n"));
    // Same length, same directory, same binary path: only the bytes differ.
    fs::write(
        root.join("src/main.rv"),
        "fn main() {\n    io::println(\"other\");\n}\n",
    )
    .unwrap();
    let second = run(&root);
    assert!(second.contains("other\n"), "stale build served: {second}");
    assert!(!second.contains("first"), "stale build served: {second}");
    let _ = fs::remove_dir_all(&root);
}
