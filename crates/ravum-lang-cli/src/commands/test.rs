//! `ravum-lang test` — build the generated test runner, execute it and parse
//! its `test <name> ... ok/FAILED/ignored` lines.
//!
//! **The CLI never reports a partial run as complete.** Before printing
//! `test result: ok` the parsed run is validated against the tests the checked
//! program actually declares (`pipeline::counts_for(...).tests` and the
//! `is_test` signatures in `CheckedProgram::facts.fn_sigs`):
//!
//! * every declared test must be accounted for: `executed + filtered_out ==
//!   declared` (a runner that executes fewer tests and omits the rest is a
//!   FAIL, not `ok` with 0 passed);
//! * the runner's final summary (the current `N passed; M failed` line or the
//!   newer `test result: ok. P passed; F failed; I ignored; K filtered out`
//!   line), the parsed per-test verdicts and the process exit code must agree;
//! * a line that looks like a test result but cannot be parsed is a FAIL
//!   (truncation and format drift are never silently ignored);
//! * `--filter` is only reported as applied when the runner accounts for it
//!   (`test result: ... K filtered out`). A runner that ignores the filter is a
//!   FAIL with an explicit message rather than a silent full run.
//!
//! The runner writes results on stdout; stderr is displayed but never parsed
//! as test results. A test body that prints a line starting with `test ` (or a
//! fake `test result:` line) on stdout can still be mistaken for runner output:
//! the generated runner has no marker prefix, so this residual ambiguity is
//! documented rather than silently trusted.

use ravum_lang_sema::Profile;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use crate::json;
use crate::pipeline::{self, BuildOptions};
use crate::util;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestStatus {
    Passed,
    Failed,
    Ignored,
}

impl TestStatus {
    fn name(self) -> &'static str {
        match self {
            TestStatus::Passed => "ok",
            TestStatus::Failed => "FAILED",
            TestStatus::Ignored => "ignored",
        }
    }
}

#[derive(Debug, Clone)]
struct TestCase {
    name: String,
    status: TestStatus,
}

/// The tests the checked program declares, before the runner runs anything.
#[derive(Debug, Clone, Default)]
struct DeclaredTests {
    /// `pipeline::counts_for(&frontend).tests`.
    total: usize,
    /// Bare names of the `is_test` signatures (`checked.facts.fn_sigs`). The
    /// runner prefixes nested-module tests with `module::`, so matching is done
    /// on the last `::` segment.
    names: Vec<String>,
}

fn declared_tests(frontend: &pipeline::Frontend) -> DeclaredTests {
    let total = pipeline::counts_for(frontend).tests;
    let mut names = Vec::new();
    if let Some(checked) = &frontend.checked {
        for signature in checked.facts.fn_sigs.values() {
            if signature.is_test {
                names.push(signature.name.clone());
            }
        }
    }
    names.sort();
    DeclaredTests { total, names }
}

pub fn run(path: &Path, filter: Option<&str>, json_output: bool) -> i32 {
    let frontend = pipeline::analyze(path, Profile::Core, false);
    if !frontend.success() {
        let declared = declared_tests(&frontend);
        let report = RunReport::not_run(declared.total, util::EXIT_FAIL, "analysis failed");
        if json_output {
            let diagnostics = json::diagnostics(frontend.diags.as_slice(), &frontend.pkg.sources);
            json::print(&report_envelope(&frontend, path, &report, diagnostics, &[]));
        } else {
            eprint!("{}", pipeline::render_diags(&frontend));
            util::err(format!(
                "test: {} error(s); the test runner was not built",
                frontend.error_count()
            ));
        }
        return util::EXIT_FAIL;
    }

    let options = BuildOptions {
        test_mode: true,
        ..BuildOptions::default()
    };
    let artifacts = match pipeline::compile_program(&frontend, &options) {
        Ok(artifacts) => artifacts,
        Err(error) => {
            error.print();
            let declared = declared_tests(&frontend);
            let report = RunReport::not_run(declared.total, error.exit_code(), &error.message());
            if json_output {
                let diagnostic = json::synthetic("E-BUILD", "error", error.message());
                json::print(&report_envelope(
                    &frontend,
                    path,
                    &report,
                    vec![diagnostic],
                    &[],
                ));
            }
            return error.exit_code();
        }
    };

    let mut command = Command::new(&artifacts.binary);
    if let Some(filter) = filter {
        command.arg("--filter").arg(filter);
    }
    command.current_dir(frontend.root());
    let output = match command.output() {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let message = format!(
                "cannot execute the generated test runner `{}`: {error}",
                util::pretty_path(&artifacts.binary)
            );
            util::err(&message);
            let declared = declared_tests(&frontend);
            let report = RunReport::not_run(declared.total, util::EXIT_TOOL, &message);
            if json_output {
                let diagnostic = json::synthetic("E-TOOL", "error", message);
                json::print(&report_envelope(
                    &frontend,
                    path,
                    &report,
                    vec![diagnostic],
                    &[],
                ));
            }
            return util::EXIT_TOOL;
        }
        Err(error) => {
            let message = format!(
                "cannot execute the generated test runner `{}`: {error}",
                util::pretty_path(&artifacts.binary)
            );
            util::err(&message);
            let declared = declared_tests(&frontend);
            let report = RunReport::not_run(declared.total, util::EXIT_FAIL, &message);
            if json_output {
                let diagnostic = json::synthetic("E-TOOL", "error", message);
                json::print(&report_envelope(
                    &frontend,
                    path,
                    &report,
                    vec![diagnostic],
                    &[],
                ));
            }
            return util::EXIT_FAIL;
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    // Results are read from stdout only; stderr is displayed as-is.
    let parsed = parse_runner_output(&stdout);
    let declared = declared_tests(&frontend);
    let report = validate_run(&declared, filter, &parsed, output.status.code());

    if json_output {
        let exit = report.cli_exit;
        json::print(&report_envelope(
            &frontend,
            path,
            &report,
            Vec::new(),
            &parsed.cases,
        ));
        return exit;
    }

    print!("{stdout}");
    eprint!("{stderr}");

    // Exactly one human summary line with the `test result:` prefix. When the
    // emitted runner already prints its measured `test result:` line, keep it
    // as the primary output and add the CLI's own accounting on a line that is
    // clearly not a second result line. With the legacy runner (no `test
    // result:` line) the CLI prints the verdict itself.
    if parsed.summary.is_some() {
        println!(
            "test cases: {} declared; {} filtered out; {} executed{}",
            report.declared,
            report.filtered,
            report.executed,
            match filter {
                Some(filter) => format!(" (filter `{filter}`)"),
                None => String::new(),
            }
        );
    } else {
        println!(
            "test result: {}; {} passed; {} failed; {} ignored; {} declared{}",
            if report.success { "ok" } else { "FAILED" },
            report.passed,
            report.failed,
            report.ignored,
            report.declared,
            match filter {
                Some(filter) => format!(" (filter `{filter}`)"),
                None => String::new(),
            }
        );
    }
    for message in &report.validation_errors {
        util::err(message);
    }

    report.cli_exit
}

fn report_envelope(
    frontend: &pipeline::Frontend,
    path: &Path,
    report: &RunReport,
    diagnostics: Vec<json::JsonDiagnostic>,
    cases: &[TestCase],
) -> json::JsonEnvelope {
    let summary = test_summary(report, cases);
    report_with(frontend, path, report, diagnostics, summary)
}

fn report_with(
    frontend: &pipeline::Frontend,
    path: &Path,
    report: &RunReport,
    diagnostics: Vec<json::JsonDiagnostic>,
    summary: serde_json::Value,
) -> json::JsonEnvelope {
    use ravum_lang_sema::loader::{CrateKind, LockfileState};
    let target_kind = if frontend.pkg.crate_kind == Some(CrateKind::File) {
        "file"
    } else {
        "package"
    };
    let lockfile_stale = match frontend.pkg.lockfile_state {
        LockfileState::Stale => Some(true),
        LockfileState::Created | LockfileState::Current => Some(false),
        _ => None,
    };
    let exit_code = report.cli_exit;
    json::envelope(
        json::ReportMeta {
            command: "test",
            success: report.success,
            exit_code,
            target_kind,
            target_path: util::target_display(path),
            profile: Some("core".to_string()),
            dependencies_digest: Some(frontend.pkg.dependencies_digest.clone()),
            lockfile_stale,
        },
        diagnostics,
        summary,
    )
}

// ---------------------------------------------------------------------------
// Parsing (pure)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct RunnerSummary {
    passed: usize,
    failed: usize,
    ignored: usize,
    filtered: usize,
}

#[derive(Debug, Default)]
struct ParsedOutput {
    /// `running N tests` written by the newer runner (the current one omits it).
    running: Option<usize>,
    /// `test <name> ... <verdict>` lines.
    cases: Vec<TestCase>,
    /// `test result: ...` (new) with filter accounting.
    summary: Option<RunnerSummary>,
    /// Legacy `N passed; M failed` line written by the current runner.
    legacy: Option<(usize, usize)>,
    /// How many final summary lines were seen (more than one is a format bug).
    summary_lines: usize,
    /// Result-looking lines that were not understood.
    malformed: Vec<String>,
}

/// Parse the runner's stdout without judging it. `validate_run` decides.
fn parse_runner_output(text: &str) -> ParsedOutput {
    let mut parsed = ParsedOutput::default();
    for raw in text.lines() {
        let line = raw.trim_end();
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("test result:") {
            parsed.summary_lines += 1;
            match parse_result_summary(rest) {
                Some(summary) => parsed.summary = Some(summary),
                None => parsed.malformed.push(line.to_string()),
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("test ") {
            match parse_case_line(rest) {
                Some(case) => parsed.cases.push(case),
                None => parsed.malformed.push(line.to_string()),
            }
            continue;
        }
        if let Some(running) = parse_running_line(trimmed) {
            if parsed.running.is_some() {
                parsed.malformed.push(line.to_string());
            } else {
                parsed.running = Some(running);
            }
            continue;
        }
        if let Some(legacy) = parse_legacy_summary(trimmed) {
            parsed.summary_lines += 1;
            parsed.legacy = Some(legacy);
        }
    }
    parsed
}

/// `test <name> ... ok|FAILED|ignored` — the `test ` prefix is already removed.
fn parse_case_line(rest: &str) -> Option<TestCase> {
    let (name, verdict) = rest.rsplit_once(" ... ")?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let status = match verdict.trim() {
        "ok" => TestStatus::Passed,
        "FAILED" => TestStatus::Failed,
        "ignored" => TestStatus::Ignored,
        _ => return None,
    };
    Some(TestCase {
        name: name.to_string(),
        status,
    })
}

/// `running N tests` (newer runner). Returns `None` for any other line.
fn parse_running_line(line: &str) -> Option<usize> {
    let rest = line.strip_prefix("running ")?.trim_end();
    let rest = rest
        .strip_suffix(" tests")
        .or_else(|| rest.strip_suffix(" test"))?;
    rest.parse().ok()
}

/// `test result: ok. P passed; F failed; I ignored; K filtered out`
/// (also tolerates cargo's `; finished in 0.01s` / `; N measured` trailers).
/// `rest` is what follows `test result:`.
fn parse_result_summary(rest: &str) -> Option<RunnerSummary> {
    let rest = rest.trim_start();
    let rest = rest
        .strip_prefix("ok")
        .or_else(|| rest.strip_prefix("FAILED"))?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('.').unwrap_or(rest).trim_start();

    let mut passed = None;
    let mut failed = None;
    let mut ignored = None;
    let mut filtered = None;
    for segment in rest.split(';') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        // Cargo-compatible trailers that carry no count we validate.
        if let Some(duration) = segment.strip_prefix("finished in ") {
            if duration.is_empty() {
                return None;
            }
            continue;
        }
        if let Some(count) = segment.strip_suffix(" measured") {
            let _: usize = count.trim().parse().ok()?;
            continue;
        }
        let (number, phrase) = segment.split_once(' ')?;
        let number: usize = number.parse().ok()?;
        match phrase.trim() {
            "passed" => {
                if passed.replace(number).is_some() {
                    return None;
                }
            }
            "failed" => {
                if failed.replace(number).is_some() {
                    return None;
                }
            }
            "ignored" => {
                if ignored.replace(number).is_some() {
                    return None;
                }
            }
            "filtered out" => {
                if filtered.replace(number).is_some() {
                    return None;
                }
            }
            // Cargo-compatible trailers that carry no count we validate.
            trailer if trailer.starts_with("finished in ") => {}
            "measured" => {}
            _ => return None,
        }
    }
    Some(RunnerSummary {
        passed: passed?,
        failed: failed?,
        ignored: ignored.unwrap_or(0),
        filtered: filtered.unwrap_or(0),
    })
}

/// The current runner's `N passed; M failed` final line.
fn parse_legacy_summary(line: &str) -> Option<(usize, usize)> {
    let (passed, failed) = line.split_once(';')?;
    let passed = passed.trim().strip_suffix(" passed")?;
    let failed = failed.trim().strip_suffix(" failed")?;
    Some((passed.parse().ok()?, failed.parse().ok()?))
}

// ---------------------------------------------------------------------------
// Validation (pure)
// ---------------------------------------------------------------------------

/// The honest result of one `test` invocation.
#[derive(Debug, Clone)]
struct RunReport {
    declared: usize,
    executed: usize,
    passed: usize,
    failed: usize,
    ignored: usize,
    filtered: usize,
    /// Raw process exit code (`None` = no code: signal or abrupt termination),
    /// reported verbatim in the JSON summary.
    exit_code: Option<i32>,
    /// The exit code this CLI invocation returns.
    cli_exit: i32,
    /// Why the report cannot be trusted as a complete run (empty when it can).
    validation_errors: Vec<String>,
    success: bool,
}

impl RunReport {
    /// The report for a run that never happened (analysis/build/exec failure).
    fn not_run(declared: usize, cli_exit: i32, error: &str) -> Self {
        RunReport {
            declared,
            executed: 0,
            passed: 0,
            failed: 0,
            ignored: 0,
            filtered: 0,
            exit_code: None,
            cli_exit,
            validation_errors: vec![error.to_string()],
            success: false,
        }
    }
}

fn count_status(cases: &[TestCase], status: TestStatus) -> usize {
    cases.iter().filter(|case| case.status == status).count()
}

/// Decide whether the parsed output of one runner invocation is a complete,
/// self-consistent run of every declared test. Pure: no I/O, no process.
fn validate_run(
    declared: &DeclaredTests,
    filter: Option<&str>,
    parsed: &ParsedOutput,
    exit_code: Option<i32>,
) -> RunReport {
    let executed = parsed.cases.len();
    let passed = count_status(&parsed.cases, TestStatus::Passed);
    let failed = count_status(&parsed.cases, TestStatus::Failed);
    let ignored = count_status(&parsed.cases, TestStatus::Ignored);
    let filtered = parsed.summary.map(|summary| summary.filtered).unwrap_or(0);
    let mut errors: Vec<String> = Vec::new();

    for line in &parsed.malformed {
        errors.push(format!(
            "unparsable test result line (truncated output or format drift): `{line}`"
        ));
    }

    if parsed.summary_lines == 0 {
        errors.push(
            "the test runner produced no final summary line; the run is truncated or the runner \
             did not finish"
                .to_string(),
        );
    } else if parsed.summary_lines > 2 {
        errors.push(format!(
            "the test runner produced {} final summary lines; the output cannot be trusted",
            parsed.summary_lines
        ));
    } else if parsed.summary_lines == 2 && !(parsed.summary.is_some() && parsed.legacy.is_some()) {
        errors.push(
            "the test runner produced two final summary lines of the same kind; the output cannot \
             be trusted"
                .to_string(),
        );
    }

    // The declared test count must be internally consistent before it can be
    // used as the expected total.
    if declared.names.len() != declared.total {
        errors.push(format!(
            "internal inconsistency: the checked program counts {} test item(s) but the checker \
             reports {} test signature(s); refusing to guess which tests should run",
            declared.total,
            declared.names.len()
        ));
    }

    // Per-test names must belong to the checked program.
    let mut expected: BTreeMap<&str, usize> = BTreeMap::new();
    for name in &declared.names {
        *expected.entry(name.as_str()).or_insert(0) += 1;
    }
    for case in &parsed.cases {
        let bare = case.name.rsplit("::").next().unwrap_or(&case.name);
        match expected.get_mut(bare) {
            Some(remaining) if *remaining > 0 => *remaining -= 1,
            _ => errors.push(format!(
                "the runner reported `{}`, which is not a declared `test fn` of the checked \
                 program",
                case.name
            )),
        }
    }

    // Counts: executed + filtered == declared (partial runs are never complete).
    match executed.checked_add(filtered) {
        Some(total) if total == declared.total => {}
        Some(total) => errors.push(format!(
            "{} test(s) declared but {} executed and {} filtered out ({total} accounted for); \
             a partial run must not be reported as complete",
            declared.total, executed, filtered
        )),
        None => errors.push("reported test counts overflow".to_string()),
    }

    if filter.is_none() && filtered > 0 {
        errors.push(format!(
            "the runner reported {filtered} filtered-out test(s) although no `--filter` was given"
        ));
    }
    if let Some(filter) = filter {
        if parsed.summary.is_none() && declared.total > 0 {
            errors.push(
                "`--filter` was requested but the generated test runner did not report filter \
                 accounting (`test result: ... K filtered out`); the filter may not have been \
                 applied"
                    .to_string(),
            );
        }
        // Independent of the runner's own accounting: a test that does not match
        // the filter must not have been executed. `--filter` is a substring match
        // on the reported test name, so this is checkable from the CLI side.
        for case in &parsed.cases {
            if !case.name.contains(filter) {
                errors.push(format!(
                    "the runner executed `{}` although `--filter {filter}` was given; the \
                     filter was not applied",
                    case.name
                ));
            }
        }
    }

    // The final summary must agree with the per-test lines.
    if let Some(summary) = parsed.summary {
        if summary.passed != passed {
            errors.push(format!(
                "summary reports {} passed but {passed} `test ... ok` line(s) were parsed",
                summary.passed
            ));
        }
        if summary.failed != failed {
            errors.push(format!(
                "summary reports {} failed but {failed} `test ... FAILED` line(s) were parsed",
                summary.failed
            ));
        }
        if summary.ignored != ignored {
            errors.push(format!(
                "summary reports {} ignored but {ignored} `test ... ignored` line(s) were parsed",
                summary.ignored
            ));
        }
    }
    if let Some((legacy_passed, legacy_failed)) = parsed.legacy {
        if legacy_passed != passed {
            errors.push(format!(
                "summary reports {legacy_passed} passed but {passed} `test ... ok` line(s) were \
                 parsed"
            ));
        }
        if legacy_failed != failed {
            errors.push(format!(
                "summary reports {legacy_failed} failed but {failed} `test ... FAILED` line(s) \
                 were parsed"
            ));
        }
    }
    if let (Some(summary), Some((legacy_passed, legacy_failed))) = (parsed.summary, parsed.legacy) {
        if summary.passed != legacy_passed || summary.failed != legacy_failed {
            errors.push(format!(
                "the two final summary lines disagree: `{} passed; {} failed` vs `test result: \
                 ... {} passed; {} failed`",
                legacy_passed, legacy_failed, summary.passed, summary.failed
            ));
        }
    }

    if let Some(running) = parsed.running {
        if running != executed {
            errors.push(format!(
                "the runner announced `running {running} test(s)` but emitted {executed} \
                 per-test result line(s)"
            ));
        }
    }

    // Exit code must agree with the verdicts.
    match exit_code {
        None => errors.push(
            "the test runner was terminated without an exit code (signal or abrupt termination)"
                .to_string(),
        ),
        Some(code) if code < 0 => errors.push(format!(
            "the test runner was terminated by exception 0x{:08X}",
            code as u32
        )),
        Some(0) if failed > 0 => errors.push(format!(
            "the runner reported {failed} failed test(s) but exited with code 0"
        )),
        Some(code) if code != 0 && failed == 0 => errors.push(format!(
            "the runner exited with code {code} but reported no failed test; refusing to trust \
             the report"
        )),
        Some(_) => {}
    }

    let success = errors.is_empty() && failed == 0 && exit_code == Some(0);
    RunReport {
        declared: declared.total,
        executed,
        passed,
        failed,
        ignored,
        filtered,
        exit_code,
        cli_exit: if success {
            util::EXIT_OK
        } else {
            util::EXIT_FAIL
        },
        validation_errors: errors,
        success,
    }
}

fn test_summary(report: &RunReport, cases: &[TestCase]) -> serde_json::Value {
    let entries = cases
        .iter()
        .map(|case| {
            json!({
                "name": case.name,
                "status": case.status.name(),
            })
        })
        .collect::<Vec<_>>();
    // `diagnostics.md` §7.2 requires `summary` to carry `errors` and `warnings`.
    // `test` reports test counts, so both stay zero unless a case produced a
    // diagnostic — the documented fields are present rather than omitted.
    let mut summary = json!({
        "errors": 0,
        "warnings": 0,
        "total": report.executed,
        "declared": report.declared,
        "passed": report.passed,
        "failed": report.failed,
        "ignored": report.ignored,
        "filtered": report.filtered,
        "exit_code": report.exit_code,
        "validation_errors": report.validation_errors,
        "tests": entries,
    });
    if let Some(error) = report.validation_errors.first() {
        summary["error"] = json!(error);
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared(total: usize, names: &[&str]) -> DeclaredTests {
        DeclaredTests {
            total,
            names: names.iter().map(|name| name.to_string()).collect(),
        }
    }

    fn report(
        declared: &DeclaredTests,
        filter: Option<&str>,
        text: &str,
        code: Option<i32>,
    ) -> RunReport {
        validate_run(declared, filter, &parse_runner_output(text), code)
    }

    #[test]
    fn parses_rust_style_per_test_lines() {
        let text = "\nrunning 3 tests\ntest alpha ... ok\ntest beta ... FAILED\ntest gamma ... ignored\n\n\
                    test result: FAILED. 1 passed; 1 failed; 1 ignored; 0 filtered out\n";
        let parsed = parse_runner_output(text);
        assert_eq!(parsed.cases.len(), 3);
        assert_eq!(parsed.cases[0].name, "alpha");
        assert_eq!(parsed.cases[0].status, TestStatus::Passed);
        assert_eq!(parsed.cases[1].status, TestStatus::Failed);
        assert_eq!(parsed.cases[2].status, TestStatus::Ignored);
        assert_eq!(parsed.running, Some(3));
        assert_eq!(
            parsed.summary,
            Some(RunnerSummary {
                passed: 1,
                failed: 1,
                ignored: 1,
                filtered: 0
            })
        );
        assert!(parsed.malformed.is_empty());
    }

    #[test]
    fn parses_the_current_legacy_summary() {
        let parsed = parse_runner_output("test a ... ok\ntest b ... ok\n\n2 passed; 0 failed\n");
        assert_eq!(parsed.cases.len(), 2);
        assert_eq!(parsed.legacy, Some((2, 0)));
        assert!(parsed.summary.is_none());
        assert!(parsed.malformed.is_empty());
    }

    #[test]
    fn ignores_unrelated_lines_but_rejects_result_looking_ones() {
        let parsed = parse_runner_output("note: something\ntest ... ok\ntest x ...\n");
        assert!(parsed.cases.is_empty());
        assert_eq!(parsed.malformed.len(), 2, "{:?}", parsed.malformed);
    }

    #[test]
    fn unknown_verdict_is_not_silently_ignored() {
        let parsed = parse_runner_output("test x ... panicked\n");
        assert!(parsed.cases.is_empty());
        assert_eq!(parsed.malformed.len(), 1);
    }

    #[test]
    fn truncation_is_a_failure_even_with_a_legacy_summary() {
        // Two tests declared, the runner is killed after the first one.
        let case = report(
            &declared(2, &["a", "b"]),
            None,
            "test a ... ok\n\n1 passed; 0 failed\n",
            Some(0),
        );
        assert!(!case.success, "{case:?}");
        assert!(
            case.validation_errors
                .iter()
                .any(|error| error.contains("declared") && error.contains("partial")),
            "{:?}",
            case.validation_errors
        );
    }

    #[test]
    fn zero_reported_tests_for_declared_tests_is_a_failure() {
        // The historical bug: 3 tests declared, the runner emits nothing but
        // `0 passed; 0 failed` and exits 0. This must never be `test result: ok`.
        let case = report(
            &declared(3, &["a", "b", "c"]),
            None,
            "\n0 passed; 0 failed\n",
            Some(0),
        );
        assert!(!case.success, "{case:?}");
        assert_eq!(case.passed, 0);
        assert!(!case.validation_errors.is_empty());
    }

    #[test]
    fn a_complete_legacy_run_is_success() {
        let case = report(
            &declared(1, &["simple"]),
            None,
            "test simple ... ok\n\n1 passed; 0 failed\n",
            Some(0),
        );
        assert!(case.success, "{:?}", case.validation_errors);
        assert_eq!(case.executed, 1);
        assert_eq!(case.passed, 1);
    }

    #[test]
    fn a_complete_new_format_run_is_success() {
        let text = "running 3 tests\ntest alpha ... ok\ntest gamma ... ignored\n\
                    test beta ... ok\n\ntest result: ok. 2 passed; 0 failed; 1 ignored; 0 filtered out; \
                    finished in 0.01s\n";
        let case = report(
            &declared(3, &["alpha", "beta", "gamma"]),
            None,
            text,
            Some(0),
        );
        assert!(case.success, "{:?}", case.validation_errors);
        assert_eq!(case.ignored, 1);
    }

    #[test]
    fn count_mismatch_between_summary_and_cases_is_a_failure() {
        let case = report(
            &declared(2, &["a", "b"]),
            None,
            "test a ... ok\n\ntest result: ok. 2 passed; 0 failed; 0 ignored; 0 filtered out\n",
            Some(0),
        );
        assert!(!case.success);
        assert!(case
            .validation_errors
            .iter()
            .any(|error| error.contains("summary reports 2 passed")));
    }

    #[test]
    fn exit_zero_with_failures_is_a_failure() {
        let case = report(
            &declared(1, &["broken"]),
            None,
            "test broken ... FAILED\n\n0 passed; 1 failed\n",
            Some(0),
        );
        assert!(!case.success);
        assert!(case
            .validation_errors
            .iter()
            .any(|error| error.contains("exited with code 0")));
    }

    #[test]
    fn exit_one_without_failures_is_a_failure() {
        let case = report(
            &declared(1, &["a"]),
            None,
            "test a ... ok\n\n1 passed; 0 failed\n",
            Some(1),
        );
        assert!(!case.success);
        assert!(case
            .validation_errors
            .iter()
            .any(|error| error.contains("exited with code 1")));
    }

    #[test]
    fn a_failing_test_with_exit_one_is_an_honest_failure_without_validation_noise() {
        let case = report(
            &declared(1, &["broken"]),
            None,
            "test broken ... FAILED\n\n0 passed; 1 failed\n",
            Some(1),
        );
        assert!(!case.success);
        assert_eq!(case.failed, 1);
        assert!(
            case.validation_errors.is_empty(),
            "a consistent failing run needs no validation error: {:?}",
            case.validation_errors
        );
    }

    #[test]
    fn stderr_noise_does_not_break_parsing() {
        // `parse_runner_output` is fed stdout; noise-free stderr is the norm.
        let case = report(
            &declared(1, &["a"]),
            None,
            "warning: unrelated\ntest a ... ok\n\n1 passed; 0 failed\n",
            Some(0),
        );
        assert!(case.success, "{:?}", case.validation_errors);
    }

    #[test]
    fn filtered_tests_are_accounted_for() {
        let text = "running 1 test\ntest keep ... ok\n\n\
                    test result: ok. 1 passed; 0 failed; 0 ignored; 2 filtered out\n";
        let case = report(
            &declared(3, &["keep", "skip1", "skip2"]),
            Some("keep"),
            text,
            Some(0),
        );
        assert!(case.success, "{:?}", case.validation_errors);
        assert_eq!(case.filtered, 2);

        // A runner that ignores the filter runs everything and reports nothing.
        let ignored_filter = report(
            &declared(3, &["keep", "skip1", "skip2"]),
            Some("keep"),
            "test keep ... ok\ntest skip1 ... ok\ntest skip2 ... ok\n\n3 passed; 0 failed\n",
            Some(0),
        );
        assert!(!ignored_filter.success);
        assert!(ignored_filter
            .validation_errors
            .iter()
            .any(|error| error.contains("filter")));
    }

    #[test]
    fn filtered_out_without_filter_is_a_failure() {
        let text = "running 1 test\ntest a ... ok\n\n\
                    test result: ok. 1 passed; 0 failed; 0 ignored; 1 filtered out\n";
        let case = report(&declared(2, &["a", "b"]), None, text, Some(0));
        assert!(!case.success);
    }

    #[test]
    fn a_runner_that_ignores_the_filter_is_detected_from_the_executed_names() {
        // New-format output (so the "no accounting" check cannot fire), but the
        // runner executed a test that does not match `--filter`.
        let text = "running 3 tests\ntest keep ... ok\ntest other ... ok\ntest more ... ok\n\n\
                    test result: ok. 3 passed; 0 failed; 0 ignored; 0 filtered out\n";
        let case = report(
            &declared(3, &["keep", "other", "more"]),
            Some("keep"),
            text,
            Some(0),
        );
        assert!(!case.success);
        assert!(
            case.validation_errors
                .iter()
                .any(|error| error.contains("filter was not applied")),
            "{:?}",
            case.validation_errors
        );
    }

    #[test]
    fn a_filter_that_matches_nothing_is_an_honest_empty_success() {
        let text =
            "running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 2 filtered out\n";
        let case = report(
            &declared(2, &["alpha", "beta"]),
            Some("none"),
            text,
            Some(0),
        );
        assert!(case.success, "{:?}", case.validation_errors);
        assert_eq!(case.filtered, 2);
        assert_eq!(case.executed, 0);
    }

    #[test]
    fn an_undeclared_reported_test_is_a_failure() {
        let case = report(
            &declared(1, &["a"]),
            None,
            "test a ... ok\ntest ghost ... ok\n\n2 passed; 0 failed\n",
            Some(0),
        );
        assert!(!case.success);
        assert!(case
            .validation_errors
            .iter()
            .any(|error| error.contains("ghost")));
    }

    #[test]
    fn nested_module_names_match_on_the_last_segment() {
        let case = report(
            &declared(1, &["nested"]),
            None,
            "test math::nested ... ok\n\n1 passed; 0 failed\n",
            Some(0),
        );
        assert!(case.success, "{:?}", case.validation_errors);
    }

    #[test]
    fn a_missing_exit_code_is_a_failure() {
        let case = report(
            &declared(1, &["a"]),
            None,
            "test a ... ok\n\n1 passed; 0 failed\n",
            None,
        );
        assert!(!case.success);
        assert!(case
            .validation_errors
            .iter()
            .any(|error| error.contains("without an exit code")));
    }

    #[test]
    fn a_negative_exit_code_is_a_failure_naming_the_exception() {
        let case = report(
            &declared(1, &["a"]),
            None,
            "test a ... ok\n\n1 passed; 0 failed\n",
            Some(-1073741819),
        );
        assert!(!case.success);
        assert!(case
            .validation_errors
            .iter()
            .any(|error| error.contains("0xC0000005")));
    }

    #[test]
    fn zero_declared_and_zero_reported_is_an_honest_empty_run() {
        let case = report(&declared(0, &[]), None, "\n0 passed; 0 failed\n", Some(0));
        assert!(case.success, "{:?}", case.validation_errors);
        assert_eq!(case.declared, 0);
    }
}
