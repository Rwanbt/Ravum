//! Rust emitter — reference backend B.
//!
//! OWNER: worker `codegen`. Turns a [`CheckedProgram`] (AST + `Facts`) into one
//! self-contained, auditable Rust file compiled by `rustc --edition 2021`.
//!
//! Guarantees:
//! * safe Rust only: no `unsafe`, no raw pointers, no `transmute`, and the file
//!   carries `#![forbid(unsafe_code)]`;
//! * no semantic guessing: every type, resolution, call target and method target
//!   comes from `Facts`; a missing fact is an explicit [`CodegenError`];
//! * Ravum semantics are *preserved*, never re-decided: moves/drops (Rust rules
//!   coincide with the conservative Ravum model), overflow checks (left to
//!   `-C overflow-checks=on`), panic/exit conventions, left-to-right evaluation,
//!   short-circuit `&&`/`||`, exhaustive `match`;
//! * deterministic: the same `CheckedProgram` and options produce byte-identical
//!   text (only ordered collections and source order are used).
//!
//! Module mapping (normative for the emitted Rust — the generator is the
//! authority for the Rust cartography): the Ravum root module becomes
//! `crate::rvm_root`, every named Ravum module `foo` becomes an inline
//! `mod rvm_foo`. All references are emitted as fully qualified `crate::...`
//! paths, so no `use` item is copied. Nominal types and function calls share
//! this single mapping.
//!
//! Path facts: `Facts::item_paths` is a *logical* Ravum path (`crate::m::f`).
//! When present it is rewritten into the emitted cartography
//! (`crate::rvm_root::rvm_m::f`): `crate` then the module prefixes from
//! `Facts::module_paths`, then the mangled item name. Because only the AST tree
//! is guaranteed to match the emitted modules, a path deduced from the AST wins
//! when both exist and disagree; see [`Emitter::item_path`].

use crate::prelude::PRELUDE;
use crate::{CodegenError, GenOptions};
use ravum_lang_sema::facts::{
    CallTarget, CheckedProgram, ConstValue, MethodTarget, Resolution, UseKind, VariantKind,
};
use ravum_lang_sema::types::Type;
use ravum_lang_syntax::ast::{
    Block, EnumItem, Expr, ExprKind, ExternBlock, FloatTy, FnItem, GenericParam, Ident, ImplItem,
    IntTy, Item, ItemKind, LitPat, MatchArm, Module, NodeId, ParamKind, Pat, PatKind, Path,
    PathRoot, Receiver, Stmt, StmtKind, StructItem, Type as AstType, TypeKind, UnOp,
    VariantDefFields, VariantPatFields,
};
use ravum_lang_syntax::span::Span;
use std::collections::{BTreeMap, BTreeSet};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const INDENT: &str = "    ";
const GENERATED_MAIN: &str = "rv_user_main";

fn err(message: impl Into<String>) -> CodegenError {
    CodegenError {
        message: message.into(),
    }
}

/// Position in which a Rust type is written. Only text types depend on it.
///
/// Ravum Core 1.0 keeps two views of the borrowed text type:
/// * a stored, field-held, returned or otherwise owned place may only hold the
///   `&'static str` view (OWN-6.2, and OWN-6.1 forbids storing any other
///   reference), so the Rust spelling needs the explicit `'static` lifetime —
///   without it `struct S { text: &str }` / `fn f() -> &str` is E0106;
/// * a function parameter may receive the *non-static* view produced inside one
///   instruction by `String::as_str` / `str::as_str` (STD-9.3, TYP-13.1,
///   OWN-6.7), e.g. `io::println(s.as_str())`. Rust elides that lifetime at the
///   parameter, so the spelling must stay `&str`: writing `&'static str` would
///   reject a program the checker accepts (`fn g(s: &str)` called with
///   `text.as_str()` — the exact check-pass/build-fail class being fixed).
///
/// `TypePos` applies to the parameter's **own** `&str` only. A `&str` nested
/// inside a container (`Option<&str>`, `[&str; 2]`, `Vec<&str>`, `(&str, i32)`)
/// is always written `&'static str`, in `Value` position: the checker proves
/// such texts static by construction (D13 rule 2 — no borrowed `&str` can be
/// stored in an aggregate), and Rust needs the explicit lifetime for a
/// projection out of the container to be usable as `&'static str`
/// (`fn pick(a: [&str; 2]) -> &str` is emitted as `[&'static str; 2]`, otherwise
/// `a[1]` is E0310).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypePos {
    /// Owned/stored/returned/annotated position: `&str` is `&'static str`.
    Value,
    /// Function parameter or foreign-function declaration: the parameter's own
    /// `&str` has its lifetime elided; nested text types stay [`TypePos::Value`].
    Param,
}

/// G-6.36: a float literal anywhere inside a pattern forces the guard lowering
/// of the enclosing `match` arm. Mirrors `typeck::pattern_contains_float`, which
/// keeps exhaustiveness conservative for the same reason.
fn pattern_contains_float(pat: &Pat) -> bool {
    match &pat.kind {
        PatKind::Lit(LitPat::Float { .. }) => true,
        PatKind::Tuple(items) => items.iter().any(pattern_contains_float),
        PatKind::Variant { fields, .. } => match fields {
            VariantPatFields::Tuple(items) => items.iter().any(pattern_contains_float),
            VariantPatFields::Struct(fields) => fields
                .iter()
                .any(|field| pattern_contains_float(&field.pat)),
        },
        _ => false,
    }
}

/// Types the backend accepts at the C ABI boundary in Core 1.0, checked on the
/// syntax tree so a hand-built `CheckedProgram` cannot smuggle an ABI-unsafe
/// signature into the output.
///
/// The list is **closed** and mirrors the checker's `resolve::ffi_safe` exactly
/// (C-FFI-1): `i8 i16 i32 i64`, `u8 u16 u32 u64`, `isize usize`, `f32 f64`,
/// `bool`, `()`. `i128`/`u128` are **not** admitted (S4, 2026-11-14; C-FFI-2):
/// no portable C width covers them (MSVC has no `__int128`). The checker already
/// refuses them (`E-FFI-TYPE`); this twin list is the backend's defence in depth
/// so a hand-built `CheckedProgram` cannot reach the ABI with them either.
/// `backend_audit::c_ffi_1_backend_refuses_i128_and_u128_in_a_hand_built_program`
/// is the non-vacuity test for this list.
fn ast_ffi_safe(ty: &AstType) -> bool {
    match &ty.kind {
        TypeKind::Unit => true,
        TypeKind::Path(path) => {
            path.segments.len() == 1
                && path.segments[0].args.is_empty()
                && matches!(
                    path.segments[0].name.name.as_str(),
                    "i8" | "i16"
                        | "i32"
                        | "i64"
                        | "u8"
                        | "u16"
                        | "u32"
                        | "u64"
                        | "isize"
                        | "usize"
                        | "f32"
                        | "f64"
                        | "bool"
                )
        }
        _ => false,
    }
}

/// Entry point object kept stable for `lib.rs`: build it and call `generate`.
pub struct Generator<'a> {
    pub checked: &'a CheckedProgram,
    pub options: &'a GenOptions,
}

impl<'a> Generator<'a> {
    pub fn new(checked: &'a CheckedProgram, options: &'a GenOptions) -> Self {
        Self { checked, options }
    }

    pub fn generate(&self) -> Result<String, CodegenError> {
        let index = Index::build(self.checked);
        let emitter = Emitter {
            checked: self.checked,
            options: self.options,
            index,
            array_moves: std::cell::RefCell::new(ArrayMovePlan::default()),
        };
        emitter.verify_ownership_facts()?;
        emitter.run()
    }
}

// ---------------------------------------------------------------------------
// Small text buffer with indentation.
// ---------------------------------------------------------------------------

struct Buf {
    out: String,
    indent: usize,
}

impl Buf {
    fn new(indent: usize) -> Buf {
        Buf {
            out: String::new(),
            indent,
        }
    }

    fn line(&mut self, text: &str) {
        for _ in 0..self.indent {
            self.out.push_str(INDENT);
        }
        self.out.push_str(text);
        self.out.push('\n');
    }

    fn blank(&mut self) {
        self.out.push('\n');
    }

    fn raw(&mut self, text: &str) {
        self.out.push_str(text);
        if !text.ends_with('\n') {
            self.out.push('\n');
        }
    }

    fn finish(self) -> String {
        self.out
    }
}

// ---------------------------------------------------------------------------
// Item index: Rust paths computed from the AST module tree.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RawKind {
    Fn,
    Struct,
    Enum,
    Const,
}

#[derive(Debug, Clone)]
struct OwnerHint {
    /// Definition item id when the impl self type was resolved to a nominal type.
    def: Option<NodeId>,
    /// Lexical type name used when no definition id is available.
    name_hint: String,
}

#[derive(Debug, Clone)]
struct RawItem {
    id: NodeId,
    name: String,
    kind: RawKind,
    is_test: bool,
    /// How the function returns (`()` versus `Result<(), Error>`).
    ret: RetKind,
    module_ravum: String,
    module_rust: String,
    owner: Option<OwnerHint>,
}

#[derive(Debug, Clone)]
struct ItemEntry {
    rust_name: String,
    rust_path: String,
}

/// How `main` / a test function returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetKind {
    Unit,
    Result,
    Other,
}

#[derive(Debug, Clone)]
struct FnEntry {
    rust_name: String,
    ret: RetKind,
}

#[derive(Debug, Clone)]
struct ModEntry {
    rust_name: String,
    #[allow(dead_code)]
    ravum_path: String,
    #[allow(dead_code)]
    rust_path: String,
}

#[derive(Debug, Default)]
struct Index {
    modules: BTreeMap<NodeId, ModEntry>,
    items: BTreeMap<NodeId, ItemEntry>,
    fns: BTreeMap<NodeId, FnEntry>,
    /// AST variant names (Rust-mangled) per enum item id, in declaration order.
    enum_variants: BTreeMap<NodeId, Vec<String>>,
    /// `(display name, item id)` of the tests, in declaration order.
    tests: Vec<(String, NodeId)>,
    user_main: Option<NodeId>,
    /// Ravum module paths (`""` for the root) used to rewrite `item_paths`.
    module_paths: BTreeSet<String>,
}

impl Index {
    fn build(checked: &CheckedProgram) -> Index {
        let mut index = Index::default();
        let mut raws: Vec<RawItem> = Vec::new();
        index.walk_module(
            checked,
            &checked.program.root,
            "",
            "crate::rvm_root",
            &mut raws,
        );

        // The user `main`: an explicit fact wins, else the first `main` declared
        // in the root module, else the first `main` in declaration order.
        let candidate = checked
            .facts
            .main_fn
            .or_else(|| first_user_main(&raws))
            .filter(|id| {
                raws.iter()
                    .any(|raw| raw.id == *id && raw.kind == RawKind::Fn)
            });
        index.user_main = candidate;

        // Rust names (the user `main` is renamed so the emitted `fn main` is ours).
        let mut rust_names: BTreeMap<NodeId, String> = BTreeMap::new();
        for raw in &raws {
            let name = if Some(raw.id) == index.user_main {
                GENERATED_MAIN.to_string()
            } else {
                mangle_ident(&raw.name)
            };
            rust_names.insert(raw.id, name);
        }

        // Pass A: non-method items (types must exist before methods reference them).
        for raw in raws.iter().filter(|raw| raw.owner.is_none()) {
            let rust_name = rust_names[&raw.id].clone();
            let rust_path = format!("{}::{}", raw.module_rust, rust_name);
            index.items.insert(
                raw.id,
                ItemEntry {
                    rust_name,
                    rust_path,
                },
            );
        }

        // Pass B: inherent methods -> `Type::method`, owned by the type path.
        for raw in raws.iter().filter(|raw| raw.owner.is_some()) {
            let rust_name = rust_names[&raw.id].clone();
            let owner_path = match &raw.owner {
                Some(owner) => match owner.def.and_then(|def| index.items.get(&def)) {
                    Some(entry) => entry.rust_path.clone(),
                    None if owner.name_hint.is_empty() => raw.module_rust.clone(),
                    None => format!("{}::{}", raw.module_rust, mangle_ident(&owner.name_hint)),
                },
                None => raw.module_rust.clone(),
            };
            index.items.insert(
                raw.id,
                ItemEntry {
                    rust_path: format!("{}::{}", owner_path, rust_name),
                    rust_name,
                },
            );
        }

        // Function table (`ret` already classifies `main`/test entry points).
        for raw in raws.iter().filter(|raw| raw.kind == RawKind::Fn) {
            let entry = match index.items.get(&raw.id) {
                Some(entry) => entry,
                None => continue,
            };
            index.fns.insert(
                raw.id,
                FnEntry {
                    rust_name: entry.rust_name.clone(),
                    ret: raw.ret,
                },
            );
        }

        // Tests: `Facts::test_fns` order is authoritative when present.
        if checked.facts.test_fns.is_empty() {
            for raw in raws.iter().filter(|raw| raw.is_test) {
                index.tests.push((test_display_name(raw), raw.id));
            }
        } else {
            for id in &checked.facts.test_fns {
                if let Some(raw) = raws.iter().find(|raw| raw.id == *id) {
                    index.tests.push((test_display_name(raw), raw.id));
                }
            }
        }

        index
    }

    fn walk_module(
        &mut self,
        checked: &CheckedProgram,
        module: &Module,
        ravum_path: &str,
        rust_path: &str,
        raws: &mut Vec<RawItem>,
    ) {
        let rust_name = if ravum_path.is_empty() {
            "rvm_root".to_string()
        } else {
            let own = module
                .name
                .as_ref()
                .map(|ident| ident.name.clone())
                .unwrap_or_default();
            format!("rvm_{}", sanitize_segment(&own))
        };
        self.modules.insert(
            module.id,
            ModEntry {
                rust_name,
                ravum_path: ravum_path.to_string(),
                rust_path: rust_path.to_string(),
            },
        );
        self.module_paths.insert(ravum_path.to_string());

        for item in &module.items {
            match &item.kind {
                ItemKind::Fn(fun) => raws.push(RawItem {
                    id: item.id,
                    name: fun.name.name.clone(),
                    kind: RawKind::Fn,
                    is_test: fun.is_test,
                    ret: ret_kind_of(checked, item.id, fun),
                    module_ravum: ravum_path.to_string(),
                    module_rust: rust_path.to_string(),
                    owner: None,
                }),
                ItemKind::Struct(structure) => raws.push(RawItem {
                    id: item.id,
                    name: structure.name.name.clone(),
                    kind: RawKind::Struct,
                    is_test: false,
                    ret: RetKind::Unit,
                    module_ravum: ravum_path.to_string(),
                    module_rust: rust_path.to_string(),
                    owner: None,
                }),
                ItemKind::Enum(enumeration) => {
                    let variants = enumeration
                        .variants
                        .iter()
                        .map(|variant| mangle_ident(&variant.name.name))
                        .collect::<Vec<_>>();
                    self.enum_variants.insert(item.id, variants);
                    raws.push(RawItem {
                        id: item.id,
                        name: enumeration.name.name.clone(),
                        kind: RawKind::Enum,
                        is_test: false,
                        ret: RetKind::Unit,
                        module_ravum: ravum_path.to_string(),
                        module_rust: rust_path.to_string(),
                        owner: None,
                    });
                }
                ItemKind::Const(constant) => raws.push(RawItem {
                    id: item.id,
                    name: constant.name.name.clone(),
                    kind: RawKind::Const,
                    is_test: false,
                    ret: RetKind::Unit,
                    module_ravum: ravum_path.to_string(),
                    module_rust: rust_path.to_string(),
                    owner: None,
                }),
                ItemKind::Impl(implementation) => {
                    let owner = impl_owner_hint(checked, implementation);
                    for sub in &implementation.items {
                        if let ItemKind::Fn(fun) = &sub.kind {
                            raws.push(RawItem {
                                id: sub.id,
                                name: fun.name.name.clone(),
                                kind: RawKind::Fn,
                                is_test: fun.is_test,
                                ret: ret_kind_of(checked, sub.id, fun),
                                module_ravum: ravum_path.to_string(),
                                module_rust: rust_path.to_string(),
                                owner: Some(owner.clone()),
                            });
                        }
                    }
                }
                ItemKind::Mod(inner) => {
                    let name = inner
                        .name
                        .as_ref()
                        .map(|ident| ident.name.clone())
                        .unwrap_or_default();
                    let child_ravum = if ravum_path.is_empty() {
                        name.clone()
                    } else {
                        format!("{ravum_path}::{name}")
                    };
                    let child_rust = format!("{rust_path}::rvm_{}", sanitize_segment(&name));
                    self.walk_module(checked, inner, &child_ravum, &child_rust, raws);
                }
                ItemKind::Use(_) | ItemKind::ModDecl(_) | ItemKind::Extern(_) => {}
            }
        }
    }
}

fn first_user_main(raws: &[RawItem]) -> Option<NodeId> {
    let candidates: Vec<&RawItem> = raws
        .iter()
        .filter(|raw| {
            raw.kind == RawKind::Fn && !raw.is_test && raw.owner.is_none() && raw.name == "main"
        })
        .collect();
    candidates
        .iter()
        .find(|raw| raw.module_ravum.is_empty())
        .or_else(|| candidates.first())
        .map(|raw| raw.id)
}

fn test_display_name(raw: &RawItem) -> String {
    if raw.module_ravum.is_empty() {
        raw.name.clone()
    } else {
        format!("{}::{}", raw.module_ravum, raw.name)
    }
}

fn impl_owner_hint(checked: &CheckedProgram, implementation: &ImplItem) -> OwnerHint {
    if let Some(Type::Nominal { def, name, .. }) =
        checked.facts.type_nodes.get(&implementation.self_ty.id)
    {
        return OwnerHint {
            def: Some(*def),
            name_hint: name.clone(),
        };
    }
    match &implementation.self_ty.kind {
        TypeKind::Path(path) => OwnerHint {
            def: None,
            name_hint: path
                .last()
                .map(|segment| segment.name.name.clone())
                .unwrap_or_default(),
        },
        _ => OwnerHint {
            def: None,
            name_hint: String::new(),
        },
    }
}

fn classify_ret(ty: &Type) -> RetKind {
    match ty {
        Type::Unit => RetKind::Unit,
        Type::Never => RetKind::Unit,
        Type::Result(ok, _) if **ok == Type::Unit => RetKind::Result,
        Type::Result(_, _) => RetKind::Other,
        _ => RetKind::Other,
    }
}

/// Return classification of a function: resolved type node first, then `FnSig`,
/// then the syntactic shape (fallback for partially filled facts).
fn ret_kind_of(checked: &CheckedProgram, id: NodeId, fun: &FnItem) -> RetKind {
    if let Some(ty) = checked.facts.type_nodes.get(&fun.ret.id) {
        return classify_ret(ty);
    }
    if let Some(sig) = checked.facts.fn_sigs.get(&id) {
        return classify_ret(&sig.ret);
    }
    match &fun.ret.kind {
        TypeKind::Unit => RetKind::Unit,
        TypeKind::Path(path) if path.last().map(|s| s.name.name.as_str()) == Some("Result") => {
            RetKind::Result
        }
        _ => RetKind::Other,
    }
}

// ---------------------------------------------------------------------------
// Identifiers.
// ---------------------------------------------------------------------------

/// Rust 2015/2018/2021 keywords plus reserved words. Ravum identifiers are
/// `[A-Za-z_][A-Za-z0-9_]*` and exclude Ravum keywords, so most of these can
/// only appear through names the Ravum keyword list does not contain (`type`,
/// `ref`, `dyn`, `static`, `box`, ...).
const RUST_KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate",
    "do", "dyn", "else", "enum", "extern", "false", "final", "fn", "for", "if", "impl", "in",
    "let", "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref",
    "return", "self", "Self", "static", "struct", "super", "trait", "true", "try", "type",
    "typeof", "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
];

/// Mangle a Ravum identifier into an always-legal Rust identifier.
///
/// `self`, `Self`, `super`, `crate` and `_` cannot be raw identifiers, so they
/// are renamed deterministically; other keywords use `r#`.
fn mangle_ident(name: &str) -> String {
    if matches!(name, "self" | "Self" | "super" | "crate" | "_" | "") {
        let cleaned = sanitize_segment(name);
        return if cleaned.is_empty() {
            "rv_unnamed".to_string()
        } else {
            format!("rv_kw_{cleaned}")
        };
    }
    if RUST_KEYWORDS.contains(&name) {
        format!("r#{name}")
    } else {
        name.to_string()
    }
}

/// Keep only characters legal in a Rust identifier; used for module segments
/// taken from fact paths.
fn sanitize_segment(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for (position, ch) in name.chars().enumerate() {
        let ok = if position == 0 {
            ch.is_ascii_alphabetic() || ch == '_'
        } else {
            ch.is_ascii_alphanumeric() || ch == '_'
        };
        out.push(if ok { ch } else { '_' });
    }
    out
}

fn indent_text(level: usize) -> String {
    INDENT.repeat(level)
}

// ---------------------------------------------------------------------------
// Literals and escaping.
// ---------------------------------------------------------------------------

fn escape_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            other if (other as u32) < 0x20 || other as u32 == 0x7f => {
                out.push_str(&format!("\\u{{{:x}}}", other as u32));
            }
            other => out.push(other),
        }
    }
    out
}

fn string_literal(text: &str) -> String {
    format!("\"{}\"", escape_string(text))
}

fn escape_char(ch: char) -> String {
    match ch {
        '\\' => "\\\\".to_string(),
        '\'' => "\\'".to_string(),
        '\n' => "\\n".to_string(),
        '\r' => "\\r".to_string(),
        '\t' => "\\t".to_string(),
        '\0' => "\\0".to_string(),
        other if (other as u32) < 0x20 || other as u32 == 0x7f => {
            format!("\\u{{{:x}}}", other as u32)
        }
        other => other.to_string(),
    }
}

fn char_literal(ch: char) -> String {
    format!("'{}'", escape_char(ch))
}

fn int_type_name(ty: &Type) -> Option<&'static str> {
    Some(match ty {
        Type::I8 => "i8",
        Type::I16 => "i16",
        Type::I32 => "i32",
        Type::I64 => "i64",
        Type::I128 => "i128",
        Type::U8 => "u8",
        Type::U16 => "u16",
        Type::U32 => "u32",
        Type::U64 => "u64",
        Type::U128 => "u128",
        Type::Isize => "isize",
        Type::Usize => "usize",
        _ => return None,
    })
}

fn builtin_type_name(name: &str) -> Option<&'static str> {
    Some(match name {
        "i8" => "i8",
        "i16" => "i16",
        "i32" => "i32",
        "i64" => "i64",
        "i128" => "i128",
        "u8" => "u8",
        "u16" => "u16",
        "u32" => "u32",
        "u64" => "u64",
        "u128" => "u128",
        "isize" => "isize",
        "usize" => "usize",
        "f32" => "f32",
        "f64" => "f64",
        "bool" => "bool",
        "char" => "char",
        "String" => "String",
        "str" => "str",
        "Vec" => "Vec",
        "Option" => "Option",
        "Result" => "Result",
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Builtin stdlib mapping.
// ---------------------------------------------------------------------------

/// A free function of the Ravum standard library and how it is emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FreeBuiltin {
    Println,
    Print,
    Eprintln,
    Eprint,
    ReadLine,
    Args,
    EnvVar,
    FsReadToString,
    FsWriteString,
    FsExists,
    Exit,
    Panic,
    Assert,
    AssertEq,
    Drop,
    StringFrom,
    ErrorNew,
    /// Emit `path(args)` verbatim: associated constructors of prelude types
    /// (`Vec::new`, `Vec::with_capacity`, `String::new`).
    RustPath(&'static str),
    ParseI64,
    ParseF64,
    MathUnary(&'static str),
    MathBinary(&'static str),
    /// `Option::Some` / `Result::Ok` ... (variant constructors that arrive as
    /// builtin free functions because `Option`/`Result` are prelude types).
    VariantCtor,
}

fn builtin_free(path: &str) -> Option<FreeBuiltin> {
    let path = path.strip_prefix("std::").unwrap_or(path);
    Some(match path {
        "io::println" | "println" => FreeBuiltin::Println,
        "io::print" | "print" => FreeBuiltin::Print,
        "io::eprintln" | "eprintln" => FreeBuiltin::Eprintln,
        "io::eprint" | "eprint" => FreeBuiltin::Eprint,
        "io::read_line" | "read_line" => FreeBuiltin::ReadLine,
        "env::args" | "args" => FreeBuiltin::Args,
        "env::var" | "env_var" => FreeBuiltin::EnvVar,
        "fs::read_to_string" | "read_to_string" => FreeBuiltin::FsReadToString,
        "fs::write_string" | "write_string" => FreeBuiltin::FsWriteString,
        "fs::exists" | "exists" => FreeBuiltin::FsExists,
        "process::exit" | "exit" => FreeBuiltin::Exit,
        "panic::panic" | "panic" => FreeBuiltin::Panic,
        "panic::assert" | "assert" => FreeBuiltin::Assert,
        "panic::assert_eq" | "assert_eq" => FreeBuiltin::AssertEq,
        "mem::drop" | "drop" => FreeBuiltin::Drop,
        "string::from" | "String::from" | "str::from" => FreeBuiltin::StringFrom,
        "Error::new" | "error::new" => FreeBuiltin::ErrorNew,
        "Vec::new" | "vec::new" => FreeBuiltin::RustPath("Vec::new"),
        "Vec::with_capacity" | "vec::with_capacity" => FreeBuiltin::RustPath("Vec::with_capacity"),
        "String::new" | "string::new" => FreeBuiltin::RustPath("String::new"),
        "string::parse_i64" | "parse_i64" | "str::parse_i64" => FreeBuiltin::ParseI64,
        "string::parse_f64" | "parse_f64" | "str::parse_f64" => FreeBuiltin::ParseF64,
        "math::abs" | "abs" => FreeBuiltin::MathUnary("abs"),
        "math::sqrt" | "sqrt" => FreeBuiltin::MathUnary("sqrt"),
        "math::floor" | "floor" => FreeBuiltin::MathUnary("floor"),
        "math::ceil" | "ceil" => FreeBuiltin::MathUnary("ceil"),
        "math::round" | "round" => FreeBuiltin::MathUnary("round"),
        "math::sin" | "sin" => FreeBuiltin::MathUnary("sin"),
        "math::cos" | "cos" => FreeBuiltin::MathUnary("cos"),
        "math::tan" | "tan" => FreeBuiltin::MathUnary("tan"),
        "math::exp" | "exp" => FreeBuiltin::MathUnary("exp"),
        "math::ln" | "ln" => FreeBuiltin::MathUnary("ln"),
        "math::log10" | "log10" => FreeBuiltin::MathUnary("log10"),
        "math::pow" | "pow" => FreeBuiltin::MathBinary("powf"),
        "math::min" | "min" => FreeBuiltin::MathBinary("min"),
        "math::max" | "max" => FreeBuiltin::MathBinary("max"),
        _ => {
            if is_option_or_result_ctor(path) {
                FreeBuiltin::VariantCtor
            } else {
                return None;
            }
        }
    })
}

/// `Option::Some`, `Result::Ok`, `Some`, `Ok` ... (unqualified accepted).
fn is_option_or_result_ctor(path: &str) -> bool {
    let segments: Vec<&str> = path.split("::").collect();
    let Some(last) = segments.last() else {
        return false;
    };
    let accepted = ["Some", "None", "Ok", "Err"];
    if !accepted.contains(last) {
        return false;
    }
    segments.len() == 1
        || matches!(
            segments.get(segments.len() - 2).copied(),
            Some("Option") | Some("Result")
        )
}

fn builtin_const(path: &str) -> Option<String> {
    let path = path.strip_prefix("std::").unwrap_or(path);
    let direct = match path {
        "math::PI" | "math::pi" => Some("std::f64::consts::PI"),
        "math::E" | "math::e" => Some("std::f64::consts::E"),
        "math::TAU" | "math::tau" => Some("std::f64::consts::TAU"),
        // `Option::None` is a constant, not a constructor call: the canonical
        // representation decided by the checker.
        "Option::None" | "None" => Some("None"),
        _ => None,
    };
    if let Some(found) = direct {
        return Some(found.to_string());
    }
    // `<primitive>::MIN` / `MAX` / `EPSILON` ... are already valid Rust paths.
    let mut segments = path.split("::");
    let owner = segments.next()?;
    let member = segments.next()?;
    if segments.next().is_some() {
        return None;
    }
    let owner_ok = builtin_type_name(owner).is_some();
    let member_ok = matches!(
        member,
        "MIN" | "MAX" | "EPSILON" | "NAN" | "INFINITY" | "NEG_INFINITY" | "DIGITS" | "BITS"
    );
    if owner_ok && member_ok {
        Some(format!("{owner}::{member}"))
    } else {
        None
    }
}

/// Builtin methods that need more than a direct `.name(args)` call.
fn builtin_method_special(owner: &str, method: &str, receiver: &str, args: &str) -> Option<String> {
    let rendered = match (owner, method) {
        (_, "to_string") => format!("format!(\"{{}}\", {receiver})"),
        // `str::as_str` (STD-11.4) is the identity on a text reference: the
        // receiver is already a `&str`, and Rust's `str` has no such method.
        ("str", "as_str") => format!("({receiver})"),
        ("String", "trim") | ("str", "trim") => format!("{receiver}.trim().to_string()"),
        ("String", "lines") | ("str", "lines") => {
            format!("{receiver}.lines().map(|rv_line| rv_line.to_string()).collect::<Vec<String>>()")
        }
        ("String", "parse_i64") | ("str", "parse_i64") => format!(
            "{receiver}.trim().parse::<i64>().map_err(|rv_error| crate::Error::new(&rv_error.to_string()))"
        ),
        ("String", "parse_f64") | ("str", "parse_f64") => format!(
            "{receiver}.trim().parse::<f64>().map_err(|rv_error| crate::Error::new(&rv_error.to_string()))"
        ),
        _ => return None,
    };
    if args.is_empty() {
        Some(rendered)
    } else {
        // These special forms take no argument in Core 1.0.
        Some(rendered)
    }
}

/// Rust method name that exists on a builtin owner, exactly as listed by the
/// Ravum Core 1.0 stdlib. Anything else is reported as an explicit
/// [`CodegenError`]: the generator never guesses a Rust method name.
fn builtin_method_is_direct(owner: &str, method: &str) -> bool {
    const STRING: &[&str] = &[
        "len",
        "is_empty",
        "as_str",
        "push_str",
        "push",
        "clone",
        "to_uppercase",
        "to_lowercase",
        "contains",
        "replace",
        "starts_with",
        "ends_with",
    ];
    const STR: &[&str] = &[
        "len",
        "is_empty",
        "to_uppercase",
        "to_lowercase",
        "contains",
        "replace",
        "starts_with",
        "ends_with",
    ];
    const VEC: &[&str] = &[
        "len", "is_empty", "push", "pop", "clear", "clone", "as_slice",
    ];
    const SLICE: &[&str] = &["len", "is_empty"];
    const OPTION: &[&str] = &["is_some", "is_none", "unwrap", "unwrap_or", "ok_or"];
    const RESULT: &[&str] = &["is_ok", "is_err", "unwrap", "unwrap_or", "ok", "err"];
    const FLOAT: &[&str] = &[
        "abs", "sqrt", "floor", "ceil", "round", "powf", "min", "max", "sin", "cos", "tan", "exp",
        "ln", "log10",
    ];
    const SIGNED: &[&str] = &[
        "abs",
        // RFC-RT-TRAPS-1 explicit forms (Rust methods exist on every integer
        // type; `checked_*` returns `Option`, which the type checker enforced).
        "wrapping_add",
        "wrapping_sub",
        "wrapping_mul",
        "wrapping_neg",
        "saturating_add",
        "saturating_sub",
        "saturating_mul",
        "saturating_neg",
        "checked_add",
        "checked_sub",
        "checked_mul",
        "checked_div",
        "checked_rem",
        "checked_neg",
    ];
    match owner {
        "String" => STRING.contains(&method),
        "str" => STR.contains(&method),
        "Vec" => VEC.contains(&method),
        "slice" | "Slice" | "[T]" => SLICE.contains(&method),
        "Option" => OPTION.contains(&method),
        "Result" => RESULT.contains(&method),
        "f32" | "f64" => FLOAT.contains(&method),
        "int" | "i8" | "i16" | "i32" | "i64" | "i128" | "isize" => SIGNED.contains(&method),
        _ => false,
    }
}

/// Split a canonical builtin method path into `(owner, method)`.
fn split_builtin_method(path: &str) -> Option<(&str, &str)> {
    let path = path.strip_prefix("std::").unwrap_or(path);
    let segments: Vec<&str> = path.split("::").filter(|s| !s.is_empty()).collect();
    if segments.len() < 2 {
        return None;
    }
    let method = segments[segments.len() - 1];
    let owner = segments[segments.len() - 2];
    Some((owner, method))
}

/// Normalize `&String`, `&mut Vec<T>`, `Vec<T>` ... to the bare builtin owner.
fn normalize_owner(owner: &str) -> String {
    let mut text = owner.trim();
    if let Some(rest) = text.strip_prefix('&') {
        text = rest.trim();
    }
    if let Some(rest) = text.strip_prefix("mut ") {
        text = rest.trim();
    }
    if let Some(position) = text.find('<') {
        text = &text[..position];
    }
    text.trim().to_string()
}

// ---------------------------------------------------------------------------
// Emitter.
// ---------------------------------------------------------------------------

struct Emitter<'a> {
    checked: &'a CheckedProgram,
    options: &'a GenOptions,
    index: Index,
    /// Array-element moves planned for the function body being emitted (see
    /// [`Emitter::plan_array_moves`]).
    array_moves: std::cell::RefCell<ArrayMovePlan>,
}

impl<'a> Emitter<'a> {
    /// Step B of RFC-OWN-FACTS-1 (debug builds only): compare the backend's
    /// copy/move classification with the checker's `OwnFacts`.
    ///
    /// The emitter mirrors `sema::own::is_copy_type` in [`Emitter::is_copy_type`]
    /// — the duplication this RFC removes. In debug builds a disagreement is an
    /// error instead of a silently different lowering. Release builds skip the
    /// check (it is a test/audit guard, not a semantic rule).
    fn verify_ownership_facts(&self) -> Result<(), CodegenError> {
        if !cfg!(debug_assertions) {
            return Ok(());
        }
        for (id, kind) in &self.checked.facts.own.use_kind {
            let Some(ty) = self.checked.facts.expr_types.get(id) else {
                continue;
            };
            let copies = self.is_copy_type(ty);
            match kind {
                ravum_lang_sema::facts::UseKind::Copy if !copies => {
                    return Err(err(format!(
                        "ownership facts disagree with the backend for expression {id}: the \
                         checker says Copy, the emitter classifies `{ty:?}` as mobile"
                    )));
                }
                ravum_lang_sema::facts::UseKind::Move if copies => {
                    return Err(err(format!(
                        "ownership facts disagree with the backend for expression {id}: the \
                         checker says Move, the emitter classifies `{ty:?}` as Copy"
                    )));
                }
                _ => {}
            }
        }
        // Step B (extended): the recorded place must match the place the syntax
        // describes. The binding identity is not re-derived (the AST does not
        // carry the declaration span of a resolved local), but every projection
        // (`x.f`, `.0`, `[lit]`, `[*]`, deref) is checked independently.
        let mut exprs: BTreeMap<NodeId, &Expr> = BTreeMap::new();
        let mut method_receivers: BTreeSet<NodeId> = BTreeSet::new();
        collect_module_exprs(
            &self.checked.program.root,
            &mut exprs,
            &mut method_receivers,
        );
        let mut reference_args: BTreeSet<NodeId> = BTreeSet::new();
        for expr in exprs.values() {
            if let ExprKind::Call { args, .. } | ExprKind::MethodCall { args, .. } = &expr.kind {
                for arg in args {
                    if matches!(
                        self.checked.facts.expr_types.get(&arg.id),
                        Some(Type::Ref { .. })
                    ) {
                        reference_args.insert(arg.id);
                    }
                }
            }
        }
        for (id, recorded) in &self.checked.facts.own.place {
            let Some(expr) = exprs.get(id) else {
                continue;
            };
            // A method receiver is auto-deref'd by the call (`xs.len()` records
            // `(*xs)`) and so is a reference-typed argument reborrowed for a
            // reference parameter (`g(xs)` with `xs: &[i32]`). The callee
            // signature is not part of that expression's syntax, so the adjusted
            // place is accepted alongside the syntactic one; any projection
            // beyond the implicit deref must still match.
            let auto_deref = method_receivers.contains(id) || reference_args.contains(id);
            match self.syntactic_place(expr) {
                Some(expected) => {
                    let mut expected_after_deref = expected.clone();
                    if auto_deref {
                        self.push_auto_derefs(expr, &mut expected_after_deref.steps);
                    }
                    if recorded == &expected || (auto_deref && recorded == &expected_after_deref) {
                        continue;
                    }
                    // Binding identity first: a place rooted at another binding
                    // is not a projection mismatch.
                    if recorded.binding != expected.binding {
                        return Err(err(format!(
                            "ownership facts disagree with the syntax for expression {id} ({}) at \
                             {}: recorded binding {:?}, syntactic binding {:?}",
                            self.expr_location(expr),
                            self.expr_span_text(expr),
                            recorded.binding,
                            expected.binding
                        )));
                    }
                    let suffix = if auto_deref {
                        format!(", after auto-deref {:?}", expected_after_deref.steps)
                    } else {
                        String::new()
                    };
                    return Err(err(format!(
                        "ownership facts disagree with the syntax for expression {id} ({}) at \
                         {}: recorded place steps {:?}, syntactic place {:?}{suffix}",
                        self.expr_location(expr),
                        self.expr_span_text(expr),
                        recorded.steps,
                        expected.steps
                    )));
                }
                None => {
                    return Err(err(format!(
                        "ownership facts recorded a place for expression {id} ({}) at {}, but \
                         the expression has no syntactic place",
                        self.expr_location(expr),
                        self.expr_span_text(expr)
                    )));
                }
            }
        }
        Ok(())
    }

    /// Place described by the syntax alone, binding identity included (step B
    /// cross-check). See `OwnPlace`.
    fn syntactic_place(&self, expr: &Expr) -> Option<ravum_lang_sema::facts::OwnPlace> {
        use ravum_lang_sema::facts::{OwnPlace, Projection as PlaceStep, ProjectionIndex};
        match &expr.kind {
            ExprKind::SelfValue => match self.checked.facts.resolutions.get(&expr.id) {
                Some(Resolution::Local { decl, .. }) => Some(OwnPlace {
                    binding: *decl,
                    steps: Vec::new(),
                }),
                _ => None,
            },
            ExprKind::Path(path) if path.segments.len() == 1 => {
                match self.checked.facts.resolutions.get(&path.id) {
                    Some(Resolution::Local { decl, .. }) => Some(OwnPlace {
                        binding: *decl,
                        steps: Vec::new(),
                    }),
                    _ => None,
                }
            }
            ExprKind::Field { base, name } => {
                let mut place = self.syntactic_place(base)?;
                self.push_auto_derefs(base, &mut place.steps);
                place.steps.push(PlaceStep::Field(name.name.clone()));
                Some(place)
            }
            ExprKind::TupleIndex { base, index } => {
                let mut place = self.syntactic_place(base)?;
                self.push_auto_derefs(base, &mut place.steps);
                place.steps.push(PlaceStep::Tuple(*index));
                Some(place)
            }
            ExprKind::Index { base, index } => {
                let mut place = self.syntactic_place(base)?;
                self.push_auto_derefs(base, &mut place.steps);
                let index = match &index.kind {
                    ExprKind::Int { value, .. } => ProjectionIndex::Lit(*value),
                    _ => ProjectionIndex::Opaque,
                };
                place.steps.push(PlaceStep::Index(index));
                Some(place)
            }
            ExprKind::Unary {
                op: UnOp::Deref,
                expr,
            } => {
                let mut place = self.syntactic_place(expr)?;
                place.steps.push(PlaceStep::Deref);
                Some(place)
            }
            _ => None,
        }
    }

    /// Human location of an expression for a debug-guard error message.
    fn expr_location(&self, expr: &Expr) -> String {
        match self.checked.files.get(expr.span.file as usize) {
            Some(file) => format!("{}:{}", file.path, file.line_of(expr.span.start)),
            None => "unknown-file".to_string(),
        }
    }

    fn expr_span_text(&self, expr: &Expr) -> String {
        format!("{}..{}", expr.span.start, expr.span.end)
    }

    /// A field/tuple/index projection on a reference auto-derefs it: the checker
    /// records one `Deref` step per reference layer before the projection.
    fn push_auto_derefs(&self, base: &Expr, steps: &mut Vec<ravum_lang_sema::facts::Projection>) {
        let mut ty = self.checked.facts.expr_types.get(&base.id);
        while let Some(Type::Ref { inner, .. }) = ty {
            steps.push(ravum_lang_sema::facts::Projection::Deref);
            ty = Some(inner);
        }
    }

    /// RFC-OWN-FACTS-1 step C: does the checker take the value of `node`?
    ///
    /// `own.rs` records `UseKind::Move` when it moves a place, `UseKind::Copy`
    /// when it reads a `Copy` one, and `Borrow`/`BorrowMut` when it only borrows
    /// it. "The value is taken" is therefore exactly `Move | Copy`; the backend
    /// reads that decision instead of re-deriving it from types or from the
    /// method signature, and it does so in **release builds too** (this helper
    /// is not behind `debug_assertions`: for the migrated families there is no
    /// second decision left to compare).
    ///
    /// An expression the checker recorded nothing for is not a place it took a
    /// decision on (a call result, a literal, an aggregate): it is not consumed
    /// here, and every caller already handles that case separately.
    fn consumed_by_checker(&self, node: NodeId) -> bool {
        matches!(
            self.facts().own.use_kind.get(&node),
            Some(UseKind::Move) | Some(UseKind::Copy)
        )
    }

    fn facts(&self) -> &ravum_lang_sema::facts::Facts {
        &self.checked.facts
    }

    fn program_name(&self) -> String {
        let name = self.options.crate_name.trim();
        if name.is_empty() {
            "ravum_program".to_string()
        } else {
            name.to_string()
        }
    }

    fn run(&self) -> Result<String, CodegenError> {
        let mut buf = Buf::new(0);
        buf.line(&format!(
            "// Generated by ravum-lang {} from {}; do not edit.",
            VERSION,
            self.program_name()
        ));
        buf.line(&format!(
            "// Ravum edition {} — reference backend B (auditable, safe Rust).",
            ravum_lang_syntax::LANGUAGE_EDITION
        ));
        if let Some(first) = self.checked.files.first() {
            buf.line(&format!("// rv source: {}", first.path));
        }
        buf.line("#![allow(dead_code, unused_variables, unused_mut, unused_assignments, unused_imports, unused_parens, unused_braces, unused_labels, unused_must_use, unused_comparisons, non_snake_case, non_camel_case_types, non_upper_case_globals, unreachable_code, unreachable_patterns, arithmetic_overflow, unconditional_panic)]");
        if self.program_uses_extern() {
            // FFI programs need `unsafe` for the generated extern wrappers only.
            // `deny` (not `allow`) keeps the guarantee "zero unsafe outside the
            // generated wrappers": each wrapper carries a local
            // `#[allow(unsafe_code)]`, and any other `unsafe` would fail the
            // build. A program without `extern` keeps `forbid`.
            buf.line("#![deny(unsafe_code)]");
        } else {
            buf.line("#![forbid(unsafe_code)]");
        }
        buf.blank();
        buf.raw(PRELUDE);
        buf.blank();
        self.emit_entry(&mut buf)?;
        buf.blank();
        self.emit_module(&mut buf, &self.checked.program.root)?;
        Ok(buf.finish())
    }

    // -- entry point -------------------------------------------------------

    fn emit_entry(&self, buf: &mut Buf) -> Result<(), CodegenError> {
        if self.options.test_mode {
            return self.emit_test_entry(buf);
        }

        buf.line("fn main() -> std::process::ExitCode {");
        buf.indent += 1;
        buf.line("crate::rv_install_panic_hook();");
        match self.index.user_main {
            Some(id) => {
                let path = self.item_path(id)?;
                let ret = match self.index.fns.get(&id) {
                    Some(entry) => entry.ret,
                    None => RetKind::Unit,
                };
                match ret {
                    RetKind::Unit => {
                        buf.line(&format!("{}();", path));
                        buf.line("std::process::ExitCode::SUCCESS");
                    }
                    RetKind::Result => {
                        buf.line(&format!("match {}() {{", path));
                        buf.indent += 1;
                        buf.line("Ok(()) => std::process::ExitCode::SUCCESS,");
                        buf.line("Err(rv_error) => {");
                        buf.indent += 1;
                        buf.line("eprintln!(\"error: {}\", rv_error.message);");
                        buf.line("std::process::ExitCode::from(1)");
                        buf.indent -= 1;
                        buf.line("}");
                        buf.indent -= 1;
                        buf.line("}");
                    }
                    RetKind::Other => {
                        return Err(err(format!(
                            "`main` must return `()` or `Result<(), Error>` (function node {id})"
                        )));
                    }
                }
            }
            None => {
                buf.line("eprintln!(\"error: this Ravum program has no `main` function\");");
                buf.line("std::process::ExitCode::from(1)");
            }
        }
        buf.indent -= 1;
        buf.line("}");
        Ok(())
    }

    fn emit_test_entry(&self, buf: &mut Buf) -> Result<(), CodegenError> {
        buf.line("fn main() -> std::process::ExitCode {");
        buf.indent += 1;
        buf.line("crate::rv_install_test_panic_hook();");
        // `ravum-lang test --filter X` forwards `--filter X` to this runner
        // (`args.rs` documents that contract). The runner honours it itself:
        // a test is selected when its name contains the filter text, like
        // rustc's libtest `--filter`.
        buf.line("let rv_filter: Option<String> = {");
        buf.indent += 1;
        buf.line("let mut rv_arguments = std::env::args().skip(1);");
        buf.line("let mut rv_filter: Option<String> = None;");
        buf.line("while let Some(rv_argument) = rv_arguments.next() {");
        buf.indent += 1;
        buf.line("if let Some(rv_text) = rv_argument.strip_prefix(\"--filter=\") {");
        buf.indent += 1;
        buf.line("rv_filter = Some(rv_text.to_string());");
        buf.indent -= 1;
        buf.line("} else if rv_argument == \"--filter\" {");
        buf.indent += 1;
        buf.line("rv_filter = rv_arguments.next();");
        buf.indent -= 1;
        buf.line("}");
        buf.indent -= 1;
        buf.line("}");
        buf.line("rv_filter");
        buf.indent -= 1;
        buf.line("};");
        buf.line(
            "let rv_matches = |rv_name: &str| match &rv_filter { \
             Some(rv_text) => rv_name.contains(rv_text.as_str()), None => true };",
        );
        buf.line("let mut rv_selected: usize = 0;");
        for (name, _id) in &self.index.tests {
            buf.line(&format!(
                "if rv_matches({}) {{ rv_selected += 1; }}",
                string_literal(name)
            ));
        }
        buf.line("println!(\"running {} tests\", rv_selected);");
        buf.line("let mut rv_passed: usize = 0;");
        buf.line("let mut rv_failed: usize = 0;");
        buf.line("let mut rv_filtered: usize = 0;");
        for (name, id) in &self.index.tests {
            let path = self.item_path(*id)?;
            let ret = match self.index.fns.get(id) {
                Some(entry) => entry.ret,
                None => RetKind::Unit,
            };
            let runner = match ret {
                RetKind::Unit => "crate::rv_test",
                RetKind::Result => "crate::rv_test_result",
                RetKind::Other => {
                    return Err(err(format!(
                        "test function `{name}` must return `()` or `Result<(), Error>` (function node {id})"
                    )));
                }
            };
            let name = string_literal(name);
            buf.line(&format!(
                "if rv_matches({name}) {{ if {runner}({name}, {path}) {{ rv_passed += 1; }} \
                 else {{ rv_failed += 1; }} }} else {{ rv_filtered += 1; }}"
            ));
        }
        // Core 1.0 has no `#[ignore]` attribute, so the ignored count is always 0;
        // the field stays in the line so a consumer can parse it unconditionally.
        buf.line("let rv_ignored: usize = 0;");
        buf.line("println!();");
        buf.line(
            "println!(\"test result: {}; {} passed; {} failed; {} ignored; {} filtered out\", \
             if rv_failed == 0 { \"ok\" } else { \"FAILED\" }, rv_passed, rv_failed, rv_ignored, \
             rv_filtered);",
        );
        buf.line(
            "if rv_failed == 0 { std::process::ExitCode::SUCCESS } else { std::process::ExitCode::from(1) }",
        );
        buf.indent -= 1;
        buf.line("}");
        Ok(())
    }

    // -- items -------------------------------------------------------------

    fn emit_module(&self, buf: &mut Buf, module: &Module) -> Result<(), CodegenError> {
        let entry = self.index.modules.get(&module.id).ok_or_else(|| {
            err(format!(
                "module node {} is missing from the index",
                module.id
            ))
        })?;
        self.marker(buf, module.span);
        buf.line(&format!("pub mod {} {{", entry.rust_name));
        buf.indent += 1;
        for item in &module.items {
            self.emit_item(buf, item)?;
        }
        buf.indent -= 1;
        buf.line("}");
        Ok(())
    }

    /// `true` when the program declares at least one `extern "C"` block. Such a
    /// program needs the generated wrappers' `unsafe` block, so the file-level
    /// `forbid(unsafe_code)` is relaxed to `allow(unsafe_code)`; the generator
    /// still never copies `unsafe` from Ravum source.
    fn program_uses_extern(&self) -> bool {
        fn scan(module: &Module) -> bool {
            module.items.iter().any(|item| match &item.kind {
                ItemKind::Extern(_) => true,
                ItemKind::Mod(inner) => scan(inner),
                _ => false,
            })
        }
        scan(&self.checked.program.root)
    }

    /// Emits one `extern "C"` declaration plus a safe wrapper per function.
    ///
    /// The wrapper is the only place where generated code uses `unsafe`; Ravum
    /// call sites go through the wrapper and remain safe Rust. `link_name`
    /// binds the Ravum declaration to the real C symbol, while the Rust-side
    /// identifier is derived from the node id to avoid collisions.
    fn emit_extern_block(&self, buf: &mut Buf, block: &ExternBlock) -> Result<(), CodegenError> {
        for f in &block.fns {
            let symbol = self
                .facts()
                .extern_symbols
                .get(&f.id)
                .ok_or_else(|| err(format!("missing extern symbol for node {}", f.id)))?;
            // Defence in depth: the checker already rejects non-FFI-safe
            // signatures (`E-FFI-TYPE`), but the generator must never emit an
            // ABI-unsafe declaration even when it is driven from a hand-built
            // `CheckedProgram` (audit finding F0b).
            let mut decls = Vec::new();
            let mut args = Vec::new();
            for param in &f.params {
                match &param.kind {
                    ParamKind::Binding { name, ty, .. } => {
                        if !ast_ffi_safe(ty) {
                            return Err(err(format!(
                                "extern function `{}` parameter `{}` has a non-FFI-safe type",
                                f.name.name, name.name
                            )));
                        }
                        let name = mangle_ident(&name.name);
                        // Foreign declarations take a plain pattern (Rust forbids
                        // `mut` there) and keep the elided text lifetime: a `&str`
                        // argument is a view valid for the call (STD-9.3).
                        decls.push(format!(
                            "{name}: {}",
                            self.type_node_pos(ty, TypePos::Param)?
                        ));
                        args.push(name);
                    }
                    ParamKind::Receiver(_) => {
                        return Err(err(format!(
                            "`self` is not allowed in an extern function (node {})",
                            f.id
                        )));
                    }
                }
            }
            if !ast_ffi_safe(&f.ret) {
                return Err(err(format!(
                    "extern function `{}` returns a non-FFI-safe type",
                    f.name.name
                )));
            }
            let ret = self.type_node(&f.ret)?;
            let ret_suffix = if ret == "()" {
                String::new()
            } else {
                format!(" -> {ret}")
            };
            let params = decls.join(", ");
            buf.line("extern \"C\" {");
            buf.indent += 1;
            buf.line(&format!("#[link_name = {}]", string_literal(symbol)));
            buf.line(&format!("fn rvm_ffi_{}({params}){ret_suffix};", f.id));
            buf.indent -= 1;
            buf.line("}");
            buf.line("#[allow(unsafe_code)]");
            buf.line(&format!(
                "pub fn rvm_ffi_wrap_{}({params}){ret_suffix} {{",
                f.id
            ));
            buf.indent += 1;
            buf.line(&format!(
                "unsafe {{ rvm_ffi_{}({}) }}",
                f.id,
                args.join(", ")
            ));
            buf.indent -= 1;
            buf.line("}");
        }
        Ok(())
    }

    fn emit_item(&self, buf: &mut Buf, item: &Item) -> Result<(), CodegenError> {
        match &item.kind {
            ItemKind::Fn(fun) => self.emit_fn(buf, item, fun),
            ItemKind::Struct(structure) => self.emit_struct(buf, item, structure),
            ItemKind::Enum(enumeration) => self.emit_enum(buf, item, enumeration),
            ItemKind::Impl(implementation) => self.emit_impl(buf, implementation),
            ItemKind::Const(constant) => self.emit_const(buf, item, constant),
            ItemKind::Mod(module) => self.emit_module(buf, module),
            ItemKind::Extern(block) => self.emit_extern_block(buf, block),
            ItemKind::Use(_) => Ok(()),
            ItemKind::ModDecl(declaration) => Err(err(format!(
                "unresolved `mod {};` declaration: the loader must expand file modules before code generation",
                declaration.name.name
            ))),
        }
    }

    fn entry(&self, id: NodeId) -> Result<&ItemEntry, CodegenError> {
        self.index
            .items
            .get(&id)
            .ok_or_else(|| err(format!("item node {id} is missing from the module index")))
    }

    fn emit_fn(&self, buf: &mut Buf, item: &Item, fun: &FnItem) -> Result<(), CodegenError> {
        self.marker(buf, item.span);
        let entry = self.index.fns.get(&item.id).ok_or_else(|| {
            err(format!(
                "function node {} is missing from the index",
                item.id
            ))
        })?;

        let mut signature = String::new();
        // `main` and test functions are called from the generated crate-root
        // entry point, so they must be visible outside their Ravum module.
        let is_entry_point = Some(item.id) == self.index.user_main || fun.is_test;
        if item.vis.is_public() || is_entry_point {
            signature.push_str("pub ");
        }
        signature.push_str("fn ");
        signature.push_str(&entry.rust_name);
        signature.push_str(&self.generics(&fun.generics));
        signature.push('(');
        let mut params = Vec::new();
        for param in &fun.params {
            match &param.kind {
                ParamKind::Receiver(receiver) => params.push(match receiver {
                    Receiver::Shared => "&self".to_string(),
                    Receiver::Exclusive => "&mut self".to_string(),
                    Receiver::ByValue => "self".to_string(),
                }),
                ParamKind::Binding { mutable, name, ty } => {
                    // A `mut` parameter binding is part of the checked program
                    // (the checker only accepts an assignment when the binding
                    // is mutable), so the emitted Rust parameter must carry
                    // `mut` too — otherwise `fn bump(mut x: i32) { x = x + 1; }`
                    // is a check-pass/build-fail (`E0384`).
                    params.push(format!(
                        "{}: {}",
                        if *mutable {
                            format!("mut {}", mangle_ident(&name.name))
                        } else {
                            mangle_ident(&name.name)
                        },
                        self.type_node_pos(ty, TypePos::Param)?
                    ));
                }
            }
        }
        signature.push_str(&params.join(", "));
        signature.push(')');
        let ret = self.type_node(&fun.ret)?;
        if ret != "()" {
            signature.push_str(&format!(" -> {ret}"));
        }
        signature.push(' ');

        // Plan the array-element paths of this body (OWN-5.4) before emitting it:
        // `expr` consults the plan when it reaches a projection or an index.
        *self.array_moves.borrow_mut() = self.plan_array_moves(&fun.body);
        let body = self.block_text(buf.indent, &fun.body)?;
        buf.line(&format!("{signature}{body}"));
        Ok(())
    }

    fn emit_struct(
        &self,
        buf: &mut Buf,
        item: &Item,
        structure: &StructItem,
    ) -> Result<(), CodegenError> {
        self.marker(buf, item.span);
        let entry = self.entry(item.id)?;
        buf.line(&self.derive_attribute(item.id));
        let visibility = if item.vis.is_public() { "pub " } else { "" };
        let generics = self.generics(&structure.generics);
        let head = format!("{visibility}struct {}{generics}", entry.rust_name);
        if structure.fields.is_empty() {
            buf.line(&format!("{head} {{}}"));
            return Ok(());
        }
        buf.line(&format!("{head} {{"));
        buf.indent += 1;
        for field in &structure.fields {
            let field_visibility = if field.vis.is_public() { "pub " } else { "" };
            buf.line(&format!(
                "{field_visibility}{}: {},",
                mangle_ident(&field.name.name),
                self.type_node(&field.ty)?
            ));
        }
        buf.indent -= 1;
        buf.line("}");
        Ok(())
    }

    fn emit_enum(
        &self,
        buf: &mut Buf,
        item: &Item,
        enumeration: &EnumItem,
    ) -> Result<(), CodegenError> {
        self.marker(buf, item.span);
        let entry = self.entry(item.id)?;
        buf.line(&self.derive_attribute(item.id));
        let visibility = if item.vis.is_public() { "pub " } else { "" };
        let generics = self.generics(&enumeration.generics);
        buf.line(&format!(
            "{visibility}enum {}{generics} {{",
            entry.rust_name
        ));
        buf.indent += 1;
        for variant in &enumeration.variants {
            let name = mangle_ident(&variant.name.name);
            match &variant.fields {
                VariantDefFields::Unit => buf.line(&format!("{name},")),
                VariantDefFields::Tuple(types) => {
                    let mut rendered = Vec::new();
                    for ty in types {
                        rendered.push(self.type_node(ty)?);
                    }
                    buf.line(&format!("{name}({}),", rendered.join(", ")));
                }
                VariantDefFields::Struct(fields) => {
                    let mut rendered = Vec::new();
                    for field in fields {
                        rendered.push(format!(
                            "{}: {}",
                            mangle_ident(&field.name.name),
                            self.type_node(&field.ty)?
                        ));
                    }
                    buf.line(&format!("{name} {{ {} }},", rendered.join(", ")));
                }
            }
        }
        buf.indent -= 1;
        buf.line("}");
        Ok(())
    }

    fn emit_impl(&self, buf: &mut Buf, implementation: &ImplItem) -> Result<(), CodegenError> {
        let self_type = self.type_node(&implementation.self_ty)?;
        let generics = self.generics(&implementation.generics);
        buf.line(&format!("impl{generics} {self_type} {{"));
        buf.indent += 1;
        for item in &implementation.items {
            match &item.kind {
                ItemKind::Fn(fun) => self.emit_fn(buf, item, fun)?,
                ItemKind::Const(constant) => self.emit_const(buf, item, constant)?,
                _ => {
                    return Err(err(format!(
                        "unsupported item inside `impl` (item node {})",
                        item.id
                    )));
                }
            }
        }
        buf.indent -= 1;
        buf.line("}");
        Ok(())
    }

    fn emit_const(
        &self,
        buf: &mut Buf,
        item: &Item,
        constant: &ravum_lang_syntax::ast::ConstItem,
    ) -> Result<(), CodegenError> {
        self.marker(buf, item.span);
        let entry = self.entry(item.id)?;
        let visibility = if item.vis.is_public() { "pub " } else { "" };
        let ty = self.type_node(&constant.ty)?;
        let value = match self.facts().consts.get(&item.id) {
            Some(info) => self.const_value(&info.ty, &info.value)?,
            None => self.literal_const_expr(&constant.value)?,
        };
        buf.line(&format!(
            "{visibility}const {}: {ty} = {value};",
            entry.rust_name
        ));
        Ok(())
    }

    fn derive_attribute(&self, id: NodeId) -> String {
        let mut derives = vec!["Debug", "Clone", "PartialEq"];
        let is_copy = self
            .facts()
            .structs
            .get(&id)
            .map(|info| info.is_copy)
            .or_else(|| self.facts().enums.get(&id).map(|info| info.is_copy))
            .unwrap_or(false);
        if is_copy {
            derives.push("Copy");
        }
        format!("#[derive({})]", derives.join(", "))
    }

    fn generics(&self, params: &[GenericParam]) -> String {
        if params.is_empty() {
            return String::new();
        }
        let names: Vec<String> = params
            .iter()
            .map(|param| mangle_ident(&param.name.name))
            .collect();
        format!("<{}>", names.join(", "))
    }

    // -- statements and blocks ---------------------------------------------

    fn block_text(&self, indent: usize, block: &Block) -> Result<String, CodegenError> {
        let mut buf = Buf::new(indent);
        buf.out.push_str("{\n");
        buf.indent += 1;
        for statement in &block.stmts {
            self.emit_stmt(&mut buf, statement)?;
        }
        if let Some(tail) = &block.tail {
            let text = self.expr(tail, buf.indent)?;
            buf.line(text.trim_end_matches('\n'));
        }
        buf.indent -= 1;
        buf.line("}");
        // The caller embeds the block inline (`fn f() {`, `if c {`, `=> {`), so
        // the closing brace must not carry a trailing newline.
        Ok(buf.finish().trim_end_matches('\n').to_string())
    }

    fn emit_stmt(&self, buf: &mut Buf, statement: &Stmt) -> Result<(), CodegenError> {
        self.marker(buf, statement.span);
        match &statement.kind {
            StmtKind::Let(declaration) => {
                let pattern = self.pattern(&declaration.pat)?;
                let annotation = match &declaration.ty {
                    Some(ty) => Some(self.type_node(ty)?),
                    None => match self.facts().pat_types.get(&declaration.pat.id) {
                        Some(resolved) if !matches!(resolved, Type::Never | Type::Error) => {
                            Some(self.fmt_type(resolved)?)
                        }
                        _ => None,
                    },
                };
                let suffix = match &annotation {
                    Some(ty) => format!(": {ty}"),
                    None => String::new(),
                };
                match &declaration.init {
                    Some(init) => {
                        let value = self.expr(init, buf.indent)?;
                        buf.line(&format!(
                            "let {pattern}{suffix} = {};",
                            value.trim_end_matches('\n')
                        ));
                    }
                    None => {
                        if annotation.is_none() {
                            return Err(err(format!(
                                "`let` without initializer requires a type annotation (statement node {})",
                                statement.id
                            )));
                        }
                        buf.line(&format!("let {pattern}{suffix};"));
                    }
                }
                Ok(())
            }
            StmtKind::Expr { expr, semi } => {
                let text = self.expr(expr, buf.indent)?;
                let text = text.trim_end_matches('\n');
                if *semi {
                    buf.line(&format!("{text};"));
                } else {
                    buf.line(text);
                }
                Ok(())
            }
        }
    }

    // -- array-element moves (OWN-5.4 / P0-A.2) ----------------------------

    /// `true` when values of `ty` are copied instead of moved.
    ///
    /// This mirrors `sema::own::is_copy_type` (OWN-2.1) so the emitter can tell
    /// a *copy read* from a *move* when it lowers `a[i]`. A mismatch is harmless
    /// in both directions: calling a Copy type mobile only moves a value with no
    /// observable drop, and calling a mobile type Copy leaves the plain `a[i]`
    /// that rustc rejects loudly — never a silent semantic change.
    fn is_copy_type(&self, ty: &Type) -> bool {
        match ty {
            Type::I8
            | Type::I16
            | Type::I32
            | Type::I64
            | Type::I128
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64
            | Type::U128
            | Type::Isize
            | Type::Usize
            | Type::F32
            | Type::F64
            | Type::Bool
            | Type::Char
            | Type::Unit
            | Type::Never
            | Type::Str
            | Type::Ref { .. } => true,
            Type::Array { elem, .. } => self.is_copy_type(elem),
            Type::Tuple(items) => items.iter().all(|item| self.is_copy_type(item)),
            Type::Option(inner) => self.is_copy_type(inner),
            Type::Result(ok, error) => self.is_copy_type(ok) && self.is_copy_type(error),
            Type::Nominal { def, .. } => self
                .facts()
                .structs
                .get(def)
                .map(|info| info.is_copy)
                .or_else(|| self.facts().enums.get(def).map(|info| info.is_copy))
                .unwrap_or(false),
            Type::String | Type::Vec(_) | Type::Slice(_) | Type::Param { .. } | Type::Error => {
                false
            }
        }
    }

    fn is_owned_position(&self, expr: &Expr) -> bool {
        self.facts()
            .expr_types
            .get(&expr.id)
            .map(|ty| !self.is_copy_type(ty))
            .unwrap_or(false)
    }

    // -- expressions -------------------------------------------------------

    fn is_atomic(kind: &ExprKind) -> bool {
        matches!(
            kind,
            ExprKind::Unit
                | ExprKind::Int { .. }
                | ExprKind::Float { .. }
                | ExprKind::Bool(_)
                | ExprKind::Char(_)
                | ExprKind::Str(_)
                | ExprKind::SelfValue
                | ExprKind::Path(_)
                | ExprKind::Call { .. }
                | ExprKind::MethodCall { .. }
                | ExprKind::Field { .. }
                | ExprKind::TupleIndex { .. }
                | ExprKind::Index { .. }
                | ExprKind::Tuple(_)
                | ExprKind::Array(_)
                | ExprKind::ArrayRepeat { .. }
                | ExprKind::Try(_)
        )
    }

    /// Wrap in parentheses when the expression cannot be used as a prefix or
    /// postfix operand as-is. Extra parentheses are always semantically neutral.
    fn operand(&self, expr: &Expr, indent: usize) -> Result<String, CodegenError> {
        let text = self.expr(expr, indent)?;
        if Self::is_atomic(&expr.kind) {
            Ok(text)
        } else {
            Ok(format!("({text})"))
        }
    }

    fn expr(&self, expr: &Expr, indent: usize) -> Result<String, CodegenError> {
        match &expr.kind {
            ExprKind::Unit => Ok("()".to_string()),
            ExprKind::Int { value, suffix } => {
                Ok(self.int_literal_text(*value, *suffix, self.facts().expr_types.get(&expr.id)))
            }
            ExprKind::Float { text, suffix } => {
                Ok(self.float_literal(*suffix, text, self.facts().expr_types.get(&expr.id)))
            }
            ExprKind::Bool(value) => Ok(if *value { "true" } else { "false" }.to_string()),
            ExprKind::Char(value) => Ok(char_literal(*value)),
            ExprKind::Str(value) => Ok(string_literal(value)),
            ExprKind::SelfValue => Ok("self".to_string()),
            ExprKind::Path(path) => self.value_path(path),
            ExprKind::Call { callee, args } => self.call_expr(expr, callee, args, indent),
            ExprKind::MethodCall {
                receiver,
                method,
                args,
            } => self.method_call_expr(expr, receiver, method, args, indent),
            ExprKind::Field { base, name } => {
                if let Some(text) = self.element_path_expr(expr, indent)? {
                    return Ok(text);
                }
                Ok(format!(
                    "{}.{}",
                    self.operand(base, indent)?,
                    mangle_ident(&name.name)
                ))
            }
            ExprKind::TupleIndex { base, index } => {
                if let Some(text) = self.element_path_expr(expr, indent)? {
                    return Ok(text);
                }
                Ok(format!("{}.{}", self.operand(base, indent)?, index))
            }
            ExprKind::Index { base, index } => {
                if let Some(text) = self.element_path_expr(expr, indent)? {
                    return Ok(text);
                }
                Ok(format!(
                    "{}[{}]",
                    self.operand(base, indent)?,
                    self.expr(index, indent)?
                ))
            }
            ExprKind::Unary { op, expr: inner } => {
                let operand = self.operand(inner, indent)?;
                Ok(match op {
                    UnOp::Neg => format!("-{operand}"),
                    UnOp::Not => format!("!{operand}"),
                    UnOp::Ref => format!("&{operand}"),
                    UnOp::RefMut => format!("&mut {operand}"),
                    UnOp::Deref => format!("*{operand}"),
                })
            }
            ExprKind::Binary { op, lhs, rhs } => Ok(format!(
                "{} {} {}",
                self.operand(lhs, indent)?,
                op.as_str(),
                self.operand(rhs, indent)?
            )),
            ExprKind::Assign { target, value } => {
                if self.path_mode(target.id) == Some(PathMode::Reinit) {
                    return self.element_reinit_expr(target, value, indent);
                }
                Ok(format!(
                    "{} = {}",
                    self.operand(target, indent)?,
                    self.expr(value, indent)?
                ))
            }
            ExprKind::AssignOp { op, target, value } => Ok(format!(
                "{} {}= {}",
                self.operand(target, indent)?,
                op.as_str(),
                self.expr(value, indent)?
            )),
            ExprKind::Cast { expr: inner, ty } => Ok(format!(
                "{} as {}",
                self.operand(inner, indent)?,
                self.type_node(ty)?
            )),
            ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => self.if_text(cond, then_block, else_branch, indent),
            ExprKind::Match { scrutinee, arms } => self.match_text(scrutinee, arms, indent),
            ExprKind::While { cond, body } => Ok(format!(
                "while {} {}",
                self.operand(cond, indent)?,
                self.block_text(indent, body)?
            )),
            ExprKind::Loop { body } => Ok(format!("loop {}", self.block_text(indent, body)?)),
            ExprKind::For { pat, iter, body } => Ok(format!(
                "for {} in {} {}",
                self.pattern(pat)?,
                self.expr(iter, indent)?,
                self.block_text(indent, body)?
            )),
            ExprKind::Block(block) => self.block_text(indent, block),
            ExprKind::Unsafe(block) => self.block_text(indent, block),
            ExprKind::Tuple(items) => {
                let mut rendered = Vec::new();
                for item in items {
                    rendered.push(self.expr(item, indent)?);
                }
                if rendered.len() == 1 {
                    Ok(format!("({},)", rendered[0]))
                } else {
                    Ok(format!("({})", rendered.join(", ")))
                }
            }
            ExprKind::Array(items) => {
                let mut rendered = Vec::new();
                for item in items {
                    rendered.push(self.expr(item, indent)?);
                }
                Ok(format!("[{}]", rendered.join(", ")))
            }
            ExprKind::ArrayRepeat { value, len } => Ok(format!(
                "[{}; {}]",
                self.expr(value, indent)?,
                self.expr(len, indent)?
            )),
            ExprKind::StructLit { path, fields } => {
                let base = self.struct_literal_path(path)?;
                if fields.is_empty() {
                    return Ok(format!("{base} {{}}"));
                }
                let mut rendered = Vec::new();
                for field in fields {
                    rendered.push(format!(
                        "{}: {}",
                        mangle_ident(&field.name.name),
                        self.expr(&field.value, indent)?
                    ));
                }
                Ok(format!("{base} {{ {} }}", rendered.join(", ")))
            }
            ExprKind::Range {
                start,
                end,
                inclusive,
            } => {
                let mut text = String::new();
                if let Some(first) = start {
                    text.push_str(&self.operand(first, indent)?);
                }
                text.push_str(if *inclusive { "..=" } else { ".." });
                if let Some(last) = end {
                    text.push_str(&self.operand(last, indent)?);
                }
                Ok(text)
            }
            ExprKind::Try(inner) => Ok(format!("{}?", self.operand(inner, indent)?)),
            ExprKind::Return(value) => match value {
                Some(value) => Ok(format!("return {}", self.expr(value, indent)?)),
                None => Ok("return".to_string()),
            },
            ExprKind::Break => Ok("break".to_string()),
            ExprKind::Continue => Ok("continue".to_string()),
        }
    }

    fn if_text(
        &self,
        cond: &Expr,
        then_block: &Block,
        else_branch: &Option<Box<Expr>>,
        indent: usize,
    ) -> Result<String, CodegenError> {
        let mut text = format!(
            "if {} {}",
            self.operand(cond, indent)?,
            self.block_text(indent, then_block)?
        );
        if let Some(otherwise) = else_branch {
            match &otherwise.kind {
                ExprKind::Block(block) => {
                    text.push_str(" else ");
                    text.push_str(&self.block_text(indent, block)?);
                }
                ExprKind::If {
                    cond,
                    then_block,
                    else_branch,
                } => {
                    text.push_str(" else ");
                    text.push_str(&self.if_text(cond, then_block, else_branch, indent)?);
                }
                _ => {
                    text.push_str(" else { ");
                    text.push_str(&self.expr(otherwise, indent)?);
                    text.push_str(" }");
                }
            }
        }
        Ok(text)
    }

    fn match_text(
        &self,
        scrutinee: &Expr,
        arms: &[MatchArm],
        indent: usize,
    ) -> Result<String, CodegenError> {
        // G-6.36: Rust has no float patterns. When an arm mentions a float
        // literal, bind the scrutinee once and lower every float sub-pattern to
        // a guard (`Some(__ravum_flt_0) if __ravum_flt_0 == 1.5f64`). The
        // checker requires a `_` arm for any float pattern, so the guards cannot
        // make the emitted match non-exhaustive.
        if !arms.iter().any(|arm| pattern_contains_float(&arm.pat)) {
            return self.plain_match_text(scrutinee, arms, indent);
        }
        let temp = "__ravum_match_scrutinee";
        let mut text = "{\n".to_string();
        text.push_str(&indent_text(indent + 1));
        text.push_str("let ");
        text.push_str(temp);
        text.push_str(" = ");
        text.push_str(&self.operand(scrutinee, indent + 1)?);
        text.push_str(";\n");
        text.push_str(&indent_text(indent + 1));
        text.push_str("match ");
        text.push_str(temp);
        text.push_str(" {\n");
        for arm in arms {
            let mut counter = 0usize;
            let mut conditions: Vec<String> = Vec::new();
            let pattern =
                self.pattern_with_float_guards(&arm.pat, &mut counter, &mut conditions)?;
            let body = self.expr(&arm.body, indent + 2)?;
            text.push_str(&indent_text(indent + 2));
            text.push_str(&pattern);
            if !conditions.is_empty() {
                text.push_str(" if ");
                text.push_str(&conditions.join(" && "));
            }
            text.push_str(" => ");
            text.push_str(&body);
            text.push_str(",\n");
        }
        text.push_str(&indent_text(indent + 1));
        text.push_str("}\n");
        text.push_str(&indent_text(indent));
        text.push('}');
        Ok(text)
    }

    fn plain_match_text(
        &self,
        scrutinee: &Expr,
        arms: &[MatchArm],
        indent: usize,
    ) -> Result<String, CodegenError> {
        let mut text = format!("match {} {{\n", self.operand(scrutinee, indent)?);
        for arm in arms {
            let pattern = self.pattern(&arm.pat)?;
            let body = self.expr(&arm.body, indent + 1)?;
            text.push_str(&indent_text(indent + 1));
            text.push_str(&pattern);
            text.push_str(" => ");
            text.push_str(&body);
            text.push_str(",\n");
        }
        text.push_str(&indent_text(indent));
        text.push('}');
        Ok(text)
    }

    /// Renders `pat` replacing every float literal by a fresh binding and
    /// recording `binding == literal` in `conditions` (G-6.36).
    fn pattern_with_float_guards(
        &self,
        pat: &Pat,
        counter: &mut usize,
        conditions: &mut Vec<String>,
    ) -> Result<String, CodegenError> {
        match &pat.kind {
            PatKind::Lit(LitPat::Float { text, suffix }) => {
                let name = format!("__ravum_flt_{counter}");
                *counter += 1;
                let literal =
                    self.float_literal(*suffix, text, self.facts().pat_types.get(&pat.id));
                conditions.push(format!("{name} == {literal}"));
                Ok(name)
            }
            PatKind::Tuple(items) => {
                let mut rendered = Vec::new();
                for item in items {
                    rendered.push(self.pattern_with_float_guards(item, counter, conditions)?);
                }
                if rendered.len() == 1 {
                    Ok(format!("({},)", rendered[0]))
                } else {
                    Ok(format!("({})", rendered.join(", ")))
                }
            }
            PatKind::Variant { path, fields } => {
                let base = self.variant_pattern_base(path)?;
                match fields {
                    VariantPatFields::Tuple(items) => {
                        let mut rendered = Vec::new();
                        for item in items {
                            rendered
                                .push(self.pattern_with_float_guards(item, counter, conditions)?);
                        }
                        Ok(format!("{base}({})", rendered.join(", ")))
                    }
                    VariantPatFields::Struct(fields) => {
                        if fields.is_empty() {
                            return Ok(format!("{base} {{}}"));
                        }
                        let mut rendered = Vec::new();
                        for field in fields {
                            rendered.push(format!(
                                "{}: {}",
                                mangle_ident(&field.name.name),
                                self.pattern_with_float_guards(&field.pat, counter, conditions)?
                            ));
                        }
                        Ok(format!("{base} {{ {} }}", rendered.join(", ")))
                    }
                }
            }
            _ => self.pattern(pat),
        }
    }

    // -- literals ----------------------------------------------------------

    fn int_literal_text(&self, value: u128, suffix: Option<IntTy>, ty: Option<&Type>) -> String {
        let name = match suffix {
            Some(suffix) => Some(suffix.name().to_string()),
            None => ty.and_then(int_type_name).map(str::to_string),
        };
        match name {
            Some(name) => format!("{value}{name}"),
            None => format!("{value}"),
        }
    }

    fn float_literal(&self, suffix: Option<FloatTy>, text: &str, ty: Option<&Type>) -> String {
        let mut out = text.trim().to_string();
        if out.is_empty() {
            out.push('0');
        }
        if !out.contains('.') && !out.contains('e') && !out.contains('E') {
            out.push_str(".0");
        }
        let is_f32 = match suffix {
            Some(FloatTy::F32) => true,
            Some(FloatTy::F64) => false,
            None => matches!(ty, Some(Type::F32)),
        };
        // Always annotate the literal. Without a suffix, `2.0` reaches `rustc` as
        // `{float}` and any method call on it fails with E0689
        // (`math::sqrt(2.0)` was accepted by `check` and refused by `rustc`).
        // The checker's recorded type decides f32 vs f64; the default is f64.
        out.push_str(if is_f32 { "f32" } else { "f64" });
        out
    }

    // -- paths -------------------------------------------------------------

    fn value_path(&self, path: &Path) -> Result<String, CodegenError> {
        match self.facts().resolutions.get(&path.id) {
            Some(Resolution::Local { name, .. }) => Ok(mangle_ident(name)),
            Some(Resolution::Item { item }) => self.item_path(*item),
            Some(Resolution::Variant { enum_item, variant }) => {
                match self.variant_name_opt(*enum_item, *variant) {
                    Some(name) => Ok(format!("{}::{}", self.enum_path(*enum_item)?, name)),
                    None => self.plain_path_text(path),
                }
            }
            Some(Resolution::BuiltinFn { path }) => self.builtin_fn_value(path),
            Some(Resolution::BuiltinConst { path }) => builtin_const(path).ok_or_else(|| {
                err(format!(
                    "unsupported builtin constant `{path}` (path node {})",
                    path
                ))
            }),
            Some(Resolution::Module) | Some(Resolution::BuiltinModule) => Err(err(format!(
                "module path `{}` used as a value (path node {})",
                path.display(),
                path.id
            ))),
            Some(Resolution::Error) => Err(err(format!(
                "path `{}` is unresolved in this checked program (path node {})",
                path.display(),
                path.id
            ))),
            None => Err(err(format!(
                "missing resolution fact for path `{}` (path node {})",
                path.display(),
                path.id
            ))),
        }
    }

    fn builtin_fn_value(&self, path: &str) -> Result<String, CodegenError> {
        Ok(match builtin_free(path) {
            Some(FreeBuiltin::Println) => "crate::rv_println".to_string(),
            Some(FreeBuiltin::Print) => "crate::rv_print".to_string(),
            Some(FreeBuiltin::Eprintln) => "crate::rv_eprintln".to_string(),
            Some(FreeBuiltin::Eprint) => "crate::rv_eprint".to_string(),
            Some(FreeBuiltin::ReadLine) => "crate::rv_read_line".to_string(),
            Some(FreeBuiltin::Args) => "crate::rv_args".to_string(),
            Some(FreeBuiltin::EnvVar) => "crate::rv_env_var".to_string(),
            Some(FreeBuiltin::FsReadToString) => "crate::rv_fs_read_to_string".to_string(),
            Some(FreeBuiltin::FsWriteString) => "crate::rv_fs_write_string".to_string(),
            Some(FreeBuiltin::FsExists) => "crate::rv_fs_exists".to_string(),
            Some(FreeBuiltin::Exit) => "crate::rv_exit".to_string(),
            Some(FreeBuiltin::Panic) => "crate::rv_panic".to_string(),
            Some(FreeBuiltin::Assert) => "crate::rv_assert".to_string(),
            Some(FreeBuiltin::AssertEq) => "crate::rv_assert_eq".to_string(),
            Some(FreeBuiltin::Drop) => "drop".to_string(),
            Some(FreeBuiltin::StringFrom) => "String::from".to_string(),
            Some(FreeBuiltin::ErrorNew) => "crate::Error::new".to_string(),
            Some(FreeBuiltin::RustPath(name)) => name.to_string(),
            Some(FreeBuiltin::ParseI64) | Some(FreeBuiltin::ParseF64) => {
                return Err(err(format!(
                    "builtin `{path}` is only supported when called directly"
                )));
            }
            Some(FreeBuiltin::MathUnary(method)) => format!("f64::{method}"),
            Some(FreeBuiltin::MathBinary(method)) => format!("f64::{method}"),
            Some(FreeBuiltin::VariantCtor) => {
                let segments: Vec<&str> = path.split("::").collect();
                segments[segments.len() - 1].to_string()
            }
            None => {
                return Err(err(format!("unsupported builtin function `{path}`")));
            }
        })
    }

    /// Path of a nominal type used as a value (`Point`, `E::V`, `Ok`).
    fn plain_path_text(&self, path: &Path) -> Result<String, CodegenError> {
        if path.root != PathRoot::Plain {
            return Err(err(format!(
                "cannot emit rooted path `{}` without a resolved item (path node {})",
                path.display(),
                path.id
            )));
        }
        let segments: Vec<String> = path
            .segments
            .iter()
            .map(|segment| mangle_ident(&segment.name.name))
            .collect();
        Ok(segments.join("::"))
    }

    fn struct_literal_path(&self, path: &Path) -> Result<String, CodegenError> {
        match self.facts().resolutions.get(&path.id) {
            Some(Resolution::Item { item }) => self.item_path(*item),
            Some(Resolution::Variant { enum_item, variant }) => {
                match self.variant_name_opt(*enum_item, *variant) {
                    Some(name) => Ok(format!("{}::{}", self.enum_path(*enum_item)?, name)),
                    None => self.plain_path_text(path),
                }
            }
            Some(_) => self.plain_path_text(path),
            None => Err(err(format!(
                "missing resolution fact for struct literal path `{}` (path node {})",
                path.display(),
                path.id
            ))),
        }
    }

    fn enum_path(&self, enum_item: NodeId) -> Result<String, CodegenError> {
        self.item_path(enum_item)
    }

    fn variant_name_opt(&self, enum_item: NodeId, variant: usize) -> Option<String> {
        if let Some(names) = self.index.enum_variants.get(&enum_item) {
            if let Some(name) = names.get(variant) {
                return Some(name.clone());
            }
        }
        self.facts()
            .enums
            .get(&enum_item)
            .and_then(|info| info.variants.get(variant))
            .map(|info| info.rust_name.clone())
    }

    /// Rust path of an item. `Facts::item_paths` is used when it is consistent
    /// with the module tree emitted from the AST; the AST tree wins on conflict
    /// because only it is guaranteed to match the emitted modules.
    fn item_path(&self, id: NodeId) -> Result<String, CodegenError> {
        let computed = self
            .index
            .items
            .get(&id)
            .map(|entry| entry.rust_path.clone());
        if let Some(raw) = self.facts().item_paths.get(&id) {
            let rewritten = self.rewrite_fact_path(raw);
            return match computed {
                Some(path) => Ok(path),
                None => Ok(rewritten),
            };
        }
        computed.ok_or_else(|| err(format!("missing Rust path for item node {id}")))
    }

    fn rewrite_fact_path(&self, raw: &str) -> String {
        let mut segments: Vec<&str> = raw.split("::").filter(|s| !s.is_empty()).collect();
        if segments.first() == Some(&"crate") {
            segments.remove(0);
        }
        let mut out = String::from("crate::rvm_root");
        let mut consumed = 0usize;
        let mut module_path = String::new();
        while consumed + 1 < segments.len() {
            let candidate = if module_path.is_empty() {
                segments[consumed].to_string()
            } else {
                format!("{module_path}::{}", segments[consumed])
            };
            if self.index.module_paths.contains(&candidate) {
                out.push_str("::rvm_");
                out.push_str(&sanitize_segment(segments[consumed]));
                module_path = candidate;
                consumed += 1;
            } else {
                break;
            }
        }
        for segment in &segments[consumed..] {
            out.push_str("::");
            out.push_str(&mangle_ident(segment));
        }
        out
    }

    // -- types -------------------------------------------------------------

    fn type_node(&self, ty: &AstType) -> Result<String, CodegenError> {
        self.type_node_pos(ty, TypePos::Value)
    }

    /// `type_node` for an explicit position (see [`TypePos`]).
    fn type_node_pos(&self, ty: &AstType, pos: TypePos) -> Result<String, CodegenError> {
        if let Some(resolved) = self.facts().type_nodes.get(&ty.id) {
            if !resolved.is_error() {
                return self.fmt_type_pos(resolved, pos);
            }
        }
        self.syntactic_type_pos(ty, pos)
    }

    fn syntactic_type_pos(&self, ty: &AstType, pos: TypePos) -> Result<String, CodegenError> {
        match &ty.kind {
            TypeKind::Unit => Ok("()".to_string()),
            TypeKind::Never => Ok("!".to_string()),
            TypeKind::SelfType => Ok("Self".to_string()),
            TypeKind::Ref { mutable, inner } => {
                if !*mutable && self.is_str_syntax(inner) {
                    // Unresolved `&str`: apply the same position rule as
                    // `fmt_type_pos` so a hand-built program without type facts
                    // still emits valid Rust.
                    return Ok(match pos {
                        TypePos::Value => "&'static str".to_string(),
                        TypePos::Param => "&str".to_string(),
                    });
                }
                Ok(format!(
                    "&{}{}",
                    if *mutable { "mut " } else { "" },
                    // See `fmt_type_pos`: nested text types are static (D13).
                    self.syntactic_type_pos(inner, TypePos::Value)?
                ))
            }
            TypeKind::Tuple(items) => {
                let mut rendered = Vec::new();
                for item in items {
                    rendered.push(self.syntactic_type_pos(item, TypePos::Value)?);
                }
                if rendered.len() == 1 {
                    Ok(format!("({},)", rendered[0]))
                } else {
                    Ok(format!("({})", rendered.join(", ")))
                }
            }
            TypeKind::Array { elem, len } => Ok(format!(
                "[{}; {}]",
                self.syntactic_type_pos(elem, TypePos::Value)?,
                self.array_length(len)?
            )),
            TypeKind::Slice(inner) => Ok(format!(
                "[{}]",
                self.syntactic_type_pos(inner, TypePos::Value)?
            )),
            TypeKind::Path(path) => self.syntactic_path_type(path),
            TypeKind::Infer => Err(err(format!(
                "missing resolved type for inferred type node {}",
                ty.id
            ))),
        }
    }

    /// `true` when this type syntax is the bare `str` keyword (`&str` inner).
    fn is_str_syntax(&self, ty: &AstType) -> bool {
        match &ty.kind {
            TypeKind::Path(path) => {
                path.root == PathRoot::Plain
                    && path.segments.len() == 1
                    && path.segments[0].name.name == "str"
            }
            _ => false,
        }
    }

    fn syntactic_path_type(&self, path: &Path) -> Result<String, CodegenError> {
        if path.root != PathRoot::Plain {
            return Err(err(format!(
                "missing resolved type for rooted type path `{}`",
                path.display()
            )));
        }
        if path.segments.len() != 1 {
            return Err(err(format!(
                "missing resolved type for qualified type path `{}`",
                path.display()
            )));
        }
        let segment = &path.segments[0];
        let name = segment.name.name.as_str();
        match builtin_type_name(name) {
            Some(builtin) => {
                if segment.args.is_empty() {
                    Ok(builtin.to_string())
                } else {
                    let mut rendered = Vec::new();
                    for argument in &segment.args {
                        rendered.push(self.syntactic_type_pos(argument, TypePos::Value)?);
                    }
                    Ok(format!("{builtin}<{}>", rendered.join(", ")))
                }
            }
            None => Err(err(format!(
                "missing resolved type for nominal type `{name}` (type node {})",
                path.id
            ))),
        }
    }

    fn array_length(&self, expr: &Expr) -> Result<String, CodegenError> {
        match &expr.kind {
            ExprKind::Int { value, .. } => Ok(value.to_string()),
            ExprKind::Path(path) => self.value_path(path),
            _ => Err(err(format!(
                "array length must be a literal integer or a constant (expression node {})",
                expr.id
            ))),
        }
    }

    fn fmt_type(&self, ty: &Type) -> Result<String, CodegenError> {
        self.fmt_type_pos(ty, TypePos::Value)
    }

    /// Emit a semantic type in an explicit position; see [`TypePos`] for why the
    /// borrowed text type is spelled `&'static str` when owned and `&str` when
    /// passed as a parameter.
    fn fmt_type_pos(&self, ty: &Type, pos: TypePos) -> Result<String, CodegenError> {
        Ok(match ty {
            Type::I8 => "i8".to_string(),
            Type::I16 => "i16".to_string(),
            Type::I32 => "i32".to_string(),
            Type::I64 => "i64".to_string(),
            Type::I128 => "i128".to_string(),
            Type::U8 => "u8".to_string(),
            Type::U16 => "u16".to_string(),
            Type::U32 => "u32".to_string(),
            Type::U64 => "u64".to_string(),
            Type::U128 => "u128".to_string(),
            Type::Isize => "isize".to_string(),
            Type::Usize => "usize".to_string(),
            Type::F32 => "f32".to_string(),
            Type::F64 => "f64".to_string(),
            Type::Bool => "bool".to_string(),
            Type::Char => "char".to_string(),
            Type::Unit => "()".to_string(),
            Type::Never => "!".to_string(),
            // `str` is unsized: it is legal in Core 1.0 media only behind the
            // `&str` view, which `Type::Ref` handles below.
            Type::Str => "str".to_string(),
            Type::String => "String".to_string(),
            Type::Ref { mutable, inner } => {
                if matches!(**inner, Type::Str) {
                    if *mutable {
                        // resolve.rs rejects `&mut str` with E-RES-UNKNOWN, so a
                        // checked program can never contain it; fail closed
                        // instead of guessing a Rust spelling.
                        return Err(err(
                            "internal error: `&mut str` has no Core 1.0 representation",
                        ));
                    }
                    return Ok(match pos {
                        // OWN-6.2: the only storable/returnable reference.
                        TypePos::Value => "&'static str".to_string(),
                        // STD-9.3 / TYP-13.1: a parameter accepts the
                        // instruction-scoped view from `String::as_str`.
                        TypePos::Param => "&str".to_string(),
                    });
                }
                format!(
                    "&{}{}",
                    if *mutable { "mut " } else { "" },
                    // A text type nested under another type constructor is
                    // always static (D13); only the parameter's own `&str` may
                    // be the elided, instruction-scoped view.
                    self.fmt_type_pos(inner, TypePos::Value)?
                )
            }
            Type::Array { elem, len } => {
                format!("[{}; {len}]", self.fmt_type_pos(elem, TypePos::Value)?)
            }
            Type::Slice(inner) => format!("[{}]", self.fmt_type_pos(inner, TypePos::Value)?),
            Type::Tuple(items) => {
                let mut rendered = Vec::new();
                for item in items {
                    rendered.push(self.fmt_type_pos(item, TypePos::Value)?);
                }
                if rendered.len() == 1 {
                    format!("({},)", rendered[0])
                } else {
                    format!("({})", rendered.join(", "))
                }
            }
            Type::Vec(inner) => format!("Vec<{}>", self.fmt_type_pos(inner, TypePos::Value)?),
            Type::Option(inner) => format!("Option<{}>", self.fmt_type_pos(inner, TypePos::Value)?),
            Type::Result(ok, error) => {
                format!(
                    "Result<{}, {}>",
                    self.fmt_type_pos(ok, TypePos::Value)?,
                    self.fmt_type_pos(error, TypePos::Value)?
                )
            }
            Type::Nominal { def, name, args } => {
                let base = self.nominal_path(*def, name)?;
                if args.is_empty() {
                    base
                } else {
                    let mut rendered = Vec::new();
                    for argument in args {
                        rendered.push(self.fmt_type_pos(argument, TypePos::Value)?);
                    }
                    format!("{base}<{}>", rendered.join(", "))
                }
            }
            Type::Param { name, .. } => mangle_ident(name),
            Type::Error => {
                return Err(err(
                    "internal error: attempted to generate the recovered error type",
                ));
            }
        })
    }

    fn nominal_path(&self, def: NodeId, name: &str) -> Result<String, CodegenError> {
        if let Some(entry) = self.index.items.get(&def) {
            return Ok(entry.rust_path.clone());
        }
        if let Some(raw) = self.facts().item_paths.get(&def) {
            return Ok(self.rewrite_fact_path(raw));
        }
        if name == "Error" {
            return Ok("crate::Error".to_string());
        }
        Err(err(format!(
            "cannot resolve nominal type `{name}` (definition node {def}) to a Rust path"
        )))
    }

    // -- patterns ----------------------------------------------------------

    fn pattern(&self, pat: &Pat) -> Result<String, CodegenError> {
        match &pat.kind {
            PatKind::Wild => Ok("_".to_string()),
            PatKind::Binding { mutable, name } => Ok(if *mutable {
                format!("mut {}", mangle_ident(&name.name))
            } else {
                mangle_ident(&name.name)
            }),
            PatKind::Lit(literal) => self.literal_pattern(pat, literal),
            PatKind::Tuple(items) => {
                let mut rendered = Vec::new();
                for item in items {
                    rendered.push(self.pattern(item)?);
                }
                if rendered.len() == 1 {
                    Ok(format!("({},)", rendered[0]))
                } else {
                    Ok(format!("({})", rendered.join(", ")))
                }
            }
            PatKind::Path(path) => match self.facts().resolutions.get(&path.id) {
                Some(Resolution::Variant { enum_item, variant }) => {
                    match self.variant_name_opt(*enum_item, *variant) {
                        Some(name) => Ok(format!("{}::{}", self.enum_path(*enum_item)?, name)),
                        None => self.plain_path_text(path),
                    }
                }
                Some(Resolution::Item { item }) => self.item_path(*item),
                Some(_) => self.plain_path_text(path),
                None => Err(err(format!(
                    "missing resolution fact for pattern path `{}` (path node {})",
                    path.display(),
                    path.id
                ))),
            },
            PatKind::Variant { path, fields } => {
                let base = self.variant_pattern_base(path)?;
                match fields {
                    VariantPatFields::Tuple(items) => {
                        let mut rendered = Vec::new();
                        for item in items {
                            rendered.push(self.pattern(item)?);
                        }
                        Ok(format!("{base}({})", rendered.join(", ")))
                    }
                    VariantPatFields::Struct(fields) => {
                        if fields.is_empty() {
                            return Ok(format!("{base} {{}}"));
                        }
                        let mut rendered = Vec::new();
                        for field in fields {
                            rendered.push(format!(
                                "{}: {}",
                                mangle_ident(&field.name.name),
                                self.pattern(&field.pat)?
                            ));
                        }
                        Ok(format!("{base} {{ {} }}", rendered.join(", ")))
                    }
                }
            }
        }
    }

    fn literal_pattern(&self, pat: &Pat, literal: &LitPat) -> Result<String, CodegenError> {
        Ok(match literal {
            LitPat::Int { value, suffix, neg } => {
                let text =
                    self.int_literal_text(*value, *suffix, self.facts().pat_types.get(&pat.id));
                if *neg {
                    format!("-{text}")
                } else {
                    text
                }
            }
            // G-6.36: Rust has no float literal patterns. `match_text` lowers
            // them to guards; reaching this point means a float pattern sits in
            // a position that cannot be lowered (`let`, parameter, `for`), and
            // emitting the literal here would produce Rust that `rustc` rejects.
            LitPat::Float { .. } => {
                return Err(err(
                    "float literal patterns are only supported in `match` arms",
                ));
            }
            LitPat::Bool(value) => {
                if *value {
                    "true".to_string()
                } else {
                    "false".to_string()
                }
            }
            LitPat::Char(value) => char_literal(*value),
            LitPat::Str(value) => string_literal(value),
        })
    }

    /// Resolves the `Enum::Variant` prefix shared by the two pattern renderers.
    fn variant_pattern_base(&self, path: &Path) -> Result<String, CodegenError> {
        match self.facts().resolutions.get(&path.id) {
            Some(Resolution::Variant { enum_item, variant }) => {
                match self.variant_name_opt(*enum_item, *variant) {
                    Some(name) => Ok(format!("{}::{}", self.enum_path(*enum_item)?, name)),
                    None => self.plain_path_text(path),
                }
            }
            Some(Resolution::Item { item }) => self.item_path(*item),
            Some(_) => self.plain_path_text(path),
            None => Err(err(format!(
                "missing resolution fact for pattern path `{}` (path node {})",
                path.display(),
                path.id
            ))),
        }
    }

    // -- calls -------------------------------------------------------------

    fn call_args(&self, args: &[Expr], indent: usize) -> Result<String, CodegenError> {
        let mut rendered = Vec::new();
        for argument in args {
            rendered.push(self.expr(argument, indent)?);
        }
        Ok(rendered.join(", "))
    }

    fn call_expr(
        &self,
        expr: &Expr,
        callee: &Expr,
        args: &[Expr],
        indent: usize,
    ) -> Result<String, CodegenError> {
        let target = self.facts().calls.get(&expr.id).ok_or_else(|| {
            err(format!(
                "missing call fact for call expression (expression node {})",
                expr.id
            ))
        })?;
        match target {
            CallTarget::UserFn { item } => Ok(format!(
                "{}({})",
                self.item_path(*item)?,
                self.call_args(args, indent)?
            )),
            CallTarget::BuiltinFn { path } => self.builtin_free_call(expr, path, args, indent),
            CallTarget::VariantCtor { enum_item, variant } => {
                let name = self.variant_name_opt(*enum_item, *variant);
                let constructor = match name {
                    Some(name) => format!("{}::{}", self.enum_path(*enum_item)?, name),
                    None => match &callee.kind {
                        ExprKind::Path(path) => self.plain_path_text(path)?,
                        _ => {
                            return Err(err(format!(
                                "unresolved variant constructor for call expression (expression node {})",
                                expr.id
                            )));
                        }
                    },
                };
                let rendered = self.call_args(args, indent)?;
                let is_unit = self.facts().enums.get(enum_item).is_some_and(|info| {
                    matches!(
                        info.variants.get(*variant).map(|variant| &variant.kind),
                        Some(VariantKind::Unit)
                    )
                });
                if rendered.is_empty() && is_unit {
                    Ok(constructor)
                } else {
                    Ok(format!("{constructor}({rendered})"))
                }
            }
            CallTarget::Error => Err(err(format!(
                "call expression (node {}) has an unresolved target",
                expr.id
            ))),
        }
    }

    fn builtin_free_call(
        &self,
        expr: &Expr,
        path: &str,
        args: &[Expr],
        indent: usize,
    ) -> Result<String, CodegenError> {
        let spec = builtin_free(path).ok_or_else(|| {
            err(format!(
                "unsupported builtin function `{path}` (call node {})",
                expr.id
            ))
        })?;
        let (file, line) = self.location(expr.span);
        let rendered = self.call_args(args, indent)?;
        let first = args.first();
        match spec {
            FreeBuiltin::Println => Ok(format!("crate::rv_println({})", self.str_arg(first, indent)?)),
            FreeBuiltin::Print => Ok(format!("crate::rv_print({})", self.str_arg(first, indent)?)),
            FreeBuiltin::Eprintln => {
                Ok(format!("crate::rv_eprintln({})", self.str_arg(first, indent)?))
            }
            FreeBuiltin::Eprint => Ok(format!("crate::rv_eprint({})", self.str_arg(first, indent)?)),
            FreeBuiltin::ReadLine => {
                self.expect_arity(path, args, 0)?;
                Ok("crate::rv_read_line()".to_string())
            }
            FreeBuiltin::Args => {
                self.expect_arity(path, args, 0)?;
                Ok("crate::rv_args()".to_string())
            }
            FreeBuiltin::EnvVar => Ok(format!("crate::rv_env_var({})", self.str_arg(first, indent)?)),
            FreeBuiltin::FsReadToString => Ok(format!(
                "crate::rv_fs_read_to_string({})",
                self.str_arg(first, indent)?
            )),
            FreeBuiltin::FsWriteString => {
                let path_arg = self.nth_arg(args, 0, indent)?;
                let contents_arg = self.nth_arg(args, 1, indent)?;
                Ok(format!(
                    "crate::rv_fs_write_string({}, {})",
                    path_arg, contents_arg
                ))
            }
            FreeBuiltin::FsExists => Ok(format!(
                "crate::rv_fs_exists({})",
                self.str_arg(first, indent)?
            )),
            FreeBuiltin::Exit => Ok(format!("crate::rv_exit({rendered})")),
            FreeBuiltin::Panic => match args.len() {
                0 => Ok("crate::rv_panic(\"panic\")".to_string()),
                1 => Ok(format!("crate::rv_panic({})", self.str_arg(first, indent)?)),
                _ => Err(err(format!(
                    "`panic` takes at most one argument (call node {})",
                    expr.id
                ))),
            },
            FreeBuiltin::Assert => match args.len() {
                1 => Ok(format!(
                    "crate::rv_assert({}, {}, {})",
                    self.expr(&args[0], indent)?,
                    string_literal(&file),
                    line
                )),
                2 => Ok(format!(
                    "crate::rv_assert_msg({}, {}, {}, {})",
                    self.expr(&args[0], indent)?,
                    self.str_arg(args.get(1), indent)?,
                    string_literal(&file),
                    line
                )),
                _ => Err(err(format!(
                    "`assert` takes one or two arguments (call node {})",
                    expr.id
                ))),
            },
            FreeBuiltin::AssertEq => {
                if args.len() != 2 {
                    return Err(err(format!(
                        "`assert_eq` takes exactly two arguments (call node {})",
                        expr.id
                    )));
                }
                // Ravum compares text values of different Rust types (`String`
                // versus `&str`); Rust's cross `PartialEq` impls cover that with
                // the `_cross` helper. Same types keep the simple `Debug` form.
                let heterogeneous = match (
                    self.facts().expr_types.get(&args[0].id),
                    self.facts().expr_types.get(&args[1].id),
                ) {
                    (Some(left), Some(right)) => left != right,
                    _ => false,
                };
                let helper = if heterogeneous {
                    "crate::rv_assert_eq_cross"
                } else {
                    "crate::rv_assert_eq"
                };
                Ok(format!(
                    "{helper}(&({}), &({}), {}, {})",
                    self.expr(&args[0], indent)?,
                    self.expr(&args[1], indent)?,
                    string_literal(&file),
                    line
                ))
            }
            FreeBuiltin::Drop => Ok(format!("drop({rendered})")),
            FreeBuiltin::StringFrom => Ok(format!(
                "String::from({})",
                self.str_arg(first, indent)?
            )),
            FreeBuiltin::ErrorNew => Ok(format!(
                "crate::Error::new({})",
                self.str_arg(first, indent)?
            )),
            FreeBuiltin::RustPath(name) => Ok(format!("{name}({rendered})")),
            FreeBuiltin::ParseI64 => Ok(format!(
                "({}).trim().parse::<i64>().map_err(|rv_error| crate::Error::new(&rv_error.to_string()))",
                self.str_borrow(first, indent)?
            )),
            FreeBuiltin::ParseF64 => Ok(format!(
                "({}).trim().parse::<f64>().map_err(|rv_error| crate::Error::new(&rv_error.to_string()))",
                self.str_borrow(first, indent)?
            )),
            FreeBuiltin::MathUnary(method) => Ok(format!(
                "({}).{method}()",
                self.nth_arg(args, 0, indent)?
            )),
            FreeBuiltin::MathBinary(method) => {
                let a = self.nth_arg(args, 0, indent)?;
                let b = self.nth_arg(args, 1, indent)?;
                // STD-6.5 fixes the observable result on signed zeros:
                // `max(-0.0, +0.0) = +0.0` and `min(-0.0, +0.0) = -0.0`.
                // `f64::max`/`min` lower to an LLVM intrinsic whose answer for
                // that tie depends on the optimisation level (`-O3` and
                // `-C opt-level=0` disagree), so the tie is emitted explicitly.
                // NaN is left to the intrinsic, which already implements
                // "if one operand is NaN, return the other".
                let is_float = matches!(
                    args.first()
                        .and_then(|argument| self.facts().expr_types.get(&argument.id)),
                    Some(Type::F32) | Some(Type::F64)
                );
                if is_float && (method == "max" || method == "min") {
                    let tie = if method == "max" { "0.0" } else { "-0.0" };
                    return Ok(format!(
                        "(if ({a}) == 0.0 && ({b}) == 0.0 {{ {tie} }} else {{ ({a}).{method}({b}) }})"
                    ));
                }
                Ok(format!("({a}).{method}({b})"))
            }
            FreeBuiltin::VariantCtor => {
                let segments: Vec<&str> = path
                    .strip_prefix("std::")
                    .unwrap_or(path)
                    .split("::")
                    .collect();
                let constructor = segments[segments.len() - 1];
                let owner = segments
                    .get(segments.len().wrapping_sub(2))
                    .copied()
                    .filter(|owner| matches!(*owner, "Option" | "Result"));
                match owner {
                    Some(owner) => Ok(format!("{owner}::{constructor}({rendered})")),
                    None => Ok(format!("{constructor}({rendered})")),
                }
            }
        }
    }

    fn expect_arity(&self, path: &str, args: &[Expr], expected: usize) -> Result<(), CodegenError> {
        if args.len() == expected {
            Ok(())
        } else {
            Err(err(format!(
                "builtin `{path}` expects {expected} argument(s), got {}",
                args.len()
            )))
        }
    }

    fn nth_arg(&self, args: &[Expr], index: usize, indent: usize) -> Result<String, CodegenError> {
        match args.get(index) {
            Some(argument) => self.expr(argument, indent),
            None => Err(err(format!(
                "builtin call is missing argument #{} (call node {})",
                index + 1,
                args.first().map(|_| "?").unwrap_or("?")
            ))),
        }
    }

    /// Emit an argument that must coerce to `&str`.
    fn str_arg(&self, arg: Option<&Expr>, indent: usize) -> Result<String, CodegenError> {
        match arg {
            Some(argument) => {
                let text = self.expr(argument, indent)?;
                match self.facts().expr_types.get(&argument.id) {
                    Some(Type::String) | Some(Type::Str) => Ok(format!("&({text})")),
                    _ => Ok(text),
                }
            }
            None => Err(err("builtin call is missing its text argument")),
        }
    }

    /// Emit an argument that must be usable as a `&str` receiver (`str::parse_*`).
    fn str_borrow(&self, arg: Option<&Expr>, indent: usize) -> Result<String, CodegenError> {
        match arg {
            Some(argument) => {
                let text = self.expr(argument, indent)?;
                match self.facts().expr_types.get(&argument.id) {
                    Some(Type::String) | Some(Type::Str) => Ok(format!("&({text})")),
                    _ => Ok(text),
                }
            }
            None => Err(err("builtin call is missing its text argument")),
        }
    }

    fn method_call_expr(
        &self,
        expr: &Expr,
        receiver: &Expr,
        method: &Ident,
        args: &[Expr],
        indent: usize,
    ) -> Result<String, CodegenError> {
        let target = self.facts().methods.get(&expr.id).ok_or_else(|| {
            err(format!(
                "missing method fact for method call (expression node {})",
                expr.id
            ))
        })?;
        let receiver_text = self.operand(receiver, indent)?;
        match target {
            MethodTarget::User { item } => {
                let name = self
                    .index
                    .fns
                    .get(item)
                    .map(|entry| entry.rust_name.clone())
                    .unwrap_or_else(|| mangle_ident(&method.name));
                Ok(format!(
                    "{receiver_text}.{name}({})",
                    self.call_args(args, indent)?
                ))
            }
            MethodTarget::Builtin { path, .. } => {
                self.builtin_method_call(expr, path, &receiver_text, args, indent)
            }
            MethodTarget::Error => Err(err(format!(
                "method call (node {}) has an unresolved target",
                expr.id
            ))),
        }
    }

    fn builtin_method_call(
        &self,
        expr: &Expr,
        path: &str,
        receiver: &str,
        args: &[Expr],
        indent: usize,
    ) -> Result<String, CodegenError> {
        let (owner, method) = split_builtin_method(path).ok_or_else(|| {
            err(format!(
                "malformed builtin method path `{path}` (method call node {})",
                expr.id
            ))
        })?;
        let owner = normalize_owner(owner);
        let rendered = self.call_args(args, indent)?;

        if let Some(special) = builtin_method_special(&owner, method, receiver, &rendered) {
            return Ok(special);
        }

        // Direct pass-through only for the methods the Core 1.0 stdlib actually
        // provides on this owner. No hidden clone, no hidden conversion.
        if method == "to_string" {
            return Ok(format!("format!(\"{{}}\", {receiver})"));
        }
        if builtin_method_is_direct(&owner, method) {
            return Ok(format!("{receiver}.{}({rendered})", mangle_ident(method)));
        }
        Err(err(format!(
            "unsupported builtin method `{path}` (owner `{owner}`, method `{method}`) at method call node {}; \
             the Ravum Core 1.0 stdlib has no such method",
            expr.id
        )))
    }

    // -- constants ---------------------------------------------------------

    fn const_value(&self, ty: &Type, value: &ConstValue) -> Result<String, CodegenError> {
        if matches!(value, ConstValue::Str(_)) && matches!(ty, Type::String) {
            return Err(err(
                "a `String` constant cannot be generated: stable Rust cannot build a non-empty `String` in a constant; declare the constant as `&str`",
            ));
        }
        Ok(match value {
            ConstValue::Int(number) => {
                let name = int_type_name(ty).ok_or_else(|| {
                    err(format!(
                        "constant with an integer value has non-integer type `{ty}`"
                    ))
                })?;
                if *number < 0 {
                    format!("-{}{}", number.unsigned_abs(), name)
                } else {
                    format!("{number}{name}")
                }
            }
            ConstValue::Bool(value) => {
                if *value {
                    "true".to_string()
                } else {
                    "false".to_string()
                }
            }
            ConstValue::Char(value) => char_literal(*value),
            ConstValue::Str(text) => string_literal(text),
        })
    }

    fn literal_const_expr(&self, expr: &Expr) -> Result<String, CodegenError> {
        match &expr.kind {
            ExprKind::Int { value, suffix } => {
                Ok(self.int_literal_text(*value, *suffix, self.facts().expr_types.get(&expr.id)))
            }
            ExprKind::Bool(value) => Ok(if *value { "true" } else { "false" }.to_string()),
            ExprKind::Char(value) => Ok(char_literal(*value)),
            ExprKind::Str(value) => Ok(string_literal(value)),
            _ => Err(err(format!(
                "missing `Facts::consts` entry for constant value (expression node {})",
                expr.id
            ))),
        }
    }

    // -- source markers ----------------------------------------------------

    fn marker(&self, buf: &mut Buf, span: Span) {
        if !self.options.source_comments {
            return;
        }
        if span.is_dummy() {
            return;
        }
        let (file, line) = self.location(span);
        buf.line(&format!("// rv: {file}:{line}"));
    }

    fn location(&self, span: Span) -> (String, u32) {
        match self.checked.files.get(span.file as usize) {
            Some(file) => (file.path.clone(), file.line_of(span.start)),
            None => ("<unknown>".to_string(), 0),
        }
    }
}

// ---------------------------------------------------------------------------
// Array-element paths (OWN-5.4 / P0-A.2).
// ---------------------------------------------------------------------------
// Ravum tracks moves per element of a fixed-size array (OWN-5.4): after
// `let s = a[0];` the element `a[0]` is moved, the others stay usable, the whole
// array is not, and assigning the array restores it. Rust has the same model, but
// only through *patterns*: `a[0]` is refused (`E0508`) while
// `let [s, ..] = a;` moves that element alone and rustc keeps checking every
// later use of the array, branch by branch.
//
// The backend therefore lowers each element path at the access itself and keeps
// the array a Rust array. Nothing is hoisted and no element is copied into a
// separate local, so no generated binding can go stale when the array is assigned
// again, whatever block, branch or loop the access sits in.
//
// An *element path* is a chain of field/tuple projections and indexes that goes
// through at least one index of a fixed-size array: `a[0]`, `h.items[1]`,
// `a[1].0`, `b[0][i]`, `make()[0]`. Its *constant run* is the longest stretch,
// starting at that index, that one pattern can express (literal in-range
// indexes, tuple elements, struct fields).
//
// * Moved path: `({ let [_, __rv_el, ..] = a; __rv_el })`. Past the constant run
//   the moved value is consumed step by step; a non-literal index consumes its
//   whole array (`into_iter().nth(i)`), which is also what the checker assumes.
// * Borrowed path of an array that is partially moved somewhere in the function:
//   `(*{ let [_, ref __rv_el, ..] = a; __rv_el })`, because a plain `a[1]`
//   borrows the whole array. Arrays that are never moved from keep `a[1]`.
// * Re-initialised path (`a[0] = e` after `a[0]` was moved): the array is
//   rebuilt around the new value. The checker decides when that is possible and
//   records it (`OwnFacts::element_reinit`).
//
// Every form is checked again by rustc: a disagreement between the checker and
// this lowering is a compile error in the generated code, never a silently
// different program.
//
// Observable difference with a naive reading: an element that is moved out is
// dropped where its new owner is dropped rather than with the array. Core 1.0
// has no `Drop`/trait implementations (`impl` is inherent only), so no user code
// runs at either point; only the instant memory is released can differ.

mod paths;

use paths::{collect_module_exprs, ArrayMovePlan, PathMode};

// ---------------------------------------------------------------------------
// Tests.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
