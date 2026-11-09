# MVP-0c — Qualification L4 : CI par impact renforcée et conservation des tests

> **Statut** : outil et selftest renforcés, sélection prouvée sur diffs réels, workflow exercé
> en `workflow_dispatch` (2 runs réels). **Aucun commit n'a été fait par ce worker** : le Lead
> est seul écrivain Git ; les modifications de cette mission sont non commitées dans le worktree.
>
> **Non-qualification** : ce document prouve le **mécanisme de sélection de tests** et son
> fail-closed. Il ne qualifie ni un compilateur, ni une preuve, ni une ABI, ni le temps réel,
> ni une release. Aucun test n'a été supprimé, exclu, ignoré ou affaibli.

- **Branche** : `feat/mvp0c-hardening`.
- **HEAD au début de la mission** : `95069fd` ; **HEAD à la fin** : `11b5e9e` (commits du Lead
  pendant la mission) ; **tête distante utilisée par les dispatches** : `54177dd`.
- **Base MVP-0b** : `origin/feat/mvp0b-hardening` = `ea7200c`.
- **Environnement** : Windows `10.0.26100.0`, Node `v26.13.0` ; machine partagée avec
  d'autres workers (compilations concurrentes) ; **aucun test cargo local lancé** par cette
  mission (périmètre temps ~10 min, workspace en cours de modification par Worker B).

---

## 1. Objectif et périmètre

Renforcer la CI par impact de `tools/impact/impact.mjs` + `.github/workflows/mvp-impact.yml`
sans affaiblir les règles conservatrices, et **prouver** que la sélection ne saute aucun test
pertinent, y compris pour les cibles L1 en cours d'ajout (`crates/ravum-unit-graph`,
`fixtures/multicrate`).

Périmètre d'écriture effectif (conforme à la tâche) :

| Fichier | État |
|---|---|
| `tools/impact/selftest.mjs` | **modifié** (nouveaux cas + inventaire) |
| `.github/workflows/mvp-impact.yml` | **modifié** (garde selftest en CI) |
| `docs/mvp/MVP-0c-qualification.md` | **créé** (ce document) |
| `evidence/mvp0c-impact-2026-11-09.json` | **créé** (preuves JSON) |
| `tools/impact/impact.mjs` | **non modifié** (aucune règle de mapping changée) |
| `scripts/impact.ps1` | **non modifié** |
| `.github/workflows/mvp.yml`, `crates/**`, `apps/**`, `Cargo.toml` | **non touchés** |

---

## 2. Audit du mapping

### 2.1 Graphe lu à l'exécution

L'outil lit le `Cargo.toml` racine (`[workspace].members`, `[workspace.dependencies]`) puis
chaque manifeste de membre. Au worktree courant (Worker B inclus) :

- **15 paquets** = 13 crates + 2 apps, dont `ravum-unit-graph` (**non commité** à `11b5e9e`) ;
- au **HEAD commité**, 14 paquets (les 54 suites du plan §7).

### 2.2 Règles conservatrices (inchangées)

| Chemin | Issue | Justification |
|---|---|---|
| `Cargo.toml`/`Cargo.lock` (tout) | FULL | le graphe de construction peut changer |
| `.github/**` | FULL | la sélection elle-même est en jeu |
| `crates/ravum-protocol/**` | FULL | contrat/ABI partagé |
| `crates/ravum-patch-host/**` | FULL | hôte natif, effets process-global |
| `fixtures/**` (dont `fixtures/multicrate/**`) | FULL | données partagées, manifests imbriqués |
| `scripts/**` | FULL | orchestration sans mapping statique |
| `tools/impact/**` | FULL | l'outil ne certifie pas sa propre PR |
| `tools/**` autres | FULL | sémantique non modélisée |
| `.github/workflows/**` | FULL | idem CI |
| `docs/**` + `*.md` racine | sélectif | 1 commande : `python tools/validate_repo_docs.py` |
| `evidence/**` | sélectif | aucun test automatisé ne le consomme |
| `crates/<membre>/**`, `apps/<membre>/**` | sélectif | `cargo test -p <paquet>` + fermeture des dépendants |
| suppression / renommage / copie | union | source + destination (R/C) ou chemin supprimé (D) |
| chemin inconnu | FULL + `unknown_paths` | fail-closed |
| diff vide, statut `U/X/B`, manifeste illisible, base introuvable | FULL + **INCERTAIN** | échec explicite après suite de secours |

Aucune de ces règles n'a été modifiée ou affaiblie dans cette mission.

---

## 3. Renforcement du selftest (`tools/impact/selftest.mjs`)

### 3.1 Avant / après

| | Cas | Assertions | Résultat |
|---|---|---|---|
| Avant (MVP-0b, HEAD `95069fd`) | **48** | **388** | `SELFTEST PASS` |
| Après (cette mission) | **52** | **491** | **`SELFTEST PASS — 52 cas, 491 assertions, 0 échec`** |

### 3.2 Cas ajoutés / renforcés

1. **Nouveau crate `ravum-unit-graph`** (L1) : un changement dans
   `crates/ravum-unit-graph/src/graph.rs` est **sélectif** (pas FULL), exige
   `cargo test -p ravum-unit-graph -- --test-threads=1` et inclut **tous ses dépendants**
   calculés par la fermeture de référence indépendante. Si le crate est absent (non encore
   commité), le cas le dit explicitement et le test d'inventaire générique prend le relais —
   aucune invention de présence.
2. **Suppression dans `ravum-unit-graph`** : fermeture conservée (le paquet reste compilé/testé).
3. **Fixture multi-crates `fixtures/multicrate`** : `core/src/lib.rs`, `application/src/main.rs`,
   `api/Cargo.toml`, `multicrate/Cargo.toml` => **FULL conservateur** (`fixtures` ou `manifest`),
   **non incertain**, avec commande suite complète ; vérifie aussi que le fixture **n'est pas**
   un member du workspace (sinon `cargo test --workspace` le compilerait).
4. **INVENTAIRE — aucun crate silencieux** (nouveau, exigé par le mandat) :
   - tout répertoire `crates/*` ou `apps/*` portant un `Cargo.toml` **doit** être membre de
     `[workspace].members` (sinon il ne serait testé par aucune sélection) ;
   - chaque paquet du workspace doit produire **soit sa suite sélective**, **soit un FULL
     conservateur explicite** — « aucun mapping de test » = échec ;
   - les seuls FULL par paquet autorisés sont `ravum-protocol` et `ravum-patch-host`
     (tout autre FULL implicite = échec, pour auditer un futur oubli) ;
   - toute commande de test doit viser un paquet du workspace.
5. **docs-only renforcé** : en plus de « 0 test Rust + check docs », le cas exige
   **exactement une commande** (`python tools/validate_repo_docs.py`).

Le cas d'inventaire rend le selftest **auto-bloquant pour tout futur crate** : si un crate est
ajouté aux `members` sans mapping de tests, le selftest échoue (et la CI échoue via la garde
ajoutée au workflow, cf. §5).

### 3.3 Garde ajoutée au workflow

`mvp-impact.yml` exécute désormais, **avant** le calcul d'impact :

```yaml
- name: Verify the impact mapping itself (Node selftest, no cargo)
  run: node tools/impact/selftest.mjs   # échec explicite si le mapping régresse (inventaire inclus)
```

Cette garde ne dépend pas de cargo (le workspace peut être en cours de modification par un
autre worker) et rend le renforcement effectif en CI.

---

## 4. Inventaire des suites de tests et des scénarios E2E

### 4.1 54 suites au HEAD commité (cargo metadata, lecture seule)

Source : `cargo metadata --no-deps --format-version 1 --offline` (sans compilation) —
**42 cibles de test + 12 harnais doc-test = 54 suites**, conformément au plan MVP-0c §7.
Le worktree contient 3 suites supplémentaires (`ravum-unit-graph`, Worker B non commité) :
**57 suites locales**.

| Paquet | Suites (HEAD) | Paquet | Suites (HEAD) |
|---|---|---|---|
| ravum-cli | 5 | ravum-protocol | 2 |
| ravum-coordinator | 2 | ravum-rust-adapter | 3 |
| ravum-daemon | 8 | ravum-snapshot | 2 |
| ravum-demo-host | 1 | ravum-store | 2 |
| ravum-dsp-harness | 4 | ravum-todo | 4 |
| ravum-patch-host | 5 | ravum-todo-core | 2 |
| ravum-proof | 3 | ravum-workspace-coordinator | 11 |
| **Total HEAD** | **54** | ravum-unit-graph (WIP) | +3 |
| **Total worktree** | **57** | | |

Couverture par la sélection : `cargo test -p <paquet>` couvre **toutes** les suites du paquet
(cibles lib/bin/`tests/*.rs` et doc-tests) ; une décision FULL couvre les 54 (57 au worktree).
Le mapping ne peut pas exclure une suite d'un paquet sélectionné.

### 4.2 10 scénarios E2E navigateur

Source : `apps/ravum-todo/e2e/browser_e2e.mjs` (`SCENARIOS`), ids 1–10 :

| id | Scénario |
|---|---|
| 1 | chargement de l'interface (titre, formulaire, version de contrat) |
| 2 | création d'une tâche et affichage dans le DOM |
| 3 | modification du titre et mise à jour du DOM |
| 4 | suppression et disparition du DOM |
| 5 | erreur de validation (titre vide) affichée dans le DOM |
| 6 | erreur d'autorisation (401) affichée sans casser l'UI |
| 7 | incompatibilité de contrat (v2 côté serveur) → message UI explicite |
| 8 | persistance : redémarrage du serveur sur la même base et rechargement |
| 9 | migration v1 → v2 : UI fonctionnelle et données v1 visibles |
| 10 | erreur réseau (serveur arrêté) visible, aucune exception non capturée |

**Couverture** : ces 10 scénarios sont exécutés par le job `mvp` de `.github/workflows/mvp.yml`
(step `Browser E2E … -RequireBrowser`), **pas** par `mvp-impact.yml`. Aucun diff de cette
mission ne touche `apps/**` ; c'est une limite connue du chemin sélectif pour une future PR UI
(§7, limite 4), pas un test sauté silencieusement.

---

## 5. Table de sélection sur des diffs réels (lecture seule)

Commandes exécutées (git lecture seule + outil Node) :

```powershell
node tools/impact/impact.mjs --base ea7200c --head 19ce0ab --json   # A
node tools/impact/impact.mjs --base 19ce0ab --head 9c02f2f --json   # B
node tools/impact/impact.mjs --base 9c02f2f --head 95069fd --json   # C
node tools/impact/impact.mjs --base ea7200c --head 95069fd --json   # D (agrégat branche)
node tools/impact/impact.mjs --files 'crates/ravum-unit-graph/src/graph.rs' --json        # E
node tools/impact/impact.mjs --files 'fixtures/multicrate/core/src/lib.rs' --json         # F
node tools/impact/impact.mjs --base refs/heads/base-qui-nexiste-pas --json                 # G
```

| # | Diff | Fichiers modifiés | Décision | Commandes exigées | Suites couvertes |
|---|---|---|---|---|---|
| A | `ea7200c..19ce0ab` | `A docs/mvp/MVP-0c-plan.md` | **SÉLECTIVE** | `python tools/validate_repo_docs.py` | 0/57 Rust (aucun code Rust touché) — 1/1 check docs |
| B | `19ce0ab..9c02f2f` | `M scripts/run-tests.ps1` | **FULL conservateur** | `cargo test --workspace -- --test-threads=1` | **57/57** (54 au HEAD) |
| C | `9c02f2f..95069fd` | `M docs/mvp/MVP-0c-plan.md` | **SÉLECTIVE** | `python tools/validate_repo_docs.py` | 0/57 Rust — 1/1 check docs |
| D | `ea7200c..95069fd` (agrégat) | docs + scripts | **FULL conservateur** + docs | suite complète **+** check docs | **57/57** + docs |
| E | worktree : `crates/ravum-unit-graph/src/graph.rs` | 1 fichier du nouveau crate | **SÉLECTIVE** | `cargo test -p ravum-unit-graph -- --test-threads=1` | **3/57** (les 3 suites du crate) |
| F | worktree : `fixtures/multicrate/core/src/lib.rs` | fixture multi-crates | **FULL conservateur** | suite complète | **57/57** |
| G | base `refs/heads/base-qui-nexiste-pas` | — (base introuvable) | **FULL + INCERTAIN** (exit 2) | suite complète de secours | **57/57** puis échec explicite |

**Analyse de non-masquage** :

- A/C : aucun fichier Rust modifié ⇒ aucune suite Rust n'est pertinente ; le check docs est
  conservé (pas d'économie silencieuse). Un full serait du gaspillage, pas une preuve.
- B/D/F : la règle conservatrice correspondante déclenche FULL ; **aucune** des 54 suites du
  HEAD n'est omise (57 au worktree).
- E : la sélection couvre les 3 suites du nouveau crate (lib unit, `tests/negative_cases.rs`,
  doc-test) ; s'il acquiert des dépendants, la fermeture les ajoute (vérifié par le selftest et
  la référence indépendante). Aucun dépendant à ce HEAD.
- G : fail-closed — le mapping ne peut pas être prouvé, donc suite complète **puis** échec
  explicite (pas de vert silencieux).
- Aucune règle n'a été affaiblie (le mapping est inchangé, cf. §1) et aucun test n'a été
  supprimé ou masqué.

---

## 6. Exercice du workflow sans PR (`workflow_dispatch`)

Workflow déclenché sur la branche (`gh workflow run mvp-impact.yml --ref feat/mvp0c-hardening`)
contre la tête distante `54177dd` (le workflow dispatch exécute la version de la branche) :

| Run | Base fournie | Décision observée dans le log | Conclusion | Durée | URL |
|---|---|---|---|---|---|
| **42537476826** | `9c02f2f` | `full=true uncertain=false docs=true` (exit outil 0) — FULL légitime (`tools/agent-usage` non mappé + docs) | **success** | 146 s | https://github.com/Rwanbt/Ravum/actions/runs/42537476826 |
| **42537714614** | `refs/heads/base-qui-nexiste-pas` | `full=true uncertain=true` (exit outil 2) — FULL de secours puis garde d'incertitude | **failure** (attendu) | 128 s | https://github.com/Rwanbt/Ravum/actions/runs/42537714614 |

Extraits de logs réels :

```
# run 42537476826 (succès)
Base du diff: 9c02f2f
Décision: full=true uncertain=false docs=true (exit outil=0)
Documentation-only check PASS: 42 proposed ADRs and required files present.
OK: suite complète exécutée et verte (FULL légitime).

# run 42537714614 (échec attendu)
Base du diff: refs/heads/base-qui-nexiste-pas
Décision: full=true uncertain=true (exit outil=2)
::error::Mapping d'impact INCERTAIN ou erreur interne (exit outil 2). La suite complète de
secours a été exécutée, mais ce job échoue explicitement...
```

Vérification locale équivalente du fail-closed (garde de l'outil) :

```powershell
node tools/impact/impact.mjs --base refs/heads/base-qui-nexiste-pas --json
# exit 2 ; decision=full-uncertain ; commande: cargo test --workspace -- --test-threads=1
```

**Blocage PR** : la création d'une PR vers `dev` reste impossible sans simuler (cf. plan
MVP-0c §7 : `dev` ne contient pas encore les crates). Aucune PR n'a été créée, aucun merge,
aucune commande Git mutante.

---

## 7. Limites honnêtes

1. **Aucune PR vers `dev`** : le chemin `pull_request` n'est pas exercé ; seuls les chemins
   `push` (runs antérieurs) et `workflow_dispatch` le sont. Blocage documenté par le plan §7.
2. **Runs dispatch sur `54177dd`** : le worktree local a ensuite avancé (`11b5e9e`) et les
   modifications de cette mission (`selftest.mjs`, garde du workflow) ne sont **pas** dans ces
   runs — le Lead doit les committer pour qu'elles s'appliquent. La garde selftest ajoutée n'a
   donc pas encore été exécutée par GitHub Actions.
3. **Aucun test cargo local** : workspace en cours de modification par Worker B et limite de
   temps ~10 min ; l'inventaire des suites vient de `cargo metadata` (lecture seule) et les
   scénarios E2E de la lecture du source, pas d'une exécution. Les suites sont comptées, pas
   qualifiées dans ce lot.
4. **E2E navigateur hors impact** : les 10 scénarios sont exécutés par `mvp.yml` uniquement.
   Aucun diff de cette mission ne touche `apps/**`, mais une future PR UI ne serait pas couverte
   par `mvp-impact.yml` sur ce point (limite inchangée depuis MVP-0b).
5. **54 vs 57 suites** : 54 au HEAD commité (plan §7) ; 57 dans le worktree à cause du crate
   `ravum-unit-graph` non commité de Worker B.
6. **Diff agrégat D** : il résout en FULL (scripts) ; il prouve la conservation intégrale, pas
   la sélectivité sur ce contenu.
7. **Le selftest ne lit pas le contenu des tests** : il prouve que chaque crate/app a un mapping
   et que les règles conservatrices s'appliquent ; il ne peut pas prouver qu'un test
   nouvellement ajouté est pertinent, ni détecter la suppression/désactivation d'un test
   existant (le workflow, lui, ne neutralise rien).
8. **Machine partagée** : la sélection Node n'a pas de dépendance cargo, mais les runs CI
   réels ont duré 128–146 s sur des runners GitHub, pas sur la machine de référence.

---

## 8. Reproductibilité

```powershell
# 1. Selftest renforcé (indépendant de cargo, ~2 s)
node tools/impact/selftest.mjs

# 2. Inventaire des suites (lecture seule, sans compilation)
cargo metadata --no-deps --format-version 1 --offline

# 3. Table de sélection (diffs réels)
node tools/impact/impact.mjs --base ea7200c --head 19ce0ab  --json
node tools/impact/impact.mjs --base 19ce0ab --head 9c02f2f  --json
node tools/impact/impact.mjs --base 9c02f2f --head 95069fd  --json
node tools/impact/impact.mjs --base ea7200c --head 95069fd  --json
node tools/impact/impact.mjs --files 'crates/ravum-unit-graph/src/graph.rs' --json
node tools/impact/impact.mjs --files 'fixtures/multicrate/core/src/lib.rs' --json

# 4. Fail-closed base introuvable (exit 2 attendu)
node tools/impact/impact.mjs --base refs/heads/base-qui-nexiste-pas --json

# 5. Exercice CI sans PR
gh workflow run mvp-impact.yml --ref feat/mvp0c-hardening -f base=9c02f2f
gh run list --workflow mvp-impact.yml --limit 3
```

---

## 9. Conclusion

- Selftest : **48 cas / 388 assertions → 52 cas / 491 assertions**, `PASS`, avec **cas
  d'inventaire** qui échoue si un crate/app du workspace n'a pas de mapping de test.
- Nouveaux cas : `ravum-unit-graph` (suite + dépendants, suppression), `fixtures/multicrate`
  ⇒ FULL conservateur, docs-only ⇒ une seule commande.
- Table de sélection prouvée sur **3 commits réels + agrégat branche + 2 cibles L1 + cas
  fail-closed** : rien de pertinent n'est sauté, les règles conservatrices sont intactes,
  aucun test supprimé/masqué.
- Workflow exercé en `workflow_dispatch` : run **42537476826 success** (FULL légitime + docs)
  et run **42537714614 failure attendu** (base introuvable ⇒ incertain ⇒ échec explicite).
- Limites listées §7 ; la première exécution `pull_request` et la garde selftest en CI
  restent à valider après commit par le Lead.

**Preuves JSON** : [`evidence/mvp0c-impact-2026-11-09.json`](../../evidence/mvp0c-impact-2026-11-09.json).

---

## 10. Correctif CI du 2026-11-09 — dev-deps DSP et décisions `benchmarks/**`

### 10.1 Panne observée

Depuis le commit `c86d1f0` (stress DSP), le job `impact` échouait à l'étape
« Verify the impact mapping itself (Node selftest, no cargo) » :

- run **[42543351783](https://github.com/Rwanbt/Ravum/actions/runs/42543351783)** (`push`, head
  `c86d1f0`) : `failure` ;
- `SELFTEST FAIL — 52 cas, 524 assertions, 2 échec(s)` :
  - `dépendant transitif et précision: ravum-snapshot: ravum-dsp-harness ne dépend pas de ravum-snapshot et ne devrait pas être testé` ;
  - `dev-dependency: un saut, pas de propagation arbitraire (rust-adapter): dsp-harness ne dépend pas de rust-adapter et ne doit pas être testé` ;
- cascade trompeuse : le step de calcul était sauté (selftest rouge) mais les étapes
  conditionnelles `always()` lisaient des sorties vides, d'où un second échec
  `::error::aucune décision d'impact`.

### 10.2 Cause racine (changement de manifeste légitime)

`crates/ravum-dsp-harness/Cargo.toml` (commit `c86d1f0`) déclare désormais :

```toml
[dev-dependencies]
ravum-rust-adapter = { workspace = true }
ravum-snapshot = { workspace = true }
```

Le graphe d'impact le voit correctement : `ravum-dsp-harness` est **dev-dépendant direct**
de `ravum-snapshot` et de `ravum-rust-adapter`. Les deux cas du selftest codaient l'ancien
graphe (exclusion explicite de `ravum-dsp-harness`) : ce n'était **pas** un bug de l'outil,
mais des attentes périmées par un changement de manifeste légitime. Le `Cargo.toml` de
`dsp-harness` n'a **pas** été modifié.

### 10.3 Règle dev-dependency : inchangée et vérifiée

Règle (aucun affaiblissement) : une **dev-dependency directe** exige les tests du dépendant ;
la propagation s'arrête à **un seul saut** (pas de transitivité arbitraire).

| Scénario | `ravum-dsp-harness` | `ravum-demo-host` (dépendant normal du dev-hop `ravum-patch-host`) | Provenance dev-hop |
|---|---|---|---|
| changement `ravum-snapshot` | **inclus** (dev-dep directe) | **exclu** (pas de propagation) | `dev-dependent` via un paquet affecté qui est bien une dev-dep déclarée (`snapshot` ou `rust-adapter`) |
| changement `ravum-rust-adapter` | **inclus** (dev-dep directe) | **exclu** (pas de propagation) | `dev-dependent` via un paquet affecté dev-dep déclaré (`rust-adapter`) |

Le `via` de provenance peut désigner n'importe quel paquet affecté dont `dsp-harness` est
une dev-dependency directe (ici `snapshot` ou `rust-adapter`, tous deux affectés) : le
selftest vérifie la règle (via ∈ dev-deps déclarées ∧ via ∈ paquets affectés), pas un ordre
de parcours.

### 10.4 Correctif du selftest : 52/524 (FAIL) → 56/583 (PASS)

| | Cas | Assertions | Résultat |
|---|---|---|---|
| Avant correctif (run rouge) | 52 | 524 | **FAIL** (2 échecs) |
| Après correctif | **56** | **583** | **`SELFTEST PASS`** |

Cas mis à jour :

- `dépendant transitif et dev-dépendant direct: ravum-snapshot` : `ravum-dsp-harness`
  désormais **inclus** comme dev-dépendant direct, provenance `dev-dependent` vérifiée,
  `ravum-demo-host` explicitement **exclu** (preuve d'absence de propagation) ;
- `dev-dependency directe: dépendant testé, pas de propagation transitive (rust-adapter)` :
  idem pour `ravum-rust-adapter`.

Cas ajoutés :

1. `règle dev-dependency: directe => testée, jamais propagée au-delà d'un saut` (les deux
   paquets, provenance, exclusion de `demo-host`) ;
2. `benchmarks/** => FULL conservateur, jamais de décision vide` ;
3. `diff mixte docs + benchmarks => FULL + check docs (décision non vide)` ;
4. `CLI --files: benchmark fixture => FULL conservateur, JSON valide (exit 0)`.

### 10.5 Décisions `benchmarks/**` (aucune décision vide)

| Diff | Décision | Commande(s) |
|---|---|---|
| `benchmarks/s8-mvp0c/fixture/Cargo.toml` | **FULL conservateur** (manifeste) | `cargo test --workspace -- --test-threads=1` |
| `benchmarks/s8/run-s8.mjs` | **FULL conservateur** (chemin inconnu listé) | suite complète |
| `docs/... + benchmarks/s8-mvp0c/fixture/Cargo.toml` | **FULL conservateur + docs** | suite complète **+** `python tools/validate_repo_docs.py` |

Dans tous les cas : `uncertain=false`, JSON valide, au moins une commande (jamais `commands: []`).

### 10.6 Décision réelle `origin/dev..HEAD`

```powershell
node tools/impact/impact.mjs --base origin/dev --head HEAD --json
# exit 0 ; decision=full-conservative ; uncertain=false ; 595 fichiers ; 2 commandes
#   cargo test --workspace -- --test-threads=1
#   python tools/validate_repo_docs.py
```

Décision valide et conservative (le diff touche de nombreux chemins hors graphe, dont
`.agents/**`), sans incertitude. Le commit DSP `c86d1f0^..c86d1f0` donne aussi
`full-conservative` (manifeste `dsp-harness` + docs), exit 0.

### 10.7 Robustesse du workflow

`mvp-impact.yml` (YAML validé, 13 steps) :

- la garde porte `id: mapping_selftest` ;
- les 3 étapes conditionnelles (docs, FULL, sélectif) exigent
  `steps.mapping_selftest.outcome == 'success'` : si le selftest échoue, le job échoue
  **une seule fois, explicitement**, sans cascade trompeuse « aucune décision » et sans
  prétendre exécuter des tests.

Simulation locale des corps de steps extraits du YAML :

- garde : **exit 0**, `SELFTEST PASS — 56 cas, 583 assertions, 0 échec` ;
- calcul (`BASE_REF=origin/dev`) : **exit 0**, `Décision: full=true uncertain=false docs=true (exit outil=0)`.

La CI GitHub redeviendra verte après le commit de ce correctif par le Lead (le run rouge
portait l'ancien selftest).

### 10.8 Limites

1. Aucun commit (le Lead est seul écrivain Git) : la CI ne peut pas être re-déclenchée avec
   le correctif avant son commit ; le run vert est attendu, pas encore observé.
2. Aucun test cargo local exécuté (workspace partagé, durée bornée) : la simulation couvre
   la garde et la décision, pas l'exécution des suites.
3. Les cas `benchmarks/**` vérifient des chemins réels mais ne compilent/lancent pas les
   benchmarks.
4. Aucune règle conservatrice n'a été affaiblie ; `tools/impact/impact.mjs` et
   `scripts/impact.ps1` sont inchangés.

**Preuve JSON** : [`evidence/mvp0c-impact-dev-dep-2026-11-09.json`](../../evidence/mvp0c-impact-dev-dep-2026-11-09.json).

---

## 11. E2E navigateur déclenchés par la CI par impact (MVP-0c L4.9)

### 11.1 Trou fermé

`mvp-impact.yml` n'exécutait aucun navigateur : une modification d'application/UI pouvait
passer avec des tests Rust verts sans jamais exercer le JavaScript réellement affiché.
Le champ `browser_e2e_required` comble ce trou, sans affaiblir les règles conservatrices.

### 11.2 Règle `browser_e2e_required` (conservatrice)

| Déclencheur | `browser_e2e_required` | Exemple de raison |
|---|---|---|
| chemin sous `apps/**` (UI/app) | **true** | `application/UI modifiée: apps/ravum-todo/src/routes.rs` |
| `crates/ravum-todo-core/**` (contrat applicatif) | **true** | `contrat applicatif modifié: …` |
| `crates/ravum-store/**` (stockage applicatif) | **true** | `stockage applicatif modifié: …` |
| `scripts/demo-m4-browser.ps1` (script E2E) | **true** | `script E2E navigateur modifié` |
| mapping **incertain** (U/X/B, diff vide, base introuvable…) | **true** (fail-closed) | `mapping incertain (fail-closed) …` |
| **FULL + manifeste** (`Cargo.toml`/`Cargo.lock`) | **true** | `décision FULL + manifeste de construction modifié` |
| **FULL + surface UI** | **true** | `décision FULL + surface applicative/UI modifiée` |
| **chemin inconnu** (FULL) | **true** (extension conservatrice) | `chemin inconnu (…) : E2E navigateur exigés par prudence` |
| docs-only | **false** | `browser-e2e — non requis: …` |
| crate sans lien applicatif (`ravum-unit-graph`, `ravum-proof`) | **false** | idem |
| FULL dû à `scripts/**` non-UI | **false** (compromis de coût) | idem |

Le champ est émis dans le JSON (`browser_e2e_required`, `browser_e2e_reason`,
`browser_e2e_reasons`), affiché par `--explain` et par `scripts/impact.ps1`, et journalisé
dans les `reasons` (`browser-e2e — requis/non requis`). En cas de champ absent ou de
décision de secours, le workflow force `browser=true` (fail-closed).

### 11.3 Workflow (YAML validé, 16 steps)

3 étapes conditionnelles ajoutées à `mvp-impact.yml` :

1. `Setup Node.js for the browser E2E (impact-selected)` (`node-version: '24'`) ;
2. `Browser E2E - real Chromium/Edge via CDP (impact-selected)` :
   `powershell -ExecutionPolicy Bypass -File scripts/demo-m4-browser.ps1 -RequireBrowser -Screenshots` ;
3. `Upload browser E2E evidence (impact-selected)` (artefact `browser-e2e-evidence`,
   `target/ravum/browser-e2e`).

Conditions : `always() && steps.mapping_selftest.outcome == 'success' && steps.impact.outputs.browser == 'true'`.
**Aucun skip transformé en vert** : `-RequireBrowser` transforme l'absence de navigateur en
échec (exit 1), et tout code non nul fait échouer le job (`::error::E2E navigateur …`).
Preuve locale : navigateur invalide + `RAVUM_E2E_REQUIRE_BROWSER=1` ⇒ **exit 1** explicite.

### 11.4 Exécution locale réelle (coût)

Le navigateur disponible localement est un **Playwright headless shell** (pas Edge CI) :

```powershell
powershell -ExecutionPolicy Bypass -File scripts/demo-m4-browser.ps1 -RequireBrowser
# exit 0 ; 10/10 scénarios réussis, 0 échec ; 10,1 s de scénarios ; 24,1 s au total (build inclus)
```

Référence CI (preuves F, run `mvp.yml` **42546044558**, head `486d4c8`) :

| Étape | Durée réelle |
|---|---|
| Setup Node.js | 4 s |
| Browser E2E (10 scénarios) | 22 s |
| Upload artefact | 1 s |
| **Ajout par exécution impact** | **~27 s** |

Le job impact peut recompiler `ravum-todo` si le cache est froid (non mesuré séparément,
borné par le timeout global). **Compromis assumé** : `mvp.yml` et `mvp-impact.yml` se
déclenchent tous deux sur `push feat/**` ; quand le navigateur est requis, les 10 E2E
peuvent tourner deux fois. La CI par impact reste autosuffisante (aucune dépendance à un
autre job pour ne pas rater une régression UI), coût ~27 s par occurrence.

### 11.5 Décisions réelles (JSON)

| Diff | Décision | `browser_e2e_required` | Raison |
|---|---|---|---|
| `apps/ravum-todo/src/routes.rs` | sélective | **true** | application/UI modifiée |
| `docs/mvp/MVP-0c-qualification.md` | sélective | **false** | non requis |
| `crates/ravum-unit-graph/src/lib.rs` | sélective | **false** | non requis |
| `Cargo.toml` | FULL conservateur | **true** | FULL + manifeste |
| `scripts/demo-m4-browser.ps1` | FULL conservateur | **true** | script E2E modifié |
| `crates/ravum-todo-core/src/lib.rs` | sélective | **true** | contrat applicatif |
| `crates/ravum-store/src/lib.rs` | sélective | **true** | stockage applicatif |
| `zorglub/x.bin` | FULL conservateur | **true** | chemin inconnu (prudence) |
| statut `U` (incertain) | FULL + incertain | **true** | fail-closed |

Diff réel `origin/dev..HEAD` (`node tools/impact/impact.mjs --base origin/dev --head HEAD --json`) :
**exit 0**, `full-conservative`, `uncertain=false`, `browser_e2e_required=true`
(`application/UI modifiée: apps/ravum-demo-host/Cargo.toml`), `docs_required=true`,
598 fichiers, 2 commandes.

### 11.6 Selftest

`SELFTEST PASS — 59 cas, 655 assertions, 0 échec` (avant : 56 cas/583 assertions ; le nombre d'assertions est dynamique car l'inventaire boucle sur les paquets du workspace, y compris les WIP d'autres workers).
Nouveaux cas : règle navigateur (true/false/incertain/FULL+manifeste/unit-graph), CLI
`apps` ⇒ true, CLI `docs-only` ⇒ false, et assertions de garde du workflow (setup-node,
`-RequireBrowser`, sortie `browser`, artefact, échec explicite).

### 11.7 Limites

1. **Aucun `workflow_dispatch` avec le nouveau code** avant le commit du Lead (aucune
   commande git mutante) : le ref distant `486d4c8` ne contient pas ces étapes ; un
   dispatch maintenant reproduirait l'ancien comportement. Commande prévue après commit :
   `gh workflow run mvp-impact.yml --ref feat/mvp0c-hardening -f base=origin/dev`.
2. Navigateur local = Playwright headless shell, pas Edge de `windows-latest` ; les 10
   scénarios réussissent localement, la CI peut différer.
3. Coût CI mesuré à cache chaud dans le job `mvp` ; coût à froid du job impact non mesuré.
4. Duplication `mvp.yml`/`mvp-impact.yml` assumée et documentée.
5. Le sélecteur déclenche les E2E existants ; il ne prouve pas une absence générale de
   régression UI, et ne remplace pas la qualification produit.

**Preuve JSON** : [`evidence/mvp0c-impact-e2e-2026-11-09.json`](../../evidence/mvp0c-impact-e2e-2026-11-09.json).

