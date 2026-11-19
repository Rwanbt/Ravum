use super::{parse_file, ParseResult};
use crate::ast::*;
use std::collections::BTreeSet;
use std::time::Instant;

fn parse(src: &str) -> ParseResult {
    parse_file(0, src)
}

fn dump_diags(result: &ParseResult) -> String {
    result
        .diags
        .iter()
        .map(|d| {
            format!(
                "{}[{}..{}]: {}",
                d.code, d.span.start, d.span.end, d.message
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_ok(src: &str) -> Program {
    let result = parse(src);
    assert!(
        !result.diags.has_errors(),
        "unexpected diagnostics for {src:?}:\n{}",
        dump_diags(&result)
    );
    result.program
}

fn top(program: &Program) -> &[Item] {
    &program.root.items
}

fn as_fn(item: &Item) -> &FnItem {
    match &item.kind {
        ItemKind::Fn(f) => f,
        other => panic!("expected a fn item, got {other:?}"),
    }
}

fn let_init(f: &FnItem, index: usize) -> &Expr {
    match &f.body.stmts[index].kind {
        StmtKind::Let(l) => l
            .init
            .as_ref()
            .unwrap_or_else(|| panic!("let #{index} has no initializer")),
        other => panic!("statement #{index} is not a let: {other:?}"),
    }
}

fn let_pat(f: &FnItem, index: usize) -> &Pat {
    match &f.body.stmts[index].kind {
        StmtKind::Let(l) => &l.pat,
        other => panic!("statement #{index} is not a let: {other:?}"),
    }
}

fn stmt_expr(stmt: &Stmt) -> (&Expr, bool) {
    match &stmt.kind {
        StmtKind::Expr { expr, semi } => (expr, *semi),
        other => panic!("statement is not an expression: {other:?}"),
    }
}

fn diags_of(src: &str) -> Vec<(String, u32, u32)> {
    parse(src)
        .diags
        .iter()
        .map(|d| (d.code.clone(), d.span.start, d.span.end))
        .collect()
}

fn assert_error_at(src: &str, code: &str, start: u32) {
    let diags = diags_of(src);
    assert!(
        diags.iter().any(|(c, s, _)| c == code && *s == start),
        "expected {code} at byte {start} for {src:?}, got {diags:?}"
    );
}

// -----------------------------------------------------------------
// Items
// -----------------------------------------------------------------

#[test]
fn unsupported_closure_reports_single_canonical_code() {
    let result = parse("fn main() { let f = |x: i32| x + 1; }");
    let codes: BTreeSet<String> = result.diags.iter().map(|d| d.code.clone()).collect();
    assert_eq!(
        codes,
        BTreeSet::from(["E-UNSUPPORTED-CLOSURE".to_string()]),
        "{}",
        dump_diags(&result)
    );
    assert_eq!(result.diags.error_count(), 1, "{}", dump_diags(&result));
}

#[test]
fn raw_pointer_type_is_e_unsupported_unsafe() {
    let result = parse("fn f(p: *const i32) {}\nfn g(p: *mut u8) {}\n");
    let codes: BTreeSet<String> = result.diags.iter().map(|d| d.code.clone()).collect();
    assert_eq!(
        codes,
        BTreeSet::from(["E-UNSUPPORTED-UNSAFE".to_string()]),
        "{}",
        dump_diags(&result)
    );
    assert_eq!(result.diags.error_count(), 2, "{}", dump_diags(&result));
}

#[test]
fn attribute_reports_only_e_unsupported_attr() {
    let result = parse("#[test]\nfn main() {}\n");
    let codes: BTreeSet<String> = result.diags.iter().map(|d| d.code.clone()).collect();
    assert_eq!(
        codes,
        BTreeSet::from(["E-UNSUPPORTED-ATTR".to_string()]),
        "{}",
        dump_diags(&result)
    );
}

#[test]
fn assignment_in_condition_is_e_parse_unexpected() {
    let result = parse("fn main() { let mut x = 1; if (x = 1) { } }");
    let codes: BTreeSet<String> = result.diags.iter().map(|d| d.code.clone()).collect();
    assert_eq!(
        codes,
        BTreeSet::from(["E-PARSE-UNEXPECTED".to_string()]),
        "{}",
        dump_diags(&result)
    );
}

#[test]
fn chained_comparison_is_rejected_but_parenthesized_is_accepted() {
    let result = parse("fn chained(a: i32, b: i32, c: i32) -> bool { let x = a < b < c; x }");
    let codes: BTreeSet<String> = result.diags.iter().map(|d| d.code.clone()).collect();
    assert_eq!(
        codes,
        BTreeSet::from(["E-PARSE-UNEXPECTED".to_string()]),
        "{}",
        dump_diags(&result)
    );

    let ok = parse_ok("fn f(a: bool, b: bool, c: bool) -> bool { (a == b) == c }");
    assert_ne!(ok.root.id, 0);
}

#[test]
fn misplaced_effects_is_a_single_parse_expected() {
    let result = parse("fn f() effects(io) -> i32 { return 0; }\nfn main() {}\n");
    let codes: BTreeSet<String> = result.diags.iter().map(|d| d.code.clone()).collect();
    assert_eq!(
        codes,
        BTreeSet::from(["E-PARSE-EXPECTED".to_string()]),
        "{}",
        dump_diags(&result)
    );
}

#[test]
fn multi_error_recovery_reports_both_errors() {
    let src = "const C = 1;\n\nfn chained(a: i32, b: i32, c: i32) -> bool {\n    a < b < c\n}\n\nfn main() {\n}\n";
    let result = parse(src);
    let codes: BTreeSet<String> = result.diags.iter().map(|d| d.code.clone()).collect();
    assert_eq!(
        codes,
        BTreeSet::from([
            "E-PARSE-EXPECTED".to_string(),
            "E-PARSE-UNEXPECTED".to_string(),
        ]),
        "{}",
        dump_diags(&result)
    );
    assert_eq!(top(&result.program).len(), 3);
}

#[test]
fn chained_tuple_index_requires_parentheses() {
    let result = parse("fn f(t: (i32, i32)) { let x = t.0.1; }");
    let codes: BTreeSet<String> = result.diags.iter().map(|d| d.code.clone()).collect();
    assert_eq!(
        codes,
        BTreeSet::from(["E-PARSE-UNEXPECTED".to_string()]),
        "{}",
        dump_diags(&result)
    );

    let ok = parse_ok("fn f(t: (i32, i32)) { let x = (t.0).0; }");
    assert_ne!(ok.root.id, 0);
}

#[test]
fn range_after_integer_is_not_a_float() {
    let result = parse("fn main() { for i in 0..10 { let x = i; } }");
    assert!(!result.diags.has_errors(), "{}", dump_diags(&result));
}

#[test]
fn empty_and_comment_only_files() {
    for src in ["", "// nothing\n", "/* nothing */\n"] {
        let program = parse_ok(src);
        assert!(program.root.items.is_empty(), "{src:?}");
        assert_ne!(program.root.id, 0);
        assert_eq!(program.root.id >> 32, 0);
    }
}

#[test]
fn modules_and_use_items() {
    let src = "\
mod foo;
mod bar { fn inner() {} }
use crate::foo::Bar;
use super::x;
use self::y;
";
    let program = parse_ok(src);
    let items = top(&program);
    assert_eq!(items.len(), 5);
    match &items[0].kind {
        ItemKind::ModDecl(decl) => assert_eq!(decl.name.name, "foo"),
        other => panic!("expected mod decl, got {other:?}"),
    }
    match &items[1].kind {
        ItemKind::Mod(module) => {
            assert_eq!(module.name.as_ref().unwrap().name, "bar");
            assert_eq!(module.items.len(), 1);
            assert_eq!(as_fn(&module.items[0]).name.name, "inner");
        }
        other => panic!("expected inline mod, got {other:?}"),
    }
    match &items[2].kind {
        ItemKind::Use(u) => {
            assert_eq!(u.path.root, PathRoot::Crate);
            assert_eq!(u.path.display(), "crate::foo::Bar");
        }
        other => panic!("expected use, got {other:?}"),
    }
    match &items[3].kind {
        ItemKind::Use(u) => assert_eq!(u.path.root, PathRoot::Super),
        other => panic!("expected use, got {other:?}"),
    }
    match &items[4].kind {
        ItemKind::Use(u) => assert_eq!(u.path.root, PathRoot::SelfMod),
        other => panic!("expected use, got {other:?}"),
    }
}

#[test]
fn function_signature_and_body() {
    let src = "\
pub fn compute<T, U>(a: T, mut b: u32) -> Vec<T> effects(io, alloc) {
    let x: i32 = 1;
    let y: i32;
    return y;
}
";
    let program = parse_ok(src);
    let item = &top(&program)[0];
    assert!(item.vis.is_public());
    let f = as_fn(item);
    assert_eq!(f.name.name, "compute");
    assert!(!f.is_test);
    assert_eq!(f.generics.len(), 2);
    assert_eq!(f.generics[0].name.name, "T");
    assert_eq!(f.generics[1].name.name, "U");
    assert_eq!(f.params.len(), 2);
    match &f.params[0].kind {
        ParamKind::Binding { name, ty, .. } => {
            assert_eq!(name.name, "a");
            assert!(matches!(&ty.kind, TypeKind::Path(p) if p.display() == "T"));
        }
        other => panic!("expected binding, got {other:?}"),
    }
    match &f.params[1].kind {
        // `mut` on a parameter is accepted and ignored by the frozen AST.
        ParamKind::Binding { name, ty, .. } => {
            assert_eq!(name.name, "b");
            assert!(matches!(&ty.kind, TypeKind::Path(p) if p.display() == "u32"));
        }
        other => panic!("expected binding, got {other:?}"),
    }
    match &f.ret.kind {
        TypeKind::Path(path) => {
            assert_eq!(path.display(), "Vec");
            assert_eq!(path.segments[0].args.len(), 1);
        }
        other => panic!("expected Vec<T> return, got {other:?}"),
    }
    let effects = f.effects.as_ref().expect("effects clause");
    assert_eq!(
        effects.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        vec!["io", "alloc"]
    );
    assert_eq!(f.body.stmts.len(), 3);
    assert!(f.body.tail.is_none());
    match &f.body.stmts[1].kind {
        StmtKind::Let(l) => {
            assert!(l.init.is_none());
            assert!(matches!(&l.ty.as_ref().unwrap().kind, TypeKind::Path(_)));
        }
        other => panic!("expected typed let without init, got {other:?}"),
    }
    let (expr, semi) = stmt_expr(&f.body.stmts[2]);
    assert!(semi);
    assert!(matches!(&expr.kind, ExprKind::Return(Some(_))));
}

#[test]
fn test_functions_and_unit_return() {
    let program = parse_ok("test fn works() { assert_eq(1, 1); }");
    let f = as_fn(&top(&program)[0]);
    assert!(f.is_test);
    assert_eq!(f.name.name, "works");
    assert!(f.params.is_empty());
    assert!(matches!(f.ret.kind, TypeKind::Unit));
    assert_eq!(f.body.stmts.len(), 1);
}

#[test]
fn struct_items_unit_named_and_visibility() {
    let src = "struct Unit;\npub struct Point<T> { pub x: T, y: T }\n";
    let program = parse_ok(src);
    let items = top(&program);
    match &items[0].kind {
        ItemKind::Struct(s) => {
            assert_eq!(s.name.name, "Unit");
            assert!(s.fields.is_empty());
            assert!(s.generics.is_empty());
        }
        other => panic!("expected unit struct, got {other:?}"),
    }
    assert!(items[1].vis.is_public());
    match &items[1].kind {
        ItemKind::Struct(s) => {
            assert_eq!(s.name.name, "Point");
            assert_eq!(s.generics.len(), 1);
            assert_eq!(s.fields.len(), 2);
            assert!(s.fields[0].vis.is_public());
            assert_eq!(s.fields[0].name.name, "x");
            assert!(!s.fields[1].vis.is_public());
            assert_eq!(s.fields[1].name.name, "y");
        }
        other => panic!("expected struct, got {other:?}"),
    }
}

#[test]
fn enum_variants_all_forms() {
    let src = "pub enum Shape<T> { Empty, Circle(T), Rect { w: T, h: T }, pub Unit }";
    let program = parse_ok(src);
    let item = &top(&program)[0];
    assert!(item.vis.is_public());
    let e = match &item.kind {
        ItemKind::Enum(e) => e,
        other => panic!("expected enum, got {other:?}"),
    };
    assert_eq!(e.name.name, "Shape");
    assert_eq!(e.generics.len(), 1);
    assert_eq!(e.variants.len(), 4);
    assert_eq!(e.variants[0].name.name, "Empty");
    assert!(matches!(e.variants[0].fields, VariantDefFields::Unit));
    match &e.variants[1].fields {
        VariantDefFields::Tuple(tys) => {
            assert_eq!(tys.len(), 1);
            assert!(matches!(&tys[0].kind, TypeKind::Path(p) if p.display() == "T"));
        }
        other => panic!("expected tuple variant, got {other:?}"),
    }
    match &e.variants[2].fields {
        VariantDefFields::Struct(fields) => {
            assert_eq!(fields.len(), 2);
            assert_eq!(fields[0].name.name, "w");
            assert_eq!(fields[1].name.name, "h");
        }
        other => panic!("expected struct variant, got {other:?}"),
    }
    assert!(matches!(e.variants[3].fields, VariantDefFields::Unit));
}

#[test]
fn impl_generics_receivers_and_associated_fns() {
    let src = "\
impl<T> Vec<T> {
    fn new() -> T { x }
    fn get(&self) -> T { x }
    fn set(&mut self, v: T) { }
}
";
    let program = parse_ok(src);
    let imp = match &top(&program)[0].kind {
        ItemKind::Impl(i) => i,
        other => panic!("expected impl, got {other:?}"),
    };
    assert_eq!(imp.generics.len(), 1);
    match &imp.self_ty.kind {
        TypeKind::Path(p) => {
            assert_eq!(p.display(), "Vec");
            assert_eq!(p.segments[0].args.len(), 1);
        }
        other => panic!("expected path self type, got {other:?}"),
    }
    assert_eq!(imp.items.len(), 3);
    let new = as_fn(&imp.items[0]);
    assert_eq!(new.name.name, "new");
    assert!(new.params.is_empty());
    let get = as_fn(&imp.items[1]);
    assert!(matches!(
        get.params[0].kind,
        ParamKind::Receiver(Receiver::Shared)
    ));
    let set = as_fn(&imp.items[2]);
    assert!(matches!(
        set.params[0].kind,
        ParamKind::Receiver(Receiver::Exclusive)
    ));
    match &set.params[1].kind {
        ParamKind::Binding { name, .. } => assert_eq!(name.name, "v"),
        other => panic!("expected binding, got {other:?}"),
    }
}

#[test]
fn const_item() {
    let program = parse_ok("const N: i32 = 10;");
    let item = &top(&program)[0];
    let c = match &item.kind {
        ItemKind::Const(c) => c,
        other => panic!("expected const, got {other:?}"),
    };
    assert_eq!(c.name.name, "N");
    assert!(matches!(&c.ty.kind, TypeKind::Path(p) if p.display() == "i32"));
    match &c.value.kind {
        ExprKind::Int { value, suffix } => {
            assert_eq!(*value, 10);
            assert!(suffix.is_none());
        }
        other => panic!("expected int literal, got {other:?}"),
    }
}

// -----------------------------------------------------------------
// Types
// -----------------------------------------------------------------

#[test]
fn all_type_forms() {
    let src = "fn types(a: &i32, b: &mut i32, c: [i32; 3], d: [i32], e: (i32, u8), f: (), g: _, h: Self, i: Vec<i32>) {}";
    let program = parse_ok(src);
    let f = as_fn(&top(&program)[0]);
    assert_eq!(f.params.len(), 9);
    let ty = |n: usize| match &f.params[n].kind {
        ParamKind::Binding { ty, .. } => ty,
        other => panic!("expected binding, got {other:?}"),
    };
    assert!(matches!(&ty(0).kind, TypeKind::Ref { mutable: false, .. }));
    assert!(matches!(&ty(1).kind, TypeKind::Ref { mutable: true, .. }));
    match &ty(2).kind {
        TypeKind::Array { elem, len } => {
            assert!(matches!(&elem.kind, TypeKind::Path(_)));
            assert!(matches!(&len.kind, ExprKind::Int { value: 3, .. }));
        }
        other => panic!("expected array, got {other:?}"),
    }
    assert!(matches!(&ty(3).kind, TypeKind::Slice(_)));
    match &ty(4).kind {
        TypeKind::Tuple(parts) => assert_eq!(parts.len(), 2),
        other => panic!("expected tuple, got {other:?}"),
    }
    assert!(matches!(&ty(5).kind, TypeKind::Unit));
    assert!(matches!(&ty(6).kind, TypeKind::Infer));
    assert!(matches!(&ty(7).kind, TypeKind::SelfType));
    match &ty(8).kind {
        TypeKind::Path(p) => {
            assert_eq!(p.display(), "Vec");
            assert_eq!(p.segments[0].args.len(), 1);
        }
        other => panic!("expected generic path, got {other:?}"),
    }
}

// -----------------------------------------------------------------
// Expressions
// -----------------------------------------------------------------

#[test]
fn literals_paths_calls_methods_fields_and_indices() {
    let src = "\
impl S {
    fn m(&self) {
        let a = self;
        let b = self.x;
        let c = 1u8;
        let d = 2.5f32;
        let e = true;
        let f = 'z';
        let g = \"hi\";
        let h = foo::bar(1);
        let i = a.method(1, 2);
        let j = pair.0;
        let k = arr[idx];
    }
}
";
    let program = parse_ok(src);
    let imp = match &top(&program)[0].kind {
        ItemKind::Impl(i) => i,
        other => panic!("expected impl, got {other:?}"),
    };
    let f = as_fn(&imp.items[0]);
    assert!(matches!(let_init(f, 0).kind, ExprKind::SelfValue));
    assert!(matches!(&let_init(f, 1).kind, ExprKind::Field { .. }));
    match &let_init(f, 2).kind {
        ExprKind::Int { value, suffix } => {
            assert_eq!(*value, 1);
            assert_eq!(*suffix, Some(IntTy::U8));
        }
        other => panic!("expected int, got {other:?}"),
    }
    match &let_init(f, 3).kind {
        ExprKind::Float { suffix, .. } => assert_eq!(*suffix, Some(FloatTy::F32)),
        other => panic!("expected float, got {other:?}"),
    }
    assert!(matches!(let_init(f, 4).kind, ExprKind::Bool(true)));
    assert!(matches!(let_init(f, 5).kind, ExprKind::Char('z')));
    assert!(matches!(&let_init(f, 6).kind, ExprKind::Str(s) if s == "hi"));
    match &let_init(f, 7).kind {
        ExprKind::Call { callee, args } => {
            assert!(matches!(&callee.kind, ExprKind::Path(p) if p.display() == "foo::bar"));
            assert_eq!(args.len(), 1);
        }
        other => panic!("expected call, got {other:?}"),
    }
    match &let_init(f, 8).kind {
        ExprKind::MethodCall { method, args, .. } => {
            assert_eq!(method.name, "method");
            assert_eq!(args.len(), 2);
        }
        other => panic!("expected method call, got {other:?}"),
    }
    match &let_init(f, 9).kind {
        ExprKind::TupleIndex { index, .. } => assert_eq!(*index, 0),
        other => panic!("expected tuple index, got {other:?}"),
    }
    assert!(matches!(&let_init(f, 10).kind, ExprKind::Index { .. }));
}

#[test]
fn binary_precedence_follows_design() {
    let program = parse_ok("fn f() { let x = 1 + 2 * 3; }");
    let f = as_fn(&top(&program)[0]);
    match &let_init(f, 0).kind {
        ExprKind::Binary { op, lhs, rhs } => {
            assert_eq!(*op, BinOp::Add);
            assert!(matches!(lhs.kind, ExprKind::Int { value: 1, .. }));
            match &rhs.kind {
                ExprKind::Binary { op, lhs, rhs } => {
                    assert_eq!(*op, BinOp::Mul);
                    assert!(matches!(lhs.kind, ExprKind::Int { value: 2, .. }));
                    assert!(matches!(rhs.kind, ExprKind::Int { value: 3, .. }));
                }
                other => panic!("expected 2 * 3, got {other:?}"),
            }
        }
        other => panic!("expected binary expression, got {other:?}"),
    }

    let program = parse_ok("fn f() { let x = a || b && c == d; }");
    let f = as_fn(&top(&program)[0]);
    match &let_init(f, 0).kind {
        ExprKind::Binary { op, rhs, .. } => {
            assert_eq!(*op, BinOp::Or);
            assert!(matches!(&rhs.kind, ExprKind::Binary { op: BinOp::And, .. }));
        }
        other => panic!("expected || at the root, got {other:?}"),
    }
}

#[test]
fn cast_and_assignment_operators() {
    let src = "fn f() { x = 1 as u8; x += 2; x >>= 1; }";
    let program = parse_ok(src);
    let f = as_fn(&top(&program)[0]);
    match &f.body.stmts[0].kind {
        StmtKind::Expr { expr, semi } => {
            assert!(semi);
            match &expr.kind {
                ExprKind::Assign { target, value } => {
                    assert!(matches!(&target.kind, ExprKind::Path(_)));
                    assert!(matches!(&value.kind, ExprKind::Cast { .. }));
                }
                other => panic!("expected assignment, got {other:?}"),
            }
        }
        other => panic!("expected expression statement, got {other:?}"),
    }
    match &stmt_expr(&f.body.stmts[1]).0.kind {
        ExprKind::AssignOp { op, .. } => assert_eq!(*op, CompoundOp::Add),
        other => panic!("expected +=, got {other:?}"),
    }
    // `>>=` is lexed as one token and split internally; it must stay Shr.
    match &stmt_expr(&f.body.stmts[2]).0.kind {
        ExprKind::AssignOp { op, .. } => assert_eq!(*op, CompoundOp::Shr),
        other => panic!("expected >>=, got {other:?}"),
    }
}

#[test]
fn if_else_chain_is_nested_expressions() {
    let program = parse_ok("fn f() { let r = if a { 1 } else if b { 2 } else { 3 }; }");
    let f = as_fn(&top(&program)[0]);
    match &let_init(f, 0).kind {
        ExprKind::If { else_branch, .. } => {
            let Some(else_branch) = else_branch else {
                panic!("missing else branch");
            };
            match &else_branch.kind {
                ExprKind::If { else_branch, .. } => {
                    let Some(inner) = else_branch else {
                        panic!("missing final else");
                    };
                    assert!(matches!(&inner.kind, ExprKind::Block(_)));
                }
                other => panic!("expected else-if, got {other:?}"),
            }
        }
        other => panic!("expected if expression, got {other:?}"),
    }
}

#[test]
fn match_arms_and_pattern_forms() {
    let src =
        "fn f() { let r = match x { Foo::A => 1, Foo::B(y) => y, Foo { f } => f, _ => 0, }; }";
    let program = parse_ok(src);
    let f = as_fn(&top(&program)[0]);
    let arms = match &let_init(f, 0).kind {
        ExprKind::Match { arms, .. } => arms,
        other => panic!("expected match, got {other:?}"),
    };
    assert_eq!(arms.len(), 4);
    assert!(matches!(&arms[0].pat.kind, PatKind::Path(_)));
    match &arms[1].pat.kind {
        PatKind::Variant { fields, .. } => {
            assert!(matches!(fields, VariantPatFields::Tuple(p) if p.len() == 1));
        }
        other => panic!("expected tuple variant pattern, got {other:?}"),
    }
    match &arms[2].pat.kind {
        PatKind::Variant { fields, .. } => match fields {
            VariantPatFields::Struct(fields) => {
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].name.name, "f");
            }
            other => panic!("expected struct fields, got {other:?}"),
        },
        other => panic!("expected struct variant pattern, got {other:?}"),
    }
    assert!(matches!(arms[3].pat.kind, PatKind::Wild));
}

#[test]
fn block_tuple_array_struct_literal_expressions() {
    let src = "fn f() { let b = { 1 }; let t = (1, 2); let u = (); let arr = [1, 2]; let rep = [0; 4]; let p = Point { x: 1, y }; }";
    let program = parse_ok(src);
    let f = as_fn(&top(&program)[0]);
    match &let_init(f, 0).kind {
        ExprKind::Block(block) => assert!(block.tail.is_some()),
        other => panic!("expected block, got {other:?}"),
    }
    match &let_init(f, 1).kind {
        ExprKind::Tuple(items) => assert_eq!(items.len(), 2),
        other => panic!("expected tuple, got {other:?}"),
    }
    assert!(matches!(let_init(f, 2).kind, ExprKind::Unit));
    match &let_init(f, 3).kind {
        ExprKind::Array(items) => assert_eq!(items.len(), 2),
        other => panic!("expected array, got {other:?}"),
    }
    match &let_init(f, 4).kind {
        ExprKind::ArrayRepeat { value, len } => {
            assert!(matches!(value.kind, ExprKind::Int { value: 0, .. }));
            assert!(matches!(len.kind, ExprKind::Int { value: 4, .. }));
        }
        other => panic!("expected array repeat, got {other:?}"),
    }
    match &let_init(f, 5).kind {
        ExprKind::StructLit { fields, path } => {
            assert_eq!(path.display(), "Point");
            assert_eq!(fields.len(), 2);
            assert_eq!(fields[0].name.name, "x");
            // Shorthand `y` becomes a path expression.
            assert!(matches!(&fields[1].value.kind, ExprKind::Path(p) if p.display() == "y"));
        }
        other => panic!("expected struct literal, got {other:?}"),
    }
}

#[test]
fn unsafe_block_and_extern_block_parse() {
    let program = parse_ok(
        "extern \"C\" {\n    fn abs(x: i32) -> i32;\n}\nfn main() { let v = unsafe { abs(-1) }; }",
    );
    let items = top(&program);
    match &items[0].kind {
        ItemKind::Extern(block) => {
            assert_eq!(block.abi.name, "C");
            assert_eq!(block.fns.len(), 1);
            assert_eq!(block.fns[0].name.name, "abs");
            assert_eq!(block.fns[0].params.len(), 1);
        }
        other => panic!("expected an extern block, got {other:?}"),
    }
    let main = as_fn(&items[1]);
    assert!(matches!(&let_init(main, 0).kind, ExprKind::Unsafe(_)));
}

#[test]
fn ranges_try_and_return() {
    // A range is a `for`-clause construct (diagnostics.md §3,
    // E-UNSUPPORTED-RANGE, parse phase). The AST is still produced so the
    // rest of the function parses, but the construct must be reported here
    // and not reclassified later as a type error.
    let source = "fn f() -> u8 { let r = 0..n; let s = 0..=n; let t = g()?; return t; }";
    let result = parse(source);
    let codes: Vec<&str> = result.diags.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(
        codes,
        vec!["E-UNSUPPORTED-RANGE", "E-UNSUPPORTED-RANGE"],
        "a range outside a `for` clause is refused in the parse phase: {}",
        dump_diags(&result)
    );
    let program = result.program;
    let f = as_fn(&top(&program)[0]);
    match &let_init(f, 0).kind {
        ExprKind::Range {
            start,
            end,
            inclusive,
        } => {
            assert!(start.is_some() && end.is_some());
            assert!(!inclusive);
        }
        other => panic!("expected exclusive range, got {other:?}"),
    }
    match &let_init(f, 1).kind {
        ExprKind::Range { inclusive, .. } => assert!(inclusive),
        other => panic!("expected inclusive range, got {other:?}"),
    }
    assert!(matches!(&let_init(f, 2).kind, ExprKind::Try(_)));
    let (expr, semi) = stmt_expr(&f.body.stmts[3]);
    assert!(semi);
    match &expr.kind {
        ExprKind::Return(Some(value)) => {
            assert!(matches!(&value.kind, ExprKind::Path(_)));
        }
        other => panic!("expected return with value, got {other:?}"),
    }
}

/// Only the outermost expression of a `for` iterator may be a range: a range
/// nested in it (an argument, an index, a block, a bound) or used as an operand
/// is outside a `for` clause and refused in the parse phase. Before this, the
/// nested forms reached the back end (rustc E0308/E0277) or built.
#[test]
fn a_range_nested_in_a_for_iterator_is_refused() {
    for source in [
        "fn f() { for i in 0..g(..3) { } }",
        "fn f() { for i in [..3][0] { } }",
        "fn f() { for i in 0..{ let r = ..3; 2 } { } }",
        "fn f() { for i in (0..3) { } }",
        "fn f() { for i in 0..2 + ..3 { } }",
        "fn f() { let x = 1 + ..3; }",
    ] {
        let result = parse(source);
        let codes: Vec<&str> = result.diags.iter().map(|d| d.code.as_str()).collect();
        assert_eq!(
            codes,
            ["E-UNSUPPORTED-RANGE"],
            "{source}: {}",
            dump_diags(&result)
        );
    }
    for source in [
        "fn f() { for i in 0..3 { for j in 0..i { } } }",
        "fn f() { for i in ..3 { } for k in 0..=2 { } }",
        "fn f() { for i in 0..{ for j in 0..2 { } 3 } { } }",
    ] {
        let result = parse(source);
        assert!(result.diags.is_empty(), "{source}: {}", dump_diags(&result));
    }
}

#[test]
fn loop_expressions() {
    let src = "fn f() { while i < 10 { i += 1; } loop { break; } for x in 0..3 { continue; } }";
    let program = parse_ok(src);
    let f = as_fn(&top(&program)[0]);
    // `for` is the final expression of the block, so it is the block tail
    // (a trailing block-like expression is the block's value).
    assert_eq!(f.body.stmts.len(), 2);
    let for_expr = f.body.tail.as_ref().expect("for must be the block tail");
    let (while_expr, semi) = stmt_expr(&f.body.stmts[0]);
    assert!(!semi, "block-like statements do not need `;`");
    match &while_expr.kind {
        ExprKind::While { cond, body } => {
            assert!(matches!(&cond.kind, ExprKind::Binary { op: BinOp::Lt, .. }));
            assert_eq!(body.stmts.len(), 1);
        }
        other => panic!("expected while, got {other:?}"),
    }
    match &stmt_expr(&f.body.stmts[1]).0.kind {
        ExprKind::Loop { body } => {
            let (break_expr, _) = stmt_expr(&body.stmts[0]);
            assert!(matches!(break_expr.kind, ExprKind::Break));
        }
        other => panic!("expected loop, got {other:?}"),
    }
    match &for_expr.kind {
        ExprKind::For { pat, iter, body } => {
            assert!(matches!(&pat.kind, PatKind::Binding { mutable: false, .. }));
            match &iter.kind {
                ExprKind::Range {
                    start,
                    end,
                    inclusive,
                } => {
                    assert!(start.is_some() && end.is_some());
                    assert!(!inclusive);
                }
                other => panic!("expected range iterator, got {other:?}"),
            }
            let (continue_expr, _) = stmt_expr(&body.stmts[0]);
            assert!(matches!(continue_expr.kind, ExprKind::Continue));
        }
        other => panic!("expected for, got {other:?}"),
    }
}

#[test]
fn pattern_forms() {
    let src = "fn f() { let _ = 1; let mut x = 2; let (a, b) = p; let E::V(q) = e; let E::S { f: g, h } = e; }";
    let program = parse_ok(src);
    let f = as_fn(&top(&program)[0]);
    assert!(matches!(let_pat(f, 0).kind, PatKind::Wild));
    assert!(matches!(
        let_pat(f, 1).kind,
        PatKind::Binding { mutable: true, .. }
    ));
    match &let_pat(f, 2).kind {
        PatKind::Tuple(pats) => assert_eq!(pats.len(), 2),
        other => panic!("expected tuple pattern, got {other:?}"),
    }
    match &let_pat(f, 3).kind {
        PatKind::Variant { fields, .. } => match fields {
            VariantPatFields::Tuple(pats) => assert_eq!(pats.len(), 1),
            other => panic!("expected tuple variant pattern, got {other:?}"),
        },
        other => panic!("expected variant pattern, got {other:?}"),
    }
    match &let_pat(f, 4).kind {
        PatKind::Variant { fields, .. } => match fields {
            VariantPatFields::Struct(fields) => {
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].name.name, "f");
                assert!(
                    matches!(&fields[0].pat.kind, PatKind::Binding { name, .. } if name.name == "g")
                );
                // Shorthand `h` binds `h`.
                assert!(
                    matches!(&fields[1].pat.kind, PatKind::Binding { name, .. } if name.name == "h")
                );
            }
            other => panic!("expected struct pattern fields, got {other:?}"),
        },
        other => panic!("expected struct variant pattern, got {other:?}"),
    }
}

// -----------------------------------------------------------------
// Node ids
// -----------------------------------------------------------------

const RICH: &str = r#"
const N: i32 = 10;

pub struct Point<T> {
    pub x: T,
    y: T,
}

pub enum Shape<T> {
    Empty,
    Circle(T),
    Rect { w: T, h: T },
}

impl<T> Point<T> {
    pub fn new(x: T, mut y: T) -> Point<T> {
        Point { x, y }
    }

    fn norm(&self) -> f64 {
        let mut total: f64 = 0.0;
        while total < 10.0 {
            total += 1.0;
        }
        for i in 0..3 {
            let _ = i;
        }
        if self.x == self.x {
            total
        } else {
            0.0
        }
    }
}

fn apply<T, U>(a: &T, b: &mut U, c: [i32; 3], d: [i32], e: (i32, u8), g: _, h: Self) -> Result<T, U> effects(io, alloc) {
    let r = [1, 2, 3];
    let s = [0; 3];
    let t = match a {
        _ => 1,
    };
    let u = foo::bar(a)?;
    return u;
}

test fn works() {
    assert_eq(1, 1);
}

mod inner {
    use crate::N;
}
"#;

fn collect_module(module: &Module, out: &mut Vec<NodeId>) {
    out.push(module.id);
    for item in &module.items {
        collect_item(item, out);
    }
}

fn collect_item(item: &Item, out: &mut Vec<NodeId>) {
    out.push(item.id);
    match &item.kind {
        ItemKind::Fn(f) => {
            for generic in &f.generics {
                out.push(generic.id);
            }
            for param in &f.params {
                out.push(param.id);
                if let ParamKind::Binding { ty, .. } = &param.kind {
                    collect_type(ty, out);
                }
            }
            collect_type(&f.ret, out);
            collect_block(&f.body, out);
        }
        ItemKind::Struct(s) => {
            for generic in &s.generics {
                out.push(generic.id);
            }
            for field in &s.fields {
                out.push(field.id);
                collect_type(&field.ty, out);
            }
        }
        ItemKind::Enum(e) => {
            for generic in &e.generics {
                out.push(generic.id);
            }
            for variant in &e.variants {
                out.push(variant.id);
                match &variant.fields {
                    VariantDefFields::Unit => {}
                    VariantDefFields::Tuple(tys) => {
                        for ty in tys {
                            collect_type(ty, out);
                        }
                    }
                    VariantDefFields::Struct(fields) => {
                        for field in fields {
                            out.push(field.id);
                            collect_type(&field.ty, out);
                        }
                    }
                }
            }
        }
        ItemKind::Impl(imp) => {
            for generic in &imp.generics {
                out.push(generic.id);
            }
            collect_type(&imp.self_ty, out);
            for sub in &imp.items {
                collect_item(sub, out);
            }
        }
        ItemKind::Const(c) => {
            collect_type(&c.ty, out);
            collect_expr(&c.value, out);
        }
        ItemKind::Use(u) => collect_path(&u.path, out),
        ItemKind::Mod(module) => collect_module(module, out),
        ItemKind::ModDecl(_) => {}
        ItemKind::Extern(block) => {
            for f in &block.fns {
                out.push(f.id);
                for param in &f.params {
                    out.push(param.id);
                    if let ParamKind::Binding { ty, .. } = &param.kind {
                        collect_type(ty, out);
                    }
                }
                collect_type(&f.ret, out);
            }
        }
    }
}

fn collect_type(ty: &Type, out: &mut Vec<NodeId>) {
    out.push(ty.id);
    match &ty.kind {
        TypeKind::Path(path) => collect_path(path, out),
        TypeKind::Ref { inner, .. } => collect_type(inner, out),
        TypeKind::Tuple(parts) => {
            for part in parts {
                collect_type(part, out);
            }
        }
        TypeKind::Array { elem, len } => {
            collect_type(elem, out);
            collect_expr(len, out);
        }
        TypeKind::Slice(elem) => collect_type(elem, out),
        TypeKind::Unit | TypeKind::Never | TypeKind::Infer | TypeKind::SelfType => {}
    }
}

fn collect_path(path: &Path, out: &mut Vec<NodeId>) {
    out.push(path.id);
    for segment in &path.segments {
        for arg in &segment.args {
            collect_type(arg, out);
        }
    }
}

fn collect_block(block: &Block, out: &mut Vec<NodeId>) {
    out.push(block.id);
    for stmt in &block.stmts {
        out.push(stmt.id);
        match &stmt.kind {
            StmtKind::Let(l) => {
                collect_pat(&l.pat, out);
                if let Some(ty) = &l.ty {
                    collect_type(ty, out);
                }
                if let Some(init) = &l.init {
                    collect_expr(init, out);
                }
            }
            StmtKind::Expr { expr, .. } => collect_expr(expr, out),
        }
    }
    if let Some(tail) = &block.tail {
        collect_expr(tail, out);
    }
}

fn collect_pat(pat: &Pat, out: &mut Vec<NodeId>) {
    out.push(pat.id);
    match &pat.kind {
        PatKind::Wild | PatKind::Binding { .. } | PatKind::Lit(_) => {}
        PatKind::Tuple(pats) => {
            for sub in pats {
                collect_pat(sub, out);
            }
        }
        PatKind::Path(path) => collect_path(path, out),
        PatKind::Variant { path, fields } => {
            collect_path(path, out);
            match fields {
                VariantPatFields::Tuple(pats) => {
                    for sub in pats {
                        collect_pat(sub, out);
                    }
                }
                VariantPatFields::Struct(fields) => {
                    for field in fields {
                        collect_pat(&field.pat, out);
                    }
                }
            }
        }
    }
}

fn collect_expr(expr: &Expr, out: &mut Vec<NodeId>) {
    out.push(expr.id);
    match &expr.kind {
        ExprKind::Unit
        | ExprKind::Int { .. }
        | ExprKind::Float { .. }
        | ExprKind::Bool(_)
        | ExprKind::Char(_)
        | ExprKind::Str(_)
        | ExprKind::SelfValue
        | ExprKind::Break
        | ExprKind::Continue => {}
        ExprKind::Path(path) => collect_path(path, out),
        ExprKind::Call { callee, args } => {
            collect_expr(callee, out);
            for arg in args {
                collect_expr(arg, out);
            }
        }
        ExprKind::MethodCall { receiver, args, .. } => {
            collect_expr(receiver, out);
            for arg in args {
                collect_expr(arg, out);
            }
        }
        ExprKind::Field { base, .. } | ExprKind::TupleIndex { base, .. } => {
            collect_expr(base, out);
        }
        ExprKind::Index { base, index } => {
            collect_expr(base, out);
            collect_expr(index, out);
        }
        ExprKind::Unary { expr, .. } => collect_expr(expr, out),
        ExprKind::Binary { lhs, rhs, .. } => {
            collect_expr(lhs, out);
            collect_expr(rhs, out);
        }
        ExprKind::Assign { target, value } | ExprKind::AssignOp { target, value, .. } => {
            collect_expr(target, out);
            collect_expr(value, out);
        }
        ExprKind::Cast { expr, ty } => {
            collect_expr(expr, out);
            collect_type(ty, out);
        }
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => {
            collect_expr(cond, out);
            collect_block(then_block, out);
            if let Some(branch) = else_branch {
                collect_expr(branch, out);
            }
        }
        ExprKind::Match { scrutinee, arms } => {
            collect_expr(scrutinee, out);
            for arm in arms {
                out.push(arm.id);
                collect_pat(&arm.pat, out);
                collect_expr(&arm.body, out);
            }
        }
        ExprKind::While { cond, body } => {
            collect_expr(cond, out);
            collect_block(body, out);
        }
        ExprKind::Loop { body } => collect_block(body, out),
        ExprKind::For { pat, iter, body } => {
            collect_pat(pat, out);
            collect_expr(iter, out);
            collect_block(body, out);
        }
        ExprKind::Block(block) => collect_block(block, out),
        ExprKind::Unsafe(block) => collect_block(block, out),
        ExprKind::Tuple(items) | ExprKind::Array(items) => {
            for item in items {
                collect_expr(item, out);
            }
        }
        ExprKind::ArrayRepeat { value, len } => {
            collect_expr(value, out);
            collect_expr(len, out);
        }
        ExprKind::StructLit { path, fields } => {
            collect_path(path, out);
            for field in fields {
                collect_expr(&field.value, out);
            }
        }
        ExprKind::Range { start, end, .. } => {
            if let Some(start) = start {
                collect_expr(start, out);
            }
            if let Some(end) = end {
                collect_expr(end, out);
            }
        }
        ExprKind::Try(inner) | ExprKind::Await(inner) => collect_expr(inner, out),
        ExprKind::Return(value) => {
            if let Some(value) = value {
                collect_expr(value, out);
            }
        }
    }
}

#[test]
fn node_ids_are_unique_and_carry_the_file_tag() {
    let program = parse_ok(RICH);
    let mut ids = Vec::new();
    collect_module(&program.root, &mut ids);
    assert!(ids.len() > 80, "expected many nodes, got {}", ids.len());
    let mut seen = BTreeSet::new();
    for id in &ids {
        assert_ne!(*id, 0, "node id 0 is not allowed");
        assert!(seen.insert(*id), "duplicate node id {id}");
        assert_eq!(*id >> 32, 0, "id {id} does not carry file 0");
    }

    let result = parse_file(7, RICH);
    assert!(!result.diags.has_errors(), "{}", dump_diags(&result));
    let mut ids7 = Vec::new();
    collect_module(&result.program.root, &mut ids7);
    let set7: BTreeSet<NodeId> = ids7.iter().copied().collect();
    assert_eq!(set7.len(), ids7.len(), "duplicate node id for file 7");
    for id in &ids7 {
        assert_eq!(*id >> 32, 7, "id {id} does not carry file 7");
        assert!(!seen.contains(id), "file 7 id {id} collides with file 0");
    }
}

// -----------------------------------------------------------------
// Diagnostics and recovery
// -----------------------------------------------------------------

#[test]
fn recovers_after_several_errors_and_keeps_parsing() {
    let src = "trait A {}\nfn bad() { let x = ; }\nstruct S { y: }\nfn last() { }\n";
    let result = parse(src);
    assert!(
        result.diags.error_count() >= 3,
        "expected at least 3 errors, got:\n{}",
        dump_diags(&result)
    );
    let names: Vec<String> = top(&result.program)
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Fn(f) => Some(f.name.name.clone()),
            ItemKind::Struct(s) => Some(s.name.name.clone()),
            _ => None,
        })
        .collect();
    assert!(names.iter().any(|n| n == "bad"), "{names:?}");
    assert!(names.iter().any(|n| n == "S"), "{names:?}");
    assert!(names.iter().any(|n| n == "last"), "{names:?}");
}

#[test]
fn errors_have_stable_codes_and_exact_spans() {
    // (source, diagnostic code, byte offset of the expected span start)
    let cases: &[(&str, &str, u32)] = &[
        ("fn f() {\n", "E-PARSE-UNCLOSED", 7),
        ("fn f( {}\n", "E-PARSE-UNCLOSED", 4),
        ("fn f() -> {}\n", "E-PARSE-EXPECTED", 10),
        ("fn f() { let x = ; }", "E-PARSE-UNEXPECTED", 17),
        ("fn f() { let x: ; }", "E-PARSE-EXPECTED", 16),
        ("fn f() { let x = 1 }", "E-PARSE-EXPECTED", 19),
        ("fn f() { if x }", "E-PARSE-EXPECTED", 14),
        ("fn f() { x. }", "E-PARSE-EXPECTED", 12),
        ("fn f() { match x { _ -> 1 } }", "E-PARSE-EXPECTED", 21),
        ("struct S { x: }", "E-PARSE-EXPECTED", 14),
        ("trait T {}", "E-UNSUPPORTED-TRAIT", 0),
        ("unsafe fn f() {}", "E-UNSUPPORTED-UNSAFE", 0),
        // `async fn` is accepted since D-19-9; `async` before anything else is not.
        ("async struct S;", "E-UNSUPPORTED-ASYNC", 0),
        ("fn f() { async { 1 } }", "E-UNSUPPORTED-ASYNC", 9),
        ("extern \"C\" fn f() {}", "E-UNSUPPORTED-EXTERN", 0),
        ("#[derive(Clone)]\nstruct S;", "E-UNSUPPORTED-ATTR", 0),
        ("fn f<T>() where T: X {}", "E-UNSUPPORTED-WHERE", 10),
        ("fn f<T: Clone>() {}", "E-UNSUPPORTED-BOUNDS", 6),
        ("use foo::*;", "E-UNSUPPORTED-TOKEN", 9),
        ("use foo as bar;", "E-UNSUPPORTED-ALIAS", 8),
        ("pub use foo::bar;", "E-UNSUPPORTED-REEXPORT", 0),
        ("pub(crate) fn f() {}", "E-UNSUPPORTED-VIS", 0),
        ("struct S(T);", "E-UNSUPPORTED-TUPLE-STRUCT", 8),
        ("fn f() { foo!(); }", "E-UNSUPPORTED-MACRO", 9),
        ("fn f() { [1, 2, 3 }", "E-PARSE-UNCLOSED", 9),
    ];
    for (src, code, start) in cases {
        assert_error_at(src, code, *start);
    }
}

#[test]
fn too_large_token_stream_is_reported_without_parsing() {
    let src = "1 ".repeat(super::MAX_TOKENS + 10);
    let result = parse(&src);
    assert!(result
        .diags
        .iter()
        .any(|d| d.code == "E-PARSE-TOO-LARGE" && d.span.start == 0));
    assert!(result.program.root.items.is_empty());
    assert_ne!(result.program.root.id, 0);
}

// -----------------------------------------------------------------
// Hostile inputs (no panic, bounded time)
// -----------------------------------------------------------------

#[test]
fn hostile_100k_tokens_parses_under_two_seconds() {
    let mut src = String::from("fn big() {\n");
    for _ in 0..20_000 {
        src.push_str("let x = 1;\n");
    }
    src.push('}');
    let started = Instant::now();
    let result = parse(&src);
    let elapsed = started.elapsed();
    assert!(!result.diags.has_errors(), "{}", dump_diags(&result));
    let f = as_fn(&top(&result.program)[0]);
    assert_eq!(f.body.stmts.len(), 20_000);
    assert!(
        elapsed.as_secs_f64() < 2.0,
        "100k-token parse took {elapsed:?}"
    );
}

#[test]
fn hostile_deep_nesting_reports_depth_without_panic() {
    let mut src = String::from("fn f() { ");
    src.push_str(&"(".repeat(1_000));
    src.push('1');
    src.push_str(&")".repeat(1_000));
    src.push_str(" }");
    let started = Instant::now();
    let result = parse(&src);
    assert!(
        result.diags.iter().any(|d| d.code == "E-PARSE-DEPTH"),
        "expected E-PARSE-DEPTH, got:\n{}",
        dump_diags(&result)
    );
    assert!(
        started.elapsed().as_secs_f64() < 2.0,
        "deep-nesting parse took {:?}",
        started.elapsed()
    );
}

#[test]
fn hostile_bizarre_inputs_never_panic() {
    let cases: &[&str] = &[
        "\u{0}\u{1}\u{2}",
        "fn f() { \"unterminated",
        "/* unterminated",
        "'",
        "fn é() {}",
        "fn f() { x <- y; }",
        "fn f() { 0x; }",
        "\u{feff}fn f() {}",
        "fn f() { let x = '\\u{d800}'; }",
        "fn\u{0}f",
        "\u{10ffff}",
        "fn f() { 😀 }",
        "fn f() { -",
        "}}}",
        "fn f() { let x = [;]; }",
    ];
    for src in cases {
        let result = parse(src);
        assert_ne!(result.program.root.id, 0);
        // No assertion on diagnostics: some hostile inputs are merely
        // lexically odd. The contract is "no panic, no hang".
    }
}

/// Source of `fn f() { <body> }` where `body` repeats `link` `count` times
/// after `head`.
fn chained(head: &str, link: &str, count: usize, tail: &str) -> String {
    format!("fn f() {{ {head}{}{tail} }}", link.repeat(count))
}

/// Left-associative chains are parsed by loops, so they nest the tree without
/// nesting the parser. Every later phase recurses on that tree: a chain is
/// bounded like any other nesting, and a hostile one is refused, not followed
/// until the stack overflows (100 000 links aborted the compiler).
#[test]
fn hostile_chains_report_depth_without_overflow() {
    let shapes: [(&str, &str, &str); 8] = [
        ("let r = 1", " + 0", ";"),
        ("let r = c", " && c", ";"),
        ("let r = 1", " as i32", ";"),
        ("let r = v", ".a", ";"),
        ("let r = v", "[0]", ";"),
        ("let r = v", ".m()", ";"),
        ("let r = v", "?", ";"),
        ("if c { }", " else if c { }", " else { }"),
    ];
    // 300 links already cross the nesting limit; 20 000 checks that a far
    // longer chain is still refused without a stack overflow. Under Miri
    // (~1000x slower) the long chains take hours, so Miri keeps the 300-link
    // run; the native workspace job runs both.
    let counts: &[usize] = if cfg!(miri) { &[300] } else { &[300, 20_000] };
    for (head, link, tail) in shapes {
        for &count in counts {
            let result = parse(&chained(head, link, count, tail));
            assert!(
                result.diags.iter().any(|d| d.code == "E-PARSE-DEPTH"),
                "`{link}` x {count}: expected E-PARSE-DEPTH, got:\n{}",
                dump_diags(&result)
            );
        }
    }
}

/// The bound is the nesting limit, not a small constant: chains of two
/// hundred links parse cleanly in every shape.
#[test]
fn long_chains_below_the_nesting_limit_parse() {
    let shapes: [(&str, &str, &str); 8] = [
        ("let r = 1", " + 0", ";"),
        ("let r = c", " && c", ";"),
        ("let r = 1", " as i32", ";"),
        ("let r = v", ".a", ";"),
        ("let r = v", "[0]", ";"),
        ("let r = v", ".m()", ";"),
        ("let r = v", "?", ";"),
        ("if c { }", " else if c { }", " else { }"),
    ];
    for (head, link, tail) in shapes {
        let result = parse(&chained(head, link, 200, tail));
        assert!(
            result.diags.is_empty(),
            "`{link}` x 200 must parse, got:\n{}",
            dump_diags(&result)
        );
    }
}

/// A chain nested inside another nesting adds to it: the limit bounds the
/// height of the tree, whatever mix of nesting and chaining builds it.
#[test]
fn a_chain_inside_a_nesting_counts_against_the_same_limit() {
    // 60 parentheses use 180 levels; 60 more links stay under the limit,
    // 120 more exceed it.
    let nested = |links: usize| {
        format!(
            "fn f() {{ let r = {}1{}{}; }}",
            "(".repeat(60),
            " + 0".repeat(links),
            ")".repeat(60)
        )
    };
    assert!(parse(&nested(60)).diags.is_empty());
    let refused = parse(&nested(120));
    assert!(refused.diags.iter().any(|d| d.code == "E-PARSE-DEPTH"));
}

/// G-5.9 (MUST): generic arguments exist in type position only. The turbofish
/// is refused on every segment of an expression or pattern path; it used to be
/// refused on a single-name path only, and silently dropped elsewhere.
#[test]
fn a_turbofish_is_refused_on_every_segment_of_an_expression_path() {
    for src in [
        "fn f() { let v = Vec::<i32>::new(); }",
        "fn f() { let a = m::g::<i64>(1); }",
        "fn f() { let e = E::<i64>::A { v: 1 }; }",
        "fn f() { let o = Option::Some::<i64>(1); }",
        "fn f(e: E<i32>) { match e { E::<i32>::A(x) => { } _ => { } } }",
    ] {
        let codes: Vec<String> = diags_of(src).into_iter().map(|(code, _, _)| code).collect();
        assert_eq!(codes, ["E-PARSE-UNEXPECTED"], "{src}");
    }
    let start = "fn f() { let v = Vec".len() as u32;
    assert_error_at(
        "fn f() { let v = Vec::<i32>::new(); }",
        "E-PARSE-UNEXPECTED",
        start,
    );
    // Type position keeps both spellings.
    parse_ok("fn f(v: Vec<i32>, w: Vec::<i32>) { }");
}

/// G-6.21/G-6.42 (MUST): a block-bodied arm needs no comma, even when the next
/// arm starts with `(` or `-`; the block is not called or subtracted from. An expression arm still needs its comma.
#[test]
fn a_block_arm_ends_before_a_parenthesised_next_arm() {
    for src in [
        "fn f(p: (bool, bool)) { match p { (true, _) => { } (false, x) => { } } }",
        "fn f(n: i32) { match n { 1 => if true { } else { } -1 => { } _ => { } } }",
        "fn f(n: i32) { match n { 1 => match n { _ => { } } (2) => { } _ => loop { } } }",
    ] {
        parse_ok(src);
    }
    let codes: Vec<String> = diags_of("fn f(n: i32) { let x = match n { 1 => 2 3 => 4 }; }")
        .into_iter()
        .map(|(code, _, _)| code)
        .collect();
    assert!(codes.contains(&"E-PARSE-EXPECTED".to_string()), "{codes:?}");
}

/// G-6.1: a block-like expression at the head of a statement ends at its `}`.
#[test]
fn a_block_like_statement_head_ends_at_its_brace() {
    for (src, head) in [
        ("fn f(c: bool) { if c { } (1, 2); }", "if"),
        ("fn f(c: bool) { match c { _ => { } } (3, 4); }", "match"),
        ("fn f() { { } (5, 6); }", "block"),
        ("fn f() { { } [1][0]; }", "block"),
        ("fn f(c: bool) { if c { } - 1; }", "if"),
    ] {
        let program = parse_ok(src);
        let body = &as_fn(&top(&program)[0]).body;
        assert_eq!(body.stmts.len(), 2, "{src}");
        let (first, semi) = stmt_expr(&body.stmts[0]);
        assert!(!semi, "{src}");
        let kind_ok = match head {
            "if" => matches!(first.kind, ExprKind::If { .. }),
            "match" => matches!(first.kind, ExprKind::Match { .. }),
            _ => matches!(first.kind, ExprKind::Block(_)),
        };
        assert!(kind_ok, "{src}: {:?}", first.kind);
        let (_, semi) = stmt_expr(&body.stmts[1]);
        assert!(semi, "{src}");
    }
}

/// G-6.1: `.` and `?` still continue a block-like head; a prefix operator or
/// a non-head position keeps the whole expression.
#[test]
fn a_block_like_head_continues_with_a_method_call_or_inside_an_expression() {
    for src in [
        "fn f(c: bool) { match c { _ => 1 } .max(2); }",
        "fn f() { -{ 1 } + 1; }",
        "fn f() { let x = { 1 } + 2; }",
        "fn f() -> i32 { 1 + { 2 } * 3 }",
    ] {
        let program = parse_ok(src);
        let body = &as_fn(&top(&program)[0]).body;
        assert_eq!(
            body.stmts.len() + usize::from(body.tail.is_some()),
            1,
            "{src}"
        );
    }
}
