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

fn main() -> ExitCode {
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
