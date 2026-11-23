//! DIAG-5.5: every diagnostic carries a span of at least one scalar. A
//! dependency refused by name (`std`, STD-1.2) used to print `--> <unknown>`;
//! it must point at its declaration in `ravum.toml`.

use std::fs;
use std::process::Command;

#[test]
fn a_refused_dependency_points_at_its_manifest_entry() {
    let root = std::env::temp_dir().join(format!("ravum-dep-span-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    for (dir, toml, file, src) in [
        (
            "app",
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n\n[dependencies]\nstd = { path = \"../std\" }\n",
            "src/main.rv",
            "fn main() {\n}\n",
        ),
        (
            "std",
            "[package]\nname = \"std\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
            "src/lib.rv",
            "pub fn f() -> i32 {\n    1\n}\n",
        ),
    ] {
        fs::create_dir_all(root.join(dir).join("src")).unwrap();
        fs::write(root.join(dir).join("ravum.toml"), toml).unwrap();
        fs::write(root.join(dir).join(file), src).unwrap();
    }
    let out = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
        .args(["check", "."])
        .current_dir(root.join("app"))
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(err.contains("error[E-RES-CONFLICT]"), "{err}");
    assert!(
        err.contains("--> ravum.toml:7:"),
        "the span must be the manifest entry: {err}"
    );
    assert!(!err.contains("<unknown>"), "{err}");
    let _ = fs::remove_dir_all(&root);
}
