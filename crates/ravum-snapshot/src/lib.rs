//! Immutable, content-addressed source snapshots (M1 / RAV-014).
//!
//! A `SourceSnapshot` is never mutated: overlays produce a *new* snapshot with a
//! *new* `SnapshotId`. That is what makes "reject results computed from an older
//! snapshot" mechanically checkable instead of a convention.

use ravum_protocol::{sha256_hex, SnapshotId};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    path: String,
    content: String,
    hash: String,
}

impl FileEntry {
    fn new(path: String, content: String) -> Self {
        let hash = sha256_hex(content.as_bytes());
        Self { path, content, hash }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn hash(&self) -> &str {
        &self.hash
    }
}

/// Canonical path form: forward slashes, no leading `./`, no absolute prefix.
pub fn canonical_path(path: &str) -> String {
    let mut p = path.replace('\\', "/");
    if let Some(stripped) = p.strip_prefix("./") {
        p = stripped.to_string();
    }
    p.trim_start_matches('/').to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSnapshot {
    id: SnapshotId,
    files: BTreeMap<String, FileEntry>,
}

impl SourceSnapshot {
    /// Build a snapshot from `(path, content)` pairs. Order does not matter.
    pub fn new(files: impl IntoIterator<Item = (String, String)>) -> Self {
        let mut map = BTreeMap::new();
        for (path, content) in files {
            let path = canonical_path(&path);
            map.insert(path.clone(), FileEntry::new(path, content));
        }
        let id = Self::digest(&map);
        Self { id, files: map }
    }

    /// Snapshot of exactly one file, read from disk (buffers on disk for M1).
    pub fn from_file(path: &Path) -> io::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "source.rs".to_string());
        Ok(Self::new([(name, content)]))
    }

    /// Snapshot of every file under `root` for which `include` returns true.
    /// Paths are stored relative to `root` with forward slashes.
    pub fn from_directory(
        root: &Path,
        include: impl Fn(&Path) -> bool,
    ) -> io::Result<Self> {
        let mut files = Vec::new();
        collect(root, root, &include, &mut files)?;
        Ok(Self::new(files))
    }

    pub fn id(&self) -> &SnapshotId {
        &self.id
    }

    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(|k| k.as_str())
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    pub fn file(&self, path: &str) -> Option<&FileEntry> {
        self.files.get(&canonical_path(path))
    }

    pub fn content_of(&self, path: &str) -> Option<&str> {
        self.file(path).map(|f| f.content())
    }

    pub fn file_hash(&self, path: &str) -> Option<&str> {
        self.file(path).map(|f| f.hash())
    }

    /// Produce a *new* snapshot where the given paths are replaced/added.
    /// The receiver is untouched: overlays never mutate shared state.
    pub fn with_overlay(
        &self,
        overlays: impl IntoIterator<Item = (String, String)>,
    ) -> Self {
        let mut files: BTreeMap<String, FileEntry> = self.files.clone();
        for (path, content) in overlays {
            let path = canonical_path(&path);
            files.insert(path.clone(), FileEntry::new(path, content));
        }
        let id = Self::digest(&files);
        Self { id, files }
    }

    /// Write the snapshot to `dir` (used only by the single Git writer).
    /// Refuses paths that would escape `dir`.
    pub fn materialize_to(&self, dir: &Path) -> io::Result<()> {
        for (path, entry) in &self.files {
            let target = safe_join(dir, path)?;
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&target, entry.content())?;
        }
        Ok(())
    }

    /// True when `expected` is still the current snapshot identity.
    pub fn is_current(&self, expected: &SnapshotId) -> bool {
        &self.id == expected
    }

    fn digest(files: &BTreeMap<String, FileEntry>) -> SnapshotId {
        let mut buf = String::from("ravum-snapshot-v1\n");
        for (path, entry) in files {
            buf.push_str(path);
            buf.push('\0');
            buf.push_str(&entry.content.len().to_string());
            buf.push('\0');
            buf.push_str(&entry.content);
            buf.push('\0');
        }
        SnapshotId::new(sha256_hex(buf.as_bytes()))
    }
}

fn safe_join(dir: &Path, rel: &str) -> io::Result<PathBuf> {
    if rel.split('/').any(|c| c == "..") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path escapes workspace: {rel}"),
        ));
    }
    Ok(dir.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR)))
}

fn collect(
    root: &Path,
    dir: &Path,
    include: &impl Fn(&Path) -> bool,
    out: &mut Vec<(String, String)>,
) -> io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.path());
    for entry in entries {
        let path = entry.path();
        let ty = entry.file_type()?;
        if ty.is_dir() {
            if is_ignored_dir(&path) {
                continue;
            }
            collect(root, &path, include, out)?;
        } else if ty.is_file() && include(&path) {
            let rel = path
                .strip_prefix(root)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path outside root"))?;
            let rel = rel.to_string_lossy().replace('\\', "/");
            out.push((rel, std::fs::read_to_string(&path)?));
        }
    }
    Ok(())
}

fn is_ignored_dir(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|n| n.to_str()).unwrap_or(""),
        ".git" | "target" | "node_modules" | ".cache"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(files: &[(&str, &str)]) -> SourceSnapshot {
        SourceSnapshot::new(
            files
                .iter()
                .map(|(p, c)| (p.to_string(), c.to_string())),
        )
    }

    #[test]
    fn identity_is_order_independent() {
        let a = snap(&[("b.rs", "2"), ("a.rs", "1")]);
        let b = snap(&[("a.rs", "1"), ("b.rs", "2")]);
        assert_eq!(a.id(), b.id());
    }

    #[test]
    fn identity_changes_with_content() {
        let a = snap(&[("a.rs", "1")]);
        let b = snap(&[("a.rs", "1 ")]);
        assert_ne!(a.id(), b.id());
    }

    #[test]
    fn path_separators_are_canonicalised() {
        let a = snap(&[("sub\\a.rs", "1")]);
        let b = snap(&[("sub/a.rs", "1")]);
        assert_eq!(a.id(), b.id());
        assert_eq!(a.content_of("sub\\a.rs"), Some("1"));
    }

    #[test]
    fn overlay_does_not_mutate_receiver() {
        let base = snap(&[("a.rs", "1"), ("b.rs", "2")]);
        let base_id = base.id().clone();
        let next = base.with_overlay([("a.rs".to_string(), "9".to_string())]);
        assert_eq!(base.id(), &base_id);
        assert_eq!(base.content_of("a.rs"), Some("1"));
        assert_eq!(next.content_of("a.rs"), Some("9"));
        assert_ne!(next.id(), &base_id);
        assert_eq!(next.file_count(), 2);
    }

    #[test]
    fn staleness_is_a_plain_comparison() {
        let base = snap(&[("a.rs", "1")]);
        let next = base.with_overlay([("a.rs".to_string(), "2".to_string())]);
        assert!(base.is_current(base.id()));
        assert!(!base.is_current(next.id()));
    }
}
