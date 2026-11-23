//! T13 step 3 oracle: on multi-file packages, an incremental package check
//! (`package::check_package`) gives the same diagnostics **and the same
//! facts** as a full check, edit after edit (bodies, signatures, renames,
//! errors), including the way back from a warm state.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ravum_lang_incr::package::check_package;
use ravum_lang_sema::facts::Facts;
use ravum_lang_sema::loader::{load_package_with_parse_cache, LoadOptions, ParseCache};
use ravum_lang_sema::syntax::FileId;
use ravum_lang_sema::{check, CheckOptions, Profile};

fn options() -> CheckOptions {
    CheckOptions {
        profile: Profile::Core,
        edition: ravum_lang_sema::syntax::LANGUAGE_EDITION.to_string(),
    }
}

/// A generated package: `main.rv` and three modules that call one another.
fn package(seed: u32) -> BTreeMap<&'static str, String> {
    let k = seed % 7;
    let mut files = BTreeMap::new();
    files.insert(
        "main.rv",
        format!(
            "use std::io;\nmod geo;\nmod calc;\nmod text;\n\nfn main() -> () effects(io, alloc) {{\n    let p = geo::Point {{ x: {k}, y: 2 }};\n    let n = calc::total(p.x, p.y);\n    let s = text::label(n);\n    io::println(&s);\n    io::println(&geo::norm(p).to_string());\n}}\n\nfn local(a: i64) -> i64 effects() {{\n    return a + {k};\n}}\n"
        ),
    );
    files.insert(
        "geo.rv",
        format!(
            "pub struct Point {{\n    pub x: i64,\n    pub y: i64,\n}}\n\npub enum Shape {{\n    Dot,\n    Line(i64),\n}}\n\npub fn norm(p: Point) -> i64 effects() {{\n    return p.x * p.x + p.y * p.y + {k};\n}}\n\npub fn kind(s: Shape) -> i64 effects() {{\n    return match s {{\n        Shape::Dot => 0,\n        Shape::Line(n) => n,\n    }};\n}}\n\nfn unused_helper(x: i64) -> i64 effects() {{\n    let tmp = x * 2;\n    return x;\n}}\n"
        ),
    );
    files.insert(
        "calc.rv",
        format!(
            "use crate::geo;\n\npub fn total(a: i64, b: i64) -> i64 effects() {{\n    let mut acc: i64 = 0;\n    let mut i: i64 = 0;\n    while i < b {{\n        acc = acc + a;\n        i = i + 1;\n    }}\n    return acc + double(a) + geo::kind(geo::Shape::Line({k}));\n}}\n\npub fn double(x: i64) -> i64 effects() {{\n    return x * 2;\n}}\n"
        ),
    );
    files.insert(
        "text.rv",
        "pub fn label(n: i64) -> String effects(alloc) {\n    let mut s = String::from(\"n=\");\n    s.push_str(&n.to_string());\n    return s;\n}\n".to_string(),
    );
    files
}

/// `(what, file, new text)` edits of one package.
fn edits(files: &BTreeMap<&'static str, String>) -> Vec<(String, &'static str, String)> {
    let mut out = Vec::new();
    for (name, text) in files {
        for (i, (at, _)) in text.match_indices(" effects(").enumerate() {
            let Some(brace) = text[at..].find("{\n") else {
                continue;
            };
            let pos = at + brace + 2;
            for (kind, stmt) in [
                ("body-ok", "    let probe: i64 = 0;\n"),
                ("body-error", "    let probe: i64 = true;\n"),
                (
                    "body-move",
                    "    let s0 = String::new();\n    let s1 = s0;\n    let s2 = s0;\n",
                ),
            ] {
                let mut t = text.clone();
                t.insert_str(pos, stmt);
                out.push((format!("{name} fn#{i} {kind}"), *name, t));
            }
        }
        // Interface edits: a signature change and a rename seen by callers.
        if let Some(t) = text.find("pub fn double(x: i64) -> i64").map(|_| {
            text.replace(
                "pub fn double(x: i64) -> i64",
                "pub fn double(x: i32) -> i64",
            )
        }) {
            out.push((format!("{name} signature"), *name, t));
        }
        if text.contains("pub fn norm(") {
            out.push((
                format!("{name} rename"),
                *name,
                text.replace("pub fn norm(", "pub fn norm2("),
            ));
        }
        // Prefix edits: an item appended (everything before is reusable),
        // one prepended (every id of the file shifts: nothing reusable), and
        // an appended item shadowing nothing but calling earlier ones.
        out.push((
            format!("{name} append"),
            *name,
            format!("{text}\nfn appended(q: i64) -> i64 effects() {{\n    return q;\n}}\n"),
        ));
        out.push((
            format!("{name} prepend"),
            *name,
            format!("fn prepended(q: i64) -> i64 effects() {{\n    return q;\n}}\n\n{text}"),
        ));
        out.push((
            format!("{name} append-bad"),
            *name,
            format!("{text}\nfn appended(q: i64) -> bool effects() {{\n    return q;\n}}\n"),
        ));
        if text.contains("pub y: i64") {
            out.push((
                format!("{name} field"),
                *name,
                text.replace("pub y: i64", "pub y: i32"),
            ));
        }
    }
    out
}

struct Loaded {
    program: Option<ravum_lang_sema::syntax::ast::Program>,
    sources: ravum_lang_sema::syntax::source::SourceMap,
    load_errors: bool,
}

fn load(
    root: &Path,
    files: &BTreeMap<&'static str, String>,
    cache: &mut ParseCache,
    ids: &mut BTreeMap<PathBuf, FileId>,
) -> Loaded {
    let src = std::fs::canonicalize(root.join("src")).unwrap();
    let overrides = files
        .iter()
        .map(|(n, t)| (src.join(n), t.clone()))
        .collect();
    let pkg = load_package_with_parse_cache(
        root,
        &LoadOptions {
            locked: false,
            overrides,
            file_ids: Some(ids.clone()),
        },
        cache,
    );
    for (p, id) in &pkg.file_ids {
        ids.insert(p.clone(), *id);
    }
    Loaded {
        load_errors: pkg.diags.has_errors(),
        program: pkg.program,
        sources: pkg.sources,
    }
}

/// Facts with binding identities replaced by their (span, name): the only
/// numbering that depends on check order.
fn norm_facts(f: &Facts) -> String {
    let mut f = f.clone();
    let by_id: BTreeMap<_, _> = f
        .own
        .bindings
        .iter()
        .map(|(k, v)| (*v, k.clone()))
        .collect();
    let named: BTreeMap<_, _> = f
        .own
        .binding_identity
        .iter()
        .map(|(n, id)| (*n, by_id.get(id).cloned()))
        .collect();
    f.own.binding_identity.clear();
    f.own.bindings.clear();
    f.reuse_bodies.clear();
    format!("{f:?} {named:?}")
}

fn norm_diags(r: &ravum_lang_sema::CheckResult) -> Vec<String> {
    let mut v: Vec<String> = r.diags.iter().map(|d| format!("{d:?}")).collect();
    v.sort();
    v
}

#[test]
fn package_check_equals_full_check_with_complete_facts() {
    let base_dir = std::env::temp_dir().join(format!("rv-incr-pkg-{}", std::process::id()));
    let (mut edits_n, mut reused, mut checked, mut facts_compared) = (0, 0, 0, 0);
    let mut reused_edited = 0;
    let mut failures = Vec::new();
    for seed in 0..3u32 {
        let root = base_dir.join(format!("p{seed}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("ravum.toml"),
            "[package]\nname = \"incrpkg\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
        )
        .unwrap();
        let base = package(seed);
        for (n, t) in &base {
            std::fs::write(root.join("src").join(n), t).unwrap();
        }
        let mut ids = BTreeMap::new();
        let mut cache = ParseCache::default();
        let b = load(&root, &base, &mut cache, &mut ids);
        assert!(!b.load_errors, "base package must load");
        let program = b.program.as_ref().unwrap();
        let (r0, s0, _) = check_package(None, program, &b.sources, &options());
        assert!(
            r0.checked.is_some(),
            "base must check: {:?}",
            norm_diags(&r0)
        );

        for (what, file, text) in edits(&base) {
            edits_n += 1;
            let mut files = base.clone();
            files.insert(file, text);
            // Warm: from the base state, with the session's parse cache.
            let w = load(&root, &files, &mut cache, &mut ids);
            let Some(wp) = w.program.as_ref() else {
                failures.push(format!("{what}: did not load"));
                continue;
            };
            let (warm, s1, stats) = check_package(Some(&s0), wp, &w.sources, &options());
            reused += stats.reused_bodies;
            reused_edited += stats.reused_in_edited_files;
            checked += stats.checked_bodies;
            // Cold: same file ids, fresh parse cache, full check.
            let mut cold_cache = ParseCache::default();
            let c = load(&root, &files, &mut cold_cache, &mut ids);
            let cold = check(c.program.as_ref().unwrap(), &c.sources, &options());
            if norm_diags(&warm) != norm_diags(&cold) {
                failures.push(format!(
                    "{what}: diagnostics differ\n  warm {:?}\n  cold {:?}",
                    norm_diags(&warm),
                    norm_diags(&cold)
                ));
                continue;
            }
            match (&warm.checked, &cold.checked) {
                (Some(a), Some(b)) => {
                    facts_compared += 1;
                    if norm_facts(&a.facts) != norm_facts(&b.facts) {
                        let fields = field_diff(&a.facts, &b.facts);
                        failures.push(format!("{what}: facts differ ({stats:?}) in {fields}"));
                        continue;
                    }
                }
                (None, None) => {}
                _ => {
                    failures.push(format!("{what}: one check succeeded, not the other"));
                    continue;
                }
            }
            // Back to the base from the warm state.
            let back = load(&root, &base, &mut cache, &mut ids);
            let (r2, _, _) = check_package(
                Some(&s1),
                back.program.as_ref().unwrap(),
                &back.sources,
                &options(),
            );
            if norm_diags(&r2) != norm_diags(&r0)
                || r2.checked.as_ref().map(|c| norm_facts(&c.facts))
                    != r0.checked.as_ref().map(|c| norm_facts(&c.facts))
            {
                failures.push(format!("{what}: revert differs from the base check"));
            }
        }
        let _ = std::fs::remove_dir_all(&root);
    }
    eprintln!(
        "T13 step 3 package cold=warm: edits={edits_n} facts compared={facts_compared} bodies reused={reused} (in edited files: {reused_edited}) checked={checked}"
    );
    assert!(
        failures.is_empty(),
        "{} unsound incremental package checks:\n{}",
        failures.len(),
        failures
            .iter()
            .take(6)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(edits_n >= 50, "edits: {edits_n}");
    assert!(facts_compared >= 20, "facts compared: {facts_compared}");
    assert!(reused > 0, "nothing reused");
    // T13: functions before the first change of an edited file are reused.
    assert!(reused_edited > 0, "nothing reused inside an edited file");
}

/// Names (and a sample) of the `Facts` fields that differ.
fn field_diff(a: &Facts, b: &Facts) -> String {
    let mut out = Vec::new();
    macro_rules! f {
        ($($x:ident).+) => {
            if format!("{:?}", a.$($x).+) != format!("{:?}", b.$($x).+) {
                let (sa, sb) = (format!("{:?}", a.$($x).+), format!("{:?}", b.$($x).+));
                let at = sa.chars().zip(sb.chars()).take_while(|(x, y)| x == y).count();
                out.push(format!("{} @{at}: warm `{}` cold `{}`", stringify!($($x).+),
                    sa.chars().skip(at.saturating_sub(40)).take(160).collect::<String>(),
                    sb.chars().skip(at.saturating_sub(40)).take(160).collect::<String>()));
            }
        };
    }
    f!(expr_types);
    f!(type_nodes);
    f!(pat_types);
    f!(resolutions);
    f!(calls);
    f!(methods);
    f!(fn_sigs);
    f!(effects);
    f!(direct_effects);
    f!(call_graph);
    f!(fn_used_imports);
    f!(static_str_fns);
    f!(own.use_kind);
    f!(own.place);
    f!(own.copy_type);
    f!(own.owned_position);
    f!(own.consumed);
    f!(own.element_reinit);
    f!(item_paths);
    out.join("; ")
}

/// The single-file case of `watch --backend jit`: FileId 0 (its first item's
/// node range starts at id 0), edits at the start, middle and end; the
/// incremental check equals a full check (diagnostics and facts), and the
/// bodies before the change are reused.
#[test]
fn single_file_with_file_id_zero_equals_a_full_check() {
    use ravum_lang_sema::syntax::parser::parse_file;
    use ravum_lang_sema::syntax::source::SourceMap;
    let base = "fn a(x: i64) -> i64 effects() {\n    return x + 1;\n}\n\nfn b(x: i64) -> i64 effects() {\n    return a(x) * 2;\n}\n\nfn c(x: i64) -> i64 effects() {\n    return b(x) - a(x);\n}\n\nfn check() -> i64 effects() {\n    return c(3);\n}\n";
    let run = |src: &str, prev| {
        let mut sources = SourceMap::new();
        let id = sources.add("src/lib.rv", src);
        assert_eq!(id, 0);
        let parsed = parse_file(id, src);
        let warm = check_package(prev, &parsed.program, &sources, &options());
        let cold = check(&parsed.program, &sources, &options());
        (warm, cold)
    };
    let ((r0, s0, _), _) = run(base, None);
    assert!(r0.checked.is_some());
    let mut reused = 0;
    for (what, from, to) in [
        ("start", "return x + 1;", "return x + 5;"),
        ("middle", "return a(x) * 2;", "return a(x) * 3;"),
        ("end", "return c(3);", "return c(4);"),
        ("error", "return c(3);", "return true;"),
    ] {
        let src = base.replace(from, to);
        let ((warm, _, st), cold) = run(&src, Some(&s0));
        reused += st.reused_bodies;
        assert_eq!(norm_diags(&warm), norm_diags(&cold), "{what}");
        assert_eq!(
            warm.checked.as_ref().map(|c| norm_facts(&c.facts)),
            cold.checked.as_ref().map(|c| norm_facts(&c.facts)),
            "{what}: facts"
        );
    }
    // `end`: a, b, c are before the change; `middle`: a.
    assert!(reused >= 4, "reused {reused}");
}
