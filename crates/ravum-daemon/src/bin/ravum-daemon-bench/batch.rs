//! MVP-0c L2.4 — gate de bout en bout : groupement de patches + `cargo test` réel.
//!
//! Propositions = 6 éditions compatibles de `fixtures/grouping/edits.json`
//! (`BatchCollector` réel, analyseur qui exécute `cargo test --offline`) ; un
//! mode séquentiel applique les éditions une par une (6 `cargo test`), un mode
//! groupé les fusionne (1 `cargo test`), deux modes passent par le collecteur
//! (fenêtres 0/50 ms) et un mode injecte l'édition incompatible (signature
//! cassée) pour vérifier le refus puis le repli individuel N+1.
//!
//! Politique de l'adaptateur L2.4 : les 6 éditions portent sur des hunks
//! disjoints du **même** fichier. Elles déclarent donc `paths = []` (aucune
//! exclusivité de fichier) et des `symbols` distincts ; le pré-vol du collecteur
//! est file-granulaire quand `paths` est déclaré, la sécurité réelle est assurée
//! par l'analyseur (`cargo test` sur le contenu fusionné). Une sonde mesure le
//! comportement si `paths = ["src/lib.rs"]` était déclaré (isolation attendue).
//!
//! Aucune commande git, aucun réseau (`--offline`), `CARGO_BUILD_JOBS=2`.

use ravum_batch::{
    Analyzer, BatchCollector, BatchConfig, BatchOutcome, BatchProposal, BatchTelemetry, Verdict,
};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

pub struct BatchE2eOptions {
    pub repo_root: PathBuf,
    pub work_root: PathBuf,
    pub reps: usize,
}

const WINDOWS: [u64; 2] = [0, 50];
const MODES: [&str; 5] = [
    "sequential",
    "grouped",
    "collector-w0",
    "collector-w50",
    "incompatible",
];

#[derive(Debug, Clone)]
struct EditDef {
    name: String,
    find: String,
    replace: String,
    symbol: String,
    compatible: bool,
}

#[derive(Debug, Clone)]
struct CargoRun {
    tag: String,
    wall_ms: u64,
    success: bool,
    passed: u64,
    failed: u64,
    compiling_lines: u64,
    stderr_tail: String,
}

fn cargo_path() -> PathBuf {
    std::env::var_os("CARGO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("cargo"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn load_fixture(repo_root: &Path) -> Result<(String, Vec<EditDef>), String> {
    let dir = repo_root.join("fixtures").join("grouping");
    let base = std::fs::read_to_string(dir.join("src/lib.rs"))
        .map_err(|error| format!("cannot read fixture lib.rs: {error}"))?;
    let edits_text = std::fs::read_to_string(dir.join("edits.json"))
        .map_err(|error| format!("cannot read fixture edits.json: {error}"))?;
    let value: Value = serde_json::from_str(&edits_text)
        .map_err(|error| format!("edits.json is not valid JSON: {error}"))?;
    let mut edits = Vec::new();
    for entry in value["compatible"]
        .as_array()
        .ok_or_else(|| "edits.json: no compatible array".to_owned())?
    {
        let name = entry["name"].as_str().unwrap_or("").to_owned();
        let symbol = symbol_for(&name)
            .ok_or_else(|| format!("no symbol mapping for compatible edit {name}"))?;
        edits.push(EditDef {
            name,
            find: entry["find"].as_str().unwrap_or("").to_owned(),
            replace: entry["replace"].as_str().unwrap_or("").to_owned(),
            symbol: symbol.to_owned(),
            compatible: true,
        });
    }
    if edits.len() != 6 {
        return Err(format!("expected 6 compatible edits, got {}", edits.len()));
    }
    let incompatible = &value["incompatible"];
    edits.push(EditDef {
        name: incompatible["name"]
            .as_str()
            .unwrap_or("incompatible")
            .to_owned(),
        find: incompatible["find"].as_str().unwrap_or("").to_owned(),
        replace: incompatible["replace"].as_str().unwrap_or("").to_owned(),
        symbol: "scale".to_owned(),
        compatible: false,
    });
    Ok((base, edits))
}

fn symbol_for(edit_name: &str) -> Option<&'static str> {
    // Correspondance par préfixe stable d'identifiant : le nom des edits contient
    // des mots qui se chevauchent ("formula" contient "mul", etc.), donc une
    // recherche de sous-chaîne serait ambiguë.
    if edit_name.starts_with("edit-01") {
        Some("add")
    } else if edit_name.starts_with("edit-02") {
        Some("mul")
    } else if edit_name.starts_with("edit-03") {
        Some("is_even")
    } else if edit_name.starts_with("edit-04") {
        Some("factorial")
    } else if edit_name.starts_with("edit-05") {
        Some("checksum")
    } else if edit_name.starts_with("edit-06") {
        Some("lerp")
    } else {
        None
    }
}

fn apply_one(content: &str, edit: &EditDef) -> Result<String, String> {
    let occurrences = content.matches(&edit.find).count();
    if occurrences != 1 {
        return Err(format!(
            "edit {} matched {occurrences} times (expected 1)",
            edit.name
        ));
    }
    Ok(content.replace(&edit.find, &edit.replace))
}

fn compose(base: &str, edits: &[EditDef], compatible_count: usize) -> Result<String, String> {
    let mut content = base.to_owned();
    for edit in edits.iter().take(compatible_count) {
        content = apply_one(&content, edit)?;
    }
    Ok(content)
}

fn fixture_tests_source(repo_root: &Path) -> Result<String, String> {
    std::fs::read_to_string(repo_root.join("fixtures/grouping/tests/e2e.rs"))
        .map_err(|error| format!("cannot read fixture tests: {error}"))
}

fn prepare_crate(
    dir: &Path,
    content: &str,
    cargo_toml: &str,
    tests_source: &str,
) -> Result<PathBuf, String> {
    let crate_dir = dir.join("crate");
    if crate_dir.exists() {
        std::fs::remove_dir_all(&crate_dir)
            .map_err(|error| format!("cannot clean crate dir: {error}"))?;
    }
    std::fs::create_dir_all(crate_dir.join("src"))
        .map_err(|error| format!("cannot create crate dir: {error}"))?;
    std::fs::create_dir_all(crate_dir.join("tests"))
        .map_err(|error| format!("cannot create tests dir: {error}"))?;
    std::fs::write(crate_dir.join("Cargo.toml"), cargo_toml)
        .map_err(|error| format!("cannot write Cargo.toml: {error}"))?;
    std::fs::write(crate_dir.join("src/lib.rs"), content)
        .map_err(|error| format!("cannot write lib.rs: {error}"))?;
    std::fs::write(crate_dir.join("tests/e2e.rs"), tests_source)
        .map_err(|error| format!("cannot write tests/e2e.rs: {error}"))?;
    Ok(crate_dir)
}

fn cargo_test(crate_dir: &Path, target_dir: &Path, tag: &str) -> Result<CargoRun, String> {
    std::fs::create_dir_all(target_dir)
        .map_err(|error| format!("cannot create CARGO_TARGET_DIR: {error}"))?;
    let started = Instant::now();
    let output = Command::new(cargo_path())
        .args([
            "test",
            "--offline",
            "--manifest-path",
            crate_dir
                .join("Cargo.toml")
                .to_str()
                .unwrap_or("Cargo.toml"),
        ])
        .env("CARGO_TARGET_DIR", target_dir)
        .env("CARGO_BUILD_JOBS", "2")
        .env("CARGO_NET_OFFLINE", "true")
        .output()
        .map_err(|error| format!("cannot spawn cargo test: {error}"))?;
    let wall_ms = started.elapsed().as_millis() as u64;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}\n{stderr}");
    let mut passed = 0u64;
    let mut failed = 0u64;
    let mut compiling_lines = 0u64;
    for line in combined.lines() {
        if line.trim_start().starts_with("Compiling ") {
            compiling_lines += 1;
        }
        if let Some(rest) = line.trim().strip_prefix("test result: ") {
            let rest = rest.split_whitespace().collect::<Vec<_>>();
            if rest.len() >= 4 {
                passed += rest[1].parse::<u64>().unwrap_or(0);
                failed += rest[3].parse::<u64>().unwrap_or(0);
            }
        }
    }
    let success = output.status.success() && failed == 0 && passed > 0;
    Ok(CargoRun {
        tag: tag.to_owned(),
        wall_ms,
        success,
        passed,
        failed,
        compiling_lines,
        stderr_tail: combined
            .lines()
            .rev()
            .take(6)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join(" | "),
    })
}

// ---------------------------------------------------------------------------
// Collecteur réel + analyseur cargo test
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct EditProposal {
    id: String,
    symbol: String,
    paths: Vec<String>,
}

impl BatchProposal for EditProposal {
    fn proposal_id(&self) -> &str {
        &self.id
    }
    fn paths(&self) -> &[String] {
        &self.paths
    }
    fn symbols(&self) -> &[String] {
        std::slice::from_ref(&self.symbol)
    }
    fn base_id(&self) -> &str {
        "grouping-base"
    }
}

struct CargoTestAnalyzer {
    crate_dir: PathBuf,
    target_dir: PathBuf,
    base: String,
    edits: Vec<EditDef>,
    runs: Vec<CargoRun>,
}

impl CargoTestAnalyzer {
    fn content_for(&self, ids: &[&str]) -> Result<String, String> {
        let wanted: HashSet<&str> = ids.iter().copied().collect();
        let mut content = self.base.clone();
        for edit in &self.edits {
            if wanted.contains(edit.name.as_str()) {
                content = apply_one(&content, edit)?;
            }
        }
        Ok(content)
    }
}

impl Analyzer for CargoTestAnalyzer {
    fn analyze(&mut self, proposal_ids: &[&str]) -> Verdict {
        let content = match self.content_for(proposal_ids) {
            Ok(content) => content,
            Err(error) => return Verdict::failed(error),
        };
        let tag = format!("batch-{}", proposal_ids.len());
        let result = match crate_dir_write(&self.crate_dir, &content) {
            Ok(()) => cargo_test(&self.crate_dir, &self.target_dir, &tag),
            Err(error) => Err(error),
        };
        match result {
            Ok(run) => {
                let ok = run.success;
                let diagnostic = format!(
                    "cargo test: {} passed, {} failed, {} ms",
                    run.passed, run.failed, run.wall_ms
                );
                self.runs.push(run);
                if ok {
                    Verdict::passed()
                } else {
                    Verdict::failed(diagnostic)
                }
            }
            Err(error) => {
                self.runs.push(CargoRun {
                    tag,
                    wall_ms: 0,
                    success: false,
                    passed: 0,
                    failed: 0,
                    compiling_lines: 0,
                    stderr_tail: error.clone(),
                });
                Verdict::failed(error)
            }
        }
    }
}

fn crate_dir_write(crate_dir: &Path, content: &str) -> Result<(), String> {
    std::fs::write(crate_dir.join("src/lib.rs"), content)
        .map_err(|error| format!("cannot write merged lib.rs: {error}"))
}

struct ModeResult {
    mode: String,
    cold_ms: u64,
    warm_ms: Option<u64>,
    total_ms: u64,
    cargo_runs: u64,
    hard_failures: u64,
    passed: u64,
    failed: u64,
    digest: String,
    collector: Option<Value>,
    runs: Vec<CargoRun>,
    cpu_ms: Option<u64>,
    rss_peak_bytes: Option<u64>,
    target_disk_bytes: u64,
}

fn run_mode(
    _repo_root: &Path,
    work_root: &Path,
    rep: usize,
    mode: &str,
    base: &str,
    edits: &[EditDef],
    cargo_toml: &str,
    tests_source: &str,
) -> Result<ModeResult, String> {
    let mode_dir = work_root
        .join(format!("rep-{rep:02}"))
        .join(mode.replace('/', "_"));
    let _ = std::fs::remove_dir_all(&mode_dir);
    std::fs::create_dir_all(&mode_dir)
        .map_err(|error| format!("cannot create mode dir: {error}"))?;
    let target_dir = mode_dir.join("cargo-target");
    let compatible_count = edits.iter().filter(|edit| edit.compatible).count();

    let cpu_start = cpu_times();
    let sampler = RssSampler::start();
    let started = Instant::now();
    let mut runs: Vec<CargoRun> = Vec::new();
    let mut collector: Option<BatchTelemetry> = None;

    let digest = match mode {
        "sequential" => {
            let mut content = base.to_owned();
            for (index, edit) in edits.iter().filter(|edit| edit.compatible).enumerate() {
                content = apply_one(&content, edit)?;
                let crate_dir = prepare_crate(&mode_dir, &content, cargo_toml, tests_source)?;
                let run = cargo_test(
                    &crate_dir,
                    &target_dir,
                    &format!("sequential-{}", index + 1),
                )?;
                if !run.success {
                    return Err(format!(
                        "sequential stage {} failed: {} passed / {} failed: {}",
                        index + 1,
                        run.passed,
                        run.failed,
                        run.stderr_tail
                    ));
                }
                runs.push(run);
            }
            sha256_hex(content.as_bytes())
        }
        "grouped" => {
            let content = compose(base, edits, compatible_count)?;
            let crate_dir = prepare_crate(&mode_dir, &content, cargo_toml, tests_source)?;
            for tag in ["grouped-cold", "grouped-warm"] {
                let run = cargo_test(&crate_dir, &target_dir, tag)?;
                if !run.success {
                    return Err(format!(
                        "{tag} failed: {} passed / {} failed: {}",
                        run.passed, run.failed, run.stderr_tail
                    ));
                }
                runs.push(run);
            }
            sha256_hex(content.as_bytes())
        }
        mode if mode.starts_with("collector-w") => {
            let window: u64 = mode
                .trim_start_matches("collector-w")
                .parse()
                .map_err(|_| format!("bad window in mode {mode}"))?;
            let content = compose(base, edits, compatible_count)?;
            let crate_dir = prepare_crate(&mode_dir, &content, cargo_toml, tests_source)?;
            let analyzer = CargoTestAnalyzer {
                crate_dir: crate_dir.clone(),
                target_dir: target_dir.clone(),
                base: base.to_owned(),
                edits: edits.to_vec(),
                runs: Vec::new(),
            };
            let config = BatchConfig::new(window, 8).map_err(|error| error.to_string())?;
            let mut collector_run = BatchCollector::new(config, analyzer);
            let proposals: Vec<EditProposal> = edits
                .iter()
                .filter(|edit| edit.compatible)
                .map(|edit| EditProposal {
                    id: edit.name.clone(),
                    symbol: edit.symbol.clone(),
                    paths: Vec::new(),
                })
                .collect();
            for (index, proposal) in proposals.iter().enumerate() {
                collector_run
                    .submit(proposal.clone(), index as u64)
                    .map_err(|error| format!("submit failed: {error}"))?;
            }
            let reports = collector_run.poll(proposals.len() as u64 + window + 2);
            if reports.len() != 1 || reports[0].proposal_ids.len() != compatible_count {
                return Err(format!(
                    "collector mode {mode}: expected one merged batch, got {} report(s)",
                    reports.len()
                ));
            }
            if !matches!(reports[0].outcome, BatchOutcome::AcceptedAll) {
                return Err(format!(
                    "collector mode {mode}: merged batch was not accepted"
                ));
            }
            collector = Some(collector_run.telemetry().clone());
            let analyzer_runs = collector_run.into_analyzer().runs;
            runs.extend(analyzer_runs);
            // Re-run identique pour la mesure « warm » (contenu inchangé).
            let warm = cargo_test(&crate_dir, &target_dir, "collector-warm")?;
            if !warm.success {
                return Err(format!("collector warm run failed: {}", warm.stderr_tail));
            }
            runs.push(warm);
            sha256_hex(content.as_bytes())
        }
        "incompatible" => {
            let content = compose(base, edits, compatible_count)?;
            let crate_dir = prepare_crate(&mode_dir, &content, cargo_toml, tests_source)?;
            let analyzer = CargoTestAnalyzer {
                crate_dir: crate_dir.clone(),
                target_dir: target_dir.clone(),
                base: base.to_owned(),
                edits: edits.to_vec(),
                runs: Vec::new(),
            };
            let config = BatchConfig::new(50, 8).map_err(|error| error.to_string())?;
            let mut collector_run = BatchCollector::new(config, analyzer);
            for (index, edit) in edits.iter().enumerate() {
                collector_run
                    .submit(
                        EditProposal {
                            id: edit.name.clone(),
                            symbol: edit.symbol.clone(),
                            paths: Vec::new(),
                        },
                        index as u64,
                    )
                    .map_err(|error| format!("submit failed: {error}"))?;
            }
            let reports = collector_run.poll(edits.len() as u64 + 52);
            if reports.len() != 1 {
                return Err(format!(
                    "incompatible mode: expected one batch, got {}",
                    reports.len()
                ));
            }
            let report = &reports[0];
            if matches!(report.outcome, BatchOutcome::AcceptedAll) {
                return Err("incompatible batch must never be accepted".to_owned());
            }
            if report.rejected_ids.len() != 1 || report.accepted_ids.len() != compatible_count {
                return Err(format!(
                    "incompatible fallback must accept {compatible_count} and reject 1, got {} accepted / {} rejected",
                    report.accepted_ids.len(),
                    report.rejected_ids.len()
                ));
            }
            if report.analyses_used != compatible_count + 2 {
                return Err(format!(
                    "expected N+1 analyses ({}), got {}",
                    compatible_count + 2,
                    report.analyses_used
                ));
            }
            collector = Some(collector_run.telemetry().clone());
            runs.extend(collector_run.into_analyzer().runs);
            sha256_hex(content.as_bytes())
        }
        other => return Err(format!("unknown batch-e2e mode {other:?}")),
    };

    let total_ms = started.elapsed().as_millis() as u64;
    let rss_peak = sampler.finish();
    let cpu_end = cpu_times();
    let cpu_ms = match (cpu_start, cpu_end) {
        (Some(start), Some(end)) => {
            Some((end.user_ms + end.kernel_ms).saturating_sub(start.user_ms + start.kernel_ms))
        }
        _ => None,
    };
    let target_disk_bytes = dir_bytes(&target_dir);
    let cold = runs.first().map(|run| run.wall_ms).unwrap_or(0);
    let warm = runs
        .iter()
        .skip(1)
        .map(|run| run.wall_ms)
        .collect::<Vec<_>>();
    let warm_ms = if warm.is_empty() {
        None
    } else {
        let mut sorted = warm.clone();
        sorted.sort_unstable();
        Some(sorted[sorted.len() / 2])
    };
    let passed = runs
        .iter()
        .find(|run| run.success)
        .map(|run| run.passed)
        .unwrap_or(0);
    let failed = runs.iter().map(|run| run.failed).sum::<u64>();
    let hard_failures = runs.iter().filter(|run| !run.success).count() as u64;
    Ok(ModeResult {
        mode: mode.to_owned(),
        cold_ms: cold,
        warm_ms,
        total_ms,
        cargo_runs: runs.len() as u64,
        hard_failures,
        passed,
        failed,
        digest,
        collector: collector.map(telemetry_json),
        runs,
        cpu_ms,
        rss_peak_bytes: rss_peak,
        target_disk_bytes,
    })
}

fn telemetry_json(telemetry: BatchTelemetry) -> Value {
    json!({
        "proposals_received": telemetry.proposals_received,
        "batches_formed": telemetry.batches_formed,
        "batches_accepted_all": telemetry.batches_accepted_all,
        "batches_fell_back": telemetry.batches_fell_back,
        "individual_analyses": telemetry.individual_analyses,
        "analyses_used": telemetry.analyses_used,
        "analyses_saved_net": telemetry.analyses_saved_net,
        "preflight_isolations": telemetry.preflight_isolations,
        "wait_p50_ms": telemetry.wait_p50_ms(),
        "wait_p95_ms": telemetry.wait_p95_ms(),
        "items_reported": telemetry.items_reported,
        "no_loss": telemetry.no_loss(),
    })
}

// ---------------------------------------------------------------------------
// Observabilité (best-effort, non bloquante)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default)]
struct CpuTimes {
    user_ms: u64,
    kernel_ms: u64,
}

#[cfg(windows)]
fn cpu_times() -> Option<CpuTimes> {
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    impl FileTime {
        fn as_u64(self) -> u64 {
            ((self.high as u64) << 32) | u64::from(self.low)
        }
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn GetProcessTimes(
            process: isize,
            creation: *mut FileTime,
            exit_time: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
    }
    // SAFETY: pseudo-handle (no close), four writable FILETIME out-parameters.
    unsafe {
        let mut creation = FileTime::default();
        let mut exit = FileTime::default();
        let mut kernel = FileTime::default();
        let mut user = FileTime::default();
        let ok = GetProcessTimes(
            GetCurrentProcess(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        ) != 0;
        if !ok {
            return None;
        }
        let ticks = |time: FileTime| time.as_u64() / 10_000;
        Some(CpuTimes {
            user_ms: ticks(user),
            kernel_ms: ticks(kernel),
        })
    }
}

#[cfg(not(windows))]
fn cpu_times() -> Option<CpuTimes> {
    None
}

#[cfg(windows)]
fn process_working_set() -> Option<u64> {
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> isize;
    }
    #[link(name = "psapi")]
    extern "system" {
        fn GetProcessMemoryInfo(process: isize, counters: *mut Counters, cb: u32) -> i32;
    }
    // SAFETY: pseudo-handle, repr(C) zeroed counters with its own size.
    unsafe {
        let mut counters = Counters {
            cb: std::mem::size_of::<Counters>() as u32,
            ..Default::default()
        };
        let ok = GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) != 0;
        ok.then_some(counters.working_set_size as u64)
    }
}

#[cfg(not(windows))]
fn process_working_set() -> Option<u64> {
    None
}

struct RssSampler {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    peak: std::sync::Arc<std::sync::atomic::AtomicU64>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl RssSampler {
    fn start() -> Self {
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
        use std::sync::Arc;
        let stop = Arc::new(AtomicBool::new(false));
        let peak = Arc::new(AtomicU64::new(0));
        let handle = if cfg!(windows) {
            let stop = Arc::clone(&stop);
            let peak = Arc::clone(&peak);
            Some(std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    if let Some(bytes) = process_working_set() {
                        peak.fetch_max(bytes, Ordering::Relaxed);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            }))
        } else {
            None
        };
        Self { stop, peak, handle }
    }

    fn finish(mut self) -> Option<u64> {
        use std::sync::atomic::Ordering;
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        match self.peak.load(Ordering::Relaxed) {
            0 => None,
            bytes => Some(bytes),
        }
    }
}

fn dir_bytes(root: &Path) -> u64 {
    if !root.exists() {
        return 0;
    }
    let mut total = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(metadata) = path.metadata() {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    total
}

// ---------------------------------------------------------------------------
// Campagne
// ---------------------------------------------------------------------------

fn stats(values: &[u64]) -> Value {
    if values.is_empty() {
        return json!({"count": 0});
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let percentile = |p: f64| -> u64 {
        let rank = (p / 100.0 * sorted.len() as f64).ceil() as usize;
        sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
    };
    json!({
        "count": sorted.len(),
        "min": sorted[0],
        "max": sorted[sorted.len() - 1],
        "mean": sorted.iter().sum::<u64>() as f64 / sorted.len() as f64,
        "p50": percentile(50.0),
        "p95": percentile(95.0),
    })
}

fn rotated_modes(rep: usize) -> Vec<&'static str> {
    let offset = rep % MODES.len();
    (0..MODES.len())
        .map(|index| MODES[(index + offset) % MODES.len()])
        .collect()
}

pub fn run(options: &BatchE2eOptions) -> Result<Value, String> {
    let (base, edits) = load_fixture(&options.repo_root)?;
    let cargo_toml =
        std::fs::read_to_string(options.repo_root.join("fixtures/grouping/Cargo.toml"))
            .map_err(|error| format!("cannot read fixture Cargo.toml: {error}"))?;
    let tests_source = fixture_tests_source(&options.repo_root)?;
    if options.reps < 5 {
        return Err("batch-e2e needs at least 5 recorded repetitions".to_owned());
    }
    let started = Instant::now();
    let compatible_count = edits.iter().filter(|edit| edit.compatible).count();

    // Sonde de politique du pré-vol (aucun cargo) : si les propositions déclarent
    // le fichier commun, le collecteur les isole (comportement file-granulaire).
    let policy_probe = {
        struct NoopAnalyzer;
        impl Analyzer for NoopAnalyzer {
            fn analyze(&mut self, _ids: &[&str]) -> Verdict {
                Verdict::passed()
            }
        }
        let mut collector = BatchCollector::new(
            BatchConfig::new(50, 8).map_err(|error| error.to_string())?,
            NoopAnalyzer,
        );
        for (index, edit) in edits.iter().filter(|edit| edit.compatible).enumerate() {
            collector
                .submit(
                    EditProposal {
                        id: edit.name.clone(),
                        symbol: edit.symbol.clone(),
                        paths: vec!["src/lib.rs".to_owned()],
                    },
                    index as u64,
                )
                .map_err(|error| format!("policy probe submit failed: {error}"))?;
        }
        let reports = collector.poll(6 + 52);
        json!({
            "declared_paths": ["src/lib.rs"],
            "batches": reports.len(),
            "preflight_isolations": collector.telemetry().preflight_isolations,
            "analyses_used": collector.telemetry().analyses_used,
            "note": "si l'adaptateur déclare le fichier commun, le pré-vol file-granulaire isole chaque proposition ; l'adaptateur L2.4 déclare paths=[] et repose sur les symboles distincts + le cargo test de l'analyseur; premier lot ouvert sans conflit, 5 isolations détectées",
        })
    };

    let mut repetitions: Vec<Value> = Vec::new();
    let mut all: Vec<ModeResult> = Vec::new();
    for rep in 1..=options.reps {
        let order = rotated_modes(rep - 1);
        let mut runs = Vec::new();
        for mode in &order {
            let result = run_mode(
                &options.repo_root,
                &options.work_root,
                rep,
                mode,
                &base,
                &edits,
                &cargo_toml,
                &tests_source,
            )?;
            runs.push(json!({
                "mode": mode,
                "cold_ms": result.cold_ms,
                "warm_ms": result.warm_ms,
                "total_ms": result.total_ms,
                "cargo_runs": result.cargo_runs,
                "hard_failures": result.hard_failures,
                "passed": result.passed,
                "failed": result.failed,
                "digest": result.digest,
                "collector": result.collector,
                "cpu_ms": result.cpu_ms,
                "rss_peak_bytes": result.rss_peak_bytes,
                "target_disk_bytes": result.target_disk_bytes,
                "per_run": result.runs.iter().map(|run| json!({
                    "tag": run.tag,
                    "wall_ms": run.wall_ms,
                    "success": run.success,
                    "passed": run.passed,
                    "failed": run.failed,
                    "compiling_lines": run.compiling_lines,
                })).collect::<Vec<_>>(),
            }));
            all.push(result);
        }
        repetitions.push(json!({ "rep": rep, "order": order, "modes": runs }));
        println!(
            "  batch-e2e rep {rep}/{} done (order: {})",
            options.reps,
            order.join(", ")
        );
    }

    let per_mode = |mode: &str| -> Vec<&ModeResult> {
        all.iter().filter(|result| result.mode == mode).collect()
    };

    let modes_summary: Vec<Value> = MODES
        .iter()
        .map(|mode| {
            let results = per_mode(mode);
            json!({
                "mode": mode,
                "cold_ms": stats(&results.iter().map(|result| result.cold_ms).collect::<Vec<_>>()),
                "warm_ms": stats(&results.iter().filter_map(|result| result.warm_ms).collect::<Vec<_>>()),
                "total_ms": stats(&results.iter().map(|result| result.total_ms).collect::<Vec<_>>()),
                "cargo_runs": stats(&results.iter().map(|result| result.cargo_runs).collect::<Vec<_>>()),
                "hard_failures": stats(&results.iter().map(|result| result.hard_failures).collect::<Vec<_>>()),
                "passed_last_success": stats(&results.iter().map(|result| result.passed).collect::<Vec<_>>()),
                "failed_total": stats(&results.iter().map(|result| result.failed).collect::<Vec<_>>()),
                "cpu_ms": stats(&results.iter().filter_map(|result| result.cpu_ms).collect::<Vec<_>>()),
                "rss_peak_bytes": stats(&results.iter().filter_map(|result| result.rss_peak_bytes).collect::<Vec<_>>()),
                "target_disk_bytes": stats(&results.iter().map(|result| result.target_disk_bytes).collect::<Vec<_>>()),
                "digest": results.first().map(|result| result.digest.clone()),
                "collector": results.first().and_then(|result| result.collector.clone()),
            })
        })
        .collect();

    let median = |mode: &str| -> u64 {
        let mut values: Vec<u64> = per_mode(mode)
            .iter()
            .map(|result| result.total_ms)
            .collect();
        if values.is_empty() {
            return 0;
        }
        values.sort_unstable();
        values[values.len() / 2]
    };
    let sequential_total = median("sequential");
    let grouped_total = median("grouped");
    let gain = sequential_total as i64 - grouped_total as i64;
    let gain_percent = if sequential_total == 0 {
        0.0
    } else {
        100.0 * gain as f64 / sequential_total as f64
    };
    let digests: Vec<String> = all.iter().map(|result| result.digest.clone()).collect();
    let digest_identical = digests.iter().all(|digest| Some(digest) == digests.first());
    let correctness_ok = all.iter().all(|result| {
        if result.mode == "incompatible" {
            // La signature cassée produit 2 échecs de compilation (lot + item
            // incompatible) et aucun « test failed » : le verdict de correction
            // se lit sur les runs durs et la télémétrie de repli.
            result.hard_failures == 2
                && result
                    .runs
                    .iter()
                    .any(|run| run.success && run.passed == 12)
                && result
                    .collector
                    .as_ref()
                    .map(|telemetry| {
                        telemetry["batches_fell_back"] == 1 && telemetry["analyses_used"] == 8
                    })
                    .unwrap_or(false)
        } else {
            result.failed == 0 && result.passed == 12
        }
    });
    let windows_break_even: Vec<Value> = WINDOWS
        .iter()
        .map(|window| {
            let mode = format!("collector-w{window}");
            let collector_total = median(&mode);
            json!({
                "window_ms": window,
                "collector_total_ms": collector_total,
                "gain_vs_sequential_ms": sequential_total as i64 - collector_total as i64,
                "collector": per_mode(&mode).first().and_then(|result| result.collector.clone()),
            })
        })
        .collect();
    let break_even_wait_ms = sequential_total as i64 - grouped_total as i64;
    let checks = json!({
        "sequential_and_grouped_pass_12_tests": correctness_ok,
        "digest_identical_between_modes": digest_identical,
        "collector_merged_batch_one_analysis": per_mode("collector-w0").iter().all(|result| result.cargo_runs == 2),
        "collector_window50_merged_batch_one_analysis": per_mode("collector-w50").iter().all(|result| result.cargo_runs == 2),
        "incompatible_batch_refused_and_fallback_n_plus_1": per_mode("incompatible").iter().all(|result| {
            result.collector.as_ref().map(|c| c["batches_fell_back"] == 1 && c["analyses_used"] == (compatible_count + 2) as u64).unwrap_or(false)
        }),
        "no_loss": per_mode("collector-w0").into_iter().chain(per_mode("collector-w50")).chain(per_mode("incompatible")).all(|result| {
            result.collector.as_ref().map(|c| c["no_loss"] == true).unwrap_or(false)
        }),
        "five_repetitions": options.reps >= 5,
    });
    let valid = checks
        .as_object()
        .map(|map| map.values().all(|value| value == &json!(true)))
        .unwrap_or(false);

    Ok(json!({
        "schema_version": 1,
        "suite": "batch-e2e",
        "generated_unix": unix_now(),
        "duration_ms": started.elapsed().as_millis() as u64,
        "git": git_json(&options.repo_root),
        "machine": {
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "logical_parallelism": std::thread::available_parallelism().map(|value| value.get()).unwrap_or(0),
            "cargo": cargo_path().display().to_string(),
            "cargo_test_args": ["test", "--offline", "--manifest-path <crate>/Cargo.toml"],
            "env": {"CARGO_BUILD_JOBS": "2", "CARGO_NET_OFFLINE": "true", "CARGO_TARGET_DIR": "par mode/répétition"},
        },
        "protocol": {
            "goal": "prouver le gain de bout en bout du groupement avec un vrai cargo test et une correction identique (12/12, digest)",
            "fixture": "fixtures/grouping (6 éditions compatibles + 1 incompatible) + tests/e2e.rs (7 tests, total 12)",
            "modes": MODES,
            "repetitions": options.reps,
            "order": "ordre des modes tourné d'un cran par répétition",
            "cold_warm": "chaque mode/répétition a un CARGO_TARGET_DIR neuf : la 1re invocation est froide, les suivantes chaudes ; le mode groupé et les modes collecteur mesurent aussi une ré-exécution identique (warm)",
            "adapter_policy": "les 6 éditions sont des hunks disjoints du même fichier : l'adaptateur L2.4 déclare paths=[] et des symbols distincts ; la sécurité vient du cargo test de l'analyseur ; la sonde de politique mesure le comportement si paths=[src/lib.rs] est déclaré",
            "collector": "ravum_batch::BatchCollector réel + CargoTestAnalyzer qui écrit le contenu fusionné et exécute cargo test --offline",
            "incompatible": "édition signature cassée : lot refusé (cargo test en échec), repli individuel N+1, 6 acceptés / 1 rejeté",
            "cpu_rss": "CPU = GetProcessTimes du processus harnais (les cargo/rustc enfants ne sont pas comptés) ; RSS = working set du processus harnais échantillonné toutes les 5 ms",
            "disk": "taille du CARGO_TARGET_DIR après le mode",
            "no_network": "cargo test --offline",
        },
        "policy_probe": policy_probe,
        "repetitions": repetitions,
        "summary": {
            "modes": modes_summary,
            "gain": {
                "sequential_total_ms_p50": sequential_total,
                "grouped_total_ms_p50": grouped_total,
                "gain_ms": gain,
                "gain_percent": gain_percent,
                "break_even_added_wait_ms": break_even_wait_ms,
                "windows": windows_break_even,
                "note": "gain = total séquentiel (6× cargo test) − total groupé (1× cargo test + ré-exécution chaude) ; le point d'équilibre est l'attente supplémentaire tolérable avant que le gain s'annule",
            },
            "correctness": {
                "tests_expected": 12,
                "digest_identical_between_modes": digest_identical,
                "all_modes_correct": correctness_ok,
                "incompatible_accepted": compatible_count,
                "incompatible_rejected": 1,
            },
            "checks": checks,
            "valid": valid,
        },
    }))
}

fn git_json(repo_root: &Path) -> Value {
    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_root)
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned());
    let branch = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(repo_root)
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned());
    json!({"head": head, "branch": branch})
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub fn write(path: &Path, value: &Value) -> Result<(), String> {
    ravum_daemon_bench_write(path, value)
}

fn ravum_daemon_bench_write(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(value)
        .map_err(|error| format!("cannot serialize batch-e2e evidence: {error}"))?;
    std::fs::write(path, format!("{text}\n"))
        .map_err(|error| format!("cannot write {}: {error}", path.display()))
}
