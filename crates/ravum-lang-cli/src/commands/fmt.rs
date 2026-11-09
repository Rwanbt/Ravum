//! `ravum-lang fmt` — format every `.rv` file of a package (or one file).
//!
//! `--check` never writes and exits 1 when a file differs. Writes are atomic.

use ravum_lang_syntax::diag::DiagnosticBag;
use ravum_lang_syntax::fmt::format_source;
use ravum_lang_syntax::source::SourceMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::util;

pub fn run(path: &Path, check: bool) -> i32 {
    let root = match resolve_root(path) {
        Ok(root) => root,
        Err(message) => {
            util::err(message);
            return util::EXIT_FAIL;
        }
    };
    let files = match collect_files(path) {
        Ok(files) => files,
        Err(message) => {
            util::err(message);
            return util::EXIT_FAIL;
        }
    };

    let mut sources = SourceMap::new();
    let mut diags = DiagnosticBag::new();
    let mut changed: Vec<PathBuf> = Vec::new();
    let mut failures = 0usize;

    for file in &files {
        let text = match util::read_text(file) {
            Ok(text) => text,
            Err(message) => {
                util::err(message);
                failures += 1;
                continue;
            }
        };
        let display = util::display_path(file, &root);
        let file_id = sources.add(display, text.clone());
        match format_source(file_id, &text) {
            Ok(formatted) => {
                if formatted != text {
                    changed.push(file.clone());
                    if !check {
                        if let Err(message) = util::write_atomic(file, &formatted) {
                            util::err(message);
                            failures += 1;
                        }
                    }
                }
            }
            Err(bag) => {
                diags.extend(bag);
            }
        }
    }

    if !diags.is_empty() {
        eprint!("{}", diags.render(&sources));
    }
    let error_count = diags.error_count() + failures;
    let mode = if check { "would reformat" } else { "reformatted" };
    for file in &changed {
        println!("{mode} {}", util::pretty_path(file));
    }
    println!(
        "fmt: {} file(s) checked, {} {}",
        files.len(),
        changed.len(),
        if check { "differ" } else { "written" }
    );

    if error_count > 0 || (check && !changed.is_empty()) {
        util::EXIT_FAIL
    } else {
        util::EXIT_OK
    }
}

fn resolve_root(path: &Path) -> Result<PathBuf, String> {
    let canonical = fs::canonicalize(path)
        .map_err(|error| format!("cannot resolve `{}`: {error}", util::pretty_path(path)))?;
    if canonical.is_file() {
        return Ok(canonical
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(".")));
    }
    if canonical.is_dir() {
        return Ok(canonical);
    }
    Err(format!(
        "`{}` is neither a file nor a directory",
        util::pretty_path(path)
    ))
}

fn collect_files(path: &Path) -> Result<Vec<PathBuf>, String> {
    let canonical = fs::canonicalize(path)
        .map_err(|error| format!("cannot resolve `{}`: {error}", util::pretty_path(path)))?;
    if canonical.is_file() {
        if canonical.extension().and_then(|extension| extension.to_str()) != Some("rv") {
            return Err(format!("`{}` is not a `.rv` file", util::pretty_path(path)));
        }
        return Ok(vec![canonical]);
    }
    let src = canonical.join("src");
    if !src.is_dir() {
        return Err(format!(
            "`{}` has no `src/` directory; nothing to format",
            util::pretty_path(&canonical)
        ));
    }
    util::collect_rv_files(&src)
}
