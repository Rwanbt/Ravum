//! Real `MergeVerifier` implementation shared by the binaries of this package.
//!
//! It is intentionally **not** part of the coordinator library: the library only
//! defines the `MergeVerifier` trait, so it never depends on a compiler or on
//! `ravum-rust-adapter`. The binaries inject this implementation.
//!
//! What is actually run:
//! * `MetadataCheck` — `rustc --emit=metadata` on the exact candidate merged
//!   snapshot, materialised in an owned scratch directory. The toolchain identity
//!   and host triple come from `ravum-rust-adapter` (single source of truth), but
//!   the compilation and the diagnostic parsing are done with the same public
//!   `rustc` CLI: the adapter's `check` parser expects cargo's
//!   `"reason":"compiler-message"` schema, while the rustc 1.95 CLI installed here
//!   emits `"$message_type":"diagnostic"` for direct invocations. That parser
//!   difference is a real finding, recorded in
//!   `docs/mvp/MVP-0b-collab-2-3.md`; harmonising it belongs to the adapter owner.
//! * `TestCompile` / `TestRun` — `rustc --test`, then execution of the produced
//!   test harness. **Opt-in** (`--verify-tests`) because it executes code from
//!   the candidate snapshot; this is not a sandbox (same user, same machine).
//!
//! Nothing here writes outside `work_dir`, and no environment variable of the
//! caller is trusted: `RUSTFLAGS`, `RUSTC_WRAPPER` and friends are removed from
//! child environments so a verdict can be attributed to the declared toolchain.

use ravum_rust_adapter::RustAdapter;
use ravum_snapshot::SourceSnapshot;
use ravum_workspace_coordinator::{
    MergeCheck, MergeCheckKind, MergeCheckStatus, MergeDiagnostic, MergeVerifier,
    VerificationOutcome, VerificationRequest,
};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static RUN_SEQ: AtomicU64 = AtomicU64::new(0);

/// Wall-clock budget for one test-harness execution.
const TEST_TIMEOUT: Duration = Duration::from_secs(30);

pub struct RustcMergeVerifier {
    rustc: PathBuf,
    work_dir: PathBuf,
    allow_test_execution: bool,
    adapter: RustAdapter,
}

/// Outcome of one compilation probe.
struct Probe {
    diagnostics: Vec<MergeDiagnostic>,
    success: bool,
}

impl RustcMergeVerifier {
    pub fn new(
        rustc: impl Into<PathBuf>,
        work_dir: impl Into<PathBuf>,
        allow_test_execution: bool,
    ) -> Self {
        let rustc = rustc.into();
        let work_dir = work_dir.into();
        let adapter =
            RustAdapter::new(rustc.clone(), work_dir.join("adapter")).with_artifact_cache(false);
        Self {
            rustc,
            work_dir,
            allow_test_execution,
            adapter,
        }
    }

    /// Toolchain identity `(release, host)` from the shared adapter. Public so the
    /// measurement binary uses the same attribution as the verifier.
    pub fn toolchain(&self) -> (String, String) {
        match self.adapter.toolchain() {
            Ok(info) => (info.release, info.host),
            Err(_) => ("unknown".to_owned(), "unknown".to_owned()),
        }
    }

    fn scratch(&self, snapshot: &SourceSnapshot) -> PathBuf {
        self.work_dir
            .join("snapshots")
            .join(snapshot.id().short())
    }

    /// Materialise the whole snapshot (safe paths only) into its scratch dir.
    fn materialize(&self, snapshot: &SourceSnapshot) -> Result<PathBuf, String> {
        let dir = self.scratch(snapshot);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create scratch dir: {e}"))?;
        snapshot
            .materialize_to(&dir)
            .map_err(|e| format!("cannot materialize candidate snapshot: {e}"))?;
        Ok(dir)
    }

    fn metadata_check(
        &self,
        request: &VerificationRequest,
        root: &str,
        release: &str,
        host: &str,
    ) -> Result<MergeCheck, String> {
        let snapshot = &request.snapshot_after;
        let dir = self.materialize(snapshot)?;
        let probe = self.compile_metadata(&dir, root)?;
        let target = format!("{host} (materialized workspace)");

        let status = if probe.success {
            MergeCheckStatus::Passed
        } else {
            MergeCheckStatus::Failed
        };
        let summary = if probe.success {
            format!("rustc {release}: candidate snapshot compiles (metadata only)")
        } else {
            format!(
                "rustc {release}: candidate snapshot does not compile ({} error(s))",
                probe.diagnostics.len()
            )
        };
        Ok(MergeCheck::new(
            MergeCheckKind::MetadataCheck,
            status,
            "rustc",
            release.to_owned(),
            target,
            snapshot.id().clone(),
            summary,
            probe.diagnostics,
        ))
    }

    fn compile_metadata(&self, dir: &Path, root: &str) -> Result<Probe, String> {
        let out_dir = dir.join(".ravum-metadata");
        std::fs::create_dir_all(&out_dir).map_err(|e| format!("cannot create out dir: {e}"))?;
        let root_path = dir.join(root.replace('/', std::path::MAIN_SEPARATOR_STR));
        let crate_type = if root.ends_with("main.rs") { "bin" } else { "lib" };
        let output = self.run_rustc(&[
            "--edition".to_owned(),
            "2021".to_owned(),
            "--crate-name".to_owned(),
            "ravum_merge_check".to_owned(),
            "--crate-type".to_owned(),
            crate_type.to_owned(),
            "--emit=metadata".to_owned(),
            "--error-format=json".to_owned(),
            "--out-dir".to_owned(),
            out_dir.to_string_lossy().to_string(),
            root_path.to_string_lossy().to_string(),
        ])?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        let diagnostics = parse_rustc_json_errors(&stderr);
        let success = output.status.success() && diagnostics.is_empty();
        Ok(Probe { diagnostics, success })
    }

    fn compile_tests(
        &self,
        request: &VerificationRequest,
        root: &str,
        release: &str,
    ) -> Result<MergeCheck, String> {
        let snapshot = &request.snapshot_after;
        let dir = self.materialize(snapshot)?;
        let exe = dir.join(format!("tests-{}", RUN_SEQ.fetch_add(1, Ordering::Relaxed)));
        let root_path = dir.join(root.replace('/', std::path::MAIN_SEPARATOR_STR));
        let output = self.run_rustc(&[
            "--edition".to_owned(),
            "2021".to_owned(),
            "--crate-name".to_owned(),
            "ravum_merge_tests".to_owned(),
            "--test".to_owned(),
            "--error-format=json".to_owned(),
            "-o".to_owned(),
            exe.to_string_lossy().to_string(),
            root_path.to_string_lossy().to_string(),
        ])?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        let diagnostics = parse_rustc_json_errors(&stderr);
        let status = if output.status.success() && diagnostics.is_empty() {
            MergeCheckStatus::Passed
        } else {
            MergeCheckStatus::Failed
        };
        let summary = if status == MergeCheckStatus::Passed {
            format!("rustc {release}: test harness compiles")
        } else {
            format!(
                "rustc {release}: test harness does not compile ({} error(s))",
                diagnostics.len()
            )
        };
        Ok(MergeCheck::new(
            MergeCheckKind::TestCompile,
            status,
            "rustc",
            release.to_owned(),
            root.to_owned(),
            snapshot.id().clone(),
            summary,
            diagnostics,
        ))
    }

    fn run_tests(
        &self,
        request: &VerificationRequest,
        root: &str,
        release: &str,
    ) -> Result<MergeCheck, String> {
        let snapshot = &request.snapshot_after;
        let dir = self.materialize(snapshot)?;
        let sequence = RUN_SEQ.fetch_add(1, Ordering::Relaxed);
        let exe = dir.join(format!("tests-run-{sequence}"));
        let root_path = dir.join(root.replace('/', std::path::MAIN_SEPARATOR_STR));
        let compile = self.run_rustc(&[
            "--edition".to_owned(),
            "2021".to_owned(),
            "--crate-name".to_owned(),
            "ravum_merge_tests".to_owned(),
            "--test".to_owned(),
            "--error-format=json".to_owned(),
            "-o".to_owned(),
            exe.to_string_lossy().to_string(),
            root_path.to_string_lossy().to_string(),
        ])?;
        let compile_stderr = String::from_utf8_lossy(&compile.stderr);
        let compile_diagnostics = parse_rustc_json_errors(&compile_stderr);
        if !compile.status.success() || !compile_diagnostics.is_empty() {
            return Ok(MergeCheck::new(
                MergeCheckKind::TestRun,
                MergeCheckStatus::Unavailable,
                "rustc",
                release.to_owned(),
                root.to_owned(),
                snapshot.id().clone(),
                format!(
                    "test harness could not be built ({} error(s)); test execution not attempted",
                    compile_diagnostics.len()
                ),
                compile_diagnostics,
            ));
        }

        let stdout_path = dir.join(format!("tests-stdout-{sequence}.log"));
        let stderr_path = dir.join(format!("tests-stderr-{sequence}.log"));
        let stdout_file = std::fs::File::create(&stdout_path)
            .map_err(|e| format!("cannot capture test stdout: {e}"))?;
        let stderr_file = std::fs::File::create(&stderr_path)
            .map_err(|e| format!("cannot capture test stderr: {e}"))?;
        let mut command = Command::new(&exe);
        command
            .stdout(std::process::Stdio::from(stdout_file))
            .stderr(std::process::Stdio::from(stderr_file))
            .env_remove("RUSTFLAGS")
            .env_remove("RUSTC_WRAPPER");
        let mut child = command
            .spawn()
            .map_err(|e| format!("cannot execute test harness: {e}"))?;
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {
                    if started.elapsed() > TEST_TIMEOUT {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Ok(MergeCheck::new(
                            MergeCheckKind::TestRun,
                            MergeCheckStatus::Unavailable,
                            "rustc",
                            release.to_owned(),
                            root.to_owned(),
                            snapshot.id().clone(),
                            format!(
                                "test harness did not finish within {}s; verdict unknown",
                                TEST_TIMEOUT.as_secs()
                            ),
                            Vec::new(),
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(e) => return Err(format!("cannot wait for test harness: {e}")),
            }
        };
        let stdout = std::fs::read_to_string(&stdout_path).unwrap_or_default();
        let stderr = std::fs::read_to_string(&stderr_path).unwrap_or_default();
        let summary = test_summary(&stdout, &stderr);
        let mut diagnostics = Vec::new();
        if !status.success() {
            for line in stdout
                .lines()
                .chain(stderr.lines())
                .filter(|line| line.contains("panicked at") || line.contains("FAILED") || line.contains("assertion"))
                .take(5)
            {
                diagnostics.push(MergeDiagnostic {
                    code: None,
                    level: "error".to_owned(),
                    message: line.trim().to_owned(),
                    line: None,
                });
            }
        }
        Ok(MergeCheck::new(
            MergeCheckKind::TestRun,
            if status.success() {
                MergeCheckStatus::Passed
            } else {
                MergeCheckStatus::Failed
            },
            "rustc --test",
            release.to_owned(),
            root.to_owned(),
            snapshot.id().clone(),
            summary,
            diagnostics,
        ))
    }

    fn run_rustc(&self, args: &[String]) -> Result<Output, String> {
        Command::new(&self.rustc)
            .args(args)
            .env_remove("RUSTFLAGS")
            .env_remove("RUSTC_WRAPPER")
            .env_remove("RUSTC_WORKSPACE_WRAPPER")
            .output()
            .map_err(|e| format!("failed to spawn {}: {e}", self.rustc.display()))
    }
}

impl MergeVerifier for RustcMergeVerifier {
    fn name(&self) -> &'static str {
        "rustc-cli"
    }

    fn verify(&self, request: &VerificationRequest) -> VerificationOutcome {
        let (release, host) = self.toolchain();
        let Some(root) = infer_crate_root(&request.snapshot_after) else {
            return VerificationOutcome::Unavailable {
                reason: format!(
                    "cannot infer a single crate root for snapshot {} (paths: {:?}); workspace-wide verification is not implemented yet",
                    request.snapshot_after.id().short(),
                    request.snapshot_after.paths().collect::<Vec<_>>()
                ),
            };
        };

        let metadata = match self.metadata_check(request, &root, &release, &host) {
            Ok(check) => check,
            Err(reason) => return VerificationOutcome::Unavailable { reason },
        };
        if metadata.status == MergeCheckStatus::Failed {
            return VerificationOutcome::Failed {
                check: metadata,
                reason: format!(
                    "merged snapshot {} does not compile with rustc {release}",
                    request.snapshot_after.id().short()
                ),
            };
        }
        let mut checks = vec![metadata];

        if request.requires(MergeCheckKind::TestCompile) || request.requires(MergeCheckKind::TestRun) {
            if !self.allow_test_execution {
                return VerificationOutcome::Unavailable {
                    reason: "test execution is disabled (pass --verify-tests to enable it); the change is behavioural and this snapshot ships tests"
                        .to_owned(),
                };
            }
            let compile = match self.compile_tests(request, &root, &release) {
                Ok(check) => check,
                Err(reason) => return VerificationOutcome::Unavailable { reason },
            };
            if compile.status != MergeCheckStatus::Passed {
                return VerificationOutcome::Failed {
                    check: compile,
                    reason: "the test harness of the merged snapshot does not compile".to_owned(),
                };
            }
            checks.push(compile);
            if request.requires(MergeCheckKind::TestRun) {
                let run = match self.run_tests(request, &root, &release) {
                    Ok(check) => check,
                    Err(reason) => return VerificationOutcome::Unavailable { reason },
                };
                if run.status != MergeCheckStatus::Passed {
                    return VerificationOutcome::Failed {
                        check: run,
                        reason: "the test harness of the merged snapshot failed".to_owned(),
                    };
                }
                checks.push(run);
            }
        }

        VerificationOutcome::Passed { checks }
    }
}

/// Pick the file a public `rustc` invocation should compile for this snapshot.
pub fn infer_crate_root(snapshot: &SourceSnapshot) -> Option<String> {
    let paths: Vec<&str> = snapshot.paths().collect();
    if paths.is_empty() {
        return None;
    }
    for candidate in ["src/lib.rs", "lib.rs", "src/main.rs", "main.rs"] {
        if paths.contains(&candidate) {
            return Some(candidate.to_owned());
        }
    }
    let rust_files: Vec<&&str> = paths.iter().filter(|path| path.ends_with(".rs")).collect();
    if rust_files.len() == 1 {
        Some((*rust_files[0]).to_owned())
    } else {
        None
    }
}

/// Parse `--error-format=json` lines into compact diagnostics (errors only).
///
/// Two schemas are accepted because the CLI changes across toolchains:
/// * cargo-wrapped rustc: `{"reason":"compiler-message","message":{...}}`;
/// * direct rustc 1.95 CLI: `{"$message_type":"diagnostic","message":...}` (flat).
/// A schema the parser does not recognise never becomes a silent pass: the
/// failure status of the process still fails the check (see `compile_metadata`).
pub fn parse_rustc_json_errors(stderr: &str) -> Vec<MergeDiagnostic> {
    let mut out = Vec::new();
    for line in stderr.lines() {
        let line = line.trim();
        if !line.starts_with('{') {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let is_cargo_schema = value
            .get("reason")
            .and_then(|reason| reason.as_str())
            == Some("compiler-message");
        let is_flat_schema = value
            .get("$message_type")
            .and_then(|kind| kind.as_str())
            == Some("diagnostic");
        let message_value = if is_cargo_schema {
            match value.get("message") {
                Some(message) => message,
                None => continue,
            }
        } else if is_flat_schema {
            &value
        } else {
            continue;
        };
        if message_value.get("level").and_then(|level| level.as_str()) != Some("error") {
            continue;
        }
        // The "aborting due to N previous errors" summary is not a diagnostic
        // with a code and would duplicate the first line of evidence.
        let code = message_value
            .get("code")
            .and_then(|code| code.get("code"))
            .and_then(|code| code.as_str())
            .map(ToOwned::to_owned);
        let message = message_value
            .get("message")
            .and_then(|message| message.as_str())
            .unwrap_or("")
            .to_owned();
        if code.is_none() && message.starts_with("aborting due to") {
            continue;
        }
        let line_number = message_value
            .get("spans")
            .and_then(|spans| spans.get(0))
            .and_then(|span| span.get("line_start"))
            .and_then(|line| line.as_u64())
            .map(|line| line as u32);
        out.push(MergeDiagnostic {
            code,
            level: "error".to_owned(),
            message,
            line: line_number,
        });
        if out.len() >= 20 {
            break;
        }
    }
    out
}

fn test_summary(stdout: &str, stderr: &str) -> String {
    let result_line = stdout
        .lines()
        .chain(stderr.lines())
        .find(|line| line.trim_start().starts_with("test result:"))
        .map(|line| line.trim().to_owned());
    match result_line {
        Some(line) => format!("test harness executed: {line}"),
        None => "test harness executed: no `test result:` line observed".to_owned(),
    }
}





