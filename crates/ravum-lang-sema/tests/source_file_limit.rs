//! T4 (Q1-1): the source-file ceiling is 65 536, not 200.
//!
//! The audit of 2026-11-14 confirmed a hard cap of 200 source files per
//! package. `NodeId` is now a `u64` (high 32 bits `FileId`, low 32 bits
//! per-file counter), and `MAX_SOURCE_FILES` is 65 536. This test loads and
//! checks a package of 1 000 modules (1 002 sources with the manifest and
//! `main.rv`), which the previous cap refused.
//!
//! The refusal path is covered by the loader unit test
//! `the_source_file_limit_is_refused_with_the_existing_diagnostic`, which lowers
//! the limit thread-locally: creating 65 537 files is not a unit test.

use ravum_lang_sema::facts::{CheckOptions, Profile};
use ravum_lang_sema::loader::{load_package_with_options, LoadOptions};
use ravum_lang_sema::{check, syntax};
use ravum_lang_syntax::ast::ItemKind;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.subsec_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "ravum-source-limit-{tag}-{}-{unique}-{nanos}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("cannot create the temporary directory");
        TempDir { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

const MODULES: usize = 1_000;

#[test]
fn a_package_of_a_thousand_source_files_is_accepted() {
    let temp = TempDir::new("thousand");
    let src = temp.path().join("src");
    std::fs::create_dir_all(&src).expect("cannot create src/");
    std::fs::write(
        temp.path().join("ravum.toml"),
        "[package]\nname = \"thousand\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
    )
    .expect("cannot write the manifest");

    let mut main = String::new();
    for index in 0..MODULES {
        main.push_str(&format!("mod m{index};\n"));
    }
    main.push_str(&format!(
        "\nfn main() {{\n    let value = crate::m{}::f{}(1);\n    io::println(&value.to_string());\n}}\n",
        MODULES - 1,
        MODULES - 1
    ));
    std::fs::write(src.join("main.rv"), main).expect("cannot write main.rv");
    for index in 0..MODULES {
        std::fs::write(
            src.join(format!("m{index}.rv")),
            format!("pub fn f{index}(x: i32) -> i32 {{ return x + {index}; }}\n"),
        )
        .expect("cannot write a module");
    }

    let loaded = load_package_with_options(
        temp.path(),
        &LoadOptions {
            locked: false,
            overrides: BTreeMap::new(),
            ..Default::default()
        },
    );
    assert!(
        !loaded.diags.has_errors(),
        "loader errors: {:?}",
        loaded
            .diags
            .iter()
            .map(|diag| (diag.code.clone(), diag.message.clone()))
            .collect::<Vec<_>>()
    );
    let program = loaded.program.expect("the loader produced no program");
    let modules = program
        .root
        .items
        .iter()
        .filter(|item| matches!(item.kind, ItemKind::Mod(_)))
        .count();
    assert_eq!(modules, MODULES, "expected {MODULES} loaded modules");

    let result = check(
        &program,
        &loaded.sources,
        &CheckOptions {
            profile: Profile::Core,
            edition: syntax::LANGUAGE_EDITION.to_string(),
        },
    );
    assert!(
        !result.diags.has_errors(),
        "checker errors: {:?}",
        result
            .diags
            .iter()
            .map(|diag| (diag.code.clone(), diag.message.clone()))
            .collect::<Vec<_>>()
    );
    assert!(result.checked.is_some(), "the checker produced no program");
}
