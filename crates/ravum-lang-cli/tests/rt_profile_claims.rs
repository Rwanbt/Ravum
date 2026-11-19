//! CONC-3.2: the `rt` profile checks that no forbidden effect is reachable; it
//! never certifies real-time behaviour. No report, text or JSON, passing or
//! failing, may contain `CERTIFIED`.

use std::fs;
use std::process::Command;

const PASSES: &str =
    "fn sum(a: i64, b: i64) -> i64 effects() {\n    return a.wrapping_add(b);\n}\n\n\
                      fn main() effects() {\n    let _s = sum(1, 2);\n}\n";
const FAILS: &str = "fn sum(a: i64, b: i64) -> i64 effects() {\n    return a + b;\n}\n\n\
                     fn main() {\n    io::println(&sum(1, 2).to_string());\n}\n";

#[test]
fn an_rt_report_never_claims_certification() {
    for (tag, source, expected) in [("pass", PASSES, 0), ("fail", FAILS, 1)] {
        let root =
            std::env::temp_dir().join(format!("ravum-rt-claims-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("ravum.toml"),
            "[package]\nname = \"rt\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
        )
        .unwrap();
        fs::write(root.join("src/main.rv"), source).unwrap();
        for json in [false, true] {
            let mut args = vec!["check", "--profile", "rt"];
            if json {
                args.push("--json");
            }
            args.push(".");
            let output = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
                .args(&args)
                .current_dir(&root)
                .output()
                .expect("run ravum-lang");
            assert_eq!(output.status.code(), Some(expected), "{tag} {args:?}");
            let all = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                all.contains("rt"),
                "{tag} {args:?}: the report names its profile"
            );
            assert!(
                !all.to_ascii_uppercase().contains("CERTIFIED"),
                "{tag} {args:?} claims certification:\n{all}"
            );
        }
        let _ = fs::remove_dir_all(&root);
    }
}
