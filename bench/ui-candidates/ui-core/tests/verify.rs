use std::collections::BTreeMap;
use ui_core::responsive::Input;
use ui_core::tokens::{Tokens, Value};
use ui_core::verify::*;
use ui_core::*;

fn fixed(w: f32, h: f32) -> Style {
    Style {
        width: Len::Fixed(w),
        height: Len::Fixed(h),
        ..Style::default()
    }
}

fn row(kids: Vec<Desc>) -> Desc {
    Desc::new(
        1,
        Style {
            dir: Dir::Row,
            ..fixed(200.0, 50.0)
        },
    )
    .with(kids)
}

fn pointer() -> Context<'static> {
    Context {
        tokens: None,
        input: Input::Pointer,
    }
}

fn rules_of(d: Desc, ctx: &Context) -> Vec<&'static str> {
    let ui = Ui::new(d, (200.0, 50.0)).unwrap();
    run(&ui, ctx)
        .unwrap()
        .findings
        .iter()
        .map(|f| f.rule)
        .collect()
}

#[test]
fn ruleset_lock_matches() {
    assert_eq!(
        fingerprint(RULES),
        RULESET_LOCK,
        "update RULESET_LOCK to {:#018x} in the same change",
        fingerprint(RULES)
    );
}

#[test]
fn a_silently_weakened_rule_is_refused() {
    let ui = Ui::new(row(vec![]), (200.0, 50.0)).unwrap();
    let mut weaker = RULES.to_vec();
    weaker[0].weight = 0;
    assert!(matches!(
        run_with(&ui, &pointer(), &weaker, RULESET_LOCK),
        Err(VerifyError::RulesetTampered { .. })
    ));
    let mut downgraded = RULES.to_vec();
    downgraded[6].severity = Severity::Warning;
    assert!(run_with(&ui, &pointer(), &downgraded, RULESET_LOCK).is_err());
    let fewer = &RULES[1..];
    assert!(run_with(&ui, &pointer(), fewer, RULESET_LOCK).is_err());
}

/// Un test positif (rien à signaler) et un négatif (la règle se déclenche)
/// pour chaque règle du ruleset.
#[test]
fn every_rule_has_a_positive_and_a_negative_case() {
    let charter = Tokens {
        base: BTreeMap::from([
            ("s".into(), Value::Dimension(4.0)),
            ("w".into(), Value::Dimension(40.0)),
            ("h".into(), Value::Dimension(50.0)),
            ("row".into(), Value::Dimension(200.0)),
        ]),
        themes: BTreeMap::new(),
    };
    let ptr = pointer();
    let touch = Context {
        tokens: None,
        input: Input::Touch,
    };
    let design = Context {
        tokens: Some(&charter),
        input: Input::Pointer,
    };
    let ok = || {
        row(vec![
            Desc::new(2, fixed(40.0, 50.0)).with(vec![Desc::text(1, "ok")])
        ])
    };
    let cases: Vec<(&str, Desc, &Context, Desc)> = vec![
        (
            "layout.overflow",
            ok(),
            &ptr,
            row(vec![
                Desc::new(2, fixed(8.0, 50.0)).with(vec![Desc::text(1, "too long")])
            ]),
        ),
        (
            "layout.out-of-container",
            ok(),
            &ptr,
            row(vec![
                Desc::new(2, fixed(150.0, 50.0)),
                Desc::new(3, fixed(150.0, 50.0)),
            ]),
        ),
        (
            "layout.overlap",
            ok(),
            &ptr,
            row(vec![
                Desc::new(2, fixed(40.0, 50.0)),
                Desc::new(
                    3,
                    Style {
                        offset: (-20.0, 0.0),
                        ..fixed(40.0, 50.0)
                    },
                ),
            ]),
        ),
        (
            "layout.hidden-content",
            ok(),
            &ptr,
            row(vec![
                Desc::new(2, fixed(0.0, 50.0)).with(vec![Desc::text(1, "secret")])
            ]),
        ),
        (
            "layout.off-viewport",
            ok(),
            &ptr,
            row(vec![Desc::new(
                2,
                Style {
                    offset: (0.0, 900.0),
                    ..fixed(1.0, 1.0)
                },
            )]),
        ),
        (
            "design.off-charter",
            Desc::new(
                1,
                Style {
                    dir: Dir::Row,
                    padding: 4.0,
                    ..fixed(200.0, 50.0)
                },
            ),
            &design,
            Desc::new(
                1,
                Style {
                    dir: Dir::Row,
                    padding: 5.0,
                    ..fixed(200.0, 50.0)
                },
            ),
        ),
        (
            "a11y.touch-target",
            row(vec![Desc::new(
                2,
                Style {
                    focusable: true,
                    ..fixed(48.0, 48.0)
                },
            )]),
            &touch,
            row(vec![Desc::new(
                2,
                Style {
                    focusable: true,
                    ..fixed(20.0, 20.0)
                },
            )]),
        ),
        (
            "a11y.empty-text",
            ok(),
            &ptr,
            row(vec![
                Desc::new(2, fixed(40.0, 50.0)).with(vec![Desc::text(1, "")])
            ]),
        ),
    ];
    let covered: Vec<&str> = cases.iter().map(|c| c.0).collect();
    assert_eq!(
        covered,
        RULES.iter().map(|r| r.id).collect::<Vec<_>>(),
        "one case per registered rule"
    );
    for (rule, good, ctx, bad) in cases {
        let g = rules_of(good, ctx);
        assert!(
            !g.contains(&rule),
            "{rule} fired on the positive case: {g:?}"
        );
        let b = rules_of(bad, ctx);
        assert!(b.contains(&rule), "{rule} missed: {b:?}");
    }
}

#[test]
fn scores_gate_and_reports_are_deterministic() {
    let clean = Ui::new(
        row(vec![
            Desc::new(2, fixed(40.0, 50.0)).with(vec![Desc::text(1, "ok")])
        ]),
        (200.0, 50.0),
    )
    .unwrap();
    let broken = Ui::new(
        row(vec![
            Desc::new(2, fixed(150.0, 50.0)),
            Desc::new(3, fixed(150.0, 50.0)).with(vec![Desc::text(1, "")]),
        ]),
        (200.0, 50.0),
    )
    .unwrap();
    let a = run(&clean, &pointer()).unwrap();
    let b = run(&broken, &pointer()).unwrap();
    assert_eq!(a.total, 100);
    assert!(b.total < 100);
    assert_eq!(gate(&a, &a), Ok(()));
    let blocked = gate(&a, &b).unwrap_err();
    // Seules les erreurs bloquent ; l'avertissement « texte vide » non.
    assert!(blocked.iter().all(|f| f.rule != "a11y.empty-text") && !blocked.is_empty());
    assert_eq!(gate(&b, &b), Ok(()), "existing findings do not block");

    let (j, s) = (to_json(&b), to_sarif(&b));
    assert!(
        j.starts_with("{\"ruleset\":\"")
            && j.contains("\"rule\":\"layout.out-of-container\",\"path\":\"1/3\""),
        "{j}"
    );
    assert!(
        s.contains("\"version\":\"2.1.0\"")
            && s.contains("\"ruleId\":\"a11y.empty-text\",\"level\":\"warning\""),
        "{s}"
    );
    assert_eq!(s.matches("\"defaultConfiguration\"").count(), RULES.len());
    for _ in 0..20 {
        let r = run(&broken, &pointer()).unwrap();
        assert_eq!((to_json(&r), to_sarif(&r)), (j.clone(), s.clone()));
    }
}
