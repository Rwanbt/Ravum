//! Ravum Core 1.0 language server.
//!
//! This crate is the *real* LSP implementation behind `ravum-lang lsp`: JSON-RPC
//! 2.0 over stdio with the standard `Content-Length` framing, incremental
//! document synchronisation, diagnostics, hover, go-to-definition, references,
//! rename, completion, document symbols, workspace symbols and formatting.
//!
//! Design rules:
//! * the analyzer is the same frozen frontend as the CLI, so an editor
//!   diagnostic and a `ravum-lang check` diagnostic cannot disagree;
//! * unsaved buffers are analysed from memory (`LoadOptions::overrides`), the
//!   user's files are never written by the server;
//! * a feature with no checker fact returns *nothing* rather than a guess.
//!
//! Qualification evidence, including the client-driven integration test and the
//! explicit limitations, lives in `docs/qualification/LSP.md`.

pub mod analysis;
pub mod convert;
pub mod features;
pub mod paths;
pub mod rpc;
pub mod server;
pub mod walk;

pub use server::{Document, Server};

/// Serve LSP on stdin/stdout until the client sends `exit` or closes the pipe.
pub fn run_stdio() -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut server = Server::new();
    server.run(stdin.lock(), stdout.lock())
}
