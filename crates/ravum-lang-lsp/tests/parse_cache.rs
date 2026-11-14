//! Phase 7 — parse cache and stale-result safety in the language server.
//!
//! * `unchanged_file_is_not_reparsed`: a second analysis with the same cache
//!   reuses both parses; editing one file re-parses only that file.
//! * `stale_diagnostics_are_never_reused` (INV-07): a diagnostic computed for an
//!   old buffer is never republished for the new one.
//! * `analysis_latency_after_one_line_edit` (ignored, measurement): p50/p95 of a
//!   full re-analysis after a one-line edit, with and without the parse cache,
//!   on `examples/package-demo`. The measurement is in-process: JSON-RPC
//!   transport and editor costs are excluded (stated in the report).
//!
//! Run the measurement with:
//! `cargo test -p ravum-lang-lsp --test parse_cache -- --ignored --nocapture`

use ravum_lang_lsp::analysis::{override_key, Analysis};
use ravum_lang_sema::loader::ParseCache;
use ravum_lang_sema::syntax::span::FileId;
use ravum_lang_sema::syntax::NodeId;
use ravum_lang_sema::Profile;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// Directory with `ravum.toml` and `src/`, no source file yet.
    fn bare(label: &str) -> TempDir {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ravum-lsp-cache-{}-{unique}-{}-{label}",
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
            "[package]\nname = \"cache\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
        )
        .expect("cannot write the manifest");
        TempDir { path }
    }

    fn new(label: &str) -> TempDir {
        let directory = TempDir::bare(label);
        directory.write(
            "src/util.rv",
            "pub fn value() -> i32 { let a = 41; return a + 1; }\n",
        );
        directory.write(
            "src/main.rv",
            "mod util;\nfn main() { let x: bool = util::value(); }\n",
        );
        directory
    }

    /// Three modules plus `main`, with no error: the fixture behind the T5
    /// session-`FileId` tests.
    fn session(label: &str) -> TempDir {
        let directory = TempDir::bare(label);
        directory.write("src/a.rv", "pub fn fa() -> i32 { return 1; }\n");
        directory.write("src/b.rv", "pub fn fb() -> i32 { return 2; }\n");
        directory.write("src/c.rv", "pub fn fc() -> i32 { return 3; }\n");
        directory.write("src/main.rv", SESSION_MAIN);
        directory
    }

    fn write(&self, relative: &str, text: &str) {
        std::fs::write(self.path.join(relative), text).expect("cannot write the fixture");
    }

    fn read(&self, relative: &str) -> String {
        std::fs::read_to_string(self.path.join(relative)).expect("cannot read the fixture")
    }

    fn path_of(&self, relative: &str) -> PathBuf {
        std::fs::canonicalize(self.path.join(relative)).expect("cannot canonicalize the fixture")
    }

    fn main(&self) -> PathBuf {
        self.path_of("src/main.rv")
    }
}

/// `aa` is declared **before** the three original modules: with dense ids it
/// would take id 2 and renumber `a`, `b` and `c`.
const SESSION_MAIN: &str = "mod a;\nmod b;\nmod c;\n\
     fn main() { let sum: i32 = a::fa() + b::fb() + c::fc(); }\n";

const SESSION_MAIN_WITH_FOURTH: &str = "mod aa;\nmod a;\nmod b;\nmod c;\n\
     fn main() { let sum: i32 = aa::faa() + a::fa() + b::fb() + c::fc(); }\n";

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn run(root: &Path, main: &Path, text: &str, cache: &mut ParseCache) -> Analysis {
    let mut overrides = BTreeMap::new();
    overrides.insert(override_key(main), text.to_string());
    Analysis::run_with_cache(root, overrides, Profile::Core, cache)
}

fn type_errors(analysis: &Analysis) -> usize {
    analysis
        .diags
        .iter()
        .filter(|diag| diag.code == "E-TYP-MISMATCH")
        .count()
}

fn semantic_errors(analysis: &Analysis) -> Vec<String> {
    analysis
        .diags
        .iter()
        .filter(|diag| diag.code.starts_with("E-"))
        .map(|diag| diag.code.clone())
        .collect()
}

#[test]
fn unchanged_file_is_not_reparsed() {
    let package = TempDir::new("reuse");
    let main = package.main();
    let mut cache = ParseCache::new();
    let original = std::fs::read_to_string(&main).expect("cannot read main.rv");

    let first = run(&package.path, &main, &original, &mut cache);
    assert_eq!(
        type_errors(&first),
        1,
        "the initial program has one type error"
    );
    assert_eq!(
        cache.misses(),
        2,
        "two files must be parsed on the first run"
    );
    assert_eq!(cache.hits(), 0);

    let second = run(&package.path, &main, &original, &mut cache);
    assert_eq!(type_errors(&second), 1);
    assert_eq!(cache.hits(), 2, "the second run must reuse both parses");
    assert_eq!(cache.misses(), 2, "the second run must parse nothing");

    let edited = "mod util;\nfn main() { let x: i32 = util::value(); }\n";
    let third = run(&package.path, &main, edited, &mut cache);
    assert!(
        semantic_errors(&third).is_empty(),
        "the edited program has no error: {:?}",
        semantic_errors(&third)
    );
    assert_eq!(cache.misses(), 3, "only the edited file is parsed again");
    assert_eq!(cache.hits(), 3, "util.rv is still served from the cache");
}

#[test]
fn stale_diagnostics_are_never_reused_for_the_new_buffer() {
    let package = TempDir::new("stale");
    let main = package.main();
    let mut cache = ParseCache::new();
    let broken = std::fs::read_to_string(&main).expect("cannot read main.rv");
    assert!(
        broken.contains("let x: bool"),
        "the fixture must start with a type error"
    );

    let old = run(&package.path, &main, &broken, &mut cache);
    assert_eq!(type_errors(&old), 1);

    // New buffer, same cache: the old diagnostic must not be republished.
    let fixed = "mod util;\nfn main() { let x: i32 = util::value(); }\n";
    let new = run(&package.path, &main, fixed, &mut cache);
    assert!(
        semantic_errors(&new).is_empty(),
        "a diagnostic computed for an old buffer was republished: {:?}",
        semantic_errors(&new)
    );
    assert!(
        cache.hits() > 0,
        "the test must exercise a cache hit, otherwise it proves nothing about stale reuse"
    );
}

// --- T5: session file ids are stable across reloads ------------------------

fn item_ids_by_file(analysis: &Analysis) -> BTreeMap<u32, Vec<NodeId>> {
    let mut out: BTreeMap<u32, Vec<NodeId>> = BTreeMap::new();
    for (id, span) in &analysis.index.items {
        out.entry(span.file).or_default().push(*id);
    }
    for ids in out.values_mut() {
        ids.sort_unstable();
    }
    out
}

fn session_run(
    root: &Path,
    main: &Path,
    text: &str,
    cache: &mut ParseCache,
    file_ids: &mut BTreeMap<PathBuf, FileId>,
) -> Analysis {
    let mut overrides = BTreeMap::new();
    overrides.insert(override_key(main), text.to_string());
    Analysis::run_with_cache_and_file_ids(root, overrides, Profile::Core, cache, file_ids)
}

#[test]
fn session_file_ids_keep_untouched_files_stable() {
    let package = TempDir::session("session-ids");
    let main = package.main();
    let mut cache = ParseCache::new();
    let mut file_ids: BTreeMap<PathBuf, FileId> = BTreeMap::new();

    let first = session_run(
        &package.path,
        &main,
        &package.read("src/main.rv"),
        &mut cache,
        &mut file_ids,
    );
    assert!(
        first.package.program.is_some(),
        "the fixture must type-check: {:?}",
        semantic_errors(&first)
    );
    assert_eq!(cache.misses(), 4, "main + a + b + c are parsed once");
    assert_eq!(cache.hits(), 0);
    let before = item_ids_by_file(&first);
    let a = package.path_of("src/a.rv");
    let b = package.path_of("src/b.rv");
    let c = package.path_of("src/c.rv");
    let ids_before = (file_ids[&a], file_ids[&b], file_ids[&c]);
    let old_max = *file_ids.values().max().expect("the table is not empty");

    package.write("src/aa.rv", "pub fn faa() -> i32 { return 7; }\n");
    package.write("src/main.rv", SESSION_MAIN_WITH_FOURTH);
    let second = session_run(
        &package.path,
        &main,
        &package.read("src/main.rv"),
        &mut cache,
        &mut file_ids,
    );
    assert!(
        second.package.program.is_some(),
        "the fixture must type-check after the fourth file: {:?}",
        semantic_errors(&second)
    );

    assert_eq!(
        cache.hits(),
        3,
        "a, b and c keep their id, so their parse is reusable"
    );
    assert_eq!(
        cache.misses(),
        6,
        "only main and the new aa are parsed again"
    );
    assert_eq!(
        (file_ids[&a], file_ids[&b], file_ids[&c]),
        ids_before,
        "an untouched file must keep its session FileId"
    );
    let aa = package.path_of("src/aa.rv");
    assert!(
        file_ids[&aa] > old_max,
        "a new file takes the next id ({} <= {old_max})",
        file_ids[&aa]
    );
    let after = item_ids_by_file(&second);
    for (name, id) in [
        ("a", ids_before.0),
        ("b", ids_before.1),
        ("c", ids_before.2),
    ] {
        assert_eq!(
            after.get(&id),
            before.get(&id),
            "the NodeIds of `{name}` changed even though its FileId did not"
        );
    }
}

#[test]
fn without_a_session_table_a_new_module_renumbers_the_other_files() {
    let package = TempDir::session("session-negative");
    let main = package.main();
    let mut cache = ParseCache::new();

    let first = run(
        &package.path,
        &main,
        &package.read("src/main.rv"),
        &mut cache,
    );
    assert_eq!(cache.misses(), 4);
    let a = package.path_of("src/a.rv");
    let a_before = first.package.file_ids[&a];
    let before = item_ids_by_file(&first);

    package.write("src/aa.rv", "pub fn faa() -> i32 { return 7; }\n");
    package.write("src/main.rv", SESSION_MAIN_WITH_FOURTH);
    let second = run(
        &package.path,
        &main,
        &package.read("src/main.rv"),
        &mut cache,
    );

    assert_eq!(
        cache.hits(),
        0,
        "without a session table every file is renumbered and re-parsed"
    );
    assert_eq!(cache.misses(), 9, "five sources are parsed twice");
    let a_after = second.package.file_ids[&a];
    assert_ne!(
        a_before, a_after,
        "the fourth file declared first must shift `a` without a session table"
    );
    let after = item_ids_by_file(&second);
    assert_ne!(
        before.get(&a_before),
        after.get(&a_after),
        "the negative control must show renumbered NodeIds, otherwise the \
         positive test would be vacuous"
    );
}

fn percentile(mut values: Vec<f64>, percent: f64) -> f64 {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if values.is_empty() {
        return 0.0;
    }
    let rank = ((percent / 100.0) * values.len() as f64).ceil() as usize;
    values[rank.max(1).min(values.len()) - 1]
}

fn demo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("repository root")
        .join("examples/package-demo/app")
}

fn measure(iterations: usize, shared_cache: bool) -> Vec<f64> {
    let root = demo_root();
    let main = std::fs::canonicalize(root.join("src/main.rv")).expect("cannot canonicalize demo");
    let original = std::fs::read_to_string(&main).expect("cannot read the demo source");
    let mut cache = ParseCache::new();
    let mut times = Vec::with_capacity(iterations);
    for index in 0..iterations {
        let text = format!("{original}// latency edit {index}\n");
        let mut overrides = BTreeMap::new();
        overrides.insert(override_key(&main), text);
        let started = Instant::now();
        if shared_cache {
            let _ = Analysis::run_with_cache(&root, overrides, Profile::Core, &mut cache);
        } else {
            let mut fresh = ParseCache::new();
            let _ = Analysis::run_with_cache(&root, overrides, Profile::Core, &mut fresh);
        }
        times.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    times
}

#[test]
#[ignore = "measurement only: run with --ignored --nocapture"]
fn analysis_latency_after_one_line_edit() {
    let runs = 30;
    let without = measure(runs, false);
    let with = measure(runs, true);
    let p50_without = percentile(without.clone(), 50.0);
    let p95_without = percentile(without, 95.0);
    let p50_with = percentile(with.clone(), 50.0);
    let p95_with = percentile(with, 95.0);
    eprintln!(
        "[lsp-latency] package-demo app, {runs} one-line edits, in-process analysis (transport excluded)"
    );
    eprintln!("[lsp-latency] without parse cache: p50={p50_without:.3} ms p95={p95_without:.3} ms");
    eprintln!("[lsp-latency] with parse cache:    p50={p50_with:.3} ms p95={p95_with:.3} ms");
    eprintln!(
        "[lsp-latency] P0-A target p95 < 50 ms: {}",
        if p95_with < 50.0 {
            "ATTEINTE"
        } else {
            "NON ATTEINTE"
        }
    );
    assert!(
        p50_with > 0.0 && p95_with >= p50_with,
        "invalid measurement"
    );
}
