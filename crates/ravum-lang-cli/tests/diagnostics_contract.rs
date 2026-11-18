//! Contract tests for `spec/language/diagnostics.md` rules that a single
//! conformance case cannot express: determinism across runs (DIAG-1.8,
//! DIAG-4.4), ordering within a phase (DIAG-4.1), the absence of colour
//! (DIAG-6.5), the shape of the JSON object (DIAG-7.2) and the absence of
//! machine-dependent values (DIAG-6.8, DIAG-7.5).

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Package {
    root: PathBuf,
}

impl Package {
    fn new(tag: &str, source: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("ravum-diag-contract-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).expect("create package");
        fs::write(
            root.join("ravum.toml"),
            "[package]\nname = \"diag\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
        )
        .expect("write manifest");
        fs::write(root.join("src/main.rv"), source).expect("write source");
        Self { root }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
            .args(args)
            .arg(&self.root)
            .current_dir(&self.root)
            .env_remove("NO_COLOR")
            .output()
            .expect("run ravum-lang")
    }

    fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for Package {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Several errors of two phases, out of source order inside each line.
const ERRORS: &str = "fn main() {\n    let a = u;\n    let b: i32 = true;\n    let c = v;\n    \
                      let d: bool = 1;\n    io::println(&(a + c).to_string());\n    \
                      let _e = (b, d);\n}\n";

fn diagnostics(stdout: &[u8]) -> Vec<Value> {
    let object: Value = serde_json::from_slice(stdout).expect("stdout is one JSON object");
    object["diagnostics"]
        .as_array()
        .expect("diagnostics array")
        .clone()
}

#[test]
fn two_runs_give_the_same_bytes() {
    // DIAG-1.8, DIAG-4.4. The first run creates the lockfile, so compare the
    // second and third runs, which see the same package state.
    let package = Package::new("determinism", ERRORS);
    let _ = package.run(&["check", "--json"]);
    let json_a = package.run(&["check", "--json"]);
    let json_b = package.run(&["check", "--json"]);
    assert_eq!(json_a.stdout, json_b.stdout);
    assert_eq!(json_a.status.code(), json_b.status.code());
    let text_a = package.run(&["check"]);
    let text_b = package.run(&["check"]);
    assert_eq!(text_a.stderr, text_b.stderr);
    assert!(!diagnostics(&json_a.stdout).is_empty());
}

#[test]
fn diagnostics_of_one_phase_are_sorted_by_file_byte_and_code() {
    // DIAG-4.1.
    let package = Package::new("order", ERRORS);
    let output = package.run(&["check", "--json"]);
    let found = diagnostics(&output.stdout);
    let mut phases: Vec<&str> = found
        .iter()
        .map(|d| d["phase"].as_str().expect("phase"))
        .collect();
    phases.dedup();
    assert!(
        phases.len() >= 2,
        "the fixture covers two phases: {phases:?}"
    );
    for phase in phases {
        let keys: Vec<(String, u64, String)> = found
            .iter()
            .filter(|d| d["phase"] == phase)
            .map(|d| {
                (
                    d["primary"]["file"].as_str().unwrap().to_string(),
                    d["primary"]["start_byte"].as_u64().unwrap(),
                    d["code"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "phase {phase} is not sorted");
    }
}

#[test]
fn no_output_carries_colour() {
    // DIAG-6.5: no escape sequence on a pipe, with or without `--json`.
    let package = Package::new("colour", ERRORS);
    for args in [&["check"][..], &["check", "--json"][..]] {
        let output = package.run(args);
        assert!(!output.stdout.contains(&0x1b), "{args:?} stdout");
        assert!(!output.stderr.contains(&0x1b), "{args:?} stderr");
    }
}

#[test]
fn json_is_one_utf8_line_without_bom() {
    // DIAG-7.2, DIAG-7.1.
    let package = Package::new("shape", ERRORS);
    let output = package.run(&["check", "--json"]);
    let text = std::str::from_utf8(&output.stdout).expect("UTF-8");
    assert!(!text.starts_with('\u{feff}'));
    let body = text.strip_suffix('\n').unwrap_or(text);
    assert!(!body.contains('\n'), "one object on one line");
    let _: Value = serde_json::from_str(body).expect("one JSON object");
}

#[test]
fn no_absolute_path_leaks_into_diagnostics() {
    // DIAG-6.8, DIAG-7.5.
    let package = Package::new("paths", ERRORS);
    let root = package.path().to_string_lossy().replace('\\', "/");
    for args in [&["check"][..], &["check", "--json"][..]] {
        let output = package.run(args);
        let all = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .replace('\\', "/");
        assert!(!all.contains(&root), "{args:?} leaks {root}");
        assert!(!all.contains("panicked at"), "{args:?} shows a stack trace");
    }
    for diagnostic in diagnostics(&package.run(&["check", "--json"]).stdout) {
        assert_eq!(diagnostic["file"], "src/main.rv");
    }
}
