# MVP-0c — L1 : incrémentalité inter-crates fiable

> **Lot L1 — worker B.** Statut : **implémenté, testé, mesuré** sur la fixture
> `fixtures/multicrate`. Aucun `PROVED` n'est revendiqué : les statuts sont des
> observations reproductibles, pas une preuve de correction du programme complet.
> Preuves brutes : [`evidence/mvp0c-cross-crate-2026-11-09.json`](../../evidence/mvp0c-cross-crate-2026-11-09.json).

## 1. Problème corrigé

COLLAB-4 (`crates/ravum-cli/src/collab_cycle.rs`) construisait un graphe de crates
avec un parseur `Cargo.toml` minimal limité aux dépendances `path`, analysait
chaque unité isolément avec `--emit=metadata` **sans `--extern`**, et n'utilisait
que les hashs de sources. Conséquences : un changement de signature de `core`
peut casser `api` sans être détecté ; les dépendances indirectes ne se propagent
pas ; aucune analyse isolée ne démontre la validité de la chaîne.

L1 ajoute un crate dédié, `crates/ravum-unit-graph`, qui :

1. découvre le workspace avec l'interface **publique** `cargo metadata --no-deps
   --format-version 1` (aucun second parseur `Cargo.toml`) ;
2. sépare les identités `content_id`, `context_id`, `interface_id`, `externs`,
   `verdict` ;
3. invalide un dépendant dès que l'interface d'une dépendance directe change ;
4. compile en chaîne topologique avec `--extern dep=<.rmeta frais>` et
   `-L dependency=<dir>` ;
5. bascule sur l'oracle Cargo dès que l'indépendance ne peut pas être établie.

## 2. Modèle et identités

`UnitGraph::discover(root)` exécute `cargo metadata --no-deps --format-version 1
--offline`. Panne de `cargo` → `UNIT_GRAPH_UNAVAILABLE` (`GraphError` typé
`CargoMissing`/`CargoFailed`) : l'appelant doit basculer sur Cargo, jamais
supposer une indépendance. Le graphe est refusé (fail-closed) si une dépendance
`path` sort des membres, si un membre manque, ou si un cycle existe.

| Identité | Contenu | SHA-256 sur |
|---|---|---|
| `content_id` | `Cargo.toml` + `build.rs` + tous les `.rs` du paquet | chemins relatifs triés, longueur + octets |
| `context_id` | `rustc -vV` (toolchain_id) + target + édition + crate-name + features + flags validés | JSON canonique |
| `interface_id` | le `.rmeta` **produit** par la compilation de l'unité | octets du fichier |
| `externs` | `(nom, interface_id)` des dépendances directes | digest ordonné |
| `verdict` | clé `(content_id, context_id, externs, toolchain_id)` | SHA-256 |

Règles : un dépendant n'est **jamais** HIT sur la seule base de ses sources ; un
artefact n'est réutilisé que si la clé exacte correspond **et** si les octets du
`.rmeta` ont le SHA-256 attendu (argument corrompu → revalidation, jamais de
réutilisation silencieuse). `ContextSpec::validate` n'accepte que des flags de
compilation inoffensifs (`-A/-W/-D`, `-C opt-level/debuginfo/overflow-checks/
force-frame-pointers/panic`) : les flags porteurs d'identité (`--extern`, `-L`,
`--cfg`, `--target`, `--emit`, `-C incremental`…) sont contrôlés par le runner.

## 3. Compilation en chaîne et fallback conservateur

Ordre topologique déterministe (noms triés à égalité). Pour chaque unité `lib`
ciblable, le runner écrit dans un répertoire d'état dédié
(`target/ravum/...`, jamais les fichiers utilisateur) :

```
rustc --crate-name api --edition 2021 --crate-type lib --emit=metadata \
      -o <state>/out/api/libapi.rmeta \
      --extern core=<state>/deps/libcore.rmeta -L dependency=<state>/deps \
      <crate>/src/lib.rs
```

Un `.rmeta` frais est copié/stocké par son SHA-256 dans
`<state>/artifacts/<interface_id>.rmeta`. Le verdict est archivé sous
`<state>/cache/verdicts.json` (écriture atomique). Une campagne HIT ne lance
aucun `rustc` pour les unités ciblées.

Fallback conservateur (obligatoire, rapporté comme `fallback_package`, jamais
comme un succès incrémental) :

| Condition | Raison (`fallback_reason`) | Oracle |
|---|---|---|
| cible binaire | `binary_target` | `cargo check -p <paquet>` |
| `build.rs` / cible `custom-build` | `build_script` | `cargo check -p` |
| features déclarées, dépendance optionnelle/avec features | `features_declared` | `cargo check -p` |
| dépendance registre/git | `external_dependency` | `cargo check -p` |
| dépendance proc-macro / cible proc-macro | `proc_macro_dependency` / `proc_macro_target` | `cargo check -p` |
| source contenant `include!` | `include_macro` | `cargo check -p` |
| source contenant `env!`/`option_env!` | `env_macro` | `cargo check -p` |
| target demandé ≠ target hôte | `target_mismatch` | `cargo check` par paquet (`fallback_full`) |
| dépendance directe non ciblée/erronée | `dependency_not_targeted` | `cargo check -p` |

Statuts explicites : `targeted_ok`, `fallback_package`, `fallback_full`,
`stale`, `error`. **Aucun statut `proved`.**

Modification de source pendant la vérification : le contenu est re-hashé après la
compilation ; toute différence → statut `stale`, verdict jeté, campagne
invalidée. Un artefact altéré/supprimé dans le store → revalidation réelle.

## 4. Fixture obligatoire

`fixtures/multicrate` (workspace autonome, minuscule, `offline`) :

- `core` (lib) → `api` (lib, `pub use core::Point`) → `consumer` (lib) →
  `application` (binaire) ;
- `features` (features `default`/`extra`, non énumérables) ;
- `buildscript` (`build.rs` qui exporte une variable d'environnement).

## 5. Cas central (traces réelles)

Commande de reproduction (régénère l'évidence) :

```powershell
cargo test -p ravum-unit-graph --test central_evidence -- --test-threads=1 --nocapture
```

Changement : `core::scale(x: f64) -> f64` devient
`core::scale(x: f64, factor: f64) -> f64` (appels internes de `core` mis à jour).
Résultats mesurés (rustc 1.95.0 x86_64-pc-windows-msvc, calibration locale
mesurée dans l'évidence) :

| Scénario | Unités ciblées | `rustc` | Cargo | Verdict |
|---|---|---|---|---|
| baseline | core, api, consumer | 3 | 3 (application, buildscript, features) | `valid=true` |
| signature cassante | core `targeted_ok`, api `error` | 2 (core, api) | 4 | `valid=false` |
| ajout d'interface compatible | core, api, consumer `targeted_ok` | 3 | 3 | `valid=true` |

Sur le scénario cassant : `api` a des **sources inchangées** (`content_id`
identique) mais n'est pas HIT ; `rustc` est réellement invoqué pour `api` et le
diagnostic réel est présent (`error[E0061]: this function takes 2 arguments but
1 argument was supplied`) ; `consumer` et `application` sont revalidés par
l'oracle Cargo (échec de chaîne rapporté, jamais masqué). Aucun verdict antérieur
n'est réutilisé. Sur le scénario compatible, core → api → consumer sont
**réellement revalidés** (miss, pas HIT) et la campagne reste valide : la
propagation transitive par l'interface fonctionne.

**Déterminisme du `.rmeta`, mesuré** : deux compilations identiques dans deux
états vierges produisent le **même SHA-256** pour `core`, `api` et `consumer` sur
ce toolchain (`identical_compilations_same_sha256=true` dans l'évidence). Si un
toolchain non déterministe apparaît, la règle conservatrice documentée est de
revalider les dépendants (jamais un HIT fondé sur une identité instable).

## 6. Les 16 cas négatifs obligatoires

Suite : `crates/ravum-unit-graph/tests/negative_cases.rs` — **18 tests, 18 verts**,
tous exécutés réellement (copie de fixture en répertoire temporaire, `rustc`/
`cargo` réels).

| # | Cas | Test | Résultat observé |
|---|---|---|---|
| 1 | corps privé sans impact d'interface | `case_01_…` | `core` recompilé ; dépendants HIT ou revalidés, jamais de réutilisation périmée |
| 2 | signature publique | `case_02_…` | `api` `error` sans changement de sources, diagnostic réel, pas de HIT |
| 3 | suppression de symbole | `case_03_…` | `api` `error` (symbole manquant) |
| 4 | type partagé modifié | `case_04_…` | `api` `error` (littéral / champ ajouté) |
| 5 | implémentation de trait | `case_05_…` | `core` recompilé, chaîne valide, `api` `targeted_ok` |
| 6 | feature | `case_06_…` | `features` `fallback_package`, raison `features_declared`, trace Cargo |
| 7 | build script | `case_07_…` | `buildscript` `fallback_package`, raison `build_script` |
| 8 | dépendance externe | `case_08_…` | `core` raison `external_dependency`, oracle Cargo, jamais d'indépendance inventée |
| 9 | contrat (`assert!`) | `case_09_…` | `core` recompilé, dépendants revalidés/HIT si interface stable |
| 10 | configuration cible | `case_10_…` | `fallback_full` pour toutes les unités, aucun `rustc` ciblé |
| 11 | dépendance transitive | `case_11_…` | ajout compatible dans `core` → `api` **et** `consumer` revalidés (miss), `rustc` invoqué |
| 12 | erreur uniquement dans un dépendant | `case_12_…` | `consumer` sources inchangées mais non `targeted_ok`, campagne invalide |
| 13 | modification pendant vérification | `case_13_…` | `stale`, verdict jeté, campagne invalide |
| 14 | artefact périmé/corrompu | `case_14_…` | pas de HIT, revalidation réelle, `targeted_ok` |
| 15 | changement de compilateur | `case_15_…` | 3 HIT sur run identique ; toolchain différent → 0 HIT |
| 16 | conflit entre propositions | `case_16_…` | `MANUAL_REQUIRED` sur désaccord, `Accepted` si identiques |

Plus deux tests de garantie : `hit_is_only_granted_when_everything_matches`
(HIT ⇒ aucun `rustc` ciblé) et `rmeta_determinism_is_measured_on_the_local_toolchain`.

## 7. Extension daemon (schéma 1 → 2, externs adressés par contenu)

Le modèle d'API est stabilisé sans toucher `daemon.rs`/`gate.rs` (propriété du
worker L2, cf. plan §4.4). Extension additive dans les quatre fichiers autorisés :

- `snapshot.rs` : manifeste de snapshot **schéma 2** avec `externs` (nom, crate
  name, `interface_id`, taille) ; artefacts stockés en contenu-adressé dans
  `<snapshot>/externs/<interface_id>.rmeta` ; nouvelles API
  `SnapshotStore::materialize_with_externs`, `ExternInput::new` (valide le nom,
  la taille et le SHA-256), `verify_snapshot_with_externs` (compare l'ensemble
  attendu) ; les manifestes **schéma 1** restent lisibles (aucun extern) ;
  `verify_snapshot` vérifie l'auto-cohérence des externs stockés ;
- `rustc_driver.rs` : `CompiledExtern { name, rmeta }`, `args_with_externs` et
  `run_with_externs` (ajoutent `--extern nom=<.rmeta>` et `-L dependency=…`,
  refus fail-closed d'un nom invalide, d'un fichier manquant ou d'un doublon) ;
- `config.rs` : `SNAPSHOT_MANIFEST_SCHEMA_VERSION = 2`, bornes
  `MAX_EXTERNS_PER_UNIT=64`, `MAX_EXTERN_BYTES=8 Mio`, validateurs
  `validate_extern_name` / `validate_interface_id` ;
- `report.rs` : `ExternUsageReport` (`serde(deny_unknown_fields)`) et
  `externs_report_digest` (jamais utilisé seul comme clé de cache).

10 tests unitaires daemon ajoutés (2 config, 4 snapshot, 2 driver dont une vraie
compilation `--extern`, 2 report). Suite daemon complète : **106 tests, 0 échec**
(94 tests existants conservés + 12 nouveaux). **Limite honnête** : `daemon.analyze`
n'utilise pas encore ces externs ; le branchement de la chaîne L1 dans le daemon
est laissé à l'intégration finale (Lead / worker L2), qui possède
`daemon.rs`/`gate.rs`. Aucune performance daemon n'est revendiquée ici.

## 8. Commandes exécutées

```powershell
$env:CARGO_BUILD_JOBS=2
cargo metadata --no-deps --format-version 1 --offline          # racine + fixtures/multicrate
cargo test -p ravum-unit-graph --offline -- --test-threads=1    # 19 tests verts (18 négatifs + évidence)
cargo test -p ravum-unit-graph --test central_evidence -- --test-threads=1 --nocapture
cargo test -p ravum-daemon --offline -- --test-threads=1        # 106 tests verts
cargo build --workspace --offline
rustfmt --edition 2021 <fichiers modifiés>
```

Durées observées sur la machine partagée (calibration `rustc -vV` dans
l'évidence) : suite négative ≈ 63 s, évidence centrale ≈ 30 s, suite daemon
≈ 43 s. Les fallbacks Cargo dominent le coût : chaque paquet en fallback relance
`cargo check`, c'est le prix de la conservation (jamais de HIT inventé).

L'évidence n'est réécrite dans le dépôt que si `RAVUM_WRITE_EVIDENCE=1`
(`$env:RAVUM_WRITE_EVIDENCE='1'; cargo test -p ravum-unit-graph --test central_evidence -- --nocapture`) ;
par défaut le même JSON est écrit sous `target/ravum-evidence/`, pour qu'une
exécution de tests ne salisse jamais l'arbre de travail.

## 9. Limites et non-claims

- **Pas de preuve** : `targeted_ok` signifie « chaîne `--extern` vérifiée par
  rustc sur ces octets », jamais « programme complet correct ». Aucun `PROVED`.
- Un seul toolchain (rustc 1.95.0 x86_64-pc-windows-msvc) et un seul target
  (hôte) mesurés ; le déterminisme du `.rmeta` n'est mesuré que localement.
- Fixture minuscule et volontairement simple ; pas de grand workspace réel, pas
  de registre/git (cargo offline), pas de proc-macro réelle exécutée.
- Seules les cibles `lib` sont chaînées : binaires, features, build scripts et
  dépendances externes passent par Cargo (conservateur, plus lent).
- Le cache de verdicts est un fichier JSON local (pas de base distribuée) ; un
  cache corrompu est traité comme vide (jamais approuvé).
- La détection `include!`/`env!` est textuelle et volontairement sur-conservative
  (faux positifs possibles → fallback, jamais l'inverse).
- **Portabilité des identités** : `interface_id` est le SHA-256 du `.rmeta`, qui
  embarque le chemin absolu du workspace temporaire. Les identités sont donc
  stables pour un même chemin (test de déterminisme) mais **non portables entre
  checkouts/machines** ; c'est conservateur (aucun HIT inventé) mais limite la
  reproduction de l'évidence. Expérience du Lead : `rustc --remap-path-prefix=<racine>=/virtual`
  produit des `.rmeta` **identiques** dans deux répertoires différents (vérifié le
  2026-11-09) → piste d'amélioration mesurée, non implémentée dans ce lot
  (à faire avec régénération de l'évidence et nouveau calcul des identités).
- La chaîne L1 n'est pas encore branchée dans `ravum-cli` ni dans
  `daemon.analyze` ; l'intégration CLI/daemon reste un travail de Lead/worker L2.
- Aucun gain de performance n'est revendiqué dans ce lot : L1 est un lot de
  **fiabilité et de correction**, L2 mesurera l'accélération.
- Machine partagée avec d'autres workers : les durées incluent la charge ambiante
  (calibration mesurée et incluse dans l'évidence).

## 10. Fichiers du lot

| Fichier | Rôle |
|---|---|
| `crates/ravum-unit-graph/{Cargo.toml,src/{lib,graph,identity,archive,runner,proposals}.rs}` | crate L1 |
| `crates/ravum-unit-graph/tests/{negative_cases,central_evidence}.rs` | 16 cas + preuve centrale |
| `fixtures/multicrate/**` | fixture multi-crates |
| `crates/ravum-daemon/src/{snapshot,rustc_driver,config,report}.rs` | extension externs schéma 2 |
| `evidence/mvp0c-cross-crate-2026-11-09.json` | preuves brutes (traces réelles) |
| `Cargo.toml` (racine) | une ligne membre (tolérance de périmètre demandée au Lead) |

## 11. Attribution des sources : root package et membres imbriqués (L1.4)

> **Statut : bug reproduit, corrigé, testé.** Fixture minimale :
> [`fixtures/unit-graph-rootapp`](../../fixtures/unit-graph-rootapp) (root package
> `root_app` qui déclare aussi `[workspace]`, avec le membre imbriqué `core`).
> Preuve brute :
> [`evidence/mvp0c-unit-graph-rootapp-2026-11-09.json`](../../evidence/mvp0c-unit-graph-rootapp-2026-11-09.json).

### 11.1 Symptôme et reproduction

Sur un workspace dont le `Cargo.toml` racine est à la fois `[package]` et
`[workspace]`, `content_id` du root package incluait les sources des membres :
le root apparaissait modifié quand seul un membre changeait (« changed unit(s):
core, root_app »), invalidant inutilement le root.

Reproduction (avant correction) :

```powershell
cargo test -p ravum-unit-graph --test rootapp_attribution -- --test-threads=1 \
  root_package_does_not_aggregate_member_sources
```

Sortie observée :

```text
root_app files: [".../Cargo.toml", ".../core/Cargo.toml", ".../core/src/lib.rs", ".../src/lib.rs"]
core files:     [".../core/Cargo.toml", ".../core/src/lib.rs"]
assertion failed: root package must not own member sources
```

Cause racine : `UnitGraph::collect_source_files(root_dir)` parcourait récursivement
tout `root_dir` sans s'arrêter aux manifestes imbriqués ; le root package
agrégeait donc `core/**` en plus de ses propres fichiers (double attribution avec
le membre).

### 11.2 Correction (nearest manifest, sans double attribution)

Deux passes dans `UnitGraph::from_metadata` :

1. construire toutes les unités et l'ensemble de leurs `root_dir` ;
2. attribuer les fichiers avec `collect_source_files(root_dir, all_roots)` :
   - un répertoire imbriqué qui est une racine d'unité **ou** qui contient son
     propre `Cargo.toml` n'est jamais traversé (il appartient au paquet le plus
     proche) ;
   - seuls `Cargo.toml`, `build.rs` et `*.rs` sont retenus ; `target`, `.git`,
     `.ravum`, `node_modules` sont ignorés ;
   - chaque fichier est attribué à exactement une unité.

Le root package ne possède donc que son `Cargo.toml`, son `build.rs` et ses
`.rs` hors sous-paquets. `content_id` reste `SHA-256(Cargo.toml + build.rs +
.rs du paquet)` (aucun changement d'algorithme). Le fail-closed de découverte
(manifeste illisible, dépendance `path` invalide, cycle) est inchangé. Limite
honnête conservée : un `.rs` situé hors de toute racine d'unité reste ignoré
(comportement antérieur, pas une erreur) ; les fichiers d'un sous-paquet exclu du
workspace sont attribués à ce sous-paquet (jamais au parent), donc jamais
comptés deux fois.

### 11.3 Tests de régression

Nouveau fichier `crates/ravum-unit-graph/tests/rootapp_attribution.rs`
(2 tests réels, fixture copiée en répertoire temporaire, rustc réel) :

| Test | Vérification |
|---|---|
| `root_package_does_not_aggregate_member_sources` | `root_app.source_files = [Cargo.toml, src/lib.rs]`, `core.source_files = [core/Cargo.toml, core/src/lib.rs]`, aucun fichier attribué deux fois, le `Cargo.toml` racine n'a qu'un propriétaire |
| `member_change_keeps_root_content_id_and_root_change_keeps_member_content_id` | modification du membre ⇒ son `content_id` change, celui du root **ne change pas** ; modification du root ⇒ son `content_id` change, celui du membre ne change pas |

Chiffres mesurés (évidence, rustc 1.95.0) :

| Étape | `root_app.content_id` | `core.content_id` | Statuts |
|---|---|---|---|
| baseline | `de73cd037d6b…` | `40d270079c70…` | root `targeted_ok`, core `targeted_ok` |
| corps privé du membre modifié | `de73cd037d6b…` (**inchangé**) | `1d9c99e6d4a0…` (changé) | root revalidé comme dépendant, core recompilé |
| interface du membre modifiée (ajout public) | `de73cd037d6b…` (**inchangé**) | `cc4505e8a4b8…` (changé) | root revalidé (interface de dépendance changée), core recompilé |
| source du root modifiée | `589bee8fbf2d…` (changé) | `cc4505e8a4b8…` (inchangé) | root recompilé, **core HIT** |

Nuance honnête : le root est un **dépendant réel** du membre. Même avec un
`content_id` minimal, il est revalidé lorsque l'artefact d'interface du membre
change (c'est le comportement conservateur correct de L1) ; ce qui est corrigé,
c'est que le `content_id` du root ne dépend plus des fichiers du membre. Le
« changed » du root ne vient plus de ses sources.

### 11.4 Vérification CLI (`ravum unit-check`)

```powershell
cargo run -q -p ravum-cli -- unit-check fixtures/unit-graph-rootapp \
  --changed core/src/lib.rs --state target/ravum/rootapp-unit-check --json
```

Résultat (exit 0, `valid=true`, `workspace_unchanged=true`) :

- `changed = ["core/src/lib.rs"]` ;
- `affected` = `core` et `root_app` (règle « changed + dépendants transitifs ») ;
- `core.content_id = 40d270079c70…` (baseline) et
  `root_app.content_id = de73cd037d6b…` (**identique à la baseline**, plus de
  source de membre agrégée) ;
- les deux unités sont `targeted_ok`.

### 11.5 Limites et non-claims

- Fixture minuscule (2 unités) et un seul toolchain ; pas de `PROVED`.
- Le root peut toujours être revalidé en tant que dépendant si l'interface du
  membre change : c'est correct et conservateur, pas une régression.
- Les fichiers `.rs` hors de toute unité ne sont pas attribués (comportement
  antérieur) ; un sous-paquet non membre possède ses propres fichiers.
- La détection `include!`/`env!` reste textuelle et sur-conservative (L1).
- Aucun fichier CLI/daemon/coordinator modifié ; seuls `graph.rs`, le nouveau test,
  la fixture, cette section et l'évidence JSON.

## 12. Régénération des preuves (opt-in)

Le test de régression `rootapp_attribution` n'écrit **plus** dans `evidence/` par
défaut : l'exécution ordinaire laisse l'arbre Git propre et écrit sous
`target/ravum-evidence/` (répertoire ignoré). Même convention que
`central_evidence` et `wire_externs`.

```powershell
# exécution normale : arbre propre
cargo test -p ravum-unit-graph --offline --test rootapp_attribution -- --test-threads=1

# régénération explicite de la preuve versionnée
$env:RAVUM_WRITE_EVIDENCE = '1'
cargo test -p ravum-unit-graph --offline --test rootapp_attribution -- --test-threads=1
Remove-Item Env:RAVUM_WRITE_EVIDENCE
```

Assertions inchangées ; preuve d'hygiène :
`evidence/mvp0c-evidence-hygiene-2026-11-09.json`.
