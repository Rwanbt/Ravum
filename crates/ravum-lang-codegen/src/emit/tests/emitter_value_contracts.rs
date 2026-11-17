use super::*;

#[test]
fn value_paths_reject_invalid_resolution_facts() {
    let mut builder = Builder::new();
    let path = builder.path(&["value"]);
    let mut program = build_program_e();
    let options = GenOptions::default();
    let cases = [
        (Some(Resolution::Module), "used as a value"),
        (Some(Resolution::BuiltinModule), "used as a value"),
        (Some(Resolution::Error), "is unresolved"),
        (
            Some(Resolution::BuiltinFn {
                path: "std::io::println".to_string(),
            }),
            "builtin function `std::io::println` must be called",
        ),
        (
            Some(Resolution::BuiltinConst {
                path: "unknown::constant".to_string(),
            }),
            "unsupported builtin constant",
        ),
        (None, "missing resolution fact"),
    ];
    for (resolution, expected) in cases {
        if let Some(resolution) = resolution {
            program
                .checked
                .facts
                .resolutions
                .insert(path.id, resolution);
        } else {
            program.checked.facts.resolutions.remove(&path.id);
        }
        let emitter = Emitter {
            checked: &program.checked,
            options: &options,
            index: Index::build(&program.checked),
            array_moves: Default::default(),
            shadowed_names: Default::default(),
        };
        let error = emitter
            .value_path(&path)
            .expect_err("invalid facts must not produce a Rust path");
        assert!(error.message.contains(expected), "{}", error.message);
    }
}

#[test]
fn assertion_and_panic_lowering_refuses_invalid_checked_arities() {
    let mut builder = Builder::new();
    let call = builder.unit();
    let arguments = [builder.text("message"), builder.text("extra")];
    let checked = builder.finish(None, vec![]);
    let options = GenOptions::default();
    let emitter = Emitter {
        checked: &checked,
        options: &options,
        index: Index::build(&checked),
        array_moves: Default::default(),
        shadowed_names: Default::default(),
    };

    for (builtin, arity, expected) in [
        ("panic", 0, 1),
        ("panic", 2, 1),
        ("assert", 0, 1),
        ("assert", 2, 1),
        ("assert_eq", 1, 2),
    ] {
        let error = emitter
            .builtin_free_call(&call, builtin, &arguments[..arity], 0)
            .unwrap_err();
        assert_eq!(
            error.message,
            format!("builtin `{builtin}` expects {expected} argument(s), got {arity}")
        );
    }
}

#[test]
fn generation_rejects_a_file_module_that_survived_loading() {
    let mut builder = Builder::new();
    let declaration = Item {
        id: builder.id(),
        vis: Vis::Private,
        kind: ItemKind::ModDecl(ravum_lang_syntax::ast::ModDecl {
            name: builder.ident("inner"),
        }),
        span: span(),
    };
    builder.items.push(declaration);
    let checked = builder.finish(None, vec![]);
    let error = crate::generate(&checked, &GenOptions::default()).unwrap_err();
    assert!(
        error
            .message
            .contains("unresolved `mod inner;` declaration"),
        "{}",
        error.message
    );
}

#[test]
fn constant_lowering_checks_the_value_type_and_missing_facts() {
    let mut builder = Builder::new();
    let integer = builder.int(255, Some(STy::U8));
    let string = builder.text("constant text");
    let boolean = builder.expr(ExprKind::Bool(false), Some(STy::Bool));
    let character = builder.expr(ExprKind::Char('\t'), Some(STy::Char));
    let unit = builder.unit();
    let checked = builder.finish(None, vec![]);
    let options = GenOptions::default();
    let emitter = Emitter {
        checked: &checked,
        options: &options,
        index: Index::build(&checked),
        array_moves: Default::default(),
        shadowed_names: Default::default(),
    };

    assert_eq!(
        emitter
            .const_value(&STy::Bool, &ConstValue::Bool(false))
            .unwrap(),
        "false"
    );
    let mismatch = emitter
        .const_value(&STy::Bool, &ConstValue::Int(1))
        .unwrap_err();
    assert!(mismatch.message.contains("non-integer type"));
    let owned = emitter
        .const_value(&STy::String, &ConstValue::Str("text".into()))
        .unwrap_err();
    assert!(owned.message.contains("declare the constant as `&str`"));

    for (expr, expected) in [
        (&integer, "255u8"),
        (&string, r#""constant text""#),
        (&boolean, "false"),
        (&character, "'\\t'"),
    ] {
        assert_eq!(emitter.literal_const_expr(expr).unwrap(), expected);
    }
    let missing = emitter.literal_const_expr(&unit).unwrap_err();
    assert!(missing.message.contains("missing `Facts::consts` entry"));
}
