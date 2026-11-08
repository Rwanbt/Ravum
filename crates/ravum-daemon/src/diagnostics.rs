//! Structured diagnostics parsed from `rustc --error-format=json`.
//!
//! Only public CLI output is consumed: no `rustc_*` API, no `RUSTC_BOOTSTRAP`.
//! The parser is deliberately tolerant on the container shape (rustc has used
//! both `{"reason":"compiler-message"}` and `{"$message_type":"diagnostic"}`)
//! but strict on content: a failing run with no structured diagnostic still
//! produces an explicit synthetic `no-diagnostic` error, so a crash is never
//! reported as a clean analysis.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

/// Severity as reported by rustc's JSON stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Note,
    Help,
    FailureNote,
    Other,
}

impl Severity {
    pub fn from_rustc(level: &str) -> Self {
        match level {
            "error" | "error: internal compiler error" => Severity::Error,
            "warning" => Severity::Warning,
            "note" => Severity::Note,
            "help" => Severity::Help,
            "failure-note" => Severity::FailureNote,
            _ => Severity::Other,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Note => "note",
            Severity::Help => "help",
            Severity::FailureNote => "failure_note",
            Severity::Other => "other",
        }
    }

    pub fn is_error(self) -> bool {
        matches!(self, Severity::Error)
    }
}

/// One normalized diagnostic. The `file` is always a logical path relative to
/// the submitted unit root when rustc reports a path below the checkout root;
/// absolute daemon-internal paths never leak into evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: Option<String>,
    pub message: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub end_line: Option<u32>,
    pub end_column: Option<u32>,
    /// Rendered human output, bounded in size. Kept for display, never used for
    /// equality between modes (the structured fields are).
    pub rendered: Option<String>,
}

impl Diagnostic {
    /// Canonical one-line signature used to compare modes. Two modes that see
    /// the same source must produce the same signature set.
    pub fn signature(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}|{}",
            self.severity.as_str(),
            self.code.as_deref().unwrap_or("-"),
            self.file.as_deref().unwrap_or("-"),
            self.line.map(|v| v.to_string()).unwrap_or_else(|| "-".to_owned()),
            self.column.map(|v| v.to_string()).unwrap_or_else(|| "-".to_owned()),
            self.end_line.map(|v| v.to_string()).unwrap_or_else(|| "-".to_owned()),
            self.message
        )
    }
}

/// Sorted signature set: order-independent comparison between modes.
pub fn signature_set(diagnostics: &[Diagnostic]) -> Vec<String> {
    let mut set: Vec<String> = diagnostics.iter().map(Diagnostic::signature).collect();
    set.sort();
    set
}

#[derive(Debug, Clone, Default)]
pub struct ParsedOutput {
    pub diagnostics: Vec<Diagnostic>,
    pub raw_lines: Vec<String>,
    pub parse_errors: usize,
}

impl ParsedOutput {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity.is_error())
    }

    pub fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity.is_error())
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count()
    }
}

/// Parses a full rustc stderr stream.
///
/// `checkout_root` is the directory containing the submitted files: when a
/// reported `file_name` lives below it, the path is reduced to a logical
/// relative path so a cold run in a temporary directory and a daemon run in its
/// snapshot directory are comparable.
pub fn parse_rustc_json(
    stderr: &str,
    checkout_root: Option<&Path>,
    exit_success: bool,
) -> ParsedOutput {
    let mut parsed = ParsedOutput::default();
    let root = checkout_root.map(|path| normalize_path(&path.to_string_lossy()));

    for line in stderr.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            parsed.parse_errors += 1;
            parsed.raw_lines.push(trimmed.to_owned());
            continue;
        };
        let message = match diagnostic_message(&value) {
            Some(message) => message,
            None => continue,
        };
        match diagnostic_from_message(message, root.as_deref()) {
            Some(diagnostic) => parsed.diagnostics.push(diagnostic),
            None => {
                parsed.parse_errors += 1;
                parsed.raw_lines.push(trimmed.to_owned());
            }
        }
    }

    if !exit_success && !parsed.has_errors() {
        let detail = parsed.raw_lines.join(" | ");
        let detail = if detail.len() > 800 {
            format!("{}...", &detail[..800])
        } else {
            detail
        };
        let message = if detail.is_empty() {
            "rustc exited with a failure status and produced no output at all".to_owned()
        } else {
            format!("rustc exited with a failure status: {detail}")
        };
        parsed.diagnostics.push(Diagnostic {
            severity: Severity::Error,
            code: Some("no-diagnostic".to_owned()),
            message,
            file: None,
            line: None,
            column: None,
            end_line: None,
            end_column: None,
            rendered: None,
        });
    }

    parsed
}

fn diagnostic_message(value: &Value) -> Option<&Value> {
    if value.get("reason").and_then(Value::as_str) == Some("compiler-message") {
        return value.get("message");
    }
    if let Some(kind) = value.get("$message_type").and_then(Value::as_str) {
        return (kind == "diagnostic").then_some(value);
    }
    // Tolerate the bare-message shape without a discriminator: it must at least
    // carry a level and a message string to be treated as a diagnostic.
    if value.get("level").is_some() && value.get("message").map(Value::is_string) == Some(true) {
        return Some(value);
    }
    None
}

fn diagnostic_from_message(message: &Value, root: Option<&str>) -> Option<Diagnostic> {
    let level = message.get("level")?.as_str()?;
    let text = message.get("message")?.as_str()?.to_owned();
    let code = message
        .get("code")
        .and_then(|code| {
            code.get("code")
                .and_then(Value::as_str)
                .or_else(|| code.as_str())
        })
        .map(str::to_owned);
    let span = message
        .get("spans")
        .and_then(Value::as_array)
        .and_then(|spans| {
            spans
                .iter()
                .find(|span| span.get("is_primary").and_then(Value::as_bool) == Some(true))
                .or_else(|| {
                    spans
                        .iter()
                        .find(|span| span.get("file_name").is_some())
                })
        });
    let rendered = message
        .get("rendered")
        .and_then(Value::as_str)
        .map(|text| {
            if text.len() > 8192 {
                format!("{}...", &text[..8192])
            } else {
                text.to_owned()
            }
        });
    let file = span
        .and_then(|span| span.get("file_name"))
        .and_then(Value::as_str)
        .map(|name| normalize_file(name, root));
    let number = |key: &str| {
        span.and_then(|span| span.get(key))
            .and_then(Value::as_u64)
            .map(|value| value.min(u32::MAX as u64) as u32)
    };
    Some(Diagnostic {
        severity: Severity::from_rustc(level),
        code,
        message: text,
        file,
        line: number("line_start"),
        column: number("column_start"),
        end_line: number("line_end"),
        end_column: number("column_end"),
        rendered,
    })
}

/// Reduces an absolute rustc path to a logical path relative to `root`.
fn normalize_file(name: &str, root: Option<&str>) -> String {
    let normalized = normalize_path(name);
    if let Some(root) = root {
        let root = root.trim_end_matches('/');
        if let Some(relative) = normalized.strip_prefix(root) {
            let relative = relative.trim_start_matches('/');
            if !relative.is_empty() {
                return relative.to_owned();
            }
        }
    }
    normalized
}

/// Separator normalization only: no canonicalization, so a relative base and an
/// absolute path are compared literally.
pub fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    const NEW_FORMAT_ERROR: &str = r#"{"$message_type":"diagnostic","message":"cannot find value `x` in this scope","code":{"code":"E0425","explanation":null},"level":"error","spans":[{"file_name":"C:\\work\\unit\\src\\lib.rs","line_start":2,"line_end":2,"column_start":17,"column_end":18,"is_primary":true}],"children":[],"rendered":"error[E0425]: cannot find value `x` in this scope\n"}"#;

    const OLD_FORMAT_ERROR: &str = r#"{"reason":"compiler-message","message":{"message":"unused variable: `x`","code":{"code":"unused_variables"},"level":"warning","spans":[{"file_name":"src/notes.rs","line_start":4,"line_end":4,"column_start":9,"column_end":10,"is_primary":true}],"rendered":"warning: unused variable: `x`\n"}}"#;

    #[test]
    fn parses_new_json_format_and_normalizes_path() {
        let parsed = parse_rustc_json(
            NEW_FORMAT_ERROR,
            Some(Path::new(r"C:\work\unit")),
            false,
        );
        assert_eq!(parsed.error_count(), 1);
        let diagnostic = &parsed.diagnostics[0];
        assert_eq!(diagnostic.code.as_deref(), Some("E0425"));
        assert_eq!(diagnostic.file.as_deref(), Some("src/lib.rs"));
        assert_eq!(diagnostic.line, Some(2));
        assert_eq!(diagnostic.column, Some(17));
    }

    #[test]
    fn parses_legacy_compiler_message_format() {
        let parsed = parse_rustc_json(OLD_FORMAT_ERROR, Some(Path::new("/work/unit")), true);
        assert_eq!(parsed.warning_count(), 1);
        assert_eq!(parsed.diagnostics[0].file.as_deref(), Some("src/notes.rs"));
    }

    #[test]
    fn summary_diagnostics_and_build_events_are_distinguished() {
        let stream = concat!(
            r#"{"reason":"build-finished","success":true}"#,
            "\n",
            r#"{"$message_type":"diagnostic","message":"aborting due to 1 previous error","code":null,"level":"error","spans":[]}"#,
        );
        let parsed = parse_rustc_json(stream, None, false);
        assert_eq!(parsed.diagnostics.len(), 1);
        assert_eq!(parsed.diagnostics[0].line, None);
    }

    #[test]
    fn failed_run_without_structured_diagnostic_still_errors() {
        let parsed = parse_rustc_json("fatal: linker exploded", None, false);
        assert!(parsed.has_errors());
        assert_eq!(parsed.diagnostics[0].code.as_deref(), Some("no-diagnostic"));
        assert!(parsed.diagnostics[0].message.contains("linker exploded"));
    }

    #[test]
    fn signatures_are_order_independent() {
        let stream = format!("{NEW_FORMAT_ERROR}\n{OLD_FORMAT_ERROR}");
        let parsed = parse_rustc_json(&stream, None, false);
        let mut reversed = parsed.diagnostics.clone();
        reversed.reverse();
        assert_eq!(signature_set(&parsed.diagnostics), signature_set(&reversed));
    }
}
