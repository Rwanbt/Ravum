//! RFC-RT-TRAPS-1 — intrinsic traps of the `rt` profile.
//!
//! In profile `rt`, an operation that can stop the thread produces
//! `E-EFF-PROFILE` unless it is one of the explicit safe forms:
//!
//! * array indexing with a literal index inside the array's known bounds;
//! * division/remainder by a **positive literal** (a bare `-1` is refused: for
//!   signed types `MIN / -1` overflows);
//! * integer `+ - *` and unary `-` only through `wrapping_*`, `saturating_*` or
//!   `checked_*`;
//! * shifts by a literal smaller than the type width;
//! * all floating-point arithmetic and `as` casts.
//!
//! `core` and `pure` are **not** affected: the pass only runs for `rt`
//! (`check.rs`). This is a deliberate asymmetry, documented in the RFC.

use crate::facts::CheckedProgram;
use crate::types::Type;
use ravum_lang_syntax::ast::{BinOp, Block, Expr, ExprKind, ItemKind, Module, StmtKind, UnOp};
use ravum_lang_syntax::diag::{Diagnostic, DiagnosticBag};
use ravum_lang_syntax::span::Span;

/// Check every function body for intrinsic traps (profile `rt` only).
pub fn check_rt_traps(checked: &CheckedProgram, diags: &mut DiagnosticBag) {
    check_module(&checked.program.root, checked, diags);
}

fn check_module(module: &Module, checked: &CheckedProgram, diags: &mut DiagnosticBag) {
    for item in &module.items {
        match &item.kind {
            ItemKind::Fn(function) => check_block(&function.body, checked, diags),
            ItemKind::Impl(implementation) => {
                for member in &implementation.items {
                    if let ItemKind::Fn(function) = &member.kind {
                        check_block(&function.body, checked, diags);
                    }
                }
            }
            ItemKind::Mod(inner) => check_module(inner, checked, diags),
            _ => {}
        }
    }
}

fn check_block(block: &Block, checked: &CheckedProgram, diags: &mut DiagnosticBag) {
    for stmt in &block.stmts {
        match &stmt.kind {
            StmtKind::Let(let_stmt) => {
                if let Some(init) = &let_stmt.init {
                    check_expr(init, checked, diags);
                }
            }
            StmtKind::Expr { expr, .. } => check_expr(expr, checked, diags),
        }
    }
    if let Some(tail) = &block.tail {
        check_expr(tail, checked, diags);
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

fn check_expr(expr: &Expr, checked: &CheckedProgram, diags: &mut DiagnosticBag) {
    match &expr.kind {
        ExprKind::Binary { op, lhs, rhs } => {
            check_binary(
                expr.span,
                *op,
                lhs,
                rhs,
                expr_type(checked, expr),
                checked,
                diags,
            );
            check_expr(lhs, checked, diags);
            check_expr(rhs, checked, diags);
        }
        ExprKind::Unary { op, expr: inner } => {
            if matches!(op, UnOp::Neg) && is_signed_integer(expr_type(checked, inner)) {
                report(
                    diags,
                    expr.span,
                    "integer negation",
                    "use `wrapping_neg`, `saturating_neg` or `checked_neg`",
                );
            }
            check_expr(inner, checked, diags);
        }
        ExprKind::Index { base, index } => {
            if !safe_index(base, index, checked) {
                report(
                    diags,
                    expr.span,
                    "indexing",
                    "index an array with a literal inside its known bounds, or use \
                     `get`/explicit checks (dynamic `Vec`/slice indexing is refused in `rt`)",
                );
            }
            check_expr(base, checked, diags);
            check_expr(index, checked, diags);
        }
        ExprKind::Call { callee, args } => {
            check_expr(callee, checked, diags);
            for arg in args {
                check_expr(arg, checked, diags);
            }
        }
        ExprKind::MethodCall { receiver, args, .. } => {
            check_expr(receiver, checked, diags);
            for arg in args {
                check_expr(arg, checked, diags);
            }
        }
        ExprKind::Field { base, .. } | ExprKind::TupleIndex { base, .. } => {
            check_expr(base, checked, diags);
        }
        ExprKind::Cast { expr: inner, .. } | ExprKind::Try(inner) => {
            check_expr(inner, checked, diags);
        }
        ExprKind::Assign { target, value } => {
            check_expr(target, checked, diags);
            check_expr(value, checked, diags);
        }
        ExprKind::AssignOp { op, target, value } => {
            // Compound assignment is the same trap as the corresponding binary
            // operation: `x += 1` is `x + 1` (RFC-RT-TRAPS-1).
            check_binary(
                expr.span,
                *op,
                target,
                value,
                expr_type(checked, target),
                checked,
                diags,
            );
            check_expr(target, checked, diags);
            check_expr(value, checked, diags);
        }
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => {
            check_expr(cond, checked, diags);
            check_block(then_block, checked, diags);
            if let Some(else_branch) = else_branch {
                check_expr(else_branch, checked, diags);
            }
        }
        ExprKind::Match { scrutinee, arms } => {
            check_expr(scrutinee, checked, diags);
            for arm in arms {
                check_expr(&arm.body, checked, diags);
            }
        }
        ExprKind::While { cond, body } => {
            check_expr(cond, checked, diags);
            check_block(body, checked, diags);
        }
        ExprKind::Loop { body } => check_block(body, checked, diags),
        ExprKind::For { iter, body, .. } => {
            check_expr(iter, checked, diags);
            check_block(body, checked, diags);
        }
        ExprKind::Block(block) | ExprKind::Unsafe(block) => check_block(block, checked, diags),
        ExprKind::Tuple(items) | ExprKind::Array(items) => {
            for item in items {
                check_expr(item, checked, diags);
            }
        }
        ExprKind::ArrayRepeat { value, len } => {
            check_expr(value, checked, diags);
            check_expr(len, checked, diags);
        }
        ExprKind::StructLit { fields, .. } => {
            for field in fields {
                check_expr(&field.value, checked, diags);
            }
        }
        ExprKind::Range { start, end, .. } => {
            if let Some(start) = start {
                check_expr(start, checked, diags);
            }
            if let Some(end) = end {
                check_expr(end, checked, diags);
            }
        }
        ExprKind::Return(Some(value)) => check_expr(value, checked, diags),
        _ => {}
    }
}

/// Shared trap rule for `ExprKind::Binary` and compound assignment.
///
/// `result` is the expression's type for a binary operation, and the target's
/// type for `x op= y` (which produces `()` but assigns to an integer).
fn check_binary(
    span: Span,
    op: BinOp,
    lhs: &Expr,
    rhs: &Expr,
    result: Option<&Type>,
    checked: &CheckedProgram,
    diags: &mut DiagnosticBag,
) {
    match op {
        BinOp::Add | BinOp::Sub | BinOp::Mul => {
            if is_integer(result) {
                report(
                    diags,
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
                    diags,
                    span,
                    "integer division/remainder",
                    "divide by a positive literal or use `checked_div`/`checked_rem`",
                );
            }
        }
        BinOp::Shl | BinOp::Shr => {
            if is_integer(expr_type(checked, lhs)) && !safe_shift(lhs, rhs, checked) {
                report(
                    diags,
                    span,
                    "integer shift",
                    "shift by a literal smaller than the type width",
                );
            }
        }
        _ => {}
    }
}

fn expr_type<'a>(checked: &'a CheckedProgram, expr: &Expr) -> Option<&'a Type> {
    checked.facts.expr_types.get(&expr.id)
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

fn safe_index(base: &Expr, index: &Expr, checked: &CheckedProgram) -> bool {
    let Some(Type::Array { len, .. }) = expr_type(checked, base) else {
        return false;
    };
    matches!(index.kind, ExprKind::Int { value, .. } if value < *len as u128)
}

#[cfg(test)]
mod tests {
    use crate::check;
    use crate::facts::{CheckOptions, Profile};
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
    #[should_panic(expected = "parse diagnostics")]
    fn invalid_source_panics_with_rendered_diagnostics() {
        let _ = check_rt("fn main( {");
    }
}
