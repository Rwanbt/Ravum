use super::*;

pub(super) struct GraphPackage {
    pub(super) dir: PathBuf,
    pub(super) manifest: PackageManifest,
    pub(super) digest: String,
}

pub(super) struct DepGraph {
    pub(super) packages: Vec<GraphPackage>,
    pub(super) by_dir: BTreeMap<PathBuf, usize>,
    pub(super) by_name: BTreeMap<String, usize>,
    pub(super) failed: bool,
}

impl DepGraph {
    pub(super) fn new(root: &Path, manifest: PackageManifest) -> Self {
        let mut graph = Self {
            packages: Vec::new(),
            by_dir: BTreeMap::new(),
            by_name: BTreeMap::new(),
            failed: false,
        };
        graph.by_dir.insert(root.to_path_buf(), 0);
        graph.by_name.insert(manifest.name.clone(), 0);
        graph.packages.push(GraphPackage {
            dir: root.to_path_buf(),
            manifest,
            digest: String::new(),
        });
        graph
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn collect_dependencies(
    dir: &Path,
    manifest: &PackageManifest,
    depth: usize,
    stack: &mut Vec<(String, PathBuf)>,
    graph: &mut DepGraph,
    sources: &mut SourceMap,
    diags: &mut DiagnosticBag,
    file_ids: &mut BTreeMap<PathBuf, FileId>,
    next_file_id: &mut FileId,
) {
    for dep in &manifest.dependencies {
        let Some(dep_dir) = resolve_dependency_dir(dir, dep, diags) else {
            graph.failed = true;
            continue;
        };
        if dep_dir == dir || stack.iter().any(|(_, p)| *p == dep_dir) {
            let position = stack.iter().position(|(_, p)| *p == dep_dir).unwrap_or(0);
            let mut chain: Vec<String> = stack[position..].iter().map(|(n, _)| n.clone()).collect();
            chain.push(dep.name.clone());
            diags.push(Diagnostic::error(
                "E-MOD-CYCLE",
                dep.span,
                format!("dependency cycle: {}", chain.join(" -> ")),
            ));
            graph.failed = true;
            continue;
        }
        if let Some(&index) = graph.by_dir.get(&dep_dir) {
            let known = &graph.packages[index].manifest;
            if known.name != dep.name {
                diags.push(Diagnostic::error(
                    "E-MOD-DEPENDENCY",
                    dep.span,
                    format!(
                        "dependency key `{}` resolves to package `{}`; the key must equal the \
                         target `package.name`",
                        dep.name, known.name
                    ),
                ));
                graph.failed = true;
            }
            continue;
        }
        if depth + 1 > MAX_PACKAGE_DEPTH {
            diags.push(Diagnostic::error(
                "E-MOD-PATH-ESCAPE",
                dep.span,
                format!(
                    "dependency graph deeper than {MAX_PACKAGE_DEPTH} packages; \
                     check for an accidental cycle or a generated tree"
                ),
            ));
            graph.failed = true;
            continue;
        }

        let target_manifest_path = dep_dir.join(MANIFEST_NAME);
        let Some(text) = read_text_limited(
            &target_manifest_path,
            MAX_FILE_SIZE,
            "E-MOD-MANIFEST",
            diags,
        ) else {
            graph.failed = true;
            continue;
        };
        let display = format!("{}/{}", dep.name, MANIFEST_NAME);
        let canonical_manifest = fs::canonicalize(&target_manifest_path)
            .unwrap_or_else(|_| target_manifest_path.clone());
        let Some(file_id) = register_source(
            sources,
            file_ids,
            next_file_id,
            &canonical_manifest,
            display,
            text.clone(),
            diags,
        ) else {
            graph.failed = true;
            continue;
        };
        let (target, target_diags) = parse_manifest(&text, file_id);
        diags.extend(target_diags);
        let Some(target) = target else {
            graph.failed = true;
            continue;
        };
        // NAT-1.1 / MOD-5.20: only the root package links native libraries.
        if let Some(native) = target.native.first() {
            diags.push(Diagnostic::error(
                "E-MOD-NATIVE",
                native.span,
                format!(
                    "dependency `{}` declares native library `{}`: only the root package may have a `[native]` section",
                    dep.name, native.name
                ),
            ));
            graph.failed = true;
            continue;
        }
        if target.name != dep.name {
            diags.push(Diagnostic::error(
                "E-MOD-DEPENDENCY",
                dep.span,
                format!(
                    "dependency key `{}` does not match the target `package.name` (`{}`)",
                    dep.name, target.name
                ),
            ));
            graph.failed = true;
            continue;
        }
        if !is_module_identifier(&target.name) {
            diags.push(Diagnostic::error(
                "E-MOD-DEPENDENCY",
                dep.span,
                format!(
                    "dependency name `{}` is not usable as a module path in Core 1.0 \
                     (ASCII identifier required; `-` is not a module name character)",
                    target.name
                ),
            ));
            graph.failed = true;
            continue;
        }
        if is_reserved_windows_component(&target.name) {
            diags.push(Diagnostic::error(
                "E-MOD-RESERVED-NAME",
                dep.span,
                format!(
                    "dependency name `{}` is a reserved Windows path component; package names \
                     used as module names must be portable",
                    target.name
                ),
            ));
            graph.failed = true;
            continue;
        }
        match symlink_metadata_optional(&dep_dir.join("src").join("lib.rv")) {
            Ok(None) => {
                diags.push(Diagnostic::error(
                    "E-MOD-DEPENDENCY",
                    dep.span,
                    format!(
                        "dependency `{}` has no `src/lib.rv`; a binary package is not importable",
                        target.name
                    ),
                ));
                graph.failed = true;
                continue;
            }
            Ok(Some(meta)) if meta.file_type().is_symlink() => {
                diags.push(Diagnostic::error(
                    "E-MOD-PATH",
                    dep.span,
                    format!("dependency `{}` has a symlinked `src/lib.rv`", target.name),
                ));
                graph.failed = true;
                continue;
            }
            Ok(Some(meta)) if !meta.is_file() => {
                diags.push(Diagnostic::error(
                    "E-MOD-DEPENDENCY",
                    dep.span,
                    format!(
                        "dependency `{}`: `src/lib.rv` is not a regular file",
                        target.name
                    ),
                ));
                graph.failed = true;
                continue;
            }
            Ok(Some(_)) => {}
            Err(e) => {
                diags.push(Diagnostic::error(
                    "E-IO",
                    dep.span,
                    format!("cannot inspect dependency `{}`: {e}", target.name),
                ));
                graph.failed = true;
                continue;
            }
        }
        if graph.by_name.contains_key(&target.name) {
            diags.push(Diagnostic::error(
                "E-MOD-DUPLICATE",
                dep.span,
                format!(
                    "two different packages are named `{}`; Core 1.0 has no multi-version or \
                     alias resolution",
                    target.name
                ),
            ));
            graph.failed = true;
            continue;
        }

        let index = graph.packages.len();
        graph.packages.push(GraphPackage {
            dir: dep_dir.clone(),
            manifest: target.clone(),
            digest: String::new(),
        });
        graph.by_dir.insert(dep_dir.clone(), index);
        graph.by_name.insert(target.name.clone(), index);
        stack.push((target.name.clone(), dep_dir.clone()));
        collect_dependencies(
            &dep_dir,
            &target,
            depth + 1,
            stack,
            graph,
            sources,
            diags,
            file_ids,
            next_file_id,
        );
        stack.pop();
    }
}

/// Resolve a dependency `path` relative to `pkg_dir`, refusing absolute paths,
/// Windows prefixes and symbolic links. `..` is allowed (local path deps).
pub(super) fn resolve_dependency_dir(
    pkg_dir: &Path,
    dep: &DependencyDecl,
    diags: &mut DiagnosticBag,
) -> Option<PathBuf> {
    let raw = dep.path.trim();
    if raw.is_empty() {
        diags.push(Diagnostic::error(
            "E-MOD-DEPENDENCY",
            dep.span,
            format!("dependency `{}` has an empty path", dep.name),
        ));
        return None;
    }
    if raw.contains('\0') {
        diags.push(Diagnostic::error(
            "E-MOD-PATH-ESCAPE",
            dep.span,
            format!("dependency `{}` path contains a NUL byte", dep.name),
        ));
        return None;
    }
    let path = Path::new(raw);
    if path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, Component::Prefix(_) | Component::RootDir))
    {
        diags.push(Diagnostic::error(
            "E-MOD-PATH-ESCAPE",
            dep.span,
            format!(
                "dependency `{}` path `{raw}` is absolute; only package-relative local paths \
                 are accepted",
                dep.name
            ),
        ));
        return None;
    }
    let depth = path
        .components()
        .filter(|component| matches!(component, Component::Normal(_) | Component::ParentDir))
        .count();
    if depth > MAX_DEPENDENCY_PATH_DEPTH {
        diags.push(Diagnostic::error(
            "E-MOD-PATH-ESCAPE",
            dep.span,
            format!(
                "dependency `{}` path `{raw}` has {depth} components; Core 1.0 accepts at most \
                 {MAX_DEPENDENCY_PATH_DEPTH}",
                dep.name
            ),
        ));
        return None;
    }

    // Path policy **before** any filesystem lookup: `..` is bounded by the
    // package parent directory. A lexical escape is `E-MOD-PATH-ESCAPE` even
    // when the target does not exist (conformance `security/dependency-path-escape`).
    let Some(allowed_parent) = pkg_dir.parent() else {
        diags.push(Diagnostic::error(
            "E-MOD-PATH-ESCAPE",
            dep.span,
            format!(
                "dependency `{}` path `{raw}`: the package has no parent directory to bound \
                 local dependencies",
                dep.name
            ),
        ));
        return None;
    };
    // The absolute-path check above refuses every `Prefix` and `RootDir`
    // component, so only `.`, `..` and names remain. A `..` that cannot pop
    // (above the filesystem root) is an escape, like one that leaves the
    // package parent directory.
    let mut normalized = pkg_dir.to_path_buf();
    let mut escaped = false;
    for component in path.components() {
        let stayed = match component {
            Component::Normal(name) => {
                normalized.push(name);
                true
            }
            Component::ParentDir => normalized.pop(),
            _ => true,
        };
        if !stayed || !normalized.starts_with(allowed_parent) {
            escaped = true;
            break;
        }
    }
    if escaped {
        diags.push(Diagnostic::error(
            "E-MOD-PATH-ESCAPE",
            dep.span,
            format!(
                "dependency `{}` path `{raw}` escapes the package parent directory; `..` is \
                 only allowed within the parent directory",
                dep.name
            ),
        ));
        return None;
    }

    // Same components, now checked against the filesystem. The lexical pass
    // above guarantees `..` never climbs above the package parent directory.
    let mut current = pkg_dir.to_path_buf();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                current.pop();
            }
            Component::Normal(name) => {
                let component = name.to_string_lossy();
                if is_reserved_windows_component(&component) {
                    diags.push(Diagnostic::error(
                        "E-MOD-RESERVED-NAME",
                        dep.span,
                        format!(
                            "dependency `{}` path `{raw}` contains the reserved Windows \
                             component `{component}`",
                            dep.name
                        ),
                    ));
                    return None;
                }
                current.push(name);
                match symlink_metadata_optional(&current) {
                    Ok(Some(meta)) if meta.file_type().is_symlink() => {
                        diags.push(Diagnostic::error(
                            "E-MOD-PATH",
                            dep.span,
                            format!(
                                "dependency `{}` path `{raw}` traverses the symbolic link `{}`; \
                                 symlinks are refused for dependencies",
                                dep.name,
                                current.display()
                            ),
                        ));
                        return None;
                    }
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        diags.push(Diagnostic::error(
                            "E-MOD-DEPENDENCY",
                            dep.span,
                            format!(
                                "dependency `{}` path `{raw}` does not exist (`{}` is missing)",
                                dep.name,
                                current.display()
                            ),
                        ));
                        return None;
                    }
                    Err(e) => {
                        diags.push(Diagnostic::error(
                            "E-IO",
                            dep.span,
                            format!(
                                "dependency `{}`: cannot inspect `{}`: {e}",
                                dep.name,
                                current.display()
                            ),
                        ));
                        return None;
                    }
                }
            }
            _ => {}
        }
    }

    let canonical = match fsutil::canonicalize(&current) {
        Ok(p) => p,
        Err(e) => {
            diags.push(Diagnostic::error(
                "E-MOD-DEPENDENCY",
                dep.span,
                format!(
                    "dependency `{}` path `{raw}` cannot be resolved (`{}`): {e}",
                    dep.name,
                    current.display()
                ),
            ));
            return None;
        }
    };
    // `..` is bounded by the parent directory of the package (Q-MOD-2):
    // siblings such as `../util` are legitimate, `../../outside` is an escape.
    let parent = pkg_dir.parent();
    if !parent
        .map(|allowed| canonical.starts_with(allowed))
        .unwrap_or(false)
    {
        diags.push(Diagnostic::error(
            "E-MOD-PATH-ESCAPE",
            dep.span,
            format!(
                "dependency `{}` path `{raw}` resolves to `{}`, outside the package parent \
                 directory; `..` is only allowed within the parent directory",
                dep.name,
                canonical.display()
            ),
        ));
        return None;
    }
    if !canonical.is_dir() {
        diags.push(Diagnostic::error(
            "E-MOD-DEPENDENCY",
            dep.span,
            format!(
                "dependency `{}` path `{raw}` is not a directory (`{}`)",
                dep.name,
                canonical.display()
            ),
        ));
        return None;
    }
    if !canonical.join(MANIFEST_NAME).is_file() {
        diags.push(Diagnostic::error(
            "E-MOD-DEPENDENCY",
            dep.span,
            format!(
                "dependency `{}` path `{raw}` does not contain a `{MANIFEST_NAME}` \
                 (`{}`)",
                dep.name,
                canonical.display()
            ),
        ));
        return None;
    }
    Some(canonical)
}

// ---------------------------------------------------------------------------
// Content digests
// ---------------------------------------------------------------------------
