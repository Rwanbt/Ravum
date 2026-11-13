//! Phase 6.1 — prove (or disprove) that `NodeId`s are stable per file.
//!
//! The audit (2026-11-13, C5) suspected that adding an instruction to one file
//! shifts the identifiers of every file loaded after it, which would make any
//! per-file cache unsound. The parser allocates `NodeId` as
//! `(FileId << 24) | per_file_counter` (`ravum-lang-syntax/src/parser.rs`), so
//! the hypothesis is only true if the `FileId` itself moves.
//!
//! The test loads a two-file package, then loads it again with one extra
//! statement in `main.rv` (through the loader's in-memory overrides, so the
//! first load is untouched), and compares the **entire AST snapshot** of the
//! unchanged module (`Debug` includes ids and spans). A difference is a phase-6
//! blocker; equality is the proof that the phase is without object.

use ravum_lang_sema::loader::{load_package_with_options, LoadOptions};
use ravum_lang_syntax::ast::{ItemKind, Module, Program};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> TempDir {
        let path = std::env::temp_dir().join(format!(
            "ravum-nodeids-{}-{}-{label}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("src")).expect("cannot create the package");
        std::fs::write(
            path.join("ravum.toml"),
            "[package]\nname = \"nodeids\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
        )
        .expect("cannot write the manifest");
        std::fs::write(
            path.join("src/main.rv"),
            "mod util;\nfn main() { let x = util::value(); }\n",
        )
        .expect("cannot write main.rv");
        std::fs::write(
            path.join("src/util.rv"),
            "pub fn value() -> i32 { let a = 41; return a + 1; }\npub fn other() -> i32 { return 2; }\n",
        )
        .expect("cannot write util.rv");
        TempDir { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn load(root: &Path, overrides: BTreeMap<PathBuf, String>) -> Program {
    let loaded = load_package_with_options(
        root,
        &LoadOptions {
            locked: false,
            overrides,
        },
    );
    assert!(
        loaded.diags.is_empty(),
        "loader diagnostics: {:?}",
        loaded.diags.iter().map(|diag| diag.code.clone()).collect::<Vec<_>>()
    );
    loaded.program.expect("the loader produced no program")
}

fn find_module<'a>(module: &'a Module, name: &str) -> Option<&'a Module> {
    for item in &module.items {
        if let ItemKind::Mod(inner) = &item.kind {
            if inner.name.as_ref().map(|ident| ident.name.as_str()) == Some(name) {
                return Some(inner);
            }
        }
        if let ItemKind::Mod(inner) = &item.kind {
            if let Some(found) = find_module(inner, name) {
                return Some(found);
            }
        }
    }
    None
}

#[test]
fn editing_one_file_does_not_move_the_other_files_node_ids() {
    let package = TempDir::new("stable");
    let before = load(&package.path, BTreeMap::new());
    let util_before = find_module(&before.root, "util").expect("util module must be loaded");
    let util_before = format!("{util_before:?}");

    let main_path = std::fs::canonicalize(package.path.join("src/main.rv"))
        .expect("cannot canonicalize main.rv");
    let mut overrides = BTreeMap::new();
    overrides.insert(
        main_path,
        "mod util;\nfn main() { let y = 1; let x = util::value(); }\n".to_string(),
    );
    let after = load(&package.path, overrides);
    let util_after = find_module(&after.root, "util").expect("util module must be loaded");
    let util_after = format!("{util_after:?}");

    // Non-vacuity: the edited file must actually have changed.
    assert_ne!(
        format!("{:?}", before.root),
        format!("{:?}", after.root),
        "the override did not change the program; the test would be vacuous"
    );
    assert_eq!(
        util_before, util_after,
        "adding a statement to main.rv changed util.rv's ids/spans: NodeId is not stable per file"
    );
}

#[test]
fn file_ids_are_contained_in_the_high_byte_of_node_ids() {
    let package = TempDir::new("high-byte");
    let program = load(&package.path, BTreeMap::new());
    let util = find_module(&program.root, "util").expect("util module must be loaded");
    let util_file = util.id >> 24;
    let root_file = program.root.id >> 24;
    assert!(util_file > 0, "the loaded module must have a non-zero FileId");
    assert_ne!(
        util_file, root_file,
        "two files must live in two different high bytes (else ids collide across files)"
    );
    assert_ne!(
        util.id & 0x00ff_ffff,
        0,
        "the per-file counter is stored in the low 24 bits"
    );
    // Every item of the unchanged module belongs to the same file byte.
    for item in &util.items {
        assert_eq!(
            item.id >> 24,
            util_file,
            "an item of util.rv carries another file's high byte"
        );
    }
}
