use super::*;

pub(super) fn compute_digests(graph: &mut DepGraph, diags: &mut DiagnosticBag) {
    for package in &mut graph.packages {
        match package_digest(&package.dir, diags) {
            Some(digest) => package.digest = digest,
            None => graph.failed = true,
        }
    }
}

/// SHA-256 of the package content: every `.rv` file under `src/` (sorted by
/// relative path) plus `ravum.toml`, encoded as `path\0len\0bytes` with `\r\n`
/// normalised to `\n` (see `spec/language/modules.md` MOD-7.2 / Q-MOD-6).
pub(super) fn package_digest(pkg_dir: &Path, diags: &mut DiagnosticBag) -> Option<String> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let manifest_path = pkg_dir.join(MANIFEST_NAME);
    if let Ok(Some(meta)) = symlink_metadata_optional(&manifest_path) {
        if meta.file_type().is_symlink() {
            err(
                diags,
                "E-MOD-PATH",
                Span::dummy(),
                format!(
                    "`{}` is a symbolic link; symlinks are refused for packages",
                    manifest_path.display()
                ),
            );
            return None;
        }
    }
    let manifest = match fs::read(&manifest_path) {
        Ok(bytes) => bytes,
        Err(e) => {
            diags.push(Diagnostic::error(
                "E-IO",
                Span::dummy(),
                format!("cannot read `{}`: {e}", manifest_path.display()),
            ));
            return None;
        }
    };
    if manifest.len() as u64 > MAX_FILE_SIZE {
        diags.push(Diagnostic::error(
            "E-LIMIT-FILE-SIZE",
            Span::dummy(),
            format!(
                "`{}` is larger than the {} byte limit",
                manifest_path.display(),
                MAX_FILE_SIZE
            ),
        ));
        return None;
    }
    files.push((MANIFEST_NAME.to_string(), normalize_newlines(&manifest)));

    let src = pkg_dir.join("src");
    if let Ok(Some(meta)) = symlink_metadata_optional(&src) {
        if meta.file_type().is_symlink() {
            err(
                diags,
                "E-MOD-PATH",
                Span::dummy(),
                format!(
                    "`{}` is a symbolic link; the package `src/` directory is never followed",
                    src.display()
                ),
            );
            return None;
        }
    }
    if symlink_metadata_optional(&src).ok().flatten().is_some() {
        let mut rv_files = Vec::new();
        if !collect_rv_files(&src, Path::new("src"), &mut rv_files, diags) {
            return None;
        }
        rv_files.sort();
        if rv_files.len() > MAX_PACKAGE_FILES {
            diags.push(Diagnostic::error(
                "E-LIMIT-FILES",
                Span::dummy(),
                format!(
                    "package `{}` contains more than {MAX_PACKAGE_FILES} `.rv` files",
                    pkg_dir.display()
                ),
            ));
            return None;
        }
        for rel in rv_files {
            let abs = pkg_dir.join(&rel);
            let data = match fs::read(&abs) {
                Ok(d) => d,
                Err(e) => {
                    diags.push(Diagnostic::error(
                        "E-IO",
                        Span::dummy(),
                        format!("cannot read `{}`: {e}", abs.display()),
                    ));
                    return None;
                }
            };
            if data.len() as u64 > MAX_FILE_SIZE {
                diags.push(Diagnostic::error(
                    "E-LIMIT-FILE-SIZE",
                    Span::dummy(),
                    format!(
                        "`{}` is larger than the {} byte limit",
                        abs.display(),
                        MAX_FILE_SIZE
                    ),
                ));
                return None;
            }
            files.push((path_to_slash(&rel), normalize_newlines(&data)));
        }
    }

    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut hasher = Sha256::new();
    for (path, bytes) in &files {
        hasher.update(path.as_bytes());
        hasher.update(&[0]);
        hasher.update(bytes.len().to_string().as_bytes());
        hasher.update(&[0]);
        hasher.update(bytes);
    }
    Some(format!("sha256:{}", hasher.finish_hex()))
}

pub(super) fn collect_rv_files(
    dir: &Path,
    rel: &Path,
    out: &mut Vec<PathBuf>,
    diags: &mut DiagnosticBag,
) -> bool {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            diags.push(Diagnostic::error(
                "E-IO",
                Span::dummy(),
                format!("cannot list `{}`: {e}", dir.display()),
            ));
            return false;
        }
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name();
        let child_rel = rel.join(&name);
        if is_reserved_windows_component(&name.to_string_lossy()) {
            diags.push(Diagnostic::error(
                "E-MOD-RESERVED-NAME",
                Span::dummy(),
                format!(
                    "`{}` is a reserved Windows path component; package sources must be \
                     portable",
                    child_rel.display()
                ),
            ));
            return false;
        }
        let path = entry.path();
        let meta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                diags.push(Diagnostic::error(
                    "E-IO",
                    Span::dummy(),
                    format!("cannot inspect `{}`: {e}", path.display()),
                ));
                return false;
            }
        };
        if meta.file_type().is_symlink() {
            diags.push(Diagnostic::error(
                "E-MOD-PATH",
                Span::dummy(),
                format!(
                    "symbolic link `{}` under `src/` is refused by the package digest",
                    path.display()
                ),
            ));
            return false;
        }
        if meta.is_dir() {
            if !collect_rv_files(&path, &child_rel, out, diags) {
                return false;
            }
        } else if meta.is_file() && path.extension().and_then(|e| e.to_str()) == Some("rv") {
            out.push(child_rel);
        }
    }
    true
}

pub(super) fn normalize_newlines(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            out.push(b'\n');
            i += 2;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

pub(super) fn aggregate_dependencies_digest(deps: &[Dependency]) -> String {
    let mut hasher = Sha256::new();
    for dep in deps {
        hasher.update(dep.name.as_bytes());
        hasher.update(&[0]);
        hasher.update(dep.version.as_bytes());
        hasher.update(&[0]);
        hasher.update(dep.digest.as_bytes());
        hasher.update(b"\n");
    }
    format!("sha256:{}", hasher.finish_hex())
}

pub(super) fn empty_dependencies_digest() -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"");
    format!("sha256:{}", hasher.finish_hex())
}

// ---------------------------------------------------------------------------
// Lockfile
// ---------------------------------------------------------------------------
