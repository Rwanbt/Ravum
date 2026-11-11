//! Lot B — overlays privés, intégration transactionnelle et absence de perte.
//!
//! Tous les cas sont déterministes et n'exécutent ni rustc ni Git : ils portent
//! sur le coordinateur d'overlays lui-même.

use ravum_workspace_coordinator::{
    validate_relative_path, BaseSnapshot, ChangeCoordinator, IntegrationOutcome, MaterializeLimits,
    Overlay, OverlayError,
};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

fn base_with(files: &[(&str, &str)]) -> BaseSnapshot {
    let mut map = BTreeMap::new();
    for (path, content) in files {
        map.insert((*path).to_owned(), content.as_bytes().to_vec());
    }
    BaseSnapshot::from_files(map).expect("test fixture paths are valid")
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/ravum-tests")
        .join(format!("overlay-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create temp dir");
    root
}

/// Crée un lien symbolique de répertoire. Retourne `false` quand l'hôte refuse la
/// création (Windows sans privilège `SeCreateSymbolicLinkPrivilege`) : l'appelant
/// doit alors SKIP explicitement, pas passer en silence.
#[cfg(windows)]
fn try_dir_symlink(target: &std::path::Path, link: &std::path::Path) -> bool {
    std::os::windows::fs::symlink_dir(target, link).is_ok()
}

#[cfg(unix)]
fn try_dir_symlink(target: &std::path::Path, link: &std::path::Path) -> bool {
    std::os::unix::fs::symlink(target, link).is_ok()
}

#[cfg(windows)]
fn try_file_symlink(target: &std::path::Path, link: &std::path::Path) -> bool {
    std::os::windows::fs::symlink_file(target, link).is_ok()
}

#[cfg(unix)]
fn try_file_symlink(target: &std::path::Path, link: &std::path::Path) -> bool {
    std::os::unix::fs::symlink(target, link).is_ok()
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
    let base = BaseSnapshot::from_files(files).expect("fixture paths are valid");
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

// ---------------------------------------------------------------------------
// Régression P0 n°1 — `publish` doit clore le cycle d'intégration.
// ---------------------------------------------------------------------------

#[test]
fn publication_starts_a_new_cycle_and_the_same_path_is_no_longer_a_false_conflict() {
    let base = base_with(&[("src/a.rs", "fn a() { 0 }"), ("src/b.rs", "fn b() {}")]);
    let mut coordinator = ChangeCoordinator::new(base.clone());

    // Cycle 1 : Alice modifie `src/a.rs`, puis l'état est publié.
    let mut alice = Overlay::new(&base, "alice");
    alice.write("src/a.rs", b"fn a() { 1 }".to_vec()).unwrap();
    let first = coordinator.integrate(&alice);
    assert!(first.is_applied(), "{first:?}");
    let published = coordinator.publish();
    assert_eq!(coordinator.base_id(), published);
    assert!(
        coordinator.applied_paths().is_empty(),
        "publish() must close the integration cycle: {:?}",
        coordinator.applied_paths()
    );

    // Cycle 2 : Bob a lu la NOUVELLE base et touche le même chemin. Ce n'est pas un
    // conflit : l'ancien code conservait `applied` et le refusait à tort.
    let new_base = coordinator.snapshot_after();
    assert_eq!(new_base.id(), published);
    let mut bob = Overlay::new(&new_base, "bob");
    bob.write("src/a.rs", b"fn a() { 2 }".to_vec()).unwrap();
    let second = coordinator.integrate(&bob);
    assert!(
        second.is_applied(),
        "a proposal built on the published base must be Applied, got {second:?}"
    );
    assert_eq!(
        coordinator.snapshot_after().read("src/a.rs"),
        Some(&b"fn a() { 2 }"[..])
    );
    assert_eq!(
        coordinator.history().len(),
        2,
        "history keeps one record per decision across publications"
    );
}

#[test]
fn publication_preserves_cas_semantics_and_resets_per_cycle_accounting() {
    let base = base_with(&[("src/a.rs", "fn a() { 0 }"), ("src/b.rs", "fn b() {}")]);
    let mut coordinator = ChangeCoordinator::new(base.clone());

    let mut alice = Overlay::new(&base, "alice");
    alice.write("src/a.rs", b"fn a() { 1 }".to_vec()).unwrap();
    assert!(coordinator.integrate(&alice).is_applied());
    assert_eq!(coordinator.storage_report().agents, 1);
    assert!(coordinator.storage_report().overlay_bytes > 0);
    let published = coordinator.publish();
    let report = coordinator.storage_report();
    assert_eq!(report.agents, 0, "a published cycle has no pending agent");
    assert_eq!(report.overlay_bytes, 0);
    assert_eq!(
        report.base_bytes,
        coordinator.snapshot_after().total_bytes()
    );

    // La CAS reste correcte après publication : l'ancienne base est périmée…
    let mut stale = Overlay::new(&base, "stale");
    stale.write("src/b.rs", b"fn b() { 1 }".to_vec()).unwrap();
    match coordinator.integrate(&stale) {
        IntegrationOutcome::RejectedStale {
            expected, actual, ..
        } => {
            assert_eq!(expected, base.id());
            assert_eq!(actual, published);
        }
        other => panic!("expected RejectedStale, got {other:?}"),
    }

    // …et la nouvelle base est acceptée.
    let new_base = coordinator.snapshot_after();
    let mut fresh = Overlay::new(&new_base, "fresh");
    fresh.write("src/b.rs", b"fn b() { 2 }".to_vec()).unwrap();
    assert!(coordinator.integrate(&fresh).is_applied());
}

#[test]
fn publish_without_integration_is_idempotent_on_the_base_identity() {
    let base = base_with(&[("src/a.rs", "fn a() {}")]);
    let mut coordinator = ChangeCoordinator::new(base.clone());
    let first = coordinator.publish();
    let second = coordinator.publish();
    assert_eq!(first, base.id());
    assert_eq!(second, base.id());
    assert_eq!(coordinator.history().len(), 0);
}

// ---------------------------------------------------------------------------
// P0 n°2 — corpus hostile : aucun chemin grammaticalement dangereux n'entre.
// ---------------------------------------------------------------------------

#[test]
fn hostile_relative_paths_are_refused_by_every_entry_point() {
    let base = base_with(&[("src/a.rs", "fn a() {}")]);
    let hostile: &[&str] = &[
        "",
        "   ",
        "../evil",
        "/abs",
        "C:\\x",
        "C:/x",
        "c:file.txt",
        "\\\\?\\C:\\x",
        "\\\\server\\share\\x",
        "a/../../b",
        "a/./b",
        "a//b",
        "a\\b",
        "src\\win.rs",
        "CON",
        "con",
        "CON.txt",
        "src/NUL/x",
        "PRN.rs",
        "AUX",
        "COM1",
        "COM9.log",
        "LPT1",
        "LPT9.tar.gz",
        "a:b",
        "file:stream",
        "a.",
        "a ",
        "trailing/",
        "a\u{0}b",
        "a\u{1f}b",
    ];
    for path in hostile {
        assert!(
            validate_relative_path(path).is_err(),
            "{path:?} must be refused by the grammar"
        );
        let mut overlay = Overlay::new(&base, "hostile");
        assert!(
            overlay.write(path, b"x".to_vec()).is_err(),
            "{path:?} must be refused by Overlay::write"
        );
        assert!(
            overlay.delete(path).is_err(),
            "{path:?} must be refused by Overlay::delete"
        );
        assert!(overlay.changed_paths().is_empty());
        let mut map = BTreeMap::new();
        map.insert((*path).to_owned(), b"x".to_vec());
        assert!(
            BaseSnapshot::from_files(map).is_err(),
            "{path:?} must be refused by BaseSnapshot::from_files"
        );
    }

    // Noms ordinaires acceptés (aucune sur-réjection).
    for path in ["src/lib.rs", "Cargo.toml", ".gitignore", "src/.gitkeep"] {
        assert!(
            validate_relative_path(path).is_ok(),
            "{path:?} is a legitimate relative path"
        );
    }

    // Longueur excessive, composant et chemin.
    let long_component = "a".repeat(256);
    assert!(matches!(
        validate_relative_path(&long_component),
        Err(OverlayError::TooLong { .. })
    ));
    let long_path = format!("{}/{}", "d".repeat(255), "e".repeat(255)).repeat(3);
    assert!(matches!(
        validate_relative_path(&long_path),
        Err(OverlayError::TooLong { .. })
    ));
}

#[test]
fn materialize_revalidates_and_writes_atomically_inside_the_root() {
    let root = temp_dir("materialize-contained");
    let view = root.join("view");
    let base = base_with(&[("src/a.rs", "fn a() {}")]);
    let mut overlay = Overlay::new(&base, "alice");
    overlay
        .write("nested/deep/b.rs", b"fn b() {}".to_vec())
        .unwrap();

    let written = overlay.materialize(&base, &view).expect("materialize");
    assert_eq!(written, 2);
    assert!(view.join("src/a.rs").is_file());
    assert!(view.join("nested/deep/b.rs").is_file());
    // Aucun temporaire ne survit à la matérialisation.
    let leftovers: Vec<_> = std::fs::read_dir(view.join("nested/deep"))
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(".ravum-"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "temporary files left behind: {leftovers:?}"
    );

    // Les chemins hostiles sont refusés avant toute écriture : rien hors racine.
    for path in [
        "../evil",
        "/abs",
        "C:/x",
        "\\\\?\\C:\\x",
        "a/../../b",
        "CON",
        "a:b",
    ] {
        assert!(overlay.write(path, b"pwned".to_vec()).is_err());
    }
    assert!(!root.join("evil").exists());
    assert!(!root.join("abs").exists());
}

#[test]
fn materialize_enforces_file_and_byte_limits() {
    let root = temp_dir("materialize-limits");
    let base = base_with(&[("a.rs", "1"), ("b.rs", "2"), ("c.rs", "3")]);
    let overlay = Overlay::new(&base, "alice");

    let by_files = overlay
        .materialize_with_limits(
            &base,
            &root.join("by-files"),
            MaterializeLimits {
                max_files: 2,
                max_bytes: 1 << 20,
            },
        )
        .expect_err("three files must exceed a two-file budget");
    assert_eq!(by_files.kind(), std::io::ErrorKind::InvalidInput);

    let by_bytes = overlay
        .materialize_with_limits(
            &base,
            &root.join("by-bytes"),
            MaterializeLimits {
                max_files: 100,
                max_bytes: 1,
            },
        )
        .expect_err("three bytes must exceed a one-byte budget");
    assert_eq!(by_bytes.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn materialize_refuses_a_symlinked_directory_component() {
    let root = temp_dir("materialize-symlink-dir");
    let outside = temp_dir("materialize-symlink-dir-outside");
    std::fs::write(outside.join("sentinel.txt"), b"outside").unwrap();
    let view = root.join("view");
    std::fs::create_dir_all(&view).unwrap();
    if !try_dir_symlink(&outside, &view.join("link")) {
        println!(
            "SKIP materialize_refuses_a_symlinked_directory_component: \
             the host refused to create a directory symlink (Windows privilege missing)"
        );
        return;
    }

    let base = base_with(&[("src/a.rs", "fn a() {}")]);
    let mut overlay = Overlay::new(&base, "alice");
    // Grammar is valid: the attack is the link that already exists on disk.
    overlay.write("link/evil.txt", b"pwned".to_vec()).unwrap();
    let error = overlay
        .materialize(&base, &view)
        .expect_err("a symlinked parent must be refused");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(
        !outside.join("evil.txt").exists(),
        "the symlink must never be followed: no byte outside the root"
    );
    assert_eq!(
        std::fs::read(outside.join("sentinel.txt")).unwrap(),
        b"outside",
        "the outside content is untouched"
    );
    // Only `sentinel.txt` plus the overlay's valid paths may exist outside; the
    // malicious file itself must be absent.
    let outside_names: Vec<String> = std::fs::read_dir(&outside)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(outside_names, vec!["sentinel.txt".to_owned()]);
}

#[test]
fn materialize_refuses_to_replace_a_symlinked_file() {
    let root = temp_dir("materialize-symlink-file");
    let outside = temp_dir("materialize-symlink-file-outside");
    let victim = outside.join("victim.txt");
    std::fs::write(&victim, b"original").unwrap();
    let view = root.join("view");
    std::fs::create_dir_all(&view).unwrap();
    if !try_file_symlink(&victim, &view.join("a.rs")) {
        println!(
            "SKIP materialize_refuses_to_replace_a_symlinked_file: \
             the host refused to create a file symlink (Windows privilege missing)"
        );
        return;
    }

    let base = base_with(&[("a.rs", "fn a() {}")]);
    let overlay = Overlay::new(&base, "alice");
    let error = overlay
        .materialize(&base, &view)
        .expect_err("replacing a symlink must be refused");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(
        std::fs::read(&victim).unwrap(),
        b"original",
        "the symlink target must never be overwritten"
    );
}
