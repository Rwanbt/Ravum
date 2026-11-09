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
    CallTarget, CheckedProgram, ConstValue, MethodTarget, Resolution, VariantKind,
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

/// Types the backend accepts at the C ABI boundary in Core 1.0, checked on the
/// syntax tree so a hand-built `CheckedProgram` cannot smuggle an ABI-unsafe
/// signature into the output.
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
                        | "i128"
                        | "u8"
                        | "u16"
                        | "u32"
                        | "u64"
                        | "u128"
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
        };
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
    const SIGNED: &[&str] = &["abs"];
    match owner {
        "String" => STRING.contains(&method),
        "str" => STR.contains(&method),
        "Vec" => VEC.contains(&method),
        "slice" | "Slice" | "[T]" => SLICE.contains(&method),
        "Option" => OPTION.contains(&method),
        "Result" => RESULT.contains(&method),
        "f32" | "f64" => FLOAT.contains(&method),
        "i8" | "i16" | "i32" | "i64" | "i128" | "isize" => SIGNED.contains(&method),
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
}

impl<'a> Emitter<'a> {
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
        buf.line("let mut rv_passed: usize = 0;");
        buf.line("let mut rv_failed: usize = 0;");
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
            buf.line(&format!(
                "if {}({}, {}) {{ rv_passed += 1; }} else {{ rv_failed += 1; }}",
                runner,
                string_literal(name),
                path
            ));
        }
        buf.line("println!();");
        buf.line("println!(\"{} passed; {} failed\", rv_passed, rv_failed);");
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
                    ParamKind::Binding { name, ty } => {
                        if !ast_ffi_safe(ty) {
                            return Err(err(format!(
                                "extern function `{}` parameter `{}` has a non-FFI-safe type",
                                f.name.name, name.name
                            )));
                        }
                        let name = mangle_ident(&name.name);
                        decls.push(format!("{name}: {}", self.type_node(ty)?));
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
                ParamKind::Binding { name, ty } => {
                    params.push(format!(
                        "{}: {}",
                        mangle_ident(&name.name),
                        self.type_node(ty)?
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
            ExprKind::Field { base, name } => Ok(format!(
                "{}.{}",
                self.operand(base, indent)?,
                mangle_ident(&name.name)
            )),
            ExprKind::TupleIndex { base, index } => {
                Ok(format!("{}.{}", self.operand(base, indent)?, index))
            }
            ExprKind::Index { base, index } => Ok(format!(
                "{}[{}]",
                self.operand(base, indent)?,
                self.expr(index, indent)?
            )),
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
            ExprKind::Assign { target, value } => Ok(format!(
                "{} = {}",
                self.operand(target, indent)?,
                self.expr(value, indent)?
            )),
            ExprKind::AssignOp { op, target, value } => {
                if !op.is_arithmetic() {
                    return Err(err(format!(
                        "compound assignment with `{}` is not supported (expression node {})",
                        op.as_str(),
                        expr.id
                    )));
                }
                Ok(format!(
                    "{} {}= {}",
                    self.operand(target, indent)?,
                    op.as_str(),
                    self.expr(value, indent)?
                ))
            }
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
        if is_f32 {
            out.push_str("f32");
        }
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
        if let Some(resolved) = self.facts().type_nodes.get(&ty.id) {
            if !resolved.is_error() {
                return self.fmt_type(resolved);
            }
        }
        self.syntactic_type(ty)
    }

    fn syntactic_type(&self, ty: &AstType) -> Result<String, CodegenError> {
        match &ty.kind {
            TypeKind::Unit => Ok("()".to_string()),
            TypeKind::SelfType => Ok("Self".to_string()),
            TypeKind::Ref { mutable, inner } => Ok(format!(
                "&{}{}",
                if *mutable { "mut " } else { "" },
                self.syntactic_type(inner)?
            )),
            TypeKind::Tuple(items) => {
                let mut rendered = Vec::new();
                for item in items {
                    rendered.push(self.syntactic_type(item)?);
                }
                if rendered.len() == 1 {
                    Ok(format!("({},)", rendered[0]))
                } else {
                    Ok(format!("({})", rendered.join(", ")))
                }
            }
            TypeKind::Array { elem, len } => Ok(format!(
                "[{}; {}]",
                self.syntactic_type(elem)?,
                self.array_length(len)?
            )),
            TypeKind::Slice(inner) => Ok(format!("[{}]", self.syntactic_type(inner)?)),
            TypeKind::Path(path) => self.syntactic_path_type(path),
            TypeKind::Infer => Err(err(format!(
                "missing resolved type for inferred type node {}",
                ty.id
            ))),
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
                        rendered.push(self.syntactic_type(argument)?);
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
            Type::Str => "str".to_string(),
            Type::String => "String".to_string(),
            Type::Ref { mutable, inner } => format!(
                "&{}{}",
                if *mutable { "mut " } else { "" },
                self.fmt_type(inner)?
            ),
            Type::Array { elem, len } => format!("[{}; {len}]", self.fmt_type(elem)?),
            Type::Slice(inner) => format!("[{}]", self.fmt_type(inner)?),
            Type::Tuple(items) => {
                let mut rendered = Vec::new();
                for item in items {
                    rendered.push(self.fmt_type(item)?);
                }
                if rendered.len() == 1 {
                    format!("({},)", rendered[0])
                } else {
                    format!("({})", rendered.join(", "))
                }
            }
            Type::Vec(inner) => format!("Vec<{}>", self.fmt_type(inner)?),
            Type::Option(inner) => format!("Option<{}>", self.fmt_type(inner)?),
            Type::Result(ok, error) => {
                format!("Result<{}, {}>", self.fmt_type(ok)?, self.fmt_type(error)?)
            }
            Type::Nominal { def, name, args } => {
                let base = self.nominal_path(*def, name)?;
                if args.is_empty() {
                    base
                } else {
                    let mut rendered = Vec::new();
                    for argument in args {
                        rendered.push(self.fmt_type(argument)?);
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
                let base = match self.facts().resolutions.get(&path.id) {
                    Some(Resolution::Variant { enum_item, variant }) => {
                        match self.variant_name_opt(*enum_item, *variant) {
                            Some(name) => format!("{}::{}", self.enum_path(*enum_item)?, name),
                            None => self.plain_path_text(path)?,
                        }
                    }
                    Some(Resolution::Item { item }) => self.item_path(*item)?,
                    Some(_) => self.plain_path_text(path)?,
                    None => {
                        return Err(err(format!(
                            "missing resolution fact for pattern path `{}` (path node {})",
                            path.display(),
                            path.id
                        )));
                    }
                };
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
            LitPat::Int { value, suffix } => {
                self.int_literal_text(*value, *suffix, self.facts().pat_types.get(&pat.id))
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
                let is_unit = self.facts().enums.get(enum_item).map_or(false, |info| {
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
            FreeBuiltin::MathBinary(method) => Ok(format!(
                "({}).{method}({})",
                self.nth_arg(args, 0, indent)?,
                self.nth_arg(args, 1, indent)?
            )),
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
// Tests.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GenOptions;
    use ravum_lang_sema::facts::{
        CallTarget, CheckedProgram, ConstInfo, ConstValue, Facts, FieldInfo, FileInfo,
        MethodTarget, Profile, Resolution, StructInfo,
    };
    use ravum_lang_sema::types::Type as STy;
    use ravum_lang_syntax::ast::{
        BinOp, Block, Expr, ExprKind, FieldDef, FieldInit, FnItem, ImplItem, Item, ItemKind,
        LetStmt, Module, Param, ParamKind, Pat, PatKind, Path, PathRoot, PathSegment, Program,
        Receiver, Stmt, StmtKind, StructItem, Type as AstType, TypeKind, UnOp, VariantDefFields,
        VariantPatFields, Vis,
    };
    use ravum_lang_syntax::span::Span;

    fn span() -> Span {
        Span::new(0, 0, 3)
    }

    // -- minimal hand-built AST/Facts ----------------------------------------

    struct Builder {
        next: NodeId,
        items: Vec<Item>,
        facts: Facts,
        files: Vec<FileInfo>,
    }

    impl Builder {
        fn new() -> Builder {
            Builder {
                next: 1,
                items: Vec::new(),
                facts: Facts::default(),
                files: vec![FileInfo {
                    path: "main.rv".to_string(),
                    line_starts: vec![0, 10, 20, 40, 80],
                }],
            }
        }

        fn id(&mut self) -> NodeId {
            let id = self.next;
            self.next += 1;
            id
        }

        fn ident(&mut self, name: &str) -> ravum_lang_syntax::ast::Ident {
            ravum_lang_syntax::ast::Ident::new(name, span())
        }

        fn path(&mut self, names: &[&str]) -> Path {
            let id = self.id();
            let mut segments = Vec::new();
            for name in names {
                let ident = self.ident(name);
                segments.push(PathSegment {
                    name: ident,
                    args: Vec::new(),
                });
            }
            Path {
                id,
                root: PathRoot::Plain,
                segments,
                span: span(),
            }
        }

        fn expr(&mut self, kind: ExprKind, ty: Option<STy>) -> Expr {
            let id = self.id();
            if let Some(ty) = ty {
                self.facts.expr_types.insert(id, ty);
            }
            Expr {
                id,
                kind,
                span: span(),
            }
        }

        fn pat(&mut self, kind: PatKind, ty: Option<STy>) -> Pat {
            let id = self.id();
            if let Some(ty) = ty {
                self.facts.pat_types.insert(id, ty);
            }
            Pat {
                id,
                kind,
                span: span(),
            }
        }

        fn ty(&mut self, kind: TypeKind, resolved: Option<STy>) -> AstType {
            let id = self.id();
            if let Some(resolved) = resolved {
                self.facts.type_nodes.insert(id, resolved);
            }
            AstType {
                id,
                kind,
                span: span(),
            }
        }

        fn named_type(&mut self, name: &str, resolved: Option<STy>) -> AstType {
            let path = self.path(&[name]);
            self.ty(TypeKind::Path(path), resolved)
        }

        fn block(&mut self, stmts: Vec<Stmt>, tail: Option<Expr>) -> Block {
            Block {
                id: self.id(),
                stmts,
                tail: tail.map(Box::new),
                span: span(),
            }
        }

        fn var(&mut self, name: &str, ty: Option<STy>) -> Expr {
            let path = self.path(&[name]);
            self.facts.resolutions.insert(
                path.id,
                Resolution::Local {
                    name: name.to_string(),
                    mutable: false,
                },
            );
            self.expr(ExprKind::Path(path), ty)
        }

        fn int(&mut self, value: u128, ty: Option<STy>) -> Expr {
            self.expr(
                ExprKind::Int {
                    value,
                    suffix: None,
                },
                ty,
            )
        }

        fn text(&mut self, value: &str) -> Expr {
            self.expr(ExprKind::Str(value.to_string()), None)
        }

        fn unit(&mut self) -> Expr {
            self.expr(ExprKind::Unit, None)
        }

        fn callee(&mut self, name: &str) -> Expr {
            let path = self.path(&[name]);
            self.expr(ExprKind::Path(path), None)
        }

        fn call(&mut self, callee: Expr, args: Vec<Expr>, target: CallTarget) -> Expr {
            let id = self.id();
            self.facts.calls.insert(id, target);
            Expr {
                id,
                kind: ExprKind::Call {
                    callee: Box::new(callee),
                    args,
                },
                span: span(),
            }
        }

        fn method(
            &mut self,
            receiver: Expr,
            name: &str,
            args: Vec<Expr>,
            target: MethodTarget,
        ) -> Expr {
            let method = self.ident(name);
            let id = self.id();
            self.facts.methods.insert(id, target);
            Expr {
                id,
                kind: ExprKind::MethodCall {
                    receiver: Box::new(receiver),
                    method,
                    args,
                },
                span: span(),
            }
        }

        fn let_stmt(&mut self, name: &str, ty: Option<AstType>, init: Expr) -> Stmt {
            let ident = self.ident(name);
            let pattern = self.pat(
                PatKind::Binding {
                    mutable: false,
                    name: ident,
                },
                None,
            );
            Stmt {
                id: self.id(),
                kind: StmtKind::Let(LetStmt {
                    pat: pattern,
                    ty,
                    init: Some(init),
                }),
                span: span(),
            }
        }

        fn expr_stmt(&mut self, expr: Expr) -> Stmt {
            Stmt {
                id: self.id(),
                kind: StmtKind::Expr { expr, semi: true },
                span: span(),
            }
        }

        fn finish(mut self, main: Option<NodeId>, tests: Vec<NodeId>) -> CheckedProgram {
            let root_id = self.id();
            self.facts.main_fn = main;
            self.facts.test_fns = tests;
            let program = Program {
                file: 0,
                root: Module {
                    id: root_id,
                    name: None,
                    items: self.items,
                    span: span(),
                },
            };
            CheckedProgram {
                program,
                facts: self.facts,
                files: self.files,
                profile: Profile::Core,
            }
        }
    }

    fn make_param(builder: &mut Builder, name: &str, ty: AstType) -> Param {
        let id = builder.id();
        Param {
            id,
            kind: ParamKind::Binding {
                name: builder.ident(name),
                ty,
            },
            span: span(),
        }
    }

    fn make_receiver(builder: &mut Builder, receiver: Receiver) -> Param {
        let id = builder.id();
        Param {
            id,
            kind: ParamKind::Receiver(receiver),
            span: span(),
        }
    }

    fn make_fn(
        builder: &mut Builder,
        id: NodeId,
        name: &str,
        visibility: Vis,
        is_test: bool,
        params: Vec<Param>,
        ret: AstType,
        body: Block,
    ) -> Item {
        let name = builder.ident(name);
        Item {
            id,
            vis: visibility,
            kind: ItemKind::Fn(FnItem {
                name,
                generics: Vec::new(),
                params,
                ret,
                effects: None,
                body,
                is_test,
                span: span(),
            }),
            span: span(),
        }
    }

    fn generate_ok(checked: &CheckedProgram, options: &GenOptions) -> String {
        crate::generate(checked, options).expect("code generation must succeed")
    }

    // -- program A: functions, struct, impl, builtins, Result main -----------

    struct ProgramA {
        checked: CheckedProgram,
        println_call_id: NodeId,
        point_id: NodeId,
        sum_id: NodeId,
    }

    fn build_program_a() -> ProgramA {
        let mut b = Builder::new();

        // struct Point { pub x: i32, pub y: i32 }
        let point_id = b.id();
        let point_ty = STy::Nominal {
            def: point_id,
            name: "Point".to_string(),
            args: Vec::new(),
        };
        let x_ty = b.named_type("i32", None);
        let y_ty = b.named_type("i32", None);
        let point_name = b.ident("Point");
        let field_x = b.id();
        let field_y = b.id();
        let x_name = b.ident("x");
        let y_name = b.ident("y");
        let struct_item = Item {
            id: point_id,
            vis: Vis::Public(span()),
            kind: ItemKind::Struct(StructItem {
                name: point_name,
                generics: Vec::new(),
                fields: vec![
                    FieldDef {
                        id: field_x,
                        vis: Vis::Public(span()),
                        name: x_name,
                        ty: x_ty,
                        span: span(),
                    },
                    FieldDef {
                        id: field_y,
                        vis: Vis::Private,
                        name: y_name,
                        ty: y_ty,
                        span: span(),
                    },
                ],
                span: span(),
            }),
            span: span(),
        };
        b.items.push(struct_item);
        b.facts.structs.insert(
            point_id,
            StructInfo {
                item: point_id,
                rust_path: "crate::Point".to_string(),
                name: "Point".to_string(),
                generics: Vec::new(),
                fields: vec![
                    FieldInfo {
                        name: "x".to_string(),
                        rust_name: "x".to_string(),
                        ty: STy::I32,
                        public: true,
                    },
                    FieldInfo {
                        name: "y".to_string(),
                        rust_name: "y".to_string(),
                        ty: STy::I32,
                        public: false,
                    },
                ],
                is_copy: true,
            },
        );

        // impl Point { fn sum(&self) -> i32 { self.x + self.y } }
        let sum_id = b.id();
        let self_value = b.expr(ExprKind::SelfValue, Some(point_ty.clone()));
        let self_x_name = b.ident("x");
        let self_x = b.expr(
            ExprKind::Field {
                base: Box::new(self_value),
                name: self_x_name,
            },
            Some(STy::I32),
        );
        let self_value = b.expr(ExprKind::SelfValue, Some(point_ty.clone()));
        let self_y_name = b.ident("y");
        let self_y = b.expr(
            ExprKind::Field {
                base: Box::new(self_value),
                name: self_y_name,
            },
            Some(STy::I32),
        );
        let sum_body_expr = b.expr(
            ExprKind::Binary {
                op: BinOp::Add,
                lhs: Box::new(self_x),
                rhs: Box::new(self_y),
            },
            Some(STy::I32),
        );
        let sum_body = b.block(Vec::new(), Some(sum_body_expr));
        let sum_ret = b.named_type("i32", Some(STy::I32));
        let receiver = make_receiver(&mut b, Receiver::Shared);
        let sum_item = make_fn(
            &mut b,
            sum_id,
            "sum",
            Vis::Private,
            false,
            vec![receiver],
            sum_ret,
            sum_body,
        );
        let self_ty = b.named_type("Point", Some(point_ty.clone()));
        let impl_item = Item {
            id: b.id(),
            vis: Vis::Private,
            kind: ItemKind::Impl(ImplItem {
                generics: Vec::new(),
                self_ty,
                items: vec![sum_item],
                span: span(),
            }),
            span: span(),
        };
        b.items.push(impl_item);

        // fn add(a: i32, b: i32) -> i32 { a + b }
        let add_id = b.id();
        let a_ty = b.named_type("i32", Some(STy::I32));
        let b_ty = b.named_type("i32", Some(STy::I32));
        let param_a = make_param(&mut b, "a", a_ty);
        let param_b = make_param(&mut b, "b", b_ty);
        let value_a = b.var("a", Some(STy::I32));
        let value_b = b.var("b", Some(STy::I32));
        let add_expr = b.expr(
            ExprKind::Binary {
                op: BinOp::Add,
                lhs: Box::new(value_a),
                rhs: Box::new(value_b),
            },
            Some(STy::I32),
        );
        let add_body = b.block(Vec::new(), Some(add_expr));
        let add_ret = b.named_type("i32", Some(STy::I32));
        let add_item = make_fn(
            &mut b,
            add_id,
            "add",
            Vis::Public(span()),
            false,
            vec![param_a, param_b],
            add_ret,
            add_body,
        );
        b.items.push(add_item);

        // fn main() -> Result<(), Error>
        let main_id = b.id();
        let println_callee = b.callee("println");
        let hello = b.text("hello");
        let println_call = b.call(
            println_callee,
            vec![hello],
            CallTarget::BuiltinFn {
                path: "std::io::println".to_string(),
            },
        );
        let println_call_id = println_call.id;
        let print_stmt = b.expr_stmt(println_call);

        let point_annotation = b.named_type("Point", Some(point_ty.clone()));
        let lit_path = b.path(&["Point"]);
        b.facts
            .resolutions
            .insert(lit_path.id, Resolution::Item { item: point_id });
        let field_literal_x = b.ident("x");
        let field_literal_y = b.ident("y");
        let one = b.int(1, Some(STy::I32));
        let two = b.int(2, Some(STy::I32));
        let point_literal = b.expr(
            ExprKind::StructLit {
                path: lit_path,
                fields: vec![
                    FieldInit {
                        name: field_literal_x,
                        value: one,
                    },
                    FieldInit {
                        name: field_literal_y,
                        value: two,
                    },
                ],
            },
            Some(point_ty.clone()),
        );
        let point_stmt = b.let_stmt("p", Some(point_annotation), point_literal);

        let receiver_p = b.var("p", Some(point_ty.clone()));
        let sum_call = b.method(
            receiver_p,
            "sum",
            Vec::new(),
            MethodTarget::User { item: sum_id },
        );
        let sum_annotation = b.named_type("i32", Some(STy::I32));
        let sum_stmt = b.let_stmt("s", Some(sum_annotation), sum_call);

        let receiver_s = b.var("s", Some(STy::I32));
        let to_string_call = b.method(
            receiver_s,
            "to_string",
            Vec::new(),
            MethodTarget::Builtin {
                path: "i32::to_string".to_string(),
                ret: STy::String,
            },
        );
        let string_annotation = b.named_type("String", Some(STy::String));
        let to_string_stmt = b.let_stmt("t", Some(string_annotation), to_string_call);

        let receiver_t = b.var("t", Some(STy::String));
        let borrow_t = b.expr(
            ExprKind::Unary {
                op: UnOp::Ref,
                expr: Box::new(receiver_t),
            },
            Some(STy::Ref {
                mutable: false,
                inner: Box::new(STy::String),
            }),
        );
        let println_t_callee = b.callee("println");
        let println_t = b.call(
            println_t_callee,
            vec![borrow_t],
            CallTarget::BuiltinFn {
                path: "std::io::println".to_string(),
            },
        );
        let print_t_stmt = b.expr_stmt(println_t);

        // let q: i32 = add(2, 3);
        let add_callee = b.callee("add");
        let two_arg = b.int(2, Some(STy::I32));
        let three_arg = b.int(3, Some(STy::I32));
        let add_call = b.call(
            add_callee,
            vec![two_arg, three_arg],
            CallTarget::UserFn { item: add_id },
        );
        let q_annotation = b.named_type("i32", Some(STy::I32));
        let q_stmt = b.let_stmt("q", Some(q_annotation), add_call);

        let ok_callee = b.callee("Ok");
        let unit_value = b.unit();
        let ok_call = b.call(
            ok_callee,
            vec![unit_value],
            CallTarget::VariantCtor {
                enum_item: 900_000,
                variant: 0,
            },
        );

        let body = b.block(
            vec![
                print_stmt,
                point_stmt,
                sum_stmt,
                to_string_stmt,
                print_t_stmt,
                q_stmt,
            ],
            Some(ok_call),
        );
        let result_path = b.path(&["Result"]);
        let result_type = b.ty(
            TypeKind::Path(result_path),
            Some(STy::Result(
                Box::new(STy::Unit),
                Box::new(STy::Nominal {
                    def: 900_001,
                    name: "Error".to_string(),
                    args: Vec::new(),
                }),
            )),
        );
        let main_item = make_fn(
            &mut b,
            main_id,
            "main",
            Vis::Private,
            false,
            Vec::new(),
            result_type,
            body,
        );
        b.items.push(main_item);

        let checked = b.finish(Some(main_id), Vec::new());
        ProgramA {
            checked,
            println_call_id,
            point_id,
            sum_id,
        }
    }

    // -- program B: test runner ---------------------------------------------

    fn build_program_b() -> CheckedProgram {
        let mut b = Builder::new();

        // fn helper() -> i32 { 7 }
        let helper_id = b.id();
        let seven = b.int(7, Some(STy::I32));
        let helper_body = b.block(Vec::new(), Some(seven));
        let helper_ret = b.named_type("i32", Some(STy::I32));
        let helper_item = make_fn(
            &mut b,
            helper_id,
            "helper",
            Vis::Private,
            false,
            Vec::new(),
            helper_ret,
            helper_body,
        );
        b.items.push(helper_item);

        // test fn passes() { assert_eq(helper(), 7); }
        let passes_id = b.id();
        let helper_callee = b.callee("helper");
        let helper_call = b.call(
            helper_callee,
            Vec::new(),
            CallTarget::UserFn { item: helper_id },
        );
        let seven_arg = b.int(7, Some(STy::I32));
        let assert_eq_callee = b.callee("assert_eq");
        let assert_eq_call = b.call(
            assert_eq_callee,
            vec![helper_call, seven_arg],
            CallTarget::BuiltinFn {
                path: "std::panic::assert_eq".to_string(),
            },
        );
        let assert_eq_stmt = b.expr_stmt(assert_eq_call);
        let passes_body = b.block(vec![assert_eq_stmt], None);
        let passes_ret = b.ty(TypeKind::Unit, Some(STy::Unit));
        let passes_item = make_fn(
            &mut b,
            passes_id,
            "passes",
            Vis::Private,
            true,
            Vec::new(),
            passes_ret,
            passes_body,
        );
        b.items.push(passes_item);

        // test fn fails() { assert(false); }
        let fails_id = b.id();
        let false_value = b.expr(ExprKind::Bool(false), Some(STy::Bool));
        let assert_callee = b.callee("assert");
        let assert_call = b.call(
            assert_callee,
            vec![false_value],
            CallTarget::BuiltinFn {
                path: "std::panic::assert".to_string(),
            },
        );
        let assert_stmt = b.expr_stmt(assert_call);
        let fails_body = b.block(vec![assert_stmt], None);
        let fails_ret = b.ty(TypeKind::Unit, Some(STy::Unit));
        let fails_item = make_fn(
            &mut b,
            fails_id,
            "fails",
            Vis::Private,
            true,
            Vec::new(),
            fails_ret,
            fails_body,
        );
        b.items.push(fails_item);

        let mut checked = b.finish(None, vec![passes_id, fails_id]);
        checked.facts.main_fn = None;
        checked
    }

    // -- program C: `main -> Result` error path -----------------------------

    fn build_program_c() -> CheckedProgram {
        let mut b = Builder::new();
        let main_id = b.id();

        let new_callee = b.callee("Error::new");
        let boom = b.text("boom");
        let error_call = b.call(
            new_callee,
            vec![boom],
            CallTarget::BuiltinFn {
                path: "Error::new".to_string(),
            },
        );
        let err_callee = b.callee("Err");
        let err_call = b.call(
            err_callee,
            vec![error_call],
            CallTarget::VariantCtor {
                enum_item: 910_000,
                variant: 1,
            },
        );
        let body = b.block(Vec::new(), Some(err_call));
        let result_path = b.path(&["Result"]);
        let result_type = b.ty(
            TypeKind::Path(result_path),
            Some(STy::Result(
                Box::new(STy::Unit),
                Box::new(STy::Nominal {
                    def: 910_001,
                    name: "Error".to_string(),
                    args: Vec::new(),
                }),
            )),
        );
        let main_item = make_fn(
            &mut b,
            main_id,
            "main",
            Vis::Private,
            false,
            Vec::new(),
            result_type,
            body,
        );
        b.items.push(main_item);
        b.finish(Some(main_id), Vec::new())
    }

    // -- program D: operator precedence -------------------------------------

    fn build_program_d() -> CheckedProgram {
        let mut b = Builder::new();
        let calc_id = b.id();
        let a_ty = b.named_type("i32", Some(STy::I32));
        let b_ty = b.named_type("i32", Some(STy::I32));
        let c_ty = b.named_type("i32", Some(STy::I32));
        let param_a = make_param(&mut b, "a", a_ty);
        let param_b = make_param(&mut b, "b", b_ty);
        let param_c = make_param(&mut b, "c", c_ty);
        let value_a = b.var("a", Some(STy::I32));
        let value_b = b.var("b", Some(STy::I32));
        let value_c = b.var("c", Some(STy::I32));
        let product = b.expr(
            ExprKind::Binary {
                op: BinOp::Mul,
                lhs: Box::new(value_b),
                rhs: Box::new(value_c),
            },
            Some(STy::I32),
        );
        let total = b.expr(
            ExprKind::Binary {
                op: BinOp::Add,
                lhs: Box::new(value_a),
                rhs: Box::new(product),
            },
            Some(STy::I32),
        );
        let body = b.block(Vec::new(), Some(total));
        let ret = b.named_type("i32", Some(STy::I32));
        let calc_item = make_fn(
            &mut b,
            calc_id,
            "calc",
            Vis::Public(span()),
            false,
            vec![param_a, param_b, param_c],
            ret,
            body,
        );
        b.items.push(calc_item);
        b.finish(None, Vec::new())
    }

    // -- structural tests ---------------------------------------------------

    #[test]
    fn banner_modules_and_entry_point() {
        let program = build_program_a();
        let code = generate_ok(&program.checked, &GenOptions::default());
        assert!(
            code.starts_with(&format!(
                "// Generated by ravum-lang {} from ravum_program; do not edit.",
                VERSION
            )),
            "unexpected banner:\n{}",
            &code[..code.len().min(200)]
        );
        assert!(code.contains("#![forbid(unsafe_code)]"));
        assert!(code.contains("pub mod rvm_root {"));
        assert!(code.contains("pub fn rv_user_main() -> Result<(), crate::Error>"));
        assert!(code.contains("fn main() -> std::process::ExitCode"));
        assert!(code.contains("crate::rv_install_panic_hook();"));
        assert!(code.contains("eprintln!(\"error: {}\", rv_error.message);"));
        assert!(code.contains("crate::rvm_root::add(2i32, 3i32)"));
    }

    #[test]
    fn structs_impls_and_builtins_are_mapped() {
        let program = build_program_a();
        let code = generate_ok(&program.checked, &GenOptions::default());
        assert!(code.contains("#[derive(Debug, Clone, PartialEq, Copy)]"));
        assert!(code.contains("pub struct Point {"));
        assert!(code.contains("pub x: i32,"));
        assert!(code.contains("impl crate::rvm_root::Point {"));
        assert!(code.contains("fn sum(&self) -> i32"));
        assert!(code.contains("crate::rv_println(\"hello\");"));
        assert!(code.contains("crate::rvm_root::Point { x: 1i32, y: 2i32 }"));
        assert!(code.contains("p.sum()"));
        assert!(code.contains("format!(\"{}\", s)"));
        assert!(code.contains("crate::rv_println(&t);"));
        assert!(code.contains("Ok(())"));
        let _ = &program.sum_id;
    }

    #[test]
    fn no_unsafe_constructs_are_generated() {
        let corpus = [
            generate_ok(&build_program_a().checked, &GenOptions::default()),
            generate_ok(&build_program_b(), &GenOptions::default()),
            generate_ok(&build_program_c(), &GenOptions::default()),
            generate_ok(&build_program_d(), &GenOptions::default()),
        ];
        for code in &corpus {
            for forbidden in [
                "unsafe fn",
                "unsafe {",
                "transmute(",
                "*const ",
                "*mut ",
                "as *const",
                "as *mut",
                "wrapping_add(",
                "wrapping_sub(",
                "wrapping_mul(",
            ] {
                assert!(
                    !code.contains(forbidden),
                    "generated code must not contain `{forbidden}`"
                );
            }
            assert!(!code.contains("std::ptr"));
        }
    }

    #[test]
    fn generation_is_deterministic() {
        let program = build_program_a();
        let options = GenOptions::default();
        let first = generate_ok(&program.checked, &options);
        let second = generate_ok(&program.checked, &options);
        assert_eq!(first, second);
        let third = crate::generate(&program.checked, &options).expect("generation");
        assert_eq!(first, third);
    }

    #[test]
    fn source_markers_follow_the_option() {
        let program = build_program_a();
        let with_markers = generate_ok(&program.checked, &GenOptions::default());
        assert!(with_markers.contains("// rv: main.rv:1"));

        let mut options = GenOptions::default();
        options.source_comments = false;
        let without_markers = generate_ok(&program.checked, &options);
        assert!(!without_markers.contains("// rv:"));
    }

    #[test]
    fn operator_precedence_is_explicit() {
        let code = generate_ok(&build_program_d(), &GenOptions::default());
        assert!(
            code.contains("a + (b * c)"),
            "expected explicit parentheses in:\n{code}"
        );
    }

    #[test]
    fn test_mode_generates_a_runner() {
        let mut options = GenOptions::default();
        options.test_mode = true;
        let code = generate_ok(&build_program_b(), &options);
        assert!(code.contains("crate::rv_install_test_panic_hook();"));
        assert!(code.contains("crate::rv_test(\"passes\""));
        assert!(code.contains("crate::rv_test(\"fails\""));
        assert!(code.contains("println!(\"{} passed; {} failed\", rv_passed, rv_failed);"));
        // The user `main` is not called in test mode.
        assert!(!code.contains("rv_user_main()"));
    }

    #[test]
    fn typed_literals_use_the_inferred_integer_type() {
        let mut b = Builder::new();
        let value_id = b.id();
        let literal = b.int(5, Some(STy::I64));
        let body = b.block(Vec::new(), Some(literal));
        let ret = b.named_type("i64", Some(STy::I64));
        let item = make_fn(
            &mut b,
            value_id,
            "value",
            Vis::Public(span()),
            false,
            Vec::new(),
            ret,
            body,
        );
        b.items.push(item);
        let program = b.finish(None, Vec::new());
        let code = generate_ok(&program, &GenOptions::default());
        assert!(code.contains("5i64"), "expected a typed literal:\n{code}");
    }

    // -- explicit failures --------------------------------------------------

    #[test]
    fn missing_call_fact_is_an_explicit_error() {
        let program = build_program_a();
        let mut facts_only = program.checked.clone();
        facts_only.facts.calls.remove(&program.println_call_id);
        let error = crate::generate(&facts_only, &GenOptions::default())
            .expect_err("missing call fact must fail");
        assert!(
            error.message.contains("missing call fact"),
            "unexpected error: {}",
            error.message
        );
    }

    #[test]
    fn unknown_builtin_is_an_explicit_error() {
        let program = build_program_a();
        let mut facts_only = program.checked.clone();
        facts_only.facts.calls.insert(
            program.println_call_id,
            CallTarget::BuiltinFn {
                path: "io::bogus".to_string(),
            },
        );
        let error = crate::generate(&facts_only, &GenOptions::default())
            .expect_err("unknown builtin must fail");
        assert!(
            error.message.contains("unsupported builtin function"),
            "unexpected error: {}",
            error.message
        );
    }

    #[test]
    fn unknown_nominal_type_is_an_explicit_error() {
        let mut program = build_program_a();
        // Drop every resolved type node of the `Point` annotation.
        let ids: Vec<NodeId> = program.checked.facts.type_nodes.keys().copied().collect();
        for id in ids {
            program.checked.facts.type_nodes.remove(&id);
        }
        let error = crate::generate(&program.checked, &GenOptions::default())
            .expect_err("unresolved nominal type must fail");
        assert!(
            error.message.contains("missing resolved type")
                || error.message.contains("cannot resolve nominal type"),
            "unexpected error: {}",
            error.message
        );
        let _ = program.point_id;
    }

    // -- rustc-backed tests -------------------------------------------------

    fn rustc_available() -> bool {
        match std::process::Command::new("rustc")
            .arg("--version")
            .output()
        {
            Ok(output) => output.status.success(),
            Err(_) => false,
        }
    }

    struct TempDir {
        path: std::path::PathBuf,
    }

    impl TempDir {
        fn new(label: &str) -> TempDir {
            static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.subsec_nanos())
                .unwrap_or(0);
            let path = std::env::temp_dir().join(format!(
                "ravum-codegen-{}-{}-{}-{}",
                std::process::id(),
                unique,
                nanos,
                label
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("cannot create the temporary directory");
            TempDir { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn compile(code: &str, label: &str) -> std::path::PathBuf {
        let dir = TempDir::new(label);
        let source = dir.path.join("program.rs");
        std::fs::write(&source, code).expect("cannot write the generated program");
        let binary = dir.path.join(if cfg!(windows) {
            "program.exe"
        } else {
            "program"
        });
        let output = std::process::Command::new("rustc")
            .arg("--edition")
            .arg("2021")
            .arg("-C")
            .arg("overflow-checks=on")
            .arg("-o")
            .arg(&binary)
            .arg(&source)
            .output()
            .expect("cannot run rustc");
        assert!(
            output.status.success(),
            "rustc rejected `{label}`:\n{}\n--- generated ---\n{}",
            String::from_utf8_lossy(&output.stderr),
            code
        );
        // Keep the directory alive by leaking it: the test binary ends right
        // after, and the OS cleans the temporary directory. This avoids keeping
        // a self-referential path in the returned value.
        std::mem::forget(dir);
        binary
    }

    fn run(binary: &std::path::Path) -> (String, String, i32) {
        run_with_args(binary, &[])
    }

    fn run_with_args(binary: &std::path::Path, args: &[&str]) -> (String, String, i32) {
        let output = std::process::Command::new(binary)
            .args(args)
            .output()
            .expect("cannot run the compiled program");
        (
            String::from_utf8_lossy(&output.stdout).to_string(),
            String::from_utf8_lossy(&output.stderr).to_string(),
            output.status.code().unwrap_or(-1),
        )
    }

    #[test]
    fn prelude_compiles_alone_with_rustc() {
        if !rustc_available() {
            eprintln!("SKIP prelude_compiles_alone_with_rustc: rustc not available");
            return;
        }
        let mut program = String::from(PRELUDE);
        program.push_str("\nfn main() {\n    crate::rv_install_panic_hook();\n}\n");
        let binary = compile(&program, "prelude");
        let (stdout, stderr, code) = run(&binary);
        assert!(stdout.is_empty(), "prelude prints nothing: {stdout}");
        assert!(stderr.is_empty(), "prelude writes nothing: {stderr}");
        assert_eq!(code, 0);
    }

    #[test]
    fn generated_program_compiles_and_runs() {
        if !rustc_available() {
            eprintln!("SKIP generated_program_compiles_and_runs: rustc not available");
            return;
        }
        let code = generate_ok(&build_program_a().checked, &GenOptions::default());
        let binary = compile(&code, "program-a");
        let (stdout, stderr, code) = run(&binary);
        assert_eq!(stdout, "hello\n3\n", "stderr: {stderr}");
        assert_eq!(code, 0, "stderr: {stderr}");
    }

    #[test]
    fn error_returning_main_exits_with_one() {
        if !rustc_available() {
            eprintln!("SKIP error_returning_main_exits_with_one: rustc not available");
            return;
        }
        let code = generate_ok(&build_program_c(), &GenOptions::default());
        let binary = compile(&code, "program-c");
        let (stdout, stderr, code) = run(&binary);
        assert_eq!(stdout, "");
        assert_eq!(stderr, "error: boom\n");
        assert_eq!(code, 1);
    }

    #[test]
    fn generated_test_runner_reports_results() {
        if !rustc_available() {
            eprintln!("SKIP generated_test_runner_reports_results: rustc not available");
            return;
        }
        let mut options = GenOptions::default();
        options.test_mode = true;
        let code = generate_ok(&build_program_b(), &options);
        let binary = compile(&code, "program-b");
        let (stdout, stderr, code) = run(&binary);
        assert!(stdout.contains("test passes ... ok"), "stdout: {stdout}");
        assert!(stdout.contains("test fails ... FAILED"), "stdout: {stdout}");
        assert!(stdout.contains("1 passed; 1 failed"), "stdout: {stdout}");
        assert!(
            stdout.contains("assertion failed at main.rv:1"),
            "stdout: {stdout}"
        );
        assert_eq!(code, 1, "stderr: {stderr}");
    }

    #[test]
    fn panicking_main_reports_panic_and_exit_one() {
        if !rustc_available() {
            eprintln!("SKIP panicking_main_reports_panic_and_exit_one: rustc not available");
            return;
        }
        let mut b = Builder::new();
        let main_id = b.id();
        let callee = b.callee("panic");
        let text = b.text("kaboom");
        let panic_call = b.call(
            callee,
            vec![text],
            CallTarget::BuiltinFn {
                path: "std::panic::panic".to_string(),
            },
        );
        let body = b.block(Vec::new(), Some(panic_call));
        let unit_ret = b.ty(TypeKind::Unit, Some(STy::Unit));
        let main_item = make_fn(
            &mut b,
            main_id,
            "main",
            Vis::Private,
            false,
            Vec::new(),
            unit_ret,
            body,
        );
        b.items.push(main_item);
        let program = b.finish(Some(main_id), Vec::new());
        let code = generate_ok(&program, &GenOptions::default());
        let binary = compile(&code, "program-panic");
        let (_stdout, stderr, code) = run(&binary);
        assert_eq!(stderr, "panic: kaboom\n");
        assert_eq!(code, 1);
    }

    /// End-to-end: source -> loader -> checker -> codegen -> rustc -> run.
    ///
    /// Proves the real pipeline on the shipped examples (`hello`, `fib`,
    /// `calculator`): argument passing, `Result` main, error exit codes and `?`.
    #[test]
    fn e2e_examples_source_to_binary() {
        if !rustc_available() {
            eprintln!("SKIP e2e_examples_source_to_binary: rustc not available");
            return;
        }
        let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        // (example, program args, expected stdout, expected stderr fragment, exit code)
        let cases: &[(&str, &[&str], &str, &str, i32)] = &[
            ("hello", &[], "hello, ravum\n", "", 0),
            ("fib", &[], "0\n1\n1\n2\n3\n5\n8\n13\n21\n34\n55\n", "", 0),
            ("calculator", &["1", "+", "2"], "3\n", "", 0),
            ("calculator", &["1", "/", "0"], "", "division by zero", 1),
        ];

        for &(name, args, expected_stdout, expected_stderr, expected_status) in cases {
            let directory = examples.join(name);
            assert!(
                directory.join("ravum.toml").exists(),
                "example `{name}` is missing at {}",
                directory.display()
            );
            let loaded = ravum_lang_sema::load_package(&directory);
            assert!(
                loaded.diags.is_empty(),
                "loader diagnostics for `{name}`:\n{}",
                loaded.diags.render(&loaded.sources)
            );
            let program = loaded
                .program
                .expect("the loader produced no program for a valid package");
            let check_options = ravum_lang_sema::facts::CheckOptions::default();
            let checked = ravum_lang_sema::check(&program, &loaded.sources, &check_options);
            assert!(
                checked.diags.is_empty(),
                "checker diagnostics for `{name}`:\n{}",
                checked.diags.render(&loaded.sources)
            );
            let checked = checked
                .checked
                .expect("the checker produced no CheckedProgram");
            let mut options = GenOptions::default();
            options.crate_name = name.to_string();
            let code = generate_ok(&checked, &options);
            let binary = compile(&code, name);
            let (stdout, stderr, status) = run_with_args(&binary, args);
            assert_eq!(
                stdout, expected_stdout,
                "example `{name}` stdout mismatch (stderr: {stderr})\n--- generated ---\n{code}"
            );
            if !expected_stderr.is_empty() {
                assert!(
                    stderr.contains(expected_stderr),
                    "example `{name}` stderr mismatch: {stderr}"
                );
            }
            assert_eq!(
                status, expected_status,
                "example `{name}` exit code mismatch (stderr: {stderr})"
            );
        }
    }

    /// End-to-end FFI: `extern "C"` declaration, `unsafe { ... }` call, real
    /// C runtime symbol, generated extern wrapper compiled by rustc.
    #[test]
    fn e2e_extern_ffi_compiles_and_runs() {
        if !rustc_available() {
            eprintln!("SKIP e2e_extern_ffi_compiles_and_runs: rustc not available");
            return;
        }
        let dir = std::env::temp_dir().join(format!("ravum-ffi-e2e-{}", std::process::id()));
        let src = dir.join("src");
        std::fs::create_dir_all(&src).expect("cannot create FFI test package");
        std::fs::write(
            dir.join("ravum.toml"),
            "[package]\nname = \"ffi-abs\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
        )
        .expect("cannot write manifest");
        std::fs::write(
            src.join("main.rv"),
            r#"use std::io;

extern "C" {
    fn abs(x: i32) -> i32;
}

fn main() {
    let value = unsafe { abs(-9) };
    io::println(&value.to_string());
}
"#,
        )
        .expect("cannot write source");

        let loaded = ravum_lang_sema::load_package(&dir);
        assert!(
            loaded.diags.is_empty(),
            "loader diagnostics:\n{}",
            loaded.diags.render(&loaded.sources)
        );
        let program = loaded.program.expect("no program for the FFI package");
        let options = ravum_lang_sema::facts::CheckOptions::default();
        let checked = ravum_lang_sema::check(&program, &loaded.sources, &options);
        assert!(
            checked.diags.is_empty(),
            "checker diagnostics:\n{}",
            checked.diags.render(&loaded.sources)
        );
        let checked = checked.checked.expect("no CheckedProgram");
        let code = generate_ok(&checked, &GenOptions::default());
        assert!(
            code.contains("#![deny(unsafe_code)]"),
            "FFI programs must keep `deny(unsafe_code)` at crate level:\n{code}"
        );
        assert!(
            code.contains("#[allow(unsafe_code)]"),
            "the generated wrapper must carry the only local `allow(unsafe_code)`:\n{code}"
        );
        assert!(
            code.contains("unsafe { rvm_ffi_"),
            "the generated extern wrapper must contain the unsafe call:\n{code}"
        );

        let binary = compile(&code, "ffi-abs");
        let (stdout, stderr, status) = run(&binary);
        assert_eq!(stdout, "9\n", "stderr: {stderr}\n--- generated ---\n{code}");
        assert_eq!(status, 0, "stderr: {stderr}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -- program E: broad language surface, compiled and executed -----------

    fn method_builtin(
        b: &mut Builder,
        receiver: Expr,
        name: &str,
        args: Vec<Expr>,
        path: &str,
        ret: STy,
    ) -> Expr {
        b.method(
            receiver,
            name,
            args,
            MethodTarget::Builtin {
                path: path.to_string(),
                ret,
            },
        )
    }

    fn free_call(b: &mut Builder, name: &str, args: Vec<Expr>, path: &str) -> Expr {
        let callee = b.callee(name);
        b.call(
            callee,
            args,
            CallTarget::BuiltinFn {
                path: path.to_string(),
            },
        )
    }

    fn user_call(b: &mut Builder, name: &str, args: Vec<Expr>, item: NodeId) -> Expr {
        let callee = b.callee(name);
        b.call(callee, args, CallTarget::UserFn { item })
    }

    fn print_value(b: &mut Builder, value: Expr, owner: &str) -> Stmt {
        let path = format!("{owner}::to_string");
        let text = method_builtin(b, value, "to_string", Vec::new(), &path, STy::String);
        let borrowed = b.expr(
            ExprKind::Unary {
                op: UnOp::Ref,
                expr: Box::new(text),
            },
            Some(STy::Ref {
                mutable: false,
                inner: Box::new(STy::String),
            }),
        );
        let call = free_call(b, "println", vec![borrowed], "std::io::println");
        b.expr_stmt(call)
    }

    fn generic_path(b: &mut Builder, name: &str, args: Vec<AstType>) -> Path {
        let id = b.id();
        let ident = b.ident(name);
        Path {
            id,
            root: PathRoot::Plain,
            segments: vec![PathSegment { name: ident, args }],
            span: span(),
        }
    }

    fn binding_pat(b: &mut Builder, name: &str, mutable: bool) -> Pat {
        let ident = b.ident(name);
        b.pat(
            PatKind::Binding {
                mutable,
                name: ident,
            },
            None,
        )
    }

    fn variant_pattern(
        b: &mut Builder,
        enum_item: NodeId,
        variant: usize,
        names: &[&str],
        fields: VariantPatFields,
    ) -> Pat {
        let path = b.path(names);
        b.facts
            .resolutions
            .insert(path.id, Resolution::Variant { enum_item, variant });
        b.pat(PatKind::Variant { path, fields }, None)
    }

    fn unit_variant_pattern(
        b: &mut Builder,
        enum_item: NodeId,
        variant: usize,
        names: &[&str],
    ) -> Pat {
        let path = b.path(names);
        b.facts
            .resolutions
            .insert(path.id, Resolution::Variant { enum_item, variant });
        b.pat(PatKind::Path(path), None)
    }

    struct ProgramE {
        checked: CheckedProgram,
        shape_id: NodeId,
        area_id: NodeId,
        max_id: NodeId,
        sign_id: NodeId,
        sum_to_id: NodeId,
        total_id: NodeId,
        cast_round_id: NodeId,
        greeting_id: NodeId,
        countdown_id: NodeId,
        type_id: NodeId,
        parse_number_id: NodeId,
        twice_id: NodeId,
    }

    fn build_program_e() -> ProgramE {
        let mut b = Builder::new();

        // const LIMIT: i32 = 10;
        let limit_id = b.id();
        let limit_ty = b.named_type("i32", Some(STy::I32));
        let limit_name = b.ident("LIMIT");
        let limit_value = b.int(10, Some(STy::I32));
        let limit_item = Item {
            id: limit_id,
            vis: Vis::Private,
            kind: ItemKind::Const(ravum_lang_syntax::ast::ConstItem {
                name: limit_name,
                ty: limit_ty,
                value: limit_value,
                span: span(),
            }),
            span: span(),
        };
        b.items.push(limit_item);
        b.facts.consts.insert(
            limit_id,
            ConstInfo {
                item: limit_id,
                rust_path: "crate::LIMIT".to_string(),
                name: "LIMIT".to_string(),
                ty: STy::I32,
                value: ConstValue::Int(10),
            },
        );

        // enum Shape { Circle(f64), Rect { w: f64, h: f64 }, Empty }
        let shape_id = b.id();
        let shape_ty = STy::Nominal {
            def: shape_id,
            name: "Shape".to_string(),
            args: Vec::new(),
        };
        let circle_payload = b.named_type("f64", Some(STy::F64));
        let rect_w_ty = b.named_type("f64", Some(STy::F64));
        let rect_h_ty = b.named_type("f64", Some(STy::F64));
        let variant_circle = b.id();
        let variant_rect = b.id();
        let variant_empty = b.id();
        let circle_name = b.ident("Circle");
        let rect_name = b.ident("Rect");
        let empty_name = b.ident("Empty");
        let rect_w = b.ident("w");
        let rect_h = b.ident("h");
        let shape_name = b.ident("Shape");
        let shape_item = Item {
            id: shape_id,
            vis: Vis::Public(span()),
            kind: ItemKind::Enum(ravum_lang_syntax::ast::EnumItem {
                name: shape_name,
                generics: Vec::new(),
                variants: vec![
                    ravum_lang_syntax::ast::VariantDef {
                        id: variant_circle,
                        name: circle_name,
                        fields: VariantDefFields::Tuple(vec![circle_payload]),
                        span: span(),
                    },
                    ravum_lang_syntax::ast::VariantDef {
                        id: variant_rect,
                        name: rect_name,
                        fields: VariantDefFields::Struct(vec![
                            FieldDef {
                                id: b.id(),
                                vis: Vis::Public(span()),
                                name: rect_w,
                                ty: rect_w_ty,
                                span: span(),
                            },
                            FieldDef {
                                id: b.id(),
                                vis: Vis::Public(span()),
                                name: rect_h,
                                ty: rect_h_ty,
                                span: span(),
                            },
                        ]),
                        span: span(),
                    },
                    ravum_lang_syntax::ast::VariantDef {
                        id: variant_empty,
                        name: empty_name,
                        fields: VariantDefFields::Unit,
                        span: span(),
                    },
                ],
                span: span(),
            }),
            span: span(),
        };
        b.items.push(shape_item);

        // fn area(shape: Shape) -> f64 { match shape { ... } }
        let area_id = b.id();
        let shape_param_ty = b.named_type("Shape", Some(shape_ty.clone()));
        let area_param = make_param(&mut b, "shape", shape_param_ty);
        let scrutinee = b.var("shape", Some(shape_ty.clone()));

        let r_binding = binding_pat(&mut b, "r", false);
        let circle_pattern = variant_pattern(
            &mut b,
            shape_id,
            0,
            &["Shape", "Circle"],
            VariantPatFields::Tuple(vec![r_binding]),
        );
        let three = b.expr(
            ExprKind::Float {
                text: "3.0".to_string(),
                suffix: None,
            },
            Some(STy::F64),
        );
        let r_value = b.var("r", Some(STy::F64));
        let r_again = b.var("r", Some(STy::F64));
        let area_product = b.expr(
            ExprKind::Binary {
                op: BinOp::Mul,
                lhs: Box::new(three),
                rhs: Box::new(r_value),
            },
            Some(STy::F64),
        );
        let circle_area = b.expr(
            ExprKind::Binary {
                op: BinOp::Mul,
                lhs: Box::new(area_product),
                rhs: Box::new(r_again),
            },
            Some(STy::F64),
        );

        let w_binding = binding_pat(&mut b, "w", false);
        let h_binding = binding_pat(&mut b, "h", false);
        let w_name = b.ident("w");
        let h_name = b.ident("h");
        let rect_pattern = variant_pattern(
            &mut b,
            shape_id,
            1,
            &["Shape", "Rect"],
            VariantPatFields::Struct(vec![
                ravum_lang_syntax::ast::FieldPat {
                    name: w_name,
                    pat: w_binding,
                },
                ravum_lang_syntax::ast::FieldPat {
                    name: h_name,
                    pat: h_binding,
                },
            ]),
        );
        let w_value = b.var("w", Some(STy::F64));
        let h_value = b.var("h", Some(STy::F64));
        let rect_area = b.expr(
            ExprKind::Binary {
                op: BinOp::Mul,
                lhs: Box::new(w_value),
                rhs: Box::new(h_value),
            },
            Some(STy::F64),
        );

        let empty_pattern = unit_variant_pattern(&mut b, shape_id, 2, &["Shape", "Empty"]);
        let zero_area = b.expr(
            ExprKind::Float {
                text: "0.0".to_string(),
                suffix: None,
            },
            Some(STy::F64),
        );

        let arm_circle = ravum_lang_syntax::ast::MatchArm {
            id: b.id(),
            pat: circle_pattern,
            body: circle_area,
            span: span(),
        };
        let arm_rect = ravum_lang_syntax::ast::MatchArm {
            id: b.id(),
            pat: rect_pattern,
            body: rect_area,
            span: span(),
        };
        let arm_empty = ravum_lang_syntax::ast::MatchArm {
            id: b.id(),
            pat: empty_pattern,
            body: zero_area,
            span: span(),
        };
        let area_match = b.expr(
            ExprKind::Match {
                scrutinee: Box::new(scrutinee),
                arms: vec![arm_circle, arm_rect, arm_empty],
            },
            Some(STy::F64),
        );
        let area_body = b.block(Vec::new(), Some(area_match));
        let area_ret = b.named_type("f64", Some(STy::F64));
        let area_item = make_fn(
            &mut b,
            area_id,
            "area",
            Vis::Public(span()),
            false,
            vec![area_param],
            area_ret,
            area_body,
        );
        b.items.push(area_item);

        // fn max(a: i32, b: i32) -> i32 { if a > b { a } else { b } }
        let max_id = b.id();
        let a_ty = b.named_type("i32", Some(STy::I32));
        let b_ty = b.named_type("i32", Some(STy::I32));
        let max_param_a = make_param(&mut b, "a", a_ty);
        let max_param_b = make_param(&mut b, "b", b_ty);
        let a_value = b.var("a", Some(STy::I32));
        let b_value = b.var("b", Some(STy::I32));
        let compare = b.expr(
            ExprKind::Binary {
                op: BinOp::Gt,
                lhs: Box::new(a_value),
                rhs: Box::new(b_value),
            },
            Some(STy::Bool),
        );
        let then_value = b.var("a", Some(STy::I32));
        let then_block = b.block(Vec::new(), Some(then_value));
        let else_value = b.var("b", Some(STy::I32));
        let else_block = b.block(Vec::new(), Some(else_value));
        let else_expr = b.expr(ExprKind::Block(else_block), Some(STy::I32));
        let max_if = b.expr(
            ExprKind::If {
                cond: Box::new(compare),
                then_block,
                else_branch: Some(Box::new(else_expr)),
            },
            Some(STy::I32),
        );
        let max_body = b.block(Vec::new(), Some(max_if));
        let max_ret = b.named_type("i32", Some(STy::I32));
        let max_item = make_fn(
            &mut b,
            max_id,
            "max",
            Vis::Public(span()),
            false,
            vec![max_param_a, max_param_b],
            max_ret,
            max_body,
        );
        b.items.push(max_item);

        // fn sign(x: i32) -> i32 { if x > 0 { 1 } else if x < 0 { -1 } else { 0 } }
        let sign_id = b.id();
        let x_ty = b.named_type("i32", Some(STy::I32));
        let sign_param = make_param(&mut b, "x", x_ty);
        let x_pos = b.var("x", Some(STy::I32));
        let zero_compare = b.int(0, Some(STy::I32));
        let positive = b.expr(
            ExprKind::Binary {
                op: BinOp::Gt,
                lhs: Box::new(x_pos),
                rhs: Box::new(zero_compare),
            },
            Some(STy::Bool),
        );
        let one = b.int(1, Some(STy::I32));
        let positive_block = b.block(Vec::new(), Some(one));
        let x_neg = b.var("x", Some(STy::I32));
        let zero_compare_2 = b.int(0, Some(STy::I32));
        let negative_cond = b.expr(
            ExprKind::Binary {
                op: BinOp::Lt,
                lhs: Box::new(x_neg),
                rhs: Box::new(zero_compare_2),
            },
            Some(STy::Bool),
        );
        let one_neg = b.int(1, Some(STy::I32));
        let negative_value = b.expr(
            ExprKind::Unary {
                op: UnOp::Neg,
                expr: Box::new(one_neg),
            },
            Some(STy::I32),
        );
        let negative_block = b.block(Vec::new(), Some(negative_value));
        let zero_value = b.int(0, Some(STy::I32));
        let zero_block = b.block(Vec::new(), Some(zero_value));
        let zero_expr = b.expr(ExprKind::Block(zero_block), Some(STy::I32));
        let nested_if = b.expr(
            ExprKind::If {
                cond: Box::new(negative_cond),
                then_block: negative_block,
                else_branch: Some(Box::new(zero_expr)),
            },
            Some(STy::I32),
        );
        let sign_if = b.expr(
            ExprKind::If {
                cond: Box::new(positive),
                then_block: positive_block,
                else_branch: Some(Box::new(nested_if)),
            },
            Some(STy::I32),
        );
        let sign_body = b.block(Vec::new(), Some(sign_if));
        let sign_ret = b.named_type("i32", Some(STy::I32));
        let sign_item = make_fn(
            &mut b,
            sign_id,
            "sign",
            Vis::Public(span()),
            false,
            vec![sign_param],
            sign_ret,
            sign_body,
        );
        b.items.push(sign_item);

        // fn sum_to(n: i32) -> i32 { let mut total = 0; for i in 0..n { total = total + i; } total }
        let sum_to_id = b.id();
        let n_ty = b.named_type("i32", Some(STy::I32));
        let sum_param = make_param(&mut b, "n", n_ty);
        let total_pat = binding_pat(&mut b, "total", true);
        let total_ty = b.named_type("i32", Some(STy::I32));
        let zero_init = b.int(0, Some(STy::I32));
        let total_let = Stmt {
            id: b.id(),
            kind: StmtKind::Let(LetStmt {
                pat: total_pat,
                ty: Some(total_ty),
                init: Some(zero_init),
            }),
            span: span(),
        };
        let range_start = b.int(0, Some(STy::I32));
        let range_end = b.var("n", Some(STy::I32));
        let range = b.expr(
            ExprKind::Range {
                start: Some(Box::new(range_start)),
                end: Some(Box::new(range_end)),
                inclusive: false,
            },
            Some(STy::I32),
        );
        let total_target = b.var("total", Some(STy::I32));
        let total_value = b.var("total", Some(STy::I32));
        let loop_i = b.var("i", Some(STy::I32));
        let sum_expr = b.expr(
            ExprKind::Binary {
                op: BinOp::Add,
                lhs: Box::new(total_value),
                rhs: Box::new(loop_i),
            },
            Some(STy::I32),
        );
        let assign = b.expr(
            ExprKind::Assign {
                target: Box::new(total_target),
                value: Box::new(sum_expr),
            },
            Some(STy::Unit),
        );
        let assign_stmt = b.expr_stmt(assign);
        let for_body = b.block(vec![assign_stmt], None);
        let loop_pat = binding_pat(&mut b, "i", false);
        let for_expr = b.expr(
            ExprKind::For {
                pat: loop_pat,
                iter: Box::new(range),
                body: for_body,
            },
            Some(STy::Unit),
        );
        let for_stmt = b.expr_stmt(for_expr);
        let total_tail = b.var("total", Some(STy::I32));
        let sum_body = b.block(vec![total_let, for_stmt], Some(total_tail));
        let sum_ret = b.named_type("i32", Some(STy::I32));
        let sum_item = make_fn(
            &mut b,
            sum_to_id,
            "sum_to",
            Vis::Public(span()),
            false,
            vec![sum_param],
            sum_ret,
            sum_body,
        );
        b.items.push(sum_item);

        // fn total(values: Vec<i32>) -> i32 { let mut sum = 0; for v in values { sum = sum + v; } sum }
        let total_id = b.id();
        let element_ty = b.named_type("i32", Some(STy::I32));
        let vec_path = generic_path(&mut b, "Vec", vec![element_ty]);
        let vec_ty = b.ty(TypeKind::Path(vec_path), Some(STy::Vec(Box::new(STy::I32))));
        let values_param = make_param(&mut b, "values", vec_ty);
        let sum_pat = binding_pat(&mut b, "sum", true);
        let sum_ty = b.named_type("i32", Some(STy::I32));
        let sum_init = b.int(0, Some(STy::I32));
        let sum_let = Stmt {
            id: b.id(),
            kind: StmtKind::Let(LetStmt {
                pat: sum_pat,
                ty: Some(sum_ty),
                init: Some(sum_init),
            }),
            span: span(),
        };
        let values_iter = b.var("values", Some(STy::Vec(Box::new(STy::I32))));
        let sum_target = b.var("sum", Some(STy::I32));
        let sum_value = b.var("sum", Some(STy::I32));
        let item_value = b.var("v", Some(STy::I32));
        let add_item = b.expr(
            ExprKind::Binary {
                op: BinOp::Add,
                lhs: Box::new(sum_value),
                rhs: Box::new(item_value),
            },
            Some(STy::I32),
        );
        let sum_assign = b.expr(
            ExprKind::Assign {
                target: Box::new(sum_target),
                value: Box::new(add_item),
            },
            Some(STy::Unit),
        );
        let sum_assign_stmt = b.expr_stmt(sum_assign);
        let total_for_body = b.block(vec![sum_assign_stmt], None);
        let item_pat = binding_pat(&mut b, "v", false);
        let total_for = b.expr(
            ExprKind::For {
                pat: item_pat,
                iter: Box::new(values_iter),
                body: total_for_body,
            },
            Some(STy::Unit),
        );
        let total_for_stmt = b.expr_stmt(total_for);
        let total_tail_value = b.var("sum", Some(STy::I32));
        let total_body = b.block(vec![sum_let, total_for_stmt], Some(total_tail_value));
        let total_ret = b.named_type("i32", Some(STy::I32));
        let total_item = make_fn(
            &mut b,
            total_id,
            "total",
            Vis::Public(span()),
            false,
            vec![values_param],
            total_ret,
            total_body,
        );
        b.items.push(total_item);

        // fn cast_round(x: i32) -> f64 { x as f64 }
        let cast_round_id = b.id();
        let cast_param_ty = b.named_type("i32", Some(STy::I32));
        let cast_param = make_param(&mut b, "x", cast_param_ty);
        let cast_source = b.var("x", Some(STy::I32));
        let cast_target = b.named_type("f64", Some(STy::F64));
        let cast_expr = b.expr(
            ExprKind::Cast {
                expr: Box::new(cast_source),
                ty: cast_target,
            },
            Some(STy::F64),
        );
        let cast_body = b.block(Vec::new(), Some(cast_expr));
        let cast_ret = b.named_type("f64", Some(STy::F64));
        let cast_item = make_fn(
            &mut b,
            cast_round_id,
            "cast_round",
            Vis::Public(span()),
            false,
            vec![cast_param],
            cast_ret,
            cast_body,
        );
        b.items.push(cast_item);

        // fn greeting(name: String) -> String { let trimmed = name.trim(); trimmed.to_uppercase() }
        let greeting_id = b.id();
        let name_ty = b.named_type("String", Some(STy::String));
        let name_param = make_param(&mut b, "name", name_ty);
        let name_value = b.var("name", Some(STy::String));
        let trim_call = method_builtin(
            &mut b,
            name_value,
            "trim",
            Vec::new(),
            "String::trim",
            STy::String,
        );
        let trimmed_ty = b.named_type("String", Some(STy::String));
        let trim_let = Stmt {
            id: b.id(),
            kind: StmtKind::Let(LetStmt {
                pat: binding_pat(&mut b, "trimmed", false),
                ty: Some(trimmed_ty),
                init: Some(trim_call),
            }),
            span: span(),
        };
        let trimmed_value = b.var("trimmed", Some(STy::String));
        let upper = method_builtin(
            &mut b,
            trimmed_value,
            "to_uppercase",
            Vec::new(),
            "String::to_uppercase",
            STy::String,
        );
        let greeting_body = b.block(vec![trim_let], Some(upper));
        let greeting_ret = b.named_type("String", Some(STy::String));
        let greeting_item = make_fn(
            &mut b,
            greeting_id,
            "greeting",
            Vis::Public(span()),
            false,
            vec![name_param],
            greeting_ret,
            greeting_body,
        );
        b.items.push(greeting_item);

        // fn countdown(n: i32) -> i32 { let mut i = n; while i > 0 { i = i - 1; } i }
        let countdown_id = b.id();
        let countdown_ty = b.named_type("i32", Some(STy::I32));
        let countdown_param = make_param(&mut b, "n", countdown_ty);
        let countdown_init = b.var("n", Some(STy::I32));
        let countdown_annotation = b.named_type("i32", Some(STy::I32));
        let countdown_let = Stmt {
            id: b.id(),
            kind: StmtKind::Let(LetStmt {
                pat: binding_pat(&mut b, "i", true),
                ty: Some(countdown_annotation),
                init: Some(countdown_init),
            }),
            span: span(),
        };
        let i_cond = b.var("i", Some(STy::I32));
        let zero_limit = b.int(0, Some(STy::I32));
        let while_cond = b.expr(
            ExprKind::Binary {
                op: BinOp::Gt,
                lhs: Box::new(i_cond),
                rhs: Box::new(zero_limit),
            },
            Some(STy::Bool),
        );
        let i_target = b.var("i", Some(STy::I32));
        let i_value = b.var("i", Some(STy::I32));
        let one_step = b.int(1, Some(STy::I32));
        let decrement = b.expr(
            ExprKind::Binary {
                op: BinOp::Sub,
                lhs: Box::new(i_value),
                rhs: Box::new(one_step),
            },
            Some(STy::I32),
        );
        let i_assign = b.expr(
            ExprKind::Assign {
                target: Box::new(i_target),
                value: Box::new(decrement),
            },
            Some(STy::Unit),
        );
        let i_assign_stmt = b.expr_stmt(i_assign);
        let while_body = b.block(vec![i_assign_stmt], None);
        let while_expr = b.expr(
            ExprKind::While {
                cond: Box::new(while_cond),
                body: while_body,
            },
            Some(STy::Unit),
        );
        let while_stmt = b.expr_stmt(while_expr);
        let countdown_tail = b.var("i", Some(STy::I32));
        let countdown_body = b.block(vec![countdown_let, while_stmt], Some(countdown_tail));
        let countdown_ret = b.named_type("i32", Some(STy::I32));
        let countdown_item = make_fn(
            &mut b,
            countdown_id,
            "countdown",
            Vis::Public(span()),
            false,
            vec![countdown_param],
            countdown_ret,
            countdown_body,
        );
        b.items.push(countdown_item);

        // fn type() -> i32 { 4 }  (Ravum identifier that is a Rust keyword)
        let type_id = b.id();
        let four = b.int(4, Some(STy::I32));
        let type_body = b.block(Vec::new(), Some(four));
        let type_ret = b.named_type("i32", Some(STy::I32));
        let type_item = make_fn(
            &mut b,
            type_id,
            "type",
            Vis::Public(span()),
            false,
            Vec::new(),
            type_ret,
            type_body,
        );
        b.items.push(type_item);

        // fn parse_number(text: &str) -> Result<i64, Error>
        let parse_number_id = b.id();
        let str_ty = b.named_type("str", Some(STy::Str));
        let text_ty = b.ty(
            TypeKind::Ref {
                mutable: false,
                inner: Box::new(str_ty),
            },
            Some(STy::Ref {
                mutable: false,
                inner: Box::new(STy::Str),
            }),
        );
        let text_param = make_param(&mut b, "text", text_ty);
        let text_value = b.var(
            "text",
            Some(STy::Ref {
                mutable: false,
                inner: Box::new(STy::Str),
            }),
        );
        let parsed_ty = STy::Result(
            Box::new(STy::I64),
            Box::new(STy::Nominal {
                def: 920_001,
                name: "Error".to_string(),
                args: Vec::new(),
            }),
        );
        let parse_call = method_builtin(
            &mut b,
            text_value,
            "parse_i64",
            Vec::new(),
            "String::parse_i64",
            parsed_ty.clone(),
        );
        let tried = b.expr(ExprKind::Try(Box::new(parse_call)), Some(STy::I64));
        let number_annotation = b.named_type("i64", Some(STy::I64));
        let number_let = Stmt {
            id: b.id(),
            kind: StmtKind::Let(LetStmt {
                pat: binding_pat(&mut b, "number", false),
                ty: Some(number_annotation),
                init: Some(tried),
            }),
            span: span(),
        };
        let number_value = b.var("number", Some(STy::I64));
        let ok_callee = b.callee("Ok");
        let ok_number = b.call(
            ok_callee,
            vec![number_value],
            CallTarget::VariantCtor {
                enum_item: 920_002,
                variant: 0,
            },
        );
        let parse_body = b.block(vec![number_let], Some(ok_number));
        let parse_result_path = b.path(&["Result"]);
        let parse_ret = b.ty(TypeKind::Path(parse_result_path), Some(parsed_ty));
        let parse_item = make_fn(
            &mut b,
            parse_number_id,
            "parse_number",
            Vis::Public(span()),
            false,
            vec![text_param],
            parse_ret,
            parse_body,
        );
        b.items.push(parse_item);

        // mod util { pub fn twice(x: i32) -> i32 { x * 2 } }
        let twice_id = b.id();
        let twice_ty = b.named_type("i32", Some(STy::I32));
        let twice_param = make_param(&mut b, "x", twice_ty);
        let twice_x = b.var("x", Some(STy::I32));
        let two_factor = b.int(2, Some(STy::I32));
        let twice_expr = b.expr(
            ExprKind::Binary {
                op: BinOp::Mul,
                lhs: Box::new(twice_x),
                rhs: Box::new(two_factor),
            },
            Some(STy::I32),
        );
        let twice_body = b.block(Vec::new(), Some(twice_expr));
        let twice_ret = b.named_type("i32", Some(STy::I32));
        let twice_item = make_fn(
            &mut b,
            twice_id,
            "twice",
            Vis::Public(span()),
            false,
            vec![twice_param],
            twice_ret,
            twice_body,
        );
        let module_id = b.id();
        let module_name = b.ident("util");
        let util_module = Module {
            id: module_id,
            name: Some(module_name),
            items: vec![twice_item],
            span: span(),
        };
        let util_item = Item {
            id: b.id(),
            vis: Vis::Private,
            kind: ItemKind::Mod(util_module),
            span: span(),
        };
        b.items.push(util_item);

        // fn main() -> Result<(), Error> { ... }
        let main_id = b.id();
        let mut statements: Vec<Stmt> = Vec::new();

        // let circle: Shape = Shape::Circle(2.0);
        let circle_call = {
            let callee = b.callee("Circle");
            let payload = b.expr(
                ExprKind::Float {
                    text: "2.0".to_string(),
                    suffix: None,
                },
                Some(STy::F64),
            );
            b.call(
                callee,
                vec![payload],
                CallTarget::VariantCtor {
                    enum_item: shape_id,
                    variant: 0,
                },
            )
        };
        let shape_annotation = b.named_type("Shape", Some(shape_ty.clone()));
        statements.push(b.let_stmt("circle", Some(shape_annotation), circle_call));
        let circle_arg = b.var("circle", Some(shape_ty.clone()));
        let area_call = user_call(&mut b, "area", vec![circle_arg], area_id);
        statements.push(print_value(&mut b, area_call, "f64"));

        // let rect: Shape = Shape::Rect { w: 2.0, h: 3.0 };
        let rect_literal = {
            let path = b.path(&["Shape", "Rect"]);
            b.facts.resolutions.insert(
                path.id,
                Resolution::Variant {
                    enum_item: shape_id,
                    variant: 1,
                },
            );
            let w_name = b.ident("w");
            let h_name = b.ident("h");
            let w_value = b.expr(
                ExprKind::Float {
                    text: "2.0".to_string(),
                    suffix: None,
                },
                Some(STy::F64),
            );
            let h_value = b.expr(
                ExprKind::Float {
                    text: "3.0".to_string(),
                    suffix: None,
                },
                Some(STy::F64),
            );
            b.expr(
                ExprKind::StructLit {
                    path,
                    fields: vec![
                        FieldInit {
                            name: w_name,
                            value: w_value,
                        },
                        FieldInit {
                            name: h_name,
                            value: h_value,
                        },
                    ],
                },
                Some(shape_ty.clone()),
            )
        };
        let rect_annotation = b.named_type("Shape", Some(shape_ty.clone()));
        statements.push(b.let_stmt("rect", Some(rect_annotation), rect_literal));
        let rect_arg = b.var("rect", Some(shape_ty.clone()));
        let area_rect = user_call(&mut b, "area", vec![rect_arg], area_id);
        statements.push(print_value(&mut b, area_rect, "f64"));

        // let empty: Shape = Shape::Empty;
        let empty_value = {
            let path = b.path(&["Shape", "Empty"]);
            b.facts.resolutions.insert(
                path.id,
                Resolution::Variant {
                    enum_item: shape_id,
                    variant: 2,
                },
            );
            b.expr(ExprKind::Path(path), Some(shape_ty.clone()))
        };
        let empty_annotation = b.named_type("Shape", Some(shape_ty.clone()));
        statements.push(b.let_stmt("empty", Some(empty_annotation), empty_value));
        let empty_arg = b.var("empty", Some(shape_ty.clone()));
        let area_empty = user_call(&mut b, "area", vec![empty_arg], area_id);
        statements.push(print_value(&mut b, area_empty, "f64"));

        // print max(3, 7)
        let max_args = vec![b.int(3, Some(STy::I32)), b.int(7, Some(STy::I32))];
        let max_call = user_call(&mut b, "max", max_args, max_id);
        statements.push(print_value(&mut b, max_call, "i32"));

        // print sum_to(4)
        let sum_arg = b.int(4, Some(STy::I32));
        let sum_call = user_call(&mut b, "sum_to", vec![sum_arg], sum_to_id);
        statements.push(print_value(&mut b, sum_call, "i32"));

        // let mut values: Vec<i32> = Vec::new(); values.push(1); push(2); push(3); print total(values)
        let values_new = free_call(&mut b, "Vec::new", Vec::new(), "Vec::new");
        let values_element = b.named_type("i32", Some(STy::I32));
        let values_path = generic_path(&mut b, "Vec", vec![values_element]);
        let values_ty = b.ty(
            TypeKind::Path(values_path),
            Some(STy::Vec(Box::new(STy::I32))),
        );
        let values_let = Stmt {
            id: b.id(),
            kind: StmtKind::Let(LetStmt {
                pat: binding_pat(&mut b, "values", true),
                ty: Some(values_ty),
                init: Some(values_new),
            }),
            span: span(),
        };
        statements.push(values_let);
        for value in [1u128, 2, 3] {
            let receiver = b.var("values", Some(STy::Vec(Box::new(STy::I32))));
            let argument = b.int(value, Some(STy::I32));
            let push = method_builtin(
                &mut b,
                receiver,
                "push",
                vec![argument],
                "Vec::push",
                STy::Unit,
            );
            statements.push(b.expr_stmt(push));
        }
        let total_arg = b.var("values", Some(STy::Vec(Box::new(STy::I32))));
        let total_call = user_call(&mut b, "total", vec![total_arg], total_id);
        statements.push(print_value(&mut b, total_call, "i32"));

        // print cast_round(3)
        let cast_arg = b.int(3, Some(STy::I32));
        let cast_call = user_call(&mut b, "cast_round", vec![cast_arg], cast_round_id);
        statements.push(print_value(&mut b, cast_call, "f64"));

        // print greeting("  hi  ")
        let greeting_text = b.text("  hi  ");
        let greeting_arg = free_call(&mut b, "String::from", vec![greeting_text], "String::from");
        let greeting_call = user_call(&mut b, "greeting", vec![greeting_arg], greeting_id);
        statements.push(print_value(&mut b, greeting_call, "String"));

        // print countdown(3)
        let countdown_arg = b.int(3, Some(STy::I32));
        let countdown_call = user_call(&mut b, "countdown", vec![countdown_arg], countdown_id);
        statements.push(print_value(&mut b, countdown_call, "i32"));

        // print type() + util::twice(3)
        let type_call = user_call(&mut b, "type", Vec::new(), type_id);
        let twice_arg = b.int(3, Some(STy::I32));
        let twice_call = user_call(&mut b, "twice", vec![twice_arg], twice_id);
        let combined = b.expr(
            ExprKind::Binary {
                op: BinOp::Add,
                lhs: Box::new(type_call),
                rhs: Box::new(twice_call),
            },
            Some(STy::I32),
        );
        statements.push(print_value(&mut b, combined, "i32"));

        // print sign(-5)
        let five = b.int(5, Some(STy::I32));
        let negative_arg = b.expr(
            ExprKind::Unary {
                op: UnOp::Neg,
                expr: Box::new(five),
            },
            Some(STy::I32),
        );
        let sign_call = user_call(&mut b, "sign", vec![negative_arg], sign_id);
        statements.push(print_value(&mut b, sign_call, "i32"));

        // let numbers: [i32; 3] = [1, 2, 3]; print numbers[1]
        let array_element = b.named_type("i32", Some(STy::I32));
        let array_len = b.int(3, Some(STy::Usize));
        let array_ty = b.ty(
            TypeKind::Array {
                elem: Box::new(array_element),
                len: Box::new(array_len),
            },
            Some(STy::Array {
                elem: Box::new(STy::I32),
                len: 3,
            }),
        );
        let array_items = vec![
            b.int(1, Some(STy::I32)),
            b.int(2, Some(STy::I32)),
            b.int(3, Some(STy::I32)),
        ];
        let array_literal = b.expr(
            ExprKind::Array(array_items),
            Some(STy::Array {
                elem: Box::new(STy::I32),
                len: 3,
            }),
        );
        let numbers_let = Stmt {
            id: b.id(),
            kind: StmtKind::Let(LetStmt {
                pat: binding_pat(&mut b, "numbers", false),
                ty: Some(array_ty),
                init: Some(array_literal),
            }),
            span: span(),
        };
        statements.push(numbers_let);
        let indexed = {
            let base = b.var(
                "numbers",
                Some(STy::Array {
                    elem: Box::new(STy::I32),
                    len: 3,
                }),
            );
            let index = b.int(1, Some(STy::Usize));
            b.expr(
                ExprKind::Index {
                    base: Box::new(base),
                    index: Box::new(index),
                },
                Some(STy::I32),
            )
        };
        statements.push(print_value(&mut b, indexed, "i32"));

        // let pair: (i32, i32) = (4, 5); print pair.0
        let tuple_a = b.named_type("i32", Some(STy::I32));
        let tuple_b = b.named_type("i32", Some(STy::I32));
        let tuple_ty = b.ty(
            TypeKind::Tuple(vec![tuple_a, tuple_b]),
            Some(STy::Tuple(vec![STy::I32, STy::I32])),
        );
        let tuple_values = vec![b.int(4, Some(STy::I32)), b.int(5, Some(STy::I32))];
        let tuple_literal = b.expr(
            ExprKind::Tuple(tuple_values),
            Some(STy::Tuple(vec![STy::I32, STy::I32])),
        );
        let pair_let = Stmt {
            id: b.id(),
            kind: StmtKind::Let(LetStmt {
                pat: binding_pat(&mut b, "pair", false),
                ty: Some(tuple_ty),
                init: Some(tuple_literal),
            }),
            span: span(),
        };
        statements.push(pair_let);
        let tuple_item = {
            let base = b.var("pair", Some(STy::Tuple(vec![STy::I32, STy::I32])));
            b.expr(
                ExprKind::TupleIndex {
                    base: Box::new(base),
                    index: 0,
                },
                Some(STy::I32),
            )
        };
        statements.push(print_value(&mut b, tuple_item, "i32"));

        // let maybe: Option<i32> = Some(3); print maybe.unwrap_or(0)
        let option_element = b.named_type("i32", Some(STy::I32));
        let option_path = generic_path(&mut b, "Option", vec![option_element]);
        let option_ty = b.ty(
            TypeKind::Path(option_path),
            Some(STy::Option(Box::new(STy::I32))),
        );
        let some_call = {
            let callee = b.callee("Some");
            let payload = b.int(3, Some(STy::I32));
            b.call(
                callee,
                vec![payload],
                CallTarget::VariantCtor {
                    enum_item: 930_000,
                    variant: 0,
                },
            )
        };
        let maybe_let = Stmt {
            id: b.id(),
            kind: StmtKind::Let(LetStmt {
                pat: binding_pat(&mut b, "maybe", false),
                ty: Some(option_ty),
                init: Some(some_call),
            }),
            span: span(),
        };
        statements.push(maybe_let);
        let unwrapped = {
            let receiver = b.var("maybe", Some(STy::Option(Box::new(STy::I32))));
            let fallback = b.int(0, Some(STy::I32));
            method_builtin(
                &mut b,
                receiver,
                "unwrap_or",
                vec![fallback],
                "Option::unwrap_or",
                STy::I32,
            )
        };
        statements.push(print_value(&mut b, unwrapped, "i32"));

        // let nothing: Option<i32> = None; print nothing.is_none()
        let nothing_value = {
            let path = b.path(&["None"]);
            b.facts.resolutions.insert(
                path.id,
                Resolution::Variant {
                    enum_item: 930_001,
                    variant: 1,
                },
            );
            b.expr(ExprKind::Path(path), Some(STy::Option(Box::new(STy::I32))))
        };
        let nothing_element = b.named_type("i32", Some(STy::I32));
        let nothing_path = generic_path(&mut b, "Option", vec![nothing_element]);
        let nothing_ty = b.ty(
            TypeKind::Path(nothing_path),
            Some(STy::Option(Box::new(STy::I32))),
        );
        let nothing_let = Stmt {
            id: b.id(),
            kind: StmtKind::Let(LetStmt {
                pat: binding_pat(&mut b, "nothing", false),
                ty: Some(nothing_ty),
                init: Some(nothing_value),
            }),
            span: span(),
        };
        statements.push(nothing_let);
        let is_none = {
            let receiver = b.var("nothing", Some(STy::Option(Box::new(STy::I32))));
            method_builtin(
                &mut b,
                receiver,
                "is_none",
                Vec::new(),
                "Option::is_none",
                STy::Bool,
            )
        };
        statements.push(print_value(&mut b, is_none, "bool"));

        // let flag: bool = !false; print flag
        let false_value = b.expr(ExprKind::Bool(false), Some(STy::Bool));
        let flag_value = b.expr(
            ExprKind::Unary {
                op: UnOp::Not,
                expr: Box::new(false_value),
            },
            Some(STy::Bool),
        );
        let flag_ty = b.named_type("bool", Some(STy::Bool));
        let flag_let = Stmt {
            id: b.id(),
            kind: StmtKind::Let(LetStmt {
                pat: binding_pat(&mut b, "flag", false),
                ty: Some(flag_ty),
                init: Some(flag_value),
            }),
            span: span(),
        };
        statements.push(flag_let);
        let flag_var = b.var("flag", Some(STy::Bool));
        statements.push(print_value(&mut b, flag_var, "bool"));

        // let parsed: i64 = parse_number("42")?; print parsed
        let parse_arg = b.text("42");
        let parse_call = user_call(&mut b, "parse_number", vec![parse_arg], parse_number_id);
        let parsed_try = b.expr(ExprKind::Try(Box::new(parse_call)), Some(STy::I64));
        let parsed_ty_node = b.named_type("i64", Some(STy::I64));
        let parsed_let = Stmt {
            id: b.id(),
            kind: StmtKind::Let(LetStmt {
                pat: binding_pat(&mut b, "parsed", false),
                ty: Some(parsed_ty_node),
                init: Some(parsed_try),
            }),
            span: span(),
        };
        statements.push(parsed_let);
        let parsed_var = b.var("parsed", Some(STy::I64));
        statements.push(print_value(&mut b, parsed_var, "i64"));

        // println("say \"hi\"")
        let escaped = b.text("say \"hi\"");
        let say_call = free_call(&mut b, "println", vec![escaped], "std::io::println");
        statements.push(b.expr_stmt(say_call));

        // let owned: String = "x".to_string(); drop(owned);
        let owned_source = b.text("x");
        let owned_text = {
            let receiver = owned_source;
            method_builtin(
                &mut b,
                receiver,
                "to_string",
                Vec::new(),
                "str::to_string",
                STy::String,
            )
        };
        let owned_ty = b.named_type("String", Some(STy::String));
        let owned_let = Stmt {
            id: b.id(),
            kind: StmtKind::Let(LetStmt {
                pat: binding_pat(&mut b, "owned", false),
                ty: Some(owned_ty),
                init: Some(owned_text),
            }),
            span: span(),
        };
        statements.push(owned_let);
        let drop_call = {
            let owned = b.var("owned", Some(STy::String));
            free_call(&mut b, "drop", vec![owned], "drop")
        };
        statements.push(b.expr_stmt(drop_call));

        // assert(max(1, 2) == 2);
        let assertion = {
            let assert_args = vec![b.int(1, Some(STy::I32)), b.int(2, Some(STy::I32))];
            let max_call = user_call(&mut b, "max", assert_args, max_id);
            let two = b.int(2, Some(STy::I32));
            let condition = b.expr(
                ExprKind::Binary {
                    op: BinOp::Eq,
                    lhs: Box::new(max_call),
                    rhs: Box::new(two),
                },
                Some(STy::Bool),
            );
            free_call(&mut b, "assert", vec![condition], "std::panic::assert")
        };
        statements.push(b.expr_stmt(assertion));

        // print LIMIT through a user function call: max(3, LIMIT)
        let limit_path = b.path(&["LIMIT"]);
        b.facts
            .resolutions
            .insert(limit_path.id, Resolution::Item { item: limit_id });
        let limit_expr = b.expr(ExprKind::Path(limit_path), Some(STy::I32));
        let limit_args = vec![b.int(3, Some(STy::I32)), limit_expr];
        let limit_max = user_call(&mut b, "max", limit_args, max_id);
        statements.push(print_value(&mut b, limit_max, "i32"));

        let ok_callee = b.callee("Ok");
        let unit_value = b.unit();
        let ok_call = b.call(
            ok_callee,
            vec![unit_value],
            CallTarget::VariantCtor {
                enum_item: 940_000,
                variant: 0,
            },
        );
        let main_body = b.block(statements, Some(ok_call));
        let main_path = b.path(&["Result"]);
        let main_ret = b.ty(
            TypeKind::Path(main_path),
            Some(STy::Result(
                Box::new(STy::Unit),
                Box::new(STy::Nominal {
                    def: 940_001,
                    name: "Error".to_string(),
                    args: Vec::new(),
                }),
            )),
        );
        let main_item = make_fn(
            &mut b,
            main_id,
            "main",
            Vis::Private,
            false,
            Vec::new(),
            main_ret,
            main_body,
        );
        b.items.push(main_item);

        ProgramE {
            checked: b.finish(Some(main_id), Vec::new()),
            shape_id,
            area_id,
            max_id,
            sign_id,
            sum_to_id,
            total_id,
            cast_round_id,
            greeting_id,
            countdown_id,
            type_id,
            parse_number_id,
            twice_id,
        }
    }

    #[test]
    fn broad_program_compiles_runs_and_keeps_semantics() {
        if !rustc_available() {
            eprintln!("SKIP broad_program_compiles_runs_and_keeps_semantics: rustc not available");
            return;
        }
        let program = build_program_e();
        let code = generate_ok(&program.checked, &GenOptions::default());
        // Structural expectations for the mapped constructs.
        assert!(code.contains("pub enum Shape {"));
        assert!(code.contains("crate::rvm_root::Shape::Circle(r)"));
        assert!(code.contains("crate::rvm_root::Shape::Rect { w: w, h: h }"));
        assert!(code.contains("crate::rvm_root::Shape::Empty =>"));
        assert!(code.contains("for i in 0i32..n"));
        assert!(code.contains("while (i > 0i32)"));
        assert!(code.contains("x as f64"));
        assert!(code.contains("name.trim().to_string()"));
        assert!(code.contains(".to_uppercase()"));
        assert!(code.contains(".parse::<i64>()"));
        assert!(code.contains("pub fn r#type() -> i32"));
        assert!(code.contains("crate::rvm_root::r#type()"));
        assert!(code.contains("pub mod rvm_util {"));
        assert!(code.contains("crate::rvm_root::rvm_util::twice(3i32)"));
        assert!(code.contains("crate::rvm_root::LIMIT"));
        assert!(code.contains("let mut values: Vec<i32> = Vec::new();"));
        assert!(code.contains("values.push(1i32);"));
        assert!(code.contains("crate::rv_assert("));
        assert!(code.contains(".unwrap_or(0i32)"));
        assert!(code.contains(".is_none()"));
        assert!(code.contains("drop(owned);"));
        assert!(code.contains("\"say \\\"hi\\\"\""));
        assert!(!code.contains("unsafe {"));
        let _ = (
            program.shape_id,
            program.area_id,
            program.max_id,
            program.sign_id,
            program.sum_to_id,
            program.total_id,
            program.cast_round_id,
            program.greeting_id,
            program.countdown_id,
            program.type_id,
            program.parse_number_id,
            program.twice_id,
        );

        let binary = compile(&code, "program-e");
        let (stdout, stderr, status) = run(&binary);
        let expected =
            "12\n6\n0\n7\n6\n6\n3\nHI\n0\n10\n-1\n2\n4\n3\ntrue\ntrue\n42\nsay \"hi\"\n10\n";
        assert_eq!(stdout, expected, "stderr: {stderr}\n--- code ---\n{code}");
        assert_eq!(status, 0, "stderr: {stderr}");
    }

    // -- fact-path mapping and boolean operators ---------------------------

    #[test]
    fn fact_item_paths_are_rewritten_into_emitted_modules() {
        let program = build_program_e();
        let options = GenOptions::default();
        let index = Index::build(&program.checked);
        let emitter = Emitter {
            checked: &program.checked,
            options: &options,
            index,
        };
        assert_eq!(
            emitter.rewrite_fact_path("crate::area"),
            "crate::rvm_root::area"
        );
        assert_eq!(
            emitter.rewrite_fact_path("crate::Shape::Circle"),
            "crate::rvm_root::Shape::Circle"
        );
        assert_eq!(
            emitter.rewrite_fact_path("crate::util::twice"),
            "crate::rvm_root::rvm_util::twice"
        );
        assert_eq!(
            emitter.rewrite_fact_path("crate::LIMIT"),
            "crate::rvm_root::LIMIT"
        );
        // Reserved Rust words stay mangled after rewriting.
        assert_eq!(
            emitter.rewrite_fact_path("crate::type"),
            "crate::rvm_root::r#type"
        );
    }

    #[test]
    fn short_circuit_operators_are_preserved() {
        let mut b = Builder::new();
        let logic_id = b.id();
        let a_ty = b.named_type("bool", Some(STy::Bool));
        let b_ty = b.named_type("bool", Some(STy::Bool));
        let c_ty = b.named_type("bool", Some(STy::Bool));
        let param_a = make_param(&mut b, "a", a_ty);
        let param_b = make_param(&mut b, "b", b_ty);
        let param_c = make_param(&mut b, "c", c_ty);
        let a_value = b.var("a", Some(STy::Bool));
        let b_value = b.var("b", Some(STy::Bool));
        let and_expr = b.expr(
            ExprKind::Binary {
                op: BinOp::And,
                lhs: Box::new(a_value),
                rhs: Box::new(b_value),
            },
            Some(STy::Bool),
        );
        let c_value = b.var("c", Some(STy::Bool));
        let or_expr = b.expr(
            ExprKind::Binary {
                op: BinOp::Or,
                lhs: Box::new(and_expr),
                rhs: Box::new(c_value),
            },
            Some(STy::Bool),
        );
        let body = b.block(Vec::new(), Some(or_expr));
        let ret = b.named_type("bool", Some(STy::Bool));
        let item = make_fn(
            &mut b,
            logic_id,
            "logic",
            Vis::Public(span()),
            false,
            vec![param_a, param_b, param_c],
            ret,
            body,
        );
        b.items.push(item);
        let program = b.finish(None, Vec::new());
        let code = generate_ok(&program, &GenOptions::default());
        assert!(code.contains("(a && b) || c"), "generated:\n{code}");
    }

    // -- test runner with `Result`-returning tests --------------------------

    #[test]
    fn test_runner_supports_result_returning_tests() {
        if !rustc_available() {
            eprintln!("SKIP test_runner_supports_result_returning_tests: rustc not available");
            return;
        }
        let mut b = Builder::new();
        let result_ret = |b: &mut Builder| {
            let path = b.path(&["Result"]);
            b.ty(
                TypeKind::Path(path),
                Some(STy::Result(
                    Box::new(STy::Unit),
                    Box::new(STy::Nominal {
                        def: 950_001,
                        name: "Error".to_string(),
                        args: Vec::new(),
                    }),
                )),
            )
        };

        // test fn happy() -> Result<(), Error> { Ok(()) }
        let happy_id = b.id();
        let ok_callee = b.callee("Ok");
        let unit_value = b.unit();
        let ok_call = b.call(
            ok_callee,
            vec![unit_value],
            CallTarget::VariantCtor {
                enum_item: 950_002,
                variant: 0,
            },
        );
        let happy_body = b.block(Vec::new(), Some(ok_call));
        let happy_ret = result_ret(&mut b);
        let happy_item = make_fn(
            &mut b,
            happy_id,
            "happy",
            Vis::Private,
            true,
            Vec::new(),
            happy_ret,
            happy_body,
        );
        b.items.push(happy_item);

        // test fn sad() -> Result<(), Error> { Err(Error::new("nope")) }
        let sad_id = b.id();
        let new_callee = b.callee("Error::new");
        let nope = b.text("nope");
        let error_call = b.call(
            new_callee,
            vec![nope],
            CallTarget::BuiltinFn {
                path: "Error::new".to_string(),
            },
        );
        let err_callee = b.callee("Err");
        let err_call = b.call(
            err_callee,
            vec![error_call],
            CallTarget::VariantCtor {
                enum_item: 950_003,
                variant: 1,
            },
        );
        let sad_body = b.block(Vec::new(), Some(err_call));
        let sad_ret = result_ret(&mut b);
        let sad_item = make_fn(
            &mut b,
            sad_id,
            "sad",
            Vis::Private,
            true,
            Vec::new(),
            sad_ret,
            sad_body,
        );
        b.items.push(sad_item);

        let program = b.finish(None, vec![happy_id, sad_id]);
        let mut options = GenOptions::default();
        options.test_mode = true;
        let code = generate_ok(&program, &options);
        assert!(code.contains("crate::rv_test_result(\"happy\""));
        assert!(code.contains("crate::rv_test_result(\"sad\""));
        let binary = compile(&code, "program-result-tests");
        let (stdout, stderr, status) = run(&binary);
        assert!(stdout.contains("test happy ... ok"), "stdout: {stdout}");
        assert!(stdout.contains("test sad ... FAILED"), "stdout: {stdout}");
        assert!(stdout.contains("error: nope"), "stdout: {stdout}");
        assert!(stdout.contains("1 passed; 1 failed"), "stdout: {stdout}");
        assert_eq!(status, 1, "stderr: {stderr}");
    }

    // -- entry-point edge cases ---------------------------------------------

    #[test]
    fn main_detection_falls_back_to_the_declared_name() {
        let mut program = build_program_a();
        // A checked program normally sets `main_fn`; the generator must still
        // find `fn main` in the root module when the fact is absent.
        program.checked.facts.main_fn = None;
        let code = generate_ok(&program.checked, &GenOptions::default());
        assert!(code.contains("match crate::rvm_root::rv_user_main() {"));
    }

    #[test]
    fn missing_main_generates_an_explicit_stub() {
        let program = build_program_d();
        let code = generate_ok(&program, &GenOptions::default());
        assert!(code.contains("this Ravum program has no `main` function"));
        assert!(code.contains("std::process::ExitCode::from(1)"));
    }

    #[test]
    fn invalid_main_return_type_is_an_explicit_error() {
        let mut b = Builder::new();
        let main_id = b.id();
        let value = b.int(1, Some(STy::I32));
        let body = b.block(Vec::new(), Some(value));
        let ret = b.named_type("i32", Some(STy::I32));
        let item = make_fn(
            &mut b,
            main_id,
            "main",
            Vis::Private,
            false,
            Vec::new(),
            ret,
            body,
        );
        b.items.push(item);
        let program = b.finish(Some(main_id), Vec::new());
        let error = crate::generate(&program, &GenOptions::default())
            .expect_err("`main -> i32` must be rejected by code generation");
        assert!(
            error.message.contains("`main` must return"),
            "unexpected error: {}",
            error.message
        );
    }

    #[test]
    fn banner_uses_the_requested_crate_name() {
        let program = build_program_a();
        let mut options = GenOptions::default();
        options.crate_name = "hello_world".to_string();
        let code = generate_ok(&program.checked, &options);
        assert!(code.starts_with("// Generated by ravum-lang "));
        assert!(code.contains("from hello_world; do not edit."));
    }

    /// End-to-end `--test`: `ravum-lang test` semantics on `examples/tests-demo`
    /// (unit tests, `assert`/`assert_eq`, owned strings, and a test function
    /// returning `Result<(), Error>`). The user `main` must not run.
    #[test]
    fn e2e_tests_demo_runner() {
        if !rustc_available() {
            eprintln!("SKIP e2e_tests_demo_runner: rustc not available");
            return;
        }
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/tests-demo");
        assert!(
            directory.join("ravum.toml").exists(),
            "example `tests-demo` is missing at {}",
            directory.display()
        );
        let loaded = ravum_lang_sema::load_package(&directory);
        assert!(
            loaded.diags.is_empty(),
            "loader diagnostics for `tests-demo`:\n{}",
            loaded.diags.render(&loaded.sources)
        );
        let program = loaded
            .program
            .expect("the loader produced no program for a valid package");
        let check_options = ravum_lang_sema::facts::CheckOptions::default();
        let checked = ravum_lang_sema::check(&program, &loaded.sources, &check_options);
        assert!(
            checked.diags.is_empty(),
            "checker diagnostics for `tests-demo`:\n{}",
            checked.diags.render(&loaded.sources)
        );
        let checked = checked
            .checked
            .expect("the checker produced no CheckedProgram");
        let mut options = GenOptions::default();
        options.test_mode = true;
        options.crate_name = "tests_demo".to_string();
        let code = generate_ok(&checked, &options);
        let binary = compile(&code, "tests-demo");
        let (stdout, stderr, status) = run(&binary);
        for expected in [
            "test addition_works ... ok",
            "test strings_work ... ok",
            "test result_propagation_works ... ok",
            "3 passed; 0 failed",
        ] {
            assert!(
                stdout.contains(expected),
                "missing `{expected}` in test output:\n{stdout}\n--- generated ---\n{code}"
            );
        }
        assert!(
            !stdout.contains("run `ravum-lang test`"),
            "the user `main` must not run in test mode: {stdout}"
        );
        assert_eq!(status, 0, "stderr: {stderr}");
    }
}
