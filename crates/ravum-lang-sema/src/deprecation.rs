//! Deprecated standard-library paths (DIAG-1.10, `W-DEPRECATED`).
//!
//! A use of a path listed in [`DEPRECATED`] is accepted and reported with one
//! `W-DEPRECATED` warning naming the version that deprecated it and its
//! replacement. The warning never changes a verdict: a deprecated path is
//! still a valid program (DEP-3.2, DIAG-1.5).
//!
//! Core 1.0 deprecates nothing, so the table is empty. A row is added only
//! with an entry in the register of `docs/release/DEPRECATION-POLICY.md` §5.

/// One deprecated standard-library path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deprecation {
    /// Canonical builtin path, as `Resolution::BuiltinFn` records it.
    pub path: &'static str,
    /// Language edition or release that deprecated it.
    pub since: &'static str,
    /// What to write instead.
    pub replacement: &'static str,
}

/// The register of deprecated paths. Empty in Core 1.0.
pub const DEPRECATED: &[Deprecation] = &[];

/// The deprecation of `path`, if it has one.
pub fn deprecation_of(path: &str) -> Option<Deprecation> {
    #[cfg(test)]
    if let Some(table) = TEST_DEPRECATED.with(std::cell::Cell::get) {
        return table.iter().copied().find(|d| d.path == path);
    }
    DEPRECATED.iter().copied().find(|d| d.path == path)
}

/// The message of the `W-DEPRECATED` warning for `deprecation`.
pub fn message(deprecation: &Deprecation) -> String {
    format!(
        "`{}` is deprecated since {}; use {} instead",
        deprecation.path, deprecation.since, deprecation.replacement
    )
}

// Test override of [`DEPRECATED`] (`None`: the real register).
#[cfg(test)]
thread_local! {
    pub(crate) static TEST_DEPRECATED: std::cell::Cell<Option<&'static [Deprecation]>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::CheckOptions;
    use ravum_lang_syntax::{parser, source::SourceMap};

    const FAKE: &[Deprecation] = &[Deprecation {
        path: "std::math::abs",
        since: "core-1.1",
        replacement: "`std::math::fabs`",
    }];

    fn diagnostics(source: &str) -> Vec<(String, String)> {
        let mut sources = SourceMap::new();
        let file = sources.add("main.rv", source);
        let parsed = parser::parse_file(file, source);
        assert!(parsed.diags.is_empty(), "{source}");
        crate::check(&parsed.program, &sources, &CheckOptions::default())
            .diags
            .iter()
            .map(|d| (d.code.clone(), d.message.clone()))
            .collect()
    }

    #[test]
    fn core_1_0_deprecates_nothing() {
        assert!(DEPRECATED.is_empty());
        assert_eq!(deprecation_of("std::math::abs"), None);
    }

    #[test]
    fn a_deprecated_path_is_accepted_with_one_warning_per_use() {
        TEST_DEPRECATED.with(|cell| cell.set(Some(FAKE)));
        let found = diagnostics(
            "use std::math;\nfn main() {\n    let a = math::abs(-1.0);\n    \
             let b = math::abs(-2.0);\n    io::println(&(a + b).to_string());\n}\n",
        );
        TEST_DEPRECATED.with(|cell| cell.set(None));
        let expected =
            "`std::math::abs` is deprecated since core-1.1; use `std::math::fabs` instead";
        assert_eq!(
            found,
            [
                ("W-DEPRECATED".to_string(), expected.to_string()),
                ("W-DEPRECATED".to_string(), expected.to_string()),
            ]
        );
    }

    #[test]
    fn a_path_outside_the_register_is_not_reported() {
        TEST_DEPRECATED.with(|cell| cell.set(Some(FAKE)));
        let found = diagnostics("fn main() {\n    io::println(\"ok\");\n}\n");
        TEST_DEPRECATED.with(|cell| cell.set(None));
        assert!(found.is_empty(), "{found:?}");
    }
}
