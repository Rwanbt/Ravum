//! Minimal unified-diff applier (COLLAB-1 granularity: hunks, not AST).
//!
//! Limits, stated on purpose: no file creation, no renames, no binary content,
//! no fuzzy matching. A hunk whose context does not match exactly is refused —
//! a "clever" fuzzy apply is exactly how a semantic conflict gets silently
//! turned into corrupted code.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffLine {
    Context(String),
    Remove(String),
    Add(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: usize,
    pub old_len: usize,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePatch {
    pub path: String,
    pub hunks: Vec<Hunk>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchError {
    Malformed(String),
    UnknownFile(String),
    ContextMismatch { path: String, line: usize },
    OverlappingHunks { path: String, line: usize },
}

impl fmt::Display for PatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PatchError::Malformed(d) => write!(f, "malformed patch: {d}"),
            PatchError::UnknownFile(p) => write!(f, "patch targets unknown file: {p}"),
            PatchError::ContextMismatch { path, line } => {
                write!(f, "context mismatch in {path} at line {line}")
            }
            PatchError::OverlappingHunks { path, line } => {
                write!(f, "overlapping hunks in {path} at line {line}")
            }
        }
    }
}

impl std::error::Error for PatchError {}

/// Parse a unified diff into per-file patches.
pub fn parse(patch: &str) -> Result<Vec<FilePatch>, PatchError> {
    let mut files: Vec<FilePatch> = Vec::new();
    let mut current: Option<FilePatch> = None;
    let mut pending_hunk: Option<Hunk> = None;
    let mut old_remaining = 0usize;
    let mut new_remaining = 0usize;

    for raw in patch.lines() {
        if raw.starts_with("diff --git") {
            flush_hunk(
                &mut current,
                &mut pending_hunk,
                &mut old_remaining,
                &mut new_remaining,
            )?;
            if let Some(p) = current.take() {
                files.push(p);
            }
            continue;
        }
        if let Some(rest) = raw.strip_prefix("--- ") {
            flush_hunk(
                &mut current,
                &mut pending_hunk,
                &mut old_remaining,
                &mut new_remaining,
            )?;
            if let Some(p) = current.take() {
                files.push(p);
            }
            let _ = rest;
            continue;
        }
        if let Some(rest) = raw.strip_prefix("+++ ") {
            let path = clean_path(rest);
            current = Some(FilePatch {
                path,
                hunks: Vec::new(),
            });
            continue;
        }
        if raw.starts_with("@@") {
            flush_hunk(
                &mut current,
                &mut pending_hunk,
                &mut old_remaining,
                &mut new_remaining,
            )?;
            let (old_start, old_len) = parse_range(raw)?;
            old_remaining = old_len;
            new_remaining = parse_new_len(raw)?;
            pending_hunk = Some(Hunk {
                old_start,
                old_len,
                lines: Vec::new(),
            });
            continue;
        }
        if old_remaining == 0 && new_remaining == 0 {
            // Header noise between files (`index`, `similarity`, ...).
            continue;
        }
        let Some(hunk) = pending_hunk.as_mut() else {
            continue;
        };
        match raw.chars().next() {
            Some(' ') | None => {
                let text = raw.get(1..).unwrap_or("").to_string();
                hunk.lines.push(DiffLine::Context(text));
                old_remaining = old_remaining.saturating_sub(1);
                new_remaining = new_remaining.saturating_sub(1);
            }
            Some('-') => {
                hunk.lines.push(DiffLine::Remove(raw[1..].to_string()));
                old_remaining = old_remaining.saturating_sub(1);
            }
            Some('+') => {
                hunk.lines.push(DiffLine::Add(raw[1..].to_string()));
                new_remaining = new_remaining.saturating_sub(1);
            }
            Some('\\') => {}
            Some(other) => {
                return Err(PatchError::Malformed(format!(
                    "unexpected diff line starting with {other:?}"
                )));
            }
        }
        if old_remaining == 0 && new_remaining == 0 {
            flush_hunk(
                &mut current,
                &mut pending_hunk,
                &mut old_remaining,
                &mut new_remaining,
            )?;
        }
    }
    flush_hunk(
        &mut current,
        &mut pending_hunk,
        &mut old_remaining,
        &mut new_remaining,
    )?;
    if let Some(p) = current.take() {
        files.push(p);
    }
    if files.is_empty() {
        return Err(PatchError::Malformed("no file header found".to_owned()));
    }
    Ok(files)
}

fn flush_hunk(
    current: &mut Option<FilePatch>,
    pending: &mut Option<Hunk>,
    old_remaining: &mut usize,
    new_remaining: &mut usize,
) -> Result<(), PatchError> {
    if *old_remaining != 0 || *new_remaining != 0 {
        return Err(PatchError::Malformed(format!(
            "truncated hunk: {old_remaining} old and {new_remaining} new lines missing"
        )));
    }
    if let Some(hunk) = pending.take() {
        match current.as_mut() {
            Some(file) => file.hunks.push(hunk),
            None => return Err(PatchError::Malformed("hunk before file header".to_owned())),
        }
    }
    Ok(())
}

fn parse_range(line: &str) -> Result<(usize, usize), PatchError> {
    let spec = line
        .strip_prefix("@@ -")
        .ok_or_else(|| PatchError::Malformed(format!("bad hunk header: {line}")))?;
    let end = spec
        .find(' ')
        .ok_or_else(|| PatchError::Malformed(format!("bad hunk header: {line}")))?;
    let (start, len) = split_range(&spec[..end])?;
    Ok((start, len))
}

fn parse_new_len(line: &str) -> Result<usize, PatchError> {
    let plus = line
        .find(" +")
        .ok_or_else(|| PatchError::Malformed(format!("bad hunk header: {line}")))?;
    let spec = &line[plus + 2..];
    let end = spec
        .find(' ')
        .ok_or_else(|| PatchError::Malformed(format!("bad hunk header: {line}")))?;
    Ok(split_range(&spec[..end])?.1)
}

fn split_range(spec: &str) -> Result<(usize, usize), PatchError> {
    let mut parts = spec.split(',');
    let start = parts
        .next()
        .and_then(|v| v.parse::<usize>().ok())
        .ok_or_else(|| PatchError::Malformed(format!("bad range: {spec}")))?;
    let len = match parts.next() {
        Some(v) => v
            .parse::<usize>()
            .map_err(|_| PatchError::Malformed(format!("bad range: {spec}")))?,
        None => 1,
    };
    Ok((start, len))
}

fn clean_path(raw: &str) -> String {
    let p = raw.split('\t').next().unwrap_or(raw).trim();
    let p = p.strip_prefix("b/").unwrap_or(p);
    p.replace('\\', "/")
}

/// Apply one file patch to `original`, exactly.
///
/// Line endings are normalized for comparison (a Windows checkout that turned LF
/// into CRLF must not make every hunk fail) and the original convention is
/// restored in the result.
pub fn apply(file_patch: &FilePatch, original: &str) -> Result<String, PatchError> {
    let crlf = original.contains("\r\n");
    let normalized = original.replace("\r\n", "\n");
    let trailing_newline = normalized.ends_with('\n');
    let src: Vec<&str> = if normalized.is_empty() {
        Vec::new()
    } else {
        normalized.trim_end_matches('\n').split('\n').collect()
    };
    let mut out: Vec<String> = Vec::with_capacity(src.len() + 8);
    let mut cursor = 0usize;

    for hunk in &file_patch.hunks {
        let start = hunk.old_start.saturating_sub(1);
        if start < cursor {
            return Err(PatchError::OverlappingHunks {
                path: file_patch.path.clone(),
                line: hunk.old_start,
            });
        }
        out.extend(
            src[cursor..start.min(src.len())]
                .iter()
                .map(|s| s.to_string()),
        );
        let mut idx = start;
        for line in &hunk.lines {
            match line {
                DiffLine::Context(text) | DiffLine::Remove(text) => {
                    if src.get(idx) != Some(&text.as_str()) {
                        return Err(PatchError::ContextMismatch {
                            path: file_patch.path.clone(),
                            line: idx + 1,
                        });
                    }
                    if matches!(line, DiffLine::Context(_)) {
                        out.push(text.clone());
                    }
                    idx += 1;
                }
                DiffLine::Add(text) => out.push(text.clone()),
            }
        }
        cursor = idx;
    }
    if cursor < src.len() {
        out.extend(src[cursor..].iter().map(|s| s.to_string()));
    }
    let mut result = out.join("\n");
    if trailing_newline {
        result.push('\n');
    }
    if crlf {
        result = result.replace('\n', "\r\n");
    }
    Ok(result)
}

/// Line coordinates (old numbering, 1-based inclusive) actually *modified* by a
/// patch, per file.
///
/// Only removed lines and insertion points count, never the shared context. Two
/// agents editing different functions of the same file usually share context
/// lines; treating that shared context as a conflict would block legitimate
/// independent work. This was measured, not guessed: see
/// `docs/mvp/M1-evidence.md` §"Constat COLLAB".
pub fn touched_ranges(files: &[FilePatch]) -> Vec<(String, usize, usize)> {
    let mut out = Vec::new();
    for file in files {
        for hunk in &file.hunks {
            let mut old_line = hunk.old_start;
            let mut removed = Vec::new();
            let mut inserted = Vec::new();
            for line in &hunk.lines {
                match line {
                    DiffLine::Context(_) => old_line += 1,
                    DiffLine::Remove(_) => {
                        removed.push(old_line);
                        old_line += 1;
                    }
                    DiffLine::Add(_) => inserted.push(old_line),
                }
            }
            // A replacement occupies the lines it removed; a pure insertion
            // competes for the position it inserts at.
            let touched = if removed.is_empty() {
                inserted
            } else {
                removed
            };
            for line in touched {
                out.push((file.path.clone(), line, line));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "fn a() -> u32 {\n    1\n}\n\nfn b() -> u32 {\n    2\n}\n";

    fn patch_of(text: &str) -> Vec<FilePatch> {
        parse(text).expect("parse")
    }

    #[test]
    fn applies_a_single_hunk() {
        let p = patch_of(
            "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,3 +1,3 @@\n fn a() -> u32 {\n-    1\n+    10\n }\n",
        );
        assert_eq!(p[0].path, "src/lib.rs");
        let applied = apply(&p[0], BASE).expect("apply");
        assert!(applied.contains("10"));
        assert!(applied.ends_with('\n'));
    }

    #[test]
    fn refuses_context_mismatch() {
        let p = patch_of("+++ b/x.rs\n@@ -1,2 +1,2 @@\n not this\n-   1\n+   2\n");
        assert!(matches!(
            apply(&p[0], BASE),
            Err(PatchError::ContextMismatch { .. })
        ));
    }

    #[test]
    fn two_independent_hunks_apply_together() {
        let patch = "+++ b/x.rs\n@@ -1,3 +1,3 @@\n fn a() -> u32 {\n-    1\n+    10\n }\n@@ -5,3 +5,3 @@\n fn b() -> u32 {\n-    2\n+    20\n }\n";
        let p = patch_of(patch);
        let applied = apply(&p[0], BASE).expect("apply");
        assert!(applied.contains("10") && applied.contains("20"));
    }

    #[test]
    fn touched_ranges_are_the_modified_lines_only() {
        // @@ -5,3: context line 5, removed line 6, context line 7.
        let p = patch_of("+++ b/x.rs\n@@ -5,3 +5,3 @@\n fn b() -> u32 {\n-    2\n+    20\n }\n");
        assert_eq!(touched_ranges(&p), vec![("x.rs".to_string(), 6, 6)]);
    }

    #[test]
    fn crlf_sources_apply_and_keep_their_line_endings() {
        let crlf = BASE.replace('\n', "\r\n");
        let p = patch_of("+++ b/x.rs\n@@ -1,3 +1,3 @@\n fn a() -> u32 {\n-    1\n+    10\n }\n");
        let applied = apply(&p[0], &crlf).expect("apply on CRLF source");
        assert!(applied.contains("\r\n"));
        assert_eq!(
            applied.matches("\r\n").count(),
            crlf.matches("\r\n").count()
        );
        assert!(applied.contains("    10"));
    }

    #[test]
    fn truncated_hunk_is_malformed() {
        assert!(matches!(
            parse("+++ b/x.rs\n@@ -1,4 +1,4 @@\n one\n"),
            Err(PatchError::Malformed(_))
        ));
    }
}
