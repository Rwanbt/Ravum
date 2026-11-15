//! Source map shared by the parser, the checker and the CLI.

use crate::span::{FileId, SourceFile, Span};

/// Files registered by one load, or by one language-server session.
///
/// Ids are values, not positions. [`SourceMap::add`] keeps the historical dense
/// allocation (0, 1, 2, … in registration order) and is what the CLI uses;
/// [`SourceMap::add_at`] registers a file under an explicit `FileId` chosen by a
/// session table, leaving a gap when that id is not the next one. The two forms
/// coexist so the language server can keep a file's id — and therefore every
/// `NodeId` that embeds it — stable across reloads in which another file is
/// added or removed (T5). [`SourceMap::files`] stays a slice sorted by id;
/// lookups are `O(log n)`.
#[derive(Debug, Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
    next: FileId,
}

impl SourceMap {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            next: 0,
        }
    }

    /// Register a file under the next dense id (0, 1, 2, …).
    pub fn add(&mut self, path: impl Into<String>, text: impl Into<String>) -> FileId {
        let id = self.next;
        self.next = self.next.saturating_add(1);
        self.files.push(SourceFile::new(id, path, text));
        id
    }

    /// Register a file under an explicit id.
    ///
    /// Returns `None` when the id is already taken: an id identifies exactly one
    /// file, so the caller must treat a collision as an inconsistent session
    /// table rather than overwrite a file another span already points into.
    /// `FileId`s handed out here are bounded by the loader's
    /// `MAX_SOURCE_FILES` (65 536), far below `u32::MAX`, so the internal
    /// "next dense id" counter cannot saturate on this path.
    pub fn add_at(
        &mut self,
        id: FileId,
        path: impl Into<String>,
        text: impl Into<String>,
    ) -> Option<FileId> {
        match self.files.binary_search_by_key(&id, |file| file.id) {
            Ok(_) => None,
            Err(position) => {
                self.files.insert(position, SourceFile::new(id, path, text));
                if id >= self.next {
                    self.next = id.saturating_add(1);
                }
                Some(id)
            }
        }
    }

    pub fn file(&self, id: FileId) -> Option<&SourceFile> {
        self.files
            .binary_search_by_key(&id, |file| file.id)
            .ok()
            .map(|position| &self.files[position])
    }

    /// Registered files, sorted by `FileId` (holes are absent, not empty).
    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

    /// Number of registered files (`add_at` gaps are not counted).
    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn path(&self, id: FileId) -> &str {
        self.file(id)
            .map(|f| f.path.as_str())
            .unwrap_or("<unknown>")
    }

    /// Human-readable `file:line:col` for a span (1-based).
    pub fn location(&self, span: Span) -> String {
        match self.file(span.file) {
            Some(f) => {
                let (line, col) = f.line_col(span.start);
                format!("{}:{}:{}", f.path, line, col)
            }
            None => "<unknown>".to_string(),
        }
    }

    pub fn slice(&self, span: Span) -> &str {
        self.file(span.file).map(|f| f.slice(span)).unwrap_or("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dense_allocation_keeps_ids_equal_to_positions() {
        let mut sources = SourceMap::new();
        assert_eq!(sources.add("src/main.rv", "fn main() {}\n"), 0);
        assert_eq!(sources.add("src/util.rv", "pub fn f() {}\n"), 1);
        assert_eq!(sources.len(), 2);
        assert_eq!(sources.files().len(), 2);
        assert_eq!(sources.file(1).expect("second file").path, "src/util.rv");
        assert!(sources.file(2).is_none());
    }

    #[test]
    fn explicit_ids_leave_gaps_but_keep_the_slice_sorted() {
        let mut sources = SourceMap::new();
        assert_eq!(sources.add_at(4, "src/d.rv", "d"), Some(4));
        assert_eq!(sources.add_at(2, "src/b.rv", "b"), Some(2));
        // `add_at` bumped the dense counter past the highest explicit id.
        assert_eq!(sources.add("src/e.rv", "e"), 5);
        assert_eq!(sources.len(), 3, "holes are not files");
        let ids: Vec<FileId> = sources.files().iter().map(|file| file.id).collect();
        assert_eq!(ids, vec![2, 4, 5]);
        assert_eq!(sources.file(2).expect("b").path, "src/b.rv");
        assert!(sources.file(3).is_none(), "a hole is not a file");
        assert_eq!(sources.file(5).expect("e").path, "src/e.rv");
        assert_eq!(
            sources.location(Span::new(2, 0, 1)),
            "src/b.rv:1:1",
            "a span still resolves through the id, not the position"
        );
    }

    #[test]
    fn an_id_identifies_exactly_one_file() {
        let mut sources = SourceMap::new();
        assert_eq!(sources.add_at(1, "src/a.rv", "a"), Some(1));
        assert_eq!(sources.add_at(1, "src/b.rv", "b"), None);
        assert_eq!(sources.file(1).expect("first file").path, "src/a.rv");
        assert_eq!(sources.len(), 1);
    }
}
