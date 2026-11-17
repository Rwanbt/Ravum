use super::*;

#[test]
fn literal_and_singleton_patterns_keep_rust_spelling() {
    let mut builder = Builder::new();
    let signed = builder.pat(
        PatKind::Lit(LitPat::Int {
            value: 128,
            suffix: Some(IntTy::I8),
            neg: true,
        }),
        None,
    );
    let signed = builder.pat(PatKind::Tuple(vec![signed]), None);
    let character = builder.pat(PatKind::Lit(LitPat::Char('\n')), None);
    let string = builder.pat(PatKind::Lit(LitPat::Str("quoted \"text\"".into())), None);
    let float = builder.pat(
        PatKind::Lit(LitPat::Float {
            text: "1.5".into(),
            suffix: None,
        }),
        None,
    );
    let program = builder.finish(None, vec![]);
    let options = GenOptions::default();
    let emitter = Emitter {
        checked: &program,
        options: &options,
        index: Index::build(&program),
        array_moves: Default::default(),
        shadowed_names: Default::default(),
    };

    assert_eq!(emitter.pattern(&signed).unwrap(), "(-128i8,)");
    assert_eq!(emitter.pattern(&character).unwrap(), "'\\n'");
    assert_eq!(emitter.pattern(&string).unwrap(), r#""quoted \"text\"""#);
    let error = emitter.pattern(&float).unwrap_err();
    assert!(error.to_string().contains("only supported in `match` arms"));
}

#[test]
fn variant_rendering_preserves_empty_shapes_and_requires_resolution() {
    let mut builder = Builder::new();
    let path = builder.path(&["Shape", "Empty"]);
    builder
        .facts
        .resolutions
        .insert(path.id, Resolution::Module);
    let unit = builder.pat(PatKind::Path(path.clone()), None);
    let tuple = builder.pat(
        PatKind::Variant {
            path: path.clone(),
            fields: VariantPatFields::Tuple(vec![]),
        },
        None,
    );
    let record = builder.pat(
        PatKind::Variant {
            path,
            fields: VariantPatFields::Struct(vec![]),
        },
        None,
    );
    let unknown = builder.path(&["Shape", "Unknown"]);
    let unresolved_unit = builder.pat(PatKind::Path(unknown.clone()), None);
    let unresolved_tuple = builder.pat(
        PatKind::Variant {
            path: unknown,
            fields: VariantPatFields::Tuple(vec![]),
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
        shadowed_names: Default::default(),
    };

    assert_eq!(emitter.pattern(&unit).unwrap(), "Shape::Empty");
    assert_eq!(emitter.pattern(&tuple).unwrap(), "Shape::Empty()");
    assert_eq!(emitter.pattern(&record).unwrap(), "Shape::Empty {}");
    for unresolved in [unresolved_unit, unresolved_tuple] {
        let error = emitter.pattern(&unresolved).unwrap_err();
        assert!(error
            .to_string()
            .contains("missing resolution fact for pattern path `Shape::Unknown`"));
    }
}
