//! Intégration transactionnelle de bout en bout : overlay → oracle → GitWriter →
//! nouvelle base, avec reprise et idempotence.
//!
//! Tous les dépôts sont jetables (`target/ravum-tests/`). Les assertions ne
//! croient jamais le cycle sur parole : l'état Git est relu avec `git` plumbing.

use ravum_protocol::{sha256_hex, ChangeProposal, SnapshotId, COLLAB_SCHEMA_VERSION};
use ravum_workspace_coordinator::{
    commit_cycle, BaseSnapshot, ChangeCoordinator, CycleOutcome, CycleRequest, GitWriter, Overlay,
    TextualSemanticsOracle,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const OLD_A: &str = "fn a() { 1 }\n";
const NEW_A: &str = "fn a() { 2 }\n";
const OLD_B: &str = "fn b() { 1 }\n";
const NEW_B: &str = "fn b() { 2 }\n";

const PATCH_A: &str = "+++ b/a.rs\n@@ -1,1 +1,1 @@\n-fn a() { 1 }\n+fn a() { 2 }\n";
const PATCH_B: &str = "+++ b/b.rs\n@@ -1,1 +1,1 @@\n-fn b() { 1 }\n+fn b() { 2 }\n";
const DELETE_B: &str = "+++ b/b.rs\n@@ -1,1 +0,0 @@\n-fn b() { 1 }\n";

// ---------------------------------------------------------------------------
// fixtures
// ---------------------------------------------------------------------------

fn tests_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ravum-tests")
}

fn test_dir(name: &str) -> PathBuf {
    let dir = tests_root().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create test directory");
    dir
}

fn fresh_journal(name: &str) -> PathBuf {
    let path = tests_root().join(name).with_extension("journal.jsonl");
    let _ = std::fs::remove_file(&path);
    path
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = git_try(dir, args);
    assert!(
        output.status.success(),
        "git {args:?} failed:\nstderr: {}\nstdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn git_try(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("run git")
}

fn init_repo(name: &str) -> PathBuf {
    let repo = test_dir(name);
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.name", "test"]);
    git(&repo, &["config", "user.email", "test@ravum.invalid"]);
    git(&repo, &["config", "core.autocrlf", "false"]);
    git(&repo, &["config", "commit.gpgsign", "false"]);
    std::fs::write(repo.join("a.rs"), OLD_A).expect("write a.rs");
    std::fs::write(repo.join("b.rs"), OLD_B).expect("write b.rs");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    repo
}

fn write_file(repo: &Path, relative: &str, content: &str) {
    let path = repo.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent directory");
    }
    std::fs::write(&path, content).expect("write file");
}

fn read_file(repo: &Path, relative: &str) -> String {
    std::fs::read_to_string(repo.join(relative)).expect("read file")
}

fn head(repo: &Path) -> String {
    git(repo, &["rev-parse", "HEAD"]).trim().to_owned()
}

fn commit_count(repo: &Path) -> usize {
    git(repo, &["rev-list", "--count", "HEAD"])
        .trim()
        .parse()
        .expect("commit count")
}

fn status(repo: &Path) -> String {
    git(repo, &["status", "--porcelain"])
}

fn head_file(repo: &Path, path: &str) -> String {
    git(repo, &["show", &format!("HEAD:{path}")])
}

fn writer(repo: &Path, journal: &Path) -> GitWriter {
    GitWriter::new(repo, "agent-cycle", "agent-cycle@ravum.invalid")
        .with_publication_journal(journal)
        .expect("publication journal")
}

fn base_from_repo(repo: &Path) -> BaseSnapshot {
    BaseSnapshot::from_directory(repo).expect("snapshot of the throw-away repository")
}

fn proposal(
    coordinator: &ChangeCoordinator,
    agent: &str,
    task: &str,
    target_paths: &[&str],
    patch: &str,
) -> ChangeProposal {
    let mut expected_hashes = BTreeMap::new();
    for path in target_paths {
        if let Some(bytes) = coordinator.base().read(path) {
            expected_hashes.insert((*path).to_owned(), sha256_hex(bytes));
        }
    }
    ChangeProposal {
        schema_version: COLLAB_SCHEMA_VERSION,
        agent_id: agent.to_owned(),
        task_id: task.to_owned(),
        base_snapshot_id: SnapshotId::new(coordinator.base_id()),
        target_paths: target_paths.iter().map(|path| (*path).to_owned()).collect(),
        target_symbols: Vec::new(),
        expected_hashes,
        declared_dependencies: Vec::new(),
        impacted_contracts: Vec::new(),
        unified_patch: patch.to_owned(),
        note: None,
    }
}

fn overlay_writing(
    coordinator: &ChangeCoordinator,
    agent: &str,
    path: &str,
    content: &str,
) -> Overlay {
    let mut overlay = Overlay::new(coordinator.base(), agent);
    overlay
        .write(path, content.as_bytes().to_vec())
        .expect("valid overlay path");
    overlay
}

// ---------------------------------------------------------------------------
// 1. cycle nominal : publication + avance de la base
// ---------------------------------------------------------------------------

#[test]
fn cycle_publishes_and_advances_the_base_exactly_once() {
    let repo = init_repo("cycle-nominal");
    let journal = fresh_journal("cycle-nominal");
    let publisher = writer(&repo, &journal);
    let mut coordinator = ChangeCoordinator::new(base_from_repo(&repo));
    let base_before = coordinator.base_id().to_owned();
    let before = head(&repo);

    let overlay = overlay_writing(&coordinator, "agent-cycle", "a.rs", NEW_A);
    let proposal = proposal(&coordinator, "agent-cycle", "t1", &["a.rs"], PATCH_A);
    let request = CycleRequest::new(&overlay, &proposal, "cycle-nominal-1", "feat: cycle")
        .with_trailer("Ravum-Agent", "agent-cycle");

    let outcome = commit_cycle(
        &mut coordinator,
        &publisher,
        &TextualSemanticsOracle,
        &request,
    );
    let (base_id, commit) = match &outcome {
        CycleOutcome::Published {
            base_id, commit, ..
        } => (base_id.clone(), commit.clone()),
        other => panic!("expected Published, got {other:?}"),
    };
    assert_ne!(base_id, base_before, "the canonical base must change");
    assert_eq!(coordinator.base_id(), base_id);
    assert!(
        coordinator.applied_paths().is_empty(),
        "the integration cycle is closed after publication"
    );
    assert_eq!(commit.parent, before, "the publication is based on HEAD");
    assert_eq!(head(&repo), commit.sha, "HEAD is the published commit");
    assert_eq!(commit_count(&repo), 2, "exactly one new commit");
    assert_eq!(read_file(&repo, "a.rs"), NEW_A);
    assert_eq!(head_file(&repo, "a.rs"), NEW_A);
    assert_eq!(status(&repo).trim(), "", "working tree/index reconciled");
    assert_eq!(
        publisher
            .publication_status("cycle-nominal-1")
            .expect("status"),
        Some(ravum_workspace_coordinator::PublicationStatus::Published)
    );
    assert_eq!(
        coordinator.snapshot_after().read("a.rs"),
        Some(NEW_A.as_bytes()),
        "the new base holds the published content"
    );
}

// ---------------------------------------------------------------------------
// 2. idempotence : un rejeu du même publication_id ne republie jamais
// ---------------------------------------------------------------------------

#[test]
fn a_replayed_cycle_is_refused_as_already_published() {
    let repo = init_repo("cycle-replay");
    let journal = fresh_journal("cycle-replay");
    let publisher = writer(&repo, &journal);
    let mut coordinator = ChangeCoordinator::new(base_from_repo(&repo));
    let old_coordinator = coordinator.clone();

    let overlay = overlay_writing(&coordinator, "agent-cycle", "a.rs", NEW_A);
    let proposal = proposal(&coordinator, "agent-cycle", "t1", &["a.rs"], PATCH_A);
    let request = CycleRequest::new(&overlay, &proposal, "cycle-replay-1", "feat: replay");
    assert!(matches!(
        commit_cycle(
            &mut coordinator,
            &publisher,
            &TextualSemanticsOracle,
            &request
        ),
        CycleOutcome::Published { .. }
    ));
    let published_head = head(&repo);
    assert_eq!(commit_count(&repo), 2);

    // Rejeu avec le coordinateur d'avant publication (cas « crash avant adoption
    // de la nouvelle base »).
    let mut replay_coordinator = old_coordinator;
    let replay = commit_cycle(
        &mut replay_coordinator,
        &publisher,
        &TextualSemanticsOracle,
        &request,
    );
    match replay {
        CycleOutcome::AlreadyPublished {
            ref publication_id,
            ref commit,
        } => {
            assert_eq!(publication_id, "cycle-replay-1");
            assert_eq!(commit, &published_head);
        }
        other => panic!("expected AlreadyPublished, got {other:?}"),
    }
    assert_eq!(head(&repo), published_head, "no second publication");
    assert_eq!(commit_count(&repo), 2);
    assert!(
        replay_coordinator.applied_paths().is_empty(),
        "the replay left no phantom state"
    );
}

// ---------------------------------------------------------------------------
// 3. CAS base ↔ HEAD : une base qui n'est pas l'état Git est refusée
// ---------------------------------------------------------------------------

#[test]
fn a_base_that_diverges_from_head_is_refused_before_any_integration() {
    let repo = init_repo("cycle-stale-base");
    let journal = fresh_journal("cycle-stale-base");
    let publisher = writer(&repo, &journal);
    let mut coordinator = ChangeCoordinator::new(base_from_repo(&repo));
    let base_before = coordinator.base_id().to_owned();
    let head_before = head(&repo);

    // L'état Git avance sans que la base du coordinateur soit mise à jour.
    write_file(&repo, "a.rs", "fn a() { external }\n");
    git(&repo, &["add", "a.rs"]);
    git(&repo, &["commit", "-q", "-m", "external writer"]);

    let overlay = overlay_writing(&coordinator, "agent-cycle", "a.rs", NEW_A);
    let proposal = proposal(&coordinator, "agent-cycle", "t1", &["a.rs"], PATCH_A);
    let request = CycleRequest::new(&overlay, &proposal, "cycle-stale-1", "feat: stale");
    let outcome = commit_cycle(
        &mut coordinator,
        &publisher,
        &TextualSemanticsOracle,
        &request,
    );
    match outcome {
        CycleOutcome::StaleBase { ref reasons } => {
            assert!(
                reasons.iter().any(|reason| reason.contains("a.rs")),
                "the refusal must name the diverging path: {reasons:?}"
            );
        }
        other => panic!("expected StaleBase, got {other:?}"),
    }
    assert_eq!(coordinator.base_id(), base_before);
    assert!(coordinator.applied_paths().is_empty());
    assert_eq!(
        head(&repo),
        git(&repo, &["rev-parse", "HEAD"]).trim(),
        "head readable"
    );
    assert_ne!(head(&repo), head_before);
    assert_eq!(commit_count(&repo), 2, "no cycle publication");
}

// ---------------------------------------------------------------------------
// 4. refus sémantique : aucun état fantôme, aucune publication
// ---------------------------------------------------------------------------

#[test]
fn a_blocking_oracle_finding_publishes_nothing_and_leaves_no_phantom_state() {
    let repo = init_repo("cycle-oracle-refusal");
    // Base avec une assertion que la proposition supprime : l'oracle textuel
    // rapporte un affaiblissement de contrat bloquant.
    write_file(
        &repo,
        "a.rs",
        "fn a() -> u32 {\n    assert!(true);\n    1\n}\n",
    );
    git(&repo, &["add", "a.rs"]);
    git(&repo, &["commit", "-q", "-m", "contract base"]);
    let journal = fresh_journal("cycle-oracle-refusal");
    let publisher = writer(&repo, &journal);
    let mut coordinator = ChangeCoordinator::new(base_from_repo(&repo));
    let base_before = coordinator.base_id().to_owned();
    let head_before = head(&repo);
    let commits_before = commit_count(&repo);

    let new_content = "fn a() -> u32 {\n    1\n}\n";
    let overlay = overlay_writing(&coordinator, "agent-cycle", "a.rs", new_content);
    let patch = "+++ b/a.rs\n@@ -1,4 +1,3 @@\n fn a() -> u32 {\n-    assert!(true);\n     1\n }\n";
    let proposal = proposal(&coordinator, "agent-cycle", "t1", &["a.rs"], patch);
    let request = CycleRequest::new(&overlay, &proposal, "cycle-oracle-1", "feat: oracle");
    let outcome = commit_cycle(
        &mut coordinator,
        &publisher,
        &TextualSemanticsOracle,
        &request,
    );
    match outcome {
        CycleOutcome::VerificationRefused { ref oracle, .. } => {
            assert!(oracle.has_blocking(), "the report must stay blocking");
        }
        other => panic!("expected VerificationRefused, got {other:?}"),
    }
    assert_eq!(coordinator.base_id(), base_before, "no phantom base");
    assert!(coordinator.applied_paths().is_empty(), "no phantom path");
    assert!(coordinator.history().is_empty(), "no phantom history");
    assert_eq!(head(&repo), head_before, "nothing published");
    assert_eq!(commit_count(&repo), commits_before);
}

// ---------------------------------------------------------------------------
// 5. échec de publication : aucun état adopté, puis reprise unique
// ---------------------------------------------------------------------------

#[test]
fn a_refused_publication_leaves_no_phantom_state_and_retries_once() {
    let repo = init_repo("cycle-publish-refusal");
    let journal = fresh_journal("cycle-publish-refusal");
    let publisher = writer(&repo, &journal);
    let mut coordinator = ChangeCoordinator::new(base_from_repo(&repo));
    let base_before = coordinator.base_id().to_owned();
    let head_before = head(&repo);

    // Crochet pré-commit qui refuse : le point de commit n'est pas atteint.
    let hooks = repo.join(".git").join("hooks");
    std::fs::create_dir_all(&hooks).expect("hooks dir");
    let hook = hooks.join("pre-commit");
    std::fs::write(&hook, "#!/bin/sh\nexit 1\n").expect("write hook");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&hook).expect("metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&hook, permissions).expect("chmod hook");
    }

    let overlay = overlay_writing(&coordinator, "agent-cycle", "a.rs", NEW_A);
    let proposal = proposal(&coordinator, "agent-cycle", "t1", &["a.rs"], PATCH_A);
    let request = CycleRequest::new(&overlay, &proposal, "cycle-refusal-1", "feat: retry");
    let outcome = commit_cycle(
        &mut coordinator,
        &publisher,
        &TextualSemanticsOracle,
        &request,
    );
    match outcome {
        CycleOutcome::PublicationRefused { ref error, .. } => {
            assert!(
                matches!(
                    error,
                    ravum_workspace_coordinator::GitError::HookFailed { .. }
                ),
                "unexpected error: {error}"
            );
        }
        other => panic!("expected PublicationRefused, got {other:?}"),
    }
    assert_eq!(coordinator.base_id(), base_before);
    assert!(coordinator.applied_paths().is_empty());
    assert!(coordinator.history().is_empty());
    assert_eq!(head(&repo), head_before, "the reference did not move");
    assert_eq!(commit_count(&repo), 1);
    assert_eq!(read_file(&repo, "a.rs"), OLD_A);

    // Le crochet est retiré : le même cycle passe une seule fois.
    std::fs::remove_file(&hook).expect("remove hook");
    let outcome = commit_cycle(
        &mut coordinator,
        &publisher,
        &TextualSemanticsOracle,
        &request,
    );
    assert!(
        matches!(outcome, CycleOutcome::Published { .. }),
        "retry must publish: {outcome:?}"
    );
    assert_eq!(commit_count(&repo), 2, "exactly one publication");
    assert_eq!(read_file(&repo, "a.rs"), NEW_A);
    assert_eq!(status(&repo).trim(), "");
}

// ---------------------------------------------------------------------------
// 6. reprise après crash avant le swap : journal relu, puis rejeu unique
// ---------------------------------------------------------------------------

#[test]
fn an_intent_left_by_a_crash_before_the_swap_is_recovered_then_replayed_once() {
    let repo = init_repo("cycle-crash-before-swap");
    let journal = fresh_journal("cycle-crash-before-swap");
    let publisher = writer(&repo, &journal);
    let mut coordinator = ChangeCoordinator::new(base_from_repo(&repo));
    let head_before = head(&repo);

    // Simule le crash du cycle juste après `prepare` (arbre + commit candidat +
    // INTENT durable), avant la publication.
    let crashed_request = ravum_workspace_coordinator::PublishRequest::new("feat: crash")
        .with_change(
            ravum_workspace_coordinator::PathUpdate::write("a.rs", NEW_A)
                .with_expected_old_sha256(sha256_hex(OLD_A.as_bytes())),
        )
        .with_publication_id("cycle-crash-1")
        .with_trailer("Ravum-Agent", "agent-cycle");
    let prepared = publisher.prepare(&crashed_request).expect("prepare");
    drop(prepared);
    assert_eq!(head(&repo), head_before, "the crash did not publish");
    assert_eq!(commit_count(&repo), 1);

    let overlay = overlay_writing(&coordinator, "agent-cycle", "a.rs", NEW_A);
    let proposal = proposal(&coordinator, "agent-cycle", "t1", &["a.rs"], PATCH_A);
    let request = CycleRequest::new(&overlay, &proposal, "cycle-crash-1", "feat: crash")
        .with_trailer("Ravum-Agent", "agent-cycle");
    let outcome = commit_cycle(
        &mut coordinator,
        &publisher,
        &TextualSemanticsOracle,
        &request,
    );
    assert!(
        matches!(outcome, CycleOutcome::Published { .. }),
        "the recovered cycle must publish: {outcome:?}"
    );
    assert_eq!(
        commit_count(&repo),
        2,
        "exactly one publication after recovery"
    );
    assert_eq!(read_file(&repo, "a.rs"), NEW_A);
    assert_eq!(status(&repo).trim(), "");
}

// ---------------------------------------------------------------------------
// 7. changement de HEAD / writer concurrent : un seul succès
// ---------------------------------------------------------------------------

#[test]
fn an_expected_head_that_moved_is_refused_without_publication() {
    let repo = init_repo("cycle-head-moved");
    let journal = fresh_journal("cycle-head-moved");
    let publisher = writer(&repo, &journal);
    let mut coordinator = ChangeCoordinator::new(base_from_repo(&repo));
    let stale_head = head(&repo);
    write_file(&repo, "b.rs", "fn b() { external }\n");
    git(&repo, &["add", "b.rs"]);
    git(&repo, &["commit", "-q", "-m", "external head"]);
    let current = head(&repo);

    let overlay = overlay_writing(&coordinator, "agent-cycle", "a.rs", NEW_A);
    let mut proposal = proposal(&coordinator, "agent-cycle", "t1", &["a.rs"], PATCH_A);
    // La base de test contient l'ancien b.rs : aligner la base sur le HEAD courant
    // n'est pas le sujet ; on ne touche que a.rs, dont le contenu n'a pas changé.
    proposal.base_snapshot_id = SnapshotId::new(coordinator.base_id());
    let request = CycleRequest::new(&overlay, &proposal, "cycle-head-1", "feat: head")
        .with_expected_head(stale_head);
    let outcome = commit_cycle(
        &mut coordinator,
        &publisher,
        &TextualSemanticsOracle,
        &request,
    );
    match outcome {
        CycleOutcome::PublicationRefused { ref error, .. } => assert!(
            matches!(
                error,
                ravum_workspace_coordinator::GitError::ConcurrentWriter { .. }
            ),
            "unexpected error: {error}"
        ),
        other => panic!("expected PublicationRefused, got {other:?}"),
    }
    assert_eq!(head(&repo), current, "the moved HEAD is preserved");
    assert_eq!(commit_count(&repo), 2);
}

#[test]
fn concurrent_cycles_publish_at_most_one_and_never_corrupt_the_repository() {
    use std::sync::{Arc, Barrier};

    let repo = init_repo("cycle-concurrent");
    let journal = fresh_journal("cycle-concurrent");
    let publisher = Arc::new(writer(&repo, &journal));
    let coordinator = ChangeCoordinator::new(base_from_repo(&repo));
    let initial_head = head(&repo);
    let barrier = Arc::new(Barrier::new(2));
    let mut handles = Vec::new();
    for (path, content, patch, id) in [
        ("a.rs", NEW_A, PATCH_A, "cycle-concurrent-a"),
        ("b.rs", NEW_B, PATCH_B, "cycle-concurrent-b"),
    ] {
        let mut local = coordinator.clone();
        let publisher = Arc::clone(&publisher);
        let barrier = Arc::clone(&barrier);
        let initial_head = initial_head.clone();
        let path = path.to_owned();
        let content = content.to_owned();
        let patch = patch.to_owned();
        handles.push(std::thread::spawn(move || {
            let overlay = overlay_writing(&local, "agent-cycle", &path, &content);
            let proposal = proposal(&local, "agent-cycle", "t1", &[path.as_str()], &patch);
            let request = CycleRequest::new(&overlay, &proposal, id, "feat: concurrent")
                .with_expected_head(initial_head);
            barrier.wait();
            let outcome = commit_cycle(&mut local, &publisher, &TextualSemanticsOracle, &request);
            (outcome, local)
        }));
    }
    let results: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().expect("cycle thread"))
        .collect();
    let published = results
        .iter()
        .filter(|(outcome, _)| outcome.is_published())
        .count();
    assert_eq!(published, 1, "exactly one cycle may publish: {results:?}");
    for (outcome, local) in &results {
        if !outcome.is_published() {
            // Le perdant est refusé soit par le CAS du writer (`ConcurrentWriter`),
            // soit plus tôt par le CAS base ↔ HEAD (`StaleBase`). Jamais publié.
            match outcome {
                CycleOutcome::PublicationRefused { error, .. } => assert!(
                    matches!(
                        error,
                        ravum_workspace_coordinator::GitError::ConcurrentWriter { .. }
                    ),
                    "unexpected refusal: {error}"
                ),
                CycleOutcome::StaleBase { .. } => {}
                other => panic!("unexpected loser outcome: {other:?}"),
            }
            assert!(
                local.applied_paths().is_empty(),
                "the loser must not hold phantom state"
            );
        }
    }
    git(&repo, &["fsck", "--no-progress"]);
    assert_eq!(commit_count(&repo), 2, "one initial commit + one cycle");
    assert_eq!(status(&repo).trim(), "", "the loser wrote nothing");
    let a = read_file(&repo, "a.rs");
    let b = read_file(&repo, "b.rs");
    assert!(
        (a == NEW_A && b == OLD_B) || (a == OLD_A && b == NEW_B),
        "exactly one file published, a={a:?} b={b:?}"
    );
}

// ---------------------------------------------------------------------------
// 8. préconditions de demande : journal obligatoire, proposition cohérente
// ---------------------------------------------------------------------------

#[test]
fn a_cycle_without_a_durable_journal_is_refused() {
    let repo = init_repo("cycle-no-journal");
    let publisher = GitWriter::new(&repo, "agent-cycle", "agent-cycle@ravum.invalid");
    let mut coordinator = ChangeCoordinator::new(base_from_repo(&repo));
    let overlay = overlay_writing(&coordinator, "agent-cycle", "a.rs", NEW_A);
    let proposal = proposal(&coordinator, "agent-cycle", "t1", &["a.rs"], PATCH_A);
    let request = CycleRequest::new(
        &overlay,
        &proposal,
        "cycle-no-journal-1",
        "feat: no journal",
    );
    match commit_cycle(
        &mut coordinator,
        &publisher,
        &TextualSemanticsOracle,
        &request,
    ) {
        CycleOutcome::InvalidRequest { reasons } => assert!(
            reasons.iter().any(|reason| reason.contains("journal")),
            "the refusal must name the journal: {reasons:?}"
        ),
        other => panic!("expected InvalidRequest, got {other:?}"),
    }
    assert_eq!(commit_count(&repo), 1);
}

#[test]
fn a_proposal_describing_another_change_is_refused() {
    let repo = init_repo("cycle-incoherent");
    let journal = fresh_journal("cycle-incoherent");
    let publisher = writer(&repo, &journal);
    let mut coordinator = ChangeCoordinator::new(base_from_repo(&repo));
    let overlay = overlay_writing(&coordinator, "agent-cycle", "a.rs", NEW_A);
    // La proposition déclare b.rs alors que l'overlay change a.rs.
    let proposal = proposal(&coordinator, "agent-cycle", "t1", &["b.rs"], PATCH_B);
    let request = CycleRequest::new(
        &overlay,
        &proposal,
        "cycle-incoherent-1",
        "feat: incoherent",
    );
    match commit_cycle(
        &mut coordinator,
        &publisher,
        &TextualSemanticsOracle,
        &request,
    ) {
        CycleOutcome::InvalidRequest { reasons } => assert!(
            reasons.iter().any(|reason| reason.contains("differ")),
            "the refusal must explain the mismatch: {reasons:?}"
        ),
        other => panic!("expected InvalidRequest, got {other:?}"),
    }
    assert_eq!(commit_count(&repo), 1);
}

#[test]
fn a_declared_contract_impact_requires_a_human_decision() {
    let repo = init_repo("cycle-contract");
    let journal = fresh_journal("cycle-contract");
    let publisher = writer(&repo, &journal);
    let mut coordinator = ChangeCoordinator::new(base_from_repo(&repo));
    let overlay = overlay_writing(&coordinator, "agent-cycle", "a.rs", NEW_A);
    let mut proposal = proposal(&coordinator, "agent-cycle", "t1", &["a.rs"], PATCH_A);
    proposal.impacted_contracts = vec!["api/a".to_owned()];
    let request = CycleRequest::new(&overlay, &proposal, "cycle-contract-1", "feat: contract");
    match commit_cycle(
        &mut coordinator,
        &publisher,
        &TextualSemanticsOracle,
        &request,
    ) {
        CycleOutcome::HumanRequired { reasons } => assert!(
            reasons.iter().any(|reason| reason.contains("INV-02")),
            "the refusal must cite INV-02: {reasons:?}"
        ),
        other => panic!("expected HumanRequired, got {other:?}"),
    }
    assert_eq!(commit_count(&repo), 1);
}

// ---------------------------------------------------------------------------
// 9. crash après le swap : la publication est observée, jamais rejouée
// ---------------------------------------------------------------------------

#[test]
fn a_crash_after_the_reference_swap_is_recovered_as_published_and_not_replayed() {
    let repo = init_repo("cycle-crash-after-swap");
    let journal = fresh_journal("cycle-crash-after-swap");
    let publisher = writer(&repo, &journal);
    let mut coordinator = ChangeCoordinator::new(base_from_repo(&repo));
    let base_before = coordinator.base_id().to_owned();
    let before = head(&repo);

    // Simule le cycle tué après le compare-and-swap de la référence, avant
    // l'écriture de l'OUTCOME et la réconciliation de l'arbre de travail.
    let crashed_request = ravum_workspace_coordinator::PublishRequest::new("feat: crash")
        .with_change(
            ravum_workspace_coordinator::PathUpdate::write("a.rs", NEW_A)
                .with_expected_old_sha256(sha256_hex(OLD_A.as_bytes())),
        )
        .with_publication_id("cycle-crash-after-swap-1")
        .with_trailer("Ravum-Agent", "agent-cycle");
    let prepared = publisher.prepare(&crashed_request).expect("prepare");
    let candidate = prepared.new_commit().to_owned();
    git(
        &repo,
        &["update-ref", "refs/heads/main", &candidate, &before],
    );
    drop(prepared); // crash avant l'OUTCOME

    assert_eq!(head(&repo), candidate);
    assert_eq!(commit_count(&repo), 2);
    // La réconciliation post-swap n'a pas eu lieu : la divergence est visible.
    assert_eq!(read_file(&repo, "a.rs"), OLD_A);
    assert!(!status(&repo).trim().is_empty());

    // Redémarrage réel : un nouveau writer (donc un nouveau jeu d'intentions
    // vivantes) relit le journal.
    let restarted = writer(&repo, &journal);
    let overlay = overlay_writing(&coordinator, "agent-cycle", "a.rs", NEW_A);
    let proposal = proposal(&coordinator, "agent-cycle", "t1", &["a.rs"], PATCH_A);
    let request = CycleRequest::new(
        &overlay,
        &proposal,
        "cycle-crash-after-swap-1",
        "feat: crash",
    )
    .with_trailer("Ravum-Agent", "agent-cycle");
    match commit_cycle(
        &mut coordinator,
        &restarted,
        &TextualSemanticsOracle,
        &request,
    ) {
        CycleOutcome::AlreadyPublished {
            ref publication_id,
            ref commit,
        } => {
            assert_eq!(publication_id, "cycle-crash-after-swap-1");
            assert_eq!(commit, &candidate);
        }
        other => panic!("expected AlreadyPublished, got {other:?}"),
    }
    assert_eq!(head(&repo), candidate, "recovery never republishes");
    assert_eq!(commit_count(&repo), 2, "no second commit");
    assert_eq!(coordinator.base_id(), base_before, "no phantom base");
    assert!(coordinator.applied_paths().is_empty());
}

// ---------------------------------------------------------------------------
// 10. reprise impossible à expliquer : jamais rejouée en aveugle
// ---------------------------------------------------------------------------

#[test]
fn an_unexplained_previous_attempt_is_refused_instead_of_being_replayed() {
    let repo = init_repo("cycle-recovered-unknown");
    let journal = fresh_journal("cycle-recovered-unknown");
    let publisher = writer(&repo, &journal);
    let mut coordinator = ChangeCoordinator::new(base_from_repo(&repo));
    let before = head(&repo);

    // INTENT durable, puis crash avant le swap.
    let crashed_request = ravum_workspace_coordinator::PublishRequest::new("feat: unknown")
        .with_change(
            ravum_workspace_coordinator::PathUpdate::write("a.rs", NEW_A)
                .with_expected_old_sha256(sha256_hex(OLD_A.as_bytes())),
        )
        .with_publication_id("cycle-unknown-1")
        .with_trailer("Ravum-Agent", "agent-cycle");
    let prepared = publisher.prepare(&crashed_request).expect("prepare");
    drop(prepared);

    // Un writer externe avance HEAD sans contenir le commit candidat : la reprise
    // ne peut pas expliquer l'état de la référence.
    write_file(&repo, "b.rs", "fn b() { external }\n");
    git(&repo, &["add", "b.rs"]);
    git(&repo, &["commit", "-q", "-m", "external head"]);
    let current = head(&repo);
    assert_ne!(current, before);
    assert_eq!(commit_count(&repo), 2);

    let restarted = writer(&repo, &journal);
    let overlay = overlay_writing(&coordinator, "agent-cycle", "a.rs", NEW_A);
    let proposal = proposal(&coordinator, "agent-cycle", "t1", &["a.rs"], PATCH_A);
    let request = CycleRequest::new(&overlay, &proposal, "cycle-unknown-1", "feat: unknown")
        .with_trailer("Ravum-Agent", "agent-cycle");
    match commit_cycle(
        &mut coordinator,
        &restarted,
        &TextualSemanticsOracle,
        &request,
    ) {
        CycleOutcome::PublicationRefused {
            error: ravum_workspace_coordinator::GitError::Journal(reason),
            recovery: Some(report),
            ..
        } => {
            assert!(
                reason.contains("unexplained"),
                "the refusal must explain the unknown state: {reason}"
            );
            assert!(
                report
                    .unknown
                    .iter()
                    .any(|unknown| unknown.publication_id == "cycle-unknown-1"),
                "the recovery report must keep the unexplained intent: {report:?}"
            );
        }
        other => panic!("expected PublicationRefused(Journal), got {other:?}"),
    }
    assert_eq!(head(&repo), current, "nothing was published");
    assert_eq!(commit_count(&repo), 2, "no guessed publication");
    assert!(coordinator.applied_paths().is_empty());
}

// ---------------------------------------------------------------------------
// 11. suppression : le cycle publie aussi les suppressions
// ---------------------------------------------------------------------------

#[test]
fn a_deletion_cycle_publishes_the_removal() {
    let repo = init_repo("cycle-delete");
    let journal = fresh_journal("cycle-delete");
    let publisher = writer(&repo, &journal);
    let mut coordinator = ChangeCoordinator::new(base_from_repo(&repo));

    let mut overlay = Overlay::new(coordinator.base(), "agent-cycle");
    overlay.delete("b.rs").expect("valid path");
    let proposal = proposal(&coordinator, "agent-cycle", "t1", &["b.rs"], DELETE_B);
    let request = CycleRequest::new(&overlay, &proposal, "cycle-delete-1", "chore: delete b");
    let outcome = commit_cycle(
        &mut coordinator,
        &publisher,
        &TextualSemanticsOracle,
        &request,
    );
    assert!(
        matches!(outcome, CycleOutcome::Published { .. }),
        "unexpected outcome: {outcome:?}"
    );
    assert_eq!(commit_count(&repo), 2);
    assert!(
        !repo.join("b.rs").exists(),
        "the file is removed from the working tree"
    );
    assert!(
        !git_try(&repo, &["show", "HEAD:b.rs"]).status.success(),
        "the published tree no longer contains b.rs"
    );
    assert_eq!(status(&repo).trim(), "");
    assert!(coordinator.snapshot_after().read("b.rs").is_none());
}
