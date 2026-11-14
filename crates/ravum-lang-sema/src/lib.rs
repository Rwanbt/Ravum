//! Ravum semantic analysis: resolution, type checking, ownership/borrowing,
//! effect inference and profile enforcement.
//!
//! The public entry point is [`check`]. A successful result is a
//! [`facts::CheckedProgram`] that the code generator may consume; a failed result
//! carries diagnostics and no checked program.

#![forbid(unsafe_code)]

pub mod builtins;
pub mod check;
pub mod effects;
pub mod facts;
pub mod loader;
mod own;
pub mod resolve;
mod traps;
pub mod typeck;
pub mod types;

pub use check::{
    check, check_entry_with_timings, check_with_entry, check_with_timings, PhaseTimings,
};
pub use facts::{
    CallTarget, CheckOptions, CheckResult, CheckedProgram, ConstInfo, Effect, EffectSet, EnumInfo,
    FnSig, MethodTarget, Profile, Resolution, StructInfo, VariantInfo, VariantKind,
};
pub use loader::{load_package, load_package_with_parse_cache, LoadedPackage, ParseCache};
pub use types::Type;

pub use ravum_lang_syntax as syntax;
