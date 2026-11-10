//! `ravum-lang` — the Ravum Core 1.0 language toolchain.
//!
//! Exit codes (stable contract, documented in `README.md`):
//! * `0` success;
//! * `1` compilation/diagnostic/test failure or a safe operational refusal;
//! * `2` invalid command line;
//! * `3` external tool missing (`rustc`) or an explicitly UNSUPPORTED command.
//!
//! A command that is not implemented is never advertised as functional. Every
//! documented command runs the real pipeline; `lsp` starts a real language
//! server on stdio (`crates/ravum-lang-lsp`).

mod args;
mod commands;
mod json;
mod pipeline;
mod util;

use std::process::ExitCode;

/// A legal program may nest up to `ravum_lang_syntax::parser::MAX_DEPTH` (256)
/// levels. A recursive-descent frame in a debug build is large enough that the
/// platform default stack dies *before* the parser reaches its own limit:
/// `let x = ((((…1…))))` with 240 parentheses aborted with a stack overflow
/// (0xC00000FD) instead of either compiling or reporting the deterministic
/// `E-PARSE-DEPTH` that `diagnostics.md` promises. Every command therefore runs
/// on a thread whose stack comfortably holds `MAX_DEPTH` frames.
const STACK_BYTES: usize = 64 * 1024 * 1024;

fn main() -> ExitCode {
    match std::thread::Builder::new()
        .name("ravum-lang".to_string())
        .stack_size(STACK_BYTES)
        .spawn(run)
    {
        // A panic inside `run` has already printed its message; do not report
        // success for a command that never returned.
        Ok(handle) => handle
            .join()
            .unwrap_or(ExitCode::from(util::EXIT_FAIL as u8)),
        Err(error) => {
            eprintln!("error: cannot start the compiler thread: {error}");
            ExitCode::from(util::EXIT_FAIL as u8)
        }
    }
}

fn run() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    match args::parse(&raw) {
        Ok(args::Invocation::Help(Some(topic))) => {
            print!("{}", args::help_for(&topic));
            ExitCode::SUCCESS
        }
        Ok(args::Invocation::Help(None)) => {
            print!("{}", args::help_text());
            ExitCode::SUCCESS
        }
        Ok(args::Invocation::Version) => {
            println!(
                "ravum-lang {} (edition {})",
                env!("CARGO_PKG_VERSION"),
                ravum_lang_syntax::LANGUAGE_EDITION
            );
            ExitCode::SUCCESS
        }
        Ok(args::Invocation::Command(command)) => ExitCode::from(commands::dispatch(command) as u8),
        Err(message) => {
            eprintln!("error: {message}");
            eprintln!("run `ravum-lang --help` for usage");
            ExitCode::from(util::EXIT_USAGE as u8)
        }
    }
}
