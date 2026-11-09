# MVP-0c L3.2 — Robustesse de la publication Git sous crash, reprise et permissions

> **Objet.** Prouver, par injection réelle et scénarios de permissions, l'invariant central de la
> publication transactionnelle : *une publication en échec ne laisse aucun état canonique
> partiellement publié ; l'ancien état reste intact et le journal durable permet la reprise sans
> double publication*. Aucun code source du coordinateur n'a été modifié.
>
> | Champ | Valeur |
> |---|---|
> | Tâche | `task-20` — MVP-0c L3.2 (mandat MVP-0c L3.4) |
> | Date | 2026-11-09 ~01:20–01:45 (Europe/Paris) |
> | Branche / HEAD | `feat/mvp0c-hardening` @ `58220f5b351668b3d6b2b2ae7772534bb9628a5a` |
> | Code source modifié | **aucun** (`crates/ravum-workspace-coordinator/src/**` intact) |
> | Fichiers produits | `tests/git_publication_crash.rs` (nouveau), cette doc, `evidence/mvp0c-publication-2026-11-09.json` |
> | Résultat | **134 tests verts / 0 échec** (119 existants préservés + 15 nouveaux) |

## 0. Contraintes respectées

- Aucune commande git mutante sur le dépôt Ravum : la branche, les refs et le worktree n'ont pas été
  touchés par l'agent. Les seules commandes git exécutées sur `D:\App\Ravum` sont en lecture
  (`status`, `diff`, `rev-parse`, `ls-tree`, `log`) ; **tous** les dépôts mutés par les tests sont
  des dépôts jetables créés sous `target/ravum-tests/`.
- `CARGO_BUILD_JOBS=2`, `--test-threads=1` ; rustfmt appliqué au fichier de test ; **0 warning
  compilateur** introduit ; le fichier de test n'ajoute **aucun** warning clippy (les warnings
  clippy préexistants du crate source sont listés en §8 et n'ont pas été touchés).
- Aucun test existant retiré ni affaibli : `tests/git_publication.rs` (16 tests), `tests/git_writer.rs`
  (2), `tests/journal_durability.rs` (3), `collab*` (20), `merge_strategies` (2) et les 76 tests
  unitaires sont inchangés et verts.

## 1. Méthode : injection de crash réelle, pas une simulation

Le writer exécute lui-même `pre-commit`/`commit-msg` et Git exécute nativement
`reference-transaction`/`post-commit`. Chaque hook peut donc **terminer le processus de publication**
à un point précis de la transaction :

```
prepare : index isolé -> blobs -> write-tree -> hooks -> commit-tree -> journal INTENT
publish : contrôles HEAD/disque -> update-ref CAS -> working tree -> index réel -> journal OUTCOME -> post-commit
```

Le test parent relance le **binaire de test courant** (`--exact crash_injection_child_entry`) avec un
scénario, un dépôt et un chemin de journal. L'enfant écrit son PID dans un fichier ; le hook installé
appelle un helper OS (`Stop-Process -Force` sous Windows, `kill -9` sous POSIX) qui tue réellement
l'enfant, puis échoue. Le parent vérifie que :

1. l'enfant s'est terminé **en échec** (tué) ;
2. le marqueur `completed` **n'existe pas** (sinon le harnais n'a pas injecté et le test échoue
   explicitement — pas de faux vert) ;
3. le **journal de hooks** contient la trace de l'étape (preuve d'exécution réelle) ;
4. l'état du dépôt respecte l'invariant propre à l'étape.

Un **test de contrôle sans hook** (`crash_harness_control_without_hooks_publishes_exactly_once`)
vérifie que le harnais distingue bien « tué » de « publication complétée » : sans hook, l'enfant va
au bout, publie exactement une fois et le retire est refusé `AlreadyPublished`.

### 1.1 Étapes de crash et état laissé

| Étape | Hook qui tue | Point exact | État canonique observé | Reprise prouvée |
|---|---|---|---|---|
| `pre_commit` | `pre-commit` | arbre candidat construit, avant le commit | HEAD = ancien, **aucun INTENT** | publication ultérieure propre, 1 seul commit ajouté |
| `commit_msg` | `commit-msg` | hooks exécutés, avant `commit-tree` | HEAD = ancien, **aucun INTENT** | idem |
| `cas_prepared` | `reference-transaction` (`prepared`) | après `commit-tree` + INTENT, dans `update-ref` **avant** le swap | HEAD = ancien, **1 INTENT** durable, commit candidat non atteignable | `recover()` ⇒ `RECOVERED_UNPUBLISHED` ; rejeu ⇒ 1 publication ; 2e `recover()` no-op |
| `cas_committed` | `reference-transaction` (`committed`) | **après** le swap, avant working tree/index/OUTCOME | HEAD = **commit candidat**, arbre complet, 1 INTENT sans OUTCOME, working tree/index **non réconciliés mais visibles** | `recover()` ⇒ `RECOVERED_PUBLISHED` ; retry ⇒ `AlreadyPublished` ; travail non lié publié ensuite sans écraser le fichier divergent |
| `post_commit` | `post-commit` | publication **complète** (OUTCOME durable) | HEAD = candidat, index/worktree réconciliés, OUTCOME `PUBLISHED` | `recover()` vide ; retry ⇒ `AlreadyPublished` |

**Invariant global vérifié après chaque crash** : le canonical (`HEAD`, objets, journal) est soit
« non publié » (ancien HEAD intact, aucun INTENT ou INTENT résolu `RECOVERED_UNPUBLISHED`), soit
« entièrement publié » (HEAD = commit candidat, arbre contenant le blob cible, OUTCOME unique,
retry `AlreadyPublished`). L'état intermédiaire « ref bougée mais commit incomplet » est impossible :
blobs/arbre/commit sont créés **avant** le CAS, et le CAS est l'unique point de commit. Tout résidu
post-CAS (working tree/index) est rendu visible (`git status`, `recover()`), jamais silencieux.

## 2. Couverture des cas du mandat (12 cas MVP-0b + exigences L3.4)

`existant` = test déjà présent dans `tests/git_publication.rs` et ré-exécuté ici ; `nouveau` =
`tests/git_publication_crash.rs`. Le détail machine est dans le JSON de preuve (`coverage`).

| # | Cas du mandat | Test | Type | Invariant prouvé |
|---|---|---|---|---|
| 1 | Écrivain externe concurrent (HEAD bouge) | `external_head_move_between_prepare_and_publish_is_refused` | existant | CAS refusé, ref intacte, candidat non atteignable |
| 2 | Deux propositions simultanées | `concurrent_proposals_publish_at_most_one` | existant | au plus une publication, l'autre `ConcurrentWriter`, worktree cohérent |
| 3 | Crash entre commit-tree et update-ref | `crash_before_reference_swap_is_recovered_as_unpublished_without_double_publication` | **nouveau** | HEAD intact, INTENT pendant, `recover()` ⇒ `RECOVERED_UNPUBLISHED`, reprise 1 fois |
| 4 | Crash avant commit-tree (`pre-commit`) | `crash_in_pre_commit_hook_publishes_nothing_and_restarts_cleanly` | **nouveau** | rien publié, aucun INTENT, redémarrage propre |
| 5 | Crash avant commit-tree (`commit-msg`) | `crash_in_commit_msg_hook_publishes_nothing_and_restarts_cleanly` | **nouveau** | rien publié, aucun INTENT, redémarrage propre |
| 6 | Crash après update-ref (journal partiel) | `crash_after_reference_swap_is_recovered_as_published_and_never_republished` | **nouveau** | HEAD = candidat, arbre complet, OUTCOME absent résolu `RECOVERED_PUBLISHED`, jamais republié |
| 7 | Redémarrage puis `recover()` | `journal_failure_after_swap_is_recovered_without_double_publication` + tests #3/#6 | **nouveau** | journal read-only après swap ⇒ erreur `Journal`, puis `recover()` répare la ligne d'issue, retry refusé |
| 8 | Verrou de maintenance présent | `maintenance_index_lock_blocks_reconciliation_without_partial_publication` | **nouveau** | commit publié, `IndexOutOfSync` explicite, réparation documentée, aucun état partiel |
| 9 | Verrou de référence concurrent | `concurrent_reference_lock_is_refused_before_any_write` | **nouveau** | ref intacte, aucune écriture worktree avant le point de commit, refus explicite, reprise 1 fois |
| 10 | Hook local refuse | `refusing_pre_commit_hook_blocks_publication_and_really_runs` | existant | hook réellement exécuté, ref intacte |
| 11 | Hook local modifie le message | `commit_msg_hook_is_executed_and_its_rewrite_is_published` | existant | message réécrit publié, attribution conservée |
| 12 | Hook `reference-transaction` refuse | `reference_transaction_hook_can_abort_the_publication` | existant | ref intacte |
| 13 | Permissions refusées sur `.git` | `protected_git_directory_refuses_publication_without_touching_the_reference` | **nouveau** | ref intacte, `REFUSED` journalisé, état sain, publication après restauration |
| 14 | Fichier en lecture seule (écriture) | `read_only_worktree_file_is_reported_as_warning_not_silently_overwritten` | **nouveau** | commit+index complets, fichier non écrasé, warning explicite, `git status` visible |
| 15 | Suppression refusée par permissions | `delete_denied_by_permissions_is_reported_and_canonical_state_stays_published` | **nouveau** | arbre publié sans le fichier, index réconcilié, unlink échoué ⇒ warning, fichier conservé |
| 16 | Fichier généré (`target/`, `BOARD.*`, `LOG.md`) déjà modifié | `modified_generated_and_auxiliary_files_are_preserved_by_publication` | **nouveau** | contenus préservés octet pour octet, jamais supprimés ; contenu généré protégé refusé sans décision explicite |
| 17 | Fichier non lié modifié | `unrelated_work_in_progress_is_preserved` | existant | fichier/index non concernés intacts |
| 18 | Renommage / suppression / ajout | `additions_deletions_and_renames_produce_the_expected_tree` | existant | arbre correct, worktree/index cohérents |
| 19 | Fichier modifié entre validation et publication | `a_file_modified_between_validation_and_publication_is_refused` | existant | `StaleWorkingTree`, aucune publication, contenu externe préservé |
| 20 | Agent écrivant dans `.git` (writer) | `paths_inside_git_metadata_are_refused_by_the_writer` | existant | `UnsafePath`, `.git` intact |
| 21 | Proposition ciblant `.git` (coordinateur) | `a_proposal_targeting_git_metadata_is_rejected_by_the_coordinator` | existant | `REJECT`, snapshot inchangé |
| 22 | Double publication de la même proposition | `a_restart_does_not_publish_twice_and_recovers_an_interrupted_swap` | existant | `AlreadyPublished`, HEAD/compteur inchangés |
| 23 | Contenu généré : opt-in explicite | `generated_content_requires_an_explicit_decision` | existant | `ProtectedPath` par défaut, publication seulement avec décision explicite |
| 24 | Crash complet après publication | `crash_in_post_commit_hook_leaves_fully_published_state` | **nouveau** | état entièrement publié, retry refusé |
| 25 | Contrôle du harnais | `crash_harness_control_without_hooks_publishes_exactly_once` | **nouveau** | le harnais distingue tué/complété, publication unique |
| 26 | Journal inécrivable avant publication | `unwritable_journal_before_publication_publishes_nothing` | **nouveau** | `Journal` fatal, rien publié, journal manquant ⇒ refus fail-closed, reprise après réparation |
| 27 | `prepare()` ne publie jamais | `crash_injection_child_entry` (mode normal) | **nouveau** | HEAD inchangé, aucun commit publié |

Les 12 cas du mandat MVP-0b sont tous couverts (lignes 1–12, 16–23) ; les exigences L3.4
(permissions fichier/`.git`, journal partiel des deux côtés du point de commit, verrous de
maintenance/référence, préservation des fichiers générés) étendent la matrice (lignes 3–9, 13–16,
24–27).

## 3. Résultats chiffrés (commande exacte)

```powershell
$env:CARGO_BUILD_JOBS='2'
cargo test -p ravum-workspace-coordinator -- --test-threads=1
```

| Cible | Avant (audit) | Après | Résultat |
|---|---:|---:|---|
| `unittests src/lib.rs` | 76 | 76 | 76/76 ok |
| `collab1_non_regression` | 2 | 2 | ok |
| `collab2_two_functions` | 3 | 3 | ok |
| `collab3_semantic` | 10 | 10 | ok |
| `collab5_semantic_statuses` | 5 | 5 | ok |
| `git_publication` | 16 | 16 | ok |
| **`git_publication_crash` (nouveau)** | 0 | **15** | **15/15 ok** (~71 s) |
| `git_writer` | 2 | 2 | ok |
| `journal_durability` | 3 | 3 | ok |
| `merge_strategies` | 2 | 2 | ok |
| **Total** | **119** | **134** | **134 passés / 0 échoué** |

Environnement : Windows 10.0.26200, `rustc 1.95.0`, `cargo 1.95.0`, `CARGO_BUILD_JOBS=2`,
`--test-threads=1`. Durées indicatives de la dernière exécution : `git_publication_crash` 71,0 s,
`git_publication` 35,0 s, `collab3` 13,8 s, `collab2` 8,7 s, `merge_strategies` 5,9 s, `collab1` 4,0 s.
Preuve machine : `evidence/mvp0c-publication-2026-11-09.json` (liste des 15 tests, matrice de
couverture à 27 lignes, compteurs par binaire, environnement, limites).

## 4. Ce que prouvent les scénarios de permissions (détail)

- **Fichier de travail en lecture seule** : le point de commit (`update-ref CAS`) est déjà passé ;
  l'écriture du working tree échoue et est restituée en **warning** dans `CommitResult` ; le commit
  publié et l'index réel sont complets, le fichier disque n'est pas écrasé, `git status` montre la
  divergence, le journal contient `PUBLISHED`. Le rejeu est refusé (`AlreadyPublished`).
- **Suppression refusée** (verrou de partage Windows / répertoire parent non inscriptible POSIX) :
  l'arbre publié ne contient plus le fichier, l'index est réconcilié, l'unlink échoue ⇒ warning, le
  fichier disque reste présent et visible ; aucun état canonique partiel.
- **`.git` protégé** : refus explicite *avant* toute écriture de working tree ; ref intacte, journal
  `REFUSED`, arbre sain (`git fsck` OK) ; après restauration des permissions, la même publication
  réussit exactement une fois.
- **Journal** : inécrivable avant publication ⇒ refus `Journal`, rien publié ; journal **manquant**
  ⇒ également fail-closed (refus), puis reprise après recréation explicite ; inécrivable **après**
  le swap ⇒ `Journal` renvoyé, commit pleinement publié, INTENT durable sans OUTCOME, puis
  `recover()` ⇒ `RECOVERED_PUBLISHED`, retry refusé — jamais de double publication.
- **Verrous** : `index.lock` (verrou de maintenance Git) ⇒ commit publié + `IndexOutOfSync` explicite
  (réparation documentée `git reset --mixed HEAD`) ; verrou de référence ⇒ refus avant écriture,
  ref intacte, journal `REFUSED`, reprise unique.
- **Fichiers générés** : `BOARD.md`, `LOG.md`, `target/ravum/board.json` déjà modifiés ne sont ni
  supprimés ni écrasés par une publication non liée (contenus identiques octet pour octet, index
  inchangé) ; les chemins générés **protégés par la politique** (lockfiles) sont refusés sans
  décision explicite et ne sont jamais créés/écrasés silencieusement.

## 5. Aucun défaut de sûreté découvert

Aucun test n'a révélé de perte d'état canonique, de publication partielle, de double publication ou
d'écrasement silencieux. Deux **observations non bloquantes** (pas des défauts de sûreté) sont
consignées dans la preuve JSON ; elles ne justifient **aucune** modification de source dans ce lot :

- **OBS-1** — Quand la référence est verrouillée par un autre processus (`refs/heads/main.lock`),
  `update-ref` échoue et le writer renvoie `ConcurrentWriter` avec `found == expected_old` : l'étiquette
  est conservatrice (HEAD n'a pas bougé), le refus est sûr et sans écriture. Reclasser ce cas
  demanderait un changement de source (hors périmètre L3.2).
- **OBS-2** — `target/`, `BOARD.*`, `LOG.md` ne figurent pas dans `GENERATED_PATTERNS` de
  `path_policy` : ils ne sont pas *refusés* par la politique de fichiers générés. Les tests prouvent
  leur **préservation** (jamais de suppression/écrasement silencieux) et le refus des motifs protégés
  existants. Ajouter ces motifs serait un changement de politique, volontairement non effectué.

## 6. Reproductibilité

```text
# suite complète (preuve principale)
CARGO_BUILD_JOBS=2 cargo test -p ravum-workspace-coordinator -- --test-threads=1

# fichier d'injection seulement
CARGO_BUILD_JOBS=2 cargo test -p ravum-workspace-coordinator --test git_publication_crash -- --test-threads=1

# formatage / lint du fichier ajouté
rustfmt --edition 2021 crates/ravum-workspace-coordinator/tests/git_publication_crash.rs
cargo clippy -p ravum-workspace-coordinator --all-targets   # aucun warning pour le nouveau fichier
```

Fichiers : `crates/ravum-workspace-coordinator/tests/git_publication_crash.rs` (nouveau, ~1 500
lignes avec helpers), `docs/mvp/MVP-0c-publication-robustness.md` (ce document),
`evidence/mvp0c-publication-2026-11-09.json` (preuve JSON). `tests/git_publication.rs` n'a **pas**
été modifié (aucun ajout nécessaire : les cas existants restent la référence). Aucun fichier source,
`Cargo.toml`, `.github` ou autre crate modifié.

## 7. Limites et non-claims

1. L'injection est une **terminaison réelle du processus** à des étapes choisies ; ce n'est pas une
   coupure d'alimentation ni une perte de données non flushées au niveau OS/FS : le crash test ne
   remplace pas un test de durabilité matérielle.
2. Les hooks qui tuent sont des artefacts de test sous `target/ravum-tests/` ; **aucun** hook n'est
   installé dans le dépôt Ravum et aucun réglage Git global n'est modifié.
3. Les scénarios de permissions Windows (ACL `icacls`, attribut lecture seule, partage de fichier) et
   POSIX (`chmod`) sont écrits pour les deux plateformes, mais n'ont été **exécutés que sur Windows**
   (machine de référence) ; les branches POSIX sont fournies mais non exercées ici.
4. Il n'existe pas de « verrou de maintenance » métier dans le crate ; le cas est couvert par les
   **verrous Git** (`index.lock`, verrou de référence), proxy documenté et suffisant pour prouver le
   fail-closed.
5. Pas de stress multi-processus répétitif au-delà des tests de concurrence existants ; les durées
   dépendent de la machine et de la charge.
6. Les tests prouvent les propriétés observables de l'implémentation actuelle ; ils ne prouvent pas
   l'absence générale de race (aucun test ne le peut) ni la durabilité d'un journal sur un disque
   défaillant sous le contrôle du noyau.
