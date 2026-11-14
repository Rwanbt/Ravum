# GOAL « production ready » — rapport final (Core 1.0)

> **Ce rapport n'est pas une déclaration de disponibilité.** Il dit ce qui a été
> **exécuté** dans cette session, avec la commande et le SHA, ce qui ne l'a pas
> été (`NOT_RUN`), ce qui est bloqué par une décision humaine (`BLOCKED`,
> `PENDING_HUMAN_APPROVAL`). Aucun `PROVED`, aucune maturité « production » n'y
> est écrit. Le label appartient au responsable.
>
> Branche : `goal/production-ready` (depuis `test/coverage-batch-1`, lui-même
> `dev` + 3 commits de couverture). SHA final mesuré : voir §2.8.
> Machine : Windows 11 x64 10.0.26100, i7-13620H 10C/16T, 16 Go,
> `rustc 1.95.0`.

## 1. Résumé en dix lignes

1. Le corpus officiel est vert : **476 cas, 475 PASS / 0 FAIL / 1 SKIP**,
   `build_verified: true`, périmètre build **203/203**, aucune régression.
2. Le workspace est vert : **100 groupes, 1 604 passés / 0 échec / 5 ignorés**
   (4 campagnes longues et la mesure LSP de bout en bout, lancées à part) ; le
   **profil release** passe la même suite : **1 603 / 0 / 5** (1 test
   debug-only exclu).
3. Un **oracle différentiel** neuf a mesuré **37 200 programmes** bien typés
   (trois campagnes déterministes ; générateur étendu à 42 gabarits) :
   `check` accepte ⇒ `rustc` compile, **0 divergence** ; la configuration
   **nominale 3 × 10 000 = 30 000 programmes a été exécutée localement**
   (8 602 s, 3 000 comparaisons debug == release, 100 % acceptés).
4. La boucle `.rv` est **mesurée** : `check` p95 à froid **1,3-8,1 ms**,
   après édition **11,0-49,8 ms** ; `build` p50 **179-331 ms** dont `rustc`
   **97,1-99,3 %**.
5. `-C incremental` a un gain **nul ou négatif** : implémenté en opt-in,
   **désactivé par défaut**, chiffres publiés.
6. Le LSP **ne re-parse plus** un fichier inchangé : p95 d'analyse après édition
   **1,362 ms** avec cache contre **1,836 ms** sans ; **bout en bout, transport
   inclus, p95 2,826 ms** (p50 2,061 ms) sur 20 éditions d'une ligne.
7. Les `NodeId` sont **déjà stables par fichier** (2 tests) : la migration
   envisagée par l'audit est **sans objet** ; la règle « aucun cache indexé par
   `FileId` » est écrite.
8. La gouvernance périmée est corrigée (`STATUS.md`, `START_HERE.md`,
   `AGENTS.md` en `PENDING_HUMAN_APPROVAL`, statut du langage en un seul état).
9. **Non fait** : phase 4.5 (étape C, suppression du `MovePlanner`), phase 5.4
   (adaptation de l'exemple DSP), phase 8 (bloc DSP) ; Miri et mutation :
   `NOT_RUN` avec raisons. **Fait depuis la première rédaction** : étapes A/B
   partielles d'`OwnFacts`, phase 5 implémentée en `rt` (gate de traps, y compris
   affectation composée, + méthodes explicites + 5 cas), et **campagne nominale
   de l'oracle (30 000 programmes) exécutée localement**.
10. **Rien n'est prouvé** au sens formel. T4 (audit externe) et T12 (Go/No-Go)
    restent `BLOCKED` par construction ; D9 (clé) reste différée.

## 2. Tableau par phase

| Phase | Statut | Commits | Tests ajoutés (positifs / négatifs) | Commande de preuve et résultat brut |
|---|---|---|---|---|
| **0** Baseline | `TESTED` | `c931fdf`, `e82ed14`, `ded0e60` | — | `cargo test --workspace --all-targets -- --test-threads=2` : 94 groupes, 1574 / 0 / 2 ; conformance 471 / 470 / 0 / 1 ; smoke 43/0/0 ; docs PASS ; registre 108/126 ; 188/188 ; fmt 0. Détail : `docs/reports/GOAL-BASELINE.md` |
| **1** Gouvernance et base de confiance | `TESTED` (+1 `PENDING_HUMAN_APPROVAL`) | `41851db`, `fac8a8c`, `c04f33e`, `dddfd32`, `b129024`, `85e993e`, `0ea05b7` | +2 gardes codegen (positifs) avec preuve de rouge ; +1 garde HashMap (positif, rouge prouvé) ; +5 tests d'hygiène racine | docs PASS ; gardes prouvées rouges puis vertes ; `C`+U+F03A supprimé après preuve (0 fichier, 6 répertoires vides) |
| **2** Oracle `check ⇒ build` | **`TESTED`** | `2539d5c`, `272ece3`, `825aea1` + campagne `aa88479` | +2 tests de non-vacuité (rustc rejette un fichier invalide ; deux graines distinctes) ; générateur **34 → 42 gabarits** | fast **600** (600 acceptés, 0 divergence) ; long local **3 000** (0 divergence) ; **nominal 3 × 10 000 = 30 000 programmes locaux, 30 000 acceptés, 0 divergence, 8 602 s**, 3 000 comparaisons release ; cumul **37 200 programmes**. Détail : `T2-FINDINGS.md` §25-§26 |
| **3** Mesure et incrémental | `TESTED` | `4262569`, `84eaa08`, `3b6eb75`, `d186e3d`, `1265de2` | +3 contrats `--timings` (JSON additif, absence du champ sans flag, phases build) | `scripts/lang-timings.ps1 -Runs 20` : chiffres §3 ; `repro-check.ps1` PASS ; tests contrat 3/3 |
| **4** `OwnFacts` | **`PARTIAL`** : 4.1-4.3 `TESTED`, 4.4 `PARTIAL`, 4.5 `NOT_RUN` | `e91ecc3` (RFC), `afb5783` (étape A), `fbb0aeb` (étape B) | +3 : 2 tests d'intégration (move/copy/borrow/move projeté + déterminisme) et 1 test négatif (désaccord injecté ⇒ `CodegenError`) | `cargo test -p ravum-lang-sema --test own_facts` 2/2 ; `cargo test -p ravum-lang-codegen --lib` 30/30 ; conformance 476 / 475 / 0 / 1 ; oracle rapide **600 programmes, 0 divergence**. L'étape B ne compare que la classification `Copy`/`Move`, pas les places |
| **5** Traps `rt` | **`PARTIAL`** : 5.1-5.3 `TESTED`, 5.4 `NOT_RUN` | `43fbd68` (RFC) + `21533dd` | +10 tests unitaires (négatifs/positifs, affectation composée, largeurs de décalage) et **5 cas de conformance** (4 compile-fail `E-EFF-PROFILE`, 1 compile-pass) | `cargo test -p ravum-lang-sema --lib traps` **10/10** ; conformance **476 / 475 / 0 / 1**, build **203/203** ; `core`/`pure` inchangés (`intrinsic-panic-not-tracked` toujours accepté). 5.4 : aucun accesseur de slice sûr en stdlib, exemple inchangé |
| **6** `NodeId` | `TESTED` (sans objet) | `0595b9f`, `7916840`, `91d3621` | +2 tests (stabilité inter-fichiers, layout 8/24 bits) | `cargo test -p ravum-lang-sema --test node_id_stability` : 2/2 ; `util.rv` inchangé après édition de `main.rv` |
| **7** Cache de parse LSP | **`TESTED`** (analyse et bout en bout local) | `a0a7c26`, `2d432be`, `5860ef3`, `6257b84`, `a980717` | +2 tests (réutilisation exacte hits/misses ; INV-07) +2 mesures `#[ignore]` (analyse, bout en bout) | `cargo test -p ravum-lang-lsp --test parse_cache` 2/2 ; `cargo test -p ravum-lang-cli --test lsp_client` 11/0 ; bout en bout **p50 2,061 ms / p95 2,826 ms** ; `docs/qualification/LSP.md` §7 |
| **8** Bloc DSP | `NOT_RUN` | `bb3edea` (RFC) | — | ABI lue (8.1) ; RFC-RV-DSP-BLOCK-1 (8.2) ; aucun wrapper, aucun test de swap |
| **9** Qualification | `PARTIAL` | ce rapport et les sections datées des registres | — | Voir §4 ; couverture **89,41/88,81** (seuils tenus, `codegen/lib.rs` à **100/100**, `traps.rs` à 98,66/96,84), fuzz long rejoué, mutation/Miri `NOT_RUN`, PR brouillon |

### 2.8 SHA final mesuré

Le SHA exact du rapport est celui du commit qui l'ajoute ; les commandes des
phases 0 à 3 ont été exécutées sur l'arbre `faf65cb`, puis les phases 4, 6, 7 et
5 sur leurs commits respectifs (voir le journal). Les chiffres « courants » de la
§3 incluent la phase 5 (`476` cas, `1 604` tests) ; le SHA de chaque mesure est
celui de son entrée de journal. La campagne nominale de l'oracle a été exécutée
sur `aa88479` (arbre inchangé depuis `28ad5da`, seuls des documents diffèrent).

## 3. Comparaison baseline (phase 0) → état final

**Une baisse mesurée au round 5, résorbée aux rounds 6-10.** La couverture totale
était passée de 89,21/88,61 à **89,07/88,43** après l'ajout d'`OwnFacts` et de
`traps.rs`. Les tests ajoutés ensuite la ramènent à **89,41/88,81** (round 10) :
`CodegenError::fmt` couvert fait passer `codegen/lib.rs` à **100,00/100,00**
(plancher relevé à 100/100), `traps.rs` passe à **98,66/96,84** avec 13 tests et
entre au cliquet à 97/95. **Tous les seuils par fichier sont tenus**, 2 112
lignes restent non couvertes (19 950 − 17 838), 10 fichiers restent sous
l'objectif. La limite structurelle des 100 % de **régions** (chemins d'erreur
`fmt::Error` inatteignables) est documentée dans
[`COVERAGE-100-OBJECTIVE.md`](../qualification/COVERAGE-100-OBJECTIVE.md) et
soumise à décision humaine.

Les autres chiffres sont identiques ou meilleurs.

| Mesure | Phase 0 (`c931fdf`) | Final (`faf65cb`+) | Écart |
|---|---|---|---|
| Conformance | 471 cas, 470 PASS / 0 FAIL / 1 SKIP | **476 cas, 475 PASS / 0 FAIL / 1 SKIP** | +5 cas (`rt-trap-*`), 0 échec |
| Périmètre build | 202/202, vérifiés 121, implicites 81, échecs 0 | **203/203**, vérifiés 122, implicites 81, échecs 0 | +1 cas vérifié |
| Workspace | 94 groupes, 1 574 / 0 / 2 | **100 groupes, 1 604 / 0 / 5** | +6 groupes, +30 tests, +3 ignorés (campagnes de mesure) |
| Workspace `--release` | non rejoué | **100 groupes, 1 603 / 0 / 5** | nouvelle preuve ; 1 test debug-only exclu (`verify_ownership_facts`) |
| `ravum-lang-sema` | 683 / 0 | 698 / 0 (NodeId, OwnFacts, traps `rt`) | +15 |
| Registre | 108 / 126 / 0 / 0 | **identique** | 0 |
| Traçabilité FeatureId | 188/188 (1 exemption) | **identique** | 0 |
| MUST | 807, 463 atteignables | **identique** | 0 |
| Couverture sema+codegen | 89,08 / 88,39 (docs, non re-mesurée en phase 0) | **89,41 / 88,81**, seuils tenus | +0,33 / +0,42 pt ; `codegen/lib.rs` clos à 100/100, `traps.rs` à 98,66/96,84 |
| Fichiers sous l'objectif 100 % | 10 (2 098 lignes, vue instanciations) | **10 (2 112 lignes)** | +14 lignes (code ajouté, mesuré hors tests de conformance) |
| Smoke exemples | 43 / 0 / 0 | **identique** | 0 |
| Fuzz syntaxe | non rejoué en phase 0 | **249 054 entrées, 0 échec**, digest `0x9b09fc61e761b87e` | nouvelle preuve |
| Fuzz sema | non rejoué en phase 0 | **100 000 itérations, 0 échec**, 4 660/s ; **rejoué après la passe de traps `rt`** : 100 000, 0 échec, 5 275/s (≈ 33 000 en profil `rt`) | nouvelle preuve |
| LSP client | non rejoué en phase 0 | **11 / 0** (+1 mesure `#[ignore]`) | nouvelle preuve |
| LSP bout en bout (transport inclus) | non mesuré | **p50 2,061 ms / p95 2,826 ms** sur 20 éditions d'une ligne | cible P0-A p95 < 50 ms atteinte sur cette mesure locale |
| Reproductibilité release | non mesurée en phase 0 | **PASS**, deux racines identiques, sha256 `988521151EFE827EA62448343C08128370CA3801A3EAEF312C04B298C43FDE8A` (rejoué au round 13 sur `662f63d`) | nouvelle preuve |
| Installateur Windows | `scripts/install.ps1 -Prefix <tmp> -SkipBuild` puis `-Uninstall` deux fois | install exit 0 (manifeste sha256 `bf55aab9…`, commit `662f63d`), désinstallation idempotente **0 / 0** | nouvelle preuve, rejouée au round 13 |
| SBOM | `SOURCE_DATE_EPOCH=0 pwsh -File scripts/sbom.ps1` | SPDX-2.3, **64 paquets / 170 relations**, sha256 `46b65d9fe941…`, `created=1970-01-01`, déterministe | nouvelle preuve, rejouée au round 13 |
| Oracle différentiel | inexistant | **37 200 programmes (3 campagnes), 0 divergence** | nouvelle preuve |

## 4. Critères PRODUCTION-READINESS et gates T0–T12

États au SHA final. Les commandes sont dans les sections précédentes ou dans le
journal ; un `PARTIAL` n'est jamais arrondi en `PASS`.

### 4.1 Critères R1-R14

| # | Critère | État | Preuve / manque |
|---|---|---|---|
| R1 | Sémantique ratifiée | `PARTIAL` | ADR `PROPOSED`, spec DRAFT ; aucune ratification par un agent |
| R2 | Aucune divergence connue | **fait sur le périmètre mesuré** | 476 cas 0 FAIL + 37 200 programmes générés 0 divergence (dont le nominal 30 000) ; ne prouve pas l'absence de forme inconnue |
| R3 | Traçabilité complète | `PARTIAL` | 188/188 FeatureIds ; **463/807 MUST**, 344 non mappés (`must_audit.py`) — D10 |
| R4 | Sûreté du frontend | `BLOCKED` | audit externe absent (T4) ; Miri `NOT_RUN` (composant `cargo-miri` absent de la toolchain locale, installer serait un outil supplémentaire) |
| R5 | Frontière FFI sûre | `PARTIAL` | inventaire et refus outillés ; contrats ABI/lifetime/unwinding non écrits ; D7 ouvert |
| R6 | Code généré sûr | **fait** | `forbid(unsafe_code)` sans `extern` ; `deny` + `allow` local au wrapper ; 2 tests de garde, rouges prouvés |
| R7 | Reproductibilité inter-machines | `PARTIAL` | deux racines identiques sur **cette** machine ; pas de deuxième machine |
| R8 | Qualité mesurable | `PARTIAL` | couverture **89,41/88,81**, seuils tenus, `codegen/lib.rs` à **100/100** (plancher relevé), `traps.rs` à 98,66/96,84 ; objectif 100 % impossible par tests sur les régions `fmt::Error` ([`COVERAGE-100-OBJECTIVE.md`](../qualification/COVERAGE-100-OBJECTIVE.md)) ; **mutation `NOT_RUN`** (cargo-mutants absent) |
| R9 | Performance sous budget | `PARTIAL` | p50/p95 publiés (`LANG-TIMINGS.md`) ; pas de campagne sous charge ni de seuil de non-régression CI |
| R10 | Interfaces stables | `PARTIAL` | JSON/CLI/diagnostic testés ; politique de dépréciation toujours absente |
| R11 | Distribution | `PARTIAL` | installateur Windows rejoué (install + 2 désinstallations idempotentes) ; installateur Linux hors contrat ; `wasm`/macOS non couverts |
| R12 | Artefacts signés | `différé` | mécanisme qualifié, **aucune clé** (D9) |
| R13 | Audit externe | `BLOCKED` | quatre revues LLM triées ; audit humain indépendant requis |
| R14 | Documentation sans fausse affirmation | `PARTIAL` | gouvernance corrigée ; guide utilisateur et notes de version absents |

### 4.2 Gates T0-T12

| Gate | État | Ce qui a changé dans cette session |
|---|---|---|
| T0 | `PASS` | inchangé (aucune modification du socle) |
| T1 | `PARTIAL` | inchangé (voie C toujours gelée, décision humaine) |
| T2 | `PARTIAL` | **preuve renforcée** (oracle 37 200 programmes dont le nominal 30 000, 0 divergence) ; le verdict reste `PARTIAL` pour D10 (344 MUST non mappés) |
| T3 | `PASS` | fuzz long rejoué (249 054 entrées, 0 échec) |
| T4 | `BLOCKED` | inchangé : audit humain absent. Aucun agent ne peut le lever |
| T5 | `PASS` | inchangé (smoke 43/0/0, binaires natifs) |
| T6 | `PARTIAL` | inchangé (FFI scalaire expérimentale) |
| T7 | `PASS` | LSP : cache de parse et latence mesurée (analyse p95 1,362 ms ; bout en bout p95 2,826 ms, transport inclus) ; client LSP 11/11 |
| T8 | `PARTIAL` | redémarrage, pas de préservation d'état ; le bloc DSP `.rv` reste `NOT_RUN` (phase 8) |
| T9 | `PARTIAL` | inchangé (10 verticales, pas de DSP hôte `.rv`) |
| T10 | `PARTIAL` | couverture/fuzz/LSP/oracle nominal mesurés ; fuzz sema rejoué après traps ; workspace release vert (1 603/0/5) ; Miri et mutation `NOT_RUN` ; audit externe absent |
| T11 | `PARTIAL` | reproductibilité release rejouée (`98852115…` sur deux racines, round 13) ; SBOM déterministe (`46b65d9f…`, 64/170) ; installateur rejoué (`bf55aab9…`, 2 désinstallations 0/0) ; aucune clé (D9) |
| T12 | `BLOCKED` | Go/No-Go humain |

### 4.3 CI observée sur la PR brouillon

La PR [#110](https://github.com/Rwanbt/Ravum/pull/110) (brouillon, base `dev`) a
déclenché les workflows de la CI. Sur le SHA `303da6a` (code identique à
`faf65cb`, seul le journal diffère), les **8 workflows exécutés sont verts** :

| Workflow | Résultat |
|---|---|
| Documentation integrity | succès (8 s) |
| Validation gate contracts (bloquant) | succès (12 s) |
| VS Code integration (éditeur réel) | succès (1 m 02) |
| Coverage ratchet (planchers par fichier) | succès (1 m 54) |
| Qualification evidence (Windows : conformance, repro, SBOM, provenance, installateur) | succès (2 m 31) |
| macOS qualification (Tier 2) | succès (3 m 57) |
| MVP product checks (build, tests, démos) | succès (10 m 39) |
| MVP impact-selected checks | succès (14 m 22) |

Un run `impact` antérieur, déclenché sur le SHA précédent (`8abf258`), a été
**annulé par le push** qui a suivi : c'est une supersession, pas un échec. Aucun
workflow n'est rouge.

Après l'ajout des étapes A/B d'`OwnFacts` (commit `3a35b49`), les **8 mêmes
workflows sont à nouveau verts** (documentation, contrats, VS Code, **coverage
ratchet**, qualification Windows, macOS, MVP produit, impact), ce qui valide les
planchers de couverture malgré le code ajouté. Aucun correctif CI n'a été
nécessaire.

Le run suivant (`e2a8abc`, docs-only) a révélé un **défaut de fixture de test**
sur macOS : `own_facts_are_deterministic` échouait en `E-MOD-LOCK` parce que
trois helpers `TempDir` de tests d'intégration construisaient leur chemin avec
`pid` + horloge en nanosecondes, et la résolution d'horloge de macOS laissait
deux tests parallèles choisir le même répertoire. Ce n'était pas un défaut du
compilateur. Correctif : compteur atomique par processus dans les trois helpers
(`50a1154`), vérifié localement avec `--test-threads=4` ; sur ce SHA, les
**8/8 workflows sont verts**, macOS inclus.

Sur `a980717` (ajout de la mesure de latence LSP de bout en bout), les
**8 workflows sont à nouveau verts**, coverage ratchet et qualification Windows
incluses. Sur `5403734` (implémentation des traps `rt` et des formes entières
explicites), les **8/8 workflows sont verts**, coverage ratchet incluse malgré le
nouveau module `traps.rs` et les 4 cas de conformance ajoutés. Sur `36ae70b`
(générateur d'oracle étendu à 42 gabarits et couverture re-mesurée), les
**8/8 workflows sont verts**, coverage ratchet incluse. Sur `28ad5da` (traps
d'affectation composée, 5e cas, 10 tests unitaires), les **8/8 workflows sont
verts**, coverage ratchet incluse. Les seuls commits postérieurs sont
documentaires.

La **campagne nominale de l'oracle (3 × 10 000)** a été exécutée **localement**
au round 8 (`aa88479`), pas par la CI de la PR : le workflow dédié
`.github/workflows/differential-oracle.yml` la rejoue séparément.

Sur `17ab64e` (premier fichier mesuré clos à 100 % de couverture,
`codegen/lib.rs`, plancher relevé à 100/100), les **8/8 workflows sont verts**,
coverage ratchet incluse. Sur `7977210` (traps à 98,66/96,84, plancher 97/95,
et document D10), les **8/8 workflows sont verts**, coverage ratchet incluse.

## 5. Éléments `PENDING_HUMAN_APPROVAL`

1. `AGENTS.md` — révision du mandat (fait périmé corrigé) : commit `fac8a8c`.
2. `RFC-OWN-FACTS-1` — extension de `Facts` (interface gelée) et étapes B/C.
3. `RFC-RT-TRAPS-1` — règle de langage `rt` (traps, formes explicites) ; deux
   points ouverts : décalages/négation, sort de `pure`.
4. `RFC-NODE-ID-1` — règle « aucun cache indexé par `FileId` ».
5. `RFC-RV-DSP-BLOCK-1` — ABI de bloc généré, `unsafe` confiné, digest de schéma.
6. FFI expérimentale (D7) — contrats ABI/lifetime/unwinding toujours non ratifiés
   (préexistant à cette session).

## 6. `BLOCKED` et `NOT_RUN` (raison et tentatives)

| Élément | Statut | Raison | Tentatives |
|---|---|---|---|
| Audit externe T4/T12 | `BLOCKED` | décision humaine | — (par construction) |
| Clé de signature D9 | `PENDING_HUMAN_APPROVAL` | aucune clé hors ligne fournie | — |
| Phase 4.3-4.5 (`OwnFacts` A/B/C) | `PARTIAL` : A `TESTED`, B partiel, C `NOT_RUN` | B ne compare que les `Copy`/`Move` ; C (suppression du `MovePlanner`) exige la comparaison des places et le diff byte-à-byte | 2 tests + garde de debug ; 600 programmes + 476 cas, 0 désaccord |
| Phase 5 (traps `rt`) | `PARTIAL` : 5.1-5.3 faits, 5.4 `NOT_RUN` | 5.4 exigerait une API de slice sûre (`get`/`set`) + son codegen, hors RFC | gate + 10 tests unitaires + 5 cas ; `examples/dsp-gain-iir` inchangé |
| Phase 8 (bloc DSP) | `NOT_RUN` | ABI + protocole + codegen, hors budget de session | RFC écrite, ABI lue |
| Mode long nominal de l'oracle (30 000) | **fait** | exécuté localement au round 8 (`aa88479`) : 3 × 10 000, **0 divergence**, 8 602 s ; 3 000 comparaisons debug == release | commande §4, détail `T2-FINDINGS.md` §26.4 |
| Miri | `NOT_RUN` | composant `cargo-miri` absent de la toolchain nightly locale ; l'installer serait un outil supplémentaire (R5) ; la preuve CI Linux existante n'est pas rejouée ici | — |
| Mutation | `NOT_RUN` | `cargo-mutants` absent ; l'installer serait une nouvelle dépendance (R5) | — |
| Couverture à 100 % | `NOT_RUN` | 2 112 lignes non couvertes (10 fichiers) ; `codegen/lib.rs` clos à 100/100 ; limites structurelles documentées | mesure + gap + [`COVERAGE-100-OBJECTIVE.md`](../qualification/COVERAGE-100-OBJECTIVE.md) |
| Découpage des monolithes (9.7) | `NOT_RUN` | conditionné à « tout le reste terminé » | — |

## 7. Décisions humaines restantes

1. **T4** : mandater un auditeur humain indépendant (borrow checker, codegen,
   prélude). Dossier prêt : `docs/audit/AUDIT-BRIEF.md`.
2. **T12** : Go/No-Go humain.
3. **D9** : custody de la clé de signature et publication d'`allowed_signers`.
4. **Ratifier ou refuser** les quatre RFC de cette session (ordre de priorité :
   `RFC-OWN-FACTS-1`, `RFC-RV-DSP-BLOCK-1`, `RFC-RT-TRAPS-1`, `RFC-NODE-ID-1`).
5. **D10** : dénominateur MUST (807 vs 463 atteignables) **et** objectif de
   couverture 100 % — la limite des régions `fmt::Error` est documentée dans
   [`COVERAGE-100-OBJECTIVE.md`](../qualification/COVERAGE-100-OBJECTIVE.md),
   trois options y sont proposées.
6. **D7** : contrats de la frontière FFI.
7. **`AGENTS.md`** : approuver la révision du mandat.

## 8. Ce qui n'a pas été exécuté (`NOT_RUN`) — liste consolidée

Voir le tableau §6. En plus : aucun test sous charge concurrente, aucune mesure
mémoire/CPU, aucune deuxième machine, aucun macOS/ARM, aucun WASM, aucun
`ravum-lang install` Linux, aucun dogfood indépendant. Les chiffres de
couverture (89,41/88,81) ont été re-mesurés au round 10 ; `codegen/lib.rs` est à
**100/100** avec un plancher relevé à 100, `traps.rs` à 98,66/96,84 (plancher
97/95), et les seuils par fichier restent tenus. La limite structurelle des 100 %
de régions est documentée dans
[`COVERAGE-100-OBJECTIVE.md`](../qualification/COVERAGE-100-OBJECTIVE.md).

## 9. Limites

- **Une seule machine** (Windows 11 x64) : les chiffres de performance et de
  reproductibilité sont liés à cette machine et à `rustc 1.95.0`.
- **Corpus fini** : 476 cas + 37 200 programmes générés sont une observation,
  pas une preuve sur tous les programmes. L'oracle lui-même ne couvre qu'un
  sous-ensemble documenté de Core 1.0.
- **Aucune preuve formelle** : pas de Kani, pas de Miri cette session, pas
  d'audit externe. La sûreté mémoire repose sur `rustc` sous `forbid(unsafe_code)`
  (voie B), pas sur une preuve Ravum.
- **Un audit mené par un agent reste un audit interne** : T4 et T12 ne peuvent
  pas être levés par ce rapport.
