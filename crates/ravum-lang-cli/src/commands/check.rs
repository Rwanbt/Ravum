//! `ravum-lang check` — load, resolve and check a package.

use ravum_lang_sema::Profile;
use std::path::Path;

use crate::json;
use crate::pipeline::{self, AnalysisTimings, Frontend};
use crate::util;

pub fn run(
    path: &Path,
    json_output: bool,
    profile: Profile,
    locked: bool,
    request_timings: bool,
    rt_entry: Option<&str>,
) -> i32 {
    let (frontend, timings) = if request_timings {
        pipeline::analyze_entry_with_timings(path, profile, locked, rt_entry)
    } else {
        (
            pipeline::analyze_with_entry(path, profile, locked, rt_entry),
            AnalysisTimings::default(),
        )
    };
    let success = frontend.success();

    if json_output {
        let diagnostics = json::diagnostics(frontend.diags.as_slice(), &frontend.pkg.sources);
        let summary = pipeline::base_summary(&frontend, profile);
        let meta = json::ReportMeta {
            command: "check",
            success,
            exit_code: if success {
                util::EXIT_OK
            } else {
                util::EXIT_FAIL
            },
            target_kind: if frontend.pkg.crate_kind
                == Some(ravum_lang_sema::loader::CrateKind::File)
            {
                "file"
            } else {
                "package"
            },
            target_path: util::target_display(path),
            profile: Some(profile.name().to_string()),
            dependencies_digest: Some(frontend.pkg.dependencies_digest.clone()),
            lockfile_stale: lockfile_stale(&frontend),
        };
        let envelope = json::envelope_with_timings(
            meta,
            diagnostics,
            summary,
            if request_timings {
                Some(timings.json())
            } else {
                None
            },
        );
        json::print(&envelope);
    } else {
        print_human(&frontend, profile);
        if request_timings {
            print_timings(&timings);
        }
    }

    if success {
        util::EXIT_OK
    } else {
        util::EXIT_FAIL
    }
}

/// `lockfile_stale` is only meaningful when a lockfile was considered.
fn lockfile_stale(frontend: &Frontend) -> Option<bool> {
    match frontend.pkg.lockfile_state {
        ravum_lang_sema::loader::LockfileState::Stale => Some(true),
        ravum_lang_sema::loader::LockfileState::Created
        | ravum_lang_sema::loader::LockfileState::Current => Some(false),
        _ => None,
    }
}

fn print_human(frontend: &Frontend, profile: Profile) {
    let rendered = pipeline::render_diags(frontend);
    if !rendered.is_empty() {
        eprint!("{rendered}");
    }
    println!("{}", pipeline::text_summary(frontend, profile));
    if !frontend.pkg.dependencies.is_empty() {
        let dependencies = frontend
            .pkg
            .dependencies
            .iter()
            .map(|dependency| {
                format!(
                    "{} {} ({})",
                    dependency.name, dependency.version, dependency.digest
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "dependencies: {} (digest {})",
            dependencies, frontend.pkg.dependencies_digest
        );
    }
    println!("lockfile: {}", frontend.pkg.lockfile_state.name());
}

fn print_timings(timings: &AnalysisTimings) {
    println!(
        "timings: load={:.3}ms parse={:.3}ms resolve={:.3}ms typeck={:.3}ms \
         effects={:.3}ms own={:.3}ms total={:.3}ms",
        pipeline::ms(timings.load),
        pipeline::ms(timings.parse),
        pipeline::ms(timings.resolve),
        pipeline::ms(timings.typeck),
        pipeline::ms(timings.effects),
        pipeline::ms(timings.own),
        pipeline::ms(timings.total()),
    );
}
