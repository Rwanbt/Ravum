use super::*;

pub(super) fn load_single_file(
    path: &Path,
    options: &LoadOptions,
    cache: &mut ParseCache,
) -> LoadedPackage {
    let mut out = LoadedPackage::empty(path);
    let canonical = match fs::canonicalize(path) {
        Ok(p) => p,
        Err(e) => {
            err(
                &mut out.diags,
                "E-IO",
                Span::dummy(),
                format!("cannot canonicalize `{}`: {e}", path.display()),
            );
            return out;
        }
    };
    let parent = canonical
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    out.root_dir = parent.display().to_string();
    out.package_name = canonical
        .file_stem()
        .map(|s| s.to_string_lossy().to_string());
    out.crate_kind = Some(CrateKind::File);

    if canonical.extension().and_then(|e| e.to_str()) != Some("rv") {
        err(
            &mut out.diags,
            "E-IO",
            Span::dummy(),
            format!(
                "`{}` is not a `.rv` file; pass a package directory instead",
                path.display()
            ),
        );
        return out;
    }

    let mut sources = SourceMap::new();
    let mut diags = DiagnosticBag::new();
    let mut file_ids: BTreeMap<PathBuf, FileId> = options.file_ids.clone().unwrap_or_default();
    let mut next_file_id = next_free_file_id(&file_ids);
    let mut synthetic: u64 = ((SYNTHETIC_FILE as u64) << 32) | 1;
    let mut parse_time = Duration::ZERO;

    let program = {
        let mut ctx = CrateCtx {
            src_root: parent.clone(),
            display_base: parent.clone(),
            display_prefix: String::new(),
            sources: &mut sources,
            diags: &mut diags,
            file_ids: &mut file_ids,
            loaded: BTreeSet::new(),
            synthetic: &mut synthetic,
            parse_time: &mut parse_time,
            next_file_id: &mut next_file_id,
            parse_cache: cache,
            overrides: &options.overrides,
        };
        load_crate_root(&mut ctx, &canonical).map(|module| Program {
            file: ctx.file_ids.get(&canonical).copied().unwrap_or(0),
            root: module,
        })
    };

    out.loaded_files = display_files(&sources);
    out.parse_time = parse_time;
    if !diags.has_errors() {
        out.program = program;
    }
    out.sources = sources;
    out.diags = diags;
    out.file_ids = file_ids;
    out
}

// ---------------------------------------------------------------------------
// Package mode
// ---------------------------------------------------------------------------

pub(super) fn load_package_dir(
    input: &Path,
    options: &LoadOptions,
    cache: &mut ParseCache,
) -> LoadedPackage {
    let mut out = LoadedPackage::empty(input);
    let root = match fs::canonicalize(input) {
        Ok(p) => p,
        Err(e) => {
            err(
                &mut out.diags,
                "E-IO",
                Span::dummy(),
                format!("cannot canonicalize `{}`: {e}", input.display()),
            );
            return out;
        }
    };
    out.root_dir = root.display().to_string();

    let mut sources = SourceMap::new();
    let mut diags = DiagnosticBag::new();
    let mut file_ids: BTreeMap<PathBuf, FileId> = options.file_ids.clone().unwrap_or_default();
    let mut next_file_id = next_free_file_id(&file_ids);

    // 1. Manifest (registered first so its diagnostics carry a real location).
    let manifest_path = root.join(MANIFEST_NAME);
    if !manifest_path.is_file() {
        // Convenience required by `check --json` on a lone script: a directory
        // with exactly one top-level `.rv` file and no manifest is treated as
        // single-file mode. More than one is ambiguous and stays an error.
        match top_level_rv_files(&root) {
            Ok(mut files) if files.len() == 1 => {
                return load_single_file(&files.remove(0), options, cache);
            }
            Ok(files) if files.len() > 1 => {
                err(
                    &mut diags,
                    "E-MOD-MANIFEST",
                    Span::dummy(),
                    format!(
                        "no `{MANIFEST_NAME}` in `{}` and {} top-level `.rv` files; pass one \
                         file explicitly or run `ravum-lang init`",
                        root.display(),
                        files.len()
                    ),
                );
                let mut out = LoadedPackage::empty(input);
                out.diags = diags;
                return out;
            }
            _ => {}
        }
    }
    let manifest_text =
        match read_text_limited(&manifest_path, MAX_FILE_SIZE, "E-MOD-MANIFEST", &mut diags) {
            Some(t) => t,
            None => {
                // `read_text_limited` reports every refusal it answers `None` to
                // (missing manifest included: "`…/ravum.toml` does not exist").
                out.diags = diags;
                return out;
            }
        };
    let manifest_id = register_manifest(
        &mut sources,
        &mut file_ids,
        &mut next_file_id,
        &manifest_path,
        manifest_text.clone(),
        &mut diags,
    );
    let (manifest, manifest_diags) = parse_manifest(&manifest_text, manifest_id);
    diags.extend(manifest_diags);
    let Some(manifest) = manifest else {
        out.sources = sources;
        out.diags = diags;
        return out;
    };

    out.package_name = Some(manifest.name.clone());
    out.package_version = Some(manifest.version.clone());
    out.edition = Some(manifest.edition.clone());

    // 2. Dependency graph + digests.
    let mut graph = DepGraph::new(&root, manifest.clone());
    let mut stack: Vec<(String, PathBuf)> = vec![(manifest.name.clone(), root.clone())];
    collect_dependencies(
        &root,
        &manifest,
        0,
        &mut stack,
        &mut graph,
        &mut sources,
        &mut diags,
        &mut file_ids,
        &mut next_file_id,
    );

    if !graph.failed {
        compute_digests(&mut graph, &mut diags);
    }

    let mut state = LockfileState::NotApplicable;
    if !graph.failed && !diags.has_errors() {
        state = update_lockfile(&root, &graph, options.locked, &mut diags);
    }

    if state != LockfileState::NotApplicable {
        out.lockfile_state = state;
        out.dependencies = graph
            .packages
            .iter()
            .skip(1)
            .map(|p| Dependency {
                name: p.manifest.name.clone(),
                version: p.manifest.version.clone(),
                digest: p.digest.clone(),
            })
            .collect();
        out.dependencies
            .sort_by(|a, b| a.name.cmp(&b.name).then(a.version.cmp(&b.version)));
        out.dependencies_digest = aggregate_dependencies_digest(&out.dependencies);
    }

    // 3. Crate roots. Errors above make the program unusable; skip parsing then.
    let mut parse_time = Duration::ZERO;
    if !diags.has_errors() {
        let mut caches = LoadCratesCaches {
            parse_time: &mut parse_time,
            parse_cache: cache,
            options,
        };
        let (program, lib_program, kind) = load_crates(
            &root,
            &graph,
            &mut sources,
            &mut diags,
            &mut caches,
            &mut file_ids,
            &mut next_file_id,
        );
        // Module/parse errors are discovered while loading: never hand a
        // partially loaded program to the checker.
        if !diags.has_errors() {
            out.program = program;
        }
        out.lib_program = lib_program;
        out.crate_kind = kind;
    }

    out.loaded_files = display_files(&sources);
    out.parse_time = parse_time;
    out.sources = sources;
    out.diags = diags;
    out.file_ids = file_ids;
    out
}

// ---------------------------------------------------------------------------
// Manifest parsing: minimal TOML subset
// ---------------------------------------------------------------------------

/// Registers the manifest of a load. It is the first source of the load's
/// fresh table, so its registration cannot collide with another file; a
/// table that already holds a file is a caller bug.
pub(super) fn register_manifest(
    sources: &mut SourceMap,
    file_ids: &mut BTreeMap<PathBuf, FileId>,
    next_file_id: &mut FileId,
    manifest_path: &Path,
    manifest_text: String,
    diags: &mut DiagnosticBag,
) -> FileId {
    register_source(
        sources,
        file_ids,
        next_file_id,
        manifest_path,
        MANIFEST_NAME.to_string(),
        manifest_text,
        diags,
    )
    .expect("invariant: the manifest is the first source registered by a load")
}
