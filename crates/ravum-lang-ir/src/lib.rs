//! Ravum IR v1 (gate T14, ADR-010): a versioned, serializable intermediate
//! representation lowered from the **checked** program, independent of the
//! Rust code generator.
//!
//! * [`ir`] — the data model and its JSON form (`IR_VERSION`).
//! * [`lower`] — fail-closed lowering from `ravum-lang-sema` facts.
//! * [`interp`] — reference interpreter, the first consumer of the IR.
//!
//! The second consumer is the native JIT backend (`ravum-lang-jit`, gate T15).
//!
//! IR v1 covers the scalar subset: integers up to 64 bits, `f32`/`f64`,
//! `bool`, `()`, functions of the root module, `let`, assignment, `if`,
//! `while`, `loop`, `for i in a..b`, `break`, `continue`, `return`, checked
//! arithmetic with the language's trap messages, `as` casts and
//! `panic("literal")`. Everything else is refused with
//! [`lower::LowerError::Unsupported`].

#![forbid(unsafe_code)]

pub mod interp;
pub mod ir;
pub mod lower;

pub use ir::{Module, IR_VERSION};

use ravum_lang_sema::syntax::{parser, SourceMap, LANGUAGE_EDITION};
use ravum_lang_sema::{check, CheckOptions, Profile};

/// Why a source could not be turned into IR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    /// Parse or check diagnostics (codes), the program is not accepted.
    Rejected(Vec<String>),
    Lower(lower::LowerError),
}

/// Schema and layout of `struct State` in the root module, if declared. The
/// schema text and digest are the ones of `ravum_lang_sema::reload::schema`
/// (HR-1.1), shared with the `rustc` reload path.
pub fn state_info(
    checked: &ravum_lang_sema::CheckedProgram,
) -> Result<Option<ir::StateInfo>, lower::LowerError> {
    use ravum_lang_sema::syntax::ast::ItemKind;
    for item in &checked.program.root.items {
        if let ItemKind::Struct(st) = &item.kind {
            if st.name.name == "State" {
                let schema =
                    ravum_lang_sema::reload::schema(checked, item.id, item.span).map_err(|_| {
                        lower::LowerError::Unsupported("`State` has no canonical schema".into())
                    })?;
                let ty = ravum_lang_sema::Type::Nominal {
                    def: item.id,
                    name: st.name.name.clone(),
                    args: Vec::new(),
                };
                let layout = lower::Layouts::new(checked).layout(&ty)?;
                return Ok(Some(ir::StateInfo {
                    text: schema.text,
                    digest: schema.digest,
                    size: layout.size,
                    align: layout.align,
                }));
            }
        }
    }
    Ok(None)
}

/// Parse, check (profile `core`, as a library: no `main` required) and lower
/// a single-file program.
pub fn compile_source(src: &str) -> Result<Module, CompileError> {
    let mut sources = SourceMap::new();
    let file = sources.add("src/lib.rv", src);
    let parsed = parser::parse_file(file, src);
    if !parsed.diags.is_empty() {
        return Err(CompileError::Rejected(
            parsed.diags.iter().map(|d| d.code.clone()).collect(),
        ));
    }
    let options = CheckOptions {
        profile: Profile::Core,
        edition: LANGUAGE_EDITION.to_string(),
    };
    let result = check(&parsed.program, &sources, &options);
    let errors: Vec<String> = result
        .diags
        .iter()
        .filter(|d| d.severity == ravum_lang_sema::syntax::Severity::Error)
        .map(|d| d.code.clone())
        .collect();
    match result.checked {
        Some(checked) if errors.is_empty() => {
            let mut m = lower::lower(&checked).map_err(CompileError::Lower)?;
            m.state = state_info(&checked).map_err(CompileError::Lower)?;
            Ok(m)
        }
        _ => Err(CompileError::Rejected(errors)),
    }
}
