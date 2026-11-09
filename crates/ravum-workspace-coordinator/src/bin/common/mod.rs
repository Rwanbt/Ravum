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
//!
//! # Transient failures are not compilation failures (L4.6)
//!
//! A `rustc` process can fail for reasons unrelated to the candidate sources
//! (loader/sysroot metadata under I/O pressure, antivirus, OS-level incident).
//! Such an incident is recognisable here by the **absence of any compiler error
//! code** (`E####`): the diagnostics come from `rustc` but they carry no code and
//! cite no source line. The classification is deliberately conservative:
//!
//! * a non-zero exit **with at least one error code** is a real compilation
//!   failure: `Failed` → `REJECT` (unchanged, fail-closed);
//! * a non-zero exit **without any error code** triggers **one bounded retry**
//!   in a **fresh state directory** (attempt 2); a success at retry is a normal
//!   pass (`SEMANTICALLY_CHECKED` as before, retry noted in the check summary);
//! * a persistent no-code failure after the retry is **not** a compilation
//!   verdict: `VerificationOutcome::Unavailable` → `MANUAL_REQUIRED`, never
//!   `REJECT` (a transient incident does not prove the patch invalid);
//! * a spawn failure stays `Unavailable` (unchanged); nothing is ever integrated
//!   without a successful verification.
//!
//! The exit code and a bounded (≤ 2 KiB) stderr excerpt are kept in the failed
//! check summary / verification reason, so they reach the durable decision
//! journal; the `MergeCheck` schema itself is owned elsewhere and is not changed
//! here.
//!
//! # Test seam: `RAVUM_RUSTC`
//!
//! When the caller does not resolve a specific compiler (the default `rustc`
//! string), `RAVUM_RUSTC` overrides it — the same convention as
//! `ravum-cli`/`ravum-daemon`. This is what makes the deterministic retry tests
//! (`tests/transient_verification.rs`) possible without a network or a second
//! toolchain. If the variable is absent, behaviour is unchanged.

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

/// Maximum stderr bytes kept as evidence (the mandate caps the excerpt at 2 KiB).
const MAX_STDERR_EXCERPT_BYTES: usize = 2 * 1024;

/// One bounded retry: at most two compilation attempts for a no-code failure.
const MAX_METADATA_ATTEMPTS: u32 = 2;

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
    /// Process exit code; `None` when the child was terminated by a signal or
    /// could not be reaped.
    exit_code: Option<i32>,
    /// Bounded (`MAX_STDERR_EXCERPT_BYTES`) single-line excerpt of stderr.
    stderr_excerpt: String,
}

impl Probe {
    /// A real compiler failure carries at least one diagnostic code (`E####`).
    /// No-code errors are treated as possibly transient (see module docs).
    fn has_compiler_error_code(&self) -> bool {
        self.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .code
                .as_deref()
                .map_or(false, |code| !code.is_empty())
        })
    }

    fn exit_text(&self) -> String {
        match self.exit_code {
            Some(code) => code.to_string(),
            None => "unknown".to_owned(),
        }
    }
}

impl RustcMergeVerifier {
    pub fn new(
        rustc: impl Into<PathBuf>,
        work_dir: impl Into<PathBuf>,
        allow_test_execution: bool,
    ) -> Self {
        let rustc = rustc.into();
        // Test/measurement seam: only when the caller did not resolve a specific
        // compiler (default `rustc`) does the project-wide `RAVUM_RUSTC`
        // override apply. An explicit `--rustc`/`RUSTC` value always wins.
        let rustc = if rustc == Path::new("rustc") {
            std::env::var_os("RAVUM_RUSTC")
                .map(PathBuf::from)
                .unwrap_or(rustc)
        } else {
            rustc
        };
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

    /// Scratch directory of one attempt. Attempt 2 uses a **distinct** directory
    /// so the bounded retry starts from a fresh state (no stale output/metadata).
    fn scratch(&self, snapshot: &SourceSnapshot, attempt: u32) -> PathBuf {
        let suffix = if attempt <= 1 {
            String::new()
        } else {
            format!("-retry{}", attempt - 1)
        };
        self.work_dir
            .join("snapshots")
            .join(format!("{}{suffix}", snapshot.id().short()))
    }

    /// Materialise the whole snapshot (safe paths only) into its scratch dir.
    fn materialize(&self, snapshot: &SourceSnapshot, attempt: u32) -> Result<PathBuf, String> {
        let dir = self.scratch(snapshot, attempt);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create scratch dir: {e}"))?;
        snapshot
            .materialize_to(&dir)
            .map_err(|e| format!("cannot materialize candidate snapshot: {e}"))?;
        Ok(dir)
    }

    /// One metadata attempt on a fresh state directory. `previous` carries the
    /// earlier failed attempt when this is the bounded retry, so the summary can
    /// prove the retry and preserve the earlier exit code / stderr excerpt.
    fn metadata_attempt(
        &self,
        request: &VerificationRequest,
        root: &str,
        release: &str,
        host: &str,
        attempt: u32,
        previous: Option<&Probe>,
    ) -> Result<(MergeCheck, Probe), String> {
        let snapshot = &request.snapshot_after;
        let dir = self.materialize(snapshot, attempt)?;
        let probe = self.compile_metadata(&dir, root)?;
        let target = if attempt <= 1 {
            format!("{host} (materialized workspace)")
        } else {
            format!("{host} (materialized workspace, retry {})", attempt - 1)
        };

        let status = if probe.success {
            MergeCheckStatus::Passed
        } else {
            MergeCheckStatus::Failed
        };
        let summary = if probe.success {
            match previous {
                Some(first) => format!(
                    "rustc {release}: candidate snapshot compiles (metadata only; succeeded on bounded retry {} of {MAX_METADATA_ATTEMPTS} after a transient no-code failure: first exit code {}, first stderr excerpt: {})",
                    attempt - 1,
                    first.exit_text(),
                    first.stderr_excerpt
                ),
                None => format!("rustc {release}: candidate snapshot compiles (metadata only)"),
            }
        } else if probe.has_compiler_error_code() {
            format!(
                "rustc {release}: candidate snapshot does not compile ({} error(s); exit code {})",
                probe.diagnostics.len(),
                probe.exit_text()
            )
        } else {
            format!(
                "rustc {release}: no compiler error code after attempt {attempt}/{MAX_METADATA_ATTEMPTS} ({} no-code diagnostic(s); exit code {})",
                probe.diagnostics.len(),
                probe.exit_text()
            )
        };
        let check = MergeCheck::new(
            MergeCheckKind::MetadataCheck,
            status,
            "rustc",
            release.to_owned(),
            target,
            snapshot.id().clone(),
            summary,
            probe.diagnostics.clone(),
        );
        Ok((check, probe))
    }

    fn compile_metadata(&self, dir: &Path, root: &str) -> Result<Probe, String> {
        let out_dir = dir.join(".ravum-metadata");
        std::fs::create_dir_all(&out_dir).map_err(|e| format!("cannot create out dir: {e}"))?;
        let root_path = dir.join(root.replace('/', std::path::MAIN_SEPARATOR_STR));
        let crate_type = if root.ends_with("main.rs") {
            "bin"
        } else {
            "lib"
        };
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
        Ok(Probe {
            diagnostics,
            success,
            exit_code: output.status.code(),
            stderr_excerpt: bounded_stderr_excerpt(output.stderr.as_slice()),
        })
    }

    fn compile_tests(
        &self,
        request: &VerificationRequest,
        root: &str,
        release: &str,
    ) -> Result<MergeCheck, String> {
        let snapshot = &request.snapshot_after;
        let dir = self.materialize(snapshot, 1)?;
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
        let dir = self.materialize(snapshot, 1)?;
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
                .filter(|line| {
                    line.contains("panicked at")
                        || line.contains("FAILED")
                        || line.contains("assertion")
                })
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

        // Attempt 1 (fresh state). A real compile failure (error code present)
        // is definitive; a no-code failure is a possible transient incident and
        // gets exactly one bounded retry in a fresh state directory.
        let (first_check, first_probe) =
            match self.metadata_attempt(request, &root, &release, &host, 1, None) {
                Ok(outcome) => outcome,
                Err(reason) => return VerificationOutcome::Unavailable { reason },
            };
        let metadata = if first_probe.success {
            first_check
        } else if first_probe.has_compiler_error_code() {
            return VerificationOutcome::Failed {
                check: first_check,
                reason: definitive_compile_reason(
                    &request.snapshot_after,
                    &release,
                    1,
                    &first_probe,
                ),
            };
        } else {
            let (retry_check, retry_probe) = match self.metadata_attempt(
                request,
                &root,
                &release,
                &host,
                MAX_METADATA_ATTEMPTS,
                Some(&first_probe),
            ) {
                Ok(outcome) => outcome,
                Err(reason) => return VerificationOutcome::Unavailable { reason },
            };
            if retry_probe.success {
                // Success on the bounded retry is a normal semantic pass; the
                // check summary records the retry and the first failure details.
                retry_check
            } else if retry_probe.has_compiler_error_code() {
                return VerificationOutcome::Failed {
                    check: retry_check,
                    reason: definitive_compile_reason(
                        &request.snapshot_after,
                        &release,
                        MAX_METADATA_ATTEMPTS,
                        &retry_probe,
                    ),
                };
            } else {
                // Persistent no-code failure: a transient/infrastructure incident
                // is not a compilation verdict. Fail closed without REJECT.
                return VerificationOutcome::Unavailable {
                    reason: transient_unavailable_reason(
                        &request.snapshot_after,
                        &release,
                        &first_probe,
                        &retry_probe,
                    ),
                };
            }
        };
        let mut checks = vec![metadata];

        if request.requires(MergeCheckKind::TestCompile)
            || request.requires(MergeCheckKind::TestRun)
        {
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

/// Bounded, single-line stderr excerpt kept as evidence.
///
/// The mandate caps the excerpt at 2 KiB; control characters and newlines are
/// flattened so the excerpt can be journaled in one reason line. Truncation is
/// explicit, never silent.
fn bounded_stderr_excerpt(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let mut out = String::with_capacity(MAX_STDERR_EXCERPT_BYTES);
    let mut used = 0usize;
    let mut truncated = false;
    for character in text.chars() {
        let rendered = if character == '\n' || character == '\r' || character == '\t' {
            ' '
        } else {
            character
        };
        if rendered.is_control() {
            continue;
        }
        let length = rendered.len_utf8();
        if used + length > MAX_STDERR_EXCERPT_BYTES {
            truncated = true;
            break;
        }
        out.push(rendered);
        used += length;
    }
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if truncated {
        format!("{collapsed}…(truncated at {MAX_STDERR_EXCERPT_BYTES} bytes)")
    } else {
        collapsed
    }
}

/// Reason journaled for a definitive compilation failure (at least one `E####`).
fn definitive_compile_reason(
    snapshot: &SourceSnapshot,
    release: &str,
    attempt: u32,
    probe: &Probe,
) -> String {
    let retry_note = if attempt > 1 {
        format!(
            " (compiler error code revealed on bounded retry {} of {MAX_METADATA_ATTEMPTS})",
            attempt - 1
        )
    } else {
        String::new()
    };
    format!(
        "merged snapshot {} does not compile with rustc {release}{retry_note} (exit code {}; stderr excerpt: {})",
        snapshot.id().short(),
        probe.exit_text(),
        probe.stderr_excerpt
    )
}

/// Reason journaled for a persistent **no-code** failure: a transient incident,
/// never a compilation verdict (`Unavailable` → `MANUAL_REQUIRED`, not `REJECT`).
fn transient_unavailable_reason(
    snapshot: &SourceSnapshot,
    release: &str,
    first: &Probe,
    retry: &Probe,
) -> String {
    format!(
        "rustc {release} produced no compiler error code while verifying merged snapshot {} after {MAX_METADATA_ATTEMPTS} attempt(s) with fresh state: transient/infrastructure failure, not a compilation verdict (attempt 1: exit code {}, stderr excerpt: {} | attempt 2: exit code {}, stderr excerpt: {}); no integration without a successful verification",
        snapshot.id().short(),
        first.exit_text(),
        first.stderr_excerpt,
        retry.exit_text(),
        retry.stderr_excerpt
    )
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
        let is_cargo_schema =
            value.get("reason").and_then(|reason| reason.as_str()) == Some("compiler-message");
        let is_flat_schema =
            value.get("$message_type").and_then(|kind| kind.as_str()) == Some("diagnostic");
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
