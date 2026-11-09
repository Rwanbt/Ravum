//! Ravum semantic analysis: resolution, type checking, ownership/borrowing,
//! effect inference and profile enforcement.
//!
//! The public entry point is [`check`]. A successful result is a
//! [`facts::CheckedProgram`] that the code generator may consume; a failed result
//! carries diagnostics and no checked program.

pub mod builtins;
pub mod check;
pub mod effects;
pub mod facts;
pub mod loader;
mod own;
pub mod resolve;
pub mod typeck;
pub mod types;

pub use check::check;
pub use facts::{
    CallTarget, CheckOptions, CheckResult, CheckedProgram, ConstInfo, Effect, EffectSet, EnumInfo,
    FnSig, MethodTarget, Profile, Resolution, StructInfo, VariantInfo, VariantKind,
};
pub use loader::{load_package, LoadedPackage};
pub use types::Type;

pub use ravum_lang_syntax as syntax;
