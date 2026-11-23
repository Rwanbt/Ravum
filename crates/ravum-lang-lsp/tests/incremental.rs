//! T13 step 3 in the language server: `Analysis::run_incremental` reuses the
//! bodies of unchanged files and gives, edit after edit, the diagnostics and
//! the facts (hover/definition inputs) of a full analysis.

use std::collections::BTreeMap;
use std::path::PathBuf;

use ravum_lang_lsp::analysis::{override_key, Analysis};
use ravum_lang_sema::loader::ParseCache;
use ravum_lang_sema::Profile;

fn codes(a: &Analysis) -> Vec<(String, u32)> {
    a.diags
        .iter()
        .map(|d| (d.code.clone(), d.span.start))
        .collect()
}

#[test]
fn incremental_analysis_equals_full_analysis_across_edits() {
    let root: PathBuf = std::env::temp_dir().join(format!("rv-lsp-incr-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("ravum.toml"),
        "[package]\nname = \"lspincr\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
    )
    .unwrap();
    let util = "pub fn value(a: i64) -> i64 effects() {\n    let b = a * 2;\n    return b + 1;\n}\n\npub fn other() -> i64 effects() {\n    return 5;\n}\n";
    let main = "mod util;\n\nfn main() -> () effects() {\n    let x: i64 = util::value(3);\n    drop(x);\n}\n";
    std::fs::write(root.join("src/util.rv"), util).unwrap();
    std::fs::write(root.join("src/main.rv"), main).unwrap();
    let key = override_key(&root.join("src/main.rv"));

    let mut cache = ParseCache::default();
    let mut ids = BTreeMap::new();
    let mut state = None;
    let edits = [
        main.to_string(),
        main.replace("util::value(3)", "util::value(4)"),
        main.replace("let x: i64", "let x: bool"),
        main.replace("util::value(3)", "util::nope(3)"),
        main.to_string(),
    ];
    for text in edits {
        let overrides: BTreeMap<_, _> = [(key.clone(), text.clone())].into_iter().collect();
        let warm = Analysis::run_incremental(
            &root,
            overrides.clone(),
            Profile::Core,
            &mut cache,
            &mut ids,
            &mut state,
        );
        let mut cold_cache = ParseCache::default();
        let mut cold_ids = ids.clone();
        let cold = Analysis::run_with_cache_and_file_ids(
            &root,
            overrides,
            Profile::Core,
            &mut cold_cache,
            &mut cold_ids,
        );
        assert_eq!(codes(&warm), codes(&cold), "diagnostics for\n{text}");
        assert_eq!(
            warm.checked.as_ref().map(|c| c.facts.expr_types.clone()),
            cold.checked.as_ref().map(|c| c.facts.expr_types.clone()),
            "facts for\n{text}"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}
