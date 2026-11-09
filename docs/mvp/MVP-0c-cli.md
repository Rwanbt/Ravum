# MVP-0c / L5.1 — Intégration CLI : `unit-check` et oracle inter-crates dans `collab-cycle`

Statut : **implémenté, testé sur fixtures réelles, mesuré** (worktree non commité —
le Lead est seul écrivain Git). Aucune commande git mutante exécutée.

Preuves : `rustc 1.95.0 (59807616e 2026-04-14)`, `cargo 1.95.0`, Windows,
machine partagée avec d'autres workers, `CARGO_BUILD_JOBS=2`, campagnes
rustc sérialisées (`--test-threads=1`). Chiffres et sorties exactes :
`evidence/mvp0c-cli-2026-11-09.json`.

---

## 1. Objectif

Les deux briques livrées n'étaient consommées par aucun binaire :
`ravum-unit-graph` (commit `cecfa77`) et `ravum-collab-oracle` (commit
`ef8984a`). Ce lot branche le CLI (`crates/ravum-cli`), avec deux surfaces :

1. **`ravum unit-check`** — campagne inter-crates réelle (graphe + compilation
   chaînée + fallbacks Cargo), **sans jamais modifier les fichiers utilisateur** ;
2. **`ravum collab-cycle --protocol v2 --oracle unit-graph --oracle-state <dir>`**
   — injection du `UnitGraphOracle` dans le coordinateur v2 ; un patch cassant une
   interface inter-crates n'est **jamais** `AUTO_SAFE`. Sans flags : comportement
   antérieur inchangé.

## 2. Dépendances (aucune modification du manifeste racine)

`crates/ravum-cli/Cargo.toml` ajoute **deux dépendances path internes** :

```toml
ravum-unit-graph = { path = "../ravum-unit-graph" }
ravum-collab-oracle = { path = "../ravum-collab-oracle" }
```

Elles existent déjà dans le workspace (membres committés) et ne créent aucun
cycle : `ravum-cli → ravum-collab-oracle → {ravum-unit-graph,
ravum-workspace-coordinator}`. **Le `Cargo.toml` racine n'a pas besoin d'être
modifié par ce lot** (pas d'ajout dans `[workspace.dependencies]`). Les autres
modifications visibles du `Cargo.toml`/`Cargo.lock` dans `git status`
appartiennent à d'autres workers (crate `ravum-batch`), pas à ce lot.

Aucune dépendance externe nouvelle ; aucun accès réseau (`cargo metadata
--offline`, `cargo check --offline`).

---

## 3. `ravum unit-check`

```
ravum unit-check <workspace> [--changed <path>]... [--state <dir>] [--json] [--out <file>]
```

### 3.1 Pipeline réel

1. valide le répertoire et la présence de `Cargo.toml` ;
2. calcule une **empreinte de contenu** du workspace (chemins relatifs triés +
   SHA-256 par fichier) ;
3. copie le workspace dans `--state` sous un nom **content-addressed**
   (`<state>/workspaces/<12 hex>`) — les invocations Cargo/rustc ne voient que la
   copie ; les liens symboliques sont ignorés (jamais suivis) ;
4. découvre le graphe via `UnitGraph::discover` (`cargo metadata --no-deps
   --offline`, fail-closed) ;
5. détermine la portée : `--changed` → unités propriétaires + **dépendants
   transitifs** (ou toutes les unités sans `--changed`) ;
6. exécute `run_chain` : compilation chaînée `--extern` `--emit=metadata` pour les
   unités indépendantes, **fallback `cargo check`** pour les unités dont
   l'indépendance n'est pas établie (binaires, features, build scripts, deps
   externes, etc.) ;
7. re-calcule l'empreinte du workspace **original** : si elle a changé pendant
   l'analyse ⇒ `stale` (exit 3), le résultat n'est pas présenté comme valide ;
8. produit le rapport (humain ou JSON), et `--out` écrit le même contenu dans un
   fichier.

### 3.2 Codes de sortie (stables, documentés aussi dans `--help`)

| code | sens |
|---|---|
| 0 | campagne valide, **aucune erreur de compilation** (fallbacks Cargo autorisés) |
| 1 | au moins une unité en **erreur de compilation** (diagnostics rustc réels) |
| 2 | usage invalide, workspace invalide (pas de `Cargo.toml`, chemin `--changed` hors unité, argument inconnu), ou **graphe invalide** (manifest incohérent, cycle, dépendance ambiguë) |
| 3 | analyse **incertaine** : `cargo`/`rustc` indisponible, campagne impossible à exécuter, sources modifiées pendant l'analyse (stale) |

Note honnête : si une unité **hors** de la portée `--changed` a une erreur
préexistante, le code est quand même 1 (la campagne globale n'est pas valide) ;
les unités affectées sont listées dans `affected_units`.

### 3.3 Rapport JSON (`schema_version: 1`)

Champs principaux : `command`, `workspace`, `analysis_copy`, `state_dir`,
`workspace_unchanged`, `exit_code`, `changed_units`, `affected_units`,
`valid`, `errors`, `stale`, `fallback_units`, `rustc_invocations`,
`cargo_fallback_invocations`, `cache_hits`, `cache_misses`, `toolchain_id`,
`host_target`, `units[]` (`unit`, `package`, `kind`, `dir`, `status`,
`fallback_reason`, `targeted_ok`, `diagnostics`, `content_id`, `interface_id`,
`affected`), et un bloc `diagnostics` explicite.

`status` ∈ `targeted_ok`, `fallback_package`, `fallback_full`, `stale`, `error`.

### 3.4 Garanties « aucun fichier utilisateur modifié »

L'analyse tourne sur la copie ; l'empreinte avant/après du workspace original est
vérifiée à chaque exécution. Le test
`unit_check_succeeds_on_multicrate_without_touching_the_workspace` vérifie sur
`fixtures/multicrate` :
* empreinte identique avant/après ;
* pas de `Cargo.lock` ni de `target/` créé dans la fixture ;
* la campagne utilise 3 rustc ciblés + 3 fallbacks Cargo (binaire, build script,
  features) et 0 erreur.

## 4. `ravum collab-cycle` : v2 + oracle injecté

Nouveaux flags (tous optionnels, défaut = comportement antérieur) :

```
ravum collab-cycle <workspace> [--proposals <dir>] [--daemon-work <dir>]
    [--journal <path>] [--out <path>] [--passes N] [--json]
    [--protocol v1|v2] [--oracle unit-graph --oracle-state <dir>]
```

Règles de validation (fail-closed, exit 2) :
* `--oracle unit-graph` **exige** `--protocol v2` (sinon l'oracle ne serait jamais
  consulté : erreur explicite plutôt qu'un no-op silencieux) ;
* `--oracle-state` exige `--oracle unit-graph` ;
* `--protocol` n'accepte que `v1`/`v2` ; `--oracle` que `unit-graph`/`none`.

Injection : `Coordinator::new(base).with_evaluation_protocol(protocol)` puis
`.with_semantic_oracle(Arc::new(UnitGraphOracle::new(state_dir)))` si demandé.
Le rapport JSON expose `protocol`, `oracle {kind, state_dir}`, et chaque décision
porte `evaluation_level`, `oracle_blocking` et l'`evaluation` complète
(`OracleReport` avec findings et evidence ids).

**Sans flags**, le coordinateur reste en `V1_COLLAB_COMPAT` textuel : les 39
tests CLI existants (dont `collab_daemon_bridge`) sont inchangés et verts.

### 4.1 Comportement observé sur un patch cassant une interface

Fixture réelle créée par le test (root package workspace + `core` + `api`) ; la
proposition change de façon cohérente la signature publique `core::scale`
(`f64→f32`, `core` compile), `api` inchangé l'utilise. Exécution :

```
ravum collab-cycle <ws> --proposals <dir> --daemon-work <dir> --journal <path> \
  --oracle-state <dir> --protocol v2 --oracle unit-graph --json
```

Sortie réelle : **exit 1** (le cycle refuse, fail-closed), rapport :

```
protocol=V2_SEMANTIC_STATUSES oracle=unit-graph decisions=1 accepted=0 violations=1
decision=REBASE_REVALIDATE level=TEXTUALLY_APPLICABLE oracle_blocking=True
oracle_name=unit-graph-l1 findings=INTERFACE_BREAK/blocking,INTERFACE_BREAK/blocking
reasons:
  oracle finding [INTERFACE_BREAK]: … (textuel)
  oracle finding [INTERFACE_BREAK]: interface break: dependent unit(s) api fail to
    compile while their own sources are unchanged; changed unit(s): core, root_app.
    First compiler diagnostic: error[E0308]: mismatched types (evidence 94285e11…)
  interface change used elsewhere: revalidation against dependent units is required
  evaluation: TEXTUALLY_APPLICABLE (protocol V2)
```

Le même patch avec `--protocol v2` **sans** oracle reste refusé (oracle textuel
par défaut) : `REBASE_REVALIDATE`, `AUTO_SAFE` jamais émis.

Limite constatée (dans la preuve JSON) : sur un workspace où le **root package**
contient des membres imbriqués, `ravum-unit-graph` agrège les sources des membres
dans le `content_id` du root package (« changed unit(s): core, root_app »). Cela
n'affecte pas l'attribution de l'unité affectée (`api`), mais le rapport peut
surestimer les unités changées. Correction à faire côté `ravum-unit-graph`
(hors périmètre de ce lot).

---

## 5. Tests exécutés

Commandes (machine chargée, builds sérialisés) :

```
CARGO_BUILD_JOBS=2 cargo test -p ravum-cli -- --test-threads=1
```

Résultat mesuré : **47 tests passés, 0 échoué** (exit 0, 0 warning), soit :
* **39 tests existants conservés** (18 unitaires `src/main.rs`, 4
  `collab_daemon_bridge`, 3 `collab_flow`, 10 `m1_patch_loop`, 4 `m2_proof_gate`)
  — aucun test existant modifié ni supprimé. La tâche annonçait « 33 tests » ;
  la mesure réelle du dépôt à ce commit est 39 (le décompte annoncé était
  obsolète) ;
* **8 nouveaux tests** :
  * `tests/unit_check.rs` (5) : succès sur `fixtures/multicrate` sans toucher la
    fixture ; portée `--changed` (4 unités affectées, 2 exclues) ; interface
    cassée ⇒ exit 1 + diagnostic `E0308` sur `api` ; arguments invalides ⇒ exit 2
    (5 variantes) ; `--out` écrit le rapport ;
  * `tests/collab_oracle_cli.rs` (3) : v2+oracle sur patch cassant ⇒ exit 1,
    `REBASE_REVALIDATE`, jamais `AUTO_SAFE`, 2 findings `INTERFACE_BREAK`
    bloquants, `api` cité, workspace inchangé ; v2 sans oracle ⇒ refus textuel ;
    arguments oracle invalides ⇒ exit 2 (4 variantes).

Les tests lançant rustc sont exécutés avec `--test-threads=1`. Durées observées
(machine partagée, caches variables) : `unit_check.rs` ≈ 6–7 s,
`collab_oracle_cli.rs` ≈ 1–2 s à chaud (campagne L1 incluse).

## 6. Exemples d'usage pour un agent / la CI

```powershell
# Vérification inter-crates des seules unités touchées par une PR :
$changed = git diff --name-only origin/main...HEAD
$args = @('unit-check', '.', '--state', 'target/ravum/unit-check', '--json', '--out', 'target/ravum/unit-check.json')
foreach ($path in $changed) { $args += @('--changed', $path) }
ravum @args
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }   # 1 = erreurs, 2 = usage, 3 = incertain

# Revue collaborative avec oracle L1 (protocole v2, fail-closed) :
ravum collab-cycle . --proposals target/proposals --protocol v2 --oracle unit-graph \
  --oracle-state target/ravum/collab-oracle --json --out target/ravum/collab-cycle.json
```

Un `unit-check` exit 3 (incertain) ne doit jamais être interprété comme un succès.

## 7. Limites honnêtes

1. **Copie du workspace** : coût IO proportionnel à la taille du workspace
   (fichiers binaires compris), et croissance de `--state` (aucune purge
   automatique dans ce lot) ; les liens symboliques ne sont pas suivis.
2. **Toolchain locale unique**, pas de cross-compilation ; pas de réseau
   (`--offline`).
3. **Fallbacks Cargo plus lents** que la compilation ciblée et moins précis
   (interface non observable) ; `unit-check` les signale explicitement.
4. `--changed` filtre la **portée du rapport** mais la campagne compile tout le
   graphe : une erreur préexistante hors portée donne quand même exit 1.
5. **Root package imbriqué** : limitation `ravum-unit-graph` documentée §4.1
   (surestimation possible des unités changées) ; l'attribution des dépendants
   impactés reste correcte dans le cas mesuré.
6. `collab-cycle` en v2 refuse par défaut (MANUAL/REBASE) tout patch sans
   vérification déclarée : c'est la règle v2, pas une régression ; les raisons
   sont journalisées dans le rapport et dans le journal durable.
7. Aucun `PROVED`, aucune preuve générale ; aucun branchement daemon/CI par
   défaut (l'exemple §6 est fourni, non câblé).

## 8. Fichiers du lot

* `crates/ravum-cli/Cargo.toml` (2 dépendances path internes) ;
* `crates/ravum-cli/src/main.rs` (module + dispatch + usage) ;
* `crates/ravum-cli/src/unit_check.rs` (nouveau, commande complète) ;
* `crates/ravum-cli/src/collab_cycle.rs` (flags, injection, champs de rapport) ;
* `crates/ravum-cli/tests/unit_check.rs` (nouveau) ;
* `crates/ravum-cli/tests/collab_oracle_cli.rs` (nouveau) ;
* `docs/mvp/MVP-0c-cli.md` (ce document) ;
* `evidence/mvp0c-cli-2026-11-09.json`.

Aucun autre crate modifié par ce lot ; `Cargo.toml` racine non modifié par ce lot.
