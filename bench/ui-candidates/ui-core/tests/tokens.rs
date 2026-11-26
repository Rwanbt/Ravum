use std::collections::BTreeMap;
use ui_core::tokens::*;
use ui_core::*;

fn charter() -> Tokens {
    let base = BTreeMap::from([
        ("space.s".to_string(), Value::Dimension(4.0)),
        ("space.m".to_string(), Value::Dimension(8.0)),
        ("size.row".to_string(), Value::Dimension(24.0)),
        ("size.panel".to_string(), Value::Dimension(200.0)),
        (
            "color.brand".to_string(),
            Value::Color([0x20, 0x60, 0xe0, 0xff]),
        ),
        (
            "color.primary".to_string(),
            Value::Ref("color.brand".into()),
        ),
        ("panel.padding".to_string(), Value::Ref("space.m".into())),
    ]);
    let dense = BTreeMap::from([("panel.padding".to_string(), Value::Ref("space.s".into()))]);
    let dark = BTreeMap::from([(
        "color.brand".to_string(),
        Value::Color([0x80, 0xa0, 0xff, 0xff]),
    )]);
    Tokens {
        base,
        themes: BTreeMap::from([("dense".into(), dense), ("dark".into(), dark)]),
    }
}

#[test]
fn references_and_themes_resolve() {
    let t = charter();
    t.check().unwrap();
    assert_eq!(t.color("color.primary", None), Ok([0x20, 0x60, 0xe0, 0xff]));
    assert_eq!(
        t.color("color.primary", Some("dark")),
        Ok([0x80, 0xa0, 0xff, 0xff])
    );
    assert_eq!(t.dimension("panel.padding", None), Ok(8.0));
    assert_eq!(t.dimension("panel.padding", Some("dense")), Ok(4.0));
}

#[test]
fn broken_charters_are_refused() {
    let mut t = charter();
    t.base.insert("a".into(), Value::Ref("b".into()));
    t.base.insert("b".into(), Value::Ref("a".into()));
    assert_eq!(
        t.resolve("a", None),
        Err(TokenError::Cycle(vec!["a".into(), "b".into(), "a".into()]))
    );
    assert!(t.check().is_err());

    let mut t = charter();
    t.base.insert("x".into(), Value::Ref("nope".into()));
    assert_eq!(t.check(), Err(TokenError::Unknown("nope".into())));
    assert_eq!(
        charter().dimension("color.primary", None),
        Err(TokenError::WrongKind {
            token: "color.primary".into(),
            expected: Kind::Dimension
        })
    );
    // Un thème peut casser une référence : refusé aussi.
    let mut t = charter();
    t.themes
        .get_mut("dark")
        .unwrap()
        .insert("space.m".into(), Value::Ref("missing".into()));
    assert!(t.check().is_err());
}

fn panel_style() -> ComponentStyle {
    ComponentStyle {
        props: BTreeMap::from([
            ("padding".into(), "panel.padding".into()),
            ("height".into(), "size.row".into()),
            ("width".into(), "size.panel".into()),
        ]),
        variants: BTreeMap::from([(
            "roomy".into(),
            BTreeMap::from([("padding".into(), "space.m".into())]),
        )]),
    }
}

/// Critère d'U5 (partie logique) : un composant de plugin hérite de la
/// charte et du thème de l'hôte ; les écarts sont détectés.
#[test]
fn a_plugin_component_inherits_the_charter_and_deviations_are_flagged() {
    let t = charter();
    let c = panel_style();
    let s = c.apply(&t, None, None, Style::default()).unwrap();
    assert_eq!(
        (s.padding, s.height, s.width),
        (8.0, Len::Fixed(24.0), Len::Fixed(200.0))
    );
    let dense = c.apply(&t, Some("dense"), None, Style::default()).unwrap();
    assert_eq!(dense.padding, 4.0);
    let roomy = c
        .apply(&t, Some("dense"), Some("roomy"), Style::default())
        .unwrap();
    assert_eq!(roomy.padding, 8.0);

    let good = Desc::new(1, s.clone()).with(vec![Desc::new(2, dense.clone())]);
    assert!(audit(&good, &t).is_empty());
    let mut off = s;
    off.padding = 7.0;
    let bad = Desc::new(1, off).with(vec![Desc::new(2, Style { gap: 3.0, ..dense })]);
    let d = audit(&bad, &t);
    assert_eq!(
        d.iter()
            .map(|d| (d.path.clone(), d.property))
            .collect::<Vec<_>>(),
        vec![(vec![1], "padding"), (vec![1, 2], "gap")]
    );
    assert_eq!(audit(&bad, &t), d, "deterministic");
    let unknown = ComponentStyle {
        props: BTreeMap::from([("margin".into(), "space.s".into())]),
        ..Default::default()
    };
    assert!(unknown.apply(&t, None, None, Style::default()).is_err());
}

#[test]
fn dtcg_round_trip_and_refusals() {
    let t = charter();
    let json = export(&t);
    assert!(
        json.contains("\"color.brand\":{\"$type\":\"color\",\"$value\":\"#2060e0ff\"}"),
        "{json}"
    );
    assert!(json.contains("\"panel.padding\":{\"$value\":\"{space.m}\"}"));
    let back = import(&json).unwrap();
    assert_eq!(back, t);
    assert_eq!(export(&back), json);
    // Espaces et couleur sur 6 chiffres acceptés.
    let ok =
        import(" { \"base\" : { \"c\" : { \"$type\" : \"color\" , \"$value\" : \"#ffffff\" } } } ")
            .unwrap();
    assert_eq!(ok.color("c", None), Ok([255, 255, 255, 255]));

    for bad in [
        "",
        "{",
        "{\"base\":{}} x",
        "{\"other\":{}}",
        "{\"base\":{\"a\":{\"$type\":\"color\",\"$value\":\"red\"}}}",
        "{\"base\":{\"a\":{\"$type\":\"dimension\",\"$value\":\"-4px\"}}}",
        "{\"base\":{\"a\":{\"$type\":\"dimension\",\"$value\":\"NaNpx\"}}}",
        "{\"base\":{\"a\":{\"$type\":\"shadow\",\"$value\":\"x\"}}}",
        "{\"base\":{\"a\":{\"$type\":\"dimension\"}}}",
        "{\"base\":{\"a\":{\"$value\":\"{b}\"}}}",
        "{\"base\":{\"a\":{\"$value\":\"{a}\"}}}",
        "{\"base\":{\"a\":{\"$type\":\"dimension\",\"$value\":\"1px\"},\"a\":{\"$type\":\"dimension\",\"$value\":\"2px\"}}}",
        "{\"base\":{\"a\\\"\":{}}}",
    ] {
        assert!(import(bad).is_err(), "accepted: {bad}");
    }
    let deep = format!("{}{}", "{\"base\":".repeat(20), "}".repeat(20));
    assert!(import(&deep).is_err());
}
