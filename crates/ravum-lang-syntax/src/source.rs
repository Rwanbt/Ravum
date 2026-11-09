//! Source map shared by the parser, the checker and the CLI.

use crate::span::{FileId, SourceFile, Span};

#[derive(Debug, Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    pub fn new() -> Self {
        Self { files: Vec::new() }
    }

    pub fn add(&mut self, path: impl Into<String>, text: impl Into<String>) -> FileId {
        let id = self.files.len() as FileId;
        self.files.push(SourceFile::new(id, path, text));
        id
    }

    pub fn file(&self, id: FileId) -> Option<&SourceFile> {
        self.files.get(id as usize)
    }

    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

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
