use super::*;

pub(super) struct ProgramE {
    pub(super) checked: CheckedProgram,
    pub(super) shape_id: NodeId,
    pub(super) area_id: NodeId,
    pub(super) max_id: NodeId,
    pub(super) sign_id: NodeId,
    pub(super) sum_to_id: NodeId,
    pub(super) total_id: NodeId,
    pub(super) cast_round_id: NodeId,
    pub(super) greeting_id: NodeId,
    pub(super) countdown_id: NodeId,
    pub(super) type_id: NodeId,
    pub(super) parse_number_id: NodeId,
    pub(super) twice_id: NodeId,
}

pub(super) fn build_program_e() -> ProgramE {
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
        // The checker records a prelude constructor as a builtin call.
        CallTarget::BuiltinFn {
            path: "Result::Ok".to_string(),
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
            // The checker records a prelude constructor as a builtin call.
            CallTarget::BuiltinFn {
                path: "Option::Some".to_string(),
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
            // The checker resolves `None` to the builtin constant.
            Resolution::BuiltinConst {
                path: "Option::None".to_string(),
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
        // The checker records a prelude constructor as a builtin call.
        CallTarget::BuiltinFn {
            path: "Result::Ok".to_string(),
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
pub(super) fn broad_program_compiles_runs_and_keeps_semantics() {
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
    let expected = "12\n6\n0\n7\n6\n6\n3\nHI\n0\n10\n-1\n2\n4\n3\ntrue\ntrue\n42\nsay \"hi\"\n10\n";
    assert_eq!(stdout, expected, "stderr: {stderr}\n--- code ---\n{code}");
    assert_eq!(status, 0, "stderr: {stderr}");
}
