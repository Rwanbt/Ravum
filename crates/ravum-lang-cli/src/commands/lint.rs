//! `ravum-lang lint` — checker warnings with explicit `--deny CODE` promotion.
//!
//! Only documented checker warnings (`W-*`) are reported. This command does not
//! invent heuristic rules that the frontend does not implement.

use ravum_lang_sema::Profile;
use std::path::Path;

use crate::pipeline::{self, Frontend};
use crate::util;

pub fn run(path: &Path, deny: &[String]) -> i32 {
    let frontend = pipeline::analyze(path, Profile::Core, false);
    let denied = count_denied(&frontend, deny);

    let rendered = pipeline::render_diags(&frontend);
    if !rendered.is_empty() {
        eprint!("{rendered}");
    }
    for code in deny {
        let matched = frontend
            .diags
            .iter()
            .filter(|diag| diag.code == *code)
            .count();
        if matched > 0 {
            eprintln!("denied: {matched} warning(s) with code `{code}`");
        }
    }
    println!(
        "lint: {} warning(s), {} error(s), {} denied warning(s){}",
        frontend.warning_count(),
        frontend.error_count(),
        denied,
        if deny.is_empty() {
            String::new()
        } else {
            format!(" (deny: {})", deny.join(", "))
        }
    );

    if frontend.error_count() > 0 || denied > 0 {
        util::EXIT_FAIL
    } else {
        util::EXIT_OK
    }
}

fn count_denied(frontend: &Frontend, deny: &[String]) -> usize {
    if deny.is_empty() {
        return 0;
    }
    let deny_all = deny.iter().any(|code| code == "all" || code == "warnings");
    frontend
        .diags
        .iter()
        .filter(|diag| diag.severity == ravum_lang_syntax::diag::Severity::Warning)
        .filter(|diag| deny_all || deny.iter().any(|code| code == &diag.code))
        .count()
}
