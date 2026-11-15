//! `ravum-lang build` — check, generate Rust, compile with rustc.

use ravum_lang_sema::Profile;
use std::path::{Path, PathBuf};

use crate::pipeline::{self, AnalysisTimings, BuildOptions, BuildTarget, BuildTimings};
use crate::util;

pub fn run(
    path: &Path,
    release: bool,
    emit_rust: Option<PathBuf>,
    emit_ir: bool,
    target: BuildTarget,
    request_timings: bool,
    rt_entry: Option<&str>,
) -> i32 {
    // `build` has no `--profile` flag: without `--rt-entry` it keeps the
    // historical Core analysis; with `--rt-entry` (T6) the analysis applies the
    // `rt` rules to the functions reachable from that entry, then the same
    // codegen runs.
    let profile = if rt_entry.is_some() {
        Profile::Rt
    } else {
        Profile::Core
    };
    let (frontend, analysis) = if request_timings {
        pipeline::analyze_entry_with_timings(path, profile, false, rt_entry)
    } else {
        (
            pipeline::analyze_with_entry(path, profile, false, rt_entry),
            AnalysisTimings::default(),
        )
    };
    if !frontend.success() {
        eprint!("{}", pipeline::render_diags(&frontend));
        util::err(format!(
            "build: {} error(s); no artifact was produced",
            frontend.error_count()
        ));
        if request_timings {
            print_timings(&analysis, &BuildTimings::default());
        }
        return util::EXIT_FAIL;
    }

    if emit_ir {
        print!("{}", pipeline::ir_dump(&frontend));
    }

    let options = BuildOptions {
        release,
        test_mode: false,
        emit_rust,
        target,
    };
    let mut build_timings = BuildTimings::default();
    let result = pipeline::compile_program_with_timings(&frontend, &options, &mut build_timings);
    match result {
        Ok(artifacts) => {
            // The documented user-facing paths come first (`.ravum/gen/main.rs`,
            // `.ravum/bin/<name>`), then the build identity that produced them and
            // the manifest a caller can use to verify artifact ↔ sources.
            println!("generated: {}", util::pretty_path(&artifacts.generated));
            println!("built: {}", util::pretty_path(&artifacts.binary));
            println!("identity: {}", artifacts.identity.digest);
            println!("manifest: {}", util::pretty_path(&artifacts.manifest));
            println!("isolated: {}", util::pretty_path(&artifacts.build_dir));
            if request_timings {
                print_timings(&analysis, &build_timings);
            }
            util::EXIT_OK
        }
        Err(error) => {
            if request_timings {
                print_timings(&analysis, &build_timings);
            }
            error.print();
            error.exit_code()
        }
    }
}

fn print_timings(analysis: &AnalysisTimings, build: &BuildTimings) {
    let total = analysis.total() + build.codegen + build.rustc;
    println!(
        "timings: load={:.3}ms parse={:.3}ms resolve={:.3}ms typeck={:.3}ms \
         effects={:.3}ms own={:.3}ms codegen={:.3}ms rustc={:.3}ms total={:.3}ms",
        pipeline::ms(analysis.load),
        pipeline::ms(analysis.parse),
        pipeline::ms(analysis.resolve),
        pipeline::ms(analysis.typeck),
        pipeline::ms(analysis.effects),
        pipeline::ms(analysis.own),
        pipeline::ms(build.codegen),
        pipeline::ms(build.rustc),
        pipeline::ms(total),
    );
}
