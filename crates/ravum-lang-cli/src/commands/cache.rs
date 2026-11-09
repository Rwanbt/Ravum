//! `ravum-lang cache status|clean` — inspect or remove `<package>/.ravum/`.
//!
//! The cache is entirely derived data; `clean` removes only that directory and
//! refuses symlinks. There is no automatic GC in Core 1.0: regeneration is
//! bounded by the package sources and `clean` is the documented eviction.

use std::fs;
use std::path::{Path, PathBuf};

use crate::args::CacheAction;
use crate::util;

pub fn run(action: CacheAction, path: &Path) -> i32 {
    let root = match locate_root(path) {
        Ok(root) => root,
        Err(message) => {
            util::err(message);
            return util::EXIT_FAIL;
        }
    };
    match action {
        CacheAction::Status => status(&root),
        CacheAction::Clean => clean(&root),
    }
}

fn locate_root(path: &Path) -> Result<PathBuf, String> {
    let canonical = fs::canonicalize(path)
        .map_err(|error| format!("cannot resolve `{}`: {error}", util::pretty_path(path)))?;
    let base = if canonical.is_file() {
        canonical
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or(canonical.clone())
    } else {
        canonical
    };
    Ok(util::find_package_root(&base))
}

fn cache_path(root: &Path) -> PathBuf {
    root.join(util::CACHE_DIR)
}

fn status(root: &Path) -> i32 {
    let cache = cache_path(root);
    println!("cache: {}", util::pretty_path(&cache));
    match fs::symlink_metadata(&cache) {
        Ok(meta) if meta.file_type().is_symlink() => {
            util::err(format!(
                "`{}` is a symbolic link; refusing to inspect it",
                util::pretty_path(&cache)
            ));
            return util::EXIT_FAIL;
        }
        Ok(meta) if !meta.is_dir() => {
            util::err(format!("`{}` is not a directory", util::pretty_path(&cache)));
            return util::EXIT_FAIL;
        }
        Ok(_) => {
            let stats = match util::dir_stats(&cache) {
                Ok(stats) => stats,
                Err(error) => {
                    util::err(format!("cannot measure `{}`: {error}", util::pretty_path(&cache)));
                    return util::EXIT_FAIL;
                }
            };
            let entries = match util::cache_entries(&cache) {
                Ok(entries) => entries,
                Err(message) => {
                    util::err(message);
                    return util::EXIT_FAIL;
                }
            };
            println!("state: present");
            println!(
                "size: {} bytes ({} file(s), {} directorie(s))",
                stats.bytes, stats.files, stats.dirs
            );
            for (name, size, is_dir) in entries {
                println!(
                    "  {} {} ({} bytes)",
                    if is_dir { "dir " } else { "file" },
                    name,
                    size
                );
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            println!("state: absent");
        }
        Err(error) => {
            util::err(format!("cannot inspect `{}`: {error}", util::pretty_path(&cache)));
            return util::EXIT_FAIL;
        }
    }
    println!("gc: manual (no automatic eviction in Core 1.0; `cache clean` removes the cache)");
    util::EXIT_OK
}

fn clean(root: &Path) -> i32 {
    let cache = cache_path(root);
    let meta = match fs::symlink_metadata(&cache) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            println!("cache: nothing to clean ({})", util::pretty_path(&cache));
            return util::EXIT_OK;
        }
        Err(error) => {
            util::err(format!("cannot inspect `{}`: {error}", util::pretty_path(&cache)));
            return util::EXIT_FAIL;
        }
    };
    if meta.file_type().is_symlink() {
        util::err(format!(
            "`{}` is a symbolic link; refusing to delete it (remove it manually after inspection)",
            util::pretty_path(&cache)
        ));
        return util::EXIT_FAIL;
    }
    if !meta.is_dir() {
        util::err(format!(
            "`{}` is not a directory; refusing to delete it",
            util::pretty_path(&cache)
        ));
        return util::EXIT_FAIL;
    }

    // Belt and braces: the resolved cache must be exactly `<root>/.ravum`.
    let canonical_root = match fs::canonicalize(root) {
        Ok(path) => path,
        Err(error) => {
            util::err(format!("cannot resolve `{}`: {error}", util::pretty_path(root)));
            return util::EXIT_FAIL;
        }
    };
    let canonical_cache = match fs::canonicalize(&cache) {
        Ok(path) => path,
        Err(error) => {
            util::err(format!("cannot resolve `{}`: {error}", util::pretty_path(&cache)));
            return util::EXIT_FAIL;
        }
    };
    if canonical_cache.parent() != Some(canonical_root.as_path())
        || canonical_cache.file_name().and_then(|name| name.to_str())
            != Some(util::CACHE_DIR)
    {
        util::err(format!(
            "`{}` is not the expected `<package>/.ravum` directory; refusing to delete it",
            util::pretty_path(&cache)
        ));
        return util::EXIT_FAIL;
    }

    let size = util::dir_stats(&cache).map(|stats| stats.bytes).unwrap_or(0);
    if let Err(error) = fs::remove_dir_all(&cache) {
        util::err(format!("cannot remove `{}`: {error}", util::pretty_path(&cache)));
        return util::EXIT_FAIL;
    }
    println!(
        "removed cache: {} ({} bytes freed)",
        util::pretty_path(&cache),
        size
    );
    util::EXIT_OK
}
