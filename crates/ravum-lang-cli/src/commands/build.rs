//! `ravum-lang build` — check, generate Rust, compile with rustc.

use ravum_lang_sema::Profile;
use std::path::{Path, PathBuf};

use crate::pipeline::{self, BuildOptions, BuildTarget};
use crate::util;

pub fn run(
    path: &Path,
    release: bool,
    emit_rust: Option<PathBuf>,
    emit_ir: bool,
    target: BuildTarget,
) -> i32 {
    let frontend = pipeline::analyze(path, Profile::Core, false);
    if !frontend.success() {
        eprint!("{}", pipeline::render_diags(&frontend));
        util::err(format!(
            "build: {} error(s); no artifact was produced",
            frontend.error_count()
        ));
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
    match pipeline::compile_program(&frontend, &options) {
        Ok(artifacts) => {
            // The documented user-facing paths come first (`.ravum/gen/main.rs`,
            // `.ravum/bin/<name>`), then the build identity that produced them and
            // the manifest a caller can use to verify artifact ↔ sources.
            println!("generated: {}", util::pretty_path(&artifacts.generated));
            println!("built: {}", util::pretty_path(&artifacts.binary));
            println!("identity: {}", artifacts.identity.digest);
            println!("manifest: {}", util::pretty_path(&artifacts.manifest));
            println!("isolated: {}", util::pretty_path(&artifacts.build_dir));
            util::EXIT_OK
        }
        Err(error) => {
            error.print();
            error.exit_code()
        }
    }
}
