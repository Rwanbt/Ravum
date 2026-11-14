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
4. **Cinq tâches ne sont pas exécutées** : T5 (`FileId` stable LSP), T6 (`rt` par
   point d'entrée), T7 (garde OwnFacts étendu), T8 (mutants survivants). Elles
   sont `NOT_RUN` avec leur raison en §5.
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
existant a changé de mode par obligation, voir T4) et le workspace de **100
suites / 1 605 tests** à **101 suites / 1 610 tests**.

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

## 3. T3 — corpus à l'échelle (Q0-4) — `PARTIAL`

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
sur `1da5544` + scripts T3 non commités.

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
pas comptés), `p50` issus des `timings` du CLI (pas de profileur). **LSP de bout
en bout à l'échelle : `NOT_RUN`** — aucun client n'a été écrit pour ce corpus ;
la seule mesure LSP reste celle des petits exemples (p95 2,826 ms).

**Statut : `PARTIAL`**.

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

## 5. T5 à T8 — `NOT_RUN`

| Tâche | Statut | Raison |
|---|---|---|
| T5 — `FileId` stable en session LSP | `NOT_RUN` | Non commencée. T4 fournit la base (ids stables par fichier), mais la table chemin canonique → `FileId` de session et le champ optionnel de `LoadOptions` demandent une passe dédiée (loader + LSP + test des 3 fichiers). |
| T6 — profil `rt` par point d'entrée | `NOT_RUN` | Non commencée. Amendement de RFC + option CLI `--rt-entry` + règle d'indexation bornée + cas positifs/négatifs + `examples/dsp-gain-iir` : aucun de ces éléments n'a été écrit. |
| T7 — garde OwnFacts étendu | `NOT_RUN` | Non commencée ; dépend du test de puissance de T2, qui est `NOT_RUN`. |
| T8 — mutants survivants | `NOT_RUN` | Non commencée ; demande de relire les 24 survivants et de rejouer la campagne mutation en CI. |

Aucun de ces statuts n'est un `PASS` déguisé : rien n'a été exécuté.

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

## 7. T10 — CI sur les branches `goal/**` — `TESTED` (configuration)

**Commit** : `1da5544`. `.github/workflows/linux-qual.yml` déclenche désormais
aussi sur `goal/**` (il ne le faisait que sur `main`, `dev`, `feat/**`,
`fix/**`). **Aucun run CI n'a été observé sur cette branche dans la session**
(pas de push) : la configuration est modifiée, l'effet réel est `NOT_RUN`.

## 8. Preuves d'ensemble

```
cargo fmt --all -- --check                                   exit 0
cargo test -p ravum-lang-syntax -p ravum-lang-sema -- --test-threads=2   exit 0
cargo test -p ravum-lang-codegen -- --test-threads=2         exit 0 (32 + 45 + 6 + 2 ignorés)
cargo run -p ravum-conformance -- run --verify-build         exit 0
  478 cas : 477 PASS / 0 FAIL / 1 SKIP ; perimeter=206/206 ; verified=123
cargo test --workspace --all-targets -- --test-threads=2     exit 0
  101 suites, 1 610 passés / 0 échec / 6 ignorés
python tools/report_consistency.py --evidence                exit 0 (19 déclarations)
```

Aucune fusion, aucun tag, aucun push, aucun RFC accepté. Les 24 `ravum.lock`
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
5. **RFC et décisions antérieures toujours ouvertes** : `RFC-OWN-FACTS-1`,
   `RFC-RT-TRAPS-1`, `RFC-NODE-ID`, `RFC-RV-DSP-BLOCK-1`, `RFC-DEV-TOOLS-1`,
   D9 (clé), D10 (dénominateur MUST), objectif 100 % de couverture.

## 10. `BLOCKED` / `NOT_RUN` consolidés

- **T2, test de puissance** : `BLOCKED` par les conflits de revert (preuve
  ci-dessus) ; score non mesuré.
- **T2, campagne nominale 3 × 2 000** : `NOT_RUN` (interrompue à 561 programmes).
- **T3, LSP de bout en bout à l'échelle** : `NOT_RUN` (pas de client écrit).
- **T4, comparaison littérale octet pour octet** : déviation documentée (15
  fichiers, ids dans les noms de symboles).
- **T5, T6, T7, T8** : `NOT_RUN` (non commencées).
- **T10, run CI** : `NOT_RUN` (pas de push).
- **T9, portée** : 3 documents, 19 déclarations (pas un balayage exhaustif).
- **Couverture, mutation, fuzz, Miri, Linux** : `NOT_RUN` cette session.

## 11. Questions pour le responsable

1. Le changement `NodeId` u64 / 65 536 fichiers est-il accepté comme nouveau
   contrat de plateforme, et le cas `modules/file-limit` requalifié comme
   ci-dessus ?
2. Pour le test de puissance : un substitut par désactivation de mécanisme
   est-il recevable, ou faut-il recréer les correctifs historiques en commits
   de revert sur l'arbre courant ?
3. T5-T8 doivent-elles être planifiées dans une session suivante, et dans quel
   ordre ? (T7 dépend du test de puissance ; T6 dépend d'un amendement de RFC.)
4. Faut-il rejouer la campagne nominale complète de l'oracle **avant** ou
   **après** le test de puissance ?
5. Le corpus synthétique doit-il être complété par un programme réel d'au moins
   3 000 lignes (décision DB de l'audit), ou les courbes synthétiques suffisent
   pour la prochaine revue ?

## 12. Ce que ce rapport ne prouve pas

- Il ne prouve aucune absence de divergence : il prouve que **deux** classes
  absentes des 42 gabarits ont été trouvées et corrigées, sur 561 programmes
  d'une génération compositionnelle. Aucun nombre de programmes n'est une
  preuve de correction.
- Il ne dit rien de la performance d'un programme réel : le corpus est
  synthétique et régulier.
- Il ne qualifie pas le LSP à l'échelle, ni la sûreté mémoire, ni la
  reproductibilité, ni Linux : ces mesures ne sont pas rejouées ici.
- Il ne remplace ni l'audit externe (T4) ni le Go/No-Go (T12), qui restent des
  décisions humaines, ni aucune ratification de RFC.
