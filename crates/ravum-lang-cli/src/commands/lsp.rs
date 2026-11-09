//! `ravum-lang lsp` — start the Ravum language server on stdin/stdout.
//!
//! This is a real LSP server (`crates/ravum-lang-lsp`), not a diagnostic
//! printer: it speaks JSON-RPC 2.0 with `Content-Length` framing and implements
//! the standard lifecycle plus diagnostics, hover, definition, references,
//! rename, completion, symbols and formatting.
//!
//! Logs go to **stderr** only: stdout carries the protocol and nothing else.

use crate::util;

pub fn run() -> i32 {
    match ravum_lang_lsp::run_stdio() {
        Ok(()) => util::EXIT_OK,
        Err(error) => {
            util::err(format!("lsp: transport error: {error}"));
            util::EXIT_FAIL
        }
    }
}
