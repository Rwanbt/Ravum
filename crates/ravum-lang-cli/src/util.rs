//! Small filesystem and formatting helpers shared by the commands.
//!
//! Security rules mirrored from the loader: never follow a symbolic link,
//! bound every read, and write only inside the package cache unless the user
//! asked for an explicit output path.

use std::fs;
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};

pub const EXIT_OK: i32 = 0;
pub const EXIT_FAIL: i32 = 1;
pub const EXIT_USAGE: i32 = 2;
pub const EXIT_TOOL: i32 = 3;

/// Maximum size of a source file read by the CLI (mirrors the loader limit).
pub const MAX_FILE_SIZE: u64 = 8 * 1024 * 1024;

/// Cache directory created under the package root.
pub const CACHE_DIR: &str = ".ravum";

pub fn err(message: impl std::fmt::Display) {
    eprintln!("error: {message}");
}

/// Read a UTF-8 file, refusing symlinks and oversized files.
pub fn read_text(path: &Path) -> Result<String, String> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) => return Err(format!("cannot inspect `{}`: {e}", path.display())),
    };
    if meta.file_type().is_symlink() {
        return Err(format!(
            "`{}` is a symbolic link; symlinks are refused",
            path.display()
        ));
    }
    if !meta.is_file() {
        return Err(format!("`{}` is not a regular file", path.display()));
    }
    if meta.len() > MAX_FILE_SIZE {
        return Err(format!(
            "`{}` is {} bytes, above the {} byte limit",
            path.display(),
            meta.len(),
            MAX_FILE_SIZE
        ));
    }
    let bytes = fs::read(path).map_err(|e| format!("cannot read `{}`: {e}", path.display()))?;
    String::from_utf8(bytes).map_err(|_| format!("`{}` is not valid UTF-8", path.display()))
}

/// Atomic write: write a temporary sibling then rename it over the target.
pub fn write_atomic(path: &Path, contents: &str) -> Result<(), String> {
    write_bytes_atomic(path, contents.as_bytes())
}

pub fn write_bytes_atomic(path: &Path, contents: &[u8]) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|e| format!("cannot create `{}`: {e}", parent.display()))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "ravum.tmp".to_string());
    let tmp = parent.join(format!(".{name}.tmp-{}", std::process::id()));
    let result = (|| -> std::io::Result<()> {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(contents)?;
        let _ = file.sync_all();
        drop(file);
        fs::rename(&tmp, path)
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&tmp);
        return Err(format!("cannot write `{}`: {e}", path.display()));
    }
    Ok(())
}

/// Publish `source` at `target` with a single atomic rename.
///
/// Both paths must live on the same volume (the isolated build directory is
/// created under `<package>/.ravum/build/`, i.e. inside the package). Platform
/// limits, measured rather than assumed:
/// * Windows: `fs::rename` is `MoveFileExW(..., MOVEFILE_REPLACE_EXISTING)`. It
///   replaces an existing file atomically, but it fails while the previous
///   artifact is memory-mapped/running (a running `.exe` cannot be replaced);
///   the previous artifact is then left untouched and the error is reported.
/// * Unix: `rename(2)`, atomic within a filesystem.
/// * There is deliberately **no** copy fallback: a copy would publish a partial
///   file, so a cross-volume rename is reported as an error instead of silently
///   weakening the publication guarantee.
pub fn publish_rename(source: &Path, target: &Path) -> Result<(), String> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|e| format!("cannot create `{}`: {e}", parent.display()))?;
    fs::rename(source, target).map_err(|e| {
        format!(
            "cannot publish `{}` as `{}`: {e}; the previously published artifact, if any, was \
             left untouched (a rename never crosses volumes and Windows refuses to replace a \
             running executable)",
            source.display(),
            target.display()
        )
    })
}

/// Resolve the package root used by `cache`/`doctor`: the first ancestor of
/// `start` (directories only) that contains `ravum.toml`, else `start`.
pub fn find_package_root(start: &Path) -> PathBuf {
    let canonical = fs::canonicalize(start).unwrap_or_else(|_| start.to_path_buf());
    let base = if canonical.is_file() {
        canonical
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or(canonical.clone())
    } else {
        canonical
    };
    let mut current = base.as_path();
    for _ in 0..16 {
        if current.join("ravum.toml").is_file() {
            return current.to_path_buf();
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => break,
        }
    }
    base
}

/// Create and validate `<root>/.ravum`, refusing a symlinked cache directory.
pub fn ensure_cache_dir(root: &Path) -> Result<PathBuf, String> {
    let cache = root.join(CACHE_DIR);
    match fs::symlink_metadata(&cache) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(format!(
                "`{}` is a symbolic link; the cache directory is never followed",
                cache.display()
            ));
        }
        Ok(meta) if !meta.is_dir() => {
            return Err(format!("`{}` is not a directory", cache.display()));
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(&cache)
                .map_err(|e| format!("cannot create `{}`: {e}", cache.display()))?;
        }
        Err(e) => return Err(format!("cannot inspect `{}`: {e}", cache.display())),
    }
    let canonical_root =
        fs::canonicalize(root).map_err(|e| format!("cannot resolve `{}`: {e}", root.display()))?;
    let canonical_cache = fs::canonicalize(&cache)
        .map_err(|e| format!("cannot resolve `{}`: {e}", cache.display()))?;
    if canonical_cache.parent() != Some(canonical_root.as_path()) {
        return Err(format!(
            "`{}` is outside the package directory; refusing to use it",
            cache.display()
        ));
    }
    // Return the *non-verbatim* form: on Windows `fs::canonicalize` yields a
    // `\\?\C:\...` path, and the MSVC linker (`link.exe`, which rustc spawns)
    // mis-parses verbatim paths in some positions ("LNK1181: cannot open input
    // file '.obj'"). The canonical form is only needed for the containment check
    // above, which is already done.
    Ok(strip_verbatim(&canonical_cache))
}

/// Collect every `.rv` file under `dir` (sorted, symlinks refused).
pub fn collect_rv_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = fs::read_dir(&current)
            .map_err(|e| format!("cannot list `{}`: {e}", current.display()))?;
        let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let meta = fs::symlink_metadata(&path)
                .map_err(|e| format!("cannot inspect `{}`: {e}", path.display()))?;
            if meta.file_type().is_symlink() {
                return Err(format!(
                    "symbolic link `{}` under the package source tree is refused",
                    path.display()
                ));
            }
            if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file() && path.extension().and_then(|e| e.to_str()) == Some("rv") {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Directory size and file count, bounded to avoid pathological caches.
pub struct DirStats {
    pub bytes: u64,
    pub files: usize,
    pub dirs: usize,
}

pub fn dir_stats(dir: &Path) -> std::io::Result<DirStats> {
    let mut stats = DirStats {
        bytes: 0,
        files: 0,
        dirs: 0,
    };
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in fs::read_dir(&current)?.filter_map(Result::ok) {
            let meta = fs::symlink_metadata(entry.path())?;
            if meta.is_dir() {
                stats.dirs += 1;
                stack.push(entry.path());
            } else if meta.is_file() {
                stats.files += 1;
                stats.bytes += meta.len();
            }
        }
    }
    Ok(stats)
}

/// Top-level cache entries with their sizes, sorted by name.
pub fn cache_entries(cache: &Path) -> Result<Vec<(String, u64, bool)>, String> {
    let mut out = Vec::new();
    let entries =
        fs::read_dir(cache).map_err(|e| format!("cannot list `{}`: {e}", cache.display()))?;
    for entry in entries.filter_map(Result::ok) {
        let meta = fs::symlink_metadata(entry.path())
            .map_err(|e| format!("cannot inspect `{}`: {e}", entry.path().display()))?;
        let is_dir = meta.is_dir();
        let size = if is_dir {
            dir_stats(&entry.path()).map(|s| s.bytes).unwrap_or(0)
        } else {
            meta.len()
        };
        out.push((
            entry.file_name().to_string_lossy().to_string(),
            size,
            is_dir,
        ));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// `crate_name` acceptable to rustc (`[A-Za-z_][A-Za-z0-9_]*`).
pub fn rust_crate_name(package: Option<&str>) -> String {
    let raw = package.unwrap_or("ravum_program");
    let mut out = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

/// Relative display path with `/` separators (stable JSON output).
pub fn display_path(path: &Path, base: &Path) -> String {
    let rel = path.strip_prefix(base).unwrap_or(path);
    rel.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().to_string()),
            Component::ParentDir => Some("..".to_string()),
            Component::CurDir => None,
            Component::RootDir => Some(String::new()),
            Component::Prefix(prefix) => Some(prefix.as_os_str().to_string_lossy().to_string()),
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Remove the Windows verbatim prefix (`\\?\C:\...` -> `C:\...`) for display.
pub fn strip_verbatim(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = text.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
    }
    path.to_path_buf()
}

/// Natural, human-readable path (no `\\?\` verbatim prefix).
pub fn pretty_path(path: &Path) -> String {
    strip_verbatim(path).display().to_string()
}

/// Stable, machine-independent target path for JSON output: relative to the
/// current directory when possible, otherwise only the final component (never
/// an absolute path, per `spec/language/diagnostics.md` DIAG-7.5).
pub fn target_display(path: &Path) -> String {
    let cwd = strip_verbatim(&std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let absolute = strip_verbatim(&fs::canonicalize(path).unwrap_or_else(|_| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            cwd.join(path)
        }
    }));
    let shown = match absolute.strip_prefix(&cwd) {
        Ok(relative) => relative.to_path_buf(),
        Err(_) => PathBuf::from(
            absolute
                .file_name()
                .map(|name| name.to_os_string())
                .unwrap_or_else(|| std::ffi::OsString::from(".")),
        ),
    };
    display_path(&shown, Path::new(""))
}

pub fn executable_name(package: Option<&str>) -> String {
    let base = rust_crate_name(package);
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base
    }
}
