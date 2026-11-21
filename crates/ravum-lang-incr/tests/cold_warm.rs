//! T13 step 2 oracle: an incremental check (`driver::check_incremental`)
//! gives **exactly** the diagnostics of a full check (code, severity,
//! message, span, notes) — over every function edit of the corpus, and over
//! a second edit made from the warm cache (back to the original).

use std::path::{Path, PathBuf};

use ravum_lang_incr::driver::{check_cold, check_incremental};
use ravum_lang_incr::fingerprint_source;
use ravum_lang_sema::syntax::diag::Diagnostic;
use ravum_lang_sema::syntax::parser;

fn rv_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            rv_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "rv") {
            out.push(p);
        }
    }
}

fn corpus() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    rv_files(&root.join("examples"), &mut files);
    rv_files(&root.join("conformance/cases"), &mut files);
    files
        .into_iter()
        .filter_map(|p| {
            let src = std::fs::read_to_string(&p).ok()?;
            // Single-file programs only: `mod x;` needs the loader.
            if src.contains("mod ") {
                return None;
            }
            if !parser::parse_file(0, &src).diags.is_empty() {
                return None;
            }
            Some((p.display().to_string(), src))
        })
        .collect()
}

/// `(kind, mutated source)` for every `fn` of `src`.
fn mutations(src: &str) -> Vec<(String, String)> {
    let fp = fingerprint_source(src);
    let mut out = Vec::new();
    for (key, info) in &fp.items {
        if !matches!(
            info.class,
            ravum_lang_incr::ItemClass::FnDeclaredEffects
                | ravum_lang_incr::ItemClass::FnInferredEffects
        ) {
            continue;
        }
        let text = &src[info.span.start as usize..info.span.end as usize];
        let Some(brace) = text.find('{') else {
            continue;
        };
        let at = info.span.start as usize + brace + 1;
        for (kind, stmt) in [
            ("body-ok", " let __incr_probe: i64 = 0;"),
            ("body-error", " let __incr_probe: i64 = true;"),
        ] {
            let mut m = src.to_string();
            m.insert_str(at, stmt);
            out.push((format!("{key} {kind}"), m));
        }
        let decl = format!("fn {}", info.name);
        if let Some(pos) = text.find(&decl) {
            let at = info.span.start as usize + pos + decl.len();
            let mut m = src.to_string();
            m.insert_str(at, "_incr_renamed");
            out.push((format!("{key} rename"), m));
        }
    }
    out
}

fn norm(ds: &[Diagnostic]) -> Vec<String> {
    let mut v: Vec<String> = ds.iter().map(|d| format!("{d:?}")).collect();
    v.sort();
    v
}

#[test]
fn incremental_check_equals_full_check_on_the_corpus() {
    let corpus = corpus();
    assert!(corpus.len() >= 20, "corpus too small: {}", corpus.len());
    let (mut edits, mut reused, mut checked) = (0usize, 0usize, 0usize);
    let mut failures = Vec::new();
    for (path, old) in &corpus {
        let (d0, cache0, _) = check_incremental(None, old);
        if norm(&d0) != norm(&check_cold(old)) {
            failures.push(format!("{path}: first check differs from cold"));
            continue;
        }
        for (what, new) in mutations(old) {
            edits += 1;
            let (warm, cache1, stats) = check_incremental(Some(&cache0), &new);
            reused += stats.reused_bodies;
            checked += stats.checked_bodies;
            let cold = check_cold(&new);
            if norm(&warm) != norm(&cold) {
                failures.push(format!(
                    "{path} [{what}]: warm != cold\n  warm {:?}\n  cold {:?}",
                    norm(&warm),
                    norm(&cold)
                ));
                continue;
            }
            // Second edit from the warm cache: back to the original.
            let (back, _, _) = check_incremental(Some(&cache1), old);
            if norm(&back) != norm(&d0) {
                failures.push(format!("{path} [{what}]: revert from warm != cold"));
            }
        }
    }
    eprintln!(
        "T13 step 2 cold=warm: files={} edits={edits} bodies reused={reused} checked={checked}",
        corpus.len()
    );
    assert!(
        failures.is_empty(),
        "{} unsound incremental checks:\n{}",
        failures.len(),
        failures
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    // Power: the incremental path really reuses work.
    assert!(edits >= 1000, "edits: {edits}");
    assert!(
        reused * 2 >= checked,
        "too little reuse: {reused} reused, {checked} checked"
    );
}

/// Measurement (ignored): one function edited in a large synthetic file,
/// full check vs incremental check.
/// `cargo test -p ravum-lang-incr --release --test cold_warm measure -- --ignored --nocapture`
#[test]
#[ignore]
fn measure_one_function_edit() {
    for n in [200usize, 1000, 3000] {
        let mut src = String::new();
        for i in 0..n {
            src.push_str(&format!(
                "fn f{i}(x: i64) -> i64 effects() {{\n    let mut acc: i64 = 0;\n    let lim: i64 = 4;\n    for k in 0..lim {{\n        acc = acc + k * 3 - (x % 7);\n    }}\n    if acc > 100 {{\n        acc = acc / 2;\n    }}\n    return acc + x;\n}}\n\n"
            ));
        }
        let lines = src.lines().count();
        let (_, cache, _) = check_incremental(None, &src);
        let edited = src.replacen("acc = acc / 2;", "acc = acc / 3;", 1);
        let mut cold = Vec::new();
        let mut warm = Vec::new();
        for _ in 0..5 {
            let t = std::time::Instant::now();
            let c = check_cold(&edited);
            cold.push(t.elapsed());
            let t = std::time::Instant::now();
            let (w, _, stats) = check_incremental(Some(&cache), &edited);
            warm.push(t.elapsed());
            assert_eq!(norm(&w), norm(&c));
            assert_eq!(stats.checked_bodies, 1, "{stats:?}");
        }
        cold.sort();
        warm.sort();
        eprintln!(
            "{lines} lines ({n} fns): full check {:?}, incremental {:?} (median of 5; 1 body checked)",
            cold[2], warm[2]
        );
    }
}
