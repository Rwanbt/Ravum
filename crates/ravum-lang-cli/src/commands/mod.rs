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
pub mod watch_jit;

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
            rt_entry,
            reload,
            dsp_block,
            sarif,
        } => check::run(
            &path,
            json,
            sarif,
            profile,
            locked,
            timings,
            rt_entry.as_deref(),
            reload,
            dsp_block,
        ),
        Command::Build {
            path,
            release,
            emit_rust,
            emit_ir,
            target,
            timings,
            rt_entry,
            dsp_block,
        } => build::run(
            &path,
            release,
            emit_rust,
            emit_ir,
            target,
            timings,
            rt_entry.as_deref(),
            dsp_block,
        ),
        Command::Run {
            path,
            args,
            target,
            json,
        } => run::run(&path, &args, target, json),
        Command::Watch {
            path,
            run,
            release,
            max_rebuilds,
            reload,
            jit,
            entry,
        } => {
            if jit {
                watch_jit::run(&path, entry.as_deref(), max_rebuilds)
            } else if reload {
                watch::run_reload(&path, release)
            } else {
                watch::run(&path, run, release, max_rebuilds)
            }
        }
        Command::Test { path, filter, json } => test::run(&path, filter.as_deref(), json),
        Command::Fmt { path, check } => fmt::run(&path, check),
        Command::Lint { path, deny } => lint::run(&path, &deny),
        Command::Doc { path, out } => doc::run(&path, out.as_deref()),
        Command::Cache { action, path } => cache::run(action, &path),
        Command::Doctor { json } => doctor::run(json),
        Command::Lsp => lsp::run(),
    }
}
