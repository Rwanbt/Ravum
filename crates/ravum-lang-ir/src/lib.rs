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
//! IR v1 covers the scalar subset: 8/16/32/64/128-bit integers, `f32`/`f64`,
//! `bool`, `()`, functions of the root module, `let`, assignment, `if`,
//! `while`, `loop`, `for i in a..b`, `break`, `continue`, `return`, checked
//! arithmetic with the language's trap messages, `as` casts and
//! `panic("literal")`. Everything else is refused with
//! [`lower::LowerError::Unsupported`].

#![forbid(unsafe_code)]

pub mod heap;
pub mod interp;
pub mod ir;
pub mod lower;
pub mod mono;

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
                return schema_info(checked, item.id, &st.name.name).map(Some);
            }
        }
    }
    Ok(None)
}

/// HR-1.1 schema and memory layout of the user type `def`.
fn schema_info(
    checked: &ravum_lang_sema::CheckedProgram,
    def: ravum_lang_sema::syntax::NodeId,
    name: &str,
) -> Result<ir::StateInfo, lower::LowerError> {
    let span = checked
        .program
        .root
        .items
        .iter()
        .find(|i| i.id == def)
        .map(|i| i.span)
        .ok_or_else(|| lower::LowerError::Unsupported("schema of a nested type".into()))?;
    let schema = ravum_lang_sema::reload::schema(checked, def, span)
        .map_err(|_| lower::LowerError::Unsupported(format!("`{name}` has no canonical schema")))?;
    let ty = ravum_lang_sema::Type::Nominal {
        def,
        name: name.to_string(),
        args: Vec::new(),
    };
    let layout = lower::Layouts::new(checked).layout(&ty)?;
    Ok(ir::StateInfo {
        text: schema.text,
        digest: schema.digest,
        size: layout.size,
        align: layout.align,
    })
}

/// Schema of `T` in `fn migrate(old: T) -> State` (HR-1.7), if declared.
pub fn migrate_info(
    checked: &ravum_lang_sema::CheckedProgram,
) -> Result<Option<ir::StateInfo>, lower::LowerError> {
    use ravum_lang_sema::syntax::ast::ItemKind;
    for item in &checked.program.root.items {
        if let ItemKind::Fn(f) = &item.kind {
            if f.name.name != "migrate" {
                continue;
            }
            let sig = checked.facts.fn_sigs.get(&item.id);
            return match sig.map(|s| (s.params.as_slice(), &s.ret)) {
                Some((
                    [ravum_lang_sema::facts::ParamSig {
                        ty: ravum_lang_sema::Type::Nominal { def, name, args },
                        ..
                    }],
                    ravum_lang_sema::Type::Nominal { name: ret, .. },
                )) if args.is_empty() && ret == "State" => {
                    schema_info(checked, *def, name).map(Some)
                }
                _ => Err(lower::LowerError::Unsupported(
                    "`migrate` must be `fn migrate(old: T) -> State` (HR-1.8)".into(),
                )),
            };
        }
    }
    Ok(None)
}

/// Parse, check (profile `core`, as a library: no `main` required) and lower
/// a single-file program.
pub fn compile_source(src: &str) -> Result<Module, CompileError> {
    compile_source_with(src, None)
}

/// [`compile_source`] for the given entry points only ([`lower_checked_for`]).
pub fn compile_source_for(src: &str, entries: &[&str]) -> Result<Module, CompileError> {
    compile_source_with(src, Some(entries))
}

fn compile_source_with(src: &str, entries: Option<&[&str]>) -> Result<Module, CompileError> {
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
        Some(checked) if errors.is_empty() => match entries {
            Some(e) => lower_checked_for(&checked, e),
            None => lower_checked(&checked),
        },
        _ => Err(CompileError::Rejected(errors)),
    }
}

/// The IR module of an already checked program (as [`compile_source`]
/// builds it): for a driver that checks incrementally (T13, `watch --backend
/// jit`) and holds complete facts.
pub fn lower_checked(checked: &ravum_lang_sema::CheckedProgram) -> Result<Module, CompileError> {
    finish(checked, lower::lower(checked))
}

/// [`lower_checked`] for the given entry points only ([`lower::lower_for`]):
/// what they cannot reach may use constructs the IR does not cover.
pub fn lower_checked_for(
    checked: &ravum_lang_sema::CheckedProgram,
    entries: &[&str],
) -> Result<Module, CompileError> {
    finish(checked, lower::lower_for(checked, entries))
}

fn finish(
    checked: &ravum_lang_sema::CheckedProgram,
    lowered: Result<Module, lower::LowerError>,
) -> Result<Module, CompileError> {
    let mut m = lowered.map_err(CompileError::Lower)?;
    m.state = state_info(checked).map_err(CompileError::Lower)?;
    m.migrate_from = migrate_info(checked).map_err(CompileError::Lower)?;
    Ok(m)
}
