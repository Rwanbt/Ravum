//! Recursive-descent parser for Ravum Core 1.0.
//!
//! OWNER: worker `syntax`.
//!
//! Contract:
//!
//! * `parse_file` never panics on arbitrary input and never loops forever;
//!   every loop either consumes a token or leaves because of a stop condition.
//! * node ids (see [`crate::ast`]) are unique across a file: the high 32 bits
//!   carry the [`FileId`], the low 32 bits a per-file counter. Every `Item`,
//!   `Module`, `Stmt`, `Block`, `Expr`, `Pat`, `Param`, `GenericParam`,
//!   `FieldDef`, `VariantDef`, `MatchArm`, `Type` and `Path` gets exactly one id.
//! * every syntax error is a [`crate::diag::Diagnostic`] with a stable
//!   `E-PARSE-*`, `E-UNSUPPORTED-*` or `E-LEX-*` code and an exact span;
//! * recovery keeps parsing after an error so several diagnostics are reported.
//!
//! # Guards
//!
//! * [`MAX_DEPTH`] bounds expression/type/pattern/item nesting; beyond it a
//!   single `E-PARSE-DEPTH` is emitted and parsing unwinds without parsing the
//!   rest of the file (the partial program is returned).
//! * [`MAX_TOKENS`] bounds the token stream; beyond it `E-PARSE-TOO-LARGE` is
//!   emitted and an empty root module is returned.
//! * [`MAX_NODES`] bounds allocated node ids so the per-file counter can never
//!   overflow into the file bits; a single `E-PARSE-TOO-LARGE` is emitted.
//!
//! # Known limitations (Core 1.0 draft)
//!
//! * `while`/`loop`/`for` are parsed (their AST nodes were added by the Lead);
//!   `break`/`continue` never take a value and labels are unsupported.
//! * Expression generic arguments require the turbofish form
//!   (`Vec::<i32>::new()`): a bare `<` in expression position is always a
//!   comparison. Type positions accept the plain form (`Vec<i32>`).
//! * `macro_rules!`/`foo!()` are rejected with `E-UNSUPPORTED-MACRO`.
//! * chained tuple indices such as `x.0.1` are rejected with
//!   `E-PARSE-UNEXPECTED`: the lexer tokenizes `0.1` as one float literal, so
//!   the frozen token stream cannot express that chain. `x.0` is supported.
//! * `pub` on an enum variant is accepted and ignored: the frozen AST has no
//!   visibility slot on `VariantDef`, and Core variants inherit the enum's
//!   visibility. `pub` on a parameter binding is *not* part of the grammar.
//! * `mut` on a `let` binding / function parameter is accepted. Parameters have
//!   no mutable flag in the frozen AST, so it is ignored there.
//! * Structs with tuple fields (`struct S(T);`) are rejected with
//!   `E-UNSUPPORTED-TUPLE-STRUCT` (only named-field and unit structs exist in
//!   the frozen AST).

use crate::ast::*;
use crate::diag::{Diagnostic, DiagnosticBag};
use crate::lexer;
use crate::span::{FileId, Span};
use crate::token::{Kw, Punct, Token, TokenKind};
use std::collections::BTreeSet;

/// Deepest expression/type/pattern/item nesting accepted before
/// `E-PARSE-DEPTH`.
pub const MAX_DEPTH: u32 = 256;
/// Largest token stream accepted before `E-PARSE-TOO-LARGE`.
pub const MAX_TOKENS: usize = 300_000;
/// Largest number of nodes a single file may allocate (per-file counter).
pub const MAX_NODES: u32 = 2_000_000;

pub struct ParseResult {
    pub program: Program,
    pub diags: DiagnosticBag,
}

/// Allocates node ids. The high 32 bits carry the FileId, the low 32 bits a
/// per-file counter, so ids are unique across files without a global allocator.
/// The policy limit per file is [MAX_NODES], far below 2^32.
struct NodeIds {
    next: NodeId,
}

impl NodeIds {
    fn new(file: FileId) -> Self {
        Self {
            next: (u64::from(file) << 32) | 1,
        }
    }

    fn next(&mut self) -> NodeId {
        let id = self.next;
        self.next = self.next.saturating_add(1);
        id
    }
}

/// Parse one `.rv` file. Module declarations (`mod foo;`) are kept unresolved
/// for the loader to expand. The parser always returns a program (possibly
/// empty or partial) plus all diagnostics; it never panics.
pub fn parse_file(file: FileId, src: &str) -> ParseResult {
    let (tokens, diags) = lexer::lex(file, src, false);
    // `#` is not a token in the frozen token set. The lexer reports it as
    // E-LEX-CHAR; for `#[...]` / `#![...]` attributes the parser reports the
    // canonical E-UNSUPPORTED-ATTR instead, so drop the redundant lexer error.
    let mut diags = strip_attribute_hash_errors(diags, src);
    let tokens = normalize_split_angles(tokens);

    let mut ids = NodeIds::new(file);
    let root_id = ids.next();

    let end = src.len().min(u32::MAX as usize) as u32;
    let root_span = Span::new(file, 0, end);

    if tokens.len() > MAX_TOKENS {
        diags.push(Diagnostic::error(
            "E-PARSE-TOO-LARGE",
            root_span,
            format!(
                "source contains {} tokens, above the parser limit of {MAX_TOKENS}",
                tokens.len()
            ),
        ));
        return ParseResult {
            program: Program {
                file,
                root: Module {
                    id: root_id,
                    name: None,
                    items: Vec::new(),
                    span: root_span,
                },
            },
            diags,
        };
    }

    let mut parser = Parser::new(file, src, tokens, diags, ids);
    let items = parser.parse_module_items(false);
    let mut diags = parser.finish();

    let program = Program {
        file,
        root: Module {
            id: root_id,
            name: None,
            items,
            span: root_span,
        },
    };
    diags.sort();
    ParseResult { program, diags }
}

/// The lexer emits `>>`, `>=` and `>>=` as single tokens. In type/generic
/// position those close generic lists, so split them once up front; expression
/// parsing recombines adjacent `>`/`=` tokens into the original operator. The
/// split preserves byte spans (`>` and `=` are ASCII, so the halves are valid
/// UTF-8 boundaries).
mod control;
mod core;
mod exprs;
mod items;
mod patterns;
mod primary;
mod stmts;
mod tokens;
mod types;

use tokens::*;

struct Parser<'a> {
    file: FileId,
    src: &'a str,
    tokens: Vec<Token>,
    pos: usize,
    diags: DiagnosticBag,
    ids: NodeIds,
    node_count: u32,
    depth: u32,
    /// Deepest nesting reached by the expression being parsed, counting one
    /// level per link of a left-associative chain (`a + b + c`, `x.f.g`).
    /// Those chains are parsed by loops, so `depth` alone does not bound the
    /// height of the tree they build.
    reach: u32,
    fatal: bool,
    depth_reported: bool,
    nodes_reported: bool,
    /// Number of enclosing condition contexts where assignment is forbidden.
    forbid_assign: u32,
    /// G-6.1: set by a block just before it parses a statement, taken by the
    /// first operand parsed. A block-like operand there ends the statement at
    /// its `}` (`stmt_end`), unless `.` or `?` follows, as in Rust.
    stmt_head: bool,
    /// The statement being parsed already ended at a block-like `}`: no
    /// operator, call, index, cast, range or assignment continues it.
    stmt_end: bool,
    /// RFC-INVARIANTS-1: the synthetic `impl` of the struct just parsed,
    /// placed right after it by `parse_module_items`.
    pending_items: Vec<Item>,
    /// Expressions that were written between parentheses. Used to distinguish
    /// `(a < b) < c` (allowed) from `a < b < c` (E-PARSE-UNEXPECTED) and
    /// `(t.0).0` (allowed) from `t.0.1` (E-PARSE-UNEXPECTED).
    paren_exprs: BTreeSet<NodeId>,
    /// True only while parsing the iterator of a `for`: a range is legal there
    /// and nowhere else (E-UNSUPPORTED-RANGE, parse phase).
    in_for_iter: bool,
}

fn vis_span(vis: &Vis) -> Span {
    match vis {
        Vis::Public(span) => *span,
        Vis::Private => Span::dummy(),
    }
}

/// Expressions that are valid as statements without a trailing `;`.
fn is_block_like(expr: &Expr) -> bool {
    matches!(
        &expr.kind,
        ExprKind::If { .. }
            | ExprKind::Match { .. }
            | ExprKind::While { .. }
            | ExprKind::Loop { .. }
            | ExprKind::For { .. }
            | ExprKind::Block(_)
    )
}

#[cfg(test)]
mod tests;
