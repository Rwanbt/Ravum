//! Lot B — overlays privés, intégration transactionnelle et absence de perte.
//!
//! Tous les cas sont déterministes et n'exécutent ni rustc ni Git : ils portent
//! sur le coordinateur d'overlays lui-même.

use ravum_workspace_coordinator::{
    validate_relative_path, BaseSnapshot, ChangeCoordinator, IntegrationOutcome, Overlay,
    OverlayError,
};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

fn base_with(files: &[(&str, &str)]) -> BaseSnapshot {
    let mut map = BTreeMap::new();
    for (path, content) in files {
        map.insert((*path).to_owned(), content.as_bytes().to_vec());
    }
    BaseSnapshot::from_files(map)
}

#[test]
fn independent_files_from_two_agents_are_both_applied() {
    let base = base_with(&[("src/a.rs", "fn a() {}"), ("src/b.rs", "fn b() {}")]);
    let mut coordinator = ChangeCoordinator::new(base.clone());

    let mut alice = Overlay::new(&base, "alice");
    alice.write("src/a.rs", b"fn a() { 1 }".to_vec()).unwrap();
    let mut bob = Overlay::new(&base, "bob");
    bob.write("src/b.rs", b"fn b() { 2 }".to_vec()).unwrap();

    let first = coordinator.integrate(&alice);
    let second = coordinator.integrate(&bob);
    assert!(first.is_applied(), "{first:?}");
    assert!(second.is_applied(), "{second:?}");

    let after = coordinator.snapshot_after();
    assert_eq!(after.read("src/a.rs"), Some(&b"fn a() { 1 }"[..]));
    assert_eq!(after.read("src/b.rs"), Some(&b"fn b() { 2 }"[..]));
    assert_ne!(after.id(), base.id(), "the canonical state changed");
    assert_eq!(after.files().len(), 2, "no file was lost");
}

#[test]
fn a_shared_path_is_never_merged_and_nothing_is_applied() {
    let base = base_with(&[("src/a.rs", "fn a() { 0 }"), ("src/b.rs", "fn b() {}")]);
    let mut coordinator = ChangeCoordinator::new(base.clone());

    let mut alice = Overlay::new(&base, "alice");
    alice.write("src/a.rs", b"fn a() { 1 }".to_vec()).unwrap();
    let mut bob = Overlay::new(&base, "bob");
    // Bob touches the same path and a new one: the whole proposal is refused,
    // so a half-applied change can never be published.
    bob.write("src/a.rs", b"fn a() { 2 }".to_vec()).unwrap();
    bob.write("src/c.rs", b"fn c() {}".to_vec()).unwrap();

    assert!(coordinator.integrate(&alice).is_applied());
    let refused = coordinator.integrate(&bob);
    match &refused {
        IntegrationOutcome::ConflictPath { paths, .. } => {
            assert_eq!(paths, &vec!["src/a.rs".to_owned()]);
        }
        other => panic!("expected ConflictPath, got {other:?}"),
    }
    let after = coordinator.snapshot_after();
    assert_eq!(
        after.read("src/a.rs"),
        Some(&b"fn a() { 1 }"[..]),
        "the first accepted change is kept"
    );
    assert!(
        after.read("src/c.rs").is_none(),
        "the refused proposal must not leave a partial change"
    );
}

#[test]
fn a_stale_base_is_rejected() {
    let base = base_with(&[("src/a.rs", "fn a() {}")]);
    let mut coordinator = ChangeCoordinator::new(base.clone());

    let mut old = Overlay::new(&base, "old");
    old.write("src/a.rs", b"fn a() { 1 }".to_vec()).unwrap();
    assert!(coordinator.integrate(&old).is_applied());
    // Publication : l'état intégré devient la nouvelle base canonique.
    let published = coordinator.publish();
    assert_eq!(coordinator.base_id(), published);

    let mut stale = Overlay::new(&base, "stale");
    stale.write("src/b.rs", b"fn b() {}".to_vec()).unwrap();
    let refused = coordinator.integrate(&stale);
    match refused {
        IntegrationOutcome::RejectedStale {
            expected, actual, ..
        } => {
            assert_eq!(expected, base.id());
            assert_ne!(actual, base.id());
        }
        other => panic!("expected RejectedStale, got {other:?}"),
    }
    assert!(coordinator.snapshot_after().read("src/b.rs").is_none());
}

#[test]
fn unsafe_paths_are_rejected_before_any_mutation() {
    assert_eq!(
        validate_relative_path("../etc/passwd"),
        Err(OverlayError::UnsafePath("../etc/passwd".to_owned()))
    );
    assert_eq!(
        validate_relative_path("src/../../escape.rs"),
        Err(OverlayError::UnsafePath("src/../../escape.rs".to_owned()))
    );
    assert_eq!(
        validate_relative_path("/abs.rs"),
        Err(OverlayError::AbsolutePath("/abs.rs".to_owned()))
    );
    assert_eq!(
        validate_relative_path("C:/abs.rs"),
        Err(OverlayError::AbsolutePath("C:/abs.rs".to_owned()))
    );
    assert_eq!(
        validate_relative_path("src\\win.rs"),
        Err(OverlayError::UnsafePath("src\\win.rs".to_owned()))
    );
    assert_eq!(validate_relative_path(""), Err(OverlayError::EmptyPath));
    assert!(validate_relative_path("src/ok.rs").is_ok());

    let base = base_with(&[("src/a.rs", "fn a() {}")]);
    let mut overlay = Overlay::new(&base, "alice");
    assert!(overlay.write("../escape.rs", b"x".to_vec()).is_err());
    assert!(overlay.changed_paths().is_empty(), "nothing was recorded");
    assert_eq!(overlay.overlay_bytes(), 0);
}

#[test]
fn an_interrupted_agent_leaves_the_base_untouched() {
    let base = base_with(&[("src/a.rs", "fn a() {}")]);
    let coordinator = ChangeCoordinator::new(base.clone());
    let mut overlay = Overlay::new(&base, "interrupted");
    overlay.write("src/a.rs", b"fn a() { 1 }".to_vec()).unwrap();
    // The agent "dies" here: its overlay is dropped without integration.
    drop(overlay);
    assert_eq!(
        coordinator.snapshot_after().read("src/a.rs"),
        Some(&b"fn a() {}"[..]),
        "a dropped overlay never mutates the canonical state"
    );
    assert_eq!(coordinator.applied_paths().len(), 0);
}

#[test]
fn concurrent_integration_is_serialised_by_the_coordinator() {
    let base = base_with(&[("src/base.rs", "fn base() {}")]);
    let coordinator = Arc::new(Mutex::new(ChangeCoordinator::new(base.clone())));
    let mut handles = Vec::new();
    for index in 0..8 {
        let base = base.clone();
        let coordinator = Arc::clone(&coordinator);
        handles.push(std::thread::spawn(move || {
            let mut overlay = Overlay::new(&base, &format!("agent-{index}"));
            overlay
                .write(&format!("src/f{index}.rs"), b"fn f() {}".to_vec())
                .unwrap();
            coordinator.lock().unwrap().integrate(&overlay)
        }));
    }
    let outcomes: Vec<IntegrationOutcome> = handles
        .into_iter()
        .map(|handle| handle.join().expect("thread"))
        .collect();
    assert_eq!(outcomes.len(), 8);
    assert!(outcomes.iter().all(IntegrationOutcome::is_applied));

    let coordinator = coordinator.lock().unwrap();
    assert_eq!(coordinator.applied_paths().len(), 8);
    let after = coordinator.snapshot_after();
    assert_eq!(after.files().len(), 9, "base file + 8 independent files");
}

#[test]
fn overlays_cost_less_than_full_private_copies() {
    // Base de 100 fichiers de 4 Kio ; 8 agents écrivent 2 fichiers de 16 Kio.
    let mut files = BTreeMap::new();
    for index in 0..100 {
        files.insert(format!("src/mod{index}.rs"), vec![b'x'; 4096]);
    }
    let base = BaseSnapshot::from_files(files);
    let mut coordinator = ChangeCoordinator::new(base.clone());
    for agent in 0..8 {
        let mut overlay = Overlay::new(&base, &format!("agent-{agent}"));
        overlay
            .write(&format!("src/own{agent}_a.rs"), vec![b'y'; 16 * 1024])
            .unwrap();
        overlay
            .write(&format!("src/own{agent}_b.rs"), vec![b'y'; 16 * 1024])
            .unwrap();
        assert!(coordinator.integrate(&overlay).is_applied());
    }
    let report = coordinator.storage_report();
    assert_eq!(report.agents, 8);
    assert_eq!(report.base_bytes, 100 * 4096);
    assert_eq!(report.overlay_bytes, 8 * 2 * 16 * 1024);
    assert_eq!(report.full_copies_bytes, 8 * 100 * 4096);
    assert!(
        report.overlay_bytes < report.full_copies_bytes,
        "overlays must cost less than eight full copies"
    );
    assert!(
        report.saved_ratio() > 0.8,
        "expected a large storage saving, got {}",
        report.saved_ratio()
    );
    println!(
        "MEASURE storage: base={} overlay={} full_copies={} saved={:.1}%",
        report.base_bytes,
        report.overlay_bytes,
        report.full_copies_bytes,
        report.saved_ratio() * 100.0
    );
}

#[test]
fn deletions_are_applied_and_never_lose_other_files() {
    let base = base_with(&[("src/a.rs", "fn a() {}"), ("src/b.rs", "fn b() {}")]);
    let mut coordinator = ChangeCoordinator::new(base.clone());
    let mut overlay = Overlay::new(&base, "remover");
    overlay.delete("src/a.rs").unwrap();
    assert!(coordinator.integrate(&overlay).is_applied());
    let after = coordinator.snapshot_after();
    assert!(after.read("src/a.rs").is_none());
    assert_eq!(after.read("src/b.rs"), Some(&b"fn b() {}"[..]));
    assert_eq!(after.files().len(), 1);
}

#[test]
fn no_modification_is_lost_across_integration() {
    let base = base_with(&[("src/seed.rs", "fn seed() {}")]);
    let mut coordinator = ChangeCoordinator::new(base.clone());
    let mut expected = BTreeMap::new();
    for index in 0..12 {
        let mut overlay = Overlay::new(&base, &format!("agent-{index}"));
        let path = format!("src/change{index}.rs");
        let content = format!("fn change{index}() {{}}");
        overlay.write(&path, content.as_bytes().to_vec()).unwrap();
        expected.insert(path, content);
        assert!(coordinator.integrate(&overlay).is_applied());
    }
    let after = coordinator.snapshot_after();
    for (path, content) in expected {
        assert_eq!(
            after
                .read(&path)
                .map(|bytes| String::from_utf8_lossy(bytes).to_string()),
            Some(content),
            "{path} was lost"
        );
    }
}
