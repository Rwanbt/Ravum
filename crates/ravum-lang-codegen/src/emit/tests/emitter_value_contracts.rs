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
