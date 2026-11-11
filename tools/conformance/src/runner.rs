//! Case execution: toolchain resolution, isolated copies, timeouts, honest
//! PASS/FAIL/SKIP/NOT_RUN evaluation with evidence.

use crate::case::{CaseEntry, CaseSpec, Fixture, FixtureKind, Mode};
use crate::json::Json;
use crate::util::{host_os, read_capped, truncate_chars, Sanitizer};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Maximum bytes captured from each stream before truncation.
const CAPTURE_CAP: u64 = 256 * 1024;
/// Polling interval while waiting for a child process.
const POLL_INTERVAL: Duration = Duration::from_millis(5);
/// Timeout used when probing `ravum-lang --version`.
const VERSION_PROBE_TIMEOUT_MS: u64 = 2_000;
/// Exit code of `ravum-lang` when an external tool is missing (`EXIT_TOOL` in
/// `crates/ravum-lang-cli/src/util.rs`). `build` returns it when `rustc` is not
/// installed: that is an environment failure, not a compiler diagnostic.
const TOOL_MISSING_EXIT_CODE: i32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Pass,
    Fail,
    Skip,
    NotRun,
}

impl Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Pass => "pass",
            Status::Fail => "fail",
            Status::Skip => "skip",
            Status::NotRun => "not_run",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Status::Pass => "PASS",
            Status::Fail => "FAIL",
            Status::Skip => "SKIP",
            Status::NotRun => "NOT_RUN",
        }
    }
}

/// Status of the `check ⇒ build` dimension for one case.
///
/// A report produced without `--verify-build` must never be citable as evidence
/// of build correctness: every case then carries [`BuildStatus::NotRequested`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildStatus {
    /// `--verify-build` was not requested: build correctness is not claimed.
    NotRequested,
    /// The mode never builds by construction (`compile-fail`, `cli`).
    NotApplicable,
    /// The case did not reach a passing verification step (failed check, skip,
    /// missing toolchain, invalid case): the build dimension is NOT measured.
    NotAttempted,
    /// An explicit `ravum-lang build` ran after a passing check and exited 0.
    Verified,
    /// The mode compiled and executed the program (`run-pass`/`run-fail`), and
    /// the expected run outcome was observed: the build necessarily succeeded.
    Implicit,
    /// The check passed but the build failed: a `check ⇒ build` divergence.
    Failed,
    /// The build could not be evaluated in this environment (missing `rustc`,
    /// spawn failure, timeout). Never a PASS.
    Environment,
}

impl BuildStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            BuildStatus::NotRequested => "not_requested",
            BuildStatus::NotApplicable => "not_applicable",
            BuildStatus::NotAttempted => "not_attempted",
            BuildStatus::Verified => "verified",
            BuildStatus::Implicit => "implicit",
            BuildStatus::Failed => "failed",
            BuildStatus::Environment => "environment",
        }
    }

    /// True when this status is positive evidence that the program builds.
    #[cfg(test)]
    pub fn is_build_evidence(&self) -> bool {
        matches!(self, BuildStatus::Verified | BuildStatus::Implicit)
    }
}

#[derive(Debug, Clone)]
pub struct CaseResult {
    pub id: String,
    pub taxonomy: String,
    pub mode: Mode,
    pub status: Status,
    pub duration_ms: u128,
    pub expected: String,
    pub actual: String,
    pub detail: String,
    /// Status of the build dimension (see [`BuildStatus`]).
    pub build: BuildStatus,
    /// Evidence for the build dimension: both check and build outputs on a
    /// divergence, or the environment reason when it could not be measured.
    pub build_detail: String,
}

#[derive(Debug, Clone)]
pub struct Toolchain {
    pub path: PathBuf,
    pub display: String,
    pub version: String,
}

pub struct RunContext<'a> {
    pub toolchain: Option<Toolchain>,
    pub toolchain_error: String,
    pub timeout_ms: u64,
    pub work_dir: PathBuf,
    pub sanitizer: &'a Sanitizer,
    /// When true, a passing check is followed by an explicit `build`.
    pub verify_build: bool,
}

pub fn locate_toolchain(explicit: Option<&str>, repo: &Path) -> Result<PathBuf, String> {
    let (requested, origin) = if let Some(explicit) = explicit {
        (Some(PathBuf::from(explicit)), "--toolchain".to_string())
    } else if let Some(env) = std::env::var_os("RAVUM_LANG").filter(|value| !value.is_empty()) {
        (Some(PathBuf::from(env)), "RAVUM_LANG".to_string())
    } else {
        (None, String::new())
    };

    if let Some(requested) = requested {
        let resolved = if requested.is_absolute() {
            requested.clone()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| repo.to_path_buf())
                .join(&requested)
        };
        if resolved.is_file() {
            return Ok(resolved);
        }
        return Err(format!(
            "toolchain not found (from {origin}): {}",
            requested.display()
        ));
    }

    let candidate = repo
        .join("target")
        .join("debug")
        .join(format!("ravum-lang{}", std::env::consts::EXE_SUFFIX));
    if candidate.is_file() {
        Ok(candidate)
    } else {
        Err(format!(
            "toolchain not found: target/debug/ravum-lang{} (build it with `cargo build -p ravum-lang-cli`, or pass --toolchain / set RAVUM_LANG)",
            std::env::consts::EXE_SUFFIX
        ))
    }
}

pub fn probe_version(toolchain: &Path, repo: &Path, scratch: &Path) -> String {
    let env = Env {
        toolchain,
        work_dir: scratch,
        timeout_ms: VERSION_PROBE_TIMEOUT_MS,
        counter: Cell::new(0),
    };
    let outcome = env.exec(&["--version".to_string()], repo, "");
    if outcome.spawn_error.is_none() && !outcome.timed_out && outcome.exit == Some(0) {
        let line = outcome.stdout.lines().next().unwrap_or("").trim();
        if !line.is_empty() {
            return truncate_chars(line, 200);
        }
    }
    "UNKNOWN".to_string()
}

struct Verdict {
    status: Status,
    expected: String,
    actual: String,
    detail: String,
}

impl Verdict {
    fn pass(expected: impl Into<String>, actual: impl Into<String>) -> Verdict {
        Verdict {
            status: Status::Pass,
            expected: expected.into(),
            actual: actual.into(),
            detail: String::new(),
        }
    }

    fn fail(
        expected: impl Into<String>,
        actual: impl Into<String>,
        detail: impl Into<String>,
    ) -> Verdict {
        Verdict {
            status: Status::Fail,
            expected: expected.into(),
            actual: actual.into(),
            detail: detail.into(),
        }
    }

    fn skip(reason: impl Into<String>) -> Verdict {
        Verdict {
            status: Status::Skip,
            expected: "run".to_string(),
            actual: "skipped".to_string(),
            detail: reason.into(),
        }
    }
}

/// Build dimension for a case that was not measured, with the reason. When the
/// flag is absent the report must stay silent about build correctness.
fn not_measured(verify_build: bool, reason: impl Into<String>) -> (BuildStatus, String) {
    if verify_build {
        (BuildStatus::NotAttempted, reason.into())
    } else {
        (BuildStatus::NotRequested, String::new())
    }
}

pub fn run_case(entry: &CaseEntry, context: &RunContext, index: usize) -> CaseResult {
    let started = Instant::now();
    let finish = |verdict: Verdict,
                  build: BuildStatus,
                  build_detail: String,
                  started: Instant| CaseResult {
        id: entry.id.clone(),
        taxonomy: entry.taxonomy.clone(),
        mode: entry.mode(),
        status: verdict.status,
        duration_ms: started.elapsed().as_millis(),
        expected: context.sanitizer.apply(&verdict.expected),
        actual: context.sanitizer.apply(&verdict.actual),
        detail: context.sanitizer.apply(&verdict.detail),
        build,
        build_detail: context.sanitizer.apply(&build_detail),
    };

    let spec = match &entry.spec {
        Ok(spec) => spec,
        Err(message) => {
            let (build, build_detail) = not_measured(
                context.verify_build,
                "the case specification is invalid: build not measured",
            );
            return finish(
                Verdict::fail(
                    "a valid case.json",
                    "invalid case specification",
                    format!("{message}\ncase directory: {}", entry.rel_dir),
                ),
                build,
                build_detail,
                started,
            );
        }
    };

    if spec.skip_on.iter().any(|os| os == host_os()) {
        let reason = if spec.notes.is_empty() {
            format!("skip_on includes the host OS `{}`", host_os())
        } else {
            format!(
                "skip_on includes the host OS `{}`; note: {}",
                host_os(),
                spec.notes
            )
        };
        let (build, build_detail) = not_measured(
            context.verify_build,
            format!("the case is skipped on `{}`: build not measured", host_os()),
        );
        return finish(Verdict::skip(reason), build, build_detail, started);
    }

    let Some(toolchain) = context.toolchain.as_ref() else {
        let (build, build_detail) = not_measured(
            context.verify_build,
            "no executable toolchain: the build dimension was NOT measured",
        );
        return finish(
            Verdict {
                status: Status::NotRun,
                expected: "an executable toolchain".to_string(),
                actual: "toolchain unavailable".to_string(),
                detail: context.toolchain_error.clone(),
            },
            build,
            build_detail,
            started,
        );
    };

    // 1. Isolated copy of the case (the toolchain may write or format files).
    // The directory name is deliberately short: it only needs to be unique per
    // run, and a long case id would push the generated object paths towards
    // Windows' MAX_PATH, where MSVC `link.exe` fails with an opaque LNK1181.
    let copy_root = context.work_dir.join(format!("{:04}", index));
    if let Err(error) = copy_tree(&entry.dir, &copy_root) {
        let (build, build_detail) = not_measured(
            context.verify_build,
            "the isolated copy could not be prepared: build not measured",
        );
        return finish(
            Verdict::fail(
                "an isolated copy of the case",
                "could not prepare the isolated copy",
                error,
            ),
            build,
            build_detail,
            started,
        );
    }

    let environment = Env {
        toolchain: &toolchain.path,
        work_dir: &context.work_dir,
        timeout_ms: spec.timeout_ms.unwrap_or(context.timeout_ms),
        counter: Cell::new(0),
    };

    // 2. Declarative fixtures (e.g. symlinks that git cannot always store).
    if let Err(failure) = apply_fixtures(&copy_root, &spec.fixtures) {
        return match failure {
            FixtureFailure::Environment(message) => {
                let (build, build_detail) = not_measured(
                    context.verify_build,
                    "a case fixture could not be created in this environment: build not measured",
                );
                finish(
                    Verdict::skip(format!(
                        "cannot create a case fixture in this environment: {message}"
                    )),
                    build,
                    build_detail,
                    started,
                )
            }
            FixtureFailure::CaseDefinition(message) => {
                let (build, build_detail) = not_measured(
                    context.verify_build,
                    "the case fixtures are invalid: build not measured",
                );
                finish(
                    Verdict::fail(
                        "valid case fixtures",
                        "case fixtures could not be materialized",
                        message,
                    ),
                    build,
                    build_detail,
                    started,
                )
            }
        };
    }

    // 3. Resolve the project root inside the copy and refuse any escape.
    let destination = match fs::canonicalize(&copy_root) {
        Ok(path) => path,
        Err(error) => {
            let (build, build_detail) = not_measured(
                context.verify_build,
                "the isolated copy could not be canonicalized: build not measured",
            );
            return finish(
                Verdict::fail(
                    "an isolated copy of the case",
                    "could not canonicalize the isolated copy",
                    error.to_string(),
                ),
                build,
                build_detail,
                started,
            );
        }
    };
    let project = match fs::canonicalize(copy_root.join(&spec.project)) {
        Ok(path) => path,
        Err(error) => {
            let (build, build_detail) = not_measured(
                context.verify_build,
                "the case has no valid project root: build not measured",
            );
            return finish(
                Verdict::fail(
                    format!("project `{}` inside the case", spec.project),
                    "project path does not exist",
                    error.to_string(),
                ),
                build,
                build_detail,
                started,
            );
        }
    };
    if !project.starts_with(&destination) {
        let (build, build_detail) = not_measured(
            context.verify_build,
            "the case project escapes the isolated copy: build not measured",
        );
        return finish(
            Verdict::fail(
                "the project stays inside the isolated copy",
                "project path escapes the isolated copy",
                format!("project `{}`", spec.project),
            ),
            build,
            build_detail,
            started,
        );
    }

    let evaluation = evaluate(
        spec,
        &environment,
        &copy_root,
        &project,
        &entry.rel_dir,
        context.verify_build,
    );
    finish(
        evaluation.verdict,
        evaluation.build,
        evaluation.build_detail,
        started,
    )
}

/// Outcome of one case: the mode verdict plus the `check ⇒ build` dimension.
struct Evaluation {
    verdict: Verdict,
    build: BuildStatus,
    build_detail: String,
}

impl Evaluation {
    fn new(verdict: Verdict, build: BuildStatus, build_detail: impl Into<String>) -> Evaluation {
        Evaluation {
            verdict,
            build,
            build_detail: build_detail.into(),
        }
    }

    /// A mode that never builds by construction (`compile-fail`, `cli`).
    fn not_applicable(verdict: Verdict, verify_build: bool, reason: &str) -> Evaluation {
        if verify_build {
            Evaluation::new(verdict, BuildStatus::NotApplicable, reason)
        } else {
            Evaluation::new(verdict, BuildStatus::NotRequested, "")
        }
    }

    /// A case whose own verification did not reach a passing state.
    fn not_attempted(verdict: Verdict, verify_build: bool, reason: &str) -> Evaluation {
        if verify_build {
            Evaluation::new(verdict, BuildStatus::NotAttempted, reason)
        } else {
            Evaluation::new(verdict, BuildStatus::NotRequested, "")
        }
    }
}

fn evaluate(
    spec: &CaseSpec,
    env: &Env<'_>,
    copy_root: &Path,
    project: &Path,
    rel_dir: &str,
    verify_build: bool,
) -> Evaluation {
    let project_arg = spec.project.clone();
    match spec.mode {
        Mode::CompilePass => {
            let mut args = vec!["check".to_string(), project_arg.clone()];
            args.extend(spec.args.iter().cloned());
            let outcome = env.exec(&args, copy_root, "");
            let verdict = evaluate_compile_pass(spec, &outcome, rel_dir);
            verify_build_after_check(env, copy_root, &project_arg, &outcome, verdict, verify_build)
        }
        Mode::CompileFail => {
            let mut args = vec!["check".to_string(), "--json".to_string(), project_arg];
            args.extend(spec.args.iter().cloned());
            let outcome = env.exec(&args, copy_root, "");
            let verdict = evaluate_compile_fail(spec, &outcome);
            // A compile-fail program is invalid by construction: a successful
            // build would contradict the case, so build success is not the
            // contract dimension for this mode.
            Evaluation::not_applicable(
                verdict,
                verify_build,
                "compile-fail programs are check-only by construction: build success is not the contract for this mode",
            )
        }
        Mode::RunPass | Mode::RunFail => {
            let mut args = vec!["run".to_string(), project_arg];
            if !spec.args.is_empty() {
                args.push("--".to_string());
                args.extend(spec.args.iter().cloned());
            }
            let outcome = env.exec(&args, copy_root, &spec.stdin);
            let verdict = evaluate_run(spec, &outcome, project);
            if !verify_build {
                return Evaluation::new(verdict, BuildStatus::NotRequested, "");
            }
            if verdict.status == Status::Pass {
                // `run` compiles the program before executing it: an expected
                // run outcome is proof that the build succeeded.
                Evaluation::new(
                    verdict,
                    BuildStatus::Implicit,
                    "`run` compiled and executed the program; the expected run outcome proves the build succeeded",
                )
            } else {
                Evaluation::not_attempted(
                    verdict,
                    verify_build,
                    "the run did not pass, so the build dimension was not measured",
                )
            }
        }
        Mode::FmtIdempotent => {
            let (verdict, check) = evaluate_fmt(spec, env, copy_root, &project_arg);
            match check {
                Some(check) => verify_build_after_check(
                    env,
                    copy_root,
                    &project_arg,
                    &check,
                    verdict,
                    verify_build,
                ),
                None => Evaluation::not_attempted(
                    verdict,
                    verify_build,
                    "the formatting contract did not pass, so the build dimension was not measured",
                ),
            }
        }
        Mode::Cli => {
            let args: Vec<String> = spec
                .args
                .iter()
                .map(|arg| arg.replace("{project}", ".").replace("{case}", "."))
                .collect();
            let outcome = env.exec(&args, project, &spec.stdin);
            let verdict = evaluate_cli(spec, &outcome, project);
            Evaluation::not_applicable(
                verdict,
                verify_build,
                "cli cases drive arbitrary commands: build success is not the contract for this mode",
            )
        }
    }
}

/// Runs the explicit `build` step after a passing check and folds it into the
/// verdict. This is the `check ⇒ build` contract:
///
/// * build exit 0 → PASS, build `verified`;
/// * build compiler failure → FAIL with **both** check and build outputs;
/// * missing `rustc` / spawn failure / timeout → NOT_RUN, never a PASS.
fn verify_build_after_check(
    env: &Env<'_>,
    copy_root: &Path,
    project: &str,
    check: &ExecOutcome,
    verdict: Verdict,
    verify_build: bool,
) -> Evaluation {
    if !verify_build {
        return Evaluation::new(verdict, BuildStatus::NotRequested, "");
    }
    if verdict.status != Status::Pass {
        return Evaluation::not_attempted(
            verdict,
            verify_build,
            "the check did not pass, so the build dimension was not measured",
        );
    }
    // `--profile` is a check-only flag: `build` has no such option and always
    // analyzes with the Core profile. Case `args` are therefore NOT forwarded.
    let build_args = vec!["build".to_string(), project.to_string()];
    let build = env.exec(&build_args, copy_root, "");
    let (status, detail) = classify_build(check, &build);
    let verdict = apply_build(verdict, status, &detail);
    Evaluation::new(verdict, status, detail)
}

/// Classifies an explicit `build` invocation that followed a passing `check`.
/// The detail always carries enough evidence to re-read the decision.
fn classify_build(check: &ExecOutcome, build: &ExecOutcome) -> (BuildStatus, String) {
    if let Some(error) = &build.spawn_error {
        return (
            BuildStatus::Environment,
            format!(
                "the build step could not be started: {error}\n\
                 environment note: this is a harness/toolchain failure, not a Ravum compiler \
                 diagnostic; the case is NOT_RUN and never a PASS\n{}",
                render_outputs(build)
            ),
        );
    }
    if build.timed_out {
        return (
            BuildStatus::Environment,
            format!(
                "the build step exceeded the timeout and was killed: the build dimension is NOT \
                 measured (not a compiler diagnostic, not a PASS)\n{}",
                render_outputs(build)
            ),
        );
    }
    match build.exit {
        Some(0) => (
            BuildStatus::Verified,
            "build exit 0 after check exit 0".to_string(),
        ),
        Some(code) if code == TOOL_MISSING_EXIT_CODE => (
            BuildStatus::Environment,
            format!(
                "the build step exits {code}: the external Rust compiler (`rustc`) is unavailable in \
                 this environment\nenvironment note: this is a toolchain/environment failure, not a \
                 Ravum diagnostic and not a PASS; the case is NOT_RUN\n{}",
                render_outputs(build)
            ),
        ),
        Some(code) => (
            BuildStatus::Failed,
            format!(
                "check passed (exit 0) but build failed (exit {code}): this is a check ⇒ build \
                 divergence\ncheck output:\n{}\nbuild output:\n{}",
                render_outputs(check),
                render_outputs(build)
            ),
        ),
        None => (
            BuildStatus::Environment,
            format!(
                "the build process terminated without an exit code (killed by the OS or a signal): \
                 the build dimension is NOT measured\n{}",
                render_outputs(build)
            ),
        ),
    }
}

/// Folds the build dimension into the verdict. A check-passing case whose build
/// fails must FAIL; a case whose build could not be evaluated must be NOT_RUN.
fn apply_build(verdict: Verdict, build: BuildStatus, detail: &str) -> Verdict {
    match build {
        BuildStatus::Failed if verdict.status == Status::Pass => Verdict {
            status: Status::Fail,
            expected: "check exit 0 and build exit 0".to_string(),
            actual: "check exit 0; build failed".to_string(),
            detail: detail.to_string(),
        },
        BuildStatus::Environment if verdict.status == Status::Pass => Verdict {
            status: Status::NotRun,
            expected: "check exit 0 and build exit 0".to_string(),
            actual: "check exit 0; build not evaluable in this environment".to_string(),
            detail: detail.to_string(),
        },
        _ => verdict,
    }
}


fn evaluate_compile_pass(spec: &CaseSpec, outcome: &ExecOutcome, rel_dir: &str) -> Verdict {
    let expected = describe_expectations("check exit 0", spec);
    let actual = describe_outcome("check", outcome);
    if let Some(failure) = execution_failure(outcome, 0) {
        return Verdict::fail(
            expected,
            actual,
            format!("case: {rel_dir}\n{failure}\n{}", render_outputs(outcome)),
        );
    }
    if let Some(message) = contains_failures(spec, outcome) {
        return Verdict::fail(
            expected,
            actual,
            format!("case: {rel_dir}\n{message}\n{}", render_outputs(outcome)),
        );
    }
    Verdict::pass(expected, actual)
}

fn evaluate_compile_fail(spec: &CaseSpec, outcome: &ExecOutcome) -> Verdict {
    let expected_codes: BTreeSet<String> = spec.expect_codes.iter().cloned().collect();
    let expected = if expected_codes.is_empty() {
        "check --json exit 1 with at least one E-* diagnostic code".to_string()
    } else {
        format!(
            "check --json exit 1; diagnostic codes exactly {{{}}}",
            expected_codes
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let extraction = extract_codes(&outcome.stdout, &outcome.stderr);
    let actual = format!(
        "{}; codes = {{{}}} (source: {})",
        describe_outcome("check --json", outcome),
        extraction
            .codes
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join(", "),
        extraction.source
    );

    if let Some(failure) = execution_failure(outcome, 1) {
        return Verdict::fail(
            expected,
            actual,
            format!("{failure}\n{}", render_outputs(outcome)),
        );
    }
    if let Some(reason) = &extraction.structured_error {
        return Verdict::fail(
            expected,
            actual,
            format!(
                "`check --json` stdout is not structured JSON: {reason}\n{}",
                render_outputs(outcome)
            ),
        );
    }

    let missing: Vec<String> = expected_codes
        .iter()
        .filter(|code| !extraction.codes.contains(*code))
        .cloned()
        .collect();
    let unexpected: Vec<String> = extraction
        .codes
        .iter()
        .filter(|code| !expected_codes.contains(*code))
        .cloned()
        .collect();
    if expected_codes.is_empty() && extraction.codes.is_empty() {
        return Verdict::fail(
            expected,
            actual,
            format!(
                "exit code is 1 but no E-* diagnostic code was found; a compile failure must name its diagnostic\n{}",
                render_outputs(outcome)
            ),
        );
    }
    if missing.is_empty() && unexpected.is_empty() {
        return Verdict::pass(expected, actual);
    }
    let mut detail = String::new();
    if !missing.is_empty() {
        detail.push_str(&format!(
            "missing diagnostic codes: {}\n",
            missing.join(", ")
        ));
    }
    if !unexpected.is_empty() {
        detail.push_str(&format!(
            "unexpected diagnostic codes: {}\n",
            unexpected.join(", ")
        ));
    }
    detail.push_str(&render_outputs(outcome));
    Verdict::fail(expected, actual, detail)
}

fn evaluate_run(spec: &CaseSpec, outcome: &ExecOutcome, project: &Path) -> Verdict {
    let want_exit = spec
        .exit_code
        .or_else(|| spec.mode.default_exit_code())
        .unwrap_or(0);
    let expected = describe_expectations(&format!("run exit {want_exit}"), spec);
    let actual = describe_outcome("run", outcome);
    if let Some(failure) = execution_failure(outcome, want_exit) {
        return Verdict::fail(
            expected,
            actual,
            format!("{failure}\n{}", render_outputs(outcome)),
        );
    }
    if let Some(message) = contains_failures(spec, outcome) {
        return Verdict::fail(
            expected,
            actual,
            format!("{message}\n{}", render_outputs(outcome)),
        );
    }
    if let Some(message) = check_expected_files(spec, project) {
        return Verdict::fail(expected, actual, message);
    }
    if let Some(message) = check_json_schema(spec, &outcome.stdout) {
        return Verdict::fail(expected, actual, message);
    }
    Verdict::pass(expected, actual)
}

fn evaluate_cli(spec: &CaseSpec, outcome: &ExecOutcome, project: &Path) -> Verdict {
    let want_exit = spec
        .exit_code
        .or_else(|| spec.mode.default_exit_code())
        .unwrap_or(0);
    let expected = describe_expectations(&format!("exit {want_exit}"), spec);
    let actual = describe_outcome("cli", outcome);
    if let Some(failure) = execution_failure(outcome, want_exit) {
        return Verdict::fail(
            expected,
            actual,
            format!("{failure}\n{}", render_outputs(outcome)),
        );
    }
    if let Some(message) = contains_failures(spec, outcome) {
        return Verdict::fail(
            expected,
            actual,
            format!("{message}\n{}", render_outputs(outcome)),
        );
    }
    if let Some(message) = check_expected_files(spec, project) {
        return Verdict::fail(expected, actual, message);
    }
    if let Some(message) = check_json_schema(spec, &outcome.stdout) {
        return Verdict::fail(expected, actual, message);
    }
    Verdict::pass(expected, actual)
}

/// Verifies the formatting contract. On success, also returns the passing
/// `check` output so `--verify-build` can run its explicit `build` step.
fn evaluate_fmt(
    spec: &CaseSpec,
    env: &Env<'_>,
    copy_root: &Path,
    project: &str,
) -> (Verdict, Option<ExecOutcome>) {
    let expected = "fmt twice -> identical .rv sources, then check exit 0".to_string();
    let fmt_args = vec!["fmt".to_string(), project.to_string()];

    let first = env.exec(&fmt_args, copy_root, "");
    if let Some(failure) = execution_failure(&first, 0) {
        return (
            Verdict::fail(
                expected,
                describe_outcome("fmt (1st)", &first),
                format!("first fmt failed\n{failure}\n{}", render_outputs(&first)),
            ),
            None,
        );
    }
    let snapshot_one = match snapshot_rv(&copy_root.join(project)) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return (
                Verdict::fail(expected, "could not snapshot .rv files".to_string(), error),
                None,
            )
        }
    };
    let second = env.exec(&fmt_args, copy_root, "");
    if let Some(failure) = execution_failure(&second, 0) {
        return (
            Verdict::fail(
                expected,
                describe_outcome("fmt (2nd)", &second),
                format!("second fmt failed\n{failure}\n{}", render_outputs(&second)),
            ),
            None,
        );
    }
    let snapshot_two = match snapshot_rv(&copy_root.join(project)) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return (
                Verdict::fail(expected, "could not snapshot .rv files".to_string(), error),
                None,
            )
        }
    };
    let differences = diff_snapshots(&snapshot_one, &snapshot_two);
    if !differences.is_empty() {
        return (
            Verdict::fail(
                expected,
                format!("second fmt changed {} .rv file(s)", differences.len()),
                format!(
                    "fmt is not idempotent; files changed by the second run:\n  {}",
                    differences.join("\n  ")
                ),
            ),
            None,
        );
    }
    let check = env.exec(&["check".to_string(), project.to_string()], copy_root, "");
    if let Some(failure) = execution_failure(&check, 0) {
        return (
            Verdict::fail(
                expected,
                describe_outcome("check", &check),
                format!(
                    "fmt output does not pass check\n{failure}\n{}",
                    render_outputs(&check)
                ),
            ),
            None,
        );
    }
    if let Some(message) = contains_failures(spec, &check) {
        return (
            Verdict::fail(expected, describe_outcome("check", &check), message),
            None,
        );
    }
    (
        Verdict::pass(
            expected,
            format!(
                "fmt x2 identical ({} .rv files), check exit 0",
                snapshot_two.len()
            ),
        ),
        Some(check),
    )
}

fn describe_expectations(base: &str, spec: &CaseSpec) -> String {
    let mut text = base.to_string();
    if let Some(contains) = &spec.stdout_contains {
        text.push_str(&format!("; stdout contains {:?}", contains));
    }
    if let Some(contains) = &spec.stderr_contains {
        text.push_str(&format!("; stderr contains {:?}", contains));
    }
    if !spec.expect_files.is_empty() {
        let names: Vec<String> = spec
            .expect_files
            .iter()
            .map(|file| match &file.contains {
                Some(contains) => format!("{} contains {:?}", file.path, contains),
                None => format!("{} exists", file.path),
            })
            .collect();
        text.push_str(&format!("; files: {}", names.join(", ")));
    }
    if let Some(version) = spec.expect_json_schema_version {
        text.push_str(&format!("; stdout JSON schema_version {version}"));
    }
    text
}

fn describe_outcome(label: &str, outcome: &ExecOutcome) -> String {
    if let Some(error) = &outcome.spawn_error {
        return format!("{label}: could not start the toolchain ({error})");
    }
    if outcome.timed_out {
        return format!("{label}: timed out");
    }
    let exit = match outcome.exit {
        Some(code) => code.to_string(),
        None => "no exit code".to_string(),
    };
    format!(
        "{label}: exit {exit}; stdout {} B; stderr {} B",
        outcome.stdout.len(),
        outcome.stderr.len()
    )
}

/// Returns a human-readable failure when the execution itself is not the
/// expected one (spawn error, timeout, crash, wrong exit code).
fn execution_failure(outcome: &ExecOutcome, want_exit: i32) -> Option<String> {
    if let Some(error) = &outcome.spawn_error {
        return Some(format!("the toolchain could not be started: {error}"));
    }
    if outcome.timed_out {
        return Some("the toolchain exceeded the timeout and was killed".to_string());
    }
    match outcome.exit {
        Some(code) if code == want_exit => None,
        Some(code) => Some(format!(
            "exit code mismatch: expected {want_exit}, got {code}"
        )),
        None => Some(
            "process terminated without an exit code (killed by the OS or a signal)".to_string(),
        ),
    }
}

fn contains_failures(spec: &CaseSpec, outcome: &ExecOutcome) -> Option<String> {
    let mut problems = Vec::new();
    if let Some(expected) = &spec.stdout_contains {
        if !outcome.stdout.contains(expected) {
            problems.push(format!(
                "stdout does not contain {:?}; stdout was:\n{}",
                expected,
                truncate_chars(&outcome.stdout, 2_000)
            ));
        }
    }
    if let Some(expected) = &spec.stderr_contains {
        if !outcome.stderr.contains(expected) {
            problems.push(format!(
                "stderr does not contain {:?}; stderr was:\n{}",
                expected,
                truncate_chars(&outcome.stderr, 2_000)
            ));
        }
    }
    if problems.is_empty() {
        None
    } else {
        Some(problems.join("\n"))
    }
}

fn check_expected_files(spec: &CaseSpec, project: &Path) -> Option<String> {
    let mut problems = Vec::new();
    for expected in &spec.expect_files {
        let path = project.join(&expected.path);
        if !path.exists() {
            problems.push(format!("expected file is missing: {}", expected.path));
            continue;
        }
        if let Some(needle) = &expected.contains {
            match read_capped(&path, 1_048_576) {
                Ok((text, _truncated, _len)) => {
                    if !text.contains(needle) {
                        problems.push(format!(
                            "expected file {} does not contain {:?}; content was:\n{}",
                            expected.path,
                            needle,
                            truncate_chars(&text, 1_500)
                        ));
                    }
                }
                Err(error) => problems.push(format!(
                    "expected file {} could not be read: {error}",
                    expected.path
                )),
            }
        }
    }
    if problems.is_empty() {
        None
    } else {
        Some(problems.join("\n"))
    }
}

fn check_json_schema(spec: &CaseSpec, stdout: &str) -> Option<String> {
    let Some(want) = spec.expect_json_schema_version else {
        return None;
    };
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Some("stdout is empty; expected a JSON report".to_string());
    }
    let value = match Json::parse(trimmed) {
        Ok(value) => value,
        Err(error) => {
            return Some(format!(
                "stdout is not a single JSON object (DIAG-7.2): {error}"
            ))
        }
    };
    if value.as_object().is_none() {
        return Some("the JSON root must be an object".to_string());
    }
    let mut problems: Vec<String> = Vec::new();
    match value.get("schema_version").and_then(Json::as_i64) {
        Some(found) if found == want => {}
        Some(found) => problems.push(format!("schema_version {found} != {want}")),
        None => problems.push("missing integer `schema_version`".to_string()),
    }
    for key in ["tool", "tool_version", "spec_version", "command"] {
        if value.get(key).and_then(Json::as_str).is_none() {
            problems.push(format!("missing string field `{key}`"));
        }
    }
    match value.get("status").and_then(Json::as_str) {
        Some("success" | "failure" | "unsupported") => {}
        Some(other) => problems.push(format!(
            "status `{other}` is outside success|failure|unsupported"
        )),
        None => problems.push("missing string field `status`".to_string()),
    }
    if value.get("exit_code").and_then(Json::as_i64).is_none() {
        problems.push("missing integer `exit_code`".to_string());
    }
    match value.get("target") {
        Some(target) if target.as_object().is_some() => {
            if target.get("kind").and_then(Json::as_str).is_none() {
                problems.push("missing string `target.kind`".to_string());
            }
            if target.get("path").and_then(Json::as_str).is_none() {
                problems.push("missing string `target.path`".to_string());
            }
        }
        _ => problems.push("missing object `target`".to_string()),
    }
    match value.get("summary") {
        Some(summary) if summary.as_object().is_some() => {
            for key in ["errors", "warnings"] {
                if summary.get(key).and_then(Json::as_i64).is_none() {
                    problems.push(format!("missing integer `summary.{key}`"));
                }
            }
        }
        _ => problems.push("missing object `summary`".to_string()),
    }
    match value.get("diagnostics").and_then(Json::as_array) {
        Some(diagnostics) => {
            for (index, diagnostic) in diagnostics.iter().enumerate() {
                if diagnostic.as_object().is_none() {
                    problems.push(format!("diagnostics[{index}] is not an object"));
                    continue;
                }
                for key in ["code", "severity", "phase", "message"] {
                    if diagnostic.get(key).and_then(Json::as_str).is_none() {
                        problems.push(format!("missing string `diagnostics[{index}].{key}`"));
                    }
                }
                if let Some(code) = diagnostic.get("code").and_then(Json::as_str) {
                    if !crate::case::is_valid_code(code) {
                        problems.push(format!("diagnostics[{index}].code `{code}` is invalid"));
                    }
                }
                if let Some(severity) = diagnostic.get("severity").and_then(Json::as_str) {
                    if !matches!(severity, "error" | "warning") {
                        problems.push(format!(
                            "diagnostics[{index}].severity `{severity}` is invalid"
                        ));
                    }
                }
                if diagnostic
                    .get("primary")
                    .and_then(Json::as_object)
                    .is_none()
                {
                    problems.push(format!("missing object `diagnostics[{index}].primary`"));
                }
                if diagnostic.get("labels").and_then(Json::as_array).is_none() {
                    problems.push(format!("missing array `diagnostics[{index}].labels`"));
                }
                if diagnostic.get("notes").and_then(Json::as_array).is_none() {
                    problems.push(format!("missing array `diagnostics[{index}].notes`"));
                }
            }
        }
        None => problems.push("missing array `diagnostics`".to_string()),
    }
    if problems.is_empty() {
        return None;
    }
    problems.truncate(12);
    Some(format!(
        "JSON report does not match spec/language/diagnostics.md section 7: {}",
        problems.join("; ")
    ))
}

fn render_outputs(outcome: &ExecOutcome) -> String {
    let mut text = String::new();
    text.push_str("stdout:\n");
    text.push_str(&indent(&truncate_chars(&outcome.stdout, 4_000)));
    text.push_str("\nstderr:\n");
    text.push_str(&indent(&truncate_chars(&outcome.stderr, 4_000)));
    if outcome.stdout_truncated || outcome.stderr_truncated {
        text.push_str("\n[some captured output was truncated; see the raw sizes above]");
    }
    text
}

fn indent(text: &str) -> String {
    if text.is_empty() {
        return "  <empty>".to_string();
    }
    text.lines()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

// --------------------------------------------------------------------------
// Diagnostic code extraction
// --------------------------------------------------------------------------

#[derive(Debug, Default)]
struct CodeExtraction {
    codes: BTreeSet<String>,
    source: &'static str,
    structured_error: Option<String>,
}

fn extract_codes(stdout: &str, stderr: &str) -> CodeExtraction {
    let mut extraction = CodeExtraction {
        source: "none",
        ..CodeExtraction::default()
    };
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        extraction.structured_error =
            Some("stdout is empty (expected a JSON diagnostics report)".to_string());
        scan_codes(stderr, &mut extraction.codes);
        if !extraction.codes.is_empty() {
            extraction.source = "text(stderr)";
        }
        return extraction;
    }

    // DIAG-7.2: exactly one JSON object per command, never NDJSON.
    let value = match Json::parse(trimmed) {
        Ok(value) => value,
        Err(error) => {
            extraction.structured_error =
                Some(format!("stdout is not a single JSON object: {error}"));
            scan_codes(stdout, &mut extraction.codes);
            if extraction.codes.is_empty() {
                scan_codes(stderr, &mut extraction.codes);
                if !extraction.codes.is_empty() {
                    extraction.source = "text(stderr)";
                }
            } else {
                extraction.source = "text(stdout)";
            }
            return extraction;
        }
    };
    if value.as_object().is_none() {
        extraction.structured_error = Some("the JSON root must be an object".to_string());
        return extraction;
    }
    extraction.source = "json";

    match value.get("schema_version").and_then(Json::as_i64) {
        Some(1) => {}
        Some(other) => {
            extraction.structured_error =
                Some(format!("unsupported schema_version {other}; expected 1"))
        }
        None => {
            extraction.structured_error =
                Some("missing integer `schema_version` (expected 1)".to_string())
        }
    }

    let Some(diagnostics) = value.get("diagnostics").and_then(Json::as_array) else {
        if extraction.structured_error.is_none() {
            extraction.structured_error =
                Some("missing `diagnostics` array in the JSON envelope".to_string());
        }
        return extraction;
    };

    for (index, diagnostic) in diagnostics.iter().enumerate() {
        if diagnostic.as_object().is_none() {
            extraction.structured_error = Some(format!("diagnostics[{index}] is not an object"));
            continue;
        }
        let code = diagnostic.get("code").and_then(Json::as_str);
        let severity = diagnostic.get("severity").and_then(Json::as_str);
        let Some(code) = code else {
            extraction.structured_error =
                Some(format!("diagnostics[{index}] has no string `code`"));
            continue;
        };
        match severity {
            Some("error") => {
                if crate::case::is_valid_code(code) {
                    extraction.codes.insert(code.to_string());
                } else {
                    extraction.structured_error = Some(format!(
                        "diagnostics[{index}] has an invalid error code `{code}` (expected E-*)"
                    ));
                }
            }
            Some("warning") => {
                if !code.starts_with("W-") {
                    extraction.structured_error = Some(format!(
                        "diagnostics[{index}] has severity `warning` but code `{code}` does not start with W-"
                    ));
                }
            }
            Some(other) => {
                extraction.structured_error = Some(format!(
                    "diagnostics[{index}] has an unknown severity `{other}` (expected error or warning)"
                ));
            }
            None => {
                extraction.structured_error =
                    Some(format!("diagnostics[{index}] has no string `severity`"));
            }
        }
    }
    extraction
}

fn scan_codes(text: &str, codes: &mut BTreeSet<String>) {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index + 2 < bytes.len() {
        if bytes[index] == b'E' && bytes[index + 1] == b'-' {
            let start = index;
            index += 2;
            while index < bytes.len()
                && (bytes[index].is_ascii_uppercase()
                    || bytes[index].is_ascii_digit()
                    || bytes[index] == b'-')
            {
                index += 1;
            }
            let mut end = index;
            while end > start + 2 && bytes[end - 1] == b'-' {
                end -= 1;
            }
            if let Some(candidate) = text.get(start..end) {
                if crate::case::is_valid_code(candidate) {
                    codes.insert(candidate.to_string());
                }
            }
        } else {
            index += 1;
        }
    }
}

// --------------------------------------------------------------------------
// Process execution
// --------------------------------------------------------------------------

#[derive(Debug, Default)]
struct ExecOutcome {
    exit: Option<i32>,
    timed_out: bool,
    spawn_error: Option<String>,
    stdout: String,
    stderr: String,
    stdout_truncated: bool,
    stderr_truncated: bool,
}

struct Env<'a> {
    toolchain: &'a Path,
    work_dir: &'a Path,
    timeout_ms: u64,
    counter: Cell<u64>,
}

impl Env<'_> {
    fn exec(&self, args: &[String], cwd: &Path, stdin: &str) -> ExecOutcome {
        let serial = self.counter.get();
        self.counter.set(serial + 1);
        let io_dir = self.work_dir.join("io").join(format!("{serial:04}"));
        if let Err(error) = fs::create_dir_all(&io_dir) {
            return ExecOutcome {
                spawn_error: Some(format!("cannot create the I/O capture directory: {error}")),
                ..ExecOutcome::default()
            };
        }
        let stdin_path = io_dir.join("stdin.bin");
        if let Err(error) = write_bytes(&stdin_path, stdin.as_bytes()) {
            return ExecOutcome {
                spawn_error: Some(format!("cannot write stdin: {error}")),
                ..ExecOutcome::default()
            };
        }
        let stdout_path = io_dir.join("stdout.txt");
        let stderr_path = io_dir.join("stderr.txt");
        let stdout_file = match fs::File::create(&stdout_path) {
            Ok(file) => file,
            Err(error) => {
                return ExecOutcome {
                    spawn_error: Some(format!("cannot create stdout capture: {error}")),
                    ..ExecOutcome::default()
                }
            }
        };
        let stderr_file = match fs::File::create(&stderr_path) {
            Ok(file) => file,
            Err(error) => {
                return ExecOutcome {
                    spawn_error: Some(format!("cannot create stderr capture: {error}")),
                    ..ExecOutcome::default()
                }
            }
        };
        let stdin_file = match fs::File::open(&stdin_path) {
            Ok(file) => file,
            Err(error) => {
                return ExecOutcome {
                    spawn_error: Some(format!("cannot open stdin capture: {error}")),
                    ..ExecOutcome::default()
                }
            }
        };

        let mut command = Command::new(self.toolchain);
        command
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::from(stdin_file))
            .stdout(Stdio::from(stdout_file))
            .stderr(Stdio::from(stderr_file));
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let _ = fs::remove_dir_all(&io_dir);
                return ExecOutcome {
                    spawn_error: Some(error.to_string()),
                    ..ExecOutcome::default()
                };
            }
        };

        let timeout = Duration::from_millis(self.timeout_ms);
        let started = Instant::now();
        let mut outcome = ExecOutcome::default();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    outcome.exit = status.code();
                    break;
                }
                Ok(None) => {
                    if started.elapsed() >= timeout {
                        let _ = child.kill();
                        let _ = child.wait();
                        outcome.timed_out = true;
                        break;
                    }
                    std::thread::sleep(POLL_INTERVAL);
                }
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    outcome.spawn_error = Some(format!("waiting for the process failed: {error}"));
                    break;
                }
            }
        }

        match read_capped(&stdout_path, CAPTURE_CAP) {
            Ok((text, truncated, _)) => {
                outcome.stdout = text;
                outcome.stdout_truncated = truncated;
            }
            Err(error) => {
                outcome.spawn_error = Some(format!("cannot read captured stdout: {error}"));
            }
        }
        match read_capped(&stderr_path, CAPTURE_CAP) {
            Ok((text, truncated, _)) => {
                outcome.stderr = text;
                outcome.stderr_truncated = truncated;
            }
            Err(error) => {
                outcome.spawn_error = Some(format!("cannot read captured stderr: {error}"));
            }
        }
        let _ = fs::remove_dir_all(&io_dir);
        outcome
    }
}

fn write_bytes(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = fs::File::create(path)?;
    file.write_all(bytes)
}

// --------------------------------------------------------------------------
// Isolated copy, fixtures and snapshots
// --------------------------------------------------------------------------

fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(source)
        .map_err(|error| format!("cannot stat {}: {error}", source.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "case tree contains a symlink at {}; declare it in `fixtures` instead",
            source.display()
        ));
    }
    if metadata.is_dir() {
        fs::create_dir_all(destination)
            .map_err(|error| format!("cannot create {}: {error}", destination.display()))?;
        let mut entries: Vec<PathBuf> = fs::read_dir(source)
            .map_err(|error| format!("cannot read {}: {error}", source.display()))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<_, _>>()
            .map_err(|error| format!("cannot read {}: {error}", source.display()))?;
        entries.sort();
        for entry in entries {
            let name = entry
                .file_name()
                .ok_or_else(|| format!("invalid entry in {}", source.display()))?;
            // `.ravum/` is the generated build cache (gitignored). Copying it
            // would make the run depend on a stale or partially written cache
            // from an in-place build; the case is defined by its declared
            // sources only, so the isolated copy excludes it.
            if name == ".ravum" {
                continue;
            }
            copy_tree(&entry, &destination.join(name))?;
        }
        Ok(())
    } else if metadata.is_file() {
        fs::copy(source, destination)
            .map(|_| ())
            .map_err(|error| format!("cannot copy {}: {error}", source.display()))
    } else {
        Err(format!(
            "case tree contains an unsupported file type at {}",
            source.display()
        ))
    }
}

enum FixtureFailure {
    Environment(String),
    CaseDefinition(String),
}

fn apply_fixtures(root: &Path, fixtures: &[Fixture]) -> Result<(), FixtureFailure> {
    for fixture in fixtures {
        let link = root.join(&fixture.path);
        if link.exists() || fs::symlink_metadata(&link).is_ok() {
            return Err(FixtureFailure::CaseDefinition(format!(
                "fixture path `{}` already exists in the case tree",
                fixture.path
            )));
        }
        if let Some(parent) = link.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                FixtureFailure::Environment(format!(
                    "cannot create the parent of `{}`: {error}",
                    fixture.path
                ))
            })?;
        }
        let resolved = link.parent().unwrap_or(root).join(&fixture.target);
        if !resolved.exists() {
            return Err(FixtureFailure::CaseDefinition(format!(
                "fixture `{}` targets `{}`, which does not exist in the case tree",
                fixture.path, fixture.target
            )));
        }
        create_symlink(&fixture.kind, &fixture.target, &link).map_err(|error| {
            FixtureFailure::Environment(format!(
                "cannot create symlink `{}` -> `{}`: {error}",
                fixture.path, fixture.target
            ))
        })?;
    }
    Ok(())
}

#[cfg(unix)]
fn create_symlink(kind: &FixtureKind, target: &str, link: &Path) -> std::io::Result<()> {
    let _ = kind;
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_symlink(kind: &FixtureKind, target: &str, link: &Path) -> std::io::Result<()> {
    match kind {
        FixtureKind::SymlinkDir => std::os::windows::fs::symlink_dir(target, link),
        FixtureKind::SymlinkFile => std::os::windows::fs::symlink_file(target, link),
    }
}

#[cfg(not(any(unix, windows)))]
fn create_symlink(_kind: &FixtureKind, _target: &str, _link: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "symlinks are not supported on this platform",
    ))
}

fn snapshot_rv(root: &Path) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut snapshot = BTreeMap::new();
    collect_rv(root, root, &mut snapshot)?;
    Ok(snapshot)
}

fn collect_rv(
    root: &Path,
    dir: &Path,
    snapshot: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), String> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|error| format!("cannot read {}: {error}", dir.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<_, _>>()
        .map_err(|error| format!("cannot read {}: {error}", dir.display()))?;
    entries.sort();
    for entry in entries {
        let metadata = fs::symlink_metadata(&entry)
            .map_err(|error| format!("cannot stat {}: {error}", entry.display()))?;
        if metadata.is_dir() {
            collect_rv(root, &entry, snapshot)?;
        } else if metadata.is_file() && entry.extension().and_then(|e| e.to_str()) == Some("rv") {
            let relative = entry
                .strip_prefix(root)
                .map_err(|error| format!("cannot relativize {}: {error}", entry.display()))?
                .to_string_lossy()
                .replace('\\', "/");
            let bytes = fs::read(&entry)
                .map_err(|error| format!("cannot read {}: {error}", entry.display()))?;
            snapshot.insert(relative, bytes);
        }
    }
    Ok(())
}

fn diff_snapshots(
    before: &BTreeMap<String, Vec<u8>>,
    after: &BTreeMap<String, Vec<u8>>,
) -> Vec<String> {
    let mut differences = Vec::new();
    for (path, bytes) in before {
        match after.get(path) {
            None => differences.push(format!("{path} (removed)")),
            Some(next) if next != bytes => differences.push(format!("{path} (modified)")),
            Some(_) => {}
        }
    }
    for path in after.keys() {
        if !before.contains_key(path) {
            differences.push(format!("{path} (added)"));
        }
    }
    differences.sort();
    differences
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::case::{CaseSpec, Mode};

    fn spec_with_schema(version: i64) -> CaseSpec {
        CaseSpec {
            id: "test".into(),
            mode: Mode::Cli,
            project: ".".into(),
            expect_codes: Vec::new(),
            args: vec!["check".into()],
            stdin: String::new(),
            stdout_contains: None,
            stderr_contains: None,
            exit_code: None,
            skip_on: Vec::new(),
            unsupported: false,
            notes: String::new(),
            expect_files: Vec::new(),
            expect_json_schema_version: Some(version),
            timeout_ms: None,
            fixtures: Vec::new(),
        }
    }

    #[test]
    fn extracts_error_codes_from_the_canonical_envelope() {
        let stdout = r#"{"schema_version":1,"diagnostics":[
            {"code":"E-LEX-CHAR","severity":"error"},
            {"code":"E-PARSE-EXPECTED","severity":"error"},
            {"code":"W-RES-UNUSED-IMPORT","severity":"warning"}
        ]}"#;
        let extraction = extract_codes(stdout, "");
        assert_eq!(extraction.source, "json");
        assert!(extraction.structured_error.is_none(), "{:?}", extraction);
        assert_eq!(
            extraction.codes.into_iter().collect::<Vec<_>>(),
            vec!["E-LEX-CHAR".to_string(), "E-PARSE-EXPECTED".to_string()]
        );
    }

    #[test]
    fn ndjson_is_rejected_because_one_object_per_command_is_required() {
        let stdout = "{\"schema_version\":1,\"diagnostics\":[]}\n{\"schema_version\":1,\"diagnostics\":[]}\n";
        let extraction = extract_codes(stdout, "");
        assert!(extraction.structured_error.is_some());
    }

    #[test]
    fn envelope_without_schema_version_or_diagnostics_is_rejected() {
        let extraction = extract_codes(
            r#"{"diagnostics":[{"code":"E-LEX-CHAR","severity":"error"}]}"#,
            "",
        );
        assert!(extraction.structured_error.is_some());
        assert!(extraction.codes.contains("E-LEX-CHAR"));

        let extraction = extract_codes(r#"{"schema_version":1}"#, "");
        assert!(extraction.structured_error.is_some());
    }

    #[test]
    fn unstructured_stdout_is_an_error_even_with_text_codes() {
        let extraction = extract_codes("error[E-LEX-STRING]: bad\n", "noise");
        assert!(extraction.structured_error.is_some());
        assert!(extraction.codes.contains("E-LEX-STRING"));
    }

    #[test]
    fn scans_codes_in_text_and_trims_trailing_dashes() {
        let mut codes = BTreeSet::new();
        scan_codes("x E-LEX-CHAR: bad, E-TYP-MISMATCH- end", &mut codes);
        assert!(codes.contains("E-LEX-CHAR"));
        assert!(codes.contains("E-TYP-MISMATCH"));
    }

    #[test]
    fn full_json_schema_check_accepts_a_canonical_report() {
        let spec = spec_with_schema(1);
        let stdout = r#"{
            "schema_version": 1,
            "tool": "ravum-lang",
            "tool_version": "0.1.0",
            "spec_version": "core-1.0-draft.1",
            "command": "check",
            "status": "failure",
            "exit_code": 1,
            "target": {"kind": "package", "path": "examples/hello"},
            "summary": {"errors": 1, "warnings": 0},
            "diagnostics": [{
                "code": "E-TYP-MISMATCH",
                "severity": "error",
                "phase": "types",
                "message": "mismatched types",
                "primary": {"file": "src/main.rv", "start_byte": 1, "end_byte": 2},
                "labels": [],
                "notes": []
            }]
        }"#;
        assert_eq!(check_json_schema(&spec, stdout), None);
    }

    #[test]
    fn full_json_schema_check_rejects_missing_fields_and_wrong_status() {
        let spec = spec_with_schema(1);
        let stdout = r#"{
            "schema_version": 1,
            "tool": "ravum-lang",
            "success": true,
            "diagnostics": []
        }"#;
        let problem = check_json_schema(&spec, stdout).expect("must be rejected");
        assert!(problem.contains("tool_version"), "{problem}");
        assert!(problem.contains("status"), "{problem}");
        assert!(problem.contains("summary"), "{problem}");
    }

    #[test]
    fn snapshot_diff_reports_added_modified_removed() {
        let mut before = BTreeMap::new();
        before.insert("a.rv".to_string(), b"a".to_vec());
        before.insert("b.rv".to_string(), b"b".to_vec());
        let mut after = BTreeMap::new();
        after.insert("a.rv".to_string(), b"changed".to_vec());
        after.insert("c.rv".to_string(), b"c".to_vec());
        let diff = diff_snapshots(&before, &after);
        assert_eq!(
            diff,
            vec![
                "a.rv (modified)".to_string(),
                "b.rv (removed)".to_string(),
                "c.rv (added)".to_string()
            ]
        );
    }

    // -- check ⇒ build contract -------------------------------------------

    #[test]
    fn isolated_copy_excludes_the_generated_ravum_cache() {
        // A case directory can be polluted by an in-place build (`.ravum/` is
        // gitignored). Copying a stale or half-written cache would make the
        // run non-deterministic, so the isolated copy must contain the
        // declared case only.
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let base = std::env::temp_dir().join(format!(
            "ravum-conformance-copytest-{}-{nanos}",
            std::process::id()
        ));
        let source = base.join("source");
        let destination = base.join("destination");
        fs::create_dir_all(source.join("src")).expect("create source");
        fs::create_dir_all(source.join(".ravum").join("bin")).expect("create cache");
        fs::write(source.join("case.json"), "{}").expect("write case.json");
        fs::write(source.join("src").join("main.rv"), "fn main() {}").expect("write source");
        fs::write(source.join(".ravum").join("bin").join("stale.exe"), "x")
            .expect("write cache file");

        copy_tree(&source, &destination).expect("copy tree");

        assert!(destination.join("case.json").is_file());
        assert!(destination.join("src").join("main.rv").is_file());
        assert!(
            !destination.join(".ravum").exists(),
            "the generated `.ravum/` cache must not leak into the isolated copy"
        );
        let _ = fs::remove_dir_all(&base);
    }

    fn passing_check() -> ExecOutcome {
        ExecOutcome {
            exit: Some(0),
            stdout: "package demo: 0 error(s)".to_string(),
            ..ExecOutcome::default()
        }
    }

    fn passing_verdict() -> Verdict {
        Verdict::pass("check exit 0", "check: exit 0")
    }

    #[test]
    fn build_failure_after_check_pass_is_a_case_fail() {
        let check = passing_check();
        let build = ExecOutcome {
            exit: Some(1),
            stderr: "error: rustc failed:\nerror[E0508]: cannot move out of type `[String; 2]`"
                .to_string(),
            ..ExecOutcome::default()
        };
        let (status, detail) = classify_build(&check, &build);
        assert_eq!(status, BuildStatus::Failed);
        assert!(detail.contains("check ⇒ build divergence"), "{detail}");
        // Both outputs are part of the evidence.
        assert!(detail.contains("package demo: 0 error(s)"), "{detail}");
        assert!(detail.contains("E0508"), "{detail}");

        let verdict = apply_build(passing_verdict(), status, &detail);
        assert_eq!(verdict.status, Status::Fail);
        assert_eq!(verdict.expected, "check exit 0 and build exit 0");
        assert_eq!(verdict.actual, "check exit 0; build failed");
        assert!(
            verdict.detail.contains("package demo: 0 error(s)")
                && verdict.detail.contains("E0508"),
            "{}",
            verdict.detail
        );
    }

    #[test]
    fn missing_rustc_is_environment_and_not_run_never_pass() {
        let build = ExecOutcome {
            exit: Some(TOOL_MISSING_EXIT_CODE),
            stderr: "`rustc` was not found; install a Rust toolchain".to_string(),
            ..ExecOutcome::default()
        };
        let (status, detail) = classify_build(&passing_check(), &build);
        assert_eq!(status, BuildStatus::Environment);
        assert!(detail.contains("environment"), "{detail}");
        assert!(detail.contains("not a Ravum diagnostic"), "{detail}");
        // The detail must also name the missing tool so the operator can fix
        // the environment rather than the corpus.
        assert!(detail.contains("rustc"), "{detail}");

        let verdict = apply_build(passing_verdict(), status, &detail);
        assert_eq!(
            verdict.status,
            Status::NotRun,
            "a missing toolchain must never be reported as PASS"
        );
    }

    #[test]
    fn build_spawn_error_and_timeout_are_environment_not_pass() {
        let spawn = ExecOutcome {
            spawn_error: Some("cannot execute".to_string()),
            ..ExecOutcome::default()
        };
        let (status, detail) = classify_build(&passing_check(), &spawn);
        assert_eq!(status, BuildStatus::Environment);
        assert!(detail.contains("could not be started"), "{detail}");
        assert_eq!(
            apply_build(passing_verdict(), status, &detail).status,
            Status::NotRun
        );

        let timeout = ExecOutcome {
            timed_out: true,
            ..ExecOutcome::default()
        };
        let (status, detail) = classify_build(&passing_check(), &timeout);
        assert_eq!(status, BuildStatus::Environment);
        assert!(detail.contains("timeout"), "{detail}");
        assert_eq!(
            apply_build(passing_verdict(), status, &detail).status,
            Status::NotRun
        );
    }

    #[test]
    fn successful_build_is_verified_and_keeps_the_pass() {
        let build = ExecOutcome {
            exit: Some(0),
            stdout: "generated: .ravum/gen/main.rs\nbuilt: .ravum/bin/demo".to_string(),
            ..ExecOutcome::default()
        };
        let (status, detail) = classify_build(&passing_check(), &build);
        assert_eq!(status, BuildStatus::Verified);
        assert!(detail.contains("build exit 0"), "{detail}");
        assert_eq!(
            apply_build(passing_verdict(), status, &detail).status,
            Status::Pass
        );
        assert!(status.is_build_evidence());
    }

    #[test]
    fn build_status_strings_are_stable_and_positive_evidence_is_explicit() {
        for (status, text) in [
            (BuildStatus::NotRequested, "not_requested"),
            (BuildStatus::NotApplicable, "not_applicable"),
            (BuildStatus::NotAttempted, "not_attempted"),
            (BuildStatus::Verified, "verified"),
            (BuildStatus::Implicit, "implicit"),
            (BuildStatus::Failed, "failed"),
            (BuildStatus::Environment, "environment"),
        ] {
            assert_eq!(status.as_str(), text);
            let evidence = matches!(status, BuildStatus::Verified | BuildStatus::Implicit);
            assert_eq!(status.is_build_evidence(), evidence, "{text}");
        }
    }

    #[test]
    fn build_is_not_attempted_when_the_check_failed() {
        let failed = Verdict::fail("check exit 0", "check: exit 1", "diagnostic");
        // The helper is exercised directly: without a passing check there is
        // no Env to run a build, so the classification below is what the
        // harness records for the case.
        let evaluation = Evaluation::not_attempted(
            failed,
            true,
            "the check did not pass, so the build dimension was not measured",
        );
        assert_eq!(evaluation.build, BuildStatus::NotAttempted);
        assert!(evaluation.build_detail.contains("did not pass"));
        assert!(!evaluation.build.is_build_evidence());
        assert_eq!(evaluation.verdict.status, Status::Fail);
        // And without the flag the report stays silent about builds.
        let evaluation = Evaluation::not_attempted(
            Verdict::fail("check exit 0", "check: exit 1", "diagnostic"),
            false,
            "should not be recorded",
        );
        assert_eq!(evaluation.build, BuildStatus::NotRequested);
        assert!(evaluation.build_detail.is_empty());
    }
}
