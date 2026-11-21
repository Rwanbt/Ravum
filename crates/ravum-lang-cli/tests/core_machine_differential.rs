//! Differential comparison backend ↔ reference machine
//! (`spec/language/core-machine.md` §8, O10, §9).
//!
//! Every program of `crates/ravum-core-machine/corpus/` whose oracle result is
//! a value or a panic is also checked, built and run by `ravum-lang`. The two
//! results must agree: same value (floats compared by IEEE predicates, never by
//! bits, MACH-8.3), or a panic on both sides. A disagreement is a defect to
//! investigate, never a tolerance (MACH-8.1). Programs whose oracle result is
//! `diverge` or `fault` are excluded (Q-MACH-3).

use ravum_core_machine::{run, Outcome, DEFAULT_BUDGET, SPEC_VERSION};
use std::fs;
use std::path::Path;
use std::process::Command;

fn float_agrees(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Option<f64> {
        match s {
            "NaN" => Some(f64::NAN),
            "inf" => Some(f64::INFINITY),
            "-inf" => Some(f64::NEG_INFINITY),
            _ => s.parse().ok(),
        }
    };
    match (parse(a), parse(b)) {
        (Some(x), Some(y)) if x.is_nan() || y.is_nan() => x.is_nan() && y.is_nan(),
        (Some(x), Some(y)) => x == y && x.is_sign_negative() == y.is_sign_negative(),
        _ => false,
    }
}

#[test]
fn the_backend_agrees_with_the_reference_machine_on_the_pure_corpus() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../ravum-core-machine/corpus");
    let mut entries: Vec<_> = fs::read_dir(&corpus)
        .expect("corpus")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("rv"))
        .collect();
    entries.sort();
    let mut compared = 0;
    let mut disagreements = Vec::new();
    for path in entries {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let source = fs::read_to_string(&path).unwrap();
        let oracle = run(&source, "check", SPEC_VERSION, DEFAULT_BUDGET);
        if matches!(oracle, Outcome::Diverge(_) | Outcome::Fault(_)) {
            continue;
        }
        let root =
            std::env::temp_dir().join(format!("ravum-core-diff-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("ravum.toml"),
            "[package]\nname = \"diff\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("src/main.rv"),
            format!("use std::io;\n{source}\nfn main() {{\n    io::println(&check().to_string());\n}}\n"),
        )
        .unwrap();
        let out = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
            .args(["run", "."])
            .current_dir(&root)
            .output()
            .expect("run ravum-lang");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let backend_line = stdout
            .lines()
            .rfind(|l| !l.starts_with("running:"))
            .unwrap_or("")
            .to_string();
        let agrees = match &oracle {
            Outcome::Value { ty, text } => {
                out.status.code() == Some(0)
                    && if ty == "f32" || ty == "f64" {
                        float_agrees(text, &backend_line)
                    } else {
                        *text == backend_line
                    }
            }
            Outcome::Panic(_) => out.status.code() == Some(1) && stderr.contains("panic:"),
            _ => unreachable!(),
        };
        compared += 1;
        if !agrees {
            disagreements.push(format!(
                "{name}: oracle `{}`, backend exit {:?} stdout `{backend_line}` stderr `{}`",
                oracle.render(),
                out.status.code(),
                stderr
                    .lines()
                    .find(|l| l.contains("panic") || l.contains("error"))
                    .unwrap_or("")
            ));
        }
        let _ = fs::remove_dir_all(&root);
    }
    assert!(compared >= 45, "only {compared} programs compared");
    assert!(
        disagreements.is_empty(),
        "{} disagreement(s) out of {compared}:\n{}",
        disagreements.len(),
        disagreements.join("\n")
    );
}
