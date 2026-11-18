//! Machine paths out of loader messages (DIAG-6.8, DIAG-7.5).
//!
//! The loader works on absolute, canonical paths, and many of its messages
//! name a file. Before the diagnostics leave the loader, every occurrence of
//! the package root, or of one of its ancestors, is rewritten relative to the
//! package root: `/home/u/app/src/b.rv` becomes `src/b.rv`, and a sibling
//! dependency `/home/u/lib/src/lib.rv` becomes `../lib/src/lib.rv`. The
//! filesystem root itself is never rewritten, so a path with no common
//! ancestor but `/` stays as it is.

use std::path::{Path, PathBuf};

pub(super) struct Relativizer {
    /// `(prefix, replacement)`, longest prefix first.
    rules: Vec<(String, String)>,
}

impl Relativizer {
    pub(super) fn new(base: &Path) -> Self {
        let mut bases: Vec<PathBuf> = Vec::new();
        let absolute = if base.is_absolute() {
            base.to_path_buf()
        } else {
            std::env::current_dir()
                .map(|dir| dir.join(base))
                .unwrap_or_else(|_| base.to_path_buf())
        };
        bases.push(absolute);
        if let Ok(canonical) = std::fs::canonicalize(base) {
            bases.push(canonical);
        }
        let mut rules = Vec::new();
        for base in &bases {
            for (depth, ancestor) in base.ancestors().enumerate() {
                if ancestor.parent().is_none() {
                    break; // never rewrite `/` or a drive root
                }
                let up = if depth == 0 {
                    String::new()
                } else {
                    "../".repeat(depth)
                };
                for shown in spellings(ancestor) {
                    let here = if depth == 0 {
                        ".".to_string()
                    } else {
                        up.trim_end_matches('/').to_string()
                    };
                    rules.push((format!("{shown}{}", std::path::MAIN_SEPARATOR), up.clone()));
                    rules.push((format!("{shown}/"), up.clone()));
                    rules.push((shown, here));
                }
            }
        }
        rules.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
        rules.dedup();
        Self { rules }
    }

    pub(super) fn apply(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        'outer: while !rest.is_empty() {
            for (prefix, replacement) in &self.rules {
                if !prefix.is_empty() && rest.starts_with(prefix.as_str()) {
                    out.push_str(replacement);
                    rest = &rest[prefix.len()..];
                    continue 'outer;
                }
            }
            let next = rest.chars().next().unwrap();
            out.push(next);
            rest = &rest[next.len_utf8()..];
        }
        out
    }
}

/// The ways a path is written in a message: as `Path::display` writes it,
/// and, on Windows, without the `\\?\` verbatim prefix of a canonical path.
fn spellings(path: &Path) -> Vec<String> {
    let shown = path.display().to_string();
    let mut all = vec![shown.clone()];
    if let Some(plain) = shown.strip_prefix(r"\\?\") {
        all.push(plain.to_string());
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_package_root_and_its_ancestors_become_relative() {
        let root = std::env::temp_dir().join("ravum-relativize").join("app");
        let r = Relativizer::new(&root);
        let shown = |p: PathBuf| p.display().to_string();
        assert_eq!(
            r.apply(&format!(
                "`{}` missing",
                shown(root.join("src").join("b.rv"))
            )),
            format!("`src{}b.rv` missing", std::path::MAIN_SEPARATOR)
        );
        let sibling = root.parent().unwrap().join("lib");
        assert_eq!(r.apply(&format!("in `{}`", shown(sibling))), "in `../lib`");
        assert_eq!(r.apply(&format!("at `{}`", shown(root.clone()))), "at `.`");
        assert_eq!(r.apply("no path here"), "no path here");
    }

    #[test]
    fn the_filesystem_root_is_never_rewritten() {
        // Built from real absolute paths so that the test holds on Windows
        // (`C:\...`) as on POSIX (`/...`).
        let base = std::env::temp_dir().join("ravum-root-check").join("b");
        let r = Relativizer::new(&base);
        let root = base.ancestors().last().unwrap().to_path_buf();
        let elsewhere = root.join("zz-ravum-elsewhere").join("hosts");
        let shown = elsewhere.display().to_string();
        assert_eq!(r.apply(&format!("`{shown}`")), format!("`{shown}`"));
        let sibling = base.parent().unwrap().join("x");
        assert_eq!(r.apply(&format!("`{}`", sibling.display())), "`../x`");
    }

    #[test]
    fn a_verbatim_windows_prefix_is_also_recognised() {
        let shown = spellings(Path::new(r"\\?\C:\work\app"));
        assert_eq!(shown, [r"\\?\C:\work\app", r"C:\work\app"]);
    }

    #[test]
    fn a_relative_base_is_resolved_against_the_current_directory() {
        let r = Relativizer::new(Path::new("pkg"));
        let file = std::env::current_dir()
            .unwrap()
            .join("pkg")
            .join("ravum.toml");
        assert_eq!(r.apply(&file.display().to_string()), "ravum.toml");
    }
}
