//! Deterministic console rendering and the machine-readable report
//! (`schema_version: 1`). User paths are sanitized before they reach the JSON.

use crate::case::{CaseEntry, TAXONOMIES};
use crate::json::Json;
use crate::runner::{BuildStatus, CaseResult, Status, Toolchain};
use crate::util::{host_os, truncate_chars};
use std::path::Path;

pub const REPORT_SCHEMA_VERSION: i64 = 1;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub total: usize,
    pub pass: usize,
    pub fail: usize,
    pub skip: usize,
    pub not_run: usize,
}

pub fn summarize(results: &[CaseResult]) -> Summary {
    let mut summary = Summary::default();
    for result in results {
        summary.total += 1;
        match result.status {
            Status::Pass => summary.pass += 1,
            Status::Fail => summary.fail += 1,
            Status::Skip => summary.skip += 1,
            Status::NotRun => summary.not_run += 1,
        }
    }
    summary
}

/// The `check ⇒ build` dimension of a run. Its perimeter is every case whose
/// mode builds a program (see [`crate::case::Mode::builds_program`]).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BuildSummary {
    /// `--verify-build` was passed.
    pub requested: bool,
    /// Cases whose mode builds a program.
    pub perimeter: usize,
    /// Explicit `build` exit 0 after a passing check.
    pub verified: usize,
    /// `run-pass`/`run-fail` cases whose expected run outcome proves the build.
    pub implicit: usize,
    /// Check passed but build failed (`check ⇒ build` divergence).
    pub failed: usize,
    /// The build could not be evaluated here (no `rustc`, timeout, spawn error).
    pub environment: usize,
    /// Perimeter cases that were never measured (check failed, SKIP, NOT_RUN).
    pub not_measured: usize,
}

impl BuildSummary {
    /// Positive evidence that every perimeter case was measured and built.
    ///
    /// This says **nothing** about the rest of the run: a report can be
    /// `build_verified: true` while `fail` is non-zero for check-only cases
    /// (e.g. a `compile-fail` case that wrongly passes). Read `summary` too.
    pub fn build_verified(&self) -> bool {
        self.requested
            && self.perimeter > 0
            && self.failed == 0
            && self.environment == 0
            && self.not_measured == 0
    }

    /// Cases with positive build evidence.
    pub fn covered(&self) -> usize {
        self.verified + self.implicit
    }
}

pub fn summarize_build(results: &[CaseResult], requested: bool) -> BuildSummary {
    let mut summary = BuildSummary {
        requested,
        ..BuildSummary::default()
    };
    for result in results {
        if !result.mode.builds_program() {
            continue;
        }
        summary.perimeter += 1;
        match result.build {
            BuildStatus::Verified => summary.verified += 1,
            BuildStatus::Implicit => summary.implicit += 1,
            BuildStatus::Failed => summary.failed += 1,
            BuildStatus::Environment => summary.environment += 1,
            BuildStatus::NotAttempted | BuildStatus::NotApplicable => summary.not_measured += 1,
            BuildStatus::NotRequested => {}
        }
    }
    summary
}

pub fn print_header(
    version: &str,
    cases_root: &str,
    toolchain: Option<&Toolchain>,
    toolchain_error: &str,
    filter: Option<&str>,
    verify_build: bool,
) {
    println!("ravum-conformance {version} — Ravum Core 1.0 conformance harness");
    println!("report schema: {REPORT_SCHEMA_VERSION}");
    println!("cases root:    {cases_root}");
    match toolchain {
        Some(toolchain) => {
            println!(
                "toolchain:     {} ({})",
                toolchain.display, toolchain.version
            );
        }
        None => {
            println!("toolchain:     NOT FOUND");
            println!("               {toolchain_error}");
        }
    }
    if verify_build {
        println!("build check:   requested (--verify-build: check ⇒ build contract)");
    } else {
        println!("build check:   NOT requested (build correctness is NOT claimed)");
    }
    if let Some(filter) = filter {
        println!("filter:        {filter:?}");
    }
    println!();
}

pub fn print_case_line(result: &CaseResult, width: usize) {
    let mut line = format!(
        "{:<7} {:<width$} {:>7} ms",
        result.status.label(),
        result.id,
        result.duration_ms,
        width = width
    );
    if result.build != BuildStatus::NotRequested {
        line.push_str(&format!("  build={}", result.build.as_str()));
    }
    println!("{line}");
}

pub fn print_failures(results: &[CaseResult]) {
    let failures: Vec<&CaseResult> = results
        .iter()
        .filter(|result| result.status == Status::Fail)
        .collect();
    println!();
    if failures.is_empty() {
        println!("FAILURES: none");
    } else {
        println!("FAILURES ({})", failures.len());
        for (index, result) in failures.iter().enumerate() {
            println!();
            println!("{}. {}  [{}]", index + 1, result.id, result.mode.as_str());
            println!("   expected: {}", result.expected);
            println!("   actual:   {}", result.actual);
            if !result.detail.is_empty() {
                println!("   detail:");
                for line in result.detail.lines() {
                    println!("     {line}");
                }
            }
        }
    }
    let not_run: Vec<&CaseResult> = results
        .iter()
        .filter(|result| result.status == Status::NotRun)
        .collect();
    if !not_run.is_empty() {
        println!();
        println!("NOT_RUN ({})", not_run.len());
        for result in not_run {
            println!(
                "- {}  [{}]: {}",
                result.id,
                result.mode.as_str(),
                result.detail
            );
        }
    }
}

pub fn print_summary(summary: Summary, build: BuildSummary) {
    println!();
    println!(
        "summary: total={} pass={} fail={} skip={} not_run={}",
        summary.total, summary.pass, summary.fail, summary.skip, summary.not_run
    );
    if build.requested {
        println!(
            "build: perimeter={} covered={} verified={} implicit={} failed={} environment={} not_measured={}",
            build.perimeter,
            build.covered(),
            build.verified,
            build.implicit,
            build.failed,
            build.environment,
            build.not_measured
        );
        println!(
            "build_verified: {} (build dimension only — check `summary` for the rest of the run)",
            if build.build_verified() {
                "true"
            } else {
                "false"
            }
        );
    } else {
        println!(
            "build_verified: false — --verify-build was not passed; this report is NOT evidence of build correctness"
        );
    }
    println!(
        "note: SKIP and NOT_RUN are never PASS; a green run is an observation on this corpus, host and commit, not a proof"
    );
}

pub fn print_list(entries: &[CaseEntry], cases_root: &str, version: &str) {
    println!("ravum-conformance {version} — case inventory (schema {REPORT_SCHEMA_VERSION})");
    println!("cases root: {cases_root}");
    let invalid = entries.iter().filter(|entry| entry.spec.is_err()).count();
    println!(
        "total: {} (valid: {}, invalid: {})",
        entries.len(),
        entries.len() - invalid,
        invalid
    );
    println!();
    println!("by taxonomy:");
    for taxonomy in TAXONOMIES {
        let count = entries
            .iter()
            .filter(|entry| entry.taxonomy == *taxonomy)
            .count();
        println!("  {taxonomy:<12} {count}");
    }
    let unsupported = entries
        .iter()
        .filter(|entry| {
            entry
                .spec
                .as_ref()
                .map(|spec| spec.unsupported)
                .unwrap_or(false)
        })
        .count();
    println!();
    println!("unsupported markers: {unsupported}");
    let count_skipped_on = |os: &str| {
        entries
            .iter()
            .filter(|entry| {
                entry
                    .spec
                    .as_ref()
                    .map(|spec| spec.skip_on.iter().any(|value| value == os))
                    .unwrap_or(false)
            })
            .count()
    };
    println!(
        "skip_on: windows={} linux={} macos={} (host: {})",
        count_skipped_on("windows"),
        count_skipped_on("linux"),
        count_skipped_on("macos"),
        host_os()
    );
    if invalid > 0 {
        println!();
        println!("INVALID CASES:");
        for entry in entries.iter().filter(|entry| entry.spec.is_err()) {
            let message = match &entry.spec {
                Err(message) => message,
                Ok(_) => unreachable!(),
            };
            println!("  {} ({}): {}", entry.id, entry.rel_dir, message);
        }
    }
    println!();
    println!("CASES:");
    let width = entries
        .iter()
        .map(|entry| entry.id.chars().count())
        .max()
        .unwrap_or(20)
        .clamp(20, 64);
    for entry in entries {
        match &entry.spec {
            Ok(spec) => {
                let mut tags = String::new();
                if !spec.skip_on.is_empty() {
                    tags.push_str(&format!(" skip_on={}", spec.skip_on.join(",")));
                }
                if spec.unsupported {
                    tags.push_str(" unsupported");
                }
                if !spec.notes.is_empty() {
                    tags.push_str(&format!(" # {}", truncate_chars(&spec.notes, 100)));
                }
                println!(
                    "  {:<width$}  {}{}",
                    entry.id,
                    spec.mode.as_str(),
                    tags,
                    width = width
                );
            }
            Err(message) => {
                println!(
                    "  {:<width$}  INVALID # {}",
                    entry.id,
                    truncate_chars(message, 200),
                    width = width
                );
            }
        }
    }
}

/// Builds a `schema_version: 1` report. Every string passed here is already
/// sanitized by the runner; this function also truncates oversized evidence.
///
/// The build fields (`verify_build`, `build_verified`, `build_summary` and the
/// per-case `build`/`build_detail`) are **additive**: `schema_version` stays 1
/// and a reader that ignores them still sees the same case/summary shape. A
/// report produced without `--verify-build` always carries
/// `build_verified: false`.
pub fn build_report(
    toolchain: Option<&Toolchain>,
    started_at: &str,
    results: &[CaseResult],
    verify_build: bool,
    filter: Option<&str>,
) -> Json {
    let summary = summarize(results);
    let build = summarize_build(results, verify_build);
    let cases: Vec<Json> = results
        .iter()
        .map(|result| {
            Json::object(vec![
                ("id", Json::string(result.id.clone())),
                ("taxonomy", Json::string(result.taxonomy.clone())),
                ("mode", Json::string(result.mode.as_str())),
                ("status", Json::string(result.status.as_str())),
                ("duration_ms", Json::uint(result.duration_ms as usize)),
                (
                    "expected",
                    Json::string(truncate_chars(&result.expected, 2_000)),
                ),
                (
                    "actual",
                    Json::string(truncate_chars(&result.actual, 2_000)),
                ),
                (
                    "detail",
                    Json::string(truncate_chars(&result.detail, 6_000)),
                ),
                ("build", Json::string(result.build.as_str())),
                (
                    "build_detail",
                    Json::string(truncate_chars(&result.build_detail, 4_000)),
                ),
            ])
        })
        .collect();
    Json::object(vec![
        ("schema_version", Json::int(REPORT_SCHEMA_VERSION)),
        (
            "toolchain",
            Json::string(
                toolchain
                    .map(|toolchain| toolchain.display.clone())
                    .unwrap_or_else(|| "<not found>".to_string()),
            ),
        ),
        (
            "toolchain_version",
            Json::string(
                toolchain
                    .map(|toolchain| toolchain.version.clone())
                    .unwrap_or_else(|| "UNKNOWN".to_string()),
            ),
        ),
        ("started_at", Json::string(started_at.to_string())),
        ("filter", Json::string(filter.unwrap_or("<none>"))),
        ("verify_build", Json::bool(verify_build)),
        ("build_verified", Json::bool(build.build_verified())),
        (
            "build_summary",
            Json::object(vec![
                ("perimeter", Json::uint(build.perimeter)),
                ("verified", Json::uint(build.verified)),
                ("implicit", Json::uint(build.implicit)),
                ("failed", Json::uint(build.failed)),
                ("environment", Json::uint(build.environment)),
                ("not_measured", Json::uint(build.not_measured)),
            ]),
        ),
        ("cases", Json::Array(cases)),
        (
            "summary",
            Json::object(vec![
                ("total", Json::uint(summary.total)),
                ("pass", Json::uint(summary.pass)),
                ("fail", Json::uint(summary.fail)),
                ("skip", Json::uint(summary.skip)),
                ("not_run", Json::uint(summary.not_run)),
            ]),
        ),
    ])
}

pub fn write_report(path: &Path, report: &Json) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "cannot create report directory {}: {error}",
                    parent.display()
                )
            })?;
        }
    }
    std::fs::write(path, report.write_pretty())
        .map_err(|error| format!("cannot write report {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::case::Mode;

    fn result(status: Status, id: &str) -> CaseResult {
        result_with_build(status, id, Mode::CompilePass, BuildStatus::NotRequested)
    }

    fn result_with_build(status: Status, id: &str, mode: Mode, build: BuildStatus) -> CaseResult {
        CaseResult {
            id: id.to_string(),
            taxonomy: "lexer".to_string(),
            mode,
            status,
            duration_ms: 3,
            expected: "e".to_string(),
            actual: "a".to_string(),
            detail: String::new(),
            build,
            build_detail: String::new(),
        }
    }

    #[test]
    fn summary_counts_every_status() {
        let results = vec![
            result(Status::Pass, "a"),
            result(Status::Fail, "b"),
            result(Status::Skip, "c"),
            result(Status::NotRun, "d"),
        ];
        let summary = summarize(&results);
        assert_eq!(
            summary,
            Summary {
                total: 4,
                pass: 1,
                fail: 1,
                skip: 1,
                not_run: 1
            }
        );
    }

    #[test]
    fn report_has_stable_schema_and_order() {
        let report = build_report(
            None,
            "2025-11-09T15:00:00Z",
            &[result(Status::Pass, "a")],
            false,
            None,
        );
        let text = report.write_pretty();
        assert!(text.starts_with("{\n  \"schema_version\": 1,\n  \"toolchain\": \"<not found>\""));
        assert!(text.contains("\"summary\": {"));
        assert!(text.contains("\"not_run\": 0"));
        // A report produced without the flag must never claim build correctness.
        assert!(text.contains("\"verify_build\": false"));
        assert!(text.contains("\"build_verified\": false"));
        assert!(text.contains("\"build\": \"not_requested\""));
        // The report must round-trip through the same parser.
        assert!(Json::parse(&text).is_ok());
    }

    #[test]
    fn build_summary_requires_every_perimeter_case() {
        let results = vec![
            result_with_build(
                Status::Pass,
                "explicit",
                Mode::CompilePass,
                BuildStatus::Verified,
            ),
            result_with_build(Status::Pass, "run", Mode::RunPass, BuildStatus::Implicit),
        ];
        let build = summarize_build(&results, true);
        assert_eq!(build.perimeter, 2);
        assert_eq!(build.covered(), 2);
        assert!(build.build_verified(), "{build:?}");

        // A build failure after a passing check breaks the contract.
        let failed = vec![
            result_with_build(
                Status::Pass,
                "explicit",
                Mode::CompilePass,
                BuildStatus::Verified,
            ),
            result_with_build(
                Status::Fail,
                "broken",
                Mode::CompilePass,
                BuildStatus::Failed,
            ),
        ];
        let build = summarize_build(&failed, true);
        assert_eq!(build.failed, 1);
        assert!(!build.build_verified(), "{build:?}");

        // An environment failure is NOT positive evidence.
        let environment = vec![result_with_build(
            Status::NotRun,
            "no-rustc",
            Mode::CompilePass,
            BuildStatus::Environment,
        )];
        let build = summarize_build(&environment, true);
        assert_eq!(build.environment, 1);
        assert!(!build.build_verified(), "{build:?}");

        // A skipped/NOT_RUN perimeter case leaves the contract unmeasured.
        let skipped = vec![result_with_build(
            Status::Skip,
            "skipped",
            Mode::FmtIdempotent,
            BuildStatus::NotAttempted,
        )];
        let build = summarize_build(&skipped, true);
        assert_eq!(build.not_measured, 1);
        assert!(!build.build_verified(), "{build:?}");
    }

    #[test]
    fn report_without_the_flag_cannot_be_build_evidence() {
        // Even a run whose perimeter cases happen to build must not claim
        // build correctness when --verify-build was absent.
        let results = vec![result_with_build(
            Status::Pass,
            "p",
            Mode::CompilePass,
            BuildStatus::NotRequested,
        )];
        let build = summarize_build(&results, false);
        assert!(!build.build_verified(), "{build:?}");
        let report = build_report(None, "2025-11-09T15:00:00Z", &results, false, None);
        let text = report.write_pretty();
        assert!(text.contains("\"verify_build\": false"));
        assert!(text.contains("\"build_verified\": false"));
    }

    #[test]
    fn report_carries_the_build_dimension_when_requested() {
        let results = vec![
            result_with_build(
                Status::Pass,
                "explicit",
                Mode::CompilePass,
                BuildStatus::Verified,
            ),
            result_with_build(Status::Pass, "run", Mode::RunPass, BuildStatus::Implicit),
            // compile-fail is outside the perimeter: it must not appear in
            // the perimeter count even when the flag is requested.
            result_with_build(
                Status::Pass,
                "negative",
                Mode::CompileFail,
                BuildStatus::NotApplicable,
            ),
        ];
        let build = summarize_build(&results, true);
        assert_eq!(build.perimeter, 2);
        let report = build_report(
            None,
            "2025-11-09T15:00:00Z",
            &results,
            true,
            Some("ownership/"),
        );
        let text = report.write_pretty();
        assert!(text.contains("\"verify_build\": true"));
        assert!(text.contains("\"build_verified\": true"));
        assert!(text.contains("\"perimeter\": 2"));
        assert!(text.contains("\"build\": \"verified\""));
        assert!(text.contains("\"build\": \"implicit\""));
        assert!(text.contains("\"build\": \"not_applicable\""));
        assert!(text.contains("\"filter\": \"ownership/\""));
    }

    #[test]
    fn filter_scope_is_recorded_in_the_report() {
        let report = build_report(None, "2025-11-09T15:00:00Z", &[], true, None);
        let text = report.write_pretty();
        assert!(text.contains("\"filter\": \"<none>\""));
        // No perimeter case → nothing was verified, even with the flag.
        assert!(text.contains("\"build_verified\": false"));
    }
}
