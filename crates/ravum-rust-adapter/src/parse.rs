//! Convert `rustc --error-format=json` output into Ravum structured diagnostics.

use ravum_protocol::{
    Diagnostic, DiagnosticSet, DiagnosticSpan, Severity, SnapshotId, DIAGNOSTICS_SCHEMA_VERSION,
};
use serde_json::Value;

fn severity(level: &str) -> Severity {
    match level {
        "error" | "error: internal compiler error" => Severity::Error,
        "warning" => Severity::Warning,
        "help" => Severity::Help,
        _ => Severity::Note,
    }
}

/// Parse one rustc JSON stream. Non-JSON lines are kept only when they carry an
/// error, so a linker crash is never silently swallowed.
pub fn parse_rustc_json(
    stderr: &str,
    snapshot: &SnapshotId,
    tool: &str,
    tool_version: &str,
    exit_success: bool,
) -> DiagnosticSet {
    let mut set = DiagnosticSet::new(snapshot.clone(), tool, tool_version);
    let mut leftovers: Vec<String> = Vec::new();
    let mut unparsed: Vec<String> = Vec::new();

    for line in stderr.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            unparsed.push(line.to_string());
            if line.starts_with("error") {
                leftovers.push(line.to_string());
            }
            continue;
        };
        // Two JSON shapes exist and both must be accepted:
        // * `cargo --message-format=json` wraps a diagnostic as
        //   {"reason":"compiler-message","message":{...}};
        // * the direct `rustc --error-format=json` stream used by this adapter emits the
        //   diagnostic object itself: {"$message_type":"diagnostic","message":...,
        //   "code":...,"level":...,"spans":[...]}.
        // Accepting only the cargo shape silently reduced every failed check to the
        // `no-diagnostic` fallback (observed in MVP-0b with rustc 1.95.0).
        let message = match value.get("reason").and_then(Value::as_str) {
            Some("compiler-message") => Some(&value["message"]),
            _ if value.get("$message_type").and_then(Value::as_str) == Some("diagnostic") => {
                Some(&value)
            }
            _ => None,
        };
        let Some(message) = message else {
            continue;
        };
        if let Some(diag) = to_diagnostic(message) {
            // rustc also emits a trailing summary line ("aborting due to N previous
            // errors"); it is not a separate finding and would double the error count.
            if diag.code.is_none() && diag.message.starts_with("aborting due to") {
                continue;
            }
            set.diagnostics.push(diag);
        }
    }

    for raw in leftovers {
        let mut diag = Diagnostic::error(raw);
        diag.code = Some("raw-stderr".to_owned());
        set.diagnostics.push(diag);
    }

    if !exit_success && !set.has_errors() {
        // Never lose the reason: a failing tool with no structured diagnostic must
        // still report what it printed, otherwise the failure is undebuggable.
        let mut detail = unparsed.join(" | ");
        if detail.len() > 800 {
            detail.truncate(800);
            detail.push_str("...");
        }
        let message = if detail.is_empty() {
            "rustc exited with a failure status and produced no output at all".to_owned()
        } else {
            format!("rustc exited with a failure status: {detail}")
        };
        let mut diag = Diagnostic::error(message);
        diag.code = Some("no-diagnostic".to_owned());
        set.diagnostics.push(diag);
    }

    set.schema_version = DIAGNOSTICS_SCHEMA_VERSION;
    set
}

fn to_diagnostic(message: &Value) -> Option<Diagnostic> {
    let level = message.get("level")?.as_str()?;
    let text = message.get("message")?.as_str()?.to_owned();
    let code = message
        .get("code")
        .and_then(|c| c.get("code"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let span = message
        .get("spans")
        .and_then(Value::as_array)
        .and_then(|spans| {
            spans
                .iter()
                .find(|s| s.get("is_primary").and_then(Value::as_bool) == Some(true))
                .or_else(|| spans.first())
        })
        .map(to_span);
    let rendered = message
        .get("rendered")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();

    Some(Diagnostic {
        code,
        level: severity(level),
        message: text,
        span,
        rendered,
    })
}

fn to_span(span: &Value) -> DiagnosticSpan {
    DiagnosticSpan {
        file: span
            .get("file_name")
            .and_then(Value::as_str)
            .unwrap_or("<unknown>")
            .to_owned(),
        line_start: span.get("line_start").and_then(Value::as_u64).unwrap_or(0) as u32,
        column_start: span.get("column_start").and_then(Value::as_u64).unwrap_or(0) as u32,
        line_end: span.get("line_end").and_then(Value::as_u64).unwrap_or(0) as u32,
        column_end: span.get("column_end").and_then(Value::as_u64).unwrap_or(0) as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ERROR_LINE: &str = r#"{"reason":"compiler-message","message":{"message":"cannot find value `x` in this scope","code":{"code":"E0425","explanation":null},"level":"error","spans":[{"file_name":"input.rs","line_start":2,"column_start":5,"line_end":2,"column_end":6,"is_primary":true}],"rendered":"error[E0425]: cannot find value"}}"#;

    #[test]
    fn parses_error_with_code_and_span() {
        let set = parse_rustc_json(ERROR_LINE, &SnapshotId::new("s"), "rustc", "1.95.0", false);
        assert_eq!(set.error_count(), 1);
        let d = &set.diagnostics[0];
        assert_eq!(d.code.as_deref(), Some("E0425"));
        assert_eq!(d.span.as_ref().unwrap().line_start, 2);
        assert_eq!(d.level, Severity::Error);
    }

    #[test]
    fn failed_process_without_diagnostic_still_reports_an_error() {
        let set = parse_rustc_json("garbage from the tool", &SnapshotId::new("s"), "rustc", "1", false);
        assert!(set.has_errors());
        assert_eq!(set.diagnostics[0].code.as_deref(), Some("no-diagnostic"));
        assert!(
            set.diagnostics[0].message.contains("garbage from the tool"),
            "the raw output must be preserved: {}",
            set.diagnostics[0].message
        );
    }

    #[test]
    fn successful_warning_only_run_has_no_error() {
        let warn = r#"{"reason":"compiler-message","message":{"message":"unused variable","code":{"code":"unused_variables"},"level":"warning","spans":[]}}"#;
        let set = parse_rustc_json(warn, &SnapshotId::new("s"), "rustc", "1", true);
        assert!(!set.has_errors());
        assert_eq!(set.warning_count(), 1);
    }

    /// Direct `rustc --error-format=json` shape (not cargo's wrapper), reproduced from
    /// rustc 1.95.0 output. Both the real error and the trailing summary are present;
    /// only the real finding must be kept.
    #[test]
    fn parses_direct_rustc_json_shape_and_ignores_the_summary_line() {
        let stream = concat!(
            r#"{"$message_type":"diagnostic","message":"mismatched types","code":{"code":"E0308","explanation":null},"level":"error","spans":[{"file_name":"unit.rs","line_start":2,"column_start":5,"line_end":2,"column_end":6,"is_primary":true}],"children":[],"rendered":"error[E0308]: mismatched types"}"#,
            "\n",
            r#"{"$message_type":"diagnostic","message":"aborting due to 1 previous error","code":null,"level":"error","spans":[],"children":[],"rendered":"error: aborting due to 1 previous error"}"#,
            "\n",
            r#"{"$message_type":"diagnostic","message":"For more information about this error, try `rustc --explain E0308`.","code":null,"level":"failure-note","spans":[],"children":[],"rendered":"For more information about this error"}"#,
            "\n"
        );
        let set = parse_rustc_json(stream, &SnapshotId::new("s"), "rustc", "1.95.0", false);
        assert_eq!(set.error_count(), 1, "diags: {:?}", set.diagnostics);
        let d = set
            .diagnostics
            .iter()
            .find(|d| d.code.is_some())
            .expect("structured diagnostic with a code");
        assert_eq!(d.code.as_deref(), Some("E0308"));
        assert_eq!(d.message, "mismatched types");
        assert_eq!(d.span.as_ref().map(|s| s.line_start), Some(2));
    }
}
