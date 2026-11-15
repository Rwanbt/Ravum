//! Ravum reference backend: generate one self-contained, auditable Rust file from
//! a [`CheckedProgram`] and compile it with `rustc` as a toolchain (option B of
//! the M5 decision).
//!
//! Guarantees of the generator:
//! * it only consumes a fully checked program (all paths/types resolved);
//! * it emits safe Rust only — no `unsafe`, no raw pointers, no `transmute`;
//! * it preserves Ravum semantics: overflow checks, panic behaviour, drop order,
//!   effect boundaries are decided by the checker, not by this backend;
//! * output is deterministic for a given input and options.

#![forbid(unsafe_code)]

use ravum_lang_sema::facts::CheckedProgram;

pub mod emit;
pub mod prelude;

pub use emit::Generator;

/// Version of the generator, part of the CLI's deterministic build identity: a
/// new generator version must never reuse an artifact produced by an older one.
pub const GENERATOR_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone)]
pub struct GenOptions {
    /// Emit `// rv: file:line` markers so generated code is auditable.
    pub source_comments: bool,
    /// Emit a test runner instead of the user `main` (for `ravum-lang test`).
    pub test_mode: bool,
    /// Name used in the generated banner (never used for logic).
    pub crate_name: String,
}

impl Default for GenOptions {
    fn default() -> Self {
        Self {
            source_comments: true,
            test_mode: false,
            crate_name: "ravum_program".to_string(),
        }
    }
}

#[derive(Debug)]
pub struct CodegenError {
    pub message: String,
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CodegenError {}

/// Generate a complete Rust source file (prelude included).
pub fn generate(checked: &CheckedProgram, options: &GenOptions) -> Result<String, CodegenError> {
    Generator::new(checked, options).generate()
}

#[cfg(test)]
mod tests {
    use super::CodegenError;

    #[test]
    fn codegen_error_displays_its_message() {
        let error = CodegenError {
            message: "unsupported builtin method".to_string(),
        };
        assert_eq!(error.to_string(), "unsupported builtin method");
        // The `std::error::Error` impl is empty; this asserts it is usable.
        let as_error: &dyn std::error::Error = &error;
        assert_eq!(as_error.to_string(), "unsupported builtin method");
    }
}
