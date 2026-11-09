//! `ravum-lang run` — build then execute the binary, forwarding stdio/args/code.

use ravum_lang_sema::Profile;
use std::path::Path;
use std::process::Command;

use crate::pipeline::{self, BuildOptions};
use crate::util;

pub fn run(path: &Path, child_args: &[String]) -> i32 {
    let frontend = pipeline::analyze(path, Profile::Core, false);
    if !frontend.success() {
        eprint!("{}", pipeline::render_diags(&frontend));
        util::err(format!(
            "run: {} error(s); the program was not executed",
            frontend.error_count()
        ));
        return util::EXIT_FAIL;
    }

    let options = BuildOptions::default();
    let artifacts = match pipeline::compile_program(&frontend, &options) {
        Ok(artifacts) => artifacts,
        Err(error) => {
            error.print();
            return error.exit_code();
        }
    };

    eprintln!("running: {}", util::pretty_path(&artifacts.binary));
    let mut command = Command::new(&artifacts.binary);
    command.args(child_args);
    command.current_dir(frontend.root());
    match command.status() {
        Ok(status) => match status.code() {
            Some(code) => code.clamp(0, 255),
            None => {
                util::err("run: the program was terminated by a signal");
                util::EXIT_FAIL
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            util::err(format!(
                "run: cannot execute `{}`: {error}",
                util::pretty_path(&artifacts.binary)
            ));
            util::EXIT_TOOL
        }
        Err(error) => {
            util::err(format!(
                "run: cannot execute `{}`: {error}",
                util::pretty_path(&artifacts.binary)
            ));
            util::EXIT_FAIL
        }
    }
}
