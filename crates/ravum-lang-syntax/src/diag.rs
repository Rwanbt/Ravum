//! Structured diagnostics with stable codes.
//!
//! Codes are part of the machine-readable contract (`spec/language/diagnostics.md`).
//! A code is never reused for a different meaning and never silently removed.

use crate::source::SourceMap;
use crate::span::{FileId, Span};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Note,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Note => "note",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub message: String,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: String,
    pub severity: Severity,
    pub message: String,
    pub span: Span,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<Note>,
}

impl Diagnostic {
    pub fn error(code: impl Into<String>, span: Span, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            severity: Severity::Error,
            message: message.into(),
            span,
            notes: Vec::new(),
        }
    }

    pub fn warning(code: impl Into<String>, span: Span, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            severity: Severity::Warning,
            message: message.into(),
            span,
            notes: Vec::new(),
        }
    }

    pub fn with_note(mut self, message: impl Into<String>, span: Option<Span>) -> Self {
        self.notes.push(Note {
            message: message.into(),
            span,
        });
        self
    }
}

/// DIAG-1.7 / DIAG-4.3 (MUST): the per-file diagnostic budget is 100. Beyond it
/// exactly one `E-PARSE-TOO-LARGE` is reported for that file and every later
/// diagnostic for the same file is dropped, so a pathological input cannot turn
/// one mistake into thousands of lines of output.
pub const DIAGNOSTIC_BUDGET: usize = 100;

/// Accumulates diagnostics; the driver decides when a phase is fatal.
#[derive(Debug, Default, Clone)]
pub struct DiagnosticBag {
    diags: Vec<Diagnostic>,
    /// Diagnostics already accepted per file, to enforce [`DIAGNOSTIC_BUDGET`].
    per_file: std::collections::BTreeMap<FileId, usize>,
    /// Files whose budget was exhausted; a single `E-PARSE-TOO-LARGE` exists.
    exhausted: std::collections::BTreeSet<FileId>,
}

impl DiagnosticBag {
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies the per-file budget, then stores the diagnostic.
    ///
    /// The budget counts every diagnostic of a file. When the 101st arrives the
    /// bag stores one `E-PARSE-TOO-LARGE` for that file and drops the rest,
    /// including the 101st: the observable stream is 100 diagnostics plus the
    /// marker, never a cascade whose length depends on the input size.
    pub fn push(&mut self, d: Diagnostic) {
        let file = d.span.file;
        if self.exhausted.contains(&file) {
            return;
        }
        let seen = self.per_file.entry(file).or_insert(0);
        if *seen >= DIAGNOSTIC_BUDGET {
            self.exhausted.insert(file);
            self.diags.push(Diagnostic::error(
                "E-PARSE-TOO-LARGE",
                d.span,
                format!(
                    "more than {DIAGNOSTIC_BUDGET} diagnostics in this file; further analysis of the file stopped"
                ),
            ));
            return;
        }
        *seen += 1;
        self.diags.push(d);
    }

    pub fn error(&mut self, code: impl Into<String>, span: Span, message: impl Into<String>) {
        self.push(Diagnostic::error(code, span, message));
    }

    pub fn warning(&mut self, code: impl Into<String>, span: Span, message: impl Into<String>) {
        self.push(Diagnostic::warning(code, span, message));
    }

    pub fn extend(&mut self, other: DiagnosticBag) {
        // Route through `push` so the per-file budget also applies to merged
        // bags; a phase that extends another must not double the budget.
        for diagnostic in other.diags {
            self.push(diagnostic);
        }
    }

    pub fn has_errors(&self) -> bool {
        self.diags.iter().any(|d| d.severity == Severity::Error)
    }

    pub fn error_count(&self) -> usize {
        self.diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.diags
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.diags.is_empty()
    }

    pub fn len(&self) -> usize {
        self.diags.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diags.iter()
    }

    pub fn into_vec(self) -> Vec<Diagnostic> {
        self.diags
    }

    pub fn as_slice(&self) -> &[Diagnostic] {
        &self.diags
    }

    /// Deterministic order: file, start offset, code, message.
    pub fn sort(&mut self) {
        self.diags.sort_by(|a, b| {
            (a.span.file, a.span.start, a.span.end, &a.code, &a.message).cmp(&(
                b.span.file,
                b.span.start,
                b.span.end,
                &b.code,
                &b.message,
            ))
        });
        self.diags.dedup();
    }

    /// Human-readable rendering with source snippets and carets.
    pub fn render(&self, sources: &SourceMap) -> String {
        let mut out = String::new();
        for d in &self.diags {
            let loc = sources.location(d.span);
            let _ = writeln!(out, "{}[{}]: {}", d.severity.as_str(), d.code, d.message);
            let _ = writeln!(out, "  --> {loc}");
            if let Some(file) = sources.file(d.span.file) {
                let (line, col) = file.line_col(d.span.start);
                let text = file.line_text(line);
                let _ = writeln!(out, "   |");
                let _ = writeln!(out, "{line:>3} | {text}");
                let width = d.span.len().max(1) as usize;
                let pad = " ".repeat(col.saturating_sub(1) as usize);
                let caret =
                    "^".repeat(width.min(text.len().saturating_sub(col as usize - 1).max(1)));
                let _ = writeln!(out, "   | {pad}{caret}");
            }
            for note in &d.notes {
                match note.span {
                    Some(span) => {
                        let _ = writeln!(
                            out,
                            "   = note: {} ({})",
                            note.message,
                            sources.location(span)
                        );
                    }
                    None => {
                        let _ = writeln!(out, "   = note: {}", note.message);
                    }
                }
            }
            let _ = writeln!(out);
        }
        out
    }
}
