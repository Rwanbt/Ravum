//! MVP-0c L2 (phase mesure) : opportunité du **groupement de patches**.
//!
//! Question mesurée : N éditions compatibles doivent-elles être analysées une
//! par une (N cycles complets) ou regroupées dans un seul snapshot cohérent
//! (1 analyse) ? Et quel est le coût de la fenêtre d'attente de collecte ?
//!
//! Ce module est un **harnais de mesure**, pas une implémentation du groupement
//! dans le cœur du daemon : la politique de groupement est simulée en composant
//! le contenu (`N` éditions appliquées aux octets sources) avant un unique appel
//! `Daemon::analyze`. Le daemon réel, ses snapshots adressés par contenu, son
//! cache et son `rustc --emit=metadata` sont utilisés tels quels.
//!
//! Modes mesurés, dans un ordre alterné à chaque répétition :
//!
//! * `sequential` — N cycles : édition i appliquée cumulativement → analyse →
//!   vérification (préconditions de contenu + identité du snapshot) ;
//! * `grouped-wXX` — fenêtre d'attente simulée `XX` ms (0/25/50/100), puis N
//!   éditions composées → 1 analyse → vérification des N préconditions ;
//! * `incompatible` — le lot contient l'édition incompatible : l'analyse doit
//!   être refusée (`rustc` en erreur), puis reprise individuelle pour isoler la
//!   faute, puis re-lot des seules éditions compatibles, accepté.
//!
//! Mesures : temps total et par phase (composition, analyse, vérification,
//! attente), invocations `rustc` (delta `daemon.stats()`), CPU processus,
//! RSS pic échantillonné, disque de l'état, latence de retour d'erreur, taux de
//! succès correct. Chaque mode/repetition utilise un répertoire d'état neuf :
//! aucun contenu n'est servi par un cache d'une autre variante (les hits/miss
//! sont vérifiés, pas supposés).
//!
//! Limites assumées : l'analyse daemon est `--emit=metadata` (pas de codegen ni
//! de lien : « build » signifie ici compilation de métadonnées) ; la
//! vérification est une postcondition sur les octets soumis + l'identité du
//! snapshot, pas une preuve sémantique ; le CPU processus ne compte pas les
//! `rustc` enfants (leur temps propre est `timings.analysis_ms`) ; la fenêtre
//! d'attente est un `sleep` simulé, pas une collecte multi-agents réelle.

use crate::harness;
use ravum_daemon::{BuildContext, Daemon, SourceFile};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Fenêtres d'attente de collecte mesurées, en millisecondes.
pub const WINDOWS_MS: [u64; 4] = [0, 25, 50, 100];

/// Nombre minimal de répétitions enregistrées (mandat : ≥ 5).
const MIN_REPS: usize = 5;

pub struct GroupingOptions {
    pub repo_root: PathBuf,
    pub work_root: PathBuf,
    pub reps: usize,
}

struct GroupingFixture {
    base: String,
    compatible: Vec<Edit>,
    incompatible: Edit,
    expected_incompatible_code: String,
}

#[derive(Debug, Clone)]
struct Edit {
    name: String,
    find: String,
    replace: String,
    expect_marker: String,
}

impl GroupingFixture {
    fn line_count(&self) -> usize {
        self.base.lines().count()
    }

    fn compatible_count(&self) -> usize {
        self.compatible.len()
    }
}

fn load_fixture(repo_root: &Path) -> Result<GroupingFixture, String> {
    let dir = repo_root.join("fixtures").join("grouping");
    let source = dir.join("src").join("lib.rs");
    let base = std::fs::read_to_string(&source)
        .map_err(|error| format!("cannot read fixture {}: {error}", source.display()))?;
    let edits_path = dir.join("edits.json");
    let text = std::fs::read_to_string(&edits_path)
        .map_err(|error| format!("cannot read {}: {error}", edits_path.display()))?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|error| format!("{} is not valid JSON: {error}", edits_path.display()))?;
    let compatible = value["compatible"]
        .as_array()
        .ok_or_else(|| format!("{}: missing compatible edits", edits_path.display()))?
        .iter()
        .map(|entry| Edit {
            name: entry["name"].as_str().unwrap_or("?").to_owned(),
            find: entry["find"].as_str().unwrap_or("").to_owned(),
            replace: entry["replace"].as_str().unwrap_or("").to_owned(),
            expect_marker: entry["expect_marker"].as_str().unwrap_or("").to_owned(),
        })
        .collect::<Vec<_>>();
    if compatible.len() < 4 || compatible.len() > 8 {
        return Err(format!(
            "fixture must define 4..=8 compatible edits, got {}",
            compatible.len()
        ));
    }
    let incompatible_entry = &value["incompatible"];
    let incompatible = Edit {
        name: incompatible_entry["name"]
            .as_str()
            .unwrap_or("?")
            .to_owned(),
        find: incompatible_entry["find"].as_str().unwrap_or("").to_owned(),
        replace: incompatible_entry["replace"]
            .as_str()
            .unwrap_or("")
            .to_owned(),
        expect_marker: String::new(),
    };
    let expected_incompatible_code = incompatible_entry["expect_diagnostic_code"]
        .as_str()
        .unwrap_or("E0061")
        .to_owned();
    let fixture = GroupingFixture {
        base,
        compatible,
        incompatible,
        expected_incompatible_code,
    };
    // Validate the frozen edit contract before measuring anything: each `find`
    // must occur exactly once in the base source.
    for edit in &fixture.compatible {
        let occurrences = fixture.base.matches(&edit.find).count();
        if occurrences != 1 {
            return Err(format!(
                "edit {} must match exactly once in the base source, got {occurrences}",
                edit.name
            ));
        }
        if edit.expect_marker.is_empty() {
            return Err(format!("edit {} has no expect_marker", edit.name));
        }
    }
    let occurrences = fixture.base.matches(&fixture.incompatible.find).count();
    if occurrences != 1 {
        return Err(format!(
            "incompatible edit {} must match exactly once in the base source, got {occurrences}",
            fixture.incompatible.name
        ));
    }
    Ok(fixture)
}

fn apply_edit(content: &str, edit: &Edit) -> Result<String, String> {
    let occurrences = content.matches(&edit.find).count();
    if occurrences != 1 {
        return Err(format!(
            "edit {} expected exactly 1 match, got {occurrences}",
            edit.name
        ));
    }
    Ok(content.replace(&edit.find, &edit.replace))
}

fn apply_compatible(content: &str, fixture: &GroupingFixture) -> Result<String, String> {
    let mut current = content.to_owned();
    for edit in &fixture.compatible {
        current = apply_edit(&current, edit)?;
    }
    Ok(current)
}

fn context() -> BuildContext {
    BuildContext {
        edition: "2021".to_owned(),
        crate_name: "grouping_unit".to_owned(),
        crate_type: "lib".to_owned(),
        flags: Vec::new(),
    }
}

fn files_for(content: &str) -> Vec<SourceFile> {
    vec![SourceFile {
        path: "src/lib.rs".to_owned(),
        content: content.to_owned(),
    }]
}

/// One analysis + verification step of the harness.
struct Step {
    analyze_ms: u64,
    verify_ms: u64,
    success: bool,
    error_count: usize,
    diagnostic_codes: Vec<String>,
    report_status: String,
    cache_kind: String,
    identity_ok: bool,
    markers_ok: bool,
    marker_checks: usize,
}

/// Submits `content` to the daemon, then verifies:
/// 1. the daemon really analysed the bytes we composed (recomputed content
///    identity equals the reported snapshot id);
/// 2. the report is `fresh`;
/// 3. every expected marker is present in the submitted content.
fn analyze_and_verify(
    daemon: &Daemon,
    revision: u64,
    label: String,
    content: &str,
    expected_markers: &[&str],
) -> Result<Step, String> {
    let request = ravum_daemon::AnalysisRequest {
        revision,
        label,
        root: "src/lib.rs".to_owned(),
        files: files_for(content),
        context: context(),
        priority: ravum_daemon::Priority::Normal,
    };
    let expected_identity = request
        .identity(daemon.toolchain())
        .map_err(|error| format!("cannot compute request identity: {error}"))?;
    let analyze_started = Instant::now();
    let report = daemon
        .analyze(&request)
        .map_err(|error| format!("daemon analyze failed: {error}"))?;
    let analyze_ms = analyze_started.elapsed().as_millis() as u64;
    let verify_started = Instant::now();
    let identity_ok = report.snapshot_id == expected_identity.snapshot_id
        && report.context_id == expected_identity.context_id;
    let mut markers_ok = true;
    for marker in expected_markers {
        if !content.contains(marker) {
            markers_ok = false;
        }
    }
    let verify_ms = verify_started.elapsed().as_millis() as u64;
    let mut diagnostic_codes: Vec<String> = report
        .diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.code.clone())
        .collect();
    diagnostic_codes.sort();
    diagnostic_codes.dedup();
    Ok(Step {
        analyze_ms,
        verify_ms,
        success: report.success && report.error_count() == 0 && identity_ok && markers_ok,
        error_count: report.error_count(),
        diagnostic_codes,
        report_status: report.status.as_str().to_owned(),
        cache_kind: report.cache.kind.as_str().to_owned(),
        identity_ok,
        markers_ok,
        marker_checks: expected_markers.len(),
    })
}

/// Fresh daemon state: no verdict cache, no previous snapshot survives, so every
/// analysis of the mode is a real compiler invocation (verified with stats).
fn fresh_daemon(work_root: &Path) -> Result<(Daemon, PathBuf), String> {
    if work_root.exists() {
        std::fs::remove_dir_all(work_root)
            .map_err(|error| format!("cannot clean {}: {error}", work_root.display()))?;
    }
    std::fs::create_dir_all(work_root)
        .map_err(|error| format!("cannot create {}: {error}", work_root.display()))?;
    let daemon = harness::open_daemon(work_root, 1)?;
    Ok((daemon, work_root.to_path_buf()))
}

fn disk_bytes(work: &Path) -> Option<u64> {
    ravum_daemon::gc::measure_total_bytes(work).ok()
}

// ---------------------------------------------------------------------------
// Process observability: CPU process time and RSS sampling (Windows reference).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default)]
struct CpuTimes {
    user_ms: u64,
    kernel_ms: u64,
}

fn cpu_times() -> Option<CpuTimes> {
    platform::cpu_times()
}

fn working_set_bytes() -> Option<u64> {
    platform::working_set_bytes()
}

/// Samples the harness process working set every couple of milliseconds for the
/// duration of one mode, so a short peak between two phases is not missed.
struct RssSampler {
    stop: Arc<AtomicBool>,
    peak: Arc<AtomicU64>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl RssSampler {
    fn start() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let peak = Arc::new(AtomicU64::new(0));
        let handle = if cfg!(windows) {
            let stop = Arc::clone(&stop);
            let peak = Arc::clone(&peak);
            Some(std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    if let Some(bytes) = working_set_bytes() {
                        peak.fetch_max(bytes, Ordering::Relaxed);
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                if let Some(bytes) = working_set_bytes() {
                    peak.fetch_max(bytes, Ordering::Relaxed);
                }
            }))
        } else {
            None
        };
        Self { stop, peak, handle }
    }

    fn finish(mut self) -> Option<u64> {
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

#[cfg(windows)]
mod platform {
    use super::CpuTimes;

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

    #[repr(C)]
    #[derive(Default)]
    struct MemoryCounters {
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
        fn GetProcessTimes(
            process: isize,
            creation_time: *mut FileTime,
            exit_time: *mut FileTime,
            kernel_time: *mut FileTime,
            user_time: *mut FileTime,
        ) -> i32;
    }

    #[link(name = "psapi")]
    extern "system" {
        fn GetProcessMemoryInfo(process: isize, counters: *mut MemoryCounters, cb: u32) -> i32;
    }

    pub(super) fn cpu_times() -> Option<CpuTimes> {
        // SAFETY: `GetCurrentProcess` returns a pseudo-handle that must not be
        // closed; the four FILETIME out-parameters are valid, writable and
        // correctly sized for the duration of the call.
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
            // FILETIME is in 100 ns ticks; only kernel+user are wanted.
            let ticks = |time: FileTime| time.as_u64() / 10_000;
            Some(CpuTimes {
                user_ms: ticks(user),
                kernel_ms: ticks(kernel),
            })
        }
    }

    pub(super) fn working_set_bytes() -> Option<u64> {
        // SAFETY: the pseudo-handle needs no release; `counters` is `repr(C)`,
        // zeroed and its `cb` field carries its own size as the API requires.
        unsafe {
            let mut counters = MemoryCounters {
                cb: std::mem::size_of::<MemoryCounters>() as u32,
                ..Default::default()
            };
            let ok = GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) != 0;
            ok.then_some(counters.working_set_size as u64)
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::CpuTimes;

    pub(super) fn cpu_times() -> Option<CpuTimes> {
        None
    }

    pub(super) fn working_set_bytes() -> Option<u64> {
        None
    }
}

// ---------------------------------------------------------------------------
// Mode measurements
// ---------------------------------------------------------------------------

fn mode_slug(mode: &str, window_ms: u64) -> String {
    match mode {
        "sequential" => "sequential".to_owned(),
        "grouped" => format!("grouped-w{window_ms:03}"),
        "incompatible" => "incompatible".to_owned(),
        other => other.to_owned(),
    }
}

/// Runs one mode on a fresh daemon and returns its raw observation.
fn run_mode(
    work_root: &Path,
    fixture: &GroupingFixture,
    mode: &str,
    window_ms: u64,
    rep: usize,
) -> Result<Value, String> {
    let slug = mode_slug(mode, window_ms);
    let mode_dir = work_root.join(format!("rep-{rep:02}")).join(&slug);
    let (daemon, _work) = fresh_daemon(&mode_dir)?;
    // The daemon owns `<work>/daemon`; `measure_total_bytes` inventories that
    // private layout (cache + snapshots + incremental), so point it there.
    let daemon_state = daemon.work_dir().to_path_buf();
    let disk_after_open = disk_bytes(&daemon_state);
    let rss_start = working_set_bytes();
    let cpu_start = cpu_times();
    let sampler = RssSampler::start();

    let mut observation = Observation::new(mode, window_ms, rep);
    let started = Instant::now();
    let mut revision: u64 = 0;
    let mut content = fixture.base.clone();
    let compatible_count = fixture.compatible_count();

    match mode {
        "sequential" => {
            for (index, edit) in fixture.compatible.iter().enumerate() {
                let compose_started = Instant::now();
                content = apply_edit(&content, edit)?;
                observation.compose_ms += compose_started.elapsed().as_millis() as u64;
                revision += 1;
                let markers: Vec<&str> = fixture.compatible[..=index]
                    .iter()
                    .map(|edit| edit.expect_marker.as_str())
                    .collect();
                let step = analyze_and_verify(
                    &daemon,
                    revision,
                    format!("grouping/sequential/rep{rep}/edit-{}", index + 1),
                    &content,
                    &markers,
                )?;
                observation.absorb_step(&step, true);
                observation.cycles.push(json!({
                    "edit": edit.name,
                    "analyze_ms": step.analyze_ms,
                    "verify_ms": step.verify_ms,
                    "status": step.report_status,
                    "success": step.success,
                    "error_count": step.error_count,
                    "cache_kind": step.cache_kind,
                }));
                if !step.success {
                    observation.correct = false;
                    observation.errors.push(format!(
                        "sequential rep {rep}: edit {} did not analyse cleanly ({} error(s), codes {:?})",
                        edit.name, step.error_count, step.diagnostic_codes
                    ));
                }
            }
        }
        "grouped" => {
            if window_ms > 0 {
                let wait_started = Instant::now();
                std::thread::sleep(Duration::from_millis(window_ms));
                observation.wait_ms = wait_started.elapsed().as_millis() as u64;
            }
            let compose_started = Instant::now();
            content = apply_compatible(&content, fixture)?;
            observation.compose_ms += compose_started.elapsed().as_millis() as u64;
            revision += 1;
            let markers: Vec<&str> = fixture
                .compatible
                .iter()
                .map(|edit| edit.expect_marker.as_str())
                .collect();
            let step = analyze_and_verify(
                &daemon,
                revision,
                format!("grouping/grouped-w{window_ms:03}/rep{rep}"),
                &content,
                &markers,
            )?;
            observation.absorb_step(&step, true);
            observation.cycles.push(json!({
                "edit_count": compatible_count,
                "window_ms": window_ms,
                "analyze_ms": step.analyze_ms,
                "verify_ms": step.verify_ms,
                "status": step.report_status,
                "success": step.success,
                "error_count": step.error_count,
                "cache_kind": step.cache_kind,
            }));
            if !step.success {
                observation.correct = false;
                observation.errors.push(format!(
                    "grouped w{window_ms}ms rep {rep}: batch did not analyse cleanly ({} error(s), codes {:?})",
                    step.error_count, step.diagnostic_codes
                ));
            }
        }
        "incompatible" => {
            // 1. Grouped batch containing the incompatible edit: must be refused.
            let batch_started = Instant::now();
            let mut batch_content = apply_compatible(&content, fixture)?;
            batch_content = apply_edit(&batch_content, &fixture.incompatible)?;
            revision += 1;
            let batch = analyze_and_verify(
                &daemon,
                revision,
                format!("grouping/incompatible-batch/rep{rep}"),
                &batch_content,
                &[],
            )?;
            let batch_latency_ms = batch_started.elapsed().as_millis() as u64;
            observation.batch_latency_ms = batch_latency_ms;
            observation.batch_rejected =
                !batch.success && batch.error_count > 0 && batch.identity_ok;
            observation.batch_error_count = batch.error_count;
            observation.batch_codes = batch.diagnostic_codes.clone();
            observation.error_latency_ms = batch.analyze_ms;
            observation.absorb_failed_step(&batch);
            observation.cycles.push(json!({
                "phase": "batch-with-incompatible",
                "latency_ms": batch_latency_ms,
                "analyze_ms": batch.analyze_ms,
                "rejected": observation.batch_rejected,
                "error_count": batch.error_count,
                "codes": batch.diagnostic_codes,
            }));

            // 2. Individual recovery: each edit alone on the base source. This is
            //    the conservative fallback a real implementation would use to
            //    isolate the faulty proposal.
            let mut compatible_failures = 0usize;
            let recovery_started = Instant::now();
            for edit in &fixture.compatible {
                let alone = apply_edit(&fixture.base, edit)?;
                revision += 1;
                let step = analyze_and_verify(
                    &daemon,
                    revision,
                    format!("grouping/recovery/rep{rep}/{}", edit.name),
                    &alone,
                    &[edit.expect_marker.as_str()],
                )?;
                observation.absorb_step(&step, true);
                if !step.success {
                    compatible_failures += 1;
                    observation.errors.push(format!(
                        "incompatible rep {rep}: compatible edit {} failed alone ({} error(s), codes {:?})",
                        edit.name, step.error_count, step.diagnostic_codes
                    ));
                }
                observation.recovery_steps.push(json!({
                    "edit": edit.name,
                    "status": step.report_status,
                    "success": step.success,
                    "analyze_ms": step.analyze_ms,
                    "error_count": step.error_count,
                    "codes": step.diagnostic_codes,
                }));
            }
            let alone_incompatible = apply_edit(&fixture.base, &fixture.incompatible)?;
            revision += 1;
            let incompatible_step = analyze_and_verify(
                &daemon,
                revision,
                format!("grouping/recovery/rep{rep}/incompatible"),
                &alone_incompatible,
                &[],
            )?;
            observation.absorb_failed_step(&incompatible_step);
            observation.incompatible_identified = !incompatible_step.success
                && incompatible_step
                    .diagnostic_codes
                    .iter()
                    .any(|code| code == &fixture.expected_incompatible_code);
            observation.recovery_steps.push(json!({
                "edit": fixture.incompatible.name,
                "status": incompatible_step.report_status,
                "success": incompatible_step.success,
                "analyze_ms": incompatible_step.analyze_ms,
                "error_count": incompatible_step.error_count,
                "codes": incompatible_step.diagnostic_codes,
                "identified": observation.incompatible_identified,
            }));

            // 3. Regroup the compatible edits only: accepted, all markers present.
            let retry_started = Instant::now();
            let regrouped = apply_compatible(&fixture.base, fixture)?;
            revision += 1;
            let retry = analyze_and_verify(
                &daemon,
                revision,
                format!("grouping/recovery-grouped/rep{rep}"),
                &regrouped,
                &fixture
                    .compatible
                    .iter()
                    .map(|edit| edit.expect_marker.as_str())
                    .collect::<Vec<_>>(),
            )?;
            observation.absorb_step(&retry, true);
            observation.retry_ms = retry_started.elapsed().as_millis() as u64;
            observation.retry_success = retry.success;
            observation.cycles.push(json!({
                "phase": "recovery-grouped-compatible",
                "analyze_ms": retry.analyze_ms,
                "status": retry.report_status,
                "success": retry.success,
                "error_count": retry.error_count,
                "cache_kind": retry.cache_kind,
            }));
            observation.recovery_total_ms =
                recovery_started.elapsed().as_millis() as u64 + batch_latency_ms;
            observation.correct = observation.batch_rejected
                && observation.incompatible_identified
                && compatible_failures == 0
                && retry.success;
            if !observation.correct {
                observation.errors.push(format!(
                    "incompatible rep {rep}: rejection={} identified={} compatible_failures={} retry_success={}",
                    observation.batch_rejected,
                    observation.incompatible_identified,
                    compatible_failures,
                    retry.success
                ));
            }
            content = regrouped;
        }
        other => return Err(format!("unknown grouping mode {other:?}")),
    }

    let total_ms = started.elapsed().as_millis() as u64;
    let rss_peak = sampler.finish();
    let rss_end = working_set_bytes();
    let cpu_end = cpu_times();
    let stats = daemon.stats();
    let disk_after = disk_bytes(&daemon_state);

    observation.total_ms = total_ms;
    observation.rss_start_bytes = rss_start;
    observation.rss_peak_bytes = rss_peak;
    observation.rss_end_bytes = rss_end;
    observation.disk_after_open_bytes = disk_after_open;
    observation.disk_after_mode_bytes = disk_after;
    observation.cpu_user_ms = match (cpu_start, cpu_end) {
        (Some(start), Some(end)) => Some(end.user_ms.saturating_sub(start.user_ms)),
        _ => None,
    };
    observation.cpu_kernel_ms = match (cpu_start, cpu_end) {
        (Some(start), Some(end)) => Some(end.kernel_ms.saturating_sub(start.kernel_ms)),
        _ => None,
    };
    observation.rustc_runs = stats.rustc_runs;
    observation.cache_hits_memory = stats.cache_hits_memory;
    observation.cache_hits_disk = stats.cache_hits_disk;
    observation.cache_misses = stats.cache_misses;
    observation.daemon_requests = stats.requests;
    observation.snapshots_created = stats.snapshots_created;
    observation.unit_files_written = stats.unit_files_written;
    observation.daemon_stats = json!(stats);
    observation.content_digest = harness::content_digest(&files_for(&content));
    Ok(observation.to_json(fixture))
}

/// Raw observation of one mode on one repetition. Defaults are the "not
/// observed" values; nothing is inferred after the fact.
struct Observation {
    mode: String,
    window_ms: u64,
    repetition: usize,
    total_ms: u64,
    compose_ms: u64,
    analyze_ms: u64,
    verify_ms: u64,
    wait_ms: u64,
    analyses: u64,
    identity_checks: u64,
    identity_failures: u64,
    marker_checks: u64,
    marker_failures: u64,
    error_count: u64,
    diagnostic_codes: Vec<String>,
    cache_kinds: Vec<String>,
    rustc_runs: u64,
    cache_hits_memory: u64,
    cache_hits_disk: u64,
    cache_misses: u64,
    daemon_requests: u64,
    snapshots_created: u64,
    unit_files_written: u64,
    daemon_stats: Value,
    success: bool,
    correct: bool,
    content_digest: String,
    cycles: Vec<Value>,
    recovery_steps: Vec<Value>,
    rss_start_bytes: Option<u64>,
    rss_peak_bytes: Option<u64>,
    rss_end_bytes: Option<u64>,
    disk_after_open_bytes: Option<u64>,
    disk_after_mode_bytes: Option<u64>,
    cpu_user_ms: Option<u64>,
    cpu_kernel_ms: Option<u64>,
    batch_latency_ms: u64,
    error_latency_ms: u64,
    batch_rejected: bool,
    batch_error_count: usize,
    batch_codes: Vec<String>,
    incompatible_identified: bool,
    recovery_total_ms: u64,
    retry_ms: u64,
    retry_success: bool,
    errors: Vec<String>,
}

impl Observation {
    fn new(mode: &str, window_ms: u64, repetition: usize) -> Self {
        Self {
            mode: mode.to_owned(),
            window_ms,
            repetition,
            total_ms: 0,
            compose_ms: 0,
            analyze_ms: 0,
            verify_ms: 0,
            wait_ms: 0,
            analyses: 0,
            identity_checks: 0,
            identity_failures: 0,
            marker_checks: 0,
            marker_failures: 0,
            error_count: 0,
            diagnostic_codes: Vec::new(),
            cache_kinds: Vec::new(),
            rustc_runs: 0,
            cache_hits_memory: 0,
            cache_hits_disk: 0,
            cache_misses: 0,
            daemon_requests: 0,
            snapshots_created: 0,
            unit_files_written: 0,
            daemon_stats: Value::Null,
            success: false,
            correct: true,
            content_digest: String::new(),
            cycles: Vec::new(),
            recovery_steps: Vec::new(),
            rss_start_bytes: None,
            rss_peak_bytes: None,
            rss_end_bytes: None,
            disk_after_open_bytes: None,
            disk_after_mode_bytes: None,
            cpu_user_ms: None,
            cpu_kernel_ms: None,
            batch_latency_ms: 0,
            error_latency_ms: 0,
            batch_rejected: false,
            batch_error_count: 0,
            batch_codes: Vec::new(),
            incompatible_identified: false,
            recovery_total_ms: 0,
            retry_ms: 0,
            retry_success: false,
            errors: Vec::new(),
        }
    }

    /// Absorbs one step whose success is a required postcondition.
    fn absorb_step(&mut self, step: &Step, success_required: bool) {
        self.analyses += 1;
        self.analyze_ms += step.analyze_ms;
        self.verify_ms += step.verify_ms;
        self.identity_checks += 1;
        if !step.identity_ok {
            self.identity_failures += 1;
        }
        self.marker_checks += step.marker_checks as u64;
        if !step.markers_ok {
            self.marker_failures += 1;
        }
        self.error_count += step.error_count as u64;
        self.diagnostic_codes.extend(step.diagnostic_codes.clone());
        self.cache_kinds.push(step.cache_kind.clone());
        if success_required && step.success {
            self.success = true;
        }
        if !step.success && success_required {
            self.correct = false;
        }
    }

    /// Absorbs a step that is *expected* to fail (negative control): only the
    /// identity/verification facts are recorded, never a success claim.
    fn absorb_failed_step(&mut self, step: &Step) {
        self.analyses += 1;
        self.analyze_ms += step.analyze_ms;
        self.verify_ms += step.verify_ms;
        self.identity_checks += 1;
        if !step.identity_ok {
            self.identity_failures += 1;
        }
        self.error_count += step.error_count as u64;
        self.diagnostic_codes.extend(step.diagnostic_codes.clone());
        self.cache_kinds.push(step.cache_kind.clone());
    }

    fn analysis_count(&self) -> u64 {
        self.analyses
    }

    fn to_json(&self, _fixture: &GroupingFixture) -> Value {
        json!({
            "mode": self.mode,
            "window_ms": self.window_ms,
            "repetition": self.repetition,
            "total_ms": self.total_ms,
            "phases_ms": {
                "wait": self.wait_ms,
                "compose": self.compose_ms,
                "analyze": self.analyze_ms,
                "verify": self.verify_ms,
                "unaccounted": self.total_ms.saturating_sub(
                    self.wait_ms + self.compose_ms + self.analyze_ms + self.verify_ms
                ),
            },
            "analyses": self.analysis_count(),
            "rustc_runs": self.rustc_runs,
            "cache": {
                "hits_memory": self.cache_hits_memory,
                "hits_disk": self.cache_hits_disk,
                "misses": self.cache_misses,
                "kinds_per_analysis": self.cache_kinds,
            },
            "daemon": {
                "requests": self.daemon_requests,
                "snapshots_created": self.snapshots_created,
                "unit_files_written": self.unit_files_written,
                "stats": self.daemon_stats,
            },
            "verification": {
                "definition": "content postcondition (all applied markers present in the submitted bytes) + recomputed content identity equals the report snapshot_id + report is fresh",
                "identity_checks": self.identity_checks,
                "identity_failures": self.identity_failures,
                "marker_checks": self.marker_checks,
                "marker_failures": self.marker_failures,
            },
            "success": self.success,
            "correct": self.correct,
            "content_digest": self.content_digest,
            "error_count": self.error_count,
            "diagnostic_codes": self.diagnostic_codes,
            "cycles": self.cycles,
            "recovery": {
                "batch_latency_ms": self.batch_latency_ms,
                "error_latency_ms": self.error_latency_ms,
                "batch_rejected": self.batch_rejected,
                "batch_error_count": self.batch_error_count,
                "batch_codes": self.batch_codes,
                "incompatible_identified": self.incompatible_identified,
                "recovery_total_ms": self.recovery_total_ms,
                "retry_ms": self.retry_ms,
                "retry_success": self.retry_success,
                "steps": self.recovery_steps,
            },
            "cpu": {
                "process_user_ms": self.cpu_user_ms,
                "process_kernel_ms": self.cpu_kernel_ms,
                "note": "CPU du processus harnais (daemon in-process); le CPU des processus rustc enfants n'est pas attribué ici, leur temps est timings.analysis_ms",
            },
            "rss_bytes": {
                "start": self.rss_start_bytes,
                "peak_sampled": self.rss_peak_bytes,
                "end": self.rss_end_bytes,
                "note": "working set du processus harnais échantillonné toutes les 2 ms; le pic peut inclure de la mémoire résiduelle des modes précédents car le processus est réutilisé",
            },
            "disk_bytes": {
                "after_daemon_open": self.disk_after_open_bytes,
                "after_mode": self.disk_after_mode_bytes,
                "note": "état du daemon (snapshots + cache disque + incrémental) mesuré par ravum_daemon::gc::measure_total_bytes",
            },
            "errors": self.errors,
        })
    }
}

// ---------------------------------------------------------------------------
// Campaign
// ---------------------------------------------------------------------------

const MODES: [&str; 6] = [
    "sequential",
    "grouped-0",
    "grouped-25",
    "grouped-50",
    "grouped-100",
    "incompatible",
];

fn parse_mode(spec: &str) -> (&'static str, u64) {
    match spec {
        "sequential" => ("sequential", 0),
        "grouped-0" => ("grouped", 0),
        "grouped-25" => ("grouped", 25),
        "grouped-50" => ("grouped", 50),
        "grouped-100" => ("grouped", 100),
        "incompatible" => ("incompatible", 0),
        other => panic!("unknown mode spec {other}"),
    }
}

/// Rotates the mode order every repetition so host-load drift is spread over
/// modes instead of being attributed to one of them.
fn rotated_modes(repetition: usize) -> Vec<&'static str> {
    let offset = repetition % MODES.len();
    let mut order = Vec::with_capacity(MODES.len());
    for index in 0..MODES.len() {
        order.push(MODES[(index + offset) % MODES.len()]);
    }
    order
}

fn stats_for(values: &[u64]) -> Value {
    harness::stats(values)
}

fn mode_key(spec: &str) -> String {
    spec.to_owned()
}

pub fn run(options: &GroupingOptions) -> Result<Value, String> {
    if options.reps < MIN_REPS {
        return Err(format!(
            "grouping campaign needs at least {MIN_REPS} recorded repetitions, got {}",
            options.reps
        ));
    }
    let fixture = load_fixture(&options.repo_root)?;
    let rustc = harness::rustc_path();
    let (_, toolchain_json) = harness::toolchain_json(&options.repo_root)?;
    let campaign_started = Instant::now();
    let calibration_start = ravum_daemon::sysinfo::spawn_calibration(&rustc, 3);
    let load_start = ravum_daemon::sysinfo::capture(250);

    // One unrecorded warm-up repetition: the first daemon opens and the first
    // rustc invocations include OS file-cache effects that are not part of the
    // measured comparison (same protocol as the MVP-0b harness). The warm-up is
    // reported as such and never enters the statistics.
    let warmup_work = options.work_root.join("warmup");
    let mut warmup_modes = Vec::new();
    for spec in rotated_modes(0) {
        let (mode, window) = parse_mode(spec);
        let observation = run_mode(&warmup_work, &fixture, mode, window, 0)?;
        warmup_modes.push(json!({
            "mode": spec,
            "total_ms": observation["total_ms"],
            "success": observation["success"],
            "correct": observation["correct"],
        }));
    }
    let _ = std::fs::remove_dir_all(&warmup_work);

    // Recorded repetitions.
    let mut repetitions: Vec<Value> = Vec::with_capacity(options.reps);
    for repetition in 1..=options.reps {
        let order = rotated_modes(repetition - 1);
        let load_before = ravum_daemon::sysinfo::capture(100);
        let mut modes = Vec::with_capacity(order.len());
        for spec in &order {
            let (mode, window) = parse_mode(spec);
            let observation = run_mode(&options.work_root, &fixture, mode, window, repetition)?;
            modes.push(json!({
                "mode_spec": spec,
                "observation": observation,
            }));
        }
        let load_after = ravum_daemon::sysinfo::capture(100);
        repetitions.push(json!({
            "repetition": repetition,
            "order": order,
            "load_before": load_before,
            "load_after": load_after,
            "modes": modes,
        }));
        println!(
            "  repetition {repetition}/{} done (order: {})",
            options.reps,
            order.join(", ")
        );
    }

    let calibration_end = ravum_daemon::sysinfo::spawn_calibration(&rustc, 3);
    let load_end = ravum_daemon::sysinfo::capture(250);
    let campaign_ms = campaign_started.elapsed().as_millis() as u64;

    let by_mode: std::collections::BTreeMap<String, Vec<&Value>> = {
        let mut map: std::collections::BTreeMap<String, Vec<&Value>> = Default::default();
        for repetition in &repetitions {
            if let Some(modes) = repetition["modes"].as_array() {
                for entry in modes {
                    let spec = entry["mode_spec"].as_str().unwrap_or("?");
                    map.entry(mode_key(spec))
                        .or_default()
                        .push(&entry["observation"]);
                }
            }
        }
        map
    };

    let total_ms_values = |spec: &str| -> Vec<u64> {
        by_mode
            .get(spec)
            .map(|observations| {
                observations
                    .iter()
                    .filter_map(|observation| observation["total_ms"].as_u64())
                    .collect()
            })
            .unwrap_or_default()
    };
    let analysis_ms_values = |spec: &str| -> Vec<u64> {
        by_mode
            .get(spec)
            .map(|observations| {
                observations
                    .iter()
                    .filter_map(|observation| observation["phases_ms"]["analyze"].as_u64())
                    .collect()
            })
            .unwrap_or_default()
    };
    let wait_ms_values = |spec: &str| -> Vec<u64> {
        by_mode
            .get(spec)
            .map(|observations| {
                observations
                    .iter()
                    .filter_map(|observation| observation["phases_ms"]["wait"].as_u64())
                    .collect()
            })
            .unwrap_or_default()
    };
    let verify_ms_values = |spec: &str| -> Vec<u64> {
        by_mode
            .get(spec)
            .map(|observations| {
                observations
                    .iter()
                    .filter_map(|observation| observation["phases_ms"]["verify"].as_u64())
                    .collect()
            })
            .unwrap_or_default()
    };
    let compose_ms_values = |spec: &str| -> Vec<u64> {
        by_mode
            .get(spec)
            .map(|observations| {
                observations
                    .iter()
                    .filter_map(|observation| observation["phases_ms"]["compose"].as_u64())
                    .collect()
            })
            .unwrap_or_default()
    };
    let rustc_values = |spec: &str| -> Vec<u64> {
        by_mode
            .get(spec)
            .map(|observations| {
                observations
                    .iter()
                    .filter_map(|observation| observation["rustc_runs"].as_u64())
                    .collect()
            })
            .unwrap_or_default()
    };
    let analyses_values = |spec: &str| -> Vec<u64> {
        by_mode
            .get(spec)
            .map(|observations| {
                observations
                    .iter()
                    .filter_map(|observation| observation["analyses"].as_u64())
                    .collect()
            })
            .unwrap_or_default()
    };

    let mode_summary = |spec: &str| -> Value {
        let observations = by_mode.get(spec).cloned().unwrap_or_default();
        let success_count = observations
            .iter()
            .filter(|observation| observation["success"].as_bool() == Some(true))
            .count();
        let correct_count = observations
            .iter()
            .filter(|observation| observation["correct"].as_bool() == Some(true))
            .count();
        json!({
            "analyses": stats_for(&analyses_values(spec)),
            "rustc_runs": stats_for(&rustc_values(spec)),
            "total_ms": stats_for(&total_ms_values(spec)),
            "phases_ms": {
                "wait": stats_for(&wait_ms_values(spec)),
                "compose": stats_for(&compose_ms_values(spec)),
                "analyze": stats_for(&analysis_ms_values(spec)),
                "verify": stats_for(&verify_ms_values(spec)),
            },
            "success_count": success_count,
            "correct_count": correct_count,
            "observations": observations.len(),
            "valid": correct_count == observations.len() && !observations.is_empty(),
        })
    };

    let mut modes_summary = serde_json::Map::new();
    for spec in MODES {
        modes_summary.insert(spec.to_owned(), mode_summary(spec));
    }

    // Paired comparison: for each repetition, sequential total minus grouped
    // total (window included). Paired deltas remove part of the host-load drift.
    let paired_gain = |window_spec: &str| -> Value {
        let sequential = by_mode.get("sequential").cloned().unwrap_or_default();
        let grouped = by_mode.get(window_spec).cloned().unwrap_or_default();
        let mut gains: Vec<u64> = Vec::new();
        let mut ratios: Vec<f64> = Vec::new();
        for index in 0..sequential.len().min(grouped.len()) {
            let Some(seq) = sequential[index]["total_ms"].as_u64() else {
                continue;
            };
            let Some(group) = grouped[index]["total_ms"].as_u64() else {
                continue;
            };
            gains.push(seq.saturating_sub(group));
            if seq > 0 {
                ratios.push(100.0 * seq.saturating_sub(group) as f64 / seq as f64);
            }
        }
        let gain_stats = stats_for(&gains);
        let ratio_stats = stats_for(
            &ratios
                .iter()
                .map(|ratio| ratio.round() as u64)
                .collect::<Vec<_>>(),
        );
        let sequential_median = stats_for(&total_ms_values("sequential"))["p50"]
            .as_u64()
            .unwrap_or(0);
        let grouped_median = stats_for(&total_ms_values(window_spec))["p50"]
            .as_u64()
            .unwrap_or(0);
        json!({
            "window_ms": wait_ms_values(window_spec).first().copied().unwrap_or(0),
            "sequential_total_ms": stats_for(&total_ms_values("sequential")),
            "grouped_total_ms": stats_for(&total_ms_values(window_spec)),
            "gain_ms_paired": gain_stats,
            "gain_percent_paired": ratio_stats,
            "gain_ms_medians": sequential_median.saturating_sub(grouped_median),
            "analyses_saved_per_cycle": analyses_values("sequential").first().copied().unwrap_or(0).saturating_sub(analyses_values(window_spec).first().copied().unwrap_or(0)),
            "rustc_runs_saved_per_cycle": rustc_values("sequential").first().copied().unwrap_or(0).saturating_sub(rustc_values(window_spec).first().copied().unwrap_or(0)),
        })
    };

    let comparisons: Vec<Value> = ["grouped-0", "grouped-25", "grouped-50", "grouped-100"]
        .iter()
        .map(|spec| paired_gain(spec))
        .collect();

    // Break-even window: derived from measured analysis cost, explicitly
    // labelled as an estimate (the campaign only measures 0..=100 ms windows).
    // Cost of one sequential cycle = total analysis time of the mode / N, so the
    // estimate is `(N-1) * per_cycle_cost`: the window can grow to that value
    // before the grouped strategy stops winning on analysis time.
    let compatible_count = fixture.compatible_count() as u64;
    let sequential_analysis_total = analysis_ms_values("sequential");
    let sequential_analysis_count = analyses_values("sequential")
        .first()
        .copied()
        .unwrap_or(compatible_count)
        .max(1);
    let per_cycle_analysis_values: Vec<u64> = sequential_analysis_total
        .iter()
        .map(|total| total / sequential_analysis_count)
        .collect();
    let per_cycle_analysis_ms = stats_for(&per_cycle_analysis_values)["p50"]
        .as_u64()
        .unwrap_or(0);
    let estimated_break_even_window_ms =
        per_cycle_analysis_ms.saturating_mul(compatible_count.saturating_sub(1));
    let max_measured_window_with_positive_gain = comparisons
        .iter()
        .filter(|comparison| comparison["gain_ms_medians"].as_i64().unwrap_or(0) > 0)
        .filter_map(|comparison| comparison["window_ms"].as_u64())
        .max()
        .unwrap_or(0);

    // Digest equality across strategies and repetitions: the grouped snapshot
    // must contain exactly the same final bytes as the sequential cumulative
    // content, and the incompatible recovery must land on the same content.
    let digests = |spec: &str| -> Vec<String> {
        by_mode
            .get(spec)
            .map(|observations| {
                observations
                    .iter()
                    .filter_map(|observation| observation["content_digest"].as_str())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    let sequential_digests = digests("sequential");
    let grouped_digests: Vec<String> = ["grouped-0", "grouped-25", "grouped-50", "grouped-100"]
        .iter()
        .flat_map(|spec| digests(spec))
        .collect();
    let incompatible_digests = digests("incompatible");
    let reference_digest = sequential_digests.first().cloned().unwrap_or_default();
    let content_digests_match = !reference_digest.is_empty()
        && sequential_digests
            .iter()
            .all(|digest| *digest == reference_digest)
        && grouped_digests
            .iter()
            .all(|digest| *digest == reference_digest)
        && incompatible_digests
            .iter()
            .all(|digest| *digest == reference_digest);

    // Identity/verification totals over every observation of every mode.
    let mut identity_checks: u64 = 0;
    let mut identity_failures: u64 = 0;
    let mut marker_checks: u64 = 0;
    let mut marker_failures: u64 = 0;
    let mut errors: Vec<String> = Vec::new();
    for repetition in &repetitions {
        if let Some(modes) = repetition["modes"].as_array() {
            for entry in modes {
                let observation = &entry["observation"];
                identity_checks += observation["verification"]["identity_checks"]
                    .as_u64()
                    .unwrap_or(0);
                identity_failures += observation["verification"]["identity_failures"]
                    .as_u64()
                    .unwrap_or(0);
                marker_checks += observation["verification"]["marker_checks"]
                    .as_u64()
                    .unwrap_or(0);
                marker_failures += observation["verification"]["marker_failures"]
                    .as_u64()
                    .unwrap_or(0);
                if let Some(list) = observation["errors"].as_array() {
                    for error in list {
                        if let Some(error) = error.as_str() {
                            errors.push(format!(
                                "rep {} {}: {error}",
                                repetition["repetition"].as_u64().unwrap_or(0),
                                entry["mode_spec"].as_str().unwrap_or("?")
                            ));
                        }
                    }
                }
            }
        }
    }

    // Expected rustc invocation counts on a fresh daemon state: one per analysis
    // (no cache hit is possible by construction) and N vs 1 for the comparison.
    let expected_rustc = |spec: &str| -> u64 {
        match spec {
            "sequential" => compatible_count,
            // batch (1) + individual recovery (N compatible + 1 incompatible) + retry (1)
            "incompatible" => compatible_count + 3,
            _ => 1,
        }
    };
    let mut counts_ok = true;
    for spec in MODES {
        let values = rustc_values(spec);
        if values.len() != options.reps || values.iter().any(|value| *value != expected_rustc(spec))
        {
            counts_ok = false;
            errors.push(format!(
                "{spec}: rustc_runs {values:?} (attendu {} par répétition)",
                expected_rustc(spec)
            ));
        }
    }

    let incompatible_summary = by_mode
        .get("incompatible")
        .map(|observations| {
            let batch_rejected = observations
                .iter()
                .filter(|observation| {
                    observation["recovery"]["batch_rejected"].as_bool() == Some(true)
                })
                .count();
            let identified = observations
                .iter()
                .filter(|observation| {
                    observation["recovery"]["incompatible_identified"].as_bool() == Some(true)
                })
                .count();
            let retry_ok = observations
                .iter()
                .filter(|observation| {
                    observation["recovery"]["retry_success"].as_bool() == Some(true)
                })
                .count();
            let batch_latency: Vec<u64> = observations
                .iter()
                .filter_map(|observation| observation["recovery"]["batch_latency_ms"].as_u64())
                .collect();
            let recovery_total: Vec<u64> = observations
                .iter()
                .filter_map(|observation| observation["recovery"]["recovery_total_ms"].as_u64())
                .collect();
            let retry: Vec<u64> = observations
                .iter()
                .filter_map(|observation| observation["recovery"]["retry_ms"].as_u64())
                .collect();
            json!({
                "observations": observations.len(),
                "batch_rejected_count": batch_rejected,
                "incompatible_identified_count": identified,
                "retry_success_count": retry_ok,
                "batch_latency_ms": stats_for(&batch_latency),
                "recovery_total_ms": stats_for(&recovery_total),
                "retry_ms": stats_for(&retry),
                "rustc_runs_per_rep": stats_for(&rustc_values("incompatible")),
                "no_incorrect_acceptance": batch_rejected == observations.len()
                    && identified == observations.len()
                    && retry_ok == observations.len(),
            })
        })
        .unwrap_or(Value::Null);

    let correct_modes = MODES.iter().all(|spec| {
        modes_summary
            .get(*spec)
            .and_then(|mode| mode["valid"].as_bool())
            == Some(true)
    });
    let incompatible_ok = incompatible_summary["no_incorrect_acceptance"].as_bool() == Some(true);
    let valid = correct_modes
        && incompatible_ok
        && counts_ok
        && content_digests_match
        && identity_failures == 0
        && marker_failures == 0
        && errors.is_empty()
        && options.reps >= MIN_REPS;

    if !valid {
        for error in &errors {
            eprintln!("grouping: {error}");
        }
    }

    Ok(json!({
        "schema_version": 1,
        "suite": "grouping",
        "generated_unix": ravum_protocol_unix(),
        "protocol": {
            "question": "N éditions compatibles : N cycles complets ou 1 snapshot groupé après une fenêtre de collecte bornée ?",
            "modes": MODES,
            "windows_ms": WINDOWS_MS,
            "repetitions_recorded": options.reps,
            "repetitions_minimum": MIN_REPS,
            "warmup": "1 répétition non enregistrée de tous les modes avant les mesures (effets de cache OS initiaux)",
            "alternation": "l'ordre des 6 modes tourne d'un cran à chaque répétition (dérive de charge répartie)",
            "fresh_state": "chaque mode/répétition utilise un répertoire d'état daemon neuf : aucun verdict ne peut être servi par le cache d'une autre variante; rustc_runs == analyses est vérifié",
            "build_definition": "l'analyse daemon est `rustc --emit=metadata`: « build » = compilation de métadonnées, aucun codegen ni lien (le daemon ne les couvre pas)",
            "verification_definition": "postcondition de contenu (marqueurs des éditions présents dans les octets soumis) + identité de contenu recalculée == snapshot_id rapporté + statut fresh; ce n'est pas une preuve sémantique",
            "wait_definition": "fenêtre de collecte simulée par sleep avant composition+analyse; elle est comptée dans le temps total",
            "phases": {
                "wait": "sleep de collecte simulé",
                "compose": "application des éditions aux octets (string replace vérifié unique)",
                "analyze": "appel daemon end-to-end (validation + identité + snapshot + cache + rustc si miss)",
                "verify": "postconditions de contenu + comparaison d'identité",
                "unaccounted": "surcoût de mesure (Instant, allocation, boucle) = total - phases",
            },
            "incompatible_scenario": "lot complet + édition incompatible => analyse refusée; reprise individuelle (N+1 analyses) pour isoler l'édition fautive; re-lot des compatibles => accepté",
            "excluded": "aucun codegen, lien, activation: le daemon est un moteur d'analyse",
            "load": "charge CPU/mémoire du système échantillonnée avant/après chaque répétition + calibration rustc -vV au début et à la fin",
        },
        "fixture": {
            "path": "fixtures/grouping",
            "source": "fixtures/grouping/src/lib.rs",
            "line_count": fixture.line_count(),
            "file_count": 1,
            "crate_name": "grouping_unit",
            "compatible_edits": fixture.compatible.iter().map(|edit| edit.name.clone()).collect::<Vec<_>>(),
            "incompatible_edit": fixture.incompatible.name,
            "expected_incompatible_code": fixture.expected_incompatible_code,
            "base_digest": harness::content_digest(&files_for(&fixture.base)),
            "compatible_digest": harness::content_digest(&files_for(
                &apply_compatible(&fixture.base, &fixture)?,
            )),
        },
        "machine": harness::machine_json(),
        "toolchain": toolchain_json,
        "git": harness::git_json(&options.repo_root),
        "campaign": {
            "started_unix": ravum_protocol_unix(),
            "duration_ms": campaign_ms,
            "calibration_start_ms": calibration_start,
            "calibration_end_ms": calibration_end,
            "load_start": load_start,
            "load_end": load_end,
            "warmup": warmup_modes,
        },
        "repetitions": repetitions,
        "summary": {
            "modes": modes_summary,
            "comparison": comparisons,
            "savings": {
                "compatible_edits": compatible_count,
                "analyses_per_sequential_cycle": compatible_count,
                "analyses_per_grouped_cycle": 1,
                "rustc_runs_saved_per_grouped_cycle": compatible_count.saturating_sub(1),
                "formula": "éditions groupées N => N analyses séquentielles remplacées par 1 analyse; économie = N-1 invocations rustc par lot",
            },
            "break_even": {
                "measured_windows_ms": WINDOWS_MS,
                "max_measured_window_with_positive_median_gain_ms": max_measured_window_with_positive_gain,
                "per_cycle_analysis_ms": stats_for(&per_cycle_analysis_values),
                "per_cycle_analysis_p50_ms": per_cycle_analysis_ms,
                "estimated_break_even_window_ms": estimated_break_even_window_ms,
                "formula": "fenêtre d'équilibre estimée = (N-1) × coût médian d'une analyse séquentielle (temps d'analyse total du mode séquentiel / N); estimation dérivée des mesures 0..=100 ms, non mesurée au-delà de 100 ms, suppose une attente purement latente",
                "status": "ESTIMATE (dérivée, pas une mesure directe de fenêtre > 100 ms)",
            },
            "correctness": {
                "all_modes_valid": correct_modes,
                "incompatible_handled": incompatible_ok,
                "rustc_counts_as_expected": counts_ok,
                "content_digests_match_across_strategies": content_digests_match,
                "reference_digest": reference_digest,
                "identity_checks": identity_checks,
                "identity_failures": identity_failures,
                "marker_checks": marker_checks,
                "marker_failures": marker_failures,
                "success_rate": json!({"modes_valid": modes_summary.values().filter(|mode| mode["valid"].as_bool() == Some(true)).count(), "modes_total": MODES.len()}),
            },
            "incompatible": incompatible_summary,
            "errors": errors,
        },
        "valid": valid,
    }))
}

fn ravum_protocol_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .map(|path| path.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    #[test]
    fn fixture_contract_is_valid_and_edits_are_unique() {
        let fixture = load_fixture(&repo_root()).expect("fixture");
        assert!(
            (300..=800).contains(&fixture.line_count()),
            "fixture line count {} outside the 300..=800 mandate",
            fixture.line_count()
        );
        assert!((4..=8).contains(&fixture.compatible_count()));
        let grouped = apply_compatible(&fixture.base, &fixture).expect("apply compatible");
        assert_ne!(grouped, fixture.base);
        for edit in &fixture.compatible {
            assert!(grouped.contains(&edit.expect_marker), "{}", edit.name);
        }
        let mut batch = grouped.clone();
        batch = apply_edit(&batch, &fixture.incompatible).expect("apply incompatible");
        assert_ne!(batch, grouped);
    }

    #[test]
    fn mode_rotation_is_a_permutation() {
        for repetition in 0..MODES.len() * 2 {
            let order = rotated_modes(repetition);
            assert_eq!(order.len(), MODES.len());
            let mut sorted = order.clone();
            sorted.sort_unstable();
            let mut expected = MODES.to_vec();
            expected.sort_unstable();
            assert_eq!(sorted, expected, "rotation must be a permutation");
        }
    }
}
