//! Artifact retention.
//!
//! Every build produces a new file (a loaded DLL must never be rewritten), so the
//! artifacts directory grows without bound. Retention keeps the newest N plus the
//! currently active artifact, and removes build-cache entries whose artifact is
//! gone. Deleting the active file is refused: the running process still maps it.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionReport {
    pub kept: Vec<PathBuf>,
    pub removed: Vec<PathBuf>,
    pub cache_entries_removed: usize,
}

/// Extract the build stamp from `patch-<snapshot>-<context>-<stamp>.<ext>`.
fn stamp_of(path: &Path) -> Option<u64> {
    let stem = path.file_stem()?.to_string_lossy();
    stem.rsplit('-').next()?.parse::<u64>().ok()
}

fn is_artifact(path: &Path) -> bool {
    let Some(name) = path.file_name().map(|n| n.to_string_lossy()) else {
        return false;
    };
    name.starts_with("patch-") && name.ends_with(std::env::consts::DLL_EXTENSION)
}

fn is_cache_entry(path: &Path) -> bool {
    path.file_name()
        .map(|n| {
            let n = n.to_string_lossy();
            n.starts_with("build-") && n.ends_with(".json")
        })
        .unwrap_or(false)
}

/// Keep the `keep` newest artifacts (by build stamp) and `active`.
///
/// Returns what was kept and removed. Never removes `active`, even when it is not
/// among the newest: the process that runs it still has it mapped.
pub fn prune_artifacts(
    dir: &Path,
    keep: usize,
    active: Option<&Path>,
) -> io::Result<RetentionReport> {
    let mut artifacts: Vec<(u64, PathBuf)> = Vec::new();
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_file() && is_artifact(&path) {
                artifacts.push((stamp_of(&path).unwrap_or(0), path));
            }
        }
    }
    artifacts.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));

    let active_name = active.and_then(|p| p.file_name()).map(|n| n.to_os_string());
    let mut kept = Vec::new();
    let mut removed = Vec::new();
    for (index, (_, path)) in artifacts.iter().enumerate() {
        let is_active = active_name
            .as_ref()
            .map(|n| path.file_name() == Some(n.as_os_str()))
            .unwrap_or(false);
        if index < keep || is_active {
            kept.push(path.clone());
        } else {
            removed.push(path.clone());
        }
    }
    for path in &removed {
        fs::remove_file(path)?;
    }

    // Drop cache entries pointing at removed artifacts.
    let kept_names: HashSet<_> = kept
        .iter()
        .filter_map(|p| p.file_name().map(|n| n.to_os_string()))
        .collect();
    let mut cache_entries_removed = 0usize;
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if !is_cache_entry(&path) {
                continue;
            }
            let stale = match fs::read_to_string(&path) {
                Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
                    Ok(value) => value
                        .get("artifact_path")
                        .and_then(|v| v.as_str())
                        .map(|p| {
                            Path::new(p)
                                .file_name()
                                .map(|n| !kept_names.contains(n))
                                .unwrap_or(true)
                        })
                        .unwrap_or(true),
                    Err(_) => true,
                },
                Err(_) => true,
            };
            if stale {
                fs::remove_file(&path)?;
                cache_entries_removed += 1;
            }
        }
    }

    Ok(RetentionReport {
        kept,
        removed,
        cache_entries_removed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ravum-retention-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(dir: &Path, stamp: u64) -> PathBuf {
        let path = dir.join(format!(
            "patch-deadbeef-cafebabe-{stamp}.{}",
            std::env::consts::DLL_EXTENSION
        ));
        fs::write(&path, b"x").unwrap();
        path
    }

    #[test]
    fn keeps_the_newest_and_the_active_one() {
        let dir = dir("keep");
        let mut paths = Vec::new();
        for stamp in 1..=5 {
            paths.push(touch(&dir, stamp));
        }
        let active = paths[3].clone(); // stamp 4, not the newest
        let report = prune_artifacts(&dir, 1, Some(&active)).unwrap();
        assert_eq!(report.kept.len(), 2, "newest + active");
        assert_eq!(report.removed.len(), 3);
        assert!(active.is_file(), "the active artifact must survive");
        assert!(paths[4].is_file(), "the newest artifact must survive");
        assert!(!paths[0].is_file());
    }

    #[test]
    fn cache_entries_of_removed_artifacts_are_dropped() {
        let dir = dir("cache");
        let old = touch(&dir, 1);
        let new = touch(&dir, 2);
        let cache = dir.join("build-deadbeef-cafebabe.json");
        fs::write(
            &cache,
            serde_json::json!({ "artifact_path": old.to_string_lossy() }).to_string(),
        )
        .unwrap();
        let report = prune_artifacts(&dir, 1, None).unwrap();
        assert_eq!(report.cache_entries_removed, 1);
        assert!(!cache.exists());
        assert!(new.is_file());
    }

    #[test]
    fn an_empty_directory_is_not_an_error() {
        let dir = dir("empty");
        let report = prune_artifacts(&dir, 4, None).unwrap();
        assert!(report.kept.is_empty());
        assert!(report.removed.is_empty());
    }
}
