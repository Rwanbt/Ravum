//! Soundness oracle for the invalidation planner (gate T13, step 1).
//!
//! For every single-file `.rv` program of `examples/` and `conformance/cases/`
//! that parses cleanly, every `fn` is mutated in three ways:
//!
//! * `body-ok`    — a valid statement is inserted at the start of the body;
//! * `body-error` — an ill-typed statement is inserted (a new diagnostic);
//! * `rename`     — the function is renamed (its callers now fail to resolve).
//!
//! Both versions are checked **cold and whole** by `ravum-lang-sema`. Every
//! item the planner declares reusable must have exactly the same diagnostics
//! (code, severity, position relative to the item) in both versions. Any
//! difference is an unsound plan and fails the test.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ravum_lang_incr::{fingerprint_source, plan, Plan};
use ravum_lang_sema::syntax::{parser, SourceMap, LANGUAGE_EDITION};
use ravum_lang_sema::{check, CheckOptions, Profile};

type ItemDiags = BTreeMap<String, Vec<(String, String, u32, u32)>>;

struct Checked {
    diags: ItemDiags,
    typed: bool,
}

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

fn item_diags(src: &str) -> Checked {
    let mut sources = SourceMap::new();
    let file = sources.add("src/main.rv", src);
    let parsed = parser::parse_file(file, src);
    let options = CheckOptions {
        profile: Profile::Core,
        edition: LANGUAGE_EDITION.to_string(),
    };
    let result = check(&parsed.program, &sources, &options);
    let fp = fingerprint_source(src);
    let mut out: ItemDiags = fp.items.keys().map(|k| (k.clone(), Vec::new())).collect();
    for d in result.diags.iter() {
        for (key, info) in &fp.items {
            if d.span.start >= info.span.start && d.span.end <= info.span.end {
                out.get_mut(key).unwrap().push((
                    d.code.clone(),
                    format!("{:?}", d.severity),
                    d.span.start - info.span.start,
                    d.span.end - info.span.start,
                ));
            }
        }
    }
    for v in out.values_mut() {
        v.sort();
    }
    Checked {
        diags: out,
        typed: result.checked.is_some(),
    }
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

#[test]
fn planner_is_sound_on_the_corpus() {
    let corpus = corpus();
    assert!(corpus.len() >= 20, "corpus too small: {}", corpus.len());
    let (mut cases, mut full, mut reused, mut items) = (0usize, 0usize, 0usize, 0usize);
    let mut failures = Vec::new();
    let mut by_kind: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for (path, old) in &corpus {
        let old_fp = fingerprint_source(old);
        let old_diags = item_diags(old);
        for (what, new) in mutations(old) {
            let new_checked = item_diags(&new);
            let p = plan(&old_fp, &fingerprint_source(&new))
                .confirm_gate(old_diags.typed, new_checked.typed);
            cases += 1;
            items += old_fp.items.len();
            let kind = what.rsplit(' ').next().unwrap_or("?").to_string();
            let entry = by_kind.entry(kind).or_default();
            entry.0 += 1;
            if matches!(p, Plan::Full { .. }) {
                entry.1 += 1;
                full += 1;
                continue;
            }
            let new_diags = &new_checked.diags;
            let old_diags = &old_diags.diags;
            for key in p.reused() {
                reused += 1;
                if old_diags.get(&key) != new_diags.get(&key) {
                    failures.push(format!(
                        "{path} [{what}] item `{key}` reused but diagnostics differ:\n  old {:?}\n  new {:?}",
                        old_diags.get(&key),
                        new_diags.get(&key)
                    ));
                }
            }
        }
    }
    eprintln!(
        "T13 oracle: files={} mutations={cases} full={full} reused_items={reused}/{items}",
        corpus.len()
    );
    for (kind, (n, f)) in &by_kind {
        eprintln!("  {kind}: mutations={n} full={f} partial={}", n - f);
    }
    assert!(
        failures.is_empty(),
        "{} unsound plans:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(
        reused > 0,
        "the planner never reused anything: the oracle proves nothing"
    );
}

#[test]
fn negative_a_planner_that_reuses_everything_is_caught() {
    // Power check: an always-reuse planner must be refuted by the oracle on at
    // least one corpus mutation, otherwise the oracle has no discriminating power.
    let mut refuted = 0usize;
    for (_, old) in corpus() {
        let old_diags = item_diags(&old).diags;
        for (_, new) in mutations(&old) {
            let new_diags = item_diags(&new).diags;
            if old_diags
                .keys()
                .any(|k| old_diags.get(k) != new_diags.get(k))
            {
                refuted += 1;
            }
        }
    }
    assert!(refuted > 0, "oracle cannot refute an always-reuse planner");
}
