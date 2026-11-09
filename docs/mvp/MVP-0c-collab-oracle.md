# MVP-0c / L3.4 — Adaptateur réel `SemanticOracle` adossé au graphe L1

Statut : **implémenté, testé sur la fixture réelle, mesuré** (worktree non commité —
le Lead est seul écrivain Git). Nouveau crate `crates/ravum-collab-oracle`, ajouté
comme membre du workspace par une seule ligne dans `Cargo.toml` racine (+
`Cargo.lock`).

Preuves : commandes exécutées sur la machine de référence (Windows, 16 Go, autres
workers compilant en parallèle, `CARGO_BUILD_JOBS=2`, campagne sérialisée),
`rustc 1.95.0 (59807616e 2026-04-14)`, `cargo 1.95.0`. Les chiffres exacts sont
dans `evidence/mvp0c-collab-oracle-2026-11-09.json`.

---

## 1. Objectif et architecture (pas de cycle)

Le protocole v2 du coordinateur (commit `8b4e18d`) définit le trait
`SemanticOracle` mais ne fournit qu'un oracle textuel par défaut (« aucune
prétention forte »). L'analyse inter-crates L1 vit dans `ravum-unit-graph` (commit
`cecfa77`) et **ne dépend pas** du coordinateur. Le branchement réel manquait.

```
ravum-workspace-coordinator  <──  ravum-collab-oracle  ──>  ravum-unit-graph
        (trait SemanticOracle)         (adaptateur)            (graphe + chaîne rustc)
```

* `ravum-workspace-coordinator` ne dépend pas de `ravum-unit-graph` ;
* `ravum-unit-graph` ne dépend pas du coordinateur ;
* **seul** le nouveau crate dépend des deux : le cycle est impossible par
  construction.

Dépendances du crate : `ravum-workspace-coordinator`, `ravum-unit-graph`,
`ravum-snapshot` (type des snapshots de la requête d'oracle), `serde`
(`OracleCost` sérialisable) ; `ravum-protocol` uniquement en `dev-dependencies`
pour le test bout en bout. Aucune dépendance externe nouvelle.

## 2. API et branchement futur (sans modifier `ravum-cli`)

```rust
use std::sync::Arc;
use ravum_collab_oracle::UnitGraphOracle;
use ravum_workspace_coordinator::Coordinator;

// Scratch exclusif (le daemon/CLI doit fournir un répertoire par session/worker).
let oracle = Arc::new(UnitGraphOracle::new("target/ravum/collab-oracle"));
let mut coordinator = Coordinator::v2(base_snapshot)      // protocole strict
    .with_semantic_oracle(oracle.clone())                  // L1 réel
    .with_merge_verifier(declared_verifier);               // vérification déclarée

let decision = coordinator.submit(&proposal);               // jamais AUTO_SAFE sur un patch cassant
let report = coordinator.evaluations().last().unwrap().oracle.clone(); // OracleReport journalisé
```

Le binaire `ravum-collab` branchera typiquement deux options — `--oracle
unit-graph` et `--oracle-state <dir>` — et appellera
`Coordinator::v2(...).with_semantic_oracle(Arc::new(UnitGraphOracle::new(dir)))`.
Ce lot **ne modifie pas** `ravum-cli` (périmètre Lead) : le crate est un membre du
workspace prêt à être consommé.

API du crate :

| élément | rôle |
|---|---|
| `UnitGraphOracle::new(state_root)` | adaptateur ; scratch content-addressed et exclusif |
| `.with_flags(flags)` | flags rustc du contexte L1 (validés par `ravum-unit-graph`) |
| `.with_expected_toolchain(id)` | refuse l'analyse si la toolchain locale diffère (test fail-closed) |
| `.last_cost() -> Option<OracleCost>` | coût/observation de la dernière évaluation (même en échec) |
| `OracleCost` | `elapsed_ms`, `rustc_invocations`, `cargo_fallback_invocations`, `cargo_metadata_invocations`, erreurs avant/après, cache hits, unités changées/affectées — sérialisable pour la preuve |
| `impl SemanticOracle` | `name() = "unit-graph-l1"`, `evaluate(&OracleRequest) -> OracleVerdict` |

## 3. Algorithme d'une évaluation

1. **Oracle textuel d'abord** (`TextualSemanticsOracle`) : les constats de contrat
   existants (surface publique, ABI, assertions retirées) sont conservés ; un
   oracle textuel indisponible est déjà fail-closed.
2. **Lockfile** : si un chemin changé est un lockfile, retour immédiat
   `LOCKFILE_CHANGE` **bloquant** — un changement de graphe de dépendances n'est
   pas attribuable par une campagne de compilation (aucune campagne lancée).
3. **Préflight** : `cargo --version` (même résolution que L1 : `CARGO` ou `cargo`)
   puis `ContextSpec::from_rustc` (`RAVUM_RUSTC` ou `rustc`). Échec ⇒
   `Unavailable`. Toolchain attendue fournie et différente ⇒ `Unavailable`.
4. **Matérialisation** des snapshots `before`/`after` dans
   `state_root/<before-id>-<after-id>/{before,after}` (content-addressed : la même
   paire réutilise le même scratch → cache de verdicts L1 et target Cargo).
5. **Deux graphes** découverts (`cargo metadata --no-deps --offline`, 2
   invocations) ; noms d'unités comparés ⇒ un changement structurel
   (ajout/suppression d'unité) est `Unavailable` (comparaison inter-versions non
   modélisée).
6. **Deux campagnes réelles** (`run_chain`) : compilation chaînée `--extern`,
   `--emit=metadata`, artefacts `.rmeta` hashés, fallbacks Cargo explicites pour
   les unités dont l'indépendance n'est pas établie (binaires, features, build
   scripts, deps externes).
   - `stale > 0` (sources modifiées pendant la vérification) ⇒ `Unavailable` ;
   - baseline en erreur ⇒ `Unavailable` (attribution impossible) ;
   - toolchain/host incohérents entre campagnes ⇒ `Unavailable`.
7. **Identités** : par unité, `content_id` (sources) et `interface_id` (SHA-256
   du `.rmeta` réellement produit) avant/après. Une unité changée **doit** avoir
   une `interface_id` observable : une unité changée passée par un fallback Cargo
   (interface non observable) ⇒ `Unavailable` ; une unité changée qui ne compile
   pas ⇒ `Unavailable` (le patch lui-même est cassé, aucune conclusion
   d'interface).
8. **Attribution des échecs** : pour chaque unité en erreur dans la campagne
   *after* :
   - sources inchangées **et** dépendance transitive changée ⇒ `INTERFACE_BREAK`
     bloquant, unités affectées listées, chemins = unités changées, symboles =
     unités affectées, message avec le premier diagnostic réel ;
   - sources inchangées sans dépendance changée ⇒ `Unavailable` (échec non
     attribuable ; **réessai unique** avec un répertoire d'état neuf pour éviter
     qu'un incident transitoire de Cargo sous charge ne devienne une conclusion
     durable — si l'échec persiste, `Unavailable`) ;
   - unité changée en erreur ⇒ `Unavailable`.
9. **Dépendances non déclarées** (si aucune rupture bloquante) : pour chaque unité
   changée, les dépendants impactés non changés ; ceux que
   `declared_dependencies` ne couvre pas produisent `UNDECLARED_DEPENDENCY`
   **warning** (le code compile : ce n'est pas bloquant) listant les unités.
10. **Verdict** : `Findings` (textuels + graphe) si au moins un finding, sinon
    `Clear` (avec le `BoundedProof` éventuel de l'oracle textuel). **Aucun chemin
    d'incertitude ne renvoie `Clear`** — tous renvoient `Unavailable`.

## 4. Résultats réels sur `fixtures/multicrate`

Fixture : `core → api → consumer → application` + `features` (fallback features)
et `buildscript` (fallback build script). Patches unifiés réels appliqués à la
fixture en mémoire (aucun fichier fixture modifié).

| scénario | observation (test) |
|---|---|
| (a) changement compatible (corps de `internal_factor`) avec dépendants déclarés | **`Clear`** ; `changed_units=["core"]`, aucune unité affectée ; `changed_interface_units` vide ; 0 erreur avant/après |
| (a bis) même changement sans déclarer les dépendants | `Findings` **non bloquant** : `UNDECLARED_DEPENDENCY` warning listant `api`, `consumer`, `application` |
| (b) `core::scale(f64)->f64` remplacé par `scale(f32)->f32` (core patché cohérent, `api` inchangé) | **`INTERFACE_BREAK` bloquant** ; `changed_units=["core"]`, `changed_interface_units=["core"]`, `affected_units` contient `api` (et les dépendants attribuables), message avec diagnostic rustc réel |
| (c) suppression de `core::add` utilisé par `api` | **`INTERFACE_BREAK` bloquant**, `affected_units` contient `api` |
| (d) changement dans `features` (unité toujours en fallback Cargo) | **`Unavailable`** : « changed unit … conservative Cargo fallback … interface identity is not observable » |
| (e) lockfile changé | **`LOCKFILE_CHANGE` bloquant**, aucune campagne lancée (`cargo_metadata_invocations=0`) |
| (f) `cargo` absent simulé (`CARGO` inexistant) | **`Unavailable`** (« cargo is unavailable ») — jamais `Clear` |
| (g) `rustc` absent simulé (`RAVUM_RUSTC` inexistant) | **`Unavailable`** (« toolchain context is unavailable ») |
| (h) toolchain attendue différente | **`Unavailable`** (« toolchain mismatch ») |

Les scénarios (b)/(c) sont de vraies campagnes de compilation : le diagnostic
rustc est produit par `api` (source inchangée) qui appelle l'interface changée de
`core` ; l'attribution ne repose donc pas sur une comparaison de signatures.

## 5. Bout en bout `proposition → oracle → décision v2`

Test `tests/end_to_end_v2.rs` avec le `Coordinator::v2` réel :

| scénario | décision observée | niveau | preuve |
|---|---|---|---|
| patch cassant `core::scale` (utilisé par `api`) + oracle L1 injecté | **`REBASE_REVALIDATE`** (jamais `AUTO_SAFE`), rien d'intégré | `TEXTUALLY_APPLICABLE` | `OracleReport` journalisé avec `INTERFACE_BREAK` bloquant, `api` cité dans la raison, `rustc_invocations > 0`, 2 metadata |
| changement compatible + oracle L1 + **aucun vérificateur déclaré** | `MANUAL_REQUIRED` | `TEXTUALLY_APPLICABLE` | oracle non bloquant, mais l'applicabilité textuelle seule n'auto-intègre pas (règle v2) |
| changement compatible + oracle L1 + vérificateur déclaré (test double) | `AUTO_SAFE` | `SEMANTICALLY_CHECKED` | vérification déclarée `passed` + oracle non bloquant ; jamais `TESTED`/`PROVED` sans test/proof |

Le test vérifie aussi l'usage par **trait object** (`Arc<dyn SemanticOracle>`),
c'est-à-dire exactement le point d'injection du coordinateur.

## 6. Coût mesuré

Méthode : 5 évaluations de la même paire de snapshots (changement compatible
déclaré) sur `fixtures/multicrate`, `cargo test -p ravum-collab-oracle --test cost
-- --test-threads=1`, machine partagée (d'autres workers compilent),
`CARGO_BUILD_JOBS=2`. La 1re exécution est froide ; les suivantes réutilisent le
scratch content-addressed (cache de verdicts L1 + target Cargo).

Mesure finale :

```
run 1: 6981 ms | rustc=6 cargo_fallback=6 cargo_metadata=2 cache_hits=0
run 2: 4629 ms | rustc=0 cargo_fallback=6 cargo_metadata=2 cache_hits=6
run 3: 5346 ms | rustc=0 cargo_fallback=6 cargo_metadata=2 cache_hits=6
run 4: 5079 ms | rustc=0 cargo_fallback=6 cargo_metadata=2 cache_hits=6
run 5: 4431 ms | rustc=0 cargo_fallback=6 cargo_metadata=2 cache_hits=6
cold_start_ms=6981 median_ms=5079 max_ms=6981
```

| indicateur | valeur mesurée |
|---|---|
| médiane (5 exécutions) | **5 079 ms** |
| maximum | **6 981 ms** (exécution froide) |
| minimum | 4 431 ms |
| invocations rustc | **6** à froid (3 unités ciblées × 2 côtés) ; **0** à chaud (verdicts en cache) |
| invocations Cargo fallback | **6** par évaluation (3 unités en fallback × 2 côtés : `application`, `features`, `buildscript`) |
| invocations `cargo metadata` | **2** par évaluation (graphes avant/après) |
| cache L1 | 6 hits à chaud |

Lecture honnête : à chaud, le coût est dominé par les **6 fallbacks `cargo check`**
imposés par la fixture (binaire, features, build script) et par les 2 `cargo
metadata`, pas par rustc (0 invocation grâce au cache de verdicts). Sur un
workspace sans unités en fallback, le coût serait plus faible ; il n'est **pas**
généralisable à un gros workspace, et la variabilité observée sous charge est
forte (3,8 s à 7,0 s pour la même opération lors d'exécutions répétées).

## 7. Limites honnêtes

1. **Une seule toolchain locale, un hôte** ; pas de cross-compilation (le L1 force
   un fallback total et l'adaptateur renverrait `Unavailable` via le contexte).
2. **Workspace local uniquement** : `cargo metadata --offline` ; les dépendances
   registre/git ne sont pas analysées (L1 bascule ces unités en fallback Cargo ;
   si une unité *changée* est concernée, `Unavailable`).
3. **La rupture d'interface est détectée par l'échec de compilation d'un dépendant
   inchangé**, pas par un diff de signatures : un changement d'interface que tous
   les dépendants tolèrent n'est pas bloquant (c'est voulu). Un changement de
   comportement compatible à la compilation n'est pas détecté par cet adaptateur.
4. **Attribution limitée** : unité changée en erreur, changement structurel du
   workspace, sources modifiées pendant la vérification, échec inexpliqué d'une
   unité inchangée ⇒ `Unavailable`, jamais `Clear`. Le réessai unique sur échec
   non attribuable est défensif ; il n'est pas exercé par un test déterministe
   (incident transitoire observé une fois sous charge).
5. **Pas de `PROVED`** : aucun chemin ne produit de preuve ; le `PROVED` borné du
   protocole v2 reste hors de portée de cet adaptateur.
6. **Coût non borné** : pas de budget de temps ; une campagne peut être longue sur
   un vrai workspace (chaque unité en erreur et chaque fallback coûte). La
   mesure ci-dessus ne vaut que pour la fixture.
7. **Scratch exclusif** : `state_root` ne doit pas être partagé par deux processus
   concurrents (documenté dans l'API) ; le futur daemon devra fournir un
   répertoire par session/worker et une politique de purge.
8. **Pas de branchement CLI/daemon dans ce lot** : `ravum-cli` et le daemon ne sont
   pas modifiés (périmètre) ; l'exemple d'injection est fourni (§2).
9. **Fixture réduite** : 6 unités, aucune dépendance externe ; les chiffres ne
   sont pas une mesure de performance d'un projet réel.

## 8. Fichiers et commandes

Nouveau crate : `crates/ravum-collab-oracle/` (`Cargo.toml`, `src/lib.rs`,
`tests/support/mod.rs`, `tests/l1_interface_breaks.rs`, `tests/unavailable.rs`,
`tests/end_to_end_v2.rs`, `tests/cost.rs`).
Hors crate : `Cargo.toml` racine (**une ligne** : member), `Cargo.lock` (généré),
ce document, `evidence/mvp0c-collab-oracle-2026-11-09.json`. Aucun autre fichier
modifié.

Commandes exécutées (dernière exécution) :

```
cargo test -p ravum-collab-oracle -- --test-threads=1      # vert, 14 tests
cargo build --workspace                                    # OK (nouveau member inclus)
cargo fmt -p ravum-collab-oracle -- --check                # OK, 0 warning
```

Détail des 14 tests : 2 unitaires (`lib.rs` : lockfile, matching des dépendances
déclarées), 7 L1 réels (`l1_interface_breaks.rs`), 3 bout en bout v2
(`end_to_end_v2.rs`), 1 fail-closed outils absents (`unavailable.rs`), 1 mesure
de coût (`cost.rs`). Aucun test existant modifié.

## 9. Prochaines étapes (Lead)

1. Brancher l'adaptateur dans `ravum-cli`/`ravum-collab` (`--oracle unit-graph`,
   `--oracle-state <dir>`, `--protocol v2`) ; c'est un changement de leur
   périmètre, l'API est prête.
2. Héberger le scratch dans le daemon (répertoire par session, purge, budget de
   temps par proposition) et rendre l'évaluation asynchrone/annulable.
3. Étendre l'analyse : unités à dépendances externes (nécessiterait un contexte
   Cargo complet), changements structurels, comparaison d'interfaces *sans*
   échec de compilation (pour un `UNDECLARED_DEPENDENCY` plus fin).
4. Ajouter une mesure de coût sur un vrai workspace et publier la distribution
   (pas seulement la médiane sur fixture).
5. Éventuellement un mode `TESTED`/`PROVED` borné (hors périmètre de cet
   adaptateur).
