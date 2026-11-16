use super::*;

#[test]
fn type_fallback_preserves_rust_spelling_without_resolution_facts() {
    let mut builder = Builder::new();
    let text = builder.named_type("str", None);
    let borrowed = builder.ty(
        TypeKind::Ref {
            mutable: false,
            inner: Box::new(text),
        },
        None,
    );
    let scalar = builder.named_type("i32", Some(STy::Error));
    let mutable = builder.ty(
        TypeKind::Ref {
            mutable: true,
            inner: Box::new(scalar.clone()),
        },
        None,
    );
    let shared = builder.ty(
        TypeKind::Ref {
            mutable: false,
            inner: Box::new(scalar.clone()),
        },
        None,
    );
    let singleton = builder.ty(TypeKind::Tuple(vec![borrowed.clone()]), None);
    let pair = builder.ty(
        TypeKind::Tuple(vec![borrowed.clone(), scalar.clone()]),
        None,
    );
    let length = builder.int(3, None);
    let array = builder.ty(
        TypeKind::Array {
            elem: Box::new(scalar.clone()),
            len: Box::new(length),
        },
        None,
    );
    let slice = builder.ty(TypeKind::Slice(Box::new(scalar.clone())), None);
    let mut option_path = builder.path(&["Option"]);
    option_path.segments[0].args.push(borrowed.clone());
    let option = builder.ty(TypeKind::Path(option_path), None);
    let unit = builder.ty(TypeKind::Unit, None);
    let never = builder.ty(TypeKind::Never, None);
    let this = builder.ty(TypeKind::SelfType, None);
    let program = builder.finish(None, vec![]);
    let options = GenOptions::default();
    let emitter = Emitter {
        checked: &program,
        options: &options,
        index: Index::build(&program),
        array_moves: Default::default(),
    };

    for (ty, position, expected) in [
        (&scalar, TypePos::Value, "i32"),
        (&borrowed, TypePos::Param, "&str"),
        (&borrowed, TypePos::Value, "&'static str"),
        (&mutable, TypePos::Param, "&mut i32"),
        (&shared, TypePos::Param, "&i32"),
        (&singleton, TypePos::Param, "(&'static str,)"),
        (&pair, TypePos::Value, "(&'static str, i32)"),
        (&array, TypePos::Value, "[i32; 3]"),
        (&slice, TypePos::Value, "[i32]"),
        (&option, TypePos::Param, "Option<&'static str>"),
        (&unit, TypePos::Value, "()"),
        (&never, TypePos::Value, "!"),
        (&this, TypePos::Value, "Self"),
    ] {
        assert_eq!(emitter.type_node_pos(ty, position).unwrap(), expected);
    }
}

#[test]
fn type_emitter_refuses_invalid_fallback_and_recovered_semantic_types() {
    let mut builder = Builder::new();
    let mut rooted = builder.path(&["Thing"]);
    rooted.root = PathRoot::Crate;
    let rooted = builder.ty(TypeKind::Path(rooted), None);
    let qualified = builder.path(&["module", "Thing"]);
    let qualified = builder.ty(TypeKind::Path(qualified), None);
    let inferred = builder.ty(TypeKind::Infer, None);
    let elem = builder.named_type("i32", None);
    let length = builder.text("bad");
    let array = builder.ty(
        TypeKind::Array {
            elem: Box::new(elem),
            len: Box::new(length),
        },
        None,
    );
    let program = builder.finish(None, vec![]);
    let options = GenOptions::default();
    let emitter = Emitter {
        checked: &program,
        options: &options,
        index: Index::build(&program),
        array_moves: Default::default(),
    };

    for (ty, expected) in [
        (&rooted, "rooted type path"),
        (&qualified, "qualified type path"),
        (&inferred, "missing resolved type for inferred"),
        (
            &array,
            "array length must be a literal integer or a constant",
        ),
    ] {
        let error = emitter.type_node(ty).unwrap_err();
        assert!(error.message.contains(expected), "{}", error.message);
    }

    for (ty, expected) in [
        (STy::Error, "recovered error type"),
        (
            STy::Ref {
                mutable: true,
                inner: Box::new(STy::Str),
            },
            "has no Core 1.0 representation",
        ),
        (
            STy::Nominal {
                def: NodeId::MAX,
                name: "Missing".into(),
                args: vec![],
            },
            "cannot resolve nominal type",
        ),
    ] {
        let error = emitter.fmt_type(&ty).unwrap_err();
        assert!(error.message.contains(expected), "{}", error.message);
    }
}

#[test]
fn semantic_nominals_use_the_resolved_rust_path_and_render_generic_arguments() {
    let mut builder = Builder::new();
    let id = builder.id();
    builder
        .facts
        .item_paths
        .insert(id, "crate::Vector".to_string());
    let program = builder.finish(None, vec![]);
    let options = GenOptions::default();
    let emitter = Emitter {
        checked: &program,
        options: &options,
        index: Index::build(&program),
        array_moves: Default::default(),
    };
    let vector = STy::Nominal {
        def: id,
        name: "Vector".into(),
        args: vec![STy::I32, STy::Bool],
    };
    assert_eq!(
        emitter.fmt_type(&vector).unwrap(),
        "crate::rvm_root::Vector<i32, bool>"
    );
    let error = STy::Nominal {
        def: NodeId::MAX,
        name: "Error".into(),
        args: vec![],
    };
    assert_eq!(emitter.fmt_type(&error).unwrap(), "crate::Error");
}
