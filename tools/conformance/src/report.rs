//! Deterministic console rendering and the machine-readable report
//! (`schema_version: 1`). User paths are sanitized before they reach the JSON.

use crate::case::{CaseEntry, TAXONOMIES};
use crate::json::Json;
use crate::runner::{CaseResult, Status, Toolchain};
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

pub fn print_header(
    version: &str,
    cases_root: &str,
    toolchain: Option<&Toolchain>,
    toolchain_error: &str,
    filter: Option<&str>,
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
    if let Some(filter) = filter {
        println!("filter:        {filter:?}");
    }
    println!();
}

pub fn print_case_line(result: &CaseResult, width: usize) {
    println!(
        "{:<7} {:<width$} {:>7} ms",
        result.status.label(),
        result.id,
        result.duration_ms,
        width = width
    );
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

pub fn print_summary(summary: Summary) {
    println!();
    println!(
        "summary: total={} pass={} fail={} skip={} not_run={}",
        summary.total, summary.pass, summary.fail, summary.skip, summary.not_run
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

/// Builds `schema_version: 1` report. Every string passed here is already
/// sanitized by the runner; this function also truncates oversized evidence.
pub fn build_report(
    toolchain: Option<&Toolchain>,
    started_at: &str,
    results: &[CaseResult],
) -> Json {
    let summary = summarize(results);
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
        CaseResult {
            id: id.to_string(),
            taxonomy: "lexer".to_string(),
            mode: Mode::CompilePass,
            status,
            duration_ms: 3,
            expected: "e".to_string(),
            actual: "a".to_string(),
            detail: String::new(),
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
        let report = build_report(None, "2025-11-09T15:00:00Z", &[result(Status::Pass, "a")]);
        let text = report.write_pretty();
        assert!(text.starts_with("{\n  \"schema_version\": 1,\n  \"toolchain\": \"<not found>\""));
        assert!(text.contains("\"summary\": {"));
        assert!(text.contains("\"not_run\": 0"));
        // The report must round-trip through the same parser.
        assert!(Json::parse(&text).is_ok());
    }
}
