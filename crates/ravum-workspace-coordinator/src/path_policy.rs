//! Path policy shared by the coordinator and the single Git writer.
//!
//! Two independent questions, deliberately kept apart:
//!
//! * **safety** — may this path be touched at all? Repository metadata (`.git`),
//!   absolute paths, drive/UNC prefixes, `..` traversal and control characters are
//!   never written, whatever a proposal or a caller claims. This is the guard that
//!   makes "an agent tried to write `.git/config`" a refusal instead of a silent
//!   repository corruption.
//! * **auto-integration** — is the content generated or externally owned
//!   (lockfiles, minified/generated artifacts)? Such a path is never overwritten
//!   silently: the coordinator classifies it `MANUAL_REQUIRED`, and the Git writer
//!   refuses it unless the caller makes an explicit opt-in decision.
//!
//! Both answers are pure functions of the path: no filesystem access, no Git
//! invocation, so the same policy can be applied before any state is created.

/// Paths whose content is generated or externally owned: never auto-integrated.
const GENERATED_PATTERNS: [&str; 6] = [
    "Cargo.lock",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    ".generated.",
    ".min.",
];

/// True when `path` names generated, minified or lockfile content.
///
/// The test is intentionally conservative: a false positive costs a manual
/// decision, a false negative would let a tool-generated file be overwritten.
pub fn is_generated_path(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    GENERATED_PATTERNS
        .iter()
        .any(|p| name == *p || name.contains(p) || name.ends_with(".lock"))
}

/// Canonical repository-relative form: forward slashes, no `./` prefix.
///
/// This is the same convention as `ravum_snapshot::canonical_path`, re-stated here
/// because the writer must not depend on the snapshot crate for a safety check.
pub fn canonical_relative_path(path: &str) -> String {
    let mut p = path.replace('\\', "/");
    while let Some(stripped) = p.strip_prefix("./") {
        p = stripped.to_string();
    }
    p
}

/// Why a path cannot be written, in a form suitable for a refusal reason.
///
/// Returns `None` when the path is a safe repository-relative path; otherwise the
/// canonical form is still produced so the caller can report exactly what it
/// would have written.
pub fn unsafe_path_reason(raw: &str) -> Option<String> {
    if raw.is_empty() {
        return Some("empty path".to_owned());
    }
    if raw.chars().any(|c| c == '\0' || (c as u32) < 0x20) {
        return Some("path contains a control character".to_owned());
    }
    let path = canonical_relative_path(raw);
    if path.is_empty() || path == "." {
        return Some("path is the workspace root".to_owned());
    }
    if path.starts_with('/') {
        return Some("absolute path".to_owned());
    }
    // Windows drive prefix (`C:`) and any `:` inside a component: git only
    // accepts `:` in a path on POSIX, and `rev:path` syntax would make the path
    // ambiguous in plumbing commands.
    if path.contains(':') {
        return Some("path contains ':' (drive prefix or ambiguous revision syntax)".to_owned());
    }
    if path.ends_with('/') {
        return Some("path names a directory".to_owned());
    }
    let mut components = 0usize;
    for part in path.split('/') {
        match part {
            "" => return Some("path contains an empty component".to_owned()),
            "." | ".." => {
                return Some(format!(
                    "path escapes the workspace with component {part:?}"
                ))
            }
            _ => {}
        }
        if part.eq_ignore_ascii_case(".git") {
            return Some("path is inside the Git metadata directory".to_owned());
        }
        components += 1;
    }
    if components == 0 {
        return Some("path has no component".to_owned());
    }
    None
}

/// Validate and return the canonical form of a safe repository-relative path.
pub fn validate_relative_path(raw: &str) -> Result<String, String> {
    match unsafe_path_reason(raw) {
        None => Ok(canonical_relative_path(raw)),
        Some(reason) => Err(reason),
    }
}

/// Allowed forms for a publication reference: `HEAD` or a local branch.
///
/// The writer resolves the reference itself; this guard exists so a caller cannot
/// turn the compare-and-swap primitive into an arbitrary ref write.
pub fn validate_reference(reference: &str) -> Result<(), String> {
    if reference == "HEAD" {
        return Ok(());
    }
    let Some(rest) = reference.strip_prefix("refs/heads/") else {
        return Err("reference must be HEAD or refs/heads/<branch>".to_owned());
    };
    if rest.is_empty()
        || rest.starts_with('/')
        || rest.ends_with('/')
        || rest.ends_with('.')
        || rest.ends_with(".lock")
        || rest.contains("..")
        || rest.contains("//")
        || rest.contains("@{")
        || rest.chars().any(|c| {
            matches!(
                c,
                '~' | '^' | ':' | '?' | '*' | '[' | '\\' | ' ' | '\t' | '\0'
            )
        })
        || rest.chars().any(|c| (c as u32) < 0x20)
    {
        return Err(format!("unsafe branch name {reference:?}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_metadata_and_traversal_are_refused() {
        for raw in [
            ".git",
            ".git/config",
            ".git/hooks/pre-commit",
            ".GIT/config",
            "sub/.git/config",
            "../outside.rs",
            "a/../../outside.rs",
            "/abs.rs",
            "C:/abs.rs",
            "a/./b.rs",
            "a//b.rs",
            "trailing/",
            "with\0nul.rs",
            "",
            ".",
        ] {
            assert!(
                unsafe_path_reason(raw).is_some(),
                "path {raw:?} must be refused"
            );
        }
    }

    #[test]
    fn ordinary_paths_are_accepted_and_canonicalised() {
        assert_eq!(unsafe_path_reason("src/lib.rs"), None);
        assert_eq!(unsafe_path_reason("./src/lib.rs"), None);
        assert_eq!(unsafe_path_reason("src\\lib.rs"), None);
        assert_eq!(canonical_relative_path("./src\\lib.rs"), "src/lib.rs");
        assert_eq!(unsafe_path_reason(".gitignore"), None);
        assert_eq!(unsafe_path_reason("src/.gitkeep"), None);
    }

    #[test]
    fn generated_paths_are_detected() {
        for path in [
            "Cargo.lock",
            "sub/Cargo.lock",
            "package-lock.json",
            "pnpm-lock.yaml",
            "yarn.lock",
            "src/schema.generated.rs",
            "web/app.min.js",
            "deps/other.lock",
        ] {
            assert!(is_generated_path(path), "{path} must be generated content");
        }
        for path in ["src/lib.rs", "Cargo.toml", "src/locker.rs", "README.md"] {
            assert!(
                !is_generated_path(path),
                "{path} must not be generated content"
            );
        }
    }

    #[test]
    fn references_are_restricted_to_head_and_local_branches() {
        assert!(validate_reference("HEAD").is_ok());
        assert!(validate_reference("refs/heads/main").is_ok());
        assert!(validate_reference("refs/heads/feat/mvp0b-hardening").is_ok());
        for bad in [
            "refs/tags/v1",
            "refs/remotes/origin/main",
            "refs/heads/../evil",
            "refs/heads/main.lock",
            "refs/heads/",
            "main",
        ] {
            assert!(validate_reference(bad).is_err(), "{bad} must be refused");
        }
    }
}
