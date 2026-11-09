//! Byte-offset <-> LSP `Position` conversion, and diagnostic mapping.
//!
//! LSP positions are `(line, character)` with **0-based** lines and `character`
//! counted in **UTF-16 code units** (the default negotiated encoding, see
//! `spec/language/lexical.md` for the source encoding itself, which is UTF-8).
//! Converting with bytes instead of code units is the classic LSP bug on
//! non-ASCII lines; the tests here cover exactly that case.

use ravum_lang_syntax::diag::{Diagnostic, Severity};
use ravum_lang_syntax::source::SourceMap;
use ravum_lang_syntax::span::Span;
use serde_json::{json, Value};

/// Precomputed line starts for one document text.
#[derive(Debug, Clone)]
pub struct TextIndex {
    line_starts: Vec<u32>,
    len: u32,
}

impl TextIndex {
    pub fn new(text: &str) -> Self {
        let mut line_starts = vec![0u32];
        for (index, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push((index + 1) as u32);
            }
        }
        Self {
            line_starts,
            len: text.len() as u32,
        }
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Byte range of a line, excluding the trailing line break.
    fn line_bounds(&self, text: &str, line: u32) -> Option<(u32, u32)> {
        let index = line as usize;
        if index >= self.line_starts.len() {
            return None;
        }
        let start = self.line_starts[index];
        let raw_end = self.line_starts.get(index + 1).copied().unwrap_or(self.len);
        let mut end = raw_end;
        while end > start {
            match text.as_bytes().get((end - 1) as usize) {
                Some(b'\n') | Some(b'\r') => end -= 1,
                _ => break,
            }
        }
        Some((start, end))
    }

    /// Byte offset for an LSP position. Out-of-range values clamp to the end of
    /// the requested line so a stale position can never panic or slice badly.
    pub fn offset_at(&self, text: &str, line: u32, character: u32) -> u32 {
        let Some((start, end)) = self.line_bounds(text, line) else {
            return self.len;
        };
        let slice = &text[start as usize..end as usize];
        let mut utf16 = 0u32;
        for (byte, ch) in slice.char_indices() {
            if utf16 >= character {
                return start + byte as u32;
            }
            let next = utf16 + ch.len_utf16() as u32;
            if next > character {
                // The requested column falls inside a surrogate pair: snap to
                // the start of the code point rather than producing a split.
                return start + byte as u32;
            }
            utf16 = next;
        }
        end
    }

    /// LSP position for a byte offset.
    pub fn position_at(&self, text: &str, offset: u32) -> (u32, u32) {
        let offset = offset.min(self.len);
        let line = match self.line_starts.binary_search(&offset) {
            Ok(index) => index,
            Err(index) => index.saturating_sub(1),
        };
        let start = self.line_starts[line];
        let len = text.len();
        let clamped = (offset as usize).min(len);
        let mut safe = clamped;
        while safe > start as usize && !text.is_char_boundary(safe) {
            safe -= 1;
        }
        let prefix = &text[start as usize..safe];
        let character = prefix.encode_utf16().count();
        (line as u32, character as u32)
    }
}

pub fn position_json(line: u32, character: u32) -> Value {
    json!({ "line": line, "character": character })
}

pub fn range_json(start: (u32, u32), end: (u32, u32)) -> Value {
    json!({
        "start": position_json(start.0, start.1),
        "end": position_json(end.0, end.1),
    })
}

/// Convert a span to an LSP range using the source text it points into.
pub fn span_range(sources: &SourceMap, span: Span) -> Option<Value> {
    let file = sources.file(span.file)?;
    let index = TextIndex::new(&file.text);
    let start = index.position_at(&file.text, span.start);
    let end = index.position_at(&file.text, span.end.max(span.start));
    Some(range_json(start, end))
}

pub fn severity_number(severity: Severity) -> u32 {
    match severity {
        Severity::Error => 1,
        Severity::Warning => 2,
        Severity::Note => 3,
    }
}

/// Convert one Ravum diagnostic into an LSP `Diagnostic`.
///
/// The full message keeps the notes: an editor hover must show the same
/// information as `ravum-lang check`, not a truncated summary.
pub fn diagnostic_json(diagnostic: &Diagnostic, sources: &SourceMap) -> Option<Value> {
    let range = span_range(sources, diagnostic.span)?;
    let mut message = diagnostic.message.clone();
    for note in &diagnostic.notes {
        message.push_str("\n\nnote: ");
        message.push_str(&note.message);
    }
    let mut value = json!({
        "range": range,
        "severity": severity_number(diagnostic.severity),
        "code": diagnostic.code,
        "source": "ravum",
        "message": message,
    });
    if let Some(file) = sources.file(diagnostic.span.file) {
        value["data"] = json!({ "path": file.path, "code": diagnostic.code });
    }
    Some(value)
}

/// FileId of the source file whose path ends with `path` (case-insensitive on
/// Windows). Used to map an editor URI back to the analyzer's file table.
pub fn file_id_for_path(sources: &SourceMap, path: &str) -> Option<u32> {
    let normalized = normalize_path(path);
    sources
        .files()
        .iter()
        .find(|file| {
            let candidate = normalize_path(&file.path);
            candidate == normalized
                || candidate.ends_with(&normalized)
                || normalized.ends_with(&candidate)
        })
        .map(|file| file.id)
}

pub fn normalize_path(path: &str) -> String {
    path.replace('\\', "/").to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_positions_round_trip() {
        let text = "line one\nline two\n";
        let index = TextIndex::new(text);
        assert_eq!(index.offset_at(text, 0, 0), 0);
        assert_eq!(index.offset_at(text, 1, 0), 9);
        assert_eq!(index.position_at(text, 9), (1, 0));
        assert_eq!(index.position_at(text, 4), (0, 4));
    }

    #[test]
    fn utf16_columns_count_code_units_not_bytes() {
        // "é" is 2 bytes / 1 UTF-16 unit; "𝄞" is 4 bytes / 2 UTF-16 units.
        let text = "x𝄞y\n";
        let index = TextIndex::new(text);
        // After 'x' (1 byte, 1 unit).
        assert_eq!(index.offset_at(text, 0, 1), 1);
        // After the clef: 1 + 2 = 3 UTF-16 units, byte offset 1 + 4 = 5.
        assert_eq!(index.offset_at(text, 0, 3), 5);
        // Inside the surrogate pair snaps to its start.
        assert_eq!(index.offset_at(text, 0, 2), 1);
        assert_eq!(index.position_at(text, 5), (0, 3));
    }

    #[test]
    fn out_of_range_line_clamps_to_end() {
        let text = "abc";
        let index = TextIndex::new(text);
        assert_eq!(index.offset_at(text, 42, 0), 3);
        assert_eq!(index.offset_at(text, 0, 999), 3);
    }

    #[test]
    fn crlf_line_bounds_exclude_the_break() {
        let text = "a\r\nb\r\n";
        let index = TextIndex::new(text);
        assert_eq!(index.offset_at(text, 0, 9), 1);
        assert_eq!(index.offset_at(text, 1, 0), 3);
    }
}
