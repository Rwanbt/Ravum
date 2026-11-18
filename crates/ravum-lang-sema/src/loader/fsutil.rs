use super::*;

/// `fs::canonicalize` for the checks that can lose a race against the file
/// system (a component removed or replaced between the checks and this call).
/// Tests replace it thread-locally to inject that failure or another answer.
pub(super) fn canonicalize(path: &Path) -> std::io::Result<PathBuf> {
    #[cfg(test)]
    if let Some(fake) = TEST_CANONICALIZE.with(std::cell::Cell::get) {
        return fake(path);
    }
    fs::canonicalize(path)
}

/// Test override of [`canonicalize`] (`None`: the real call).
#[cfg(test)]
pub(super) type FakeCanonicalize = fn(&Path) -> std::io::Result<PathBuf>;

#[cfg(test)]
thread_local! {
    pub(super) static TEST_CANONICALIZE: std::cell::Cell<Option<FakeCanonicalize>> =
        const { std::cell::Cell::new(None) };
}

pub(super) fn next_synthetic(counter: &mut NodeId) -> NodeId {
    let id = *counter;
    *counter = counter.saturating_add(1);
    if (*counter >> 32) != u64::from(SYNTHETIC_FILE) {
        *counter = (u64::from(SYNTHETIC_FILE) << 32) | 1;
    }
    id
}

// ---------------------------------------------------------------------------
// Filesystem helpers
// ---------------------------------------------------------------------------

pub(super) fn read_text_limited(
    path: &Path,
    limit: u64,
    code: &str,
    diags: &mut DiagnosticBag,
) -> Option<String> {
    match symlink_metadata_optional(path) {
        Ok(Some(meta)) if meta.file_type().is_symlink() => {
            err(
                diags,
                "E-MOD-PATH",
                Span::dummy(),
                format!(
                    "`{}` is a symbolic link; refusing to read it",
                    path.display()
                ),
            );
            return None;
        }
        Ok(Some(meta)) if meta.is_file() => {
            if meta.len() > limit {
                err(
                    diags,
                    "E-LIMIT-FILE-SIZE",
                    Span::dummy(),
                    format!(
                        "`{}` is {} bytes, above the {} byte limit",
                        path.display(),
                        meta.len(),
                        limit
                    ),
                );
                return None;
            }
        }
        Ok(Some(_)) => {
            err(
                diags,
                code,
                Span::dummy(),
                format!("`{}` is not a regular file", path.display()),
            );
            return None;
        }
        Ok(None) => {
            err(
                diags,
                code,
                Span::dummy(),
                format!("`{}` does not exist", path.display()),
            );
            return None;
        }
        Err(e) => {
            err(
                diags,
                code,
                Span::dummy(),
                format!("cannot inspect `{}`: {e}", path.display()),
            );
            return None;
        }
    }
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            err(
                diags,
                code,
                Span::dummy(),
                format!("cannot read `{}`: {e}", path.display()),
            );
            return None;
        }
    };
    match String::from_utf8(bytes) {
        Ok(text) => Some(text),
        Err(_) => {
            err(
                diags,
                "E-UNSUPPORTED-TOKEN",
                Span::dummy(),
                format!(
                    "`{}` is not valid UTF-8; Ravum Core 1.0 sources are UTF-8",
                    path.display()
                ),
            );
            None
        }
    }
}

pub(super) fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "tmp".to_string());
    let tmp = parent.join(format!(".{file_name}.tmp-{}", std::process::id()));
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        let _ = file.sync_all();
    }
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

pub(super) fn symlink_metadata_optional(path: &Path) -> std::io::Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(meta) => Ok(Some(meta)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

pub(super) fn path_is_under(path: &Path, base: &Path) -> bool {
    path.strip_prefix(base).is_ok()
}

/// Walk the *lexical* path `target` (which must start with `base`) and refuse
/// any symbolic-link component. This is required in addition to canonical
/// escape checks: a symlink inside `src/` pointing to another `src/` directory
/// would otherwise be silently accepted.
pub(super) fn check_lexical_no_symlinks(
    base: &Path,
    target: &Path,
    diags: &mut DiagnosticBag,
) -> bool {
    let Ok(relative) = target.strip_prefix(base) else {
        return true;
    };
    let mut current = base.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match symlink_metadata_optional(&current) {
            Ok(Some(meta)) if meta.file_type().is_symlink() => {
                err(
                    diags,
                    "E-MOD-PATH",
                    Span::dummy(),
                    format!(
                        "`{}` traverses the symbolic link `{}`; symlinks are refused",
                        target.display(),
                        current.display()
                    ),
                );
                return false;
            }
            Ok(_) => {}
            Err(e) => {
                err(
                    diags,
                    "E-IO",
                    Span::dummy(),
                    format!("cannot inspect `{}`: {e}", current.display()),
                );
                return false;
            }
        }
    }
    true
}

/// Walk from `base` to `target` (both canonical) and return an error if any
/// component in between is a symbolic link. Returns the target on success.
pub(super) fn check_path_no_symlinks(
    base: &Path,
    target: &Path,
    diags: &mut DiagnosticBag,
) -> Option<()> {
    let Ok(relative) = target.strip_prefix(base) else {
        return Some(());
    };
    let mut current = base.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match symlink_metadata_optional(&current) {
            Ok(Some(meta)) if meta.file_type().is_symlink() => {
                err(
                    diags,
                    "E-MOD-PATH",
                    Span::dummy(),
                    format!(
                        "`{}` traverses the symbolic link `{}`; symlinks are refused",
                        target.display(),
                        current.display()
                    ),
                );
                return None;
            }
            Ok(_) => {}
            Err(e) => {
                err(
                    diags,
                    "E-IO",
                    Span::dummy(),
                    format!("cannot inspect `{}`: {e}", current.display()),
                );
                return None;
            }
        }
    }
    Some(())
}

/// True when `dir` holds an entry whose stored name is exactly `name`
/// (byte for byte), whatever the case sensitivity of the file system.
pub(super) fn has_exact_entry(dir: &Path, name: &str) -> bool {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .any(|entry| entry.file_name() == std::ffi::OsStr::new(name))
        })
        .unwrap_or(false)
}

pub(super) fn display_relative(path: &Path, base: &Path) -> String {
    match path.strip_prefix(base) {
        Ok(rel) => path_to_slash(rel),
        Err(_) => path_to_slash(path),
    }
}

pub(super) fn path_to_slash(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::CurDir => None,
            Component::ParentDir => Some("..".to_string()),
            Component::RootDir => Some(String::new()),
            // A normal part and a Windows prefix (`C:`) are written as they are.
            other => Some(other.as_os_str().to_string_lossy().to_string()),
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Top-level regular `.rv` files of a directory (symlinks ignored; they would be
/// refused by [`load_single_file`] anyway).
pub(super) fn top_level_rv_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let meta = fs::symlink_metadata(&path)?;
        if meta.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("rv") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

pub(super) fn display_files(sources: &SourceMap) -> Vec<String> {
    sources
        .files()
        .iter()
        .map(|file| file.path.clone())
        .collect()
}

pub(super) fn err(diags: &mut DiagnosticBag, code: &str, span: Span, message: impl Into<String>) {
    diags.push(Diagnostic::error(code, span, message));
}

// ---------------------------------------------------------------------------
// SHA-256 (internal, dependency-free; tested against FIPS 180-4 vectors)
// ---------------------------------------------------------------------------
