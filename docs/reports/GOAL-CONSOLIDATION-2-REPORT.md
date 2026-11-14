# GOAL « seconde vague de consolidation » — rapport final

> **Ce rapport n'est pas une déclaration de disponibilité.** Il rapporte ce qui a
> été **exécuté** dans cette session, avec la commande et le SHA, ce qui ne l'a
> pas été (`NOT_RUN`), ce qui est bloqué (`BLOCKED`) et ce qui attend une
> décision humaine (`PENDING_HUMAN_APPROVAL`). Aucun `PROVED`, aucune maturité
> « production » n'y est écrit. Les statuts autorisés sont utilisés tels quels :
> `TESTED`, `PARTIAL`, `NOT_RUN`, `UNKNOWN`, `BLOCKED`,
> `PENDING_HUMAN_APPROVAL`.
>
> Branche : `goal/consolidation-2`, créée depuis `dev` à `49cdc83` (PR #109 et
> #110 fusionnées le 2026-11-14). SHA mesuré de la session : **`77c3c0f`** ; le
> présent rapport est ajouté par le commit qui suit immédiatement (voir
> `docs/operations/sessions/GOAL-JOURNAL.md`). Machine : Windows 11 x64
> 10.0.26100, i7-13620H 10C/16T, 16 Go, `rustc 1.95.0`. L'audit d'entrée du
> 2026-11-14 est fourni avec la consigne ; il n'est **pas** suivi dans ce dépôt.

## 0. En tête : scores défavorables et régressions

1. **T2 — test de puissance : `NOT_RUN`, 0 forme sur 13 testée.** Le revert du
   plus récent correctif historique (`21cf7d4`) dans un worktree jetable
   **entre en conflit** (`emit.rs`, `backend_audit.rs`) : les commits du
   2026-11-11 ne s'appliquent plus à l'arbre courant. Aucun substitute n'a été
   exécuté. Le score n'est donc ni bon ni mauvais : il n'existe pas. Le critère
   d'acceptation de T2 n'est pas satisfait.
2. **T3 — `check` n'est pas linéaire.** De 5 110 à 50 110 lignes (×9,8), `check`
   passe de **365 ms à 26 424 ms** (×72). La phase `own` porte **97,5 %** du
   temps à 50 k lignes ; `rustc` ne pèse plus que **5,6 %** du build. L'option
   incrémentale **n'est jamais plus rapide**. La cible P0-A « p95 < 50 ms » est
   tenue sur les petits exemples et **rien ne dit qu'elle le serait à l'échelle**.
3. **T4 — la comparaison octet pour octet n'est pas satisfaite littéralement.**
   15 des 232 fichiers Rust générés diffèrent avant/après, uniquement par des
   **numéros d'identifiants dans des noms de symboles** (`__rv_root_<id>`,
   `rvm_ffi_<id>`) ; après normalisation de ces numéros, **232/232 sont
   identiques**. Le GOAL disait « toute différence = arrêt » : la comparaison
   littérale a été arrêtée, l'écart a été expliqué et vérifié, puis le travail a
   continué. C'est une **déviation assumée et rapportée**, pas un « octet pour
   octet ».
4. **T8 est `PARTIAL`** : la liste des 24 survivants et une justification écrite
   par mutant existent (`MUTATION-EQUIVALENCE.md` §9.1–§9.2, 24/24) et ont été
   revérifiées à HEAD ; le **rejeu de la campagne** est `NOT_RUN` cette session
   (`cargo-mutants` absent ; son installation = `RFC-DEV-TOOLS-1` en attente ;
   la CI demande un push, interdit). T5, T6 et T7 sont **exécutées depuis** la
   rédaction (`e3823cb`, `986f542`, `ad62f32`…`0d04106`, §5).
5. **La campagne nominale 3 × 2 000 de T2 n'est pas terminée** : elle a été
   interrompue après 561 programmes (graine 1) pour corriger la seconde
   divergence trouvée. Le total de 6 000 programmes n'est pas mesuré.
6. **T9 est partiel** : 19 déclarations d'état courant dans 3 documents sont
   contrôlées, pas tous les chiffres de `docs/reports/` et `docs/roadmap/`.
7. **Aucune mesure de couverture, de mutation, de fuzz, de Miri ni de Linux
   n'a été rejouée** dans cette session. Les chiffres antérieurs ne sont pas
   revalidés ici.
8. **Rien de ce rapport n'a été poussé** : aucun push, aucune fusion, aucun tag,
   aucune release, aucun RFC accepté.

Aucun test, aucun cas de conformance et aucun seuil n'a été supprimé, ignoré ou
affaibli. Le corpus passe de **476 à 478 cas** (deux cas ajoutés ; un cas
existant a changé de mode par obligation, voir T4), puis à **483 cas** avec les
cinq cas `rt-entry` de T6 ; le workspace de **100 suites / 1 605 tests** à
**101 suites / 1 610 tests**, puis **1 618** après T5, **1 624** après T6,
**1 627** après T7 et **1 634** après les mesures de couverture et de latence
LSP à l'échelle (§5). Aucun
plancher de couverture n'a été abaissé : la réécriture T6 avait fait passer
`traps.rs` sous son plancher, il est rétabli à **97,61/97,13** par six tests de
couverture et les totaux remontent à **89,79/89,23** (`coverage_report --check`
exit 0, §5 T6 et §8).

## 1. T1 — corriger les affirmations (Q0-2) — `TESTED`

**Commit** : `383cf5a` `docs(report): qualify the differential oracle and restore R2 to PARTIAL`.

**Modifications** : `docs/reports/GOAL-PRODUCTION-READINESS-REPORT.md`,
`docs/roadmap/CORE-1.0-GATE-MATRIX.md`, `docs/roadmap/PRODUCTION-READINESS.md`.
R2 redevient `PARTIAL` ; chaque mention des « 40 200 programmes » porte
désormais « 42 gabarits d'instructions indépendantes, sans composition » et dit
que ces nombres comptent des exécutions, pas des formes.

**Preuve** :

```
python tools/report_consistency.py
figures checked : 6
report-consistency: PASS — the report carries the register figures
```

Aucun autre changement dans ces documents (le reste de la session ne les touche
que pour les chiffres d'état courant de T9).

## 2. T2 — vrai générateur dirigé par les types (Q0-3) — `PARTIAL`

**Commits** : `40f1737` (générateur), `1121308` (divergence 1), `b7e6c7a`
(divergence 2). Détail complet : `docs/qualification/T2-FINDINGS.md` §27.

**Ce qui est ajouté** — un **second** générateur dans
`crates/ravum-lang-codegen/tests/differential_oracle.rs` ; les 42 gabarits et
leurs tests restent intacts :

- environnement de variables `{ nom, type, état }`, état ∈ `Init`, `Moved`,
  `PartMoved(champs)`, `Uninit`, avec pile de portées (shadowing) ;
- types : `i32`, `bool`, `String`, quatre tuples, `[i32; 2]`, `[i32; 3]`,
  `[String; 2]`, `Vec<i32>`, `Option<i32>`, `Option<String>`, les deux structs
  (`Pair`, `Holder`) et les deux enums (`Shape`, `Text`) de l'en-tête ;
- `gen_expr(ty, profondeur)` parmi : littéral, variable vivante (move si
  non-`Copy`), projection `x.f` / `x.0` / `x[lit.]`, appel, constructeur,
  `if/else`, `match` avec liaisons, bloc `{ let …; expr }` ; profondeur max 4,
  et 0 ⇒ littéral/variable seulement ;
- `gen_stmt` : `let` avec shadowing, affectation, `if/else`, `match`, `while`
  borné, `for i in 0..K`, bloc imbriqué ;
- **jonction de branches = intersection** (déplacée dans une branche ⇒
  considérée déplacée) ;
- **10 %** de programmes fautifs (usage après move, double move, non
  initialisée), propriété : `check` **refuse** avec `E-OWN-*`.

**Mesures publiées** (campagne rapide, 3 × 40 = 120 programmes, profondeur 4) :

| Mesure | Valeur |
|---|---|
| Sans faute | 110 |
| Acceptés par `check` | **110 / 110 = 100,0 %** |
| Refusés sans faute | 0 |
| Fautifs refusés avec `E-OWN-*` | **10 / 10 = 100 %** |
| Divergences `check → build` | 0 |
| Règles distinctes tirées | **53** (histogramme complet en T2-FINDINGS §27.2) |
| Instructions | 2 964 |
| Profondeur moyenne / max | **0,96 / 4** |

Le taux de 100 % est celui d'un générateur bien typé par construction : la
discrimination vient des fautifs et de la propriété `check ⇒ rustc`, pas d'un
taux de refus. **Deux divergences réelles ont été trouvées** — deux classes que
le générateur à 42 gabarits n'avait jamais produites :

1. `a = [a[0], …]` : `check` 0, `rustc` **E0508**. La cible d'affectation était
   comptée comme un *whole use* et bloquait le hiss du RHS. Correction
   `1121308` : une écriture n'est pas une lecture ; une région de hiss se
   termine à l'écriture. Cas `run/array-move-assign-back`.
2. move d'élément dans une branche à côté d'un *whole use* de la même
   instruction (`if false { a = if true { a } else { … }; } else { a[1] }`) :
   **E0508** sur `v1[1usize]`. Correction `b7e6c7a` : un whole use de la même
   instruction ne bloque pas le lowering inline (branches exclusives ou use
   antérieur). Cas `run/array-move-branch-reassign`.

**Test de puissance — `NOT_RUN`.** Tentative mesurée dans un worktree détaché :

```
git revert -n 21cf7d4
CONFLICT (content): Merge conflict in crates/ravum-lang-codegen/src/emit.rs
CONFLICT (content): Merge conflict in crates/ravum-lang-codegen/tests/backend_audit.rs
error: could not revert 21cf7d4...
```

Les 13 formes historiques (§22 à §24) sont réparties sur quatre lots de
correctifs (dont `21cf7d4`, `5268838`, `d693162`) qui ne s'appliquent plus. Le
substitut « désactiver le mécanisme » n'a pas été exécuté (≈ 25 min par
mécanisme, budget insuffisant). **Score exact : 0 forme testée sur 13 —
`NOT_RUN`, et non « 0 trouvée ».** Cinq des 13 formes sont de toute façon hors
de la grammaire actuelle du générateur (projections d'un seul niveau, pas de
tableau de tuples, pas de `self` sur un élément).

**Commandes** :

```
cargo test -p ravum-lang-codegen --test differential_oracle          6 passed / 2 ignored
cargo test -p ravum-lang-codegen --test differential_oracle typed_oracle_fast -- --nocapture
  TOTAL generated=120 faults=10 fault_rejected=10 accepted=110 rejected=0 divergences=0
  acceptance=100.0% rules_fired=53 statements_generated=2964 average_expr_depth=0.96 max_expr_depth=4
cargo run -p ravum-conformance -- run --verify-build
  summary: total=478 pass=477 fail=0 skip=1 not_run=0
  build: perimeter=206 covered=206 verified=123 implicit=83 failed=0
  build_verified: true
```

**Statut : `PARTIAL`** — générateur et corrections `TESTED` ; test de puissance
`NOT_RUN` ; campagne nominale incomplète.

## 3. T3 — corpus à l'échelle (Q0-4) — `TESTED`

**Commit** : `7f645d9`. **Livrables** :
`tools/gen_scale_corpus.py` (déterministe : M modules, F fonctions chaînées,
appel au module précédent, `main` qui imprime),
`scripts/lang-timings.ps1 -Scale` (check, build froid, build froid avec
`RAVUM_INCREMENTAL=1`, rebuild après édition avec et sans, pic mémoire du
processus), et `docs/reports/LANG-TIMINGS.md` §8.

Il n'existe **pas** d'option `ravum-lang build --incremental` : l'option est la
variable d'environnement `RAVUM_INCREMENTAL=1` (vérifié dans l'aide et le code).

**Commande** :
`pwsh -File scripts/lang-timings.ps1 -Scale -ScaleRuns 2 -Output target/lang-timings/scale.json`
sur `c61cb92` (T4) + scripts T3 non commités.

| Lignes | Modules | `check` p50 | pic RSS | dont `own` | build froid | dont `rustc` | incrémental froid |
|---|---|---|---|---|---|---|---|
| **50 110** | 150 | **26 423,5 ms** | **151 Mo** | 25 758,3 ms (97,5 %) | 27 632,1 ms | 1 545,9 ms (5,6 %) | 27 394,9 ms |
| **20 110** | 150 | 4 625,0 ms | 66 Mo | 4 342,6 ms (93,9 %) | 5 238,8 ms | 836,5 ms (16,0 %) | 5 538,3 ms |
| **5 110** | 150 | 364,9 ms | 21 Mo | 261,6 ms (71,7 %) | 903,0 ms | 510,5 ms (56,5 %) | 1 062,3 ms |
| **1 025** | 35 | 34,0 ms | 10 Mo | 11,5 ms (33,8 %) | 338,6 ms | 299,0 ms (88,3 %) | 469,2 ms |

Constats mesurés (sans extrapolation) : `own` est superlinéaire (×6,3 de lignes
⇒ ×22,7 de temps `own` entre 1 k et 5 k ; ×1,33 ⇒ ×5,9 entre 20 k et 50 k) ;
`rustc` est négligeable à 50 k ; l'incrémental est plus lent à froid à toutes
les tailles et au mieux égal après édition (+1,7 % à 50 k) ; mémoire 151 Mo
(check) / 236 Mo (build) à 50 k.

**Limites** : corpus synthétique régulier, **2 exécutions** par point, une
machine, M ≤ 150 (la limite de 200 était en vigueur au moment de la mesure), pic
mesuré par échantillonnage du process `ravum-lang` (les enfants `rustc` ne sont
pas comptés), `p50` issus des `timings` du CLI (pas de profileur).

**LSP de bout en bout à l'échelle (ajout, commit `379b6fe`).** Le harnais client
existant (`crates/ravum-lang-cli/tests/lsp_client.rs`) reçoit deux tests sur un
paquet généré de la même forme que `tools/gen_scale_corpus.py` (déterminisme
vérifié contre le générateur Python : 5 110 lignes, 150 modules, 152 fichiers) :
un test fonctionnel (35 modules, 1 025 lignes — définition inter-modules vers
`m34.rv`, erreur d'arité publiée puis effacée) et une mesure `#[ignore]` de 20
éditions d'une ligne. Résultat : **p50 322,4 ms / p95 337,6 ms** à 5 110 lignes,
contre **2,061 / 2,826 ms** sur `package-demo`. **La cible P0-A « p95 < 50 ms »
n'est pas tenue à cette échelle** : le coût est celui du `check` complet (365 ms
p50 à 5 110 lignes, [`LANG-TIMINGS.md`](LANG-TIMINGS.md) §9) ; l'analyse
sémantique reste globale et le cache de parse ne couvre que le parser. Écart
documenté, pas un `PASS` déguisé ; la cible reste tenue sur les petits paquets.

**Statut : `TESTED`** — les deux manques de T3 (corpus, latence LSP) sont
mesurés ; le paquet de 1 000 fichiers est couvert par
`crates/ravum-lang-sema/tests/source_file_limit.rs` (T4).

## 4. T4 — plafond de fichiers et `NodeId` 64 bits (Q1-1) — `PARTIAL`

**Commit** : `c61cb92`. **Changements** : `NodeId = u64` (32 bits hauts
`FileId`, 32 bits bas compteur par fichier) dans `ast.rs` ; encodage du parser
(`(file as u64) << 32 | 1`, tests `>> 32`) ; `MAX_SOURCE_FILES` **200 → 65 536** ;
identifiants synthétiques du loader sur un `FileId` dédié `u32::MAX - 1` (l'ancien
marqueur `0xFE` de l'octet haut entrerait en collision dès 254 fichiers) ;
conversion des ensembles d'ids de `check.rs` et de l'instantané de fuzz ;
`node_id_stability` mis à jour.

**Tests ajoutés** :

- `crates/ravum-lang-sema/tests/source_file_limit.rs` — **paquet de 1 000
  modules** (1 002 sources avec le manifeste) : chargement **et** vérification
  complets, 0 erreur. Avant, le plafond de 200 le refusait.
- Test unitaire loader `the_source_file_limit_is_refused_with_the_existing_diagnostic` :
  limite abaissée localement (thread-local) pour exercer le refus
  `E-LIMIT-FILES` et son message sans créer 65 537 fichiers.
- `conformance/cases/modules/file-limit` : **laissé** mais passé de
  `compile-fail` à `compile-pass` (201 sources sont désormais acceptées ; le
  refus est couvert par le test unitaire). C'est un changement de mode imposé
  par le nouveau contrat, pas une suppression : le cas reste, et il est
  maintenant **vérifié en build** (853 ms).

**Comparaison du Rust généré (examples/ + tout le corpus)** : 475 paquets
examinés, 232 produisent du Rust (`--emit-rust`), 243 échouent (cas
compile-fail). Avant et après, 232/232 mêmes paquets réussissent ; **15 fichiers
diffèrent littéralement**, uniquement par des numéros d'identifiants dans des
noms de symboles (`__rv_root_<id>`, `__rv_sel_<id>`, `rvm_ffi_<id>`) ; après
normalisation de ces numéros, **232/232 identiques**, 0 différence inexpliquée.
Le seul changement de *statut* est `modules/file-limit` (refusé → accepté), par
construction.

**Statut : `PARTIAL`** — la migration est complète et testée, mais le critère
littéral « octet pour octet, toute différence = arrêt » n'est pas satisfait.

## 5. T5 — `FileId` stable en session LSP (Q1-2) — `TESTED`

**Commit** : `e3823cb` `feat(lsp): keep session file ids stable across reloads`.
**Fichiers** : `crates/ravum-lang-syntax/src/source.rs`,
`crates/ravum-lang-sema/src/{loader,facts}.rs`,
`crates/ravum-lang-lsp/src/{analysis,server}.rs`, tests associés, plus la mise à
jour mécanique des `LoadOptions` littéraux (cli, codegen).

**Problème.** Chaque `NodeId` embarque le `FileId` (T4 : 32 bits hauts). Le
loader allouait les ids **dans l'ordre de découverte** (`SourceMap::add` =
position). Ajouter un module déclaré *avant* les autres — ou en retirer un —
renumérote les fichiers suivants : leur entrée de cache de parse devient
inutilisable (`(hash, FileId)`) et leurs `NodeId` changent, alors que l'utilisateur
ne les a pas touchés.

**Changement.**

- `LoadOptions::file_ids` (optionnel) : table de session `chemin canonique ->
  FileId`, **croissante et jamais réattribuée**. `LoadedPackage::file_ids` renvoie
  la table effectivement utilisée, que la session fusionne après chaque analyse.
  Le loader réutilise l'id d'un chemin connu et donne `max(id) + 1` à un chemin
  inconnu ; `ravum.toml` et les manifestes de dépendances font partie de la table
  (ce sont des `SourceFile` comme les autres).
- `SourceMap::add_at` : les ids deviennent des **valeurs, pas des positions** ;
  `files()` reste un slice trié par id (recherche binaire), et
  `facts::file_infos` remplit les trous pour que le backend continue d'indexer la
  table par `span.file` (`emit::location`), sans changer le chemin dense du CLI.
- `Analysis::run_with_cache_and_file_ids` et le champ `file_ids` de `Server` : la
  session LSP garde la table d'une analyse à l'autre ; `Analysis::run_with_cache`
  (tests, CLI) passe `file_ids: None` et garde les ids denses historiques.
- Table incohérente (un id pour deux fichiers) ⇒ **`E-IO`, chargement refusé**
  (`program` à `None`), jamais de réutilisation silencieuse.

**Preuve.** Test positif = celui du GOAL, dans
`crates/ravum-lang-lsp/tests/parse_cache.rs` : paquet à **3 modules**
(`a.rv`, `b.rv`, `c.rv`) + `main.rv`, puis un **4e module déclaré avant**
(`mod aa;` inséré avant `mod a;`). Les trois fichiers inchangés gardent leur
`FileId` et leurs `NodeId`, le cache de parse sert **3 hits** (seuls `main.rv` et
`aa.rv` sont re-parsés), et `aa.rv` prend `max + 1`. Le test négatif rejoue la
même édition **sans** table de session : ids décalés, **0 hit**, `NodeId`
changés — la preuve que le test positif n'est pas vide. Deux tests loader
couvrent la réutilisation des ids et le refus fail-closed.

```
cargo test -p ravum-lang-lsp --test parse_cache -- --test-threads=2
  test session_file_ids_keep_untouched_files_stable ... ok
  test without_a_session_table_a_new_module_renumbers_the_other_files ... ok
  test unchanged_file_is_not_reparsed ... ok
  test stale_diagnostics_are_never_reused_for_the_new_buffer ... ok
  test result: ok. 4 passed; 0 failed; 1 ignored

cargo test -p ravum-lang-sema --lib session -- --test-threads=2
  loader::tests::a_session_table_that_maps_one_id_to_two_files_fails_closed ... ok
  loader::tests::session_file_ids_survive_a_file_added_before_the_existing_ones ... ok
  test result: ok. 2 passed; 0 failed

cargo run -p ravum-conformance -- run --verify-build
  summary: total=478 pass=477 fail=0 skip=1 not_run=0
  build: perimeter=206 covered=206 verified=123 implicit=83 failed=0
```

**Effet sur le corpus** : aucun — conformance et périmètre inchangés (le CLI
n'utilise pas la table de session). Workspace : voir §8.

**Limites.** La table vit le temps du processus `ravum-lang-lsp` (aucune
persistance, aucune borne de croissance au-delà de la mémoire) ; le gain n'est
mesuré qu'in-processus, pas à travers le transport JSON-RPC ni sur plusieurs
paquets concurrents ; `SourceMap::add` et `add_at` partagent le même compteur,
mais le loader plafonne à 65 536 fichiers, donc un id proche de `u32::MAX` n'est
pas constructible par ce chemin.

### T6 — profil `rt` par point d'entrée et indexation bornée (Q1-3) — `PENDING_HUMAN_APPROVAL` (implémentation `TESTED`)

**Commit** : `986f542` `feat(rt): scope the rt gate to a named entry and bound
loop indexes`. **RFC amendée** : `docs/rfc/RFC-RT-TRAPS.md` §2 et §7.

**Ce qui est ajouté.**

- `--rt-entry <module::fonction>` sur `check` (exige `--profile rt`) et sur
  `build` (qui n'a pas `--profile` : l'option y implique l'analyse `rt`). Les
  effets interdits **et** les traps ne s'appliquent qu'aux fonctions
  **atteignables** depuis cette entrée ; `--profile rt` sans `--rt-entry` garde
  le comportement historique (paquet entier, `main`/tests).
- Atteignabilité calculée sur le graphe d'appels déjà construit par l'inférence
  d'effets (`effects::reachable_from`), partagée par les deux gates.
- Entrée inconnue : **`E-TOOL`** (code d'outillage, hors registre du langage),
  exit 1, aucune vérification exécutée — jamais un ensemble vide accepté.
- **Indexation bornée** : `a[i]` sûr si `i` est la variable d'un
  `for i in 0..K` à borne littérale (`Kusize`, l'index de tableau devant être
  `usize`) avec `K <=` taille du tableau (références dépliées), ou d'un
  `for i in 0..a.len()` sur un tableau/tranche non réaffecté et non masqué.
  Refusés : index calculé (`a[i.wrapping_add(0)]`), borne trop grande, variable
  masquée ou réaffectée, `Vec`.
- `examples/dsp-gain-iir` réécrit en buffers fixes + entrée `process` :
  `check --profile rt --rt-entry process` passe, et la **sortie est identique
  octet pour octet** à l'ancienne version (le seul écart du run complet est la
  ligne `running: <chemin>` du CLI). Smoke : **43 PASS / 0 FAIL / 0 NOT_RUN**.
  Le `ravum.lock` de l'exemple suit le digest des sources.

**Preuves.**

```
cargo test -p ravum-lang-sema --lib traps -- --test-threads=2
  17 passed / 0 failed   (5 nouveaux : portée de l'entrée sur traps et effets,
                         deux formes bornées, trois refus, entrée inconnue)
cargo test -p ravum-lang-cli -- --test-threads=2
  8 groupes : 54 + 7 + 44 + 6 + 11 + 5 + 3 + 3 passés / 0 échec
cargo run -p ravum-conformance -- run --verify-build
  483 cas : 482 PASS / 0 FAIL / 1 SKIP ; perimeter=207/207 ; verified=124
pwsh -File scripts/evidence.ps1                7 contrôles PASS
```

**Statut** : la règle et l'option sont `PENDING_HUMAN_APPROVAL` ; l'implémentation
et les preuves sont `TESTED`. L'audit demandait exactement « statut :
PENDING_HUMAN_APPROVAL ».

**Limites** : atteignabilité syntaxique, sans analyse d'alias ; l'entrée est
déclarée, pas prouvée appelée depuis un callback audio ; la règle d'indexation
est locale à deux formes et ne prouve pas les bornes en général ; `Vec<T>` reste
refusé ; les méthodes d'`impl` ne sont pas adressables ; `build` sans `--profile`
ne peut pas exprimer `pure` ; la clause « base non réaffectée » est défensive
(la réaffectation d'une tranche est déjà refusée par la propriété).

### T7 — garde OwnFacts étendu (Q1-4) — `TESTED`

**Commits** (un par exclusion) : `ad62f32` (récepteurs de méthode), `7f7f431`
(arguments de référence), `0d04106` (identité de liaison). Le garde de l'étape B
(`emit.rs::verify_ownership_facts`, **debug uniquement**) ne *saute* plus ces
trois catégories : il les vérifie.

- **Récepteurs de méthode** : la place enregistrée est comparée à la place
  syntaxique et à sa forme après auto-déréf (`xs.len()` enregistre `(*xs)`), la
  signature de l'appelé ne faisant pas partie de la syntaxe du récepteur.
- **Arguments de référence** : même règle pour un argument de type référence
  reborrowé vers un paramètre référence (`g(xs)` avec `xs: &[i32]`).
- **Identité de liaison** : `Resolution::Local` porte désormais le span de
  déclaration de la liaison, avec la même identité qu'`OwnFacts::place` (span du
  `Pat` pour un `let`/match/`for`, span de l'item englobant pour un paramètre ou
  `self`) ; le garde compare la liaison **avant** les projections.

Chaque extension a son **test négatif** qui injecte un fait faux (projection
supplémentaire sur un récepteur, sur un argument de référence, span de liaison
remplacé) et attend un `CodegenError` nommant le désaccord.

**Générateur T2 avec le garde actif** :

```
cargo test -p ravum-lang-codegen --test differential_oracle typed_oracle_fast
  TOTAL generated=120 faults=10 fault_rejected=10 accepted=110 rejected=0 divergences=0
  acceptance=100.0% rules_fired=53 statements_generated=2964 depth=0.96/4
```

**0 désaccord** sur 120 programmes (3 graines) : aucune classification
checker/backend n'était nécessaire, aucun cas à corriger. Ce n'est pas une
preuve pour un programme arbitraire (voir limites), et ce n'est pas le test de
puissance de T2.

**Limites** : garde **debug-only** (les builds release ne le paient pas) ; la
campagne exécutée est la campagne rapide de 120 programmes, pas la nominale
`3 × 2 000` (toujours `NOT_RUN` pour T2) ; le générateur actuel ne produit pas
les 13 formes historiques du test de puissance ; le garde ne vérifie que les
places *enregistrées*, pas l'absence d'un fait oublié.

**Suivi de couverture (même jour).** La réécriture T6 avait fait passer
`traps.rs` sous son plancher (86,70/78,02 < 97/95) : le gate de qualification
était rouge. Il est rétabli **sans abaisser aucun seuil** — six tests de
couverture pour l'analyse AST pure et la suppression d'une branche inatteignable
de `facts.rs` (commit `1b76926`) : `traps.rs` **97,61/97,13**, `facts.rs`
**100/100**, totaux **89,79/89,23**, `coverage_report.py --check` **exit 0**
(§8, [`COVERAGE-100-OBJECTIVE.md`](../qualification/COVERAGE-100-OBJECTIVE.md) §5).

### T8 — mutants survivants (Q1-6) — `PARTIAL`

**Ce qui existe déjà** : `docs/qualification/MUTATION-EQUIVALENCE.md` §9 (passe 7,
2026-11-11, `1376449`) liste **les 24 survivants ligne par ligne** (§9.1) et
donne pour chacun une justification écrite d'équivalence (§9.2, classes E1–E6) :
**24/24 justifiés, 0 lacune de test**. La répartition est 1 E1 (`own.rs`), 15 E2
(`resolve.rs`, bras `PathRoot::Plain`/`Self` rendus inatteignables par le
parseur), 1 E3 (`resolve.rs::type_is_copy`), 3 E4 (`typeck.rs::check_struct_lit`,
arguments génériques refusés en expression), 1 E5 (`check_const_expr`), 4 E6
(`pattern_contains_float`, filtre redondant depuis la matrice de couverture
récursive). La campagne CI du 2026-11-13 (run [43565189293](https://github.com/Rwanbt/Ravum/actions/runs/43565189293),
`b5298db`) retrouve **exactement** 183 mutants / 150 tués / 24 survivants / 9 non
viables : les mêmes familles, aucun nouveau survivant. Aucun test n'a été ajouté
— un test qui ne peut pas échouer sur le code muté serait un faux `PASS` (§9.1).

**Revérification à HEAD (`0d04106`, cette session)** : les sites et les
conditions des six classes sont toujours présents (numéros de ligne décalés) —
`own.rs::check_availability` garde `!st.partial.is_empty() && !whole_reinit` ;
`resolve.rs` garde les bras `"crate"/"self"/"super"` et le `||` de
`type_is_copy` avec `PRELUDE_ERROR_DEF` ; `typeck.rs` garde
`check_struct_lit`, `check_const_expr` (borné par `int_value_fits`),
`pattern_contains_float` et ses quatre bras ; `Boite::<i32> { … }` est toujours
refusé par le parseur (`E-PARSE-UNEXPECTED`, mesuré). Les changements T4–T7 ne
touchent aucun de ces sites.

**Rejeu de la campagne — `NOT_RUN` cette session.** `cargo mutants --version`
→ **exit 101** (outil absent localement ; son installation est l'objet de
`RFC-DEV-TOOLS-1`, `PENDING_HUMAN_APPROVAL`), et le workflow `mutation.yml` ne
peut être dispatché que sur une branche poussée — aucun push n'est autorisé.

**Statut : `PARTIAL`** — la liste et les justifications (l'exigence « un test qui
le tue, ou une justification écrite d'équivalence ») sont présentes, revérifiées
et référencées ; le rejeu de la campagne de cette session est `NOT_RUN`, avec sa
cause. Ce n'est pas un `PASS` déguisé.

## 6. T9 — source unique des chiffres (Q1-5) — `PARTIAL`

**Commit** : `77c3c0f`. **Livrables** : `scripts/evidence.ps1`,
`evidence/current.json`, `tools/report_consistency.py --evidence`.

Le script lance sept contrôles rapides et écrit un instantané (SHA, branche,
sale/propre, date UTC, OS/CPU/RAM, `rustc`, commande, code de sortie, durée,
nombres extraits, queue de sortie) :

| Contrôle | Résultat | Durée |
|---|---|---|
| `cargo fmt --all -- --check` | PASS | 1 758 ms |
| `cargo run -p ravum-conformance -- run --verify-build` | PASS | 96 335 ms |
| `python tools/registry_audit.py` | PASS | 803 ms |
| `python tools/features_coverage.py --strict` | PASS | 229 ms |
| `python tools/must_audit.py` | PASS | 265 ms |
| `python -m unittest discover -s tools/tests` | PASS (38 tests) | 1 038 ms |
| `python tools/report_consistency.py` | PASS | 179 ms |

Métriques extraites : conformance 478 / 477 PASS / 0 FAIL / 1 SKIP,
`perimeter=206`, `verified=123`, registre 108 déclarés / 126 émis,
traçabilité 188/188, MUST 807 dont 463 mappés, 38 tests Python.

`report_consistency.py --evidence` compare **19 déclarations d'état courant**
dans `docs/reports/LANGUAGE-CORE-STATUS.md`,
`docs/roadmap/CORE-1.0-GATE-MATRIX.md` et `docs/roadmap/PRODUCTION-READINESS.md`
à ce fichier ; toute valeur différente fait échouer la commande. Les sections
datées/historiques ne sont ni réécrites ni contrôlées — **la portée est donc
partielle**, et l'outil le dit.

**Statut : `PARTIAL`**.

## 7. T10 — CI sur les branches `goal/**` — `TESTED`

**Commit** : `1da5544`. `.github/workflows/linux-qual.yml` déclenche désormais
aussi sur `goal/**` (il ne le faisait que sur `main`, `dev`, `feat/**`,
`fix/**`).

**Run réel exécuté après le push (2026-11-14)** : branche poussée et PR
brouillon **#111** ouverte vers `dev`. Sur `e58ed35`, GitHub Actions exécute la
qualification de la branche `goal/consolidation-2` — **tous les jobs sont verts** :
workspace Linux et macOS arm64, **Miri** (nightly, syntax + sema, 9 min 23),
conformance Linux/Windows/macOS, couverture par fichier, smoke Linux, fuzz
smoke, wasm32-wasip1, installateur, release evidence (repro, SBOM, provenance),
mvp, impact, vscode-e2e ; TSan `NOT_APPLICABLE` (aucun thread/async en Core 1.0).

Ce run a trouvé un **vrai défaut** : l'audit des drapeaux documentés
(`scripts/cli-flag-audit.ps1`) invoquait le nouveau `--rt-entry` sans valeur ni
`--profile rt`, et le job smoke Linux échouait. Corrigé dans `e58ed35` (valeur
`main` + compagnon `--profile rt` pour `check` seulement ; `build` choisit le
profil `rt` lui-même), vérifié localement **22/22 drapeaux acceptés**, puis le
run complet est repassé au vert. Statut : `TESTED`.

## 8. Preuves d'ensemble

```
cargo fmt --all -- --check                                   exit 0
cargo test -p ravum-lang-syntax -p ravum-lang-sema -- --test-threads=2   exit 0
cargo test -p ravum-lang-lsp -- --test-threads=2             exit 0
cargo test -p ravum-lang-cli -- --test-threads=2             exit 0
cargo test -p ravum-lang-codegen -- --test-threads=2         exit 0 (35 + 45 + 6 + 2 ignorés)
cargo test -p ravum-lang-codegen --test differential_oracle typed_oracle_fast
  garde étape B actif : 120 programmes, 0 divergence, 0 désaccord
cargo test -p ravum-lang-cli --test lsp_client                        exit 0
  12 passés / 0 échec / 2 ignorés ; paquet d'échelle 1 025 lignes : définition
  inter-modules + aller-retour diagnostic
cargo test -p ravum-lang-cli --test lsp_client end_to_end_diagnostic_latency_on_the_scale_corpus -- --ignored --nocapture
  p50=322,4 ms p95=337,6 ms (5 110 lignes, 150 modules) — cible 50 ms non tenue
RAVUM_FUZZ_ITERS=100000 cargo test -p ravum-lang-sema --test fuzz_check fuzz_check_long -- --ignored
  100 000 itérations, 0 échec, 4 327/s (23,1 s)
pwsh -File scripts/repro-check.ps1                           PASS (exit 0)
  deux racines, deux target/ : sha256 9FC5E3F2…757E77 identique
cargo llvm-cov -p ravum-lang-sema -p ravum-lang-codegen --json --output-path target/cov-all.json
python tools/coverage_report.py --input target/cov-all.json --check    exit 0
  89,79 % lignes / 89,23 % régions ; traps.rs 97,61/97,13 ; facts.rs 100/100
cargo run -p ravum-conformance -- run --verify-build         exit 0
  483 cas : 482 PASS / 0 FAIL / 1 SKIP ; perimeter=207/207 ; verified=124
cargo test --workspace --all-targets -- --test-threads=2     exit 0
  101 groupes : 1 634 passés / 0 échec / 7 ignorés
pwsh -File scripts/lang-smoke.ps1                            exit 0
  43 PASS / 0 FAIL / 0 NOT_RUN
cargo run -p ravum-lang-cli -- check examples/dsp-gain-iir --profile rt --rt-entry process
  exit 0 (0 erreur) — sortie de l'exemple identique à l'ancienne version
cargo mutants --version                                      exit 101 (outil absent localement)
cargo run -q -p ravum-lang-cli -- check <turbofish>.rv       exit 1 (E-PARSE-UNEXPECTED, condition E4)
python tools/report_consistency.py --evidence                exit 0 (19 déclarations)
```

Aucune fusion, aucun tag, aucun push, aucun RFC accepté. Les 455 `ravum.lock` 
générés dans les cas de conformance par les campagnes locales ont été supprimés
(ils n'étaient pas suivis) ; les caches `.ravum` restent des artefacts ignorés.

## 9. `PENDING_HUMAN_APPROVAL`

1. **`NodeId` u64 et plafond de 65 536 fichiers** : changement de contrat de
   plateforme (layout des identifiants, limite documentée, marqueur synthétique).
   Il est implémenté et testé, mais aucune décision humaine ne l'a ratifié.
2. **`conformance/cases/modules/file-limit` passé de `compile-fail` à
   `compile-pass`** : conséquence directe du point 1 ; à confirmer.
3. **Méthode du test de puissance de T2** : accepter un substitut « mécanisme
   désactivé » (et lequel), ou exiger des commits de revert recréés.
4. **`AGENTS.md`** : la révision du 2026-11-13 reste `PENDING_HUMAN_APPROVAL`
   (inchangée par cette session).
5. **Amendement T6 de `RFC-RT-TRAPS-1`** : option `--rt-entry` (les règles `rt`
   s'appliquent aux seules fonctions atteignables) et les deux formes
   d'indexation bornée. La règle est implémentée, mesurée et documentée dans la
   RFC, mais **non ratifiée** ; l'entrée est déclarée par l'utilisateur, pas
   prouvée appelée depuis un callback temps réel.
6. **RFC et décisions antérieures toujours ouvertes** : `RFC-OWN-FACTS-1`,
   `RFC-RT-TRAPS-1`, `RFC-NODE-ID`, `RFC-RV-DSP-BLOCK-1`, `RFC-DEV-TOOLS-1`,
   D9 (clé), D10 (dénominateur MUST), objectif 100 % de couverture.

## 10. `BLOCKED` / `NOT_RUN` consolidés

- **T2, test de puissance** : `BLOCKED` par les conflits de revert (preuve
  ci-dessus) ; score non mesuré.
- **T2, campagne nominale 3 × 2 000** : `NOT_RUN` (interrompue à 561 programmes).
- **T3, LSP de bout en bout à l'échelle** : `TESTED` depuis (`379b6fe`) —
  p95 **337,6 ms** à 5 110 lignes ; **la cible P0-A « p95 < 50 ms » n'est pas
  tenue à cette échelle** (écart documenté, [`LANG-TIMINGS.md`](LANG-TIMINGS.md)
  §9) ; la cible reste tenue sur les petits paquets (2,826 ms).
- **T4, comparaison littérale octet pour octet** : déviation documentée (15
  fichiers, ids dans les noms de symboles).
- **T8** : `PARTIAL` — liste et justifications 24/24 revérifiées à HEAD ; rejeu
  de la campagne `NOT_RUN` (`cargo-mutants` absent, push interdit). **T5**,
  **T6** et **T7** : exécutées depuis la rédaction (`e3823cb`, `986f542`,
  `ad62f32`…`0d04106`, §5) ; la règle T6 reste `PENDING_HUMAN_APPROVAL`.
- **T10, run CI** : `TESTED` — PR #111, run vert sur `e58ed35` (workspace
  Linux/macOS, Miri, conformance, couverture, smoke, fuzz, wasm32, installateur,
  release evidence ; TSan `NOT_APPLICABLE`), après correction du flag audit
  (`--rt-entry`, 22/22).
- **T9, portée** : 3 documents, 19 déclarations (pas un balayage exhaustif).
- **Couverture** : `TESTED` cette session — **89,79/89,23** lignes/régions,
  cliquet rétabli (`traps.rs` **97,61/97,13**, `facts.rs` **100/100**),
  `coverage_report --check` exit 0 (§5 T6, §8).
- **Fuzz long** : `TESTED` cette session — **100 000 itérations, 0 échec**,
  4 327/s (23,1 s) à HEAD.
- **Reproductibilité** : `TESTED` cette session — deux builds, deux racines et
  deux `target/` : sha256
  `9FC5E3F2159F6E1218149AF773193C78B1CDC6C84C33085A4A93804B44875E77`.
- **Mutation** : `NOT_RUN` cette session (`cargo-mutants` absent ; installation =
  `RFC-DEV-TOOLS-1` `PENDING_HUMAN_APPROVAL`).
- **Miri, Linux** : `NOT_RUN` cette session (CI seulement, push interdit).

## 11. Questions pour le responsable

1. Le changement `NodeId` u64 / 65 536 fichiers est-il accepté comme nouveau
   contrat de plateforme, et le cas `modules/file-limit` requalifié comme
   ci-dessus ?
2. Pour le test de puissance : un substitut par désactivation de mécanisme
   est-il recevable, ou faut-il recréer les correctifs historiques en commits
   de revert sur l'arbre courant ?
3. Le rejeu de la campagne mutation doit-il être fait en CI (push autorisé), et
   faut-il autoriser l'installation locale de `cargo-mutants`
   (`RFC-DEV-TOOLS-1`) pour éviter un push à chaque rejeu ?
4. Faut-il rejouer la campagne nominale complète de l'oracle **avant** ou
   **après** le test de puissance ?
5. Le corpus synthétique doit-il être complété par un programme réel d'au moins
   3 000 lignes (décision DB de l'audit), ou les courbes synthétiques suffisent
   pour la prochaine revue ?

## 12. Ce que ce rapport ne prouve pas

- Il ne prouve aucune absence de divergence : il prouve que **deux** classes
  absentes des 42 gabarits ont été trouvées et corrigées, sur 561 programmes  d'une génération compositionnelle. Aucun nombre de programmes n'est une
  preuve de correction.
- Il ne dit rien de la performance d'un programme réel : le corpus est
  synthétique et régulier.
- Il ne qualifie pas la sûreté mémoire (Miri) ni Linux : ces mesures ne sont pas
  rejouées ici. Le LSP à l'échelle est mesuré (p95 **337,6 ms** à 5 110 lignes,
  cible 50 ms non tenue) et la reproductibilité est rejouée (**PASS**, sha256
  `9FC5E3F2…757E77`).
- T5 est prouvé **in-processus** (analyse LSP et cache de parse) : la table de
  session n'est ni persistée ni bornée, et le transport JSON-RPC n'est pas
  mesuré pour ce cas.
- T6 est une analyse **syntaxique** : l'atteignabilité suit le graphe d'appels,
  l'entrée est déclarée par l'utilisateur, et les deux formes d'indexation
  bornée ne prouvent pas les bornes en général. `Vec` reste refusé et le corpus
  `rt` reste restreint.
- T7 est un garde **debug-only**, exécuté ici sur les 120 programmes de la
  campagne rapide : il ne prouve pas l'absence d'un fait oublié et ne change
  rien aux builds release.
- T8 ne rejoue pas la campagne : les justifications E1–E6 sont des revues
  statiques revérifiées à HEAD, pas une mesure de mutants exécutée cette
  session ; 150/183 reste un chiffre ciblé, pas un score de crate.
- Il ne remplace ni l'audit externe (T4) ni le Go/No-Go (T12), qui restent des
  décisions humaines, ni aucune ratification de RFC.

## 13. Addendum du 2026-11-14 (soir) — les `NOT_RUN` de ce rapport ont été rejoués

Campagne de fermeture, rapport détaillé :
[`GOAL-CLOSING-CAMPAIGN-2026-11-14.md`](GOAL-CLOSING-CAMPAIGN-2026-11-14.md).

| Point de ce rapport | État à l'addendum |
|---|---|
| T8 — rejeu de la campagne mutation (`NOT_RUN` ici) | **rejoué en CI** (run 43622868110) : 219/180/30/9, dont la première campagne `emit.rs` (36/30/6) ; §12 de [`MUTATION-EQUIVALENCE.md`](../qualification/MUTATION-EQUIVALENCE.md) |
| T2 — campagne nominale 3 × 2 000 (`NOT_RUN` ici) | **exécutée** : 6 000 programmes, 0 rejet, 0 divergence, 2 217 s ; le test de puissance reste `BLOCKED` par sa méthode |
| Couverture (89,79/89,23 dans ce rapport) | **90,44/89,88**, produit **88,55 %** après 25 tests loader et correction de l'outil de split |
| T10 — run CI `e58ed35` | le run de la campagne finale est vert sur `0f28e43` (voir le rapport de campagne §6) ; gate-contracts a été corrigé (nom d'étape YAML) |
| Mutation locale (`cargo-mutants` absent) | inchangé : la campagne tourne en CI (`RFC-DEV-TOOLS-1` toujours ouverte) |

Les statuts de gates de ce rapport ne changent pas : l'addendum ne fait que
remplacer des `NOT_RUN` par des preuves datées.
