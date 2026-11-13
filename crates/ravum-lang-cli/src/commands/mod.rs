//! Command implementations.

pub mod build;
pub mod cache;
pub mod check;
pub mod doc;
pub mod doctor;
pub mod fmt;
pub mod init;
pub mod lint;
pub mod lsp;
pub mod run;
pub mod test;
pub mod watch;

use crate::args::Command;

pub fn dispatch(command: Command) -> i32 {
    match command {
        Command::Init { dir, lib, force } => init::run(&dir, lib, force),
        Command::Check {
            path,
            json,
            profile,
            locked,
            timings,
        } => check::run(&path, json, profile, locked, timings),
        Command::Build {
            path,
            release,
            emit_rust,
            emit_ir,
            target,
            timings,
        } => build::run(&path, release, emit_rust, emit_ir, target, timings),
        Command::Run { path, args, target } => run::run(&path, &args, target),
        Command::Watch {
            path,
            run,
            release,
            max_rebuilds,
        } => watch::run(&path, run, release, max_rebuilds),
        Command::Test { path, filter, json } => test::run(&path, filter.as_deref(), json),
        Command::Fmt { path, check } => fmt::run(&path, check),
        Command::Lint { path, deny } => lint::run(&path, &deny),
        Command::Doc { path, out } => doc::run(&path, out.as_deref()),
        Command::Cache { action, path } => cache::run(action, &path),
        Command::Doctor { json } => doctor::run(json),
        Command::Lsp => lsp::run(),
    }
}
