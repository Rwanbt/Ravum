# GOAL « vague 3 » — décisions tranchées et exécution parallèle

> **Rapport de campagne, pas un certificat.** Dépôt `Rwanbt/Ravum`, branche
> d'intégration `goal/wave-3` (créée depuis `goal/consolidation-2`, PR #111,
> base `124e7a4`), Windows 11 x64, `rustc 1.95.0`. Le commit de code intégré
> mesuré est **`5fd54b8`** ; les chiffres de ce rapport viennent des commandes
> citées, exécutées dans cette vague. Aucun `PROVED`, aucun gate promu à `PASS`,
> aucune revendication de maturité « production ready », aucun tag, aucune
> release, aucune signature. Les décisions `D2`, `D4`, `D9`, `T12`,
> `D-2026-11-14-6` et `D-2026-11-14-7` restent **non tranchées**.

## 0. En tête — régressions et scores défavorables

1. **T2 — test de puissance : 9 formes testées sur 13, 4 `NOT_RUN`.** Score
   initial **1/9** (forme 3 seule), **4/9** après extension du générateur (formes
   3, 5, 6, 8) ; rapporté aux 13 formes historiques, **1/13 et 4/13**, jamais
   extrapolé. La campagne nominale **3 × 2 000 est `NOT_RUN`** : seules 3 × 200
   ont été exécutées. Total réellement exécuté sous inverses : **5 400
   programmes** (9 × 600), plus 1 200 en rejeu d'extension et 1 200 sur l'arbre
   sain.
2. **Une divergence `check → build` réelle a été trouvée sur l'arbre sain.**
   `run/power-array-move-write-region-nested` : `check` exit 0, `build` exit 1,
   `error[E0508]: cannot move out of type [String; 2]`. Le cas de conformance
   est **volontairement rouge** et non corrigé (S6 : signaler, ne pas corriger le
   compilateur). Conséquence : **conformance 487 cas / 485 PASS / 1 FAIL /
   1 SKIP**, `build_verified: false`, `perimeter=208`, `covered=207`,
   `not_measured=1`. C'est le seul échec de la vérification complète, et il est
   un **résultat**, pas un incident de gate.
3. **S5 — objectif de couverture non atteint.** Code produit :
   **90,37 %** (1 305 lignes non couvertes sur 13 548), contre 100 % demandés.
   7 fichiers sont sous 100 % en lignes produit : `emit.rs` 85,77 %,
   `loader.rs` 83,69 %, `own.rs` 89,86 %, `resolve.rs` 92,61 %, `typeck.rs`
   93,85 %, `effects.rs` 96,85 %, `traps.rs` 98,40 %. Les 14 exemptions
   proposées (140 lignes) sont **`PENDING_HUMAN_APPROVAL`** et ne sont pas
   défalquées : le solde hors exemptions reste ≈ **1 165 lignes**.
4. **OwnFacts étape C : `PARTIAL`.** 2 familles migrées (`receiver`,
   `arguments`), **5 familles `BLOCKED`** avec cause mesurée (`move-simple`,
   `descents`, `pattern-consumption`, `field-projection`, `literal-index`).
   `MovePlanner` reste entier ; une extension de schéma (nouvelle RFC) est
   nécessaire pour les cinq.
5. **Mutation : aucun score de crate complet.** Les 183 mutants sema sont
   reproduits à l'identique (**183 / 150 tués / 24 survivants triés / 9 non
   viables**), E7.3 et E7.4 sont **observés tués** (4/4 et 13/13). La campagne
   complète `emit.rs` a été **interrompue** après **32 mutants testés / 32 tués /
   0 survivant** ; les 4 non testés sont les mutants de formatage E7.1/E7.2.
   Aucun `outcomes.json` complet, donc aucun score de campagne `emit.rs` publié.
6. **LSP et phase `own` : cibles tenues, mais sous conditions.** `own` passe de
   l'exposant **1,9803** à **0,8372** ; `check` à 50 110 lignes passe de
   **26 879,7 ms** à **602,6 ms** (debug). LSP release : p95 **19,55 ms** à
   1 025 lignes (< 50) et **80,28 ms** à 5 110 lignes (≤ 150) sur machine
   calme, avec preuve de charge. Ce sont les deux résultats favorables
   principaux ; ils ne compensent pas les points 1 à 5.
7. **Aucun verdict de gate n'a été changé par cette vague.** `T4` et `T12`
   restent `BLOCKED` ; `T2`, `T10` et `R2` restent défavorables ou `PARTIAL`.

## 1. Décisions enregistrées

Les décisions du responsable du 2026-11-14 sont enregistrées mot pour mot dans
[`docs/decision/DECISIONS-2026-11-14.md`](../decision/DECISIONS-2026-11-14.md)
(agent D, commit `b3b4500`) : **S1–S8** et **A1–A5**, avec la mention « décisions
du responsable, transmises le 2026-11-14 ». Elles sont reportées dans
`AGENTS.md`, `DECISIONS_REQUIRED.md` §0, les 21 fiches de
`HUMAN-DECISIONS-REQUIRED.md`, les statuts des RFC concernées, la matrice des
gates (§0 decies) et `PRODUCTION-READINESS.md` §11. **Aucun verdict de gate n'a
été modifié** (preuve : §2 de la matrice identique à la base).

Restent **non tranchées**, non touchées : `D2` (ratification du socle et de
`RFC-D2`), `D4` (audit externe), `D9` (clé de signature), `T12` (Go/No-Go),
`D-2026-11-14-6`, `D-2026-11-14-7`.

## 2. Par agent — statut, commits, vérifications

Chaque ligne « relancé par l'orchestrateur » est une commande que j'ai exécutée
moi-même sur la branche de l'agent avant fusion, pas une citation de son rapport.

| Agent | Statut | Branche | Commits | Vérification indépendante |
|---|---|---|---|---|
| **A** — phase `own` | `TESTED` | `wave3/a-own-perf` | `501bb47`, `3878172`, `dbde542` | `cargo fmt` 0 ; équivalence `OwnFacts` **207 packages / 178 faits / 0 écart** ; `cargo test -p ravum-lang-sema` 0 ; conformance **484 / 483 / 0 / 1** ; tests Python **73 OK** |
| **B** — test de puissance | `PARTIAL` | `wave3/b-power-oracle` | `ead48dc`, `216569e`, `fe47d88` | `power_forms_build_on_the_healthy_tree` **1 passed** ; conformance **485 / 483 / 1 FAIL / 1 SKIP** (avant fusion de C), échec = cas nouveau attendu |
| **C** — frontière FFI | `TESTED` (périmètre) | `wave3/c-ffi-boundary` | `e39942a`, `9c4a70a`, `8faa8a2`, `b54423f` | `cargo fmt` 0 ; `cargo test -p ravum-lang-sema --test ffi` **9/9** ; conformance **486 / 485 / 0 / 1** ; `registry_audit` 108 codes, 0 non enregistré |
| **D** — décisions | `TESTED` | `wave3/d-decisions` | `b3b4500` | `must_audit` **463/807, 344 non mappés** à son SHA (puis 465/809/344 après les deux MUST FFI de C) ; `validate_repo_docs` PASS ; `report_consistency` PASS ; verdicts de gates inchangés |
| **E** — OwnFacts étape C | `PARTIAL` | `wave3/e-ownfacts-c` | `30085ea`, `47efb99`, `f76cc8b`, `dcbf4fc` | `cargo fmt` 0 ; codegen **36 + 46 + 6** tests, 0 échec ; conformance **486 / 485 / 0 / 1** ; comparaison S7 **239/239 fichiers Rust identiques**, 497 paquets |
| **F** — couverture/mutation | `PARTIAL` | `wave3/f-coverage-mutation` | `3d9bd3d`, `74292ca`, `38fbca7`, `7fe2af2`, `8846132` | `cargo fmt` 0 ; sema et codegen 0 échec ; conformance **486 / 485 / 0 / 1** ; `coverage_report --check` PASS ; produit **90,17 %** à son SHA ; `features_coverage --strict` 188/188 |
| **G** — LSP/S8 | `TESTED` | `wave3/g-lsp-targets` | `7b34cc3`, `fd42519`, `e4a388f` | tests release 1 025 et 5 110 : **p95 19,551 ms** et **80,283 ms** ; `timings_gate --lsp` **PASS** sur machine calme |

### A — la phase `own` n'est plus quadratique

L'hypothèse est confirmée par instrumentation temporaire : à 20 110 lignes, les
deux `BTreeMap::append` portent **98,00 %** de la phase `own`, `check_fn` 1,78 %.
`check_ownership` partage désormais un unique `OwnFacts` ; chaque `OwnChecker`
insère directement. Preuve d'équivalence : test comparant les tables sérialisées
et les diagnostics d'ownership sur **207 packages** (10 exemples + 197 cas
acceptés), **178 non vides, 0 différence**. `check` : **89,1 ms** à 5 110 lignes,
**271,9 ms** à 20 110, **602,6 ms** à 50 110 (debug) ; exposants **0,8372**
(50k/5k) et **0,8143** (20k/5k), sous le plafond 1,3. Le contrôle de linéarité
`timings_gate.py --scale` et le job CI correspondant sont ajoutés (11 tests de
plus, dont cas linéaire accepté et quadratique refusé).

### B — le test de puissance a trouvé un vrai défaut

Les 13 formes historiques ont été recréées par inverse écrit à la main (pas par
`git revert`, impossible depuis la réécriture d'`emit.rs`), dans des copies
jetables jamais commitées. Neuf inverses ont été compilés, le cas de conformance
associé échoue pour les neuf, et 600 programmes (3 × 200) ont été exécutés par
inverse. Le tableau complet est dans
[`T2-FINDINGS.md`](../qualification/T2-FINDINGS.md) §28. Le générateur est étendu
de 13 règles `stmt:power:*` (66 règles au total), avec un test qui vérifie que
les 13 formes sont acceptées, compilées et exécutées sur l'arbre sain.

La divergence nouvelle est réduite et enregistrée :
`conformance/cases/run/power-array-move-write-region-nested`. Elle est
**antérieure aux inverses** (retrouvée dans les neuf campagnes au même index,
puis confirmée sur l'arbre sain). **Non corrigée**, conformément à S6.

### C — `i128`/`u128` retirés des deux listes

Le vérificateur (`resolve.rs::ffi_safe`) refuse désormais `i128`/`u128` avec
`E-FFI-TYPE` ; deux cas de conformance négatifs et quatre tests FFI (dont
`c_ffi_1_verifier_refuses_i128_and_u128_before_the_backend`) le verrouillent.
L'agent E a aligné la liste jumelle du backend (`emit.rs::ast_ffi_safe`) et
ajouté le test `c_ffi_1_backend_refuses_i128_and_u128_in_a_hand_built_program` ;
`SAFETY.md` (commit `b54423f`) ne prétend plus que la liste backend est en
retard. `C-FFI-4` est écrit : une fonction C qui déroule, `longjmp`, lève un
signal ou termine le processus est **hors contrat**, obligation de l'appelant.

### D — décisions et renumérotation

Un seul commit documentaire, 16 fichiers. `AGENTS.md` perd son bandeau
`PENDING_HUMAN_APPROVAL` (S1) ; `DECISIONS_REQUIRED.md` §0 enregistre S1–S8 et
A1–A5 ; les RFC concernées passent `ACCEPTED`/`DEFERRED` ; `grammar.md` renumérote
les doublons (`G-5.14`, `G-6.48`) et échappe les barres. `must_audit` : **463/807,
344 non mappés** à ce SHA. Après les deux MUST ABI ajoutés par C, le dénominateur
passe à **809** ; les deux clauses sont tracées vers `F-FFI-003` (commit
`6b25ce6`), ce qui donne **809 / 465 mappés / 344 non mappés**.

### E — OwnFacts étape C, deux familles sur sept

S7 (normalisation limitée à `__rv_root_<id>` et `rvm_ffi_<id>`) : **497 paquets,
239 fichiers Rust émis, 239/239 identiques octet pour octet**. Familles migrées :
`receiver` (le backend lit `UseKind` au lieu de la signature et de la liste
STD-11.3 recopiée) et `arguments` (idem pour les positions d'arguments). Familles
`BLOCKED` : `move-simple` (2 fichiers changent), `descents` (1 fichier),
`pattern-consumption` (S7 identique mais l'oracle typé diverge), `field-projection`
(deux paramètres partagent le même `binding`), `literal-index` (même cause que
`descents`). Le `MovePlanner` reste donc entier ; la cause est un schéma
`OwnFacts` trop grossier, pas un simple déplacement de lecture.

### F — couverture et mutation

Trois nouveaux fichiers de tests (39 + 16 + 10 tests), **65 tests de comportement**
au total. Couverture à son SHA : globale **91,48 / 91,23**, produit **90,17 %**
(1 337 non couvertes) ; après intégration d'E et de G, la mesure finale est
**91,65 / 91,32**, produit **90,37 %** (1 305 non couvertes). Tous les seuils
par fichier ont été **relevés**, aucun abaissé (`emit.rs` 86/85 → 90/90,
`loader.rs` 76/80 → 85/88, etc.) et `coverage_report --check` passe. Les 14
exemptions (140 lignes) sont documentées avec preuve et restent
`PENDING_HUMAN_APPROVAL`. Mutation sema : **183/150/24/9**, identique à la
référence ; E7.3 et E7.4 tués ; campagne `emit.rs` complète interrompue après
**32/32** mutants tués, 4 non testés (formatage).

### G — cibles LSP S8 tenues

Mesures release, 20 éditions, transport inclus, machine calme (preuve de charge
jointe) : **1 025 lignes p95 19,551 ms** (< 50), **5 110 lignes p95 80,283 ms**
(≤ 150). Les passages chargés (p95 46,4 ms et 308,3 ms) sont publiés comme
contraste et **non retenus** : la latence ne peut qu'augmenter sous charge, les
chiffres calmes sont donc des majorants sur machine au repos. Le gate `--lsp`
est fail-closed (corpus, lignes, profil, statistiques, nombre d'éditions) et un
job CI séparé `lsp-targets` a été ajouté sans toucher au job de linéarité de A.
Risque : le runner `windows-latest` peut être plus lent que la machine de
référence ; la cible n'a pas été affaiblie.

## 3. Avant / après

| Mesure | Avant la vague | Après intégration | Commande / source |
|---|---|---|---|
| Conformance | **484 cas, 483 PASS / 0 FAIL / 1 SKIP** | **487 cas, 485 PASS / 1 FAIL / 1 SKIP** | `cargo run -p ravum-conformance -- run --verify-build` (`5fd54b8`, + `6b25ce6` docs) |
| Tests workspace | **1 660 passés / 0 échec / 7 ignorés** | **1 732 passés / 0 échec / 8 ignorés** | `cargo test --workspace --all-targets -- --test-threads=2` |
| Couverture globale | 90,44 / 89,88 | **91,65 / 91,32** | `cargo llvm-cov -p ravum-lang-sema -p ravum-lang-codegen` |
| Couverture produit (S5) | 88,55 % (1 557 / 13 598) | **90,37 %** (1 305 / 13 548) | `tools/coverage_product_split.py` |
| Mutation sema | 183 / 150 / 24 / 9 | **183 / 150 / 24 / 9** (reproduit) | `cargo mutants`, portée du workflow |
| Mutation `emit.rs` | 36 / 30 / 6 / 0 (campagne historique) | **32/32 tués** puis interruption ; E7.3/E7.4 tués | `cargo mutants`, 4 non testés |
| `check` 5 110 / 20 110 / 50 110 lignes | 365 / 4 625 / 26 424 ms | **89,1 / 271,9 / 602,6 ms** (debug) | `scripts/lang-timings.ps1 -Scale` |
| Exposant de `check` (5k→50k) | 1,9803 | **0,8372** | `tools/timings_gate.py` |
| LSP 1 025 lignes p95 | non mesuré sous cible | **19,551 ms** (< 50) | test release, machine calme |
| LSP 5 110 lignes p95 | 163,5 ms | **80,283 ms** (≤ 150) | test release, machine calme |
| MUST | 807 / 463 / 344 | **809 / 465 / 344** | `python tools/must_audit.py` |
| Smoke | 43 / 0 / 0 | **43 / 0 / 0** | `pwsh -File scripts/lang-smoke.ps1 -SkipBuild` |
| `unsafe` | 4 `forbid`, 1 `deny`, 2 `allow` | **inchangé** | `python tools/unsafe_audit.py` |
| Contrôles d'évidence | 7 PASS | **6 PASS / 1 FAIL** (conformance) | `pwsh -File scripts/evidence.ps1` |

La couverture « globale » et la couverture « produit » ne mesurent pas la même
chose : la première inclut les modules `#[cfg(test)]`, la seconde les exclut.
Les seuils ratchet de `coverage_report.py` portent sur la première ; l'objectif
S5 porte sur la seconde. Un gate de seuils vert ne prouve donc **pas** S5.

## 4. État des gates T0–T12 et des critères R1–R14

Aucun verdict n'est promu par cette vague : les commandes n'ont pas fermé les
conditions manquantes, et la nouvelle divergence aggrave T2/R2. Le registre
canonique reste [`CORE-1.0-GATE-MATRIX.md`](../roadmap/CORE-1.0-GATE-MATRIX.md).

| Gate | Verdict | Preuve de cette vague | Ce qui manque |
|---|---|---|---|
| T0 | `PASS` | inchangé | — |
| T1 | `PARTIAL` | A3 enregistrée : voie C hors 1.0, voies A/B comparées, B retenue | comparaison A/B formelle et ratification |
| T2 | `PARTIAL` | 9/13 formes, 4 `NOT_RUN` ; **1 divergence réelle** ; 809/465/344 | 4 inverses, nominale 3 × 2 000, correction de la divergence |
| T3 | `PASS` | `own` linéaire (exposant 0,837), corpus 1k/5k/20k/50k | — |
| T4 | `BLOCKED` | S7 239/239, mais aucun audit humain | **D4** |
| T5 | `PASS` | smoke 43/0/0 ; conformance build 207/208 | — |
| T6 | `PARTIAL` | S3/S4 appliqués (`--rt-entry`, FFI scalaire) | threads/réseau/async hors périmètre ; D4 |
| T7 | `PASS` | LSP 12 tests client, cibles S8 tenues, job CI | — |
| T8 | `PARTIAL` | A2 : DSP reporté 1.1 | préservation d'état, ABI de patch |
| T9 | `PARTIAL` | 10 verticales, file-store | DSP hôte, HTTP/SQLite, décision D6/D7 |
| T10 | `PARTIAL` | couverture produit 90,37 %, mutation ciblée | 100 % S5, exemptions à ratifier |
| T11 | `PARTIAL` | reproductibilité deux runners, installateur | artefacts signés (D9) |
| T12 | `BLOCKED` | dossier interne | audit D4 et Go/No-Go |

| Critère | État | Preuve / manque |
|---|---|---|
| R1 — sémantique ratifiée | `PARTIAL` | S1–S8 enregistrées ; **D2** non ratifiée |
| R2 — aucune divergence connue | **`PARTIAL`** | **une divergence connue ouverte** (`power-array-move-write-region-nested`) |
| R3 — traçabilité complète | `PARTIAL` | 188/188 FeatureIds ; 465/809 MUST, 344 non mappés |
| R4 — sûreté du frontend | `BLOCKED` | **D4** |
| R5 — frontière FFI sûre | `PARTIAL` | S4 écrite et implémentée ; C-FFI-4 hors contrat ; audit externe absent |
| R6 — code généré sûr | `TESTED` | `forbid(unsafe_code)` ; `unsafe_audit` PASS |
| R7 — reproductibilité | `PARTIAL` | deux runners, même image/toolchain ; pas inter-OS |
| R8 — qualité mesurable | `PARTIAL` | couverture 90,37 % produit ; mutation partielle |
| R9 — performance | `PARTIAL` | `check` linéaire, LSP sous cible ; `build`/`watch` sans budget |
| R10 — interfaces stables | `PARTIAL` | politique de dépréciation proposée, non ratifiée, sans CHANGELOG |
| R11 — distribution | `PARTIAL` | installateur CI ; aucun bundle signé |
| R12 — artefacts signés | `DIFFÉRÉ` | **D9** |
| R13 — audit externe | `BLOCKED` | **D4** |
| R14 — documentation | `PARTIAL` | plusieurs chiffres courants restent périmés (liste §5) |

## 5. Ce qui reste au responsable, et rien d'autre

Décisions humaines déjà connues, non tranchées : **D2**, **D4**, **D9**,
**T12**, **D-2026-11-14-6**, **D-2026-11-14-7**.

Décisions ou actions nouvelles que cette vague rend nécessaires :

1. **Politique du cas rouge.** Garder
   `run/power-array-move-write-region-nested` dans la porte de conformance
   (rouge volontaire, `build_verified: false`) ou le tenir hors gate jusqu'au
   correctif. Le cas est la preuve d'une divergence réelle ; l'option
   recommandée est de le garder et de corriger la phase de planification des
   moves de tableau en 1.0.
2. **Ratifier ou rejeter les 14 exemptions de couverture** (140 lignes). Sans
   ratification, le chiffre officiel reste **1 305 lignes non couvertes
   (90,37 %)**, et le solde hors exemptions ≈ 1 165.
3. **OwnFacts étape C (5 familles `BLOCKED`)** : autoriser une extension de
   schéma (une décision par nœud de chaîne de place, propagation de « consommé »
   à travers `match`/`if`/bloc, identité de liaison unique par paramètre), ou
   accepter que `MovePlanner` reste en place en 1.0.
4. **Mutation `emit.rs` complète** : lancer `mutation.yml` sur la CI (budget
   local insuffisant) pour couvrir les 4 mutants de formatage non testés et
   obtenir un `outcomes.json` complet.
5. **Capacité du runner LSP** : le job `lsp-targets` sur `windows-latest` peut
   être plus lent que la machine de référence ; décider runner dédié ou
   interprétation documentée d'un rouge, sans abaisser la cible.
6. **Chiffres courants périmés** : `docs/architecture/STATUS.md`,
   `docs/roadmap/CORE-1.0-GATE-MATRIX.md` (163,5 ms, 471 cas),
   `docs/reports/LANGUAGE-CORE-STATUS.md`, `RELEASE-NOTES-0.1.0-DRAFT.md`,
   `GETTING-STARTED.md`, `HUMAN-DECISIONS-REQUIRED.md` (D-2026-11-14-5 résolu
   par S8). Le présent rapport est la source datée ; la mise à jour des
   documents courants reste à faire.
7. **Dette MUST préexistante** : 344 énoncés sans FeatureId et deux lignes à
   `|` non échappé (`core-machine.md:88`, `expressions.md:254`) ; S5 les classe
   en dette tracée, pas en blocage.
8. **Fusion de la PR** : la branche `goal/wave-3` est prête pour une PR
   **brouillon** vers `dev` ; la fusion et le Go/No-Go restent humains.

## 6. Preuves de la vérification complète

```text
cargo fmt --all -- --check                                             exit 0
cargo build -p ravum-lang-cli --bin ravum-lang -j 4                    exit 0
cargo test --workspace --all-targets -- --test-threads=2               1 732 / 0 / 8  exit 0
cargo run -p ravum-conformance -- run --verify-build                   487 : 485 PASS / 1 FAIL / 1 SKIP  exit 1
pwsh -File scripts/lang-smoke.ps1 -SkipBuild                           43 PASS / 0 FAIL / 0 NOT_RUN  exit 0
pwsh -File scripts/evidence.ps1                                        6 PASS / 1 FAIL (conformance)  exit 1
python tools/validate_repo_docs.py                                     PASS  exit 0
python tools/report_consistency.py                                     PASS (6 figures historiques)  exit 0
python tools/registry_audit.py                                         108 / 126 / 0  exit 0
python tools/features_coverage.py --strict                             188/188  exit 0
python tools/unsafe_audit.py                                           PASS  exit 0
python tools/must_audit.py                                             809 / 465 / 344  exit 0
cargo llvm-cov ... --json --output-path target/cov-wave3-final.json    91,65 / 91,32  exit 0
python tools/coverage_report.py --check                                PASS  exit 0
python tools/coverage_product_split.py                                 90,37 % / 1 305  exit 0
```

Le seul échec est le cas de divergence décrit au §0.2. Aucun autre contrôle
n'échoue. Aucun test, cas de conformance, seuil ou assertion n'a été supprimé,
ignoré ou affaibli ; le seul cas rouge est un cas **ajouté** qui documente un
défaut réel.

## 7. PR

La PR est ouverte en **brouillon** vers `dev`, avec pour corps les sections 0
et 5 de ce rapport. Aucune fusion, aucun tag, aucune release, aucune signature.
