use ui_core::*;

fn st() -> Style {
    Style::default()
}

fn fixed(w: f32, h: f32) -> Style {
    Style {
        width: Len::Fixed(w),
        height: Len::Fixed(h),
        ..st()
    }
}

/// Application de qualification 0 (PLAN §6) : compteur et liste de tâches.
struct App {
    count: i64,
    tasks: Vec<(u64, String)>,
    label: String,
}

impl App {
    fn view(&self) -> Desc {
        let rows: Vec<Desc> = self
            .tasks
            .iter()
            .map(|(k, t)| {
                Desc::new(
                    *k,
                    Style {
                        dir: Dir::Row,
                        height: Len::Fixed(20.0),
                        width: Len::Fill(1.0),
                        focusable: true,
                        ..st()
                    },
                )
                .with(vec![Desc::text(1, t)])
            })
            .collect();
        Desc::new(
            1,
            Style {
                dir: Dir::Column,
                width: Len::Fixed(400.0),
                height: Len::Fixed(300.0),
                padding: 4.0,
                gap: 4.0,
                ..st()
            },
        )
        .with(vec![
            Desc::new(
                10,
                Style {
                    dir: Dir::Row,
                    height: Len::Fixed(24.0),
                    width: Len::Fill(1.0),
                    gap: 4.0,
                    ..st()
                },
            )
            .with(vec![
                Desc::new(
                    11,
                    Style {
                        focusable: true,
                        ..fixed(40.0, 24.0)
                    },
                )
                .with(vec![Desc::text(1, "+")]),
                Desc::new(
                    12,
                    Style {
                        width: Len::Fill(1.0),
                        height: Len::Fixed(24.0),
                        ..st()
                    },
                )
                .with(vec![Desc::text(
                    1,
                    &format!("{}: {}", self.label, self.count),
                )]),
            ]),
            Desc::new(
                20,
                Style {
                    dir: Dir::Column,
                    scroll: true,
                    width: Len::Fill(1.0),
                    height: Len::Fill(1.0),
                    gap: 2.0,
                    ..st()
                },
            )
            .with(rows),
        ])
    }
}

fn app(n: u64) -> App {
    App {
        count: 0,
        tasks: (0..n).map(|k| (100 + k, format!("task {k}"))).collect(),
        label: "count".into(),
    }
}

#[test]
fn layout_places_fixed_fill_and_padding() {
    let ui = Ui::new(app(3).view(), (800.0, 600.0)).unwrap();
    assert_eq!(
        ui.rect(&[1]).unwrap(),
        Rect {
            x: 0.0,
            y: 0.0,
            w: 800.0,
            h: 600.0
        }
    );
    assert_eq!(
        ui.rect(&[1, 10, 11]).unwrap(),
        Rect {
            x: 4.0,
            y: 4.0,
            w: 40.0,
            h: 24.0
        }
    );
    // Fill : le reste de la rangée après le bouton et l'écart.
    assert_eq!(ui.rect(&[1, 10, 12]).unwrap().w, 800.0 - 8.0 - 40.0 - 4.0);
    // La liste prend le reste de la colonne.
    assert_eq!(ui.rect(&[1, 20]).unwrap().h, 600.0 - 8.0 - 24.0 - 4.0);
    assert_eq!(ui.rect(&[1, 20, 101]).unwrap().y, 4.0 + 24.0 + 4.0 + 22.0);
    assert!(ui.verify().is_empty(), "{:?}", ui.verify());
}

#[test]
fn keyed_patch_keeps_identity_focus_and_scroll() {
    let mut a = app(40);
    let mut ui = Ui::new(a.view(), (400.0, 300.0)).unwrap();
    let r = ui.rect(&[1, 20, 105]).unwrap();
    ui.dispatch(&Event::Click {
        x: r.x + 1.0,
        y: r.y + 1.0,
    });
    assert_eq!(ui.focus(), Some(&vec![1, 20, 105]));
    let l = ui.rect(&[1, 20]).unwrap();
    ui.dispatch(&Event::Scroll {
        x: l.x + 5.0,
        y: l.y + 5.0,
        dy: 50.0,
    });
    assert_eq!(ui.state(&[1, 20]).unwrap().scroll, 50.0);

    a.tasks.insert(10, (999, "new".into()));
    a.tasks.remove(0);
    let stats = ui.patch(ui.generation(), a.view()).unwrap();
    assert_eq!((stats.created, stats.removed), (2, 2)); // la ligne et son texte
    assert_eq!(ui.focus(), Some(&vec![1, 20, 105]));
    assert_eq!(ui.state(&[1, 20]).unwrap().scroll, 50.0);

    // Retirer la ligne qui a le focus retire le focus, pas un autre nœud.
    a.tasks.retain(|(k, _)| *k != 105);
    ui.patch(ui.generation(), a.view()).unwrap();
    assert_eq!(ui.focus(), None);
    assert!(ui.state(&[1, 20, 105]).is_none());
}

#[test]
fn broken_patches_are_refused_and_the_tree_is_kept() {
    let a = app(3);
    let mut ui = Ui::new(a.view(), (400.0, 300.0)).unwrap();
    let g = ui.generation();
    let before = ui.rects().clone();

    let mut dup = a.view();
    dup.children[1].children.push(Desc::text(100, "dup"));
    assert_eq!(
        ui.patch(g, dup),
        Err(PatchError::DuplicateKey {
            parent: vec![1, 20],
            key: 100
        })
    );

    for bad in [f32::NAN, -1.0, f32::INFINITY] {
        let mut v = a.view();
        v.children[0].children[0].style.width = Len::Fixed(bad);
        assert_eq!(
            ui.patch(g, v),
            Err(PatchError::InvalidSize {
                path: vec![1, 10, 11]
            })
        );
    }
    let mut v = a.view();
    v.children[0].style.offset = (f32::NAN, 0.0);
    assert!(matches!(
        ui.patch(g, v),
        Err(PatchError::InvalidSize { .. })
    ));

    assert_eq!(ui.generation(), g);
    assert_eq!(ui.rects(), &before);

    // Patch périmé : construit sur une génération déjà dépassée.
    ui.patch(g, a.view()).unwrap();
    assert_eq!(
        ui.patch(g, a.view()),
        Err(PatchError::Stale {
            expected: g + 1,
            got: g
        })
    );
    assert!(Ui::new(Desc::new(1, fixed(-3.0, 1.0)), (1.0, 1.0)).is_err());
}

#[test]
fn verify_flags_overflow_out_of_container_and_overlap() {
    let tree = |kids: Vec<Desc>| {
        Desc::new(
            1,
            Style {
                dir: Dir::Row,
                ..fixed(100.0, 20.0)
            },
        )
        .with(kids)
    };

    let ok = Ui::new(
        tree(vec![
            Desc::new(2, fixed(40.0, 20.0)).with(vec![Desc::text(1, "abc")])
        ]),
        (100.0, 20.0),
    )
    .unwrap();
    assert!(ok.verify().is_empty());

    let over = Ui::new(
        tree(vec![
            Desc::new(2, fixed(16.0, 20.0)).with(vec![Desc::text(1, "abc")])
        ]),
        (100.0, 20.0),
    )
    .unwrap();
    assert_eq!(
        over.verify(),
        vec![Diagnostic {
            rule: Rule::Overflow,
            path: vec![1, 2, 1],
            other: None
        }]
    );

    let out = Ui::new(
        tree(vec![
            Desc::new(2, fixed(60.0, 20.0)),
            Desc::new(3, fixed(60.0, 20.0)),
        ]),
        (100.0, 20.0),
    )
    .unwrap();
    assert_eq!(
        out.verify(),
        vec![Diagnostic {
            rule: Rule::OutOfContainer,
            path: vec![1, 3],
            other: None
        }]
    );

    let lap = Ui::new(
        tree(vec![
            Desc::new(2, fixed(30.0, 20.0)),
            Desc::new(
                3,
                Style {
                    offset: (-10.0, 0.0),
                    ..fixed(30.0, 20.0)
                },
            ),
        ]),
        (100.0, 20.0),
    )
    .unwrap();
    assert_eq!(
        lap.verify(),
        vec![Diagnostic {
            rule: Rule::Overlap,
            path: vec![1, 2],
            other: Some(vec![1, 3])
        }]
    );

    // Un conteneur défilant peut dépasser dans son axe principal.
    let ui = Ui::new(app(100).view(), (400.0, 300.0)).unwrap();
    assert!(ui.verify().is_empty(), "{:?}", ui.verify());
    // Mais pas sans défilement.
    let mut v = app(100).view();
    v.children[1].style.scroll = false;
    let ui = Ui::new(v, (400.0, 300.0)).unwrap();
    assert!(ui.verify().iter().all(|d| d.rule == Rule::OutOfContainer) && !ui.verify().is_empty());
}

#[test]
fn verify_is_deterministic() {
    let mut v = app(200).view();
    v.children[1].style.scroll = false;
    v.children[0].children[0].style.offset = (500.0, 3.0);
    let first = Ui::new(v.clone(), (400.0, 300.0)).unwrap().verify();
    assert!(!first.is_empty());
    for _ in 0..20 {
        assert_eq!(Ui::new(v.clone(), (400.0, 300.0)).unwrap().verify(), first);
    }
}

#[test]
fn clipped_children_are_not_hit() {
    let ui_app = app(100);
    let mut ui = Ui::new(ui_app.view(), (400.0, 300.0)).unwrap();
    let far = ui.rect(&[1, 20, 199]).unwrap();
    assert!(far.y > 300.0);
    ui.dispatch(&Event::Click {
        x: far.x + 1.0,
        y: 299.0,
    });
    assert!(ui.take_clicks().iter().all(|p| p != &vec![1, 20, 199]));
}

/// U3 : 10 000 événements et 20 éditions sans perte d'état ni d'événement.
#[test]
fn ten_thousand_events_and_twenty_edits_lose_nothing() {
    let mut a = app(30);
    let mut ui = Ui::new(a.view(), (400.0, 300.0)).unwrap();
    let plus = ui.rect(&[1, 10, 11]).unwrap();
    let list = ui.rect(&[1, 20]).unwrap();
    ui.dispatch(&Event::Scroll {
        x: list.x + 2.0,
        y: list.y + 2.0,
        dy: 30.0,
    });
    let mut expected = 0i64;
    let mut seed = 0x2545_f491_u64;
    for i in 0..10_000u32 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        if seed.is_multiple_of(3) {
            ui.dispatch(&Event::Click {
                x: plus.x + 2.0,
                y: plus.y + 2.0,
            });
            expected += 1;
        } else {
            ui.dispatch(&Event::Click {
                x: 300.0,
                y: plus.y + 2.0,
            });
        }
        for p in ui.take_clicks() {
            if p.starts_with(&[1, 10, 11]) {
                a.count += 1;
            }
        }
        if i % 500 == 499 {
            // Édition : texte, structure (une tâche en plus) et style.
            a.label = format!("n{}", i / 500);
            a.tasks.push((1000 + i as u64, format!("t{i}")));
            ui.patch(ui.generation(), a.view()).unwrap();
            assert_eq!(ui.focus(), Some(&vec![1, 10, 11]));
            assert_eq!(ui.state(&[1, 20]).unwrap().scroll, 30.0);
        }
    }
    assert_eq!(a.count, expected);
    assert!(expected > 3000);
    assert_eq!(ui.generation(), 21);
    assert!(ui.verify().is_empty());
}

#[test]
fn focus_cycles_in_tree_order() {
    let mut ui = Ui::new(app(2).view(), (400.0, 300.0)).unwrap();
    let order: Vec<Path> = (0..4)
        .map(|_| {
            ui.dispatch(&Event::FocusNext);
            ui.focus().unwrap().clone()
        })
        .collect();
    assert_eq!(
        order,
        vec![
            vec![1, 10, 11],
            vec![1, 20, 100],
            vec![1, 20, 101],
            vec![1, 10, 11]
        ]
    );
}

#[test]
fn a_stack_measure_drives_layout_and_verify() {
    let d = Desc::new(1, fixed(100.0, 40.0)).with(vec![Desc::text(2, "abcdef")]);
    // The root takes the viewport: make it the 100 × 40 box.
    let mut ui = Ui::new(d.clone(), (100.0, 40.0)).unwrap();
    // Monospace: 6 × 8 = 48 px (auto width = measured width), fits.
    assert_eq!(ui.rect(&[1, 2]).unwrap().w, 48.0);
    assert!(ui.verify().is_empty());
    // A wider font: 6 × 20 = 120 px > 100: the same tree now overflows.
    assert!(ui.set_measure(std::rc::Rc::new(|t: &str| (
        t.chars().count() as f32 * 20.0,
        18.0
    ))));
    assert_eq!(ui.rect(&[1, 2]).unwrap().w, 120.0);
    assert_eq!(ui.verify().len(), 1, "{:?}", ui.verify());
    // Auto height follows the measured line height.
    let auto = Desc::new(
        1,
        Style {
            width: Len::Fixed(300.0),
            ..Style::default()
        },
    )
    .with(vec![Desc::text(2, "abc")]);
    let g = ui.generation();
    ui.patch(g, auto).unwrap();
    assert_eq!(ui.rect(&[1, 2]).unwrap().h, 18.0);
}

#[test]
fn a_bad_measure_is_refused_and_changes_nothing() {
    let d = Desc::new(1, fixed(100.0, 40.0)).with(vec![Desc::text(2, "abc")]);
    let mut ui = Ui::new(d, (200.0, 100.0)).unwrap();
    let before = ui.rects().clone();
    for bad in [f32::NAN, f32::INFINITY, -1.0] {
        assert!(!ui.set_measure(std::rc::Rc::new(move |_: &str| (bad, 16.0))));
        assert!(!ui.set_measure(std::rc::Rc::new(move |_: &str| (8.0, bad))));
    }
    assert_eq!(ui.rects(), &before);
    // A measure that breaks only on some text: refused when that text
    // arrives by patch, and the active tree stays.
    assert!(ui.set_measure(std::rc::Rc::new(|t: &str| if t == "boom" {
        (f32::NAN, 16.0)
    } else {
        (8.0, 16.0)
    })));
    let g = ui.generation();
    let bad = Desc::new(1, fixed(100.0, 40.0)).with(vec![Desc::text(2, "boom")]);
    assert_eq!(
        ui.patch(g, bad),
        Err(PatchError::InvalidSize { path: vec![1, 2] })
    );
    assert_eq!(ui.generation(), g);
    assert_eq!(ui.find(&[1, 2]).unwrap().text.as_deref(), Some("abc"));
}
