use std::collections::BTreeMap;
use ui_core::hot::*;
use ui_core::*;

fn fixed(w: f32, h: f32) -> Style {
    Style {
        width: Len::Fixed(w),
        height: Len::Fixed(h),
        ..Style::default()
    }
}

fn view(label: &str, button_w: f32, extra: bool) -> Desc {
    let mut kids = vec![
        Desc::new(2, fixed(button_w, 20.0)).with(vec![Desc::text(1, "+")]),
        Desc::new(3, fixed(100.0, 20.0)).with(vec![Desc::text(1, label)]),
    ];
    if extra {
        kids.push(Desc::new(4, fixed(40.0, 20.0)).with(vec![Desc::text(1, "-")]));
    }
    Desc::new(
        1,
        Style {
            dir: Dir::Row,
            ..fixed(300.0, 20.0)
        },
    )
    .with(kids)
}

fn inc(s: &mut i64) {
    *s += 1;
}
fn inc10(s: &mut i64) {
    *s += 10;
}
fn dec(s: &mut i64) {
    *s -= 1;
}

fn behavior(version: u64, plus: Handler<i64>) -> Behavior<i64> {
    let mut handlers: BTreeMap<String, Handler<i64>> = BTreeMap::new();
    handlers.insert("plus".into(), plus);
    handlers.insert("minus".into(), dec);
    Behavior { version, handlers }
}

fn binds(extra: bool) -> BTreeMap<Path, String> {
    let mut b = BTreeMap::from([(vec![1, 2], "plus".to_string())]);
    if extra {
        b.insert(vec![1, 4], "minus".into());
    }
    b
}

fn click(s: &mut Session<i64>, p: &[u64]) {
    let r = s.ui().rect(p).unwrap();
    s.dispatch(&Event::Click {
        x: r.x + 1.0,
        y: r.y + 1.0,
    });
}

type Case = (
    Desc,
    BTreeMap<Path, String>,
    Option<Behavior<i64>>,
    fn(&ReloadError) -> bool,
);

fn session() -> Session<i64> {
    Session::new(
        view("n", 40.0, false),
        binds(false),
        behavior(1, inc),
        0,
        (300.0, 20.0),
    )
    .unwrap()
}

#[test]
fn levels_are_classified() {
    let a = view("n", 40.0, false);
    assert_eq!(classify(&a, &a), Level::None);
    assert_eq!(classify(&a, &view("n", 50.0, false)), Level::Style);
    assert_eq!(classify(&a, &view("m", 40.0, false)), Level::Structure);
    assert_eq!(classify(&a, &view("n", 40.0, true)), Level::Structure);
}

#[test]
fn l0_l1_l2_keep_state_and_focus() {
    let mut s = session();
    click(&mut s, &[1, 2]);
    assert_eq!(s.state, 1);

    let g = s.ui().generation();
    let r = s
        .reload(g, view("n", 50.0, false), binds(false), None)
        .unwrap();
    assert_eq!(
        r,
        Report {
            view: Level::Style,
            behavior_changed: false
        }
    );

    let g = s.ui().generation();
    let r = s
        .reload(g, view("n", 50.0, true), binds(true), None)
        .unwrap();
    assert_eq!(r.view, Level::Structure);
    click(&mut s, &[1, 4]);
    assert_eq!(s.state, 0);

    // L2 : nouveau comportement, même état.
    let g = s.ui().generation();
    let r = s
        .reload(
            g,
            view("n", 50.0, true),
            binds(true),
            Some(behavior(2, inc10)),
        )
        .unwrap();
    assert_eq!(
        r,
        Report {
            view: Level::None,
            behavior_changed: true
        }
    );
    click(&mut s, &[1, 2]);
    assert_eq!(s.state, 10);
    assert_eq!(s.behavior_version(), 2);
}

#[test]
fn refused_reloads_keep_the_whole_active_version() {
    let mut s = session();
    let g = s.ui().generation();
    let before = s.ui().rects().clone();
    let cases: Vec<Case> = vec![
        // Liaison vers un nœud absent.
        (
            view("n", 40.0, false),
            binds(true),
            Some(behavior(2, inc10)),
            |e| matches!(e, ReloadError::UnboundPath(p) if p == &vec![1, 4]),
        ),
        // Gestionnaire absent.
        (
            view("n", 40.0, false),
            BTreeMap::from([(vec![1, 2], "nope".to_string())]),
            Some(behavior(2, inc10)),
            |e| matches!(e, ReloadError::MissingHandler { .. }),
        ),
        // Comportement pas plus récent.
        (
            view("n", 40.0, false),
            binds(false),
            Some(behavior(1, inc10)),
            |e| matches!(e, ReloadError::BehaviorNotNewer { active: 1, got: 1 }),
        ),
        // Vue cassée.
        (
            Desc::new(1, fixed(300.0, 20.0)).with(vec![
                Desc::new(2, fixed(1.0, 1.0)),
                Desc::new(2, fixed(1.0, 1.0)),
            ]),
            binds(false),
            Some(behavior(2, inc10)),
            |e| matches!(e, ReloadError::Patch(PatchError::DuplicateKey { .. })),
        ),
        // Gate Verify : le bouton sort du conteneur.
        (
            view("n", 400.0, false),
            binds(false),
            Some(behavior(2, inc10)),
            |e| matches!(e, ReloadError::VerifyRegression(d) if !d.is_empty()),
        ),
    ];
    for (v, b, beh, ok) in cases {
        let e = s.reload(g, v, b, beh).unwrap_err();
        assert!(ok(&e), "{e:?}");
        assert_eq!(s.ui().generation(), g);
        assert_eq!(s.ui().rects(), &before);
        assert_eq!(s.behavior_version(), 1);
        click(&mut s, &[1, 2]);
    }
    // L'ancien comportement (+1) a servi à chaque clic.
    assert_eq!(s.state, 5);

    // Patch périmé.
    s.reload(g, view("n", 41.0, false), binds(false), None)
        .unwrap();
    assert!(matches!(
        s.reload(g, view("n", 42.0, false), binds(false), None),
        Err(ReloadError::Patch(PatchError::Stale { .. }))
    ));
}

#[test]
fn twenty_reloads_and_ten_thousand_events_lose_nothing() {
    let mut s = session();
    let mut expected = 0i64;
    let mut step = 1i64;
    for i in 0..10_000u32 {
        click(&mut s, &[1, 2]);
        expected += step;
        if i % 500 == 499 {
            let k = i / 500;
            let g = s.ui().generation();
            let (plus, st): (Handler<i64>, i64) = if k % 2 == 0 { (inc10, 10) } else { (inc, 1) };
            s.reload(
                g,
                view(&format!("v{k}"), 40.0 + (k % 3) as f32, false),
                binds(false),
                Some(behavior(2 + k as u64, plus)),
            )
            .unwrap();
            step = st;
        }
    }
    assert_eq!(s.state, expected);
    assert_eq!(s.behavior_version(), 21);
    assert_eq!(s.ui().focus(), None);
}

/// Le gate d'activation est le Verify complet (U8) : un avertissement ne
/// bloque pas, un contournement et une cible tactile trop petite si.
#[test]
fn reloads_go_through_the_full_verify_gate() {
    let mut s = session();
    // Texte vide : avertissement seulement, rechargement accepté.
    let mut v = view("n", 40.0, false);
    v.children[1].children[0].text = Some(String::new());
    let g = s.ui().generation();
    s.reload(g, v, binds(false), None).unwrap();

    // Contenu caché dans une boîte de largeur nulle : refusé.
    let g = s.ui().generation();
    let e = s
        .reload(g, view("n", 0.0, false), binds(false), None)
        .unwrap_err();
    assert!(
        matches!(&e, ReloadError::VerifyRegression(f) if f.iter().any(|f| f.rule == "layout.hidden-content")),
        "{e:?}"
    );

    // En tactile, une nouvelle cible de 20 px de haut est refusée ; la
    // cible existante, déjà signalée dans la version active, ne bloque pas.
    s.input = ui_core::responsive::Input::Touch;
    let mut v = view("n", 40.0, false);
    v.children.push(Desc::new(
        5,
        Style {
            focusable: true,
            ..fixed(20.0, 20.0)
        },
    ));
    let g = s.ui().generation();
    let e = s.reload(g, v, binds(false), None).unwrap_err();
    assert!(
        matches!(&e, ReloadError::VerifyRegression(f) if f.len() == 1 && f[0].rule == "a11y.touch-target" && f[0].path == vec![1, 5]),
        "{e:?}"
    );
    assert_eq!(s.ui().generation(), g);
}
