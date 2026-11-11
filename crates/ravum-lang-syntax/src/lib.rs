//! Ravum language frontend: source map, spans, diagnostics, lexer, AST, parser
//! and formatter.
//!
//! This crate is deliberately independent from the Rust toolchain and from the
//! orchestration crates: it can parse untrusted `.rv` input without touching the
//! filesystem and without `rustc_private`.

pub mod ast;
pub mod diag;
pub mod fmt;
pub mod lexer;
pub mod parser;
pub mod source;
pub mod span;
pub mod token;

pub use ast::{NodeId, Program};
pub use diag::{Diagnostic, DiagnosticBag, Severity};
pub use source::SourceMap;
pub use span::{FileId, Span};
pub use token::{Kw, Token, TokenKind};

/// Version of the language edition implemented by this frontend.
pub const LANGUAGE_EDITION: &str = "core-1.0-draft.1";
