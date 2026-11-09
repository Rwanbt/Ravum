//! Small filesystem helpers shared by the snapshot store and the result cache.

use crate::error::{DaemonError, Result};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static WRITE_SEQ: AtomicU64 = AtomicU64::new(0);

/// Joins a validated logical path onto a base directory, refusing anything that
/// could escape that base. The check is redundant with request validation by
/// design: two independent guards are cheap, a path escape is expensive.
pub fn safe_join(base: &Path, logical_path: &str) -> Result<PathBuf> {
    let candidate = Path::new(logical_path);
    if candidate.is_absolute() {
        return Err(DaemonError::invalid(format!(
            "logical path {logical_path:?} must be relative"
        )));
    }
    let mut out = base.to_path_buf();
    for component in candidate.components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(DaemonError::invalid(format!(
                    "logical path {logical_path:?} escapes the unit directory"
                )));
            }
        }
    }
    Ok(out)
}

/// Writes `bytes` to `path` through a same-directory temporary file followed by
/// a rename, so a crash never leaves a reader with a half-written JSON file.
/// On Windows a rename over an existing file is refused by the OS: the previous
/// file is removed first; the fallback direct write remains safe because the
/// result cache treats any unreadable entry as a miss (fail-closed).
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        DaemonError::io(format!(
            "cannot write {}: no parent directory",
            path.display()
        ))
    })?;
    std::fs::create_dir_all(parent)?;
    let seq = WRITE_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = parent.join(format!(
        ".{}.tmp-{}-{seq}",
        path.file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "write".to_owned()),
        std::process::id()
    ));
    std::fs::write(&tmp, bytes)?;
    if path.exists() {
        // Content-addressed targets are only rewritten to heal a corrupt entry.
        std::fs::remove_file(path)?;
    }
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(_) => {
            let result = std::fs::write(path, bytes);
            let _ = std::fs::remove_file(&tmp);
            result.map_err(DaemonError::from)
        }
    }
}

/// Removes a directory tree only when its name matches the expected marker.
/// Used exclusively on daemon-private benchmark/campaign directories; never on
/// user paths.
pub fn remove_dir_if_marked(path: &Path, marker: &str) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    if !name.contains(marker) {
        return Err(DaemonError::invalid(format!(
            "refusing to remove {}: name does not contain marker {marker:?}",
            path.display()
        )));
    }
    std::fs::remove_dir_all(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_join_refuses_traversal() {
        let base = Path::new("base");
        assert!(safe_join(base, "src/lib.rs").is_ok());
        assert!(safe_join(base, "../escape.rs").is_err());
        assert!(safe_join(base, "src/../../escape.rs").is_err());
        assert!(safe_join(base, "/abs.rs").is_err());
        assert!(safe_join(base, "C:/abs.rs").is_err());
    }

    #[test]
    fn atomic_write_replaces_existing_file() {
        let dir = std::env::temp_dir().join(format!("ravum-daemon-fsutil-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("entry.json");
        atomic_write(&path, b"first").unwrap();
        atomic_write(&path, b"second").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_dir_if_marked_refuses_foreign_directory() {
        let dir = std::env::temp_dir().join("ravum-daemon-fsutil-foreign");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(remove_dir_if_marked(&dir, "daemon-bench").is_err());
        assert!(dir.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
