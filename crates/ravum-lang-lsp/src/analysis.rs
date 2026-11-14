//! Package analysis for the language server.
//!
//! One [`Analysis`] is produced per package root. It reuses the *same* frozen
//! frontend the CLI uses (`sema::loader` -> `sema::check`), so an editor
//! diagnostic is by construction the same diagnostic `ravum-lang check` prints
//! for the same source text.
//!
//! Unsaved buffers are analysed through `LoadOptions::overrides`: the text lives
//! in memory, the file on disk is never written, and no temporary file is
//! created. A buffer that is not part of a loaded package simply has no
//! semantic diagnostics (its syntax diagnostics are still produced by the
//! loader, and reported).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ravum_lang_sema::facts::{CheckOptions, CheckedProgram};
use ravum_lang_sema::loader::{
    load_package_with_parse_cache, LoadOptions, LoadedPackage, ParseCache,
};
use ravum_lang_sema::{check, Profile};
use ravum_lang_syntax::diag::Diagnostic;
use ravum_lang_syntax::source::SourceMap;
use ravum_lang_syntax::span::FileId;

use crate::walk::AstIndex;

pub const MANIFEST: &str = "ravum.toml";

pub struct Analysis {
    pub root: PathBuf,
    pub package: LoadedPackage,
    pub checked: Option<CheckedProgram>,
    pub diags: Vec<Diagnostic>,
    pub index: AstIndex,
}

impl Analysis {
    pub fn run(root: &Path, overrides: BTreeMap<PathBuf, String>, profile: Profile) -> Self {
        let mut cache = ParseCache::default();
        Self::run_with_cache(root, overrides, profile, &mut cache)
    }

    /// `run`, reusing the parses stored in `cache` for files whose content (and
    /// `FileId`) did not change. The semantic check still runs globally over the
    /// whole package: only the parser is skipped.
    pub fn run_with_cache(
        root: &Path,
        overrides: BTreeMap<PathBuf, String>,
        profile: Profile,
        cache: &mut ParseCache,
    ) -> Self {
        Self::analyse(
            root,
            &LoadOptions {
                locked: false,
                overrides,
                file_ids: None,
            },
            profile,
            cache,
        )
    }

    /// `run_with_cache`, with the append-only `canonical path -> FileId` table
    /// of one language-server session (T5).
    ///
    /// Every `NodeId` embeds the `FileId`, so a file the user did not touch must
    /// keep its id when another file starts or stops being part of the package
    /// (a new `mod` declared before the existing ones, a removed module). The
    /// loader reuses the ids found in `file_ids` and returns the table it built
    /// in [`ravum_lang_sema::loader::LoadedPackage::file_ids`]; this function
    /// merges it back before returning, so the caller only has to keep the map
    /// alive across analyses. A plain [`Self::run_with_cache`] call (tests, CLI
    /// path) keeps the historical dense ids and is not affected.
    pub fn run_with_cache_and_file_ids(
        root: &Path,
        overrides: BTreeMap<PathBuf, String>,
        profile: Profile,
        cache: &mut ParseCache,
        file_ids: &mut BTreeMap<PathBuf, FileId>,
    ) -> Self {
        let options = LoadOptions {
            locked: false,
            overrides,
            file_ids: Some(file_ids.clone()),
        };
        let analysis = Self::analyse(root, &options, profile, cache);
        for (path, id) in &analysis.package.file_ids {
            file_ids.insert(path.clone(), *id);
        }
        analysis
    }

    fn analyse(
        root: &Path,
        options: &LoadOptions,
        profile: Profile,
        cache: &mut ParseCache,
    ) -> Self {
        let mut package = load_package_with_parse_cache(root, options, cache);
        let mut diags = std::mem::take(&mut package.diags);
        let mut checked = None;

        if !diags.has_errors() {
            if let Some(program) = &package.program {
                let check_options = CheckOptions {
                    profile,
                    edition: ravum_lang_syntax::LANGUAGE_EDITION.to_string(),
                };
                let result = check(program, &package.sources, &check_options);
                diags.extend(result.diags);
                checked = result.checked;
            }
        }

        let index = match &checked {
            Some(checked) => AstIndex::build(&checked.program),
            None => match &package.program {
                Some(program) => AstIndex::build(program),
                None => AstIndex::default(),
            },
        };

        let mut collected = diags.into_vec();
        collected.sort_by(|a, b| {
            (a.span.file, a.span.start, a.span.end, &a.code).cmp(&(
                b.span.file,
                b.span.start,
                b.span.end,
                &b.code,
            ))
        });
        Self {
            root: root.to_path_buf(),
            package,
            checked,
            diags: collected,
            index,
        }
    }

    pub fn sources(&self) -> &SourceMap {
        &self.package.sources
    }

    /// Diagnostics grouped by the display path of the file they point into.
    pub fn by_file(&self) -> BTreeMap<String, Vec<&Diagnostic>> {
        let mut grouped: BTreeMap<String, Vec<&Diagnostic>> = BTreeMap::new();
        for diagnostic in &self.diags {
            let Some(file) = self.package.sources.file(diagnostic.span.file) else {
                continue;
            };
            grouped
                .entry(file.path.clone())
                .or_default()
                .push(diagnostic);
        }
        grouped
    }
}

/// Package root for a document: the closest ancestor containing `ravum.toml`,
/// otherwise the file itself (single-file mode).
pub fn package_root(document: &Path) -> PathBuf {
    let mut current = document.parent();
    while let Some(directory) = current {
        if directory.join(MANIFEST).is_file() {
            return directory.to_path_buf();
        }
        current = directory.parent();
    }
    document.to_path_buf()
}

/// Path a file must have in the loader's override map: the canonical path when
/// the file exists (the loader canonicalizes), else the absolute path.
pub fn override_key(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}
