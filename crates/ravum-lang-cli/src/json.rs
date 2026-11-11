//! Stable machine-readable output (`schema_version: 1`).
//!
//! The envelope follows `spec/language/diagnostics.md` §7 while keeping the
//! compatibility fields required by the CLI mission (`success`, flat
//! `file/line/column/end_line/end_column` on each diagnostic). Unknown fields
//! must be ignored by consumers; `schema_version` only changes on a breaking
//! change. Exactly one JSON object is printed on stdout, on one line, with no
//! colour and no other text.

use ravum_lang_syntax::diag::Diagnostic;
use ravum_lang_syntax::source::SourceMap;
use ravum_lang_syntax::span::Span;
use serde::Serialize;
use serde_json::Value;

pub const SCHEMA_VERSION: u32 = 1;
pub const TOOL: &str = "ravum-lang";

#[derive(Debug, Serialize)]
pub struct JsonEnvelope {
    pub schema_version: u32,
    pub tool: &'static str,
    pub tool_version: &'static str,
    pub spec_version: &'static str,
    pub command: String,
    /// `success` | `failure` | `unsupported`
    pub status: String,
    pub exit_code: i32,
    /// Compatibility with the CLI mission contract.
    pub success: bool,
    pub target: JsonTarget,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependencies_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lockfile_stale: Option<bool>,
    pub summary: Value,
    pub diagnostics: Vec<JsonDiagnostic>,
}

#[derive(Debug, Serialize)]
pub struct JsonTarget {
    /// `file` | `package` | `workspace`
    pub kind: String,
    pub path: String,
}

#[derive(Debug, Serialize)]
pub struct JsonDiagnostic {
    pub code: String,
    pub severity: String,
    pub phase: String,
    pub message: String,
    /// Flat compatibility view (0/`null` when the diagnostic has no source).
    pub file: Option<String>,
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
    /// `spec/language/diagnostics.md` §7.3 span object.
    pub primary: JsonSpan,
    pub labels: Vec<JsonSpan>,
    pub notes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct JsonSpan {
    pub file: Option<String>,
    pub start_byte: u32,
    pub end_byte: u32,
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
    pub start_utf16: u32,
    pub end_utf16: u32,
}

/// Metadata needed to build the envelope for one command.
pub struct ReportMeta {
    pub command: &'static str,
    pub success: bool,
    pub exit_code: i32,
    pub target_kind: &'static str,
    pub target_path: String,
    pub profile: Option<String>,
    pub dependencies_digest: Option<String>,
    pub lockfile_stale: Option<bool>,
}

fn utf16_offset(text: &str, byte: u32) -> u32 {
    let mut offset = (byte as usize).min(text.len());
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    text[..offset].encode_utf16().count() as u32
}

fn span_object(sources: &SourceMap, span: Span) -> JsonSpan {
    if span.is_dummy() {
        return JsonSpan {
            file: None,
            start_byte: 0,
            end_byte: 0,
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 0,
            start_utf16: 0,
            end_utf16: 0,
        };
    }
    let Some(file) = sources.file(span.file) else {
        return JsonSpan {
            file: None,
            start_byte: 0,
            end_byte: 0,
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 0,
            start_utf16: 0,
            end_utf16: 0,
        };
    };
    let (start_line, start_col) = file.line_col(span.start);
    let (end_line, end_col) = file.line_col(span.end);
    JsonSpan {
        file: Some(file.path.clone()),
        start_byte: span.start,
        end_byte: span.end,
        start_line,
        start_col,
        end_line,
        end_col,
        start_utf16: utf16_offset(&file.text, span.start),
        end_utf16: utf16_offset(&file.text, span.end),
    }
}

/// Phase from the code prefix, as defined by the canonical registry
/// (`spec/language/diagnostics.md` §3). Every registered code reports exactly
/// the phase its registry row names (§4 fixes the phase order); the irregular
/// codes whose prefix lies about their phase are listed explicitly below, and
/// `every_registered_code_reports_its_registry_phase` fails when a registry
/// row and this mapping drift apart.
pub fn phase_for_code(code: &str) -> &'static str {
    match code {
        "E-UNSUPPORTED-EDITION" | "E-MOD-BOTH-ROOTS" | "E-MOD-RESERVED-NAME" => "modules",
        // Lexical despite the `E-UNSUPPORTED-` prefix (§4 phase 1).
        "E-UNSUPPORTED-IDENT" | "E-UNSUPPORTED-TOKEN" => "lex",
        // Emitted by the parser on `impl Drop` / destructors.
        "E-UNSUPPORTED-DROP-IMPL" => "parse",
        // Declared-effect and FFI-safety checks run in the resolver.
        "E-UNSUPPORTED-EFFECT" | "E-FFI-TYPE" => "resolve",
        "E-UNSAFE-CONTEXT" => "types",
        // Resource limits are enforced by the loader before any analysis: the
        // registry files them under `parse` (§3, §4).
        "E-LIMIT-FILE-SIZE" | "E-LIMIT-FILES" => "parse",
        _ if code.starts_with("E-MOD") => "modules",
        _ if code.starts_with("E-LEX") => "lex",
        _ if code.starts_with("E-PARSE") => "parse",
        _ if code.starts_with("E-UNSUPPORTED") => "parse",
        _ if code.starts_with("E-RES") || code.starts_with("W-RES") => "resolve",
        _ if code.starts_with("E-TYP") => "types",
        _ if code.starts_with("E-OWN") || code.starts_with("W-OWN") => "ownership",
        _ if code.starts_with("E-EFF") => "effects",
        _ if code.starts_with("E-CODEGEN") => "codegen",
        _ => "tool",
    }
}

pub fn diagnostic(diag: &Diagnostic, sources: &SourceMap) -> JsonDiagnostic {
    let primary = span_object(sources, diag.span);
    let (file, line, column, end_line, end_column) = match &primary.file {
        Some(_) => (
            primary.file.clone(),
            primary.start_line,
            primary.start_col,
            primary.end_line,
            primary.end_col,
        ),
        None => (None, 0, 0, 0, 0),
    };
    let notes = diag
        .notes
        .iter()
        .map(|note| match note.span {
            Some(span) if !span.is_dummy() => {
                format!("{} ({})", note.message, sources.location(span))
            }
            _ => note.message.clone(),
        })
        .collect();
    JsonDiagnostic {
        code: diag.code.clone(),
        severity: diag.severity.as_str().to_string(),
        phase: phase_for_code(&diag.code).to_string(),
        message: diag.message.clone(),
        file,
        line,
        column,
        end_line,
        end_column,
        primary,
        labels: Vec::new(),
        notes,
    }
}

pub fn diagnostics(diags: &[Diagnostic], sources: &SourceMap) -> Vec<JsonDiagnostic> {
    diags.iter().map(|diag| diagnostic(diag, sources)).collect()
}

/// A diagnostic not attached to source (toolchain/usage failure), still using
/// the stable schema so `--json` output is always parseable.
pub fn synthetic(code: &str, severity: &str, message: impl Into<String>) -> JsonDiagnostic {
    JsonDiagnostic {
        code: code.to_string(),
        severity: severity.to_string(),
        phase: phase_for_code(code).to_string(),
        message: message.into(),
        file: None,
        line: 0,
        column: 0,
        end_line: 0,
        end_column: 0,
        primary: JsonSpan {
            file: None,
            start_byte: 0,
            end_byte: 0,
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 0,
            start_utf16: 0,
            end_utf16: 0,
        },
        labels: Vec::new(),
        notes: Vec::new(),
    }
}

pub fn envelope(
    meta: ReportMeta,
    diagnostics: Vec<JsonDiagnostic>,
    summary: Value,
) -> JsonEnvelope {
    let status = if meta.command == "lsp" {
        "unsupported"
    } else if meta.success {
        "success"
    } else {
        "failure"
    };
    JsonEnvelope {
        schema_version: SCHEMA_VERSION,
        tool: TOOL,
        tool_version: env!("CARGO_PKG_VERSION"),
        spec_version: ravum_lang_syntax::LANGUAGE_EDITION,
        command: meta.command.to_string(),
        status: status.to_string(),
        exit_code: meta.exit_code,
        success: meta.success,
        target: JsonTarget {
            kind: meta.target_kind.to_string(),
            path: meta.target_path,
        },
        profile: meta.profile,
        dependencies_digest: meta.dependencies_digest,
        lockfile_stale: meta.lockfile_stale,
        summary,
        diagnostics,
    }
}

pub fn print(envelope: &JsonEnvelope) {
    // DIAG-7.2: one object per command, single line, UTF-8, no noise.
    match serde_json::to_string(envelope) {
        Ok(text) => println!("{text}"),
        Err(err) => {
            // A serialization failure is a programming error; still emit a
            // valid minimal envelope so callers never parse noise.
            eprintln!("internal error: could not serialize JSON: {err}");
            println!(
                "{{\"schema_version\":{SCHEMA_VERSION},\"tool\":\"ravum-lang\",\
                 \"command\":\"error\",\"status\":\"failure\",\"exit_code\":1,\
                 \"success\":false,\"target\":{{\"kind\":\"package\",\"path\":\".\"}},\
                 \"summary\":{{}},\"diagnostics\":[]}}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::phase_for_code;

    /// DIAG-1.1: every diagnostic carries the phase that produced it, and the
    /// registry (`spec/language/diagnostics.md` §3) is the contract. This test
    /// reads the registry and checks the mapping for every registered code, so
    /// adding or re-phasing a code without updating `phase_for_code` fails the
    /// build instead of drifting silently into the JSON output.
    #[test]
    fn every_registered_code_reports_its_registry_phase() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../spec/language/diagnostics.md");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));

        let mut checked = 0usize;
        let mut mismatches: Vec<String> = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            // Registry rows look like: | `E-CODE` | phase | meaning |
            let Some(rest) = line.strip_prefix("| `") else {
                continue;
            };
            let mut parts = rest.split('|');
            let Some(code_cell) = parts.next() else {
                continue;
            };
            let Some(phase_cell) = parts.next() else {
                continue;
            };
            let Some(code) = code_cell.trim().strip_suffix('`') else {
                continue;
            };
            // Skip wildcard rows such as `E-LEX-*` (order table) and rows that
            // are not a single canonical code.
            if code.contains('*') || code.contains(' ') || code.contains(',') {
                continue;
            }
            let phase_cell = phase_cell.trim();
            let Some(expected) = phase_cell.split('/').next() else {
                continue;
            };
            if !matches!(
                expected,
                "lex" | "parse" | "modules" | "resolve" | "types" | "ownership" | "effects"
            ) {
                continue;
            }
            checked += 1;
            let actual = phase_for_code(code);
            if actual != expected {
                mismatches.push(format!("{code}: registry `{expected}`, json `{actual}`"));
            }
        }

        assert!(
            checked >= 100,
            "registry parse found only {checked} codes; the table format changed?"
        );
        assert!(
            mismatches.is_empty(),
            "codes whose JSON phase differs from the registry:\n{}",
            mismatches.join("\n")
        );
    }
}
