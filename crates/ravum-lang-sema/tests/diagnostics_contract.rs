//! Regression locks for the diagnostics contract (`spec/language/diagnostics.md`).
//!
//! These tests exist because the compiler once emitted `W-TYP-UNREACHABLE` for
//! an unreachable `match` arm while three normative rules forbade any such
//! diagnostic:
//!
//! * EXPR-5.9 (MUST): an unreachable arm is not an error, is not removed and
//!   produces **no diagnostic** (closed warning list);
//! * G-6.44 (MUST): same rule at the grammar level;
//! * DIAG-1.6 (MUST): the warning list is closed to `W-RES-UNUSED-IMPORT` and
//!   `W-OWN-UNUSED-VARIABLE`.
//!
//! `tools/registry_audit.py` blocks any *code literal* outside the registry, but
//! it cannot prove that a registered code is not emitted on a path where the
//! specification forbids it. This test exercises that path through the real
//! parser and checker.

use ravum_lang_sema::syntax::{parser, source::SourceMap, LANGUAGE_EDITION};
use ravum_lang_sema::{check, CheckOptions, CheckResult, Profile};

fn run(src: &str) -> CheckResult {
    let mut sources = SourceMap::new();
    let file = sources.add("src/lib.rv", src);
    let parsed = parser::parse_file(file, src);
    let parse_codes: Vec<String> = parsed.diags.iter().map(|d| d.code.clone()).collect();
    assert!(
        parsed.diags.is_empty(),
        "regression case is not syntactically valid: {parse_codes:?}"
    );
    check(
        &parsed.program,
        &sources,
        &CheckOptions {
            profile: Profile::Core,
            edition: LANGUAGE_EDITION.to_string(),
        },
    )
}

fn describe(result: &CheckResult) -> String {
    result
        .diags
        .iter()
        .map(|diag| format!("{}: {}", diag.code, diag.message))
        .collect::<Vec<_>>()
        .join(" | ")
}

#[test]
fn unreachable_wildcard_arm_emits_no_diagnostic() {
    let result = run(r#"
enum Flag { On, Off }

fn label(flag: Flag) -> i32 {
    match flag {
        Flag::On => 1,
        Flag::Off => 2,
        _ => 3,
    }
}
"#);
    assert!(
        result.diags.is_empty(),
        "EXPR-5.9/G-6.44 forbid any diagnostic for an unreachable arm; got: {}",
        describe(&result)
    );
}

#[test]
fn unreachable_after_catch_all_emits_no_diagnostic() {
    // The second `_` is unreachable because the first one already covers every
    // remaining value. The arm must still be parsed, checked and kept.
    let result = run(r#"
fn classify(n: i32) -> i32 {
    match n {
        _ => 1,
        _ => 2,
    }
}
"#);
    assert!(
        result.diags.is_empty(),
        "EXPR-5.9/G-6.44 forbid any diagnostic for an unreachable arm; got: {}",
        describe(&result)
    );
}
