# MVP-0b — COLLAB-4 / L3.4 : analyse ciblée des changements acceptés

Statut : **TESTED** (exécution réelle + tests) — pas `PROVED`, pas une release.
Branche : `feat/mvp0b-hardening`, HEAD de départ vérifié : `a44b371`.
Date de campagne : 2026-11-08. Machine de référence chargée (autres agents en
parallèle) : mesures mono-exécution non représentatives, voir §Limites.

| Élément | Valeur observée |
|---|---|
| OS / arch | Windows / x86_64 |
| `rustc` | 1.95.0 (`59807616e1fa2540724bfbac14d7976d7e4a3860`, 2026-04-14) |
| host | `x86_64-pc-windows-msvc` |
| daemon | `ravum-daemon` 0.1.0, schéma 1, moteur `rustc-cli/metadata-v1` |
| preuve JSON | [`evidence/mvp0b-collab4-2026-11-08.json`](../../evidence/mvp0b-collab4-2026-11-08.json) |
| démo | `scripts/demo-collab4.ps1`, exécutée sous Windows PowerShell 5.1 (`powershell.exe -File …`) : exit 0 ; également vérifiée sous PowerShell 7 (`pwsh`) |

## 1. Objectif

Un changement **accepté** par le coordinateur collaboratif doit déclencher
**uniquement les analyses nécessaires** du daemon, en partageant entre les deux
composants les identités (contenu exact) et l'invalidation (graphe d'unités),
**sans second moteur de snapshots**.

Le code livré est un *composition root* dans `ravum-cli` :
`crates/ravum-cli/src/collab_cycle.rs`. Ni `ravum-workspace-coordinator`, ni
`ravum-daemon` ne sont modifiés ni couplés entre eux : `ravum-cli` assemble leurs
interfaces publiques (c'est la seule raison du passage de
`ravum-workspace-coordinator` de `[dev-dependencies]` à `[dependencies]` ; aucune
dépendance externe nouvelle).

## 2. Ce qui est partagé (et ce qui ne l'est pas)

Partagé :

1. **Le contenu, pas le chemin.** Le workspace est lu une seule fois par
   `ravum-snapshot` (`SourceSnapshot`). Chaque `AnalysisRequest` daemon est
   construit à partir des octets de ce snapshot (`content_of`), jamais relus
   depuis le disque ; le daemon ne relit aucun fichier utilisateur (contrat
   public : les fichiers sont fournis par l'appelant).
2. **L'identité de contenu.** Le coordinateur adresse un `SnapshotId` de
   workspace ; le daemon adresse un `snapshot_id = SHA-256(racine + contexte +
   toolchain + hash des fichiers de l'unité)`. Ces deux identifiants ne sont
   **jamais égaux par construction** : aucune égalité n'est revendiquée. La
   correspondance est **vérifiée** fichier par fichier (`file_hash()` du
   coordinateur vs `SnapshotFileId.sha256` calculé par le daemon depuis les
   octets soumis), et le rapport garde les deux hashs côte à côte.
3. **L'identité par rejeu.** Un second passage identique renvoie exactement le
   même `daemon_snapshot_id` et est servi par le cache de verdicts (mémoire puis
   disque) — c'est la preuve opérationnelle que l'identité est portée par le
   contenu, pas par le chemin ni l'horloge.
4. **L'invalidation.** Le graphe d'unités (crates + dépendances `path`)
   sélectionne les unités affectées ; le contenu inchangé d'un dépendant est un
   *hit*, le contenu modifié une *miss*. Les unités non affectées ne sont jamais
   soumises au daemon et sont listées explicitement.
5. **Le journal des décisions.** `Coordinator::with_journal` écrit une ligne
   JSON append-only par décision ; le cycle vérifie que le nombre de lignes
   durables, le nombre de décisions et les séquences `1..N` coïncident.
6. **Les enregistrements de changement.** `AcceptedChange` (snapshots
   avant/après, chemins, symboles, `journal_seq`, record de vérification) est
   consommé tel quel et recopié dans le rapport.

Non partagé (assumé) : aucun historique d'activation native, aucun artefact
construit, aucune vérification sémantique du merge (voir §7).

## 3. Commande et protocole

```text
ravum collab-cycle <workspace> [--proposals <dir>] [--daemon-work <dir>]
                   [--journal <path>] [--out <path>] [--passes N] [--json]
```

* `<workspace>` : racine lue récursivement (fichiers `.rs` + `Cargo.toml`,
  `target/`, `.git/` ignorés par `ravum-snapshot`). Défaut du mode démo :
  `fixtures/collab4/workspace`.
* `--proposals <dir>` : charge tous les `*.json` du dossier (format
  `ravum_protocol::ChangeProposal`, ordre lexicographique). Sans ce drapeau,
  exécute le **scénario intégré déterministe** (deux agents, deux fonctions
  différentes de `crate_a`).
* `--daemon-work <dir>` : état persistant du daemon (snapshots immuables, cache
  de verdicts, répertoires incrémentaux). Défaut :
  `target/ravum/collab4-daemon`. Le réutiliser d'un processus à l'autre est ce
  qui produit les *hits disque*.
* `--journal <path>` : journal durable du passage 1. Défaut :
  `target/ravum/collab4/journal.jsonl`. Les passages suivants écrivent
  `<stem>.pass<N><ext>` pour ne jamais tronquer d'historique.
* `--out <path>` : rapport JSON écrit (dossiers créés au besoin).
* `--passes N` (1..=8, défaut 1) : rejoue le cycle depuis la même base. Chaque
  passage recrée un coordinateur sur le snapshot initial ; le daemon, lui,
  persiste : le passage 2 doit être **entièrement** servi par le cache.
* `--json` : imprime le rapport JSON sur stdout (au lieu du résumé humain).

Déroulé d'un cycle :

```text
fichiers .rs + Cargo.toml
        │ (une seule lecture disque, ravum-snapshot)
        ▼
SourceSnapshot ──► UnitGraph (crates + deps path, fail-closed)
        │
        ├─► Coordinator::submit(proposition) ──► ProposalDecision + journal
        │        └─ AUTO_SAFE ──► AcceptedChange (snapshot_before/after, paths)
        │
        └─► unités affectées = unités modifiées + dépendants transitifs
                 │  (ordre topologique : dépendances d'abord)
                 ▼
            AnalysisRequest {root, files, BuildContext}  (contenu du snapshot)
                 │
                 ▼
            Daemon::analyze ──► AnalysisReport {snapshot_id, cache.kind,
                                timings, status, diagnostics}
                 │
                 ▼
            vérifications : identité calculée == identité rapportée,
            hashs de contenu coordinateur == daemon, statut fresh,
            error_count == 0, rejeu => hit
```

### Rapport JSON (`--out`)

Sections : `workspace_snapshot` (id + 6 fichiers), `unit_graph` (unités,
fichiers, racine, dépendances, dépendants, ordre topologique), `decisions`,
`accepted_changes`, `analyses` (une par changement accepté × unité affectée,
avec `coordinator_snapshot_before_id` / `_after_id`, `daemon_snapshot_id`,
`daemon_context_id`, `cache.kind`, `timings`, `daemon_error_count`,
`file_content_mapping` et le rapport daemon complet), `skipped_units`,
`passes` (analyses/hits/misses/rustc_runs/durée), `journal_paths`, `daemon.stats`,
`identity` (règles de vérification), `violations`, `exit_code`, `duration_ms`.

### Codes de sortie

| Code | Sens |
|---|---|
| 0 | cycle conforme : décisions `AUTO_SAFE`, analyses fraîches sans erreur, aucune unité non affectée analysée, identités vérifiées |
| 1 | décision non-`AUTO_SAFE` inattendue (proposition refusée/conflictuelle/manuelle) |
| 2 | usage, chargement, graphe illisible ou introuvable (fail-closed, avant toute analyse) |
| 3 | analyse d'une unité affectée en échec (diagnostics d'erreur, statut non `fresh`) |
| 4 | incohérence d'identité ou d'invariant (scope, hash de contenu, id daemon, journal) |

Codes testés réellement : 0, 1, 2, 3. Le code 4 est implémenté et couvert par les
tests de l'invariant de périmètre (`scope_violations`) ; il n'est pas déclenché
par une exécution réelle dans cette campagne.

## 4. Graphe d'unités (minimal, fail-closed)

* **Unité** = le `Cargo.toml` le plus proche d'un fichier. L'unité est
  identifiée par son dossier workspace-relatif ; `package.name` devient
  `--crate-name` (`-` → `_`, unicité vérifiée).
* **Fichiers de l'unité** : son `Cargo.toml` + tous les `.rs` dont le manifeste
  le plus proche est celui de l'unité. La racine d'analyse est `src/lib.rs`
  (ou `lib.rs`) : les unités binaires sont **refusées** (le daemon n'émet que
  des métadonnées de bibliothèque).
* **Arêtes** : uniquement les dépendances `path` de `[dependencies]`, sous forme
  inline (`x = { path = "..." }`) ou table (`[dependencies.x] path = "..."`).
  Les `dev-dependencies`/`build-dependencies`, les dépendances de registre, les
  features et les cibles conditionnelles ne sont **pas** modélisées
  volontairement.
* **Affectées** = unités possédant un chemin modifié, plus leurs dépendants
  transitifs. Ordre d'analyse = ordre topologique (dépendances d'abord).
* **Refus explicites** (aucune analyse « au hasard ») : workspace sans
  `Cargo.toml` ; manifeste sans `[package] name` ; ligne illisible ; cible de
  dépendance `path` sans `Cargo.toml` ; sortie du workspace (`..`) ; cycle de
  dépendances ; `.rs` hors de toute crate ; racine `src/lib.rs` absente ; noms
  de crate en collision. Le parseur TOML est un sous-ensemble minimal (pas de
  dépendance externe disponible) : tout ce qui n'est pas compris et qui
  ressemble à une dépendance est refusé.

## 5. Fixture `fixtures/collab4/workspace/`

Trois crates autonomes (chaque `Cargo.toml` contient un `[workspace]` vide : ils
ne sont jamais membres du workspace Ravum) :

* `crate_a` : `double()` et `shift()` indépendants + un test unitaire de
  propriété (reste vrai après les deux modifications du scénario) ;
* `crate_b` : dépend de `crate_a` par `path = "../crate_a"` ; son code de
  production est local à l'unité, l'usage réel de `crate_a` est dans son test
  (`#[cfg(test)]`) — l'analyse daemon (`--emit=metadata` d'une seule unité) ne
  lit pas `cfg(test)` et ne lie pas les dépendances ;
* `crate_c` : unité de contrôle indépendante, jamais affectée → doit être
  sautée.

Vérifications réelles (avec `CARGO_TARGET_DIR` hors fixture) :

```text
cargo check --manifest-path fixtures/collab4/workspace/crate_a/Cargo.toml   -> 0
cargo check --manifest-path fixtures/collab4/workspace/crate_b/Cargo.toml   -> 0
cargo check --manifest-path fixtures/collab4/workspace/crate_c/Cargo.toml   -> 0
cargo check --tests --manifest-path .../crate_b/Cargo.toml                  -> 0
cargo test --manifest-path .../crate_b/Cargo.toml
    test tests::composes_with_crate_a ... ok      (1 passed)
```

Scénario intégré : deux agents modifient chacun une fonction différente de
`crate_a/src/lib.rs`, sur la même base, avec les hashs attendus de la base
(vérification compare-and-swap du coordinateur). Les deux décisions sont
`AUTO_SAFE` ; les analyses attendues sont `crate_a` (modifiée) et `crate_b`
(dépendante), `crate_c` étant sautée. Le rapport l'enregistre explicitement
(`skipped_units`).

## 6. Résultats mesurés réels

Exécution : `powershell -ExecutionPolicy Bypass -File scripts/demo-collab4.ps1`
(dégrise le daemon `target/ravum/collab4-demo/daemon`, exit **0**).
Preuve versionnée : [`evidence/mvp0b-collab4-2026-11-08.json`](../../evidence/mvp0b-collab4-2026-11-08.json).

### Passage 1 (processus neuf, `--passes 2`)

| Élément | Valeur |
|---|---|
| décisions | 2 (`AUTO_SAFE`, `journal_seq` 1 et 2) |
| changements acceptés | 2 |
| analyses | 4 (2 changements × 2 unités affectées) |
| unités analysées | `crate_a` (changed), `crate_b` (dependent) |
| unités sautées | `crate_c` |
| hits / misses | 1 hit mémoire / 3 misses |
| `rustc_runs` | 3 |
| durée passage 1 | 1644 ms |
| durée passage 2 (rejeu mémoire) | 35 ms |
| durée totale du cycle | 2048 ms |

Détail des analyses du passage 1 (machine chargée : un `rustc` metadata a pris
938 ms sur ce run, contre ~140–165 ms sur des runs moins chargés — voir
§Limites) :

| changement | unité | rôle | cache | `analysis_ms` | `total_ms` | daemon `snapshot_id` |
|---|---|---|---|---|---|---|
| agent-a/tune-double | crate_a | changed | miss | 938 | 948 | `e84cebcf09a2…` |
| agent-a/tune-double | crate_b | dependent | miss | 303 | 311 | `49250b0337f6…` |
| agent-b/tune-shift | crate_a | changed | miss | 366 | 375 | `1bba4b1d1731…` |
| agent-b/tune-shift | crate_b | dependent | memory | 0 | 0 | `49250b0337f6…` |

Lecture : `crate_b` est *programmée* par le graphe aux deux changements, mais son
contenu est identique entre les deux : la seconde analyse est un **hit**. La
deuxième modification de `crate_a` produit un nouveau contenu : miss + `rustc`.
`daemon.stats` : `requests=8`, `cache_hits_memory=5`, `cache_misses=3`,
`rustc_runs=3`, `rustc_ms_total=1607`, `fresh=8`, `peak_active_jobs=1`.

Chaîne d'identités coordinateur : `aa7b74c18c79` (base) →
`c79059544d85` (après agent-a) → `bb0a7296bea9` (après agent-b) ; les deux
changements portent les mêmes `snapshot_before/after` dans `AcceptedChange` que
la chaîne observée.

### Passage 2 (second processus, même `--daemon-work`)

| Élément | Valeur |
|---|---|
| analyses | 4 |
| hits | 4 (3 disque + 1 mémoire) / misses 0 |
| `rustc_runs` | **0** |
| durée totale | 147 ms (ouverture daemon incluse), passage 10 ms |

C'est la preuve exigée : **contenu identique ⇒ aucune compilation**, y compris
après redémarrage du processus. Le cache disque persistant rejoue les verdicts
`(source, contexte, toolchain)`.

### Preuve d'identité par contenu

Pour chaque analyse, le rapport contient `file_content_mapping` : le hash
SHA-256 de chaque fichier du snapshot coordinateur et le hash calculé par le
daemon pour le même fichier logique, avec `equal: true` (2 fichiers par unité :
`Cargo.toml` + `src/lib.rs`). Les `snapshot_id` coordinateur et daemon restent
différents **par conception** (portée et formule différentes) ; le rapport
l'explique (`identity.identity_note`) au lieu de revendiquer une égalité non
vérifiée. Les `daemon_snapshot_id` sont stables pour un contenu identique entre
les passages (vérifié par test).

## 7. Tests

Aucun test existant n'a été supprimé, désactivé ou affaibli : les 22 tests
initiaux de `ravum-cli` passent inchangés. Nouveaux tests : 10 unitaires
(graphe + scénario, sans `rustc`) et 4 d'intégration (flux réel).

Commande exécutée (sérialisée à cause de la charge machine) :

```text
cargo test -p ravum-cli -- --test-threads=1
   bins ravum : 15 passed (5 préexistants + 10 collab_cycle)
   tests/collab_daemon_bridge.rs : 4 passed
   tests/collab_flow.rs          : 3 passed (préexistants)
   tests/m1_patch_loop.rs        : 10 passed (préexistants)
   tests/m2_proof_gate.rs        : 4 passed (préexistants)
   TOTAL : 36 passed, 0 failed
```

Tests ajoutés :

* `collab_cycle::tests::graph_discovers_units_and_path_dependent_edges` —
  trois unités, arête `crate_b -> crate_a`, ordre topologique.
* `affected_closure_is_direct_changed_plus_transitive_dependents` — chaîne
  `a -> b -> c` : affectées `{a,b,c}`, sautée `{d}`.
* `unreadable_manifest_fails_closed`, `missing_path_dependency_fails_closed`,
  `rust_file_outside_every_crate_fails_closed`,
  `dependency_cycle_fails_closed` — refus explicites.
* `builtin_scenario_is_auto_safe_and_targets_only_crate_a_plus_dependent` —
  2 `AUTO_SAFE`, périmètre `{crate_a, crate_b}`, `crate_c` sautée.
* `replacement_patch_applies_exactly_and_rejects_ambiguous_markers` — hunks
  exacts dérivés du contenu réel.
* `scope_validation_detects_unaffected_analysis_and_missing_analysis` —
  invariant « unité non affectée jamais analysée » (base du code de sortie 4).
* `manifest_parser_reads_package_edition_and_table_dependencies` — sous-ensemble
  TOML.
* `tests/collab_daemon_bridge.rs` :
  * `accepted_changes_analyse_only_affected_units_and_replay_hits_the_cache` —
    flux complet binaire réel : 8 analyses, `crate_c` sautée, identités de
    contenu vérifiées, passage 2 = 4 hits / 0 `rustc`, journal 2 lignes/passage ;
  * `a_second_process_is_served_by_the_persistent_disk_cache` — 2 processus
    successifs : second = 0 `rustc`, hits disque ;
  * `unreadable_graph_fails_closed_with_a_nonzero_exit_code` — exit 2 et
    message explicite, aucun snapshot daemon créé ;
  * `a_textually_accepted_change_that_fails_analysis_exits_nonzero_and_reports_it`
    — patch textuellement accepté produisant du Rust invalide : exit 3,
    violation `analysis_failed`, diagnostic enregistré, identité vérifiée quand
    même.

Tests négatifs exécutés à la main en complément (documentés, non versionnés
comme artefacts) :

* graphe illisible (dépendance `path` introuvable) → sortie 2, message
  « analyse refusée plutôt que devinée » ;
* proposition dont les hashs attendus sont faux → `REJECT` → sortie 1 ;
* patch textuellement valide mais Rust invalide → sortie 3.

## 8. Limites honnêtes

1. **Historique d'intégration ≠ historique d'activation native.** Le cycle
   journalise des décisions et des `AcceptedChange` ; il ne construit, ne
   vérifie ni n'active aucun artefact (pas de `GitWriter`, pas de `PatchHost`).
   Aucun résultat ici n'est une preuve de correction du code intégré.
2. **Pas de vérification sémantique.** Le coordinateur est utilisé en mode
   textuel (`Coordinator::new`) : `AUTO_SAFE` signifie « le patch s'applique
   exactement et ne recouvre pas un hunk intégré », et le record de vérification
   est explicitement `skipped`. L'analyse daemon vérifie la compilation
   (`--emit=metadata`) de l'unité, pas les tests, pas le comportement.
3. **L'analyse daemon est mono-unité.** Elle ne lie pas les dépendances
   (`--extern` absent) : `crate_b` est programmée par le graphe mais analysée sur
   ses propres octets. Une rupture d'API de `crate_a` ne serait donc pas
   détectée par l'analyse de `crate_b` dans cette démo (contenu inchangé ⇒ hit) ;
   le graphe est une politique de programmation, pas une propagation sémantique.
4. **Graphe limité aux `path`-deps de `[dependencies]`.** Pas de
   `dev`/`build-dependencies`, pas de dépendances de registre, pas de features,
   pas de cibles conditionnelles, pas de workspaces virtuels ; sous-ensemble
   TOML minimal ; tout ce qui n'est pas compris est refusé (exit 2).
5. **Pas de crash-test multi-processus.** Un seul processus CLI exécute le
   cycle ; le cache disque est prouvé par deux processus **successifs**, pas par
   des coordinateurs/daemons concurrents, pas de panne injectée, pas de
   récupération après crash.
6. **Mesures non représentatives.** Machine chargée (campagne multi-agents),
   mono-exécution, pas de p50/p95 ; les durées varient fortement avec la charge
   (le même `rustc` metadata a mesuré 134–165 ms sur certains runs et 938 ms sur
   le run de la preuve versionnée). Seuls les compteurs (analyses, hits,
   misses, `rustc_runs`, unités sautées) sont stables et constituent la preuve ;
   les durées sont indicatives.
7. **Unités limitées aux bibliothèques.** Racine `src/lib.rs`/`lib.rs`
   uniquement ; pas de binaire, pas de `[[bin]]`, pas de multiple racines.
8. `--passes > 1` écrit un journal par passage (`<journal>`, puis
   `<journal>.passN`) afin de ne jamais tronquer l'historique ; le journal du
   passage 1 est celui demandé.
9. Le daemon conserve son propre magasin de snapshots immuables : c'est sa
   provenance d'analyse, pas un second état du workspace — l'appelant ne lui
   fournit que des octets, jamais des chemins à relire.

## 9. Reproduction

```text
cargo build -p ravum-cli
cargo test -p ravum-cli -- --test-threads=1
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/demo-collab4.ps1
```

Ces trois commandes ont été exécutées réellement sur cette campagne (Windows
PowerShell 5.1 pour le script, code de sortie 0 ; `pwsh` 7 vérifié aussi). Le
script réinitialise `target/ravum/collab4-demo`, exécute le cycle à froid
(`--passes 2`, preuve JSON versionnée), puis un second processus sur le même
`--daemon-work` (cache disque, zéro `rustc`), vérifie les propriétés attendues et
projette le code de sortie. Sortie attendue : `OK: analyses ciblees, crate_c
sautee, hits de cache memoire et disque verifies.` et code 0.

Fichiers de cette livraison :

* `crates/ravum-cli/src/collab_cycle.rs` (nouveau, composition root + tests)
* `crates/ravum-cli/tests/collab_daemon_bridge.rs` (nouveau, 4 tests réels)
* `crates/ravum-cli/src/main.rs` (dispatch `collab-cycle` + usage)
* `crates/ravum-cli/Cargo.toml` (coordinator en dépendance normale)
* `fixtures/collab4/workspace/{crate_a,crate_b,crate_c}` (fixture)
* `scripts/demo-collab4.ps1` (démo, PowerShell 5.1 compatible)
* `docs/mvp/MVP-0b-collab-4.md` (ce document)
* `evidence/mvp0b-collab4-2026-11-08.json` (preuve d'une exécution réelle)

Aucun commit n'a été créé ; les changements restent non commités pour le Lead.
