//! Shared benchmark harness: fixture loading, plain `rustc` reference modes,
//! statistics and evidence fragments.
//!
//! Three modes are compared on the same content states:
//!
//! * `cold` — one `rustc` per edit, no incremental state (the MVP-0a behaviour);
//! * `incremental` — plain `rustc` with a *persistent* `-C incremental` dir;
//! * `daemon` — the new persistent daemon (cache + incremental + snapshots).
//!
//! Frozen measurement definitions are written into the evidence file: the cold
//! and incremental numbers measure the compiler process only (source files are
//! already written), while the daemon number is the end-to-end `analyze` call;
//! the daemon's internal `analysis_ms` is reported separately so the comparison
//! is transparent instead of flattering.

use ravum_daemon::{
    parse_rustc_json, signature_set, BuildContext, Daemon, DaemonConfig, Diagnostic, SourceFile,
    ToolchainInfo,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

/// Frozen definition of each measured stage, quoted in the evidence files.
pub fn measurement_definitions() -> Value {
    json!({
        "cold_ms": "wall time of one `rustc --emit=metadata --error-format=json` process, source files already written, fresh --out-dir per call, no -C incremental: the MVP-0a behaviour",
        "incremental_ms": "wall time of the same `rustc` invocation with a persistent `-C incremental=<dir>` across samples, source files already synced, same paths and same --out-dir",
        "daemon_ms": "end-to-end `Daemon::analyze` call (contract validation + SHA-256 identity + snapshot/checkout sync + cache lookup + compiler when needed), measured by the harness with Instant around the call",
        "daemon_analysis_ms": "daemon-internal `timings.analysis_ms`: rustc process wall time only, 0 on a cache hit",
        "percentiles": "nearest-rank (ceil(p/100*n)-1), no interpolation; with 10 samples p95 is the 10th value",
        "samples": "10 recorded samples per category and mode after one unrecorded warm-up call per mode",
        "order": "modes rotated per sample (cold/incremental/daemon, then incremental/daemon/cold, ...) to spread host-load drift across modes",
        "correctness": "diagnostic signature sets (severity, code, logical file, line, column, end_line, message) must be identical across all three modes for the same state; any mismatch fails the campaign",
        "load": "CPU busy ratio sampled over 250 ms, free physical memory, process count and `rustc -vV` spawn calibration are recorded at the start/end of every category",
        "excluded": "no codegen, no link, no artifact verification: this campaign measures the analysis stage only"
    })
}

/// The fixture unit used by every category.
#[derive(Clone)]
pub struct Fixture {
    pub root: String,
    pub files: Vec<SourceFile>,
    pub context: BuildContext,
    pub dir: PathBuf,
}

pub fn load_fixture(repo_root: &Path) -> Result<Fixture, String> {
    let dir = repo_root
        .join("crates")
        .join("ravum-daemon")
        .join("fixtures")
        .join("unit");
    let src = dir.join("src");
    let mut files = Vec::new();
    let mut names: Vec<String> = std::fs::read_dir(&src)
        .map_err(|error| format!("cannot list fixture dir {}: {error}", src.display()))?
        .flatten()
        .filter(|entry| entry.path().extension().map(|ext| ext == "rs") == Some(true))
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    for name in names {
        let content = std::fs::read_to_string(src.join(&name))
            .map_err(|error| format!("cannot read fixture file {name}: {error}"))?;
        files.push(SourceFile {
            path: format!("src/{name}"),
            content,
        });
    }
    if files.is_empty() {
        return Err(format!("fixture dir {} has no .rs file", src.display()));
    }
    Ok(Fixture {
        root: "src/lib.rs".to_owned(),
        files,
        context: BuildContext {
            edition: "2021".to_owned(),
            crate_name: "acme_unit".to_owned(),
            crate_type: "lib".to_owned(),
            flags: Vec::new(),
        },
        dir,
    })
}

/// One run of one mode on one content state.
#[derive(Debug, Clone)]
pub struct ModeRun {
    pub mode: &'static str,
    pub ms: u64,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub diagnostics: Vec<Diagnostic>,
    pub cache_kind: Option<String>,
    pub report_status: Option<String>,
    pub timings: Option<ravum_daemon::Timings>,
}

impl ModeRun {
    pub fn signatures(&self) -> Vec<String> {
        signature_set(&self.diagnostics)
    }

    pub fn to_json(&self) -> Value {
        let mut value = json!({
            "mode": self.mode,
            "ms": self.ms,
            "success": self.success,
            "exit_code": self.exit_code,
            "diagnostic_count": self.diagnostics.len(),
            "diagnostic_signatures": self.signatures(),
        });
        if let Some(kind) = &self.cache_kind {
            value["cache_kind"] = json!(kind);
        }
        if let Some(status) = &self.report_status {
            value["status"] = json!(status);
        }
        if let Some(timings) = &self.timings {
            value["timings"] = json!({
                "accept_ms": timings.accept_ms,
                "queue_ms": timings.queue_ms,
                "lookup_ms": timings.lookup_ms,
                "analysis_ms": timings.analysis_ms,
                "total_ms": timings.total_ms,
            });
        }
        value
    }
}

/// Cold `rustc`: fresh output directory, no incremental state, one process.
pub fn run_cold(
    work_root: &Path,
    rustc: &Path,
    fixture: &Fixture,
    category: &str,
    sequence: u64,
    files: &[SourceFile],
) -> Result<ModeRun, String> {
    let call_dir = work_root
        .join("cold")
        .join(category)
        .join(format!("call-{sequence:06}"));
    let _ = std::fs::remove_dir_all(&call_dir);
    let pairs = file_pairs(files)?;
    ravum_daemon::sync_unit_dir(&call_dir.join("unit"), &pairs)
        .map_err(|error| format!("cannot write cold unit: {error}"))?;
    let out = call_dir.join("out");
    std::fs::create_dir_all(&out).map_err(|error| format!("cannot create out dir: {error}"))?;
    let root = call_dir.join("unit").join(&fixture.root);
    let started = Instant::now();
    let output = Command::new(rustc)
        .args(cold_args(&fixture.context, &out, &root))
        .env_remove("RUSTFLAGS")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_BOOTSTRAP")
        .output()
        .map_err(|error| format!("cannot spawn rustc (cold): {error}"))?;
    let ms = started.elapsed().as_millis() as u64;
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let parsed = parse_rustc_json(
        &stderr,
        Some(&call_dir.join("unit")),
        output.status.success(),
    );
    Ok(ModeRun {
        mode: "cold",
        ms,
        success: output.status.success() && !parsed.has_errors(),
        exit_code: output.status.code(),
        diagnostics: parsed.diagnostics,
        cache_kind: None,
        report_status: None,
        timings: None,
    })
}

/// Persistent `-C incremental` reference mode, still one process per edit.
pub fn run_incremental(
    work_root: &Path,
    rustc: &Path,
    fixture: &Fixture,
    category: &str,
    files: &[SourceFile],
) -> Result<ModeRun, String> {
    let base = work_root.join("incremental-mode").join(category);
    let unit = base.join("unit");
    let out = base.join("out");
    let incremental = base.join("incremental");
    std::fs::create_dir_all(&out).map_err(|error| format!("cannot create out dir: {error}"))?;
    let pairs = file_pairs(files)?;
    ravum_daemon::sync_unit_dir(&unit, &pairs)
        .map_err(|error| format!("cannot sync incremental unit: {error}"))?;
    let root = unit.join(&fixture.root);
    let started = Instant::now();
    let output = Command::new(rustc)
        .args(incremental_args(
            &fixture.context,
            &out,
            &incremental,
            &root,
        ))
        .env_remove("RUSTFLAGS")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_BOOTSTRAP")
        .output()
        .map_err(|error| format!("cannot spawn rustc (incremental): {error}"))?;
    let ms = started.elapsed().as_millis() as u64;
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let parsed = parse_rustc_json(&stderr, Some(&unit), output.status.success());
    Ok(ModeRun {
        mode: "incremental",
        ms,
        success: output.status.success() && !parsed.has_errors(),
        exit_code: output.status.code(),
        diagnostics: parsed.diagnostics,
        cache_kind: None,
        report_status: None,
        timings: None,
    })
}

fn cold_args(context: &BuildContext, out: &Path, root: &Path) -> Vec<String> {
    vec![
        format!("--edition={}", context.edition),
        format!("--crate-name={}", context.crate_name),
        format!("--crate-type={}", context.crate_type),
        "--emit=metadata".to_owned(),
        "--error-format=json".to_owned(),
        "--diagnostic-width=120".to_owned(),
        format!("--out-dir={}", out.display()),
        root.display().to_string(),
    ]
}

fn incremental_args(
    context: &BuildContext,
    out: &Path,
    incremental: &Path,
    root: &Path,
) -> Vec<String> {
    let mut args = cold_args(context, out, root);
    args.insert(6, "-C".to_owned());
    args.insert(7, format!("incremental={}", incremental.display()));
    args
}

pub fn file_pairs(files: &[SourceFile]) -> Result<Vec<(String, &str)>, String> {
    let mut pairs: Vec<(String, &str)> = files
        .iter()
        .map(|file| (file.path.clone(), file.content.as_str()))
        .collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(pairs)
}

/// Loads the fixture as `Vec<SourceFile>` (used by every category base).
pub fn base_files(fixture: &Fixture) -> Vec<SourceFile> {
    fixture.files.clone()
}

/// Content identity of a state (first 16 hex chars of SHA-256 over sorted
/// logical paths and contents), quoted in the evidence so a reader can verify
/// that every sample really submitted different bytes.
pub fn content_digest(files: &[SourceFile]) -> String {
    let mut pairs = file_pairs(files).unwrap_or_default();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    let mut hasher_input = Vec::new();
    for (path, content) in pairs {
        hasher_input.extend_from_slice(path.as_bytes());
        hasher_input.push(0);
        hasher_input.extend_from_slice(content.as_bytes());
        hasher_input.push(0);
    }
    ravum_daemon::hash::sha256_hex(&hasher_input)[..16].to_owned()
}

/// Nearest-rank statistics, same rule as the MVP-0a harness.
pub fn stats(values: &[u64]) -> Value {
    if values.is_empty() {
        return json!({"count": 0});
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let percentile = |p: f64| -> u64 {
        let rank = (p / 100.0 * sorted.len() as f64).ceil() as usize;
        sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
    };
    let sum: u64 = sorted.iter().sum();
    json!({
        "count": sorted.len(),
        "min": sorted[0],
        "max": sorted[sorted.len() - 1],
        "mean": (sum as f64) / (sorted.len() as f64),
        "p50": percentile(50.0),
        "p95": percentile(95.0),
    })
}

/// `rustc -vV` parsed as evidence (the same call the daemon uses).
pub fn toolchain_json(repo_root: &Path) -> Result<(ToolchainInfo, Value), String> {
    let rustc = rustc_path();
    let toolchain =
        ToolchainInfo::query(&rustc).map_err(|error| format!("cannot query rustc: {error}"))?;
    let version = Command::new(&rustc)
        .arg("-vV")
        .output()
        .map_err(|error| format!("cannot run rustc -vV: {error}"))?;
    let text = String::from_utf8_lossy(&version.stdout).to_string();
    Ok((
        toolchain,
        json!({
            "rustc_path": rustc.display().to_string(),
            "rustc_vv": text,
            "repo_root": repo_root.display().to_string(),
        }),
    ))
}

/// `rustc` executable: `RAVUM_RUSTC` when set, else `rustc` from `PATH`.
pub fn rustc_path() -> PathBuf {
    std::env::var_os("RAVUM_RUSTC")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("rustc"))
}

/// Machine description and current load.
pub fn machine_json() -> Value {
    json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "family": std::env::consts::FAMILY,
        "logical_parallelism": std::thread::available_parallelism().map(|value| value.get()).unwrap_or(0),
        "cpu_model": std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_else(|_| "unknown".to_owned()),
        "windows_version": windows_version(),
    })
}

fn windows_version() -> String {
    #[cfg(windows)]
    {
        // `cmd /c ver` is a one-shot read of the OS description, no side effect.
        if let Ok(output) = Command::new("cmd").args(["/C", "ver"]).output() {
            return String::from_utf8_lossy(&output.stdout).trim().to_owned();
        }
    }
    "unknown".to_owned()
}

/// Git provenance of the working tree (commit + dirty flag).
pub fn git_json(repo_root: &Path) -> Value {
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
    let status = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(repo_root)
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).to_string())
        .unwrap_or_default();
    let dirty_files = status
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    json!({
        "branch": branch,
        "head": head,
        "dirty_files": dirty_files,
        "dirty_note": "the working tree is shared with other MVP-0b workers; a non-zero dirty count is expected and is not part of this campaign's identity",
    })
}

/// Load snapshot of the host, recorded at category boundaries.
pub fn load_json(calibration_ms: &[u64], rustc: &Path) -> Value {
    let snapshot = ravum_daemon::sysinfo::capture(250);
    let calibration = ravum_daemon::sysinfo::spawn_calibration(rustc, 3);
    json!({
        "system": snapshot,
        "rustc_spawn_calibration": {
            "samples_ms": calibration,
            "stats": stats(&calibration),
            "note": "bare `rustc -vV` spawn: the machine-load floor any analysis measurement sits on (MVP-0a used the same calibration)",
        },
        "previous_calibration": {
            "samples_ms": calibration_ms,
            "stats": stats(calibration_ms),
        },
    })
}

/// Creates the campaign daemon with the measurement configuration.
pub fn open_daemon(work_root: &Path, max_jobs: usize) -> Result<Daemon, String> {
    Daemon::open(DaemonConfig {
        rustc: rustc_path(),
        work_dir: work_root.join("daemon"),
        max_concurrent_jobs: max_jobs,
        max_cache_entries: 512,
        poll_interval_ms: 2,
        analysis_timeout_ms: 120_000,
        toolchain_override: None,
        auto_gc: ravum_daemon::AutoGcPolicy::default(),
    })
    .map_err(|error| format!("cannot open daemon: {error}"))
}

/// Builds an `AnalysisRequest`; the revision counter is supplied by the caller.
pub fn request(
    revision: u64,
    label: String,
    fixture: &Fixture,
    files: &[SourceFile],
) -> ravum_daemon::AnalysisRequest {
    ravum_daemon::AnalysisRequest {
        revision,
        label,
        root: fixture.root.clone(),
        files: files.to_vec(),
        context: fixture.context.clone(),
        priority: ravum_daemon::Priority::Normal,
    }
}

/// Runs one daemon analysis and adapts it to `ModeRun`, keeping both the
/// end-to-end wall time and the daemon-internal timings.
pub fn run_daemon(
    daemon: &Daemon,
    revision: u64,
    label: String,
    fixture: &Fixture,
    files: &[SourceFile],
) -> Result<ModeRun, String> {
    let request = request(revision, label, fixture, files);
    let started = Instant::now();
    let report = daemon
        .analyze(&request)
        .map_err(|error| format!("daemon analyze failed: {error}"))?;
    let wall_ms = started.elapsed().as_millis() as u64;
    Ok(ModeRun {
        mode: "daemon",
        ms: wall_ms,
        success: report.success,
        exit_code: report.exit_code,
        diagnostics: report.diagnostics,
        cache_kind: Some(report.cache.kind.as_str().to_owned()),
        report_status: Some(report.status.as_str().to_owned()),
        timings: Some(report.timings),
    })
}

/// Compares the diagnostic signature sets of several runs. Returns `None` when
/// they agree, or a description of the first mismatch.
pub fn compare_runs(runs: &[&ModeRun]) -> Option<String> {
    let reference = runs.first().map(|run| run.signatures()).unwrap_or_default();
    for run in runs.iter().skip(1) {
        let signatures = run.signatures();
        if signatures != reference {
            return Some(format!(
                "{} produced {} diagnostics different from {}",
                run.mode,
                signatures.len(),
                runs[0].mode
            ));
        }
    }
    None
}

/// Writes a pretty JSON evidence file, creating the parent directory.
pub fn write_evidence(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(value)
        .map_err(|error| format!("cannot serialize evidence: {error}"))?;
    std::fs::write(path, text).map_err(|error| format!("cannot write {}: {error}", path.display()))
}
