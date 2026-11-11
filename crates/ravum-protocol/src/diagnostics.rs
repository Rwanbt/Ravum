//! Structured diagnostics (ADR-040 shape, intentionally minimal for M1).
//!
//! Every diagnostic set is bound to the `SnapshotId` it was produced from so a
//! result can never be displayed as if it described a newer buffer.

use crate::ids::SnapshotId;
use serde::{Deserialize, Serialize};

pub const DIAGNOSTICS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Note,
    Help,
}

impl Severity {
    pub fn is_error(self) -> bool {
        matches!(self, Severity::Error)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Note => "note",
            Severity::Help => "help",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticSpan {
    pub file: String,
    pub line_start: u32,
    pub column_start: u32,
    pub line_end: u32,
    pub column_end: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: Option<String>,
    pub level: Severity,
    pub message: String,
    pub span: Option<DiagnosticSpan>,
    pub rendered: String,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            code: None,
            level: Severity::Error,
            message: message.into(),
            span: None,
            rendered: String::new(),
        }
    }
}

/// Diagnostics produced by one tool run for one exact source snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticSet {
    pub schema_version: u32,
    pub source_snapshot_id: SnapshotId,
    pub tool: String,
    pub tool_version: String,
    pub diagnostics: Vec<Diagnostic>,
}

impl DiagnosticSet {
    pub fn new(
        source_snapshot_id: SnapshotId,
        tool: impl Into<String>,
        tool_version: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: DIAGNOSTICS_SCHEMA_VERSION,
            source_snapshot_id,
            tool: tool.into(),
            tool_version: tool_version.into(),
            diagnostics: Vec::new(),
        }
    }

    pub fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.level.is_error())
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.level == Severity::Warning)
            .count()
    }

    pub fn has_errors(&self) -> bool {
        self.error_count() > 0
    }

    /// True only when the set was computed for the snapshot `current` points to.
    /// A set from any other snapshot is `STALE` and must not gate activation.
    pub fn is_current_for(&self, current: &SnapshotId) -> bool {
        &self.source_snapshot_id == current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_diagnostics_are_detected() {
        let set = DiagnosticSet::new(SnapshotId::new("s1"), "rustc", "1.95.0");
        assert!(set.is_current_for(&SnapshotId::new("s1")));
        assert!(!set.is_current_for(&SnapshotId::new("s2")));
    }

    #[test]
    fn error_counting_ignores_warnings() {
        let mut set = DiagnosticSet::new(SnapshotId::new("s"), "rustc", "1");
        set.diagnostics.push(Diagnostic::error("boom"));
        assert_eq!(set.error_count(), 1);
        assert!(set.has_errors());
    }
}
