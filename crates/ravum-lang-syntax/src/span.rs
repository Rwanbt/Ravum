//! Byte spans and source files.
//!
//! Spans are byte offsets into a single [`SourceFile`] and are stable for the
//! lifetime of that file. UTF-8 and UTF-16 columns are derived on demand so the
//! LSP (T7) can reuse the same identifiers without a second coordinate system.

use serde::{Deserialize, Serialize};

/// Identifier of a file inside a [`crate::source::SourceMap`].
pub type FileId = u32;

/// Half-open byte range `[start, end)` inside one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Span {
    pub file: FileId,
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub const fn new(file: FileId, start: u32, end: u32) -> Self {
        Self { file, start, end }
    }

    /// A span that cannot be attributed to real source (generated or recovered).
    pub const fn dummy() -> Self {
        Self {
            file: u32::MAX,
            start: 0,
            end: 0,
        }
    }

    pub fn is_dummy(&self) -> bool {
        self.file == u32::MAX
    }

    pub fn join(self, other: Span) -> Span {
        if self.is_dummy() {
            return other;
        }
        if other.is_dummy() || other.file != self.file {
            return self;
        }
        Span {
            file: self.file,
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    pub fn len(&self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A parsed source file plus precomputed line offsets.
#[derive(Debug, Clone)]
pub struct SourceFile {
    pub id: FileId,
    pub path: String,
    pub text: String,
    /// Byte offset of the start of every line (line 1 is `starts[0] == 0`).
    line_starts: Vec<u32>,
}

impl SourceFile {
    pub fn new(id: FileId, path: impl Into<String>, text: impl Into<String>) -> Self {
        let text = text.into();
        let mut line_starts = vec![0u32];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push((i + 1) as u32);
            }
        }
        Self {
            id,
            path: path.into(),
            text,
            line_starts,
        }
    }

    /// 1-based line and column (UTF-8 bytes) for a byte offset.
    pub fn line_col(&self, offset: u32) -> (u32, u32) {
        let offset = offset.min(self.text.len() as u32);
        let line_idx = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let line_start = self.line_starts[line_idx];
        (line_idx as u32 + 1, offset - line_start + 1)
    }

    /// 0-based (line, UTF-16 column) for the LSP. Counts code units, not bytes.
    pub fn lsp_position(&self, offset: u32) -> (u32, u32) {
        let offset = offset.min(self.text.len() as u32);
        let (line, _) = self.line_col(offset);
        let line_start = self.line_starts[line as usize - 1] as usize;
        let prefix = &self.text[line_start..offset as usize];
        let col: usize = prefix.encode_utf16().count();
        (line - 1, col as u32)
    }

    /// Text of a span, bounded to valid UTF-8 boundaries when possible.
    pub fn slice(&self, span: Span) -> &str {
        let start = (span.start as usize).min(self.text.len());
        let end = (span.end as usize).min(self.text.len());
        if start > end {
            return "";
        }
        // Snap to char boundaries so a corrupted span never panics.
        let mut s = start;
        while s > 0 && !self.text.is_char_boundary(s) {
            s -= 1;
        }
        let mut e = end;
        while e < self.text.len() && !self.text.is_char_boundary(e) {
            e += 1;
        }
        &self.text[s..e]
    }

    pub fn line_text(&self, line: u32) -> &str {
        let idx = (line as usize).saturating_sub(1);
        if idx >= self.line_starts.len() {
            return "";
        }
        let start = self.line_starts[idx] as usize;
        let end = self
            .line_starts
            .get(idx + 1)
            .map(|v| *v as usize)
            .unwrap_or(self.text.len());
        self.text[start..end].trim_end_matches(['\n', '\r'])
    }
}
