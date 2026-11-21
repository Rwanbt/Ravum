//! The `check ⇒ build` contract at the code-generation boundary, over the
//! whole conformance corpus and every example, in process.
//!
//! `ravum-conformance run --verify-build` checks the same contract through the
//! `ravum-lang` binary and `rustc`. That run is the reference, but it happens
//! in another process, so the backend's own coverage measurement never sees
//! it. This test drives the real loader, checker and backend on every program
//! of the corpus and requires, for each program `check` accepts:
//!
//! * code generation succeeds, in normal mode and in test mode (the shapes
//!   `ravum-lang build` and `ravum-lang test` ask for);
//! * the generated Rust is byte-identical across two runs (determinism, S7);
//! * with source markers disabled, no `// rv:` marker appears.
//!
//! A program `check` refuses is skipped: refusing is the checker's verdict to
//! make, and the conformance corpus pins which code it gives.
//!
//! The corpus is copied to a temporary directory first, because loading a
//! package writes its `ravum.lock` next to the sources.

use ravum_lang_codegen::{generate, GenOptions};
use ravum_lang_sema::facts::{CheckOptions, Profile};
use std::fs;
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("cannot create the copy directory");
    for entry in fs::read_dir(from).expect("cannot list the corpus directory") {
        let entry = entry.expect("cannot read a corpus entry");
        let kind = entry.file_type().expect("cannot read an entry type");
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            if entry.file_name() != ".ravum" {
                copy_tree(&entry.path(), &target);
            }
        } else if kind.is_file() {
            fs::copy(entry.path(), &target).expect("cannot copy a corpus file");
        }
    }
}

/// Every package directory (a `ravum.toml`) and every standalone `.rv` file
/// that sits in a directory without a manifest.
fn programs(root: &Path, out: &mut Vec<PathBuf>) {
    let manifest = root.join("ravum.toml");
    let mut entries: Vec<PathBuf> = fs::read_dir(root)
        .expect("cannot list a corpus directory")
        .map(|entry| entry.expect("cannot read a corpus entry").path())
        .collect();
    entries.sort();
    if manifest.is_file() {
        out.push(root.to_path_buf());
    }
    for path in entries {
        if path.is_dir() {
            if path
                .file_name()
                .is_some_and(|name| name != "src" && name != ".ravum")
            {
                programs(&path, out);
            }
        } else if !manifest.is_file() && path.extension().is_some_and(|ext| ext == "rv") {
            out.push(path);
        }
    }
}

fn profile_of(program: &Path) -> Profile {
    let case = if program.is_dir() {
        program.join("case.json")
    } else {
        program.with_file_name("case.json")
    };
    let text = fs::read_to_string(case).unwrap_or_default();
    if text.contains("\"profile\": \"pure\"") || text.contains("\"profile\":\"pure\"") {
        Profile::Pure
    } else if text.contains("\"profile\": \"rt\"") || text.contains("\"profile\":\"rt\"") {
        Profile::Rt
    } else {
        Profile::Core
    }
}

/// ABI-4.7 (amended 2026-11-21): the only `allow`s in generated Rust are the
/// one file-level closed list and the local `allow(unsafe_code)` of the FFI
/// and DSP wrappers. Returns the first offending line, if any.
const ABI_4_7_HEADER: &str = "#![allow(dead_code, unused_variables, unused_mut, unused_assignments, unused_imports, unused_parens, unused_braces, unused_labels, unused_must_use, unused_comparisons, non_snake_case, non_camel_case_types, non_upper_case_globals, unreachable_code, unreachable_patterns, arithmetic_overflow, unconditional_panic)]";

fn abi_4_7_violation(code: &str) -> Option<String> {
    let mut headers = 0;
    for line in code.lines() {
        let t = line.trim();
        if !(t.starts_with("#[allow") || t.starts_with("#![allow") || t.contains("allow(")) {
            continue;
        }
        if t.starts_with("//") {
            continue;
        }
        if t == ABI_4_7_HEADER {
            headers += 1;
        } else if t != "#[allow(unsafe_code)]" {
            return Some(t.to_string());
        }
    }
    (headers != 1).then(|| format!("{headers} file-level allow lists (expected exactly 1)"))
}

struct Tally {
    programs: usize,
    generated: usize,
    failures: Vec<String>,
}

fn generate_corpus(root: &Path, tally: &mut Tally) {
    let mut found = Vec::new();
    programs(root, &mut found);
    for program in found {
        tally.programs += 1;
        let loaded = ravum_lang_sema::load_package(&program);
        let Some(parsed) = loaded.program else {
            continue;
        };
        if loaded.diags.has_errors() {
            continue;
        }
        let options = CheckOptions {
            profile: profile_of(&program),
            ..CheckOptions::default()
        };
        let result = ravum_lang_sema::check(&parsed, &loaded.sources, &options);
        let Some(checked) = result.checked else {
            continue;
        };
        let label = program
            .strip_prefix(root)
            .unwrap_or(&program)
            .display()
            .to_string();
        let modes = [
            GenOptions::default(),
            GenOptions {
                test_mode: true,
                ..GenOptions::default()
            },
            GenOptions {
                source_comments: false,
                ..GenOptions::default()
            },
        ];
        for (index, options) in modes.iter().enumerate() {
            let first = generate(&checked, options);
            let second = generate(&checked, options);
            match (first, second) {
                (Ok(first), Ok(second)) => {
                    if first != second {
                        tally
                            .failures
                            .push(format!("{label} (mode {index}): non-deterministic output"));
                    }
                    if !options.source_comments && first.contains("// rv: ") {
                        tally
                            .failures
                            .push(format!("{label}: source markers emitted while disabled"));
                    }
                    if let Some(line) = abi_4_7_violation(&first) {
                        tally
                            .failures
                            .push(format!("{label} (mode {index}): ABI-4.7 forbids `{line}`"));
                    }
                    if index == 0 {
                        tally.generated += 1;
                    }
                }
                (Err(error), _) | (_, Err(error)) => tally
                    .failures
                    .push(format!("{label} (mode {index}): {error}")),
            }
        }
    }
}

#[test]
fn every_checked_program_of_the_corpus_generates_deterministic_rust() {
    // The checker and the backend recurse on the tree; run them on a stack
    // sized like the toolchain's own (`ravum-lang` uses a 64 MiB thread).
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(|| {
            let scratch =
                std::env::temp_dir().join(format!("ravum-corpus-codegen-{}", std::process::id()));
            let _ = fs::remove_dir_all(&scratch);
            let mut tally = Tally {
                programs: 0,
                generated: 0,
                failures: Vec::new(),
            };
            for corpus in ["conformance/cases", "examples"] {
                let copy = scratch.join(corpus);
                copy_tree(&repo().join(corpus), &copy);
                generate_corpus(&copy, &mut tally);
            }
            let _ = fs::remove_dir_all(&scratch);
            assert!(
                tally.failures.is_empty(),
                "{} failure(s):\n{}",
                tally.failures.len(),
                tally.failures.join("\n")
            );
            // Non-vacuity: `tools/s7_snapshot.sh` emits Rust for 264 targets of
            // the same corpus; a walk that finds far fewer has lost its inputs.
            assert!(
                tally.generated >= 250,
                "only {} of {} programs generated code",
                tally.generated,
                tally.programs
            );
            eprintln!(
                "corpus codegen: {} programs, {} generated",
                tally.programs, tally.generated
            );
        })
        .expect("the thread starts")
        .join()
        .expect("the corpus generates");
}

/// Negative control of the ABI-4.7 check: a lint outside the closed list, a
/// second list and a missing list are each reported.
#[test]
fn abi_4_7_check_reports_any_other_allow() {
    let ok = format!("{ABI_4_7_HEADER}\nfn main() {{}}\n#[allow(unsafe_code)]\nfn w() {{}}\n");
    assert_eq!(abi_4_7_violation(&ok), None);
    let extra = format!("{ok}#[allow(clippy::all)]\n");
    assert!(abi_4_7_violation(&extra).unwrap().contains("clippy::all"));
    assert!(abi_4_7_violation("fn main() {}\n").is_some(), "no list");
    let twice = format!("{ABI_4_7_HEADER}\n{ABI_4_7_HEADER}\n");
    assert!(abi_4_7_violation(&twice).is_some(), "two lists");
}
