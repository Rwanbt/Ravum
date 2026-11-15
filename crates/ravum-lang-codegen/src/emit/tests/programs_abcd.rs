use super::*;

// -- program A: functions, struct, impl, builtins, Result main -----------

pub(super) struct ProgramA {
    pub(super) checked: CheckedProgram,
    pub(super) println_call_id: NodeId,
    pub(super) point_id: NodeId,
    pub(super) sum_id: NodeId,
}

pub(super) fn build_program_a() -> ProgramA {
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

pub(super) fn build_program_b() -> CheckedProgram {
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

pub(super) fn build_program_c() -> CheckedProgram {
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

pub(super) fn build_program_d() -> CheckedProgram {
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
