use super::*;

pub(super) struct CrateCtx<'a> {
    /// Canonical `src/` directory of the package (module escape bound).
    pub(super) src_root: PathBuf,
    /// Canonical package directory used to render display paths.
    pub(super) display_base: PathBuf,
    /// Prefix for display paths (`"util/"` for dependency sources).
    pub(super) display_prefix: String,
    pub(super) sources: &'a mut SourceMap,
    pub(super) diags: &'a mut DiagnosticBag,
    pub(super) file_ids: &'a mut BTreeMap<PathBuf, FileId>,
    /// Next id handed to a file the session table has not seen yet. Shared by
    /// the crate and every dependency so ids stay unique across a load.
    pub(super) next_file_id: &'a mut FileId,
    // Files already included once in *this* crate (duplicate detection).
    pub(super) loaded: BTreeSet<PathBuf>,
    pub(super) synthetic: &'a mut u64,
    /// Accumulated wall-clock time spent in the parser (see `parse_time`).
    pub(super) parse_time: &'a mut Duration,
    /// Per-session parse cache (see [`ParseCache`]).
    pub(super) parse_cache: &'a mut ParseCache,
    /// Canonical path -> in-memory text (editor buffers not yet saved).
    pub(super) overrides: &'a BTreeMap<PathBuf, String>,
}

/// Parse-time buffers shared by the crate-root loader.
pub(super) struct LoadCratesCaches<'a> {
    pub(super) parse_time: &'a mut Duration,
    pub(super) parse_cache: &'a mut ParseCache,
    pub(super) options: &'a LoadOptions,
}

pub(super) fn load_crates(
    root: &Path,
    graph: &DepGraph,
    sources: &mut SourceMap,
    diags: &mut DiagnosticBag,
    caches: &mut LoadCratesCaches<'_>,
    file_ids: &mut BTreeMap<PathBuf, FileId>,
    next_file_id: &mut FileId,
) -> (Option<Program>, Option<Program>, Option<CrateKind>) {
    let parse_time = &mut *caches.parse_time;
    let parse_cache = &mut *caches.parse_cache;
    let options = caches.options;
    let src = root.join("src");
    let main_path = src.join("main.rv");
    let lib_path = src.join("lib.rv");
    let has_main = match regular_file(&main_path, diags) {
        Ok(v) => v,
        Err(()) => return (None, None, None),
    };
    let has_lib = match regular_file(&lib_path, diags) {
        Ok(v) => v,
        Err(()) => return (None, None, None),
    };
    if !has_main && !has_lib {
        err(
            diags,
            "E-MOD-MISSING",
            Span::dummy(),
            format!(
                "package `{}` has neither `src/main.rv` nor `src/lib.rv`",
                root.display()
            ),
        );
        return (None, None, None);
    }
    if has_main && has_lib {
        // Decision Core 1.0 (Lead): no silent choice between two roots. Also
        // documents that `main.rv` importing the package library is
        // UNSUPPORTED (E-UNSUPPORTED-SELF-LIB, checker-side) — keeping a single
        // root makes the rule unambiguous.
        err(
            diags,
            "E-MOD-BOTH-ROOTS",
            Span::dummy(),
            format!(
                "package `{}` contains both `src/main.rv` and `src/lib.rv`; Core 1.0 \
                 requires exactly one crate root (remove one of them)",
                root.display()
            ),
        );
        return (None, None, None);
    }

    let mut synthetic: u64 = ((SYNTHETIC_FILE as u64) << 32) | 1;
    let (root_file, kind) = if has_main {
        (&main_path, CrateKind::Bin)
    } else {
        (&lib_path, CrateKind::Lib)
    };
    let program = load_crate_program(
        root_file,
        root,
        &src,
        graph,
        sources,
        diags,
        file_ids,
        next_file_id,
        &mut synthetic,
        parse_time,
        parse_cache,
        options,
    );
    (program, None, Some(kind))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn load_crate_program(
    root_file: &Path,
    package_root: &Path,
    src_root: &Path,
    graph: &DepGraph,
    sources: &mut SourceMap,
    diags: &mut DiagnosticBag,
    file_ids: &mut BTreeMap<PathBuf, FileId>,
    next_file_id: &mut FileId,
    synthetic: &mut u64,
    parse_time: &mut Duration,
    parse_cache: &mut ParseCache,
    options: &LoadOptions,
) -> Option<Program> {
    let mut ctx = CrateCtx {
        src_root: src_root.to_path_buf(),
        display_base: package_root.to_path_buf(),
        display_prefix: String::new(),
        sources,
        diags,
        file_ids,
        loaded: BTreeSet::new(),
        synthetic,
        parse_time,
        next_file_id,
        parse_cache,
        overrides: &options.overrides,
    };
    let module = load_crate_root(&mut ctx, root_file)?;
    let file = module.span.file;
    let mut program = Program { file, root: module };
    merge_dependencies(&mut program, graph, &mut ctx);
    Some(program)
}

pub(super) fn regular_file(path: &Path, diags: &mut DiagnosticBag) -> Result<bool, ()> {
    match symlink_metadata_optional(path) {
        Ok(Some(meta)) if meta.file_type().is_symlink() => {
            diags.push(Diagnostic::error(
                "E-MOD-PATH",
                Span::dummy(),
                format!(
                    "`{}` is a symbolic link; crate roots and modules are never followed",
                    path.display()
                ),
            ));
            Err(())
        }
        Ok(Some(meta)) if meta.is_file() => Ok(true),
        Ok(Some(_)) => {
            diags.push(Diagnostic::error(
                "E-IO",
                Span::dummy(),
                format!("`{}` is not a regular file", path.display()),
            ));
            Err(())
        }
        Ok(None) => Ok(false),
        Err(e) => {
            diags.push(Diagnostic::error(
                "E-IO",
                Span::dummy(),
                format!("cannot inspect `{}`: {e}", path.display()),
            ));
            Err(())
        }
    }
}

pub(super) fn load_crate_root(ctx: &mut CrateCtx<'_>, root_file: &Path) -> Option<Module> {
    if !check_lexical_no_symlinks(&ctx.src_root, root_file, ctx.diags) {
        return None;
    }
    let canonical = match fsutil::canonicalize(root_file) {
        Ok(p) => p,
        Err(e) => {
            err(
                ctx.diags,
                "E-IO",
                Span::dummy(),
                format!("cannot resolve `{}`: {e}", root_file.display()),
            );
            return None;
        }
    };
    if !path_is_under(&canonical, &ctx.src_root) {
        err(
            ctx.diags,
            "E-MOD-PATH-ESCAPE",
            Span::dummy(),
            format!(
                "crate root `{}` is outside the package `src/` directory",
                canonical.display()
            ),
        );
        return None;
    }
    check_path_no_symlinks(&ctx.src_root, &canonical, ctx.diags)?;
    ctx.loaded.insert(canonical.clone());
    load_file_module(ctx, &canonical, None)
}

pub(super) fn load_file_module(
    ctx: &mut CrateCtx<'_>,
    canonical: &Path,
    name: Option<Ident>,
) -> Option<Module> {
    let file_id = ensure_source(ctx, canonical)?;
    let text = ctx.sources.file(file_id)?.text.clone();
    let key = (content_hash(text.as_bytes()), file_id);
    let (program, parsed_diags) = match ctx.parse_cache.get(&key) {
        Some((program, diags)) => (program, diags),
        None => {
            let parse_started = Instant::now();
            let parsed = parse_file(file_id, &text);
            *ctx.parse_time += parse_started.elapsed();
            let diags = parsed.diags.into_vec();
            ctx.parse_cache
                .insert(key, parsed.program.clone(), diags.clone());
            (parsed.program, diags)
        }
    };
    for diagnostic in parsed_diags {
        ctx.diags.push(diagnostic);
    }
    let mut module = program.root;
    module.name = name;
    let file_dir = canonical
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| ctx.src_root.clone());
    expand_module(ctx, &mut module, &file_dir, Path::new(""));
    Some(module)
}

pub(super) fn expand_module(
    ctx: &mut CrateCtx<'_>,
    module: &mut Module,
    file_dir: &Path,
    inline_prefix: &Path,
) {
    let mut kept: Vec<Item> = Vec::with_capacity(module.items.len());
    for mut item in std::mem::take(&mut module.items) {
        match item.kind {
            ItemKind::Mod(mut inner) => {
                let inline_name = inner
                    .name
                    .as_ref()
                    .map(|ident| ident.name.clone())
                    .unwrap_or_default();
                let next_prefix = if inline_name.is_empty() {
                    inline_prefix.to_path_buf()
                } else {
                    inline_prefix.join(&inline_name)
                };
                expand_module(ctx, &mut inner, file_dir, &next_prefix);
                item.kind = ItemKind::Mod(inner);
                kept.push(item);
            }
            ItemKind::ModDecl(decl) => {
                if let Some(mut loaded) = resolve_module_file(ctx, &decl, file_dir, inline_prefix) {
                    loaded.name = Some(decl.name.clone());
                    item.kind = ItemKind::Mod(loaded);
                    kept.push(item);
                }
            }
            other => {
                item.kind = other;
                kept.push(item);
            }
        }
    }
    module.items = kept;
}

pub(super) fn resolve_module_file(
    ctx: &mut CrateCtx<'_>,
    decl: &ravum_lang_syntax::ast::ModDecl,
    file_dir: &Path,
    inline_prefix: &Path,
) -> Option<Module> {
    let name = &decl.name.name;
    if is_reserved_windows_component(name) {
        err(
            ctx.diags,
            "E-MOD-RESERVED-NAME",
            decl.name.span,
            format!(
                "module `{name}` is a reserved Windows path component; module names must be \
                 portable (CON, PRN, AUX, NUL, COM1-9, LPT1-9, trailing dot/space)"
            ),
        );
        return None;
    }
    // The prefix is built by `expand_module` from inline module names only, so
    // every component is a plain name.
    for part in inline_prefix.iter() {
        let part = part.to_string_lossy();
        if is_reserved_windows_component(&part) {
            err(
                ctx.diags,
                "E-MOD-RESERVED-NAME",
                decl.name.span,
                format!(
                    "inline module path component `{part}` is a reserved Windows path \
                     component"
                ),
            );
            return None;
        }
    }
    let dir = file_dir.join(inline_prefix);
    let candidate_file = dir.join(format!("{name}.rv"));
    let candidate_dir = dir.join(name).join("mod.rv");

    let file_meta = match symlink_metadata_optional(&candidate_file) {
        Ok(meta) => meta,
        Err(e) => {
            err(
                ctx.diags,
                "E-IO",
                decl.name.span,
                format!("cannot inspect `{}`: {e}", candidate_file.display()),
            );
            return None;
        }
    };
    let dir_meta = match symlink_metadata_optional(&candidate_dir) {
        Ok(meta) => meta,
        Err(e) => {
            err(
                ctx.diags,
                "E-IO",
                decl.name.span,
                format!("cannot inspect `{}`: {e}", candidate_dir.display()),
            );
            return None;
        }
    };

    // MOD-2.6, MOD-1.6: a module name matches its file name exactly. On a
    // case-insensitive file system `src/foo.rv` answers for `mod Foo;`; the
    // directory listing is the only place where the stored name is visible,
    // so a candidate counts only if an entry carries exactly that name.
    let file_meta = file_meta.filter(|_| has_exact_entry(&dir, &format!("{name}.rv")));
    let dir_meta = dir_meta
        .filter(|_| has_exact_entry(&dir, name) && has_exact_entry(&dir.join(name), "mod.rv"));

    if file_meta
        .as_ref()
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
        || dir_meta
            .as_ref()
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
    {
        err(
            ctx.diags,
            "E-MOD-PATH",
            decl.name.span,
            format!(
                "module `{name}` resolves through a symbolic link; symlinks are refused for \
                 module resolution"
            ),
        );
        return None;
    }

    let is_file = file_meta.map(|m| m.is_file()).unwrap_or(false);
    let is_dir_file = dir_meta.map(|m| m.is_file()).unwrap_or(false);
    let candidate = match (is_file, is_dir_file) {
        (true, true) => {
            err(
                ctx.diags,
                "E-MOD-AMBIGUOUS",
                decl.name.span,
                format!(
                    "module `{name}` is ambiguous: both `{}` and `{}` exist",
                    candidate_file.display(),
                    candidate_dir.display()
                ),
            );
            return None;
        }
        (true, false) => candidate_file,
        (false, true) => candidate_dir,
        (false, false) => {
            err(
                ctx.diags,
                "E-MOD-MISSING",
                decl.name.span,
                format!(
                    "module `{name}` not found: neither `{}` nor `{}` exists",
                    candidate_file.display(),
                    candidate_dir.display()
                ),
            );
            return None;
        }
    };

    if !check_lexical_no_symlinks(&ctx.src_root, &candidate, ctx.diags) {
        return None;
    }
    let canonical = match fsutil::canonicalize(&candidate) {
        Ok(p) => p,
        Err(e) => {
            err(
                ctx.diags,
                "E-MOD-MISSING",
                decl.name.span,
                format!(
                    "cannot resolve module `{name}` (`{}`): {e}",
                    candidate.display()
                ),
            );
            return None;
        }
    };
    if !path_is_under(&canonical, &ctx.src_root) {
        err(
            ctx.diags,
            "E-MOD-PATH-ESCAPE",
            decl.name.span,
            format!(
                "module `{name}` resolves outside the package `src/` directory (`{}`)",
                canonical.display()
            ),
        );
        return None;
    }
    check_path_no_symlinks(&ctx.src_root, &canonical, ctx.diags)?;
    if ctx.loaded.contains(&canonical) {
        err(
            ctx.diags,
            "E-MOD-DUPLICATE",
            decl.name.span,
            format!(
                "module file `{}` is included more than once in this crate",
                display_relative(&canonical, &ctx.display_base)
            ),
        );
        return None;
    }
    ctx.loaded.insert(canonical.clone());
    load_file_module(ctx, &canonical, Some(decl.name.clone()))
}

pub(super) fn ensure_source(ctx: &mut CrateCtx<'_>, canonical: &Path) -> Option<FileId> {
    let display = format!(
        "{}{}",
        ctx.display_prefix,
        display_relative(canonical, &ctx.display_base)
    );
    if let Some(id) = ctx.file_ids.get(canonical).copied() {
        match ctx.sources.file(id) {
            Some(file) if file.path == display => return Some(id),
            Some(file) => {
                let taken = file.path.clone();
                err(
                    ctx.diags,
                    "E-IO",
                    Span::dummy(),
                    format!(
                        "session file id {id} is already used by `{taken}`; the session file \
                         table is inconsistent"
                    ),
                );
                return None;
            }
            // A session entry for a path this load has not registered yet: the
            // file is read below and registered under the same id.
            None => {}
        }
    }
    if ctx.sources.len() >= source_file_limit() {
        let limit = source_file_limit();
        err(
            ctx.diags,
            "E-LIMIT-FILES",
            Span::dummy(),
            format!(
                "more than {limit} source files were loaded; the parser's NodeId \
                 reserves 32 bits for the FileId"
            ),
        );
        return None;
    }
    let text = match ctx.overrides.get(canonical) {
        // Editor buffer not saved yet: the overlay is analysed in place, never
        // written. The same size limit applies as for a real file so an
        // adversarial buffer cannot exhaust memory.
        Some(text) => {
            if text.len() as u64 > MAX_FILE_SIZE {
                err(
                    ctx.diags,
                    "E-LIMIT-FILE-SIZE",
                    Span::dummy(),
                    format!(
                        "`{}` is {} bytes, above the {} byte limit",
                        canonical.display(),
                        text.len(),
                        MAX_FILE_SIZE
                    ),
                );
                return None;
            }
            text.clone()
        }
        None => read_text_limited(canonical, MAX_FILE_SIZE, "E-IO", ctx.diags)?,
    };
    register_source(
        ctx.sources,
        ctx.file_ids,
        ctx.next_file_id,
        canonical,
        display,
        text,
        ctx.diags,
    )
}

/// Next id a session table has never handed out.
pub(super) fn next_free_file_id(table: &BTreeMap<PathBuf, FileId>) -> FileId {
    table
        .values()
        .copied()
        .max()
        .map_or(0, |id| id.saturating_add(1))
}

/// Register `text` for `canonical` under the id the session table already chose,
/// or under the next free id. See [`LoadOptions::file_ids`].
///
/// A table that maps one id to two files is a caller bug, not user input: the
/// loader fails closed with `E-IO` instead of silently reusing the id, because
/// every span already parsed for that id would point into the wrong file.
#[allow(clippy::too_many_arguments)]
pub(super) fn register_source(
    sources: &mut SourceMap,
    file_ids: &mut BTreeMap<PathBuf, FileId>,
    next: &mut FileId,
    canonical: &Path,
    display: String,
    text: String,
    diags: &mut DiagnosticBag,
) -> Option<FileId> {
    let id = match file_ids.get(canonical).copied() {
        Some(id) => {
            if let Some(file) = sources.file(id) {
                if file.path == display {
                    return Some(id);
                }
                let taken = file.path.clone();
                err(
                    diags,
                    "E-IO",
                    Span::dummy(),
                    format!(
                        "session file id {id} is already used by `{taken}`; the session file \
                         table is inconsistent"
                    ),
                );
                return None;
            }
            id
        }
        None => {
            let id = *next;
            *next = next.saturating_add(1);
            id
        }
    };
    if sources.add_at(id, display, text).is_none() {
        err(
            diags,
            "E-IO",
            Span::dummy(),
            format!(
                "session file id {id} is already registered; the session file table is \
                 inconsistent"
            ),
        );
        return None;
    }
    file_ids.insert(canonical.to_path_buf(), id);
    Some(id)
}

/// Merge every resolved dependency (direct and transitive) into the crate root
/// as a public module named after the package, so `use util::helper;` resolves.
///
/// This is the encoding allowed by the frozen `sema::check(program, sources,
/// options)` interface: the checker only sees one `Program`. Cross-package
/// *visibility* rules (only declared dependencies are importable) remain the
/// checker's responsibility; the loader provides the definitions.
pub(super) fn merge_dependencies(program: &mut Program, graph: &DepGraph, ctx: &mut CrateCtx<'_>) {
    let mut packages: Vec<&GraphPackage> = graph.packages.iter().skip(1).collect();
    packages.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));

    let local_names: BTreeSet<String> = program
        .root
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Mod(module) => module.name.as_ref().map(|n| n.name.clone()),
            _ => None,
        })
        .collect();

    for package in packages {
        let name = package.manifest.name.clone();
        if local_names.contains(&name) {
            err(
                ctx.diags,
                "E-MOD-DUPLICATE",
                Span::dummy(),
                format!(
                    "dependency `{name}` collides with a local module of the same name; \
                     rename one of them"
                ),
            );
            continue;
        }
        let lib_root = package.dir.join("src").join("lib.rv");
        let dep_src = package.dir.join("src");
        let mut dep_ctx = CrateCtx {
            src_root: dep_src.clone(),
            display_base: package.dir.clone(),
            display_prefix: format!("{name}/"),
            sources: &mut *ctx.sources,
            diags: &mut *ctx.diags,
            file_ids: &mut *ctx.file_ids,
            loaded: BTreeSet::new(),
            synthetic: &mut *ctx.synthetic,
            parse_time: &mut *ctx.parse_time,
            next_file_id: &mut *ctx.next_file_id,
            parse_cache: &mut *ctx.parse_cache,
            overrides: ctx.overrides,
        };
        let module = load_crate_root(&mut dep_ctx, &lib_root);
        if let Some(module) = module {
            let item_id = next_synthetic(dep_ctx.synthetic);
            let module_id = next_synthetic(dep_ctx.synthetic);
            let mut module = module;
            module.id = module_id;
            module.name = Some(Ident::new(name.clone(), Span::dummy()));
            program.root.items.push(Item {
                id: item_id,
                vis: Vis::Public(Span::dummy()),
                kind: ItemKind::Mod(module),
                span: Span::dummy(),
            });
        }
    }
}
