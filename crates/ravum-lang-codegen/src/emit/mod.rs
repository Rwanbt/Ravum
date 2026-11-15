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
    BindingId, CallTarget, CheckedProgram, ConstValue, MethodTarget, Resolution, UseKind,
    VariantKind,
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

mod builtins;
mod index;
mod names;

use builtins::*;
use index::*;
use names::*;
mod emitter_calls;
mod emitter_expr;
mod emitter_items;
mod emitter_patterns;
mod emitter_types;
mod emitter_value;

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
    /// Step B of RFC-OWN-FACTS-1 (debug builds only): check that the ownership
    /// facts the backend reads are coherent.
    ///
    /// Since RFC-OWN-FACTS-2 step C5 the emitter has no copy/move classifier of
    /// its own: `ravum_lang_sema::is_copy_type` is the single implementation,
    /// and it is called here only to check that the published tables still
    /// match the types they were computed from (a stale or hand-assembled
    /// table). In debug builds an incoherence is an error instead of a silently
    /// different lowering. Release builds skip the check (it is a test/audit
    /// guard, not a semantic rule).
    fn verify_ownership_facts(&self) -> Result<(), CodegenError> {
        if !cfg!(debug_assertions) {
            return Ok(());
        }
        for (id, kind) in &self.checked.facts.own.use_kind {
            let Some(ty) = self.checked.facts.expr_types.get(id) else {
                continue;
            };
            let copies = ravum_lang_sema::is_copy_type(ty, &self.checked.facts);
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
        // RFC-OWN-FACTS-2 step C1: copy-ness is published per type node; the
        // table must be the one the single classifier yields for these types.
        for (id, ty) in &self.checked.facts.type_nodes {
            let derived = ravum_lang_sema::is_copy_type(ty, &self.checked.facts);
            let recorded = self.checked.facts.own.copy_type.contains(id);
            if derived != recorded {
                return Err(err(format!(
                    "ownership facts disagree with the backend for type {id}: the table says \
                     Copy={recorded}, the classifier says Copy={derived} for `{ty:?}`"
                )));
            }
        }
        self.verify_ownership_tables()?;
        let mut bodies: Vec<&Block> = Vec::new();
        collect_fn_bodies(&self.checked.program.root, &mut bodies);
        for body in bodies {
            if let Some(message) = self.path_fact_disagreements(body).into_iter().next() {
                return Err(err(message));
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

    /// RFC-OWN-FACTS-2 steps C2–C4: the tables the checker publishes must agree
    /// with each other and with the resolution facts.
    ///
    /// * `consumed` is exactly the set of expressions whose use is a `Move`;
    /// * `consumed` is a subset of `owned_position`, and an owned position is a
    ///   `Move` or a `Copy`, never a borrow;
    /// * a binding identity names the binding the path resolves to.
    fn verify_ownership_tables(&self) -> Result<(), CodegenError> {
        let own = &self.checked.facts.own;
        for (id, kind) in &own.use_kind {
            let moved = matches!(kind, UseKind::Move);
            if moved != own.consumed.contains(id) {
                return Err(err(format!(
                    "ownership facts disagree with each other for expression {id}: use kind \
                     {kind:?}, consumed={}",
                    own.consumed.contains(id)
                )));
            }
        }
        for id in &own.consumed {
            if !own.owned_position.contains(id) || !own.use_kind.contains_key(id) {
                return Err(err(format!(
                    "ownership facts disagree with each other for expression {id}: consumed \
                     outside an owned position"
                )));
            }
        }
        for id in &own.owned_position {
            if !matches!(
                own.use_kind.get(id),
                Some(UseKind::Move) | Some(UseKind::Copy)
            ) {
                return Err(err(format!(
                    "ownership facts disagree with each other for expression {id}: owned \
                     position with use kind {:?}",
                    own.use_kind.get(id)
                )));
            }
        }
        let mut exprs: BTreeMap<NodeId, &Expr> = BTreeMap::new();
        let mut method_receivers: BTreeSet<NodeId> = BTreeSet::new();
        collect_module_exprs(
            &self.checked.program.root,
            &mut exprs,
            &mut method_receivers,
        );
        let declarations: BTreeMap<_, _> = own
            .bindings
            .iter()
            .map(|(declaration, id)| (*id, declaration))
            .collect();
        for (node, binding) in &own.binding_identity {
            let Some(expr) = exprs.get(node) else {
                continue;
            };
            let resolution = match &expr.kind {
                ExprKind::Path(path) => self.checked.facts.resolutions.get(&path.id),
                _ => self.checked.facts.resolutions.get(&expr.id),
            };
            let resolved = match resolution {
                Some(Resolution::Local { name, decl, .. }) => Some((*decl, name.clone())),
                Some(_) => None,
                None => {
                    return Err(err(format!(
                        "missing resolution fact for expression {node} ({}): the checker \
                         recorded a binding identity for it",
                        self.expr_location(expr)
                    )));
                }
            };
            let recorded = declarations
                .get(binding)
                .map(|declaration| (**declaration).clone());
            if resolved.is_none() || resolved != recorded {
                return Err(err(format!(
                    "ownership facts disagree with the resolution for expression {node} ({}): \
                     recorded binding identity {recorded:?}, resolved binding {resolved:?}",
                    self.expr_location(expr)
                )));
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

use paths::{collect_fn_bodies, collect_module_exprs, ArrayMovePlan, PathMode};

// ---------------------------------------------------------------------------
// Tests.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
