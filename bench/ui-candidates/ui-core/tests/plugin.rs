use std::collections::{BTreeMap, BTreeSet};
use ui_core::hot::Handler;
use ui_core::plugin::*;
use ui_core::*;

fn fixed(w: f32, h: f32) -> Style {
    Style {
        width: Len::Fixed(w),
        height: Len::Fixed(h),
        ..Style::default()
    }
}

const SLOT: u64 = 20;

fn shell() -> Desc {
    Desc::new(
        1,
        Style {
            dir: Dir::Column,
            ..fixed(400.0, 300.0)
        },
    )
    .with(vec![
        Desc::new(10, fixed(400.0, 20.0)).with(vec![Desc::text(1, "app")]),
        Desc::new(
            SLOT,
            Style {
                dir: Dir::Column,
                gap: 2.0,
                ..fixed(400.0, 280.0)
            },
        ),
    ])
}

fn panel(key: u64, label: &str, h: f32) -> Desc {
    Desc::new(key, fixed(200.0, h)).with(vec![Desc::text(1, label)])
}

fn bump(s: &mut i64) {
    *s += 1;
}
fn bump100(s: &mut i64) {
    *s += 100;
}

fn plugin(
    id: &str,
    version: u64,
    provides: &[&str],
    requires: &[&str],
    panels: Vec<Desc>,
    cmd: Option<Handler<i64>>,
) -> Plugin<i64> {
    Plugin {
        id: id.into(),
        version,
        provides: provides.iter().map(|s| s.to_string()).collect(),
        requires: requires.iter().map(|s| s.to_string()).collect(),
        panels,
        commands: cmd
            .into_iter()
            .map(|h| ("go".to_string(), h))
            .collect::<BTreeMap<_, _>>(),
    }
}

fn all(p: &Plugin<i64>) -> BTreeSet<Permission> {
    p.needs()
}

fn host() -> Host<i64> {
    Host::new(shell(), SLOT, 0, (400.0, 300.0)).unwrap()
}

/// Critère de sortie d'U4 (partie logique) : un plugin ajoute, modifie et
/// retire un panneau dans une application ouverte, état conservé.
#[test]
fn a_plugin_adds_updates_and_removes_a_panel_in_a_running_app() {
    let mut h = host();
    h.state = 7;
    let p = plugin(
        "meter",
        1,
        &[],
        &[],
        vec![panel(100, "meter v1", 40.0)],
        Some(bump),
    );
    let g = all(&p);
    h.install(p, g).unwrap();
    assert!(h.ui().rect(&[1, SLOT, 100]).is_some());
    assert!(h.run("meter.go"));
    assert_eq!(h.state, 8);

    h.update(plugin(
        "meter",
        2,
        &[],
        &[],
        vec![panel(100, "meter v2", 60.0)],
        Some(bump100),
    ))
    .unwrap();
    assert_eq!(h.ui().rect(&[1, SLOT, 100]).unwrap().h, 60.0);
    assert!(h.run("meter.go"));
    assert_eq!(h.state, 108);

    h.remove("meter").unwrap();
    assert!(h.ui().rect(&[1, SLOT, 100]).is_none());
    // Nettoyage automatique : la commande a disparu avec le plugin.
    assert!(!h.run("meter.go"));
    assert!(h.commands().is_empty());
    assert_eq!(h.state, 108);
    assert_eq!(
        h.log,
        vec![
            Lifecycle::Activated("meter".into(), 1),
            Lifecycle::Deactivated("meter".into(), 1),
            Lifecycle::Activated("meter".into(), 2),
            Lifecycle::Deactivated("meter".into(), 2),
        ]
    );
}

#[test]
fn services_and_dependencies() {
    let mut h = host();
    let user = plugin(
        "view",
        1,
        &[],
        &["audio"],
        vec![panel(101, "view", 20.0)],
        None,
    );
    let g = all(&user);
    assert_eq!(
        h.install(user, g),
        Err(PluginError::MissingService {
            id: "view".into(),
            service: "audio".into()
        })
    );

    let eng = plugin("engine", 1, &["audio"], &[], vec![], None);
    let g = all(&eng);
    h.install(eng, g).unwrap();
    let user = plugin(
        "view",
        1,
        &[],
        &["audio"],
        vec![panel(101, "view", 20.0)],
        None,
    );
    let g = all(&user);
    h.install(user, g).unwrap();

    // Retrait refusé tant qu'un utilisateur dépend du service.
    assert_eq!(
        h.remove("engine"),
        Err(PluginError::InUse {
            id: "engine".into(),
            by: vec!["view".into()]
        })
    );
    // Mise à jour qui retire un service encore utilisé : refusée.
    assert!(matches!(
        h.update(plugin("engine", 2, &[], &[], vec![], None)),
        Err(PluginError::MissingService { .. })
    ));
    // Doublon de service.
    let dup = plugin("other", 1, &["audio"], &[], vec![], None);
    let g = all(&dup);
    assert!(matches!(
        h.install(dup, g),
        Err(PluginError::DuplicateService { .. })
    ));
    // Cycle introduit par une mise à jour.
    let ui_p = plugin("chrome", 1, &["ui"], &["audio"], vec![], None);
    let g = all(&ui_p);
    h.install(ui_p, g).unwrap();
    let e = h
        .update(plugin("engine", 2, &["audio"], &["ui"], vec![], None))
        .unwrap_err();
    // Les permissions accordées à l'installation ne couvrent pas `Use(ui)`.
    assert_eq!(
        e,
        PluginError::PermissionDenied {
            id: "engine".into(),
            permission: Permission::Use("ui".into())
        }
    );
    assert_eq!(
        h.installed(),
        vec![
            ("chrome".into(), 1),
            ("engine".into(), 1),
            ("view".into(), 1)
        ]
    );

    h.remove("view").unwrap();
    h.remove("chrome").unwrap();
    h.remove("engine").unwrap();
    assert!(h.installed().is_empty());
}

#[test]
fn dependency_cycles_are_refused() {
    let mut h = host();
    let a = plugin("a", 1, &["x"], &[], vec![], None);
    let ga = BTreeSet::from([Permission::Provide("x".into()), Permission::Use("y".into())]);
    h.install(a, ga).unwrap();
    let b = plugin("b", 1, &["y"], &["x"], vec![], None);
    let gb = all(&b);
    h.install(b, gb).unwrap();
    let e = h
        .update(plugin("a", 2, &["x"], &["y"], vec![], None))
        .unwrap_err();
    assert_eq!(
        e,
        PluginError::Cycle(vec!["a".into(), "b".into(), "a".into()])
    );
    assert_eq!(h.installed(), vec![("a".into(), 1), ("b".into(), 1)]);
}

#[test]
fn refusals_change_nothing() {
    let mut h = host();
    let p = plugin(
        "meter",
        1,
        &[],
        &[],
        vec![panel(100, "m", 40.0)],
        Some(bump),
    );
    let g = all(&p);
    h.install(p, g).unwrap();
    let rects = h.ui().rects().clone();
    let gen = h.ui().generation();
    let log = h.log.clone();

    // Permission non accordée.
    let q = plugin("spy", 1, &[], &[], vec![panel(200, "s", 10.0)], Some(bump));
    assert_eq!(
        h.install(q, BTreeSet::from([Permission::AddPanel])),
        Err(PluginError::PermissionDenied {
            id: "spy".into(),
            permission: Permission::Command
        })
    );
    // Collision de clé de panneau.
    let q = plugin("clash", 1, &[], &[], vec![panel(100, "c", 10.0)], None);
    let g = all(&q);
    assert_eq!(
        h.install(q, g),
        Err(PluginError::KeyCollision {
            key: 100,
            a: "clash".into(),
            b: "meter".into()
        })
    );
    // Gate Verify : un panneau trop large sort de l'application.
    let q = plugin(
        "wide",
        1,
        &[],
        &[],
        vec![Desc::new(300, fixed(900.0, 10.0))],
        None,
    );
    let g = all(&q);
    assert!(matches!(
        h.install(q, g),
        Err(PluginError::VerifyRegression(_))
    ));
    // Vue cassée.
    let q = plugin(
        "bad",
        1,
        &[],
        &[],
        vec![Desc::new(400, fixed(f32::NAN, 1.0))],
        None,
    );
    let g = all(&q);
    assert!(matches!(
        h.install(q, g),
        Err(PluginError::Patch(PatchError::InvalidSize { .. }))
    ));
    // Doublons et versions.
    let q = plugin("meter", 1, &[], &[], vec![], None);
    let g = all(&q);
    assert_eq!(
        h.install(q, g),
        Err(PluginError::AlreadyInstalled("meter".into()))
    );
    assert_eq!(
        h.update(plugin("meter", 1, &[], &[], vec![], None)),
        Err(PluginError::NotNewer {
            id: "meter".into(),
            active: 1,
            got: 1
        })
    );
    assert_eq!(
        h.update(plugin("ghost", 1, &[], &[], vec![], None)),
        Err(PluginError::NotInstalled("ghost".into()))
    );
    assert_eq!(
        h.remove("ghost"),
        Err(PluginError::NotInstalled("ghost".into()))
    );
    // Mise à jour refusée par Verify : l'ancienne version reste, commande comprise.
    assert!(h
        .update(plugin(
            "meter",
            2,
            &[],
            &[],
            vec![panel(100, "m", 900.0)],
            Some(bump100)
        ))
        .is_err());

    assert_eq!(h.ui().rects(), &rects);
    assert_eq!(h.ui().generation(), gen);
    assert_eq!(h.log, log);
    assert_eq!(h.installed(), vec![("meter".into(), 1)]);
    assert!(h.run("meter.go"));
    assert_eq!(h.state, 1);
}
