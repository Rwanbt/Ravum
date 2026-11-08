# MVP-0b — CI sélective par impact (L4-E)

> **Statut** : outil, workflow, preuves locales **et première exécution réelle sur GitHub Actions** (run [42489068304](https://github.com/Rwanbt/Ravum/actions/runs/42489068304), succès : décision FULL légitime sur un diff touchant `scripts/` et `crates/ravum-protocol`, suite complète verte, garde documentaire exécutée)
> dans ce contexte (voir §7). Le workflow a été validé syntaxiquement et sa logique de
> commandes a été exercée localement via `scripts/impact.ps1` et par simulation des steps.
>
> **Correction de conception intégrée** (demande Lead) : un **FULL par règle conservatrice**
> (manifeste, protocole/ABI, patch-host, CI, fixtures, scripts, outil d'impact, chemin
> inconnu) est une **issue légitime** — la suite complète est exécutée et le job est **vert
> si les tests passent**. Seul un mapping **incertain / erreur interne** échoue
> explicitement, après exécution de secours. La variable `RAVUM_IMPACT_ALLOW_FULL` a été
> **supprimée** : aucune variable n'est requise pour qu'une PR normale soit verte.
>
> **Non-qualification** : cette CI sélectionne des tests. Elle ne qualifie ni un compilateur,
> ni une preuve, ni une ABI, ni le temps réel, ni une release.

- **Mandat** : L4-E — CI réellement sélective mais conservatrice, sans jamais masquer un échec.
- **Dépôt** : `D:\App\Ravum`, branche `feat/mvp0b-hardening`.
- **HEAD au début de la tâche** : `7026f21` ; **HEAD pendant les mesures** : `25d9856`
  (le Lead a intégré d'autres workers en cours de route ; aucun commit n'a été créé par
  ce worker, tous les changements L4-E restent **non commités**).
- **Environnement mesuré** : Windows, Node `v26.13.0`, `cargo 1.95.0`, `rustc 1.95.0`
  (2026-04-14), machine de référence ~15,7 Go RAM, **machine partagée avec d'autres
  workers** (charge CPU 1 % → 82 % pendant les mesures, 3,0–3,4 Go libres).
- **Fichiers créés** (périmètre exclusif, rien d'autre n'est modifié) :
  - `tools/impact/impact.mjs` — graphe d'impact + décision JSON / `--explain` ;
  - `tools/impact/selftest.mjs` — corpus de preuve du mapping ;
  - `.github/workflows/mvp-impact.yml` — workflow `pull_request` ;
  - `scripts/impact.ps1` — lanceur local, projection des codes de sortie ;
  - `docs/mvp/MVP-0b-ci.md` — ce document.

---

## 1. Objectif

À partir des fichiers modifiés par une PR, calculer **exactement** les tests requis :

```
fichiers modifiés → composants → paquets Rust → fermeture des dépendants
                 → commandes de test sérialisées (--test-threads=1)
```

Deux exigences simultanées :

1. **sélectif** : un changement de documentation ne doit pas relancer `cargo test --workspace` ;
   un changement dans `ravum-todo-core` doit tester aussi `ravum-store` et `ravum-todo` ;
2. **conservateur / fail-closed** : tout ce qui n'est pas prouvé comme non impactant
   déclenche la **suite complète**, et une incertitude de mapping **échoue explicitement**.

---

## 2. Modèle du graphe

### 2.1 Sources du graphe

Lues directement, sans dépendance npm :

- `Cargo.toml` racine : `[workspace].members`, `[workspace.dependencies]` (dont les
  dépendances héritées `{ workspace = true }`) ;
- chaque `Cargo.toml` de membre : `[package].name`, `[dependencies]`,
  `[dev-dependencies]`, `[build-dependencies]`, y compris les tables ciblées
  `[target.'cfg(...)'.dependencies]` et les clés pointées (`serde.workspace = true`).

Le parseur est un **sous-ensemble TOML** maison (tables, tableaux multi-lignes, tables
inline, clés pointées, chaînes, commentaires). Tout manifeste non compris rend le mapping
**INCERTAIN** (donc FULL + échec explicite) : c'est volontaire.

### 2.2 Arêtes et sémantique

| Arête Cargo | Propagation inverse | Raison |
|---|---|---|
| `[dependencies]` | **transitive** | le code du dépendant change de comportement si la dépendance change |
| `[build-dependencies]` | **transitive** (conservateur) | le script de build du dépendant peut générer un code différent |
| `[dev-dependencies]` | **un seul saut** | les tests du dépendant recompilent la dépendance, mais le code du dépendant ne change pas : ses propres dépendants ne recompilent pas cette dépendance |

Un changement dans un paquet `P` exige donc :

- les tests de `P` ;
- les tests de tous les paquets qui dépendent de `P` (transitivement, arêtes normales/build) ;
- les tests des paquets qui ont une `dev-dependency` vers un paquet déjà affecté
  (un seul saut, sans propagation au-delà).

Les commandes sont émises **une par paquet**, triées par ordre topologique
(dépendances avant dépendants) et **sérialisées** : `--test-threads=1`, et le workflow
positionne `CARGO_BUILD_JOBS=2` (machine ~16 Go, tests M1 construisant de vraies
bibliothèques natives).

### 2.3 Fermeture en une phrase

`ravum-protocol` → presque tous les crates (mais règle FULL) ;
`ravum-snapshot` → `ravum-rust-adapter`, `ravum-coordinator`, `ravum-cli`,
`ravum-workspace-coordinator` (+ dev-dépendants `ravum-patch-host`, `ravum-proof`) ;
`ravum-todo-core` → `ravum-store` → `ravum-todo` (app + E2E) ; `ravum-daemon` → lui seul.

---

## 3. Table de mapping chemins → composants → tests

| Chemin modifié | Composant | Tests requis | FULL ? |
|---|---|---|---|
| `crates/<membre>/**` | `pkg:<membre>` | `cargo test -p <membre>` + fermeture des dépendants | non |
| `apps/<membre>/**` | `pkg:<membre>` | idem (+ suite E2E Rust du paquet) | non |
| `crates/<inconnu>/**`, `apps/<inconnu>/**` | inconnu | — | **oui** |
| `docs/**`, `*.md` à la racine | `docs` | `python tools/validate_repo_docs.py` | non |
| `evidence/**` | `evidence` | aucun test automatisé ne le consomme (seuls scripts de démo) | non |
| `Cargo.toml` (tout), `Cargo.lock` | manifeste | suite complète | **oui** |
| `.github/**` | CI | suite complète (la sélection est en jeu) | **oui** |
| `crates/ravum-protocol/**` | contrat | suite complète (ABI/contrat partagé) | **oui** |
| `crates/ravum-patch-host/**` | hôte natif | suite complète (effets process-global) | **oui** |
| `fixtures/**` | données partagées | suite complète | **oui** |
| `scripts/**` | orchestration | suite complète (les scripts eux-mêmes restent couverts par `mvp.yml`) | **oui** |
| `tools/impact/**` | auto-modification | suite complète (l'outil ne certifie pas sa propre PR) | **oui** |
| `tools/validate_repo_docs.py` | docs | le validateur docs (exécuté) | non |
| `tools/**` (autres) | outillage non mappé | suite complète | **oui** |
| tout autre chemin | inconnu | suite complète + `unknown_paths` | **oui** |

Règles complémentaires :

- **renommage / copie** : union des composants **source + destination** (`R`/`C`) ;
- **suppression** : composant du chemin supprimé (et donc de tous ses dépendants) ;
- **fichier de test** (`**/tests/**`, `tests.rs`) : au minimum sa suite, plus la fermeture ;
- **UI statique** (`apps/*/static/**`) : au minimum la suite Rust du paquet ; aucune suite
  navigateur n'est branchée sur ce workflow (limite, cf. §7).

---

## 4. Règles fail-closed et incertitude

Un fichier suffit pour basculer en **FULL** : manifeste, `.github/**`, `ravum-protocol`,
`ravum-patch-host`, `fixtures/**`, `scripts/**`, `tools/impact/**`, outillage non mappé,
chemin inconnu.

**Deux issues distinctes** (champ JSON `decision`, et non un échec par défaut) :

- `full-conservative` — FULL par **règle conservatrice de contenu** : c'est une **issue
  légitime** (le diff ne peut pas être certifié sélectivement). La suite complète est
  exécutée ; le job **réussit si les tests passent** et échoue selon leur résultat réel.
  Aucun code non nul artificiel, aucune variable de dépôt requise ;
- `full-uncertain` — mapping **INCERTAIN** ou erreur interne : la suite complète est
  exécutée par sécurité **puis** la CI échoue explicitement (code non nul + message).
  C'est la **seule** catégorie qui échoue pour raison de mapping.

Le mapping est déclaré **INCERTAIN** dans les cas non prouvables :

- diff vide (impossible de distinguer « aucun changement » d'une base Git erronée) ;
- statut Git non résolu (`U`, `X`, `B`) ;
- manifeste illisible ou workspace incohérent (membre sans `Cargo.toml`, `[package].name`
  absent, doublons, dépendance `workspace = true` introuvable, arête vers un paquet absent) ;
- base Git explicite introuvable (**aucune** substitution silencieuse : `origin/main`/`main`
  ne servent de repli que si `--base` n'est pas fourni).

### 4.1 Codes de sortie

| Code | `impact.mjs` | `scripts/impact.ps1` | Workflow `mvp-impact.yml` |
|---|---|---|---|
| 0 | décision rendue (sélective **ou** FULL conservateur légitime) | idem | tests requis exécutés avec succès (FULL légitime **vert si la suite passe**) |
| 1 | — | ≥ 1 commande requise en échec | échec réel de test(s) / job rouge |
| 2 | FULL + mapping INCERTAIN | incertitude projetée | suite complète de secours exécutée **puis échec explicite** (seul échec « pour raison de mapping ») |
| 3 | usage / erreur sans décision | outil/Node/JSON illisible | (panne convertie en FULL+INCERTAIN, puis échec) |
| 4 | FULL avec `--fail-on-full` | FULL avec `-FailOnFull` (mode strict local opt-in, rien n'est exécuté) | — (jamais utilisé par la CI) |
| 5 | — | check docs requis mais Python absent | — |

### 4.2 Politique FULL dans le workflow

- **FULL conservateur (`full-conservative`) = issue légitime.** Le workflow exécute la
  suite complète `cargo test --workspace -- --test-threads=1` (rien n'est sauté), rapporte
  la décision et ses raisons dans le résumé d'étape, puis **réussit si la suite passe** et
  **échoue seulement si des tests échouent**. Aucune variable de dépôt, aucun contournement
  n'est nécessaire pour qu'une PR normale (manifeste, protocole, patch-host, chemin
  inconnu, etc.) soit verte lorsque ses tests sont verts.
- **Mapping incertain (`full-uncertain`) = échec explicite.** La suite complète de secours
  est exécutée (fail-closed), puis le job échoue avec `::error::` et un code non nul. Cette
  catégorie n'est pas contournable.
- Le mode strict `--fail-on-full` (outil) / `-FailOnFull` (lanceur local) reste disponible
  **hors CI**, désactivé par défaut : il sert à refuser explicitement une décision FULL
  depuis un hook local ou un script de qualification manuelle. Il n'est pas utilisé par
  `.github/workflows/mvp-impact.yml`.

---

## 5. Preuve de conservation des tests (non-masquage)

### 5.1 Selftest — `node tools/impact/selftest.mjs`

**Résultat réel après correction : `SELFTEST PASS — 48 cas, 385 assertions, 0 échec.`**

Le corpus compare chaque décision à une **implémentation de référence indépendante** de la
fermeture (point fixe sur les arêtes inverses, écrite dans le selftest, sans réutiliser le
code de `impact.mjs`), et couvre : impact direct, dépendant transitif, dev-dépendant,
docs-only, protocole ⇒ FULL, hôte natif ⇒ FULL, manifeste ⇒ FULL, CI ⇒ FULL,
fixtures ⇒ FULL, scripts ⇒ FULL, auto-modification ⇒ FULL, validateur docs, chemin
inconnu ⇒ FULL, fichier racine inconnu ⇒ FULL, renommage ⇒ union, copie ⇒ union,
suppression, fichier de test, diff mixte docs+crate, artefact de preuve seul, diff vide,
statut `U`, parseurs `git diff -z` / `--files`, CLI de bout en bout (dont mode Git réel
`HEAD~1...HEAD`).

Verrouillages ajoutés par la correction de conception :

- **une classification trop étroite est refusée** : cas dédiés (`ravum-todo-core`,
  `ravum-rust-adapter`) qui exigent que la sortie soit strictement plus large que « le
  crate modifié seul », égale à la fermeture de référence, avec une commande de test pour
  chaque dépendant omis par l'approche étroite ;
- **FULL légitime ⇒ exit 0** : le protocole et un manifeste `Cargo.toml` donnent
  `decision = full-conservative`, `uncertain = false`, exit **0** (aucun faux échec) ;
- **incertain ⇒ exit 2** : statut `U` et diff vide donnent `decision = full-uncertain`,
  exit **2** (seul échec pour raison de mapping) ;
- **garde statique du workflow** : présence de `--test-threads=1`, `--workspace`,
  `full-conservative`, absence de variable d'échappement, absence de step d'escalade FULL,
  et le bloc du step FULL ne contient **aucun `exit 1`** (simulation incluse, cf. §5.7) ;
  absence de `continue-on-error`, de `--exclude`, `--skip`, `--ignore`.

### 5.2 Le cas où une classification naïve aurait sauté des tests

Diff réel `e0a7114` (une seule ligne modifiée : `crates/ravum-rust-adapter/src/parse.rs`) :

```
PAQUETS DIRECTEMENT MODIFIÉS: ravum-rust-adapter
FERMETURE DES DÉPENDANTS (tests requis)
  - ravum-patch-host  [dépendant de test (dev-dependency) de ravum-rust-adapter]
  - ravum-proof  [dépendant de test (dev-dependency) de ravum-rust-adapter]
  - ravum-rust-adapter  [impact direct]
  - ravum-coordinator  [dépendant transitif via ravum-rust-adapter (dépendance)]
  - ravum-cli  [dépendant transitif via ravum-rust-adapter (dépendance)]
  - ravum-workspace-coordinator  [dépendant transitif via ravum-rust-adapter (dépendance)]

PREUVE CONTRE UNE CLASSIFICATION NAÏVE
  classification naïve (crates directement modifiés seulement): ravum-rust-adapter
  tests/paquets qu'une classification naïve aurait SAUTÉS:
    ravum-patch-host, ravum-proof, ravum-coordinator, ravum-cli, ravum-workspace-coordinator
```

Une classification « seulement le crate modifié » aurait donc sauté **5 paquets de tests**
(les suites `rt_lifecycle`, `proof_cache`, m1/m2, `collab_flow`, `git_publication`…).
La fermeture les inclut tous.

### 5.3 Preuve par exécution : les tests des dépendants sont réellement lancés

Diff **exécuté localement** (synthétique, mais sur l'arbre réel) :
`crates/ravum-todo-core/src/lib.rs` ⇒ décision `ravum-todo-core, ravum-store, ravum-todo`,
3 commandes séparées, **27 tests exécutés et verts** (7 + 6 + 4 + 10 répartis sur
8 binaires de test), 2,2 s. Une classification naïve (todo-core seul) aurait sauté
**20 de ces 27 tests**, dont la suite E2E de l'app.

### 5.4 Preuve par exécution : docs-only ne saute pas le check docs

Diff réel `86f90b4` (2 fichiers : `docs/mvp/MVP-0b-audit.md`, `evidence/…txt`) ⇒
**0 test Rust**, mais la commande `python tools/validate_repo_docs.py` est bien exigée,
exécutée et verte (`Documentation-only check PASS: 42 proposed ADRs and required files
present.`). Le check docs n'est donc pas « économisé » au passage.

### 5.5 Preuve par exécution : FULL ne masque rien

Diff réel `f01d7b7` (14 fichiers, dont `crates/ravum-protocol/src/dsp.rs` et
`crates/ravum-patch-host/**`) ⇒ **FULL** (`full-conservative`, issue légitime). La suite
complète a été exécutée une fois :

- **28 binaires de test, 236 tests exécutés, 234 réussis, 2 échoués**, 0 ignoré ;
- les 2 échecs (`semantics::tests::identifiers_are_filtered`,
  `verifier::tests::a_passing_unrelated_check_does_not_cover_a_required_one`) proviennent
  de fichiers **non suivis par Git** (`crates/ravum-workspace-coordinator/src/semantics.rs`,
  `src/verifier.rs`) ajoutés par un **autre worker pendant les mesures (WIP)** — ils ne sont
  pas dans le HEAD `25d9856` et ne sont pas une régression du dépôt ;
- ces échecs **n'ont pas été masqués** : la commande FULL a retourné `exit 101`, le
  lanceur local a projeté le code 1 et affiché `1 échec(s)`. C'est le comportement correct :
  un **échec réel de test** rend le job rouge.

Point de correction important : auparavant, un FULL **légitime** avec des tests verts
rendait aussi le job rouge (faux échec de blocage par construction). Ce n'est plus le cas :
le step FULL ne propage que le code de sortie réel des tests (cf. §5.7), et la variable
`RAVUM_IMPACT_ALLOW_FULL` a été supprimée.

### 5.7 Preuve de la politique FULL légitime (YAML + simulation du step réel)

- YAML validé par PyYAML : **12 steps**, aucune erreur de syntaxe ;
- `RAVUM_IMPACT_ALLOW_FULL` : **absent** du workflow ; step `Escalate FULL` : **absent** ;
- step `Run the FULL workspace suite (legitimate conservative mapping)` extrait du YAML et
  exécuté en simulation :
  - avec une **vraie commande cargo verte** (`cargo test -p ravum-todo-core`, sous-ensemble
    réel pour ne pas relancer la full) ⇒ **exit 0** — un FULL légitime n'échoue pas quand
    les tests passent ;
  - avec une commande en échec (stub `exit 7`) ⇒ **exit 7** — le code de sortie réel des
    tests est propagé, sans transformation ;
  - le bloc du step FULL ne contient **aucun `exit 1`** (vérifié aussi statiquement par le
    selftest) ;
- step `Fail explicitly on an uncertain mapping` exécuté en simulation avec une décision
  `full-uncertain` ⇒ **exit 1** avec message `::error::` — seule catégorie bloquante.

### 5.6 Garantie statique : aucun test neutralisé

- toutes les commandes `cargo` contiennent `--test-threads=1` ;
- aucun `--exclude`, `--skip`, `--ignore`, `--no-run`, `--no-fail-fast` n'est émis
  (vérifié par assertion) ;
- le workflow ne contient **pas** `continue-on-error` et exécute toutes les suites
  requises même si l'une échoue (les échecs sont agrégés, pas court-circuités) ;
- le workflow ne contient **aucun échec artificiel conditionné au FULL** ni variable
  d'échappement (assertion statique du selftest) : un FULL légitime est vert si les tests
  passent ;
- aucun test n'est supprimé ni modifié par cette livraison (périmètre d'écriture limité à
  `tools/impact/**`, `.github/workflows/mvp-impact.yml`, `scripts/impact.ps1`,
  `docs/mvp/MVP-0b-ci.md`).

---

## 6. Mesures locales

**Protocole** : machine partagée (charge CPU relevée juste avant chaque exécution),
cache `target/` chaud (artefacts de builds antérieurs, dont ceux des autres workers),
toutes les commandes en `--test-threads=1` et `CARGO_BUILD_JOBS=2`.
`scripts/impact.ps1 -Run -Offline` mesure chaque commande séparément.
Les durées « total » incluent le calcul d'impact (Node), le lanceur PowerShell et
l'exécution. Les temps `cargo` sont les temps de test affichés par le lanceur.

> **Correction post-mesure** : la politique FULL du workflow a été corrigée après ces
> mesures (FULL légitime non bloquant, incertitude seule bloquante). Cette correction ne
> change **aucune commande de test** ni aucun chemin de code exécuté ; les durées restent
> donc valides. La suite complète n'a **pas été relancée** après la correction (contrainte
> machine partagée, une seule exécution de `cargo test --workspace`).

| # | Diff | Nature | Décision | Paquets / commandes | Tests exécutés | Temps cargo | Temps total | Charge avant | RAM libre |
|---|---|---|---|---|---|---|---|---|---|
| A | `86f90b4^...86f90b4` | réel : docs + evidence | SÉLECTIVE | 0 Rust ; 1 check docs | 0 test Rust ; docs PASS | 0,5 s | **2,5 s** | 62 % | 3,0 Go |
| B | `aa7663c^...aa7663c` | réel : 1 fichier `ravum-cli` | SÉLECTIVE | `ravum-cli` (1 cmd) | **19** (2+3+10+4) | 19,5 s | **21,3 s** | 71 % | 3,2 Go |
| C | `crates/ravum-todo-core/src/lib.rs` (synthétique) | dépendants | SÉLECTIVE | 3 paquets (3 cmds) | **27** (7+6+4+10) | 2,2 s | **3,7 s** | 57 % | 3,2 Go |
| D | `f01d7b7^...f01d7b7` | réel : protocole + hôte natif | **FULL légitime** | workspace (1 cmd) | **236** (234 ok, 2 échecs concurrents) | 68,8 s | **70,6 s** | 60 % | 3,4 Go |
| E | `e0a7114^...e0a7114` | réel : 1 fichier `ravum-rust-adapter` | SÉLECTIVE (calcul seul) | 6 paquets | non exécuté (charge) | — | 646 ms (calcul) | 82 % | 3,1 Go |

Autres mesures :

| Mesure | Valeur |
|---|---|
| Calcul d'impact Node (4 diffs réels A–D) | 529 ms – 927 ms par diff (démarrage Node inclus) |
| Diff de branche réel `origin/main...HEAD` (237 fichiers) | **FULL légitime**, 528 ms, 0 incertitude |
| `scripts/impact.ps1` sur FULL légitime (calcul seul, `f01d7b7`) | exit **0** (aucun faux échec) |
| `scripts/impact.ps1 -Files ""` (diff vide volontaire) | exit **2** (incertain, échec explicite exigé) |
| `scripts/impact.ps1 -Files "U:zorglub/x.rs"` | exit **2** (incertain) |
| `scripts/impact.ps1 -Files "docs/architecture/STATUS.md"` | exit **0** |
| `scripts/impact.ps1 -Files "crates/ravum-protocol/src/lib.rs" -FailOnFull` | exit **4** (mode strict local opt-in uniquement) |
| Simulation du step FULL extrait du YAML, commande cargo verte réelle | exit **0** |
| Simulation du step FULL avec commande en échec (`exit 7`) | exit **7** (code réel propagé) |
| Simulation du step incertain (`full-uncertain`) | exit **1** + `::error::` |

**Lecture** : sur les diffs où la sélection s'applique, la décision d'impact coûte
~0,5–0,9 s et l'exécution sélective est nettement plus courte que la suite complète
(docs-only : 0 test Rust ; ~21 s pour un crate contre ~69 s pour la suite complète,
soit ~3,3× ; la suite complète exécute 236 tests contre 19). Le gain réel dépend du diff :
un changement d'un crate très dépendant (comme `ravum-rust-adapter`, 6 paquets) se rapproche
de la suite complète — c'est le prix de la conservation des tests.

**Limites de mesure honnêtes** :

- cache `target/` **chaud** : les temps n'incluent pas une compilation à froid ; en CI,
  le cache GitHub Actions et la compilation initiale peuvent dominer (`mvp.yml` existant
  compile déjà l'espace de travail avant de tester) ;
- **machine partagée et chargée** (CPU 1–82 %, autres agents actifs, 3,0–3,4 Go libres
  sur 15,7 Go) : les durées sont bruitées et supérieures à une machine isolée ;
- le diff D a été exécuté sur l'arbre de travail courant (avec le travail non commité
  d'autres workers), pas sur un checkout exact de `f01d7b7` ; les fichiers du diff sont
  historiques, l'exécution porte sur le code présent — les 2 échecs observés le montrent ;
- `cargo test --workspace` n'a été lancé **qu'une seule fois** (contrainte machine
  partagée), et seulement sur le diff D (FULL) ;
- les diffs B et D ont été mesurés à des charges différentes (71 % puis 60 %) : la
  comparaison est indicative, pas un benchmark.

---

## 7. Limites assumées

1. **Pas d'analyse sémantique** : `include!`, macros procédurales, `cfg`, scripts de build,
   imports dynamiques ne sont pas suivis ; seule la déclaration Cargo est modélisée.
2. **Parseur TOML restreint** : tout manifeste non compris ⇒ FULL + incertain (fail-closed).
3. **`dev-dependencies` à un seul saut** : le test du dépendant est exigé, pas celui de ses
   propres dépendants (le code du dépendant ne change pas).
4. **`fixtures/**` ⇒ FULL** : données consommées par plusieurs crates de test et la démo ;
   aucun mapping statique sûr.
5. **`scripts/**` ⇒ FULL**, mais les scripts de démo/orchestration ne sont **pas exécutés**
   par `mvp-impact.yml` (ils restent couverts par `mvp.yml`) ; le FULL ne prouve donc pas
   un script de démo.
6. **UI statique et E2E navigateur** : `apps/*/static/**` exige la suite Rust du paquet ;
   une éventuelle suite E2E navigateur (ex. `apps/ravum-todo/e2e/**`, ajoutée par un autre
   chantier) n'est pas exécutée ici. Un futur branchement devra être ajouté au mapping.
7. **Fichiers de données, printf/CSV, rapports `evidence/`** : `evidence/**` ne déclenche
   aucun test ; les autres données non reconnues passent par la règle « chemin inconnu ⇒ FULL ».
8. **Nouveaux répertoires racine** (ex. `benchmarks/`, `.agents/`) : inconnus ⇒ FULL tant
   qu'une règle explicite n'est pas ajoutée.
9. **Suppression/désactivation d'un test** : le graphe ne peut pas la détecter (il ne lit
   pas le contenu des tests). Le workflow ne neutralise rien, mais ne prouve pas non plus
   l'absence de tests retirés : c'est un gate de sélection, pas un gate anti-suppression.
10. **Première exécution GitHub Actions réalisée le 2026-11-08** (run
    42489068304, déclenchée par un push sur `feat/**` conformément au déclencheur ajouté) :
    diff vs `origin/dev` => FULL par règles conservatrices (`scripts/**`, protocole), suite
    complète exécutée et verte, contrôle documentaire exécuté, étape sélective sautée comme
    attendu. Limite restante : ce run n'exerce pas le chemin *sélectif* (le diff était
    FULL) ; la validation locale du 2026-11-08 couvre ce chemin avec 3 diffs sélectifs et
    seuls les runners GitHub restent à éprouver. Détail : validation locale =
    syntaxe YAML (`PyYAML OK`, 12 steps), simulation des steps FULL/incertain extraits du
    YAML, logique de commandes exercée via `scripts/impact.ps1`, garde statique dans le
    selftest. La première exécution réelle sur `pull_request` reste à faire par le Lead.
11. **Politique FULL** : un FULL par règle conservatrice de contenu est une **issue
    légitime** — la suite complète est exécutée et le job est **vert si les tests passent**
    (aucun faux échec, aucune variable de dépôt requise). Seul un mapping **incertain**
    (diff vide, statut Git non résolu, manifeste illisible, base introuvable) échoue
    explicitement, après exécution de secours. Le mode `--fail-on-full` / `-FailOnFull`
    reste un outil strict **local**, désactivé par défaut et non utilisé par la CI.
12. **Runners différents** : GitHub Actions `windows-latest` peut se comporter autrement
    (versions Rust/Python/Node, cache, charge). L'outil exige Node ≥ 20 (`parseArgs`,
    ESM) ; le runner doit fournir cette version ou plus récente. Les temps locaux ne
    sont pas des temps CI.

---

## 8. Reproductibilité

```powershell
# Preuve du mapping (indépendante du réseau et du cache cargo)
node tools/impact/selftest.mjs

# Calcul seul, rapport lisible, sur un diff réel de commit
node tools/impact/impact.mjs --base "86f90b4^" --head 86f90b4 --explain

# Mode Git par défaut (origin/main puis main)
node tools/impact/impact.mjs --explain

# Diff synthétique, JSON
node tools/impact/impact.mjs --files "crates/ravum-todo-core/src/lib.rs" --json

# FULL légitime => exit 0 (issue conservatrice, aucun faux échec)
node tools/impact/impact.mjs --files "Cargo.toml" --json
node tools/impact/impact.mjs --files "crates/ravum-protocol/src/lib.rs" --json

# INCERTAIN => exit 2 (seul échec pour raison de mapping)
node tools/impact/impact.mjs --files "" --json
node tools/impact/impact.mjs --files "U:zorglub/x.rs" --json

# Lanceur local : calcul, puis exécution réelle sérialisée
powershell -ExecutionPolicy Bypass -File scripts/impact.ps1 -Files "crates/ravum-todo-core/src/lib.rs" -Explain
powershell -ExecutionPolicy Bypass -File scripts/impact.ps1 -Files "crates/ravum-todo-core/src/lib.rs" -Run -Offline

# FULL légitime vu par le lanceur local (calcul seul) => exit 0
powershell -ExecutionPolicy Bypass -File scripts/impact.ps1 -Base "f01d7b7^" -Head "f01d7b7"

# Incertitude projetée par le lanceur local => exit 2
powershell -ExecutionPolicy Bypass -File scripts/impact.ps1 -Files ""

# Mode strict LOCAL opt-in (jamais utilisé par la CI) : refus explicite d'un FULL
powershell -ExecutionPolicy Bypass -File scripts/impact.ps1 -Files "crates/ravum-protocol/src/lib.rs" -FailOnFull
```

Dans le workflow, le diff est fourni par `github.event.pull_request.base.sha` (merge-base
via `git diff base...HEAD`, `fetch-depth: 0`), avec repli documenté `origin/main` puis
`main` pour un lancement manuel (`workflow_dispatch`).

---

## 9. Conclusion

- Le graphe est **construit et vérifié** : 14 paquets, arêtes normales/dev/build typées,
  fermeture transitive des dépendants, preuve indépendante dans le selftest (48 cas,
  385 assertions, dont un cas dédié qui refuse une classification trop étroite).
- La sélection **n'a jamais masqué un échec** : les 2 échecs dus à un travail concurrent
  non commité (WIP d'un autre worker) ont fait échouer la commande FULL et le lanceur local
  (exit 1) ; un échec réel de test rend le job rouge.
- Un **FULL légitime** (manifeste, protocole, patch-host, chemin inconnu…) exécute la suite
  complète et est **vert si les tests passent** : plus de faux échec de blocage, aucune
  variable de dépôt requise. Seul l'**incertain** échoue explicitement (exit 2 côté outil,
  exit 1 côté job), après exécution de secours.
- Les tests des dépendants **sont réellement exécutés** quand un crate change
  (démonstration exécutée : 27 tests sur 3 paquets pour un changement de `ravum-todo-core`).
- Les limites sont explicites (§7) ; la première exécution GitHub Actions a été réalisée (run 42489068304) et ses limites sont reportées.
- **Aucun résultat ci-dessus ne qualifie le produit Ravum** : c'est une preuve de
  mécanisme de sélection de tests, sur une machine partagée et chargée.
