//! RFC-RT-TRAPS-1 — intrinsic traps of the `rt` profile.
//!
//! In profile `rt`, an operation that can stop the thread produces
//! `E-EFF-PROFILE` unless it is one of the explicit safe forms:
//!
//! * array indexing with a literal index inside the array's known bounds;
//! * array indexing bounded by a recognized loop (T6): `for i in 0..K` with a
//!   literal `K` no larger than the array length, or `for i in 0..a.len()`
//!   when `a` is an array or slice that the loop body neither rebinds nor
//!   reassigns;
//! * division/remainder by a **positive literal** (a bare `-1` is refused: for
//!   signed types `MIN / -1` overflows);
//! * integer `+ - *` and unary `-` only through `wrapping_*`, `saturating_*` or
//!   `checked_*`;
//! * shifts by a literal smaller than the type width;
//! * all floating-point arithmetic and `as` casts.
//!
//! `core` and `pure` are **not** affected: the pass only runs for `rt`
//! (`check.rs`). With `--rt-entry` (T6) only the functions reachable from the
//! real-time entry are checked; setup code outside that set may allocate and
//! perform I/O. This is a deliberate asymmetry, documented in the RFC.

use crate::facts::CheckedProgram;
use crate::types::Type;
use ravum_lang_syntax::ast::{
    BinOp, Block, Expr, ExprKind, FieldPat, ItemKind, Module, NodeId, Pat, PatKind, PathRoot,
    StmtKind, UnOp, VariantPatFields,
};
use ravum_lang_syntax::diag::{Diagnostic, DiagnosticBag};
use ravum_lang_syntax::span::Span;
use std::collections::BTreeSet;

/// Check the functions of a program for intrinsic traps (profile `rt` only).
///
/// `reachable` is the set of function ids reachable from the `--rt-entry`
/// point; `None` checks every function (historical whole-package behaviour).
pub fn check_rt_traps(
    checked: &CheckedProgram,
    diags: &mut DiagnosticBag,
    reachable: Option<&BTreeSet<NodeId>>,
) {
    let mut ctx = Ctx {
        checked,
        diags,
        loops: Vec::new(),
    };
    ctx.module(&checked.program.root, reachable);
}

struct Ctx<'a> {
    checked: &'a CheckedProgram,
    diags: &'a mut DiagnosticBag,
    /// Enclosing `for` loops whose recognized form may justify `a[i]`.
    loops: Vec<SafeLoop>,
}

/// One `for` loop whose index variable and bound match the bounded-index rule.
struct SafeLoop {
    var: String,
    bound: LoopBound,
}

enum LoopBound {
    /// `for i in 0..K`, `K` literal: `a[i]` is safe while `K <= len(a)`.
    Literal(u128),
    /// `for i in 0..a.len()`: `a[i]` is safe when the indexed base is `a`.
    SelfLen(String),
}

impl Ctx<'_> {
    fn module(&mut self, module: &Module, reachable: Option<&BTreeSet<NodeId>>) {
        for item in &module.items {
            match &item.kind {
                ItemKind::Fn(function) => {
                    if is_reachable(item.id, reachable) {
                        self.block(&function.body);
                    }
                }
                ItemKind::Impl(implementation) => {
                    for member in &implementation.items {
                        if let ItemKind::Fn(function) = &member.kind {
                            if is_reachable(member.id, reachable) {
                                self.block(&function.body);
                            }
                        }
                    }
                }
                ItemKind::Mod(inner) => self.module(inner, reachable),
                _ => {}
            }
        }
    }

    fn block(&mut self, block: &Block) {
        for stmt in &block.stmts {
            match &stmt.kind {
                StmtKind::Let(let_stmt) => {
                    if let Some(init) = &let_stmt.init {
                        self.expr(init);
                    }
                }
                StmtKind::Expr { expr, .. } => self.expr(expr),
            }
        }
        if let Some(tail) = &block.tail {
            self.expr(tail);
        }
    }

    fn expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Binary { op, lhs, rhs } => {
                let ty = expr_type(self.checked, expr);
                self.binary(expr.span, *op, lhs, rhs, ty);
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Unary { op, expr: inner } => {
                if matches!(op, UnOp::Neg) && is_signed_integer(expr_type(self.checked, inner)) {
                    report(
                        self.diags,
                        expr.span,
                        "integer negation",
                        "use `wrapping_neg`, `saturating_neg` or `checked_neg`",
                    );
                }
                self.expr(inner);
            }
            ExprKind::Index { base, index } => {
                if !self.index_is_safe(base, index) {
                    report(
                        self.diags,
                        expr.span,
                        "indexing",
                        "index an array with a literal inside its known bounds, with \
                         `for i in 0..K` where `K` is a literal no larger than the \
                         array, or with `for i in 0..a.len()` on an array/slice the \
                         body does not rebind; other dynamic indexing (notably `Vec`) \
                         is refused in `rt`",
                    );
                }
                self.expr(base);
                self.expr(index);
            }
            ExprKind::Call { callee, args } => {
                self.expr(callee);
                for arg in args {
                    self.expr(arg);
                }
            }
            ExprKind::MethodCall { receiver, args, .. } => {
                self.expr(receiver);
                for arg in args {
                    self.expr(arg);
                }
            }
            ExprKind::Field { base, .. } | ExprKind::TupleIndex { base, .. } => {
                self.expr(base);
            }
            ExprKind::Cast { expr: inner, .. } | ExprKind::Try(inner) => {
                self.expr(inner);
            }
            ExprKind::Assign { target, value } => {
                self.expr(target);
                self.expr(value);
            }
            ExprKind::AssignOp { op, target, value } => {
                // Compound assignment is the same trap as the corresponding
                // binary operation: `x += 1` is `x + 1` (RFC-RT-TRAPS-1).
                let ty = expr_type(self.checked, target);
                self.binary(expr.span, *op, target, value, ty);
                self.expr(target);
                self.expr(value);
            }
            ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => {
                self.expr(cond);
                self.block(then_block);
                if let Some(else_branch) = else_branch {
                    self.expr(else_branch);
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                self.expr(scrutinee);
                for arm in arms {
                    self.expr(&arm.body);
                }
            }
            ExprKind::While { cond, body } => {
                self.expr(cond);
                self.block(body);
            }
            ExprKind::Loop { body } => self.block(body),
            ExprKind::For { pat, iter, body } => {
                self.expr(iter);
                let safe = self.safe_loop(pat, iter, body);
                let pushed = safe.is_some();
                if let Some(safe) = safe {
                    self.loops.push(safe);
                }
                self.block(body);
                if pushed {
                    self.loops.pop();
                }
            }
            ExprKind::Block(block) | ExprKind::Unsafe(block) => self.block(block),
            ExprKind::Tuple(items) | ExprKind::Array(items) => {
                for item in items {
                    self.expr(item);
                }
            }
            ExprKind::ArrayRepeat { value, len } => {
                self.expr(value);
                self.expr(len);
            }
            ExprKind::StructLit { fields, .. } => {
                for field in fields {
                    self.expr(&field.value);
                }
            }
            ExprKind::Range { start, end, .. } => {
                if let Some(start) = start {
                    self.expr(start);
                }
                if let Some(end) = end {
                    self.expr(end);
                }
            }
            ExprKind::Return(Some(value)) => self.expr(value),
            _ => {}
        }
    }

    /// Shared trap rule for `ExprKind::Binary` and compound assignment.
    ///
    /// `result` is the expression's type for a binary operation, and the
    /// target's type for `x op= y` (which produces `()` but assigns to an
    /// integer).
    fn binary(&mut self, span: Span, op: BinOp, lhs: &Expr, rhs: &Expr, result: Option<&Type>) {
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul => {
                if is_integer(result) {
                    report(
                        self.diags,
                        span,
                        "integer arithmetic",
                        "use `wrapping_add/sub/mul`, `saturating_add/sub/mul` or \
                         `checked_add/sub/mul`",
                    );
                }
            }
            BinOp::Div | BinOp::Rem => {
                if is_integer(result) && !positive_int_literal(rhs) {
                    report(
                        self.diags,
                        span,
                        "integer division/remainder",
                        "divide by a positive literal or use `checked_div`/`checked_rem`",
                    );
                }
            }
            BinOp::Shl | BinOp::Shr => {
                if is_integer(expr_type(self.checked, lhs)) && !safe_shift(lhs, rhs, self.checked) {
                    report(
                        self.diags,
                        span,
                        "integer shift",
                        "shift by a literal smaller than the type width",
                    );
                }
            }
            _ => {}
        }
    }

    /// Literal index inside the array, or a recognized bounded-loop index.
    fn index_is_safe(&self, base: &Expr, index: &Expr) -> bool {
        if let ExprKind::Int { value, .. } = &index.kind {
            if let Some(len) = array_len(self.checked, base) {
                if *value < len {
                    return true;
                }
            }
        }
        let Some(name) = simple_path_name(index) else {
            return false;
        };
        // The *innermost* loop that binds this name decides: an inner loop
        // reusing `i` shadows the outer binding, so the outer rule must not
        // apply to it.
        let Some(safe) = self.loops.iter().rev().find(|safe| safe.var == name) else {
            return false;
        };
        match &safe.bound {
            LoopBound::Literal(bound) => {
                array_len(self.checked, base).is_some_and(|len| *bound <= len)
            }
            LoopBound::SelfLen(base_name) => {
                simple_path_name(base) == Some(base_name.as_str())
                    && is_array_or_slice(self.checked, base)
            }
        }
    }

    /// Recognize the bounded-index `for` forms of T6, or `None`.
    fn safe_loop(&self, pat: &Pat, iter: &Expr, body: &Block) -> Option<SafeLoop> {
        let PatKind::Binding { name, .. } = &pat.kind else {
            return None;
        };
        let ExprKind::Range {
            start,
            end,
            inclusive,
        } = &iter.kind
        else {
            return None;
        };
        if *inclusive {
            return None;
        }
        let start = start.as_deref()?;
        if !matches!(start.kind, ExprKind::Int { value: 0, .. }) {
            return None;
        }
        let end = end.as_deref()?;
        let bound = match &end.kind {
            ExprKind::Int { value, .. } => LoopBound::Literal(*value),
            ExprKind::MethodCall {
                receiver,
                method,
                args,
            } if method.name == "len" && args.is_empty() => {
                LoopBound::SelfLen(simple_path_name(receiver)?.to_string())
            }
            _ => return None,
        };
        // The form holds only while `i` is the loop binding: a `let` that
        // shadows it, a nested `for` that rebinds it, or an assignment to it
        // makes a later `a[i]` opaque (conservative, one scan of the body).
        if body_rebinds(body, &name.name) {
            return None;
        }
        if let LoopBound::SelfLen(base) = &bound {
            // `a.len()` is a bound only while `a` is not reassigned in the
            // body: another slice can have another length.
            if body_reassigns(body, base) {
                return None;
            }
        }
        Some(SafeLoop {
            var: name.name.clone(),
            bound,
        })
    }
}

fn report(diags: &mut DiagnosticBag, span: Span, operation: &str, advice: &str) {
    diags.push(Diagnostic::error(
        "E-EFF-PROFILE",
        span,
        format!(
            "`{operation}` can panic (trap) in profile `rt`; {advice} \
             (RFC-RT-TRAPS-1, PENDING_HUMAN_APPROVAL)"
        ),
    ));
}

fn is_reachable(id: NodeId, reachable: Option<&BTreeSet<NodeId>>) -> bool {
    reachable.is_none_or(|set| set.contains(&id))
}

/// Single-segment plain path (`i`, `a`), the only shape the rules accept.
fn simple_path_name(expr: &Expr) -> Option<&str> {
    match &expr.kind {
        ExprKind::Path(path) if path.root == PathRoot::Plain && path.segments.len() == 1 => {
            Some(path.segments[0].name.name.as_str())
        }
        _ => None,
    }
}

fn expr_type<'a>(checked: &'a CheckedProgram, expr: &Expr) -> Option<&'a Type> {
    checked.facts.expr_types.get(&expr.id)
}

/// Known length of an array, looking through references (`&[T; N]`).
fn array_len(checked: &CheckedProgram, expr: &Expr) -> Option<u128> {
    fn len_of(ty: &Type) -> Option<u128> {
        match ty {
            Type::Array { len, .. } => Some(*len as u128),
            Type::Ref { inner, .. } => len_of(inner),
            _ => None,
        }
    }
    len_of(expr_type(checked, expr)?)
}

/// Array or slice, looking through references: the types `0..a.len()` bounds.
fn is_array_or_slice(checked: &CheckedProgram, expr: &Expr) -> bool {
    fn array_or_slice(ty: &Type) -> bool {
        match ty {
            Type::Array { .. } | Type::Slice(_) => true,
            Type::Ref { inner, .. } => array_or_slice(inner),
            _ => false,
        }
    }
    expr_type(checked, expr).is_some_and(array_or_slice)
}

fn is_integer(ty: Option<&Type>) -> bool {
    matches!(
        ty,
        Some(
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
        )
    )
}

fn is_signed_integer(ty: Option<&Type>) -> bool {
    matches!(
        ty,
        Some(Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128 | Type::Isize)
    )
}

fn integer_bits(ty: &Type) -> Option<u32> {
    Some(match ty {
        Type::I8 | Type::U8 => 8,
        Type::I16 | Type::U16 => 16,
        Type::I32 | Type::U32 => 32,
        Type::I64 | Type::U64 => 64,
        Type::I128 | Type::U128 => 128,
        Type::Isize | Type::Usize => 64,
        _ => return None,
    })
}

fn positive_int_literal(expr: &Expr) -> bool {
    matches!(expr.kind, ExprKind::Int { value, .. } if value > 0)
}

fn safe_shift(lhs: &Expr, rhs: &Expr, checked: &CheckedProgram) -> bool {
    let Some(ty) = expr_type(checked, lhs) else {
        return false;
    };
    let Some(bits) = integer_bits(ty) else {
        return false;
    };
    matches!(rhs.kind, ExprKind::Int { value, .. } if value < bits as u128)
}

// ---------------------------------------------------------------------------
// Rebinding scan for the bounded-index rule
// ---------------------------------------------------------------------------

/// True when the body binds `name` (`let`, nested `for`) or assigns it.
fn body_rebinds(block: &Block, name: &str) -> bool {
    let mut found = false;
    scan_block(block, name, None, &mut found);
    found
}

/// True when the body assigns the simple path `name`.
fn body_reassigns(block: &Block, name: &str) -> bool {
    let mut found = false;
    scan_block(block, "", Some(name), &mut found);
    found
}

fn scan_block(block: &Block, var: &str, base: Option<&str>, found: &mut bool) {
    for stmt in &block.stmts {
        match &stmt.kind {
            StmtKind::Let(let_stmt) => {
                if !var.is_empty() && pat_binds(&let_stmt.pat, var) {
                    *found = true;
                }
                if let Some(init) = &let_stmt.init {
                    scan_expr(init, var, base, found);
                }
            }
            StmtKind::Expr { expr, .. } => scan_expr(expr, var, base, found),
        }
    }
    if let Some(tail) = &block.tail {
        scan_expr(tail, var, base, found);
    }
}

fn scan_expr(expr: &Expr, var: &str, base: Option<&str>, found: &mut bool) {
    match &expr.kind {
        ExprKind::Assign { target, value } | ExprKind::AssignOp { target, value, .. } => {
            if !var.is_empty() && simple_path_name(target) == Some(var) {
                *found = true;
            }
            if let Some(base) = base {
                if simple_path_name(target) == Some(base) {
                    *found = true;
                }
            }
            scan_expr(target, var, base, found);
            scan_expr(value, var, base, found);
        }
        ExprKind::For { pat, iter, body } => {
            if !var.is_empty() && pat_binds(pat, var) {
                *found = true;
            }
            scan_expr(iter, var, base, found);
            scan_block(body, var, base, found);
        }
        ExprKind::Call { callee, args } => {
            scan_expr(callee, var, base, found);
            for arg in args {
                scan_expr(arg, var, base, found);
            }
        }
        ExprKind::MethodCall { receiver, args, .. } => {
            scan_expr(receiver, var, base, found);
            for arg in args {
                scan_expr(arg, var, base, found);
            }
        }
        ExprKind::Field { base: inner, .. } | ExprKind::TupleIndex { base: inner, .. } => {
            scan_expr(inner, var, base, found);
        }
        ExprKind::Index { base: inner, index } => {
            scan_expr(inner, var, base, found);
            scan_expr(index, var, base, found);
        }
        ExprKind::Unary { expr: inner, .. }
        | ExprKind::Cast { expr: inner, .. }
        | ExprKind::Try(inner) => scan_expr(inner, var, base, found),
        ExprKind::Binary { lhs, rhs, .. } => {
            scan_expr(lhs, var, base, found);
            scan_expr(rhs, var, base, found);
        }
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => {
            scan_expr(cond, var, base, found);
            scan_block(then_block, var, base, found);
            if let Some(else_branch) = else_branch {
                scan_expr(else_branch, var, base, found);
            }
        }
        ExprKind::Match { scrutinee, arms } => {
            scan_expr(scrutinee, var, base, found);
            for arm in arms {
                scan_expr(&arm.body, var, base, found);
            }
        }
        ExprKind::While { cond, body } => {
            scan_expr(cond, var, base, found);
            scan_block(body, var, base, found);
        }
        ExprKind::Loop { body } => scan_block(body, var, base, found),
        ExprKind::Block(block) | ExprKind::Unsafe(block) => scan_block(block, var, base, found),
        ExprKind::Tuple(items) | ExprKind::Array(items) => {
            for item in items {
                scan_expr(item, var, base, found);
            }
        }
        ExprKind::ArrayRepeat { value, len } => {
            scan_expr(value, var, base, found);
            scan_expr(len, var, base, found);
        }
        ExprKind::StructLit { fields, .. } => {
            for field in fields {
                scan_expr(&field.value, var, base, found);
            }
        }
        ExprKind::Range { start, end, .. } => {
            if let Some(start) = start {
                scan_expr(start, var, base, found);
            }
            if let Some(end) = end {
                scan_expr(end, var, base, found);
            }
        }
        ExprKind::Return(Some(value)) => scan_expr(value, var, base, found),
        ExprKind::Unit
        | ExprKind::Int { .. }
        | ExprKind::Float { .. }
        | ExprKind::Bool(_)
        | ExprKind::Char(_)
        | ExprKind::Str(_)
        | ExprKind::SelfValue
        | ExprKind::Path(_)
        | ExprKind::Break
        | ExprKind::Continue
        | ExprKind::Return(None) => {}
    }
}

fn pat_binds(pat: &Pat, name: &str) -> bool {
    match &pat.kind {
        PatKind::Binding { name: bound, .. } => bound.name == name,
        PatKind::Tuple(items) => items.iter().any(|item| pat_binds(item, name)),
        PatKind::Variant { fields, .. } => match fields {
            VariantPatFields::Tuple(items) => items.iter().any(|item| pat_binds(item, name)),
            VariantPatFields::Struct(fields) => fields
                .iter()
                .any(|FieldPat { pat, .. }| pat_binds(pat, name)),
        },
        PatKind::Wild | PatKind::Lit(_) | PatKind::Path(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::check::{check, check_with_entry};
    use crate::facts::{CheckOptions, Profile};
    use ravum_lang_syntax::ast::{ItemKind, Module, NodeId, Program};
    use ravum_lang_syntax::parser;
    use ravum_lang_syntax::source::SourceMap;

    fn check_rt(source: &str) -> Vec<String> {
        let mut sources = SourceMap::new();
        let file = sources.add("main.rv", source);
        let parsed = parser::parse_file(file, source);
        assert!(
            parsed.diags.is_empty(),
            "parse diagnostics: {}",
            parsed.diags.render(&sources)
        );
        let result = check(
            &parsed.program,
            &sources,
            &CheckOptions {
                profile: Profile::Rt,
                edition: ravum_lang_syntax::LANGUAGE_EDITION.to_string(),
            },
        );
        result.diags.iter().map(|diag| diag.code.clone()).collect()
    }

    /// `check_rt` with an explicit `--rt-entry` function (T6).
    fn check_rt_entry(source: &str, entry: &str) -> Vec<String> {
        let mut sources = SourceMap::new();
        let file = sources.add("main.rv", source);
        let parsed = parser::parse_file(file, source);
        assert!(
            parsed.diags.is_empty(),
            "parse diagnostics: {}",
            parsed.diags.render(&sources)
        );
        let id = find_fn(&parsed.program.root, entry)
            .unwrap_or_else(|| panic!("entry function `{entry}` is not in the fixture"));
        check_rt_id(&parsed.program, &sources, id)
    }

    fn check_rt_id(program: &Program, sources: &SourceMap, id: NodeId) -> Vec<String> {
        let result = check_with_entry(
            program,
            sources,
            &CheckOptions {
                profile: Profile::Rt,
                edition: ravum_lang_syntax::LANGUAGE_EDITION.to_string(),
            },
            Some(id),
        );
        result.diags.iter().map(|diag| diag.code.clone()).collect()
    }

    fn find_fn(module: &Module, name: &str) -> Option<NodeId> {
        for item in &module.items {
            if let ItemKind::Fn(function) = &item.kind {
                if function.name.name == name {
                    return Some(item.id);
                }
            }
            if let ItemKind::Mod(inner) = &item.kind {
                if let Some(id) = find_fn(inner, name) {
                    return Some(id);
                }
            }
        }
        None
    }

    fn is_trap(codes: &[String]) -> bool {
        codes.iter().any(|code| code == "E-EFF-PROFILE")
    }

    #[test]
    fn bare_integer_arithmetic_is_a_trap() {
        assert!(is_trap(&check_rt(
            "fn main() { let x: i32 = 1; let y = x + 1; }"
        )));
        assert!(is_trap(&check_rt(
            "fn main() { let x: i32 = 1; let y = x * 2; }"
        )));
    }

    #[test]
    fn literal_bound_index_is_safe_and_out_of_bounds_is_a_trap() {
        assert!(!is_trap(&check_rt(
            "fn main() { let a = [1, 2, 3]; let v = a[1]; }"
        )));
        assert!(is_trap(&check_rt(
            "fn main() { let a = [1, 2, 3]; let v = a[9]; }"
        )));
    }

    #[test]
    fn dynamic_slice_index_is_a_trap() {
        assert!(is_trap(&check_rt(
            "fn first(xs: &[i32]) -> i32 effects() { return xs[0]; }\n\
             fn main() { let a = [1, 2, 3]; let v = first(&a); }"
        )));
    }

    #[test]
    fn division_by_a_positive_literal_is_safe() {
        assert!(!is_trap(&check_rt(
            "fn main() { let x: i32 = 10; let q = x / 3; }"
        )));
        assert!(is_trap(&check_rt(
            "fn ratio(x: i32, y: i32) -> i32 effects() { return x / y; }\n\
             fn main() { let v = ratio(4, 2); }"
        )));
    }

    #[test]
    fn float_arithmetic_is_safe() {
        assert!(!is_trap(&check_rt(
            "fn main() { let x = 1.5 * 2.0 + 0.5; let y = x / 3.0; }"
        )));
    }

    #[test]
    fn explicit_integer_forms_are_safe() {
        assert!(!is_trap(&check_rt(
            "fn main() {\n\
                 let x: i32 = 7;\n\
                 let a = x.wrapping_add(1);\n\
                 let b = a.saturating_mul(2);\n\
                 let c = b.checked_sub(3);\n\
                 let d = c.unwrap_or(0);\n\
                 let e = x.checked_div(2);\n\
                 let f = e.unwrap_or(0);\n\
                 let g = x.wrapping_neg();\n\
             }"
        )));
    }

    #[test]
    fn bare_negation_and_dynamic_shift_are_traps() {
        assert!(is_trap(&check_rt(
            "fn main() { let x: i32 = 5; let y = -x; }"
        )));
        assert!(is_trap(&check_rt(
            "fn shift(x: i32, n: u32) -> i32 effects() { return x << n; }\n\
             fn main() { let v = shift(1, 2); }"
        )));
        assert!(!is_trap(&check_rt(
            "fn main() { let x: i32 = 5; let y = x << 2; }"
        )));
    }

    #[test]
    fn compound_assignment_is_a_trap_for_arithmetic() {
        assert!(is_trap(&check_rt(
            "fn main() { let mut x: i32 = 0; x += 1; }"
        )));
        assert!(is_trap(&check_rt(
            "fn main() { let mut x: i32 = 10; let y: i32 = 3; x /= y; }"
        )));
        assert!(!is_trap(&check_rt(
            "fn main() { let mut x: i32 = 10; x /= 2; }"
        )));
    }

    #[test]
    fn shift_widths_are_checked_per_integer_type() {
        assert!(!is_trap(&check_rt(
            "fn main() { let x: u8 = 1; let a = x << 7; \
             let y: u16 = 1; let b = y << 15; let z: u128 = 1; let c = z << 127; }"
        )));
        assert!(is_trap(&check_rt(
            "fn main() { let x: u8 = 1; let a = x << 8; }"
        )));
    }

    #[test]
    fn aggregates_and_control_flow_are_walked() {
        assert!(!is_trap(&check_rt(
            "struct P { x: i32 }\n\
             fn main() { let p = P { x: 1 }; let y = p.x; \
             let t = (1, 2); let a = [3, 4]; }"
        )));
        assert!(!is_trap(&check_rt(
            "fn main() { let x: i32 = 1; \
             let y = if true { 1 } else { 2 }; \
             let z = match x { 1 => y, _ => 0 }; }"
        )));
        assert!(!is_trap(&check_rt(
            "fn main() { let mut i: i32 = 0; \
             while i < 2 { i = i.wrapping_add(1); } }"
        )));
        assert!(!is_trap(&check_rt(
            "fn main() { let s = \"abc\"; let n = s.len(); }"
        )));
        assert!(!is_trap(&check_rt(
            "fn main() { let x: i64 = 1; let y = x as i32; }"
        )));
    }

    #[test]
    fn try_blocks_ranges_and_repeat_arrays_are_walked() {
        // `?` exercises ExprKind::Try.
        assert!(!is_trap(&check_rt(
            "fn get() -> Result<i32, Error> { return Ok(1); }\n\
             fn pass() -> Result<i32, Error> { let v = get()?; return Ok(v); }\n\
             fn main() { let v = get().unwrap_or(0); }"
        )));
        // Block expression, array repeat and range/for.
        assert!(!is_trap(&check_rt(
            "fn main() { let x = { 1 }; let a = [0; 3]; \
             let mut acc: i32 = 0; \
             for i in 0..2 { acc = acc.wrapping_add(i); } }"
        )));
    }

    #[test]
    fn every_integer_width_is_known_to_the_shift_rule() {
        assert!(!is_trap(&check_rt(
            "fn main() { \
             let a: i64 = 1; let b = a << 1; \
             let c: u64 = 1; let d = c << 1; \
             let e: isize = 1; let f = e << 1; \
             let g: usize = 1; let h = g << 1; \
             let i: i64 = 5; let j = i.wrapping_neg(); }"
        )));
    }

    #[test]
    fn rt_entry_checks_only_the_reachable_functions() {
        let source = "fn process(a: [i32; 4]) -> i32 { let mut s: i32 = 0; \
                      for i in 0..4usize { s = s.wrapping_add(a[i]); } return s; }\n\
                      fn main() { let a = [1, 2, 3, 4]; let v = process(a); let y = v + 1; }";
        assert!(
            is_trap(&check_rt(source)),
            "without an entry, `main`'s bare `+` is a trap"
        );
        assert!(
            !is_trap(&check_rt_entry(source, "process")),
            "a function outside the reachable set must not be checked"
        );
    }

    #[test]
    fn bounded_index_accepts_literal_bound_and_len_bound() {
        // `for i in 0..K` with K literal <= the array length. Core 1.0 array
        // indexing requires `usize`, hence the `usize` suffix on the bound; the
        // rule itself only reads the literal value.
        let literal = check_rt_entry(
            "fn process(a: [i32; 4]) -> i32 { let mut s: i32 = 0; \
             for i in 0..4usize { s = s.wrapping_add(a[i]); } return s; }\n\
             fn main() { let v = process([1, 2, 3, 4]); }",
            "process",
        );
        assert!(
            literal.iter().all(|code| !code.starts_with('E')),
            "literal-bound loop must be accepted: {literal:?}"
        );
        // `for i in 0..a.len()` over a slice (`&[T]`); arrays have no `.len()`.
        let slice = check_rt_entry(
            "fn process(a: &[i32]) -> i32 { let mut s: i32 = 0; \
             for i in 0..a.len() { s = s.wrapping_add(a[i]); } return s; }\n\
             fn main() { let v = process(&[1, 2, 3]); }",
            "process",
        );
        assert!(
            slice.iter().all(|code| !code.starts_with('E')),
            "slice-len loop must be accepted: {slice:?}"
        );
        // References to arrays are looked through: `&[i32; 4]`.
        let reference = check_rt_entry(
            "fn process(a: &[i32; 4]) -> i32 { let mut s: i32 = 0; \
             for i in 0..4usize { s = s.wrapping_add(a[i]); } return s; }\n\
             fn main() { let b = [1, 2, 3, 4]; let v = process(&b); }",
            "process",
        );
        assert!(
            reference.iter().all(|code| !code.starts_with('E')),
            "a referenced array must be accepted: {reference:?}"
        );
    }

    #[test]
    fn bounded_index_refuses_computed_reassigned_and_too_large() {
        // Index computed from the loop variable: not the recognized form.
        assert!(is_trap(&check_rt_entry(
            "fn process(a: [i32; 4]) -> i32 { let mut s: i32 = 0; \
             for i in 0..4usize { s = s.wrapping_add(a[i.wrapping_add(0)]); } return s; }\n\
             fn main() { let v = process([1, 2, 3, 4]); }",
            "process"
        )));
        // Bound larger than the array length.
        assert!(is_trap(&check_rt_entry(
            "fn process(a: [i32; 4]) -> i32 { let mut s: i32 = 0; \
             for i in 0..9usize { s = s.wrapping_add(a[i]); } return s; }\n\
             fn main() { let v = process([1, 2, 3, 4]); }",
            "process"
        )));
        // The bounded slice is reassigned in the body: its length can change.
        // Core 1.0 refuses storing/assigning a reference (`E-OWN-REF-ESCAPE`),
        // so this clause is defensive: the program is refused for both reasons
        // and the trap must still be emitted.
        assert!(is_trap(&check_rt_entry(
            "fn process(mut a: &[i32], b: &[i32]) -> i32 { let mut s: i32 = 0; \
             for i in 0..a.len() { a = b; s = s.wrapping_add(a[i]); } return s; }\n\
             fn main() { let v = process(&[1, 2, 3], &[4, 5, 6]); }",
            "process"
        )));
        // The loop variable is shadowed in the body: `a[i]` no longer refers to
        // the loop index, so the rule must not apply.
        assert!(is_trap(&check_rt_entry(
            "fn process(a: [i32; 4]) -> i32 { let mut s: i32 = 0; \
             for i in 0..4usize { let i: usize = 9; s = s.wrapping_add(a[i]); } return s; }\n\
             fn main() { let v = process([1, 2, 3, 4]); }",
            "process"
        )));
    }

    #[test]
    fn rt_entry_limits_the_effect_gate_to_the_reachable_functions() {
        let source = "use std::io;\n\
                      fn process(a: [i32; 4]) -> i32 { let mut s: i32 = 0; \
                      for i in 0..4usize { s = s.wrapping_add(a[i]); } return s; }\n\
                      fn main() { let a = [1, 2, 3, 4]; let v = process(a); \
                      io::println(&v.to_string()); }";
        assert!(
            is_trap(&check_rt(source)),
            "whole-package `rt` must refuse `main`'s `io` effect"
        );
        let codes = check_rt_entry(source, "process");
        assert!(
            codes.iter().all(|code| !code.starts_with('E')),
            "outside the reachable set, `io` must not be an error: {codes:?}"
        );
    }

    #[test]
    fn an_entry_that_is_not_a_function_is_refused() {
        let source = "fn main() { }";
        let mut sources = SourceMap::new();
        let file = sources.add("main.rv", source);
        let parsed = parser::parse_file(file, source);
        let codes = check_rt_id(&parsed.program, &sources, 0xdead_beef);
        assert!(
            is_trap(&codes),
            "an unknown entry must fail closed instead of checking nothing: {codes:?}"
        );
    }

    #[test]
    #[should_panic(expected = "parse diagnostics")]
    fn invalid_source_panics_with_rendered_diagnostics() {
        let _ = check_rt("fn main( {");
    }
}
