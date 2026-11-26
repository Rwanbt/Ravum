use ui_core::responsive::*;
use ui_core::*;

fn bp() -> Breakpoints {
    Breakpoints(vec![(0.0, "compact"), (600.0, "medium"), (1000.0, "wide")])
}

fn button(key: u64, p: &Profile) -> Desc {
    let h = if p.input == Input::Touch { 48.0 } else { 28.0 };
    Desc::new(
        key,
        Style {
            width: Len::Fixed(96.0),
            height: Len::Fixed(h),
            focusable: true,
            ..Style::default()
        },
    )
    .with(vec![Desc::text(1, "save")])
}

/// Vue correcte : zones sûres réservées, colonne en compact, barre latérale
/// au-delà.
fn good(p: &Profile, bp: &str) -> Desc {
    view(p, bp, true)
}

/// Vue fautive : barre latérale fixe à toutes les largeurs, sans zones sûres.
fn bad(p: &Profile, bp: &str) -> Desc {
    let _ = bp;
    view(p, "wide", false)
}

fn view(p: &Profile, bp: &str, safe: bool) -> Desc {
    let [t, _, b, _] = if safe { p.safe } else { [0.0; 4] };
    let spacer = |k: u64, h: f32| {
        Desc::new(
            k,
            Style {
                width: Len::Fixed(0.0),
                height: Len::Fixed(h),
                ..Style::default()
            },
        )
    };
    let content = match bp {
        "compact" => Desc::new(
            10,
            Style {
                dir: Dir::Column,
                width: Len::Fill(1.0),
                height: Len::Fill(1.0),
                padding: 8.0,
                gap: 8.0,
                ..Style::default()
            },
        )
        .with(vec![
            button(11, p),
            Desc::new(
                12,
                Style {
                    width: Len::Fill(1.0),
                    height: Len::Fill(1.0),
                    ..Style::default()
                },
            ),
        ]),
        _ => Desc::new(
            10,
            Style {
                dir: Dir::Row,
                width: Len::Fill(1.0),
                height: Len::Fill(1.0),
                padding: 8.0,
                gap: 8.0,
                ..Style::default()
            },
        )
        .with(vec![
            Desc::new(
                13,
                Style {
                    dir: Dir::Column,
                    width: Len::Fixed(if bp == "wide" { 480.0 } else { 240.0 }),
                    height: Len::Fill(1.0),
                    ..Style::default()
                },
            )
            .with(vec![button(11, p)]),
            Desc::new(
                12,
                Style {
                    width: Len::Fill(1.0),
                    height: Len::Fill(1.0),
                    ..Style::default()
                },
            ),
        ]),
    };
    let mut kids = vec![];
    if t > 0.0 {
        kids.push(spacer(2, t));
    }
    kids.push(content);
    if b > 0.0 {
        kids.push(spacer(3, b));
    }
    Desc::new(
        1,
        Style {
            dir: Dir::Column,
            width: Len::Fill(1.0),
            height: Len::Fill(1.0),
            ..Style::default()
        },
    )
    .with(kids)
}

#[test]
fn breakpoints_select_by_width() {
    let b = bp();
    assert_eq!(
        (b.at(390.0), b.at(599.0), b.at(600.0), b.at(1280.0)),
        ("compact", "compact", "medium", "wide")
    );
}

#[test]
fn strict_gates_pass_on_every_profile() {
    for p in [desktop(), tablet(), phone()] {
        assert_eq!(check(&good, &bp(), &p), vec![], "{}", p.name);
    }
}

#[test]
fn gates_catch_safe_area_touch_and_overflow() {
    let phone = phone();
    let f = check(&bad, &bp(), &phone);
    assert!(
        f.iter()
            .any(|f| matches!(f, Failure::OutsideSafeArea(p) if p == &vec![1, 10])),
        "{f:?}"
    );
    assert!(f.iter().any(|f| matches!(f, Failure::Verify(_))), "{f:?}");
    // Cible tactile trop petite : la vue « pointeur » sur un écran tactile.
    let pointer_view = |p: &Profile, b: &str| {
        good(
            &Profile {
                input: Input::Pointer,
                ..p.clone()
            },
            b,
        )
    };
    let f = check(&pointer_view, &bp(), &tablet());
    assert_eq!(f, vec![Failure::TouchTargetTooSmall(vec![1, 10, 13, 11])]);
    // Sur PC, la même vue passe.
    assert!(check(&pointer_view, &bp(), &desktop()).is_empty());
}

#[test]
fn bounded_sweep_publishes_coverage_and_score() {
    let s = sweep(&good, &bp(), &phone(), 320.0, 1400.0, 40.0);
    assert!(
        s.widths.contains(&599.0)
            && s.widths.contains(&600.0)
            && s.widths.contains(&601.0)
            && s.widths.contains(&1000.0)
    );
    assert!(s.widths.windows(2).all(|w| w[0] < w[1]));
    assert_eq!(s.failing, vec![]);
    assert_eq!(s.score, 1.0);

    // Zones sûres respectées, mais barre latérale fixe à toutes les largeurs.
    let rigid = |p: &Profile, _: &str| view(p, "wide", true);
    let s = sweep(&rigid, &bp(), &phone(), 320.0, 1400.0, 40.0);
    assert!(s.score < 1.0 && s.score > 0.0, "{}", s.score);
    // Les largeurs étroites échouent (barre latérale de 480 px), pas les larges.
    assert!(s.failing.iter().any(|(w, _)| *w == 320.0));
    assert!(s.failing.iter().all(|(w, _)| *w < 1400.0));
    assert_eq!(
        sweep(&rigid, &bp(), &phone(), 320.0, 1400.0, 40.0),
        s,
        "deterministic"
    );
}
