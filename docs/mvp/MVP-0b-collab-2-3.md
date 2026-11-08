# MVP-0b / COLLAB-2 & COLLAB-3 — même fichier multi-symboles, conflits sémantiques, vérification réelle du fusionné

Statut : **implémenté, mesuré, tests exécutés localement** (MVP-0b, branche
`feat/mvp0b-hardening`). Aucun commit n'est créé par ce lot : le Lead committe.

Preuves de cette page : commandes exécutées sur la machine de référence
(Windows, 10 cœurs / 16 threads, ~16 Go de RAM dont ~3,7 Go libres pendant la
campagne, plusieurs agents compilant en parallèle), `rustc 1.95.0
(59807616e 2026-04-14)`, `cargo 1.95.0`, `git 2.51.1.windows.1`. Toute la
campagne est **sérialisée** (`--test-threads=1`, un seul build à la fois). Les
durées varient avec la charge : elles sont donc données à titre indicatif.

---

## 1. Objectif et périmètre

COLLAB-1 démontrait : propositions, CAS par hash, refus explicites, publication
Git transactionnelle. Deux limites restaient ouvertes :

1. **COLLAB-2** — deux agents qui modifient deux symboles *indépendants* du même
   fichier doivent être intégrés ensemble, et la fusion doit être **réellement
   compilée** avant d'être considérée comme acceptée. Une fusion textuellement
   propre mais qui ne compile pas (interaction entre deux changements en apparence
   indépendants) doit être refusée, pas publiée.
2. **COLLAB-3** — dix familles de conflits sémantiques (même fonction deux fois,
   signature publique, appelant incompatible, contrat affaibli, dépendance non
   déclarée, import ambigu, ABI, lockfile, régression fonctionnelle, snapshot
   périmé) doivent produire des verdicts explicites avec raison et preuve.

Périmètre d'écriture de ce lot : `crates/ravum-workspace-coordinator/**`,
`fixtures/collab2/**`, `scripts/demo-collab2.ps1`, ce document. Aucun autre crate
n'est modifié.

---

## 2. Protocole d'intégration et de vérification

### 2.1 Pipeline

```
proposition JSON
   │
   ▼
[1] évaluation textuelle (COLLAB-1, inchangée)
    chemins sûrs → lockfiles/générés → patch parsable → CAS par hash
    → chevauchement symboles/lignes réellement modifiées → application exacte
    → déclarations (dépendances, contrats) → base périmée
   │  candidat AUTO_SAFE
   ▼
[2] gate sémantique (NOUVEAU, opt-in)
    a. politique (heuristiques) : surface publique modifiée, ABI modifiée,
       assertion de contrat retirée, dépendance non déclarée sur un symbole
       modifié par un autre agent           → MANUAL_REQUIRED + preuve
    b. plan de vérification dérivé du changement :
         - MetadataCheck TOUJOURS exigé ;
         - TestCompile + TestRun si le changement touche un corps de fonction
           ET que le snapshot courant contient du code de test.
    c. MergeVerifier injecté (implémentation réelle : rustc CLI public) :
         Passed      → intégration, AcceptedChange journalisé
         Failed      → REJECT avec code/ diagnostic/ evidence id
         Unavailable → MANUAL_REQUIRED (fail-closed)
   │
   ▼
[3] overlay en mémoire → snapshot avant/après content-adressé
    → record AcceptedChange (API L2/COLLAB-4)
   │
   ▼
[4] publication Git transactionnelle (GitWriter existant, inchangé)
    index isolé, commit non publié, update-ref CAS, hooks honorés,
    journal de publication idempotent + trailers Ravum-Evidence
```

Une fusion **textuellement propre n'est jamais `AUTO_SAFE`** si la vérification
sémantique est demandée : soit elle passe (avec preuve), soit elle échoue
(`REJECT`), soit elle est impossible (`MANUAL_REQUIRED`). Le mode par défaut du
binaire reste **le comportement COLLAB-1** (voir §2.4).

### 2.2 API ajoutée

| Élément | Rôle |
|---|---|
| `verifier::MergeVerifier` | Trait d'injection (`Send + Sync`) : `verify(&VerificationRequest) -> VerificationOutcome`. **Le crate coordinateur n'importe aucun compilateur ni adaptateur** ; le binaire injecte l'implémentation. |
| `verifier::VerificationRequest` | snapshots avant/après (exacts), chemins, symboles déclarés, `required_checks` calculés par le coordinateur (jamais par l'agent). |
| `verifier::VerificationOutcome` | `Passed{checks}` / `Failed{check, reason}` / `Unavailable{reason}`. |
| `verifier::missing_required_checks` | un verifier qui prétend `Passed` en omettant un check requis est refusé (`MANUAL_REQUIRED`, « proof incomplete »). |
| `records::MergeCheckKind` | `metadata_check`, `test_compile`, `test_run`. |
| `records::MergeCheckStatus` | `passed`, `failed`, `unavailable`, `skipped`. |
| `records::MergeCheck` | kind/statut/outil/version/cible/snapshot/diagnostics + `evidence_id` déterministe (sha256, 64 hex). |
| `records::VerificationRecord` | record agrégé (checks requis, statut, evidence id) attaché à chaque changement accepté. |
| `records::AcceptedChange` | API L2/COLLAB-4 (§6). |
| `coordinator::Coordinator::with_merge_verifier` / `semantic` | active le mode sémantique ; `Coordinator::new` reste le mode COLLAB-1 (compatibilité). |
| `coordinator::Coordinator::accepted_changes()` | records des changements acceptés, dans l'ordre d'intégration. |
| `semantics` (module public) | heuristiques : spans de symboles, lignes réellement modifiées, surface publique/ABI, assertions retirées, dépendance non déclarée. |

**Implémentation réelle du verifier** : `src/bin/common/mod.rs`, partagée par les
binaires du paquet. Elle matérialise le snapshot candidat dans son répertoire de
travail et exécute le CLI public :

* `rustc --edition 2021 --crate-type lib --emit=metadata --error-format=json`
  sur la racine de crate inférée (`src/lib.rs` > `lib.rs` > `src/main.rs` >
  `main.rs` > unique `.rs`) — jamais `rustc_private`, jamais `RUSTC_BOOTSTRAP` ;
  `RUSTFLAGS`/`RUSTC_WRAPPER` sont retirés de l'environnement enfant ;
* `rustc --test` puis **exécution** du binaire de test, uniquement avec
  `--verify-tests` (le code du snapshot est exécuté : ce n'est pas un bac à
  sable ; timeout de 30 s ; un timeout est `Unavailable`, jamais un succès).

L'identité toolchain/hôte est lue via `ravum-rust-adapter` (source unique), mais
la compilation et le parsing des diagnostics se font avec le CLI public : voir la
limite §8.1 (schéma JSON `$message_type` de rustc 1.95 non reconnu par le
parseur actuel de l'adaptateur).

### 2.3 Ce que le gate ajoute au journal

Chaque décision journalisée garde ses raisons, complétées par :

* `semantic verification: passed (metadata_check=passed, ... evidence <64 hex>)` ;
* une ligne par check : `metadata_check: rustc 1.95.0: candidate snapshot
  compiles (...) [passed] evidence <64 hex>` ;
* en cas d'échec, `semantic verification failed: ...`, le check fautif, son
  evidence id, puis jusqu'à 5 diagnostics `code [level] message` (ex. `E0428
  [error] the name 'helper2' is defined multiple times`) ;
* en cas d'impossibilité, `semantic verification required but unavailable: ...` +
  `fail-closed` ;
* pour les refus de politique : la raison, la ligne concernée et
  `semantic policy evidence: <64 hex>`.

La publication Git ajoute, pour chaque changement accepté, des trailers
`Ravum-Evidence` (record agrégé) et `Ravum-Check-Evidence` (un par check).

### 2.4 Modes et non-régression COLLAB-1

| Invocation | Mode | `AUTO_SAFE` signifie |
|---|---|---|
| `ravum-collab integrate ...` (défaut) | **textuel (COLLAB-1)** | patch exact, pas de chevauchement — **pas** « compilé ». Le record AcceptedChange a `verification.status = skipped`. |
| `... --verify` | sémantique | checks requis passés sur le snapshot fusionné exact. |
| `... --verify-tests` | sémantique + tests | idem + compilation/exécution du harnais de test. |

La vérification est **opt-in** : les appelants COLLAB-1 et la démo historique
(`scripts/demo-collab.ps1`, workspace de deux `.rs` sans `Cargo.toml`, donc non
compilable comme crate) doivent conserver exactement leurs décisions. Quand la
vérification est demandée sur un tel workspace, le résultat est `MANUAL_REQUIRED`
avec `cannot infer a single crate root ...`, jamais un succès silencieux.

Non-régression verrouillée par
`tests/collab1_non_regression.rs` :
* sans opt-in : `AUTO_SAFE=2` sur le fixture COLLAB-1 (2 fichiers `.rs`, pas de
  `Cargo.toml`) et publication des deux changements ;
* avec `--verify` sur le même workspace : `MANUAL_REQUIRED`, rien de publié.

---

## 3. Corpus de scénarios (fusion)

`fixtures/collab2/corpus/<id>/` contient `base.rs`, `a.patch`, `b.patch`,
`scenario.json` (attente, symboles déclarés, marqueurs attendus). Les patches ont
été générés par `git diff` (donc syntaxe réelle), y compris `-U0` pour les
scénarios où le contexte de lignes masquerait l'interaction mesurée.

| # | id | attente | pourquoi ce scénario |
|---|---|---|---|
| 1 | `s01_two_functions` | accept | deux fonctions indépendantes, même fichier |
| 2 | `s02_adjacent_functions` | accept | fonctions adjacentes (contextes de hunk voisins) |
| 3 | `s03_same_function` | conflit textuel | même fonction, même ligne |
| 4 | `s04_line_shift_insert_head` | accept | insertion en tête : décale les lignes de B |
| 5 | `s05_append_tail` | accept | insertion en fin de fichier |
| 6 | `s06_symbol_moved` | accept | fonction déplacée (alpha après beta) + édition de beta |
| 7 | `s07_name_conflict` | échec compilation | deux renommages indépendants créent `helper2` en double (E0428) |
| 8 | `s08_import_change` | échec compilation | A retire l'import `Debug`, B ajoute du code qui en dépend (E0404) |
| 9 | `s09_same_function_disjoint_lines` | conflit sémantique | même fonction, lignes **différentes** : compiles, mais conflit d'intention |
| 10 | `s10_undeclared_dependency` | risque sémantique | B modifie `compose` qui appelle `helper` modifié par A, sans le déclarer |

### 3.1 Stratégies comparées

1. **`naive_textual`** — fusion textuelle naïve : les plages de lignes réellement
   modifiées ne doivent pas se chevaucher, puis B est appliqué avec compensation
   des décalages de lignes introduits par A.
2. **`strict_hunks`** — hunk matching à contexte strict : application exacte de B
   sur le contenu fusionné, sans compensation d'offset ni relocalisation (c'est
   la couche d'application exacte de COLLAB-1).
3. **`syntactic_anchors`** — ancres syntaxiques : le symbole englobant de chaque
   bloc modifié est identifié par appariement d'accolades ; un même symbole touché
   par les deux agents est un conflit ; sinon le bloc est relocalisé dans le
   symbole correspondant du contenu fusionné (gère déplacements et décalages).
4. **`symbols_ranges_deps`** — approche retenue : symboles déclarés + lignes
   réellement modifiées + application exacte (le coordinateur COLLAB-1 réel),
   **complétée du graphe de dépendances** (ligne ajoutée référençant un symbole
   modifié par un autre agent).

Chaque fusion acceptée est compilée par **le vérificateur de production**
(`RustcMergeVerifier`, `--emit=metadata`) ; le digest du contenu fusionné est
enregistré à côté du verdict. La ligne `symbols_ranges_deps_verified` ajoute la
garde de compilation de bout en bout (celle du binaire) au-dessus de la stratégie
4 : c'est le pipeline de production.

---

## 4. Résultats mesurés

Commande (sérialisée, machine chargée) :

```
cargo test -p ravum-workspace-coordinator --test merge_strategies -- --test-threads=1 --nocapture
# ou directement :
target/debug/collab2-lab.exe --corpus fixtures/collab2/corpus \
    --work target/ravum/collab2-lab --json target/ravum/collab2-lab/report.json
```

Sortie mesurée (rustc 1.95.0, x86_64-pc-windows-msvc ; **5,8 s** pour 10 scénarios ×
5 stratégies, compilation comprise, sur la machine chargée) :

```
scenario                               expect                 naive_textual  strict_hunks   syntactic      symbols        verified
s01_two_functions                      accept                 ACCEPT/ok      ACCEPT/ok      ACCEPT/ok      ACCEPT/ok      ACCEPT/ok
s02_adjacent_functions                 accept                 ACCEPT/ok      ACCEPT/ok      ACCEPT/ok      ACCEPT/ok      ACCEPT/ok
s03_same_function                      textual_conflict       REFUSE         REFUSE         REFUSE         REFUSE         REFUSE
s04_line_shift_insert_head             accept                 ACCEPT/ok      FALSE-CONFLICT ACCEPT/ok      FALSE-CONFLICT FALSE-CONFLICT
s05_append_tail                        accept                 ACCEPT/ok      ACCEPT/ok      ACCEPT/ok      ACCEPT/ok      ACCEPT/ok
s06_symbol_moved                       accept                 FALSE-CONFLICT FALSE-CONFLICT ACCEPT/ok      FALSE-CONFLICT FALSE-CONFLICT
s07_name_conflict                      compile_failure        ACCEPT+E       ACCEPT+E       ACCEPT+E       ACCEPT+E       REFUSE
s08_import_change                      compile_failure        ACCEPT+E       ACCEPT+E       ACCEPT+E       ACCEPT+E       REFUSE
s09_same_function_disjoint_lines       same_function_conflict UNSAFE-ACCEPT  UNSAFE-ACCEPT  REFUSE         REFUSE         REFUSE
s10_undeclared_dependency              semantic_risk          UNSAFE-ACCEPT  UNSAFE-ACCEPT  UNSAFE-ACCEPT  REFUSE         REFUSE

strategy                   accepted  refused  false_conflict  bad_merge  unsafe_semantic  compile_caught  correct_accept  correct_refuse  all_compiled
naive_textual                     8        2               1          0                2               2               4               1          true
strict_hunks                      7        3               2          0                2               2               3               1          true
symbols_ranges_deps               5        5               2          0                0               2               3               3          true
symbols_ranges_deps_verified      3        7               2          0                0               0               3               5          true
syntactic_anchors                 8        2               0          0                1               2               5               2          true
```

Lecture :

* **Aucune fusion acceptée n'est non compilée** (`all_compiled = true` partout ;
  le lab échoue si un accept n'a pas de verdict de compilation).
* `compile_caught` compte les fusions acceptées par la couche textuelle dont le
  oracle de compilation prouve l'échec : 2 pour chaque stratégie purement
  textuelle (`s07`, `s08`). C'est la démonstration que le score de fusion ne
  suffit pas et que la garde de compilation est nécessaire.
* `unsafe_semantic` compte les acceptations sémantiquement fausses que la
  compilation ne détecte pas (`s09` : même fonction modifiée deux fois sur des
  lignes différentes ; `s10` : dépendance non déclarée). Seules les stratégies
  conscientes des symboles/du graphe les refusent.
* Le pipeline retenu + compilation (`symbols_ranges_deps_verified`) a **0 fusion
  incorrecte, 0 acceptation sémantiquement dangereuse, 2 faux conflits** (`s04`,
  `s06`), et refuse correctement `s03`, `s07`, `s08`, `s09`, `s10`.
* `syntactic_anchors` est la seule stratégie qui relocalise correctement `s06`
  (symbole déplacé) et ne produit aucun faux conflit sur ce corpus, mais elle
  accepte `s10` (aucune conscience des dépendances) et son heuristique de
  relocalisation n'est pas prouvée sur du code arbitraire.

### 4.1 Décision technique : l'approche retenue est conservée, complétée

**Conservée** : l'identification par **symboles déclarés + lignes réellement
modifiées** reste la couche de décision, et l'application exacte (contexte strict)
reste la seule écriture autorisée. Raisons mesurées :

1. c'est la seule couche qui refuse `s09` *sans* compilation (conflit
   d'intention) ;
2. elle n'a produit **aucune fusion incorrecte** sur le corpus ;
3. elle est déterministe, sans heuristique de relocalisation dans le chemin
   d'écriture, et fail-closed : un patch qui ne s'applique pas exactement est
   refusé.

**Complétée** par deux ajouts, pas remplacée :

1. **graphe de dépendances** pour la dépendance non déclarée (`s10`) ;
2. **vérification réelle du fusionné** (compilation, et tests si demandés) pour
   `s07`/`s08` : c'est la seule façon objective de transformer une fusion
   textuellement propre mais invalide en refus avec preuve.

**Alternatives rejetées comme couche de décision** :

* fusion textuelle naïve (`naive_textual`) : la plus permissive (8 acceptations),
  mais 2 acceptations non compilables et 2 acceptations sémantiquement fausses ;
  un score d'acceptation plus élevé n'est pas un progrès ;
* hunk matching strict seul : correct en refus mais 2 faux conflits sur des
  décalages/déplacements légitimes (`s04`, `s06`) ; déjà le comportement actuel,
  donc rien à gagner ;
* ancres syntaxiques seules : gère bien les déplacements, mais accepte `s10` et
  repose sur une relocalisation heuristique non prouvée ; utilisable comme
  **amélioration future** du re-lignage uniquement si son résultat est compilé à
  chaque fois (donc derrière le verifier), et après démonstration sur un corpus
  plus large. Non adoptée dans MVP-0b : elle déplacerait une écriture sans preuve
  avant la compilation.

Les 2 faux conflits (`s04`, `s06`) du pipeline retenu sont une **limite mesurée
assumée** : coût = intervention humaine sur des insertions en tête/déplacements ;
bénéfice = aucune écriture non exacte. L'amélioration envisagée est un
décalage/re-lignage *dérivé du symbole* mais **uniquement** lorsque l'indépendance
par symboles est déjà établie, et toujours suivi de la compilation avant
acceptation (voir §9).

---

## 5. COLLAB-3 : les dix cas, verdicts et preuves

Tests : `crates/ravum-workspace-coordinator/tests/collab3_semantic.rs` (via le
binaire réel, mode `--verify`/`--verify-tests`). Tous les refus vérifient en plus
que le snapshot et l'arbre Git sont inchangés.

| # | Cas | Fixture | Verdict obtenu | Preuve journalisée |
|---|---|---|---|---|
| 1 | même fonction modifiée deux fois | `semantic/same_function` | 1er `AUTO_SAFE` (vérifié), 2e `CONFLICT` | « symbol "alpha" is already modified by agent-a/alpha » |
| 2 | signature publique changée | `semantic/public_signature` | `MANUAL_REQUIRED` | « public API surface changed … external contracts require human arbitration » + `semantic policy evidence: <64 hex>` |
| 3 | appelant devenu incompatible | `semantic/caller_incompatible` | `REJECT` | diagnostic compilateur `E0061`, « does not compile », evidence id |
| 4 | contrat affaibli | `semantic/weakened_contract` | `MANUAL_REQUIRED` | a) retrait d'assertion : « a contract assertion was removed … weaken … requires human arbitration » ; b) `impacted_contracts` déclaré : « contract changes require human arbitration (INV-02) » |
| 5 | dépendance non déclarée | `semantic/undeclared_dependency` | 1er `AUTO_SAFE`, 2e `MANUAL_REQUIRED` | « undeclared semantic dependency: a changed line references symbol "helper", whose definition was modified by agent-a/helper … declare the dependency … » ; contrôle positif (sans référence à `helper`) : `AUTO_SAFE,AUTO_SAFE` |
| 6 | import ambigu | `semantic/ambiguous_import_glob` | 1er `AUTO_SAFE`, 2e `REJECT` | `E0659` (import glob ambigu introduit par la fusion), seul A publié |
| 7 | changement d'ABI | `semantic/abi_change` | `MANUAL_REQUIRED` | « ABI surface changed … ABI compatibility cannot be proven by a local compile … » + evidence |
| 8 | modification de lockfile | `semantic/lockfile` | `MANUAL_REQUIRED` | « target path "Cargo.lock" is generated or a lockfile: never auto-integrated » |
| 9 | régression fonctionnelle dans du code qui compile | `semantic/functional_regression` | sans exécution de tests : `MANUAL_REQUIRED` (« test execution is disabled », fail-closed) ; avec `--verify-tests` : `REJECT` | harnais de test en échec (`test harness of the merged snapshot failed`) + assertions/evidence |
| 10 | agent sur snapshot périmé | `semantic/two_agents_same_file` (base modifiée) | `REBASE_REVALIDATE` | « base snapshot … is stale and expected hashes do not cover every target path » |

Points de conception confirmés par ces cas :

* la classification est faite **avant** toute intégration, dans l'ordre :
  politique → vérification ; un refus ne modifie jamais le snapshot ;
* une vérification **impossible** mais requise n'est jamais convertie en succès
  (cas 9a) ;
* une vérification **échouée** n'est jamais convertie en `CONFLICT` ambigu : elle
  est `REJECT` avec le code du compilateur et un evidence id (cas 3, 6, 9b).

---

## 6. API exploitable par L2 / COLLAB-4

### 6.1 `AcceptedChange` (un par changement accepté)

Exposé par `Coordinator::accepted_changes()` et dans le rapport JSON
(`accepted_changes`) ; sérialisable en JSON, schéma versionné
(`COLLAB_RECORD_SCHEMA_VERSION = 1`) :

```json
{
  "schema_version": 1,
  "proposal_id": "agent-a/alpha",
  "agent_id": "agent-a",
  "task_id": "alpha",
  "decision": "AUTO_SAFE",
  "journal_seq": 1,
  "snapshot_before": "<sha256 du snapshot courant avant intégration>",
  "snapshot_after": "<sha256 du snapshot après intégration>",
  "paths": ["src/lib.rs"],
  "declared_symbols": ["alpha"],
  "modified_symbols": ["alpha"],
  "verification_required": true,
  "verification": {
    "schema_version": 1,
    "required_checks": ["metadata_check", "test_compile", "test_run"],
    "status": "passed",
    "checks": [
      {
        "kind": "metadata_check",
        "status": "passed",
        "tool": "rustc",
        "tool_version": "1.95.0",
        "target": "x86_64-pc-windows-msvc (materialized workspace)",
        "snapshot_id": "<sha256 du snapshot candidat>",
        "summary": "rustc 1.95.0: candidate snapshot compiles (metadata only)",
        "diagnostics": [],
        "evidence_id": "<64 hex>"
      }
    ],
    "evidence_id": "<64 hex>"
  },
  "evidence_ids": ["<check ids>", "<aggregate id>"],
  "recorded_at_unix": 1794145458
}
```

Propriétés utiles au daemon :

* `snapshot_before`/`snapshot_after` sont les identités content-adressées
  **partagées** avec `ravum-snapshot`/`ravum-protocol` : un daemon peut les
  comparer sans rejouer le coordinateur ;
* chaque preuve est **déterministe** (même contenu, même outil, même verdict ⇒
  même `evidence_id`) ;
* `journal_seq` relie le record à la ligne correspondante du journal durable
  (`JournalEntry`) ;
* le record ne dépend d'**aucun** crate `ravum-daemon` : c'est un type du
  coordinateur, exportable en JSON.

### 6.2 Point d'intégration futur (documenté, non implémenté ici)

1. **Identifiants de snapshots partagés** : le daemon (L2/COLLAB-4) consomme les
   `AcceptedChange` et publie les activations par `snapshot_after`. Une activation
   n'est valide que si le snapshot activé est encore `current` (sinon `STALE`,
   jamais réutilisée).
2. **Invalidation ciblée** : quand un nouveau changement touche un chemin ou un
   symbole déjà présent dans un `AcceptedChange`, les records antérieurs dont
   `paths`/`modified_symbols`/`evidence_ids` intersectent doivent être marqués
   périmés (`STALE`), et non silencieusement conservés. Le champ `journal_seq`
   donne l'ordre ; `paths` et `modified_symbols` donnent la clé d'invalidation.
3. **Journal des activations** : pour chaque `AcceptedChange` activée, appendre
   `(snapshot_after, evidence_id, active_version_id, journal_seq)`. Le présent lot
   ne crée pas ce journal (hors périmètre) ; le daemon devra l'écrire avant de
   remplacer le snapshot actif.
4. **Replay** : un redémarrage peut reconstruire l'état courant à partir du
   journal durable des décisions ; les records ne sont jamais la source de
   vérité — ils sont la trace d'audit et la clé d'invalidation.

---

## 7. Commandes exécutées et résultats

Toutes les commandes ci-dessous ont été exécutées sur la branche
`feat/mvp0b-hardening` avec les changements non commités de ce lot.

### 7.1 Tests du crate (sérialisés)

```
cargo test -p ravum-workspace-coordinator -- --test-threads=1
```

Résultat exact de la dernière exécution — **87 tests, 0 échec** :

* unitaires `src/lib.rs` : **49 passed, 0 failed** (dont les 26 unitaires
  COLLAB-1 d'origine, préservés avec les mêmes assertions) ;
* `tests/collab1_non_regression.rs` : 2 passed ;
* `tests/collab2_two_functions.rs` : 3 passed ;
* `tests/collab3_semantic.rs` : 10 passed ;
* `tests/merge_strategies.rs` : 2 passed ;
* `tests/git_publication.rs` : 16 passed (COLLAB-1 transactionnel, inchangé) ;
* `tests/git_writer.rs` : 2 passed ;
* `tests/journal_durability.rs` : 3 passed ;
* doc-tests et unitaires des binaires : 0.

Total : 49 + 2 + 3 + 10 + 2 + 16 + 2 + 3 = **87 tests passés, 0 échoué**
(`cargo test -p ravum-workspace-coordinator -- --test-threads=1`, code de sortie 0).

### 7.2 Mesure des 4 stratégies

```
target/debug/collab2-lab.exe --corpus fixtures/collab2/corpus \
    --work target/ravum/collab2-lab --json target/ravum/collab2-lab/report.json
```

Table §4 ; code de sortie 0 quand l'invariant « toute fusion acceptée est
compilée » tient, 1 sinon.

### 7.3 Démonstration COLLAB-2/3

```
powershell -ExecutionPolicy Bypass -File scripts/demo-collab2.ps1
```

Sortie mesurée (extraits) :

```
[3/6] Intégration avec vérification sémantique réelle (--verify-tests)
      record #1: 0df3dd844cbb -> 27f96f12409a [passed] 59b9488691647156
        - metadata_check  passed  rustc 1.95.0: candidate snapshot compiles (metadata only)
        - test_compile    passed  rustc 1.95.0: test harness compiles
        - test_run        passed  test harness executed: test result: ok. 1 passed; 0 failed; ...
      record #2: 27f96f12409a -> 6fb37ad8a9c6 [passed] da6e6df8b427b715
        ...
[4/6] Publication Git : les deux changements doivent être conservés
      commit = 8cecb5d748a8 (parent df964775b0f7)
[5/6] Contre-vérification indépendante du code publié (rustc --test)
      test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; ...
...
      refus = semantic verification failed: merged snapshot 0136adf53873 does not compile
              with rustc 1.95.0 | failed check metadata_check on snapshot 0136adf53873
              (evidence c461e721...c170) | diagnostic: E0428 [error] the name `helper2` is defined multiple times
DÉMO COLLAB-2/3 : SUCCÈS
```

Code de sortie : 0 (le script propage le code de sortie ; 1 si une assertion ou
une commande échoue). Le script utilise une **vraie crate**
(`fixtures/collab2/demo-crate` : `Cargo.toml` + `src/lib.rs` + tests) et passe
explicitement `--verify-tests`.

### 7.4 Non-régression COLLAB-1 (exigence du mandat)

```
powershell -ExecutionPolicy Bypass -File scripts/demo-collab.ps1 -Commit
```

Résultat : code de sortie **0** et `AUTO_SAFE=2 CONFLICT=1 REJECT=1
MANUAL_REQUIRED=1 REBASE_REVALIDATE=1` (décisions historiques inchangées), les
deux changements publiés. Le script n'a pas été modifié (hors périmètre) ; c'est
le binaire qui conserve son comportement par défaut, la vérification sémantique
étant opt-in.

### 7.5 Build

```
cargo build -p ravum-workspace-coordinator
```

OK. Une seule dépendance ajoutée : `ravum-rust-adapter` (workspace, existe déjà
dans le dépôt ; aucun crate externe nouveau). Elle n'est utilisée que par les
binaires (verifier + identité toolchain) ; la bibliothèque coordinateur ne
l'importe pas.

---

## 8. Limites honnêtes

### 8.1 Ce qui n'est pas démontré / limites techniques

1. **Pas de vrais agents IA concurrents.** Les « agents » sont des propositions
   JSON déterministes. Aucune mesure de latence, de qualité de patch ou
   d'hallucination d'un LLM n'est faite ici.
2. **Pas d'AST complète.** Les faits sémantiques (`semantics`) sont des
   heuristiques : appariement d'accolades, détection de `pub`/ABI par texte,
   assertions retirées par texte, symboles englobants. Les signatures multi-lignes,
   macros générant des items, `impl`/traits ambigus, chaînes brutes ou
   commentaires multi-lignes sont approximés. Toute ambiguïté produit un refus
   fail-closed (`MANUAL_REQUIRED`), jamais une acceptation silencieuse — mais un
   refus peut être un faux positif.
3. **Pas de CRDT, pas de convergence.** Deux agents qui modifient la même fonction
   sont refusés ; ce n'est pas une fusion convergente.
4. **La régularité comportementale n'est prouvée que si des tests existent et que
   `--verify-tests` est utilisé.** Sans tests, un changement de comportement qui
   compile n'est pas détecté. Un harnais de test exécuté est du code du snapshot :
   ce n'est pas un bac à sable (même utilisateur, même machine), d'où l'opt-in.
5. **ABI** : un conflit d'ABI n'est pas prouvable par une compilation locale
   (surtout avec des consommateurs hors workspace). La politique refuse toute
   modification de marqueur ABI sans arbitrage humain ; ce n'est pas une preuve
   d'incompatibilité ABI.
6. **Contrats affaiblis** : détectés si déclarés (`impacted_contracts`), si une
   assertion de contrat est retirée, ou si un test échoue. Un affaiblissement
   sémantique non testé et non déclaré reste indétectable.
7. **Dépendance non déclarée** : heuristique symbole→symbole (ligne ajoutée
   référençant un symbole dont un autre agent a modifié la définition). Les
   dépendances par effets de bord, données externes ou conventions non
   symboliques ne sont pas modélisées.
8. **Vérification de workspace** : une seule racine de crate est inférée et
   compilée. Multi-crates/multi-workspaces Cargo ne sont pas vérifiés ; en mode
   sémantique cela produit `MANUAL_REQUIRED` (fail-closed), pas une acceptation.
9. **`ravum-rust-adapter` et schéma JSON rustc 1.95** : le parseur de
   l'adaptateur attend `{"reason":"compiler-message"}` (schéma cargo) ; le CLI
   rustc 1.95 émet `{"$message_type":"diagnostic"}` en invocation directe. Le
   verifier contourne en reparsant le CLI public (`$message_type` **et**
   `reason`), et c'est un point d'harmonisation à traiter par le propriétaire de
   l'adaptateur (hors périmètre de ce lot). Conséquence : l'adaptateur n'est pas
   utilisé pour la compilation du fusionné, seulement pour l'identité toolchain.
10. **Course vérification/publication** : le verifier compile le snapshot candidat
    en mémoire ; `GitWriter` republie avec CAS sur la tête Git et vérifie les
    hashes du contenu, mais un changement externe du working tree entre les deux
    est détecté par `StaleWorkingTree` (refus), pas réconcilié.
11. **Performance** : aucun budget de temps par proposition n'est imposé ; les
    mesures sont sur des fichiers minuscules (~10–30 lignes). Le coût d'un
    `rustc --emit=metadata` sur un vrai crate n'est pas mesuré ici.
12. **Démo COLLAB-2/3** : elle démontre le pipeline et la publication, mais reste
    un scénario scripté, pas une charge concurrente réelle.
13. **`ravum-daemon`** n'est pas utilisée (en cours par un autre worker) ; l'API
    L2 est spécifiée et testée au niveau coordinateur seulement.

### 8.2 Fausses alertes / bruit

* Le pipeline retenu produit 2 faux conflits mesurés (`s04`, `s06`) ;
* l'heuristique de dépendance peut refuser une modification qui ne dépend
  réellement pas du symbole modifié (fail-closed) ;
* l'heuristique de surface publique peut signaler un changement de visibilité
  local comme contrat (conservateur).

---

## 9. Prochaines étapes

1. **Hybride ancres + symboles** : après établissement de l'indépendance par
   symboles, relocaliser les hunks par ancre syntaxique (comme
   `syntactic_anchors`) puis **toujours** compiler avant acceptation, pour
   supprimer les 2 faux conflits mesurés. Nécessite un corpus plus large et des
   tests négatifs sur la relocalisation.
2. **Vérification multi-crates/workspace** : matérialiser un workspace Cargo,
   compiler avec `cargo check`/`cargo test` épinglé, et traiter l'adaptateur comme
   fournisseur (harmoniser le parseur JSON `$message_type`).
3. **Intégration L2/COLLAB-4** : consumer les `AcceptedChange`, journal
   d'activation, invalidation ciblée par chemins/symboles, partage des snapshot
   ids avec le daemon.
4. **Tests exécutés en isolation** : si `--verify-tests` devient un mode de
   production, exécuter le harnais dans un environnement restreint (répertoire
   jetable, limites CPU/mémoire/temps, pas de réseau) plutôt que sous
   l'utilisateur courant.
5. **Corpus étendu** : vrais projets Rust, fichiers de plusieurs centaines de
   lignes, plusieurs crates, macros, et mesures de coût par vérification.
6. **Décisions d'acceptation** : étudier si `ADMIN`/`MANUAL_REQUIRED` doit
   produire un artefact de reprise (patch revalidable plus tard) au lieu d'un
   simple refus.


