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
use ravum_lang_sema::Profile;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> TempDir {
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
        std::fs::write(
            path.join("src/util.rv"),
            "pub fn value() -> i32 { let a = 41; return a + 1; }\n",
        )
        .expect("cannot write util.rv");
        std::fs::write(
            path.join("src/main.rv"),
            "mod util;\nfn main() { let x: bool = util::value(); }\n",
        )
        .expect("cannot write main.rv");
        TempDir { path }
    }

    fn main(&self) -> PathBuf {
        std::fs::canonicalize(self.path.join("src/main.rv")).expect("cannot canonicalize main.rv")
    }
}

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
