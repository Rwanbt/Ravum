//! Repository guard: the language crates must stay deterministic.
//!
//! `crates/ravum-lang-*` may not use `HashMap` or `HashSet`: iteration order is
//! not specified, and the language pipeline must be byte-for-byte deterministic
//! (same source => same diagnostics, same generated Rust). `BTreeMap`/`BTreeSet`
//! are the only ordered collections allowed.
//!
//! The guard walks the source tree at test time (no external dependency) and
//! fails on any occurrence, including in comments: the property under test is
//! "the string does not appear in language sources". It also asserts
//! non-vacuity (at least five crates and twenty files were actually scanned), so
//! a moved/renamed tree cannot turn the guard into a green no-op.

use std::path::{Path, PathBuf};

fn crates_dir() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .expect("crates/ravum-lang-sema must have a parent directory")
        .to_path_buf()
}

fn collect_rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => panic!("cannot read `{}`: {error}", dir.display()),
    };
    for entry in entries {
        let entry = entry.expect("cannot read a directory entry");
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn ravum_lang_sources_do_not_use_hash_collections() {
    let root = crates_dir();
    let mut crates_scanned = 0usize;
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&root).expect("cannot read the crates directory") {
        let path = entry.expect("cannot read a crate entry").path();
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        if path.is_dir() && name.starts_with("ravum-lang-") {
            crates_scanned += 1;
            let src = path.join("src");
            if src.is_dir() {
                collect_rust_files(&src, &mut files);
            }
        }
    }

    assert!(
        crates_scanned >= 5,
        "determinism guard is vacuous: only {crates_scanned} ravum-lang-* crates found under {}",
        root.display()
    );
    assert!(
        files.len() >= 20,
        "determinism guard is vacuous: only {} Rust source files found under {}",
        files.len(),
        root.display()
    );

    let mut violations = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file)
            .unwrap_or_else(|error| panic!("cannot read `{}`: {error}", file.display()));
        for forbidden in ["HashMap", "HashSet"] {
            if text.contains(forbidden) {
                let line = text
                    .lines()
                    .position(|line| line.contains(forbidden))
                    .map(|index| index + 1)
                    .unwrap_or(0);
                violations.push(format!("{}:{line}: {forbidden}", file.display()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "HashMap/HashSet are forbidden in crates/ravum-lang-*/src (determinism); \
         use BTreeMap/BTreeSet instead:\n{}",
        violations.join("\n")
    );
}
