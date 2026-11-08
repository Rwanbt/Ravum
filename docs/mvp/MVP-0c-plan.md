# MVP-0c — Lot L0 : audit, consolidation et plan des six lots

Statut : **AUDIT EXÉCUTÉ** (faits vérifiés le 2026-11-09, machine de référence
chargée). Ce document n'est ni une release, ni une acceptation d'ADR, ni une
autorisation de fusion. Les 42 ADR originaux restent `PROPOSED` ; la RFC
`RFC-SHARED-WORKSPACE-MULTIAGENT` reste `PROPOSED`.

Branche de travail MVP-0c : `feat/mvp0c-hardening`, créée depuis
`feat/mvp0b-hardening` @ `ea7200c47991bc17044cf3cb50cbdd78c20005f9`.
Aucune branche existante n'a été réécrite ; les PR #78 et #80 restent ouvertes.

## 1. État initial vérifié (2026-11-09T00:35–00:55+01:00)

### 1.1 Dépôt de code `Rwanbt/Ravum`

| Élément | Valeur observée | Méthode |
|---|---|---|
| Branche courante (audit) | `feat/mvp0b-hardening` @ `ea7200c` = `origin/feat/mvp0b-hardening` | `git status`, `git log` |
| `origin/dev` | `78411024e2665dafce31b029786ef56d400bdab2` | `gh api branches/dev` |
| Contenu de `dev` | `.github`, `.gitignore`, `AGENTS.md`, `CONTRIBUTING.md`, `README.md`, `SECURITY.md`, `docs`, `tools` — **aucun crate, aucun workflow** | `gh api contents?ref=dev` |
| `origin/main` | `16b1ff2` (`main` local = `origin/main` + 1 commit local `chore: install ai-native dev-stack (verified profile)`, non poussé by design) | `git branch -vv` |
| Branches distantes | `main`, `dev`, `feat/3-mvp0a-continuous-loop`, `feat/mvp0b-hardening` | `git branch -a` |
| Modifications non commitées | aucune (`stash` vide) | `git status`, `git stash list` |
| PR #78 | ouverte, base `dev`, tête `feat/3-mvp0a-continuous-loop`, **MERGEABLE**, 186 fichiers, +25 528 / −6, 22 commits | `gh pr list --json` |
| PR #80 | ouverte, base `feat/3-mvp0a-continuous-loop` (empilée), tête `feat/mvp0b-hardening`, **MERGEABLE**, 304 fichiers, +128 220 / −304, 54 commits | `gh pr list --json` |
| Protection `main` | PR requise, check `mvp` requis, force push et suppression interdits, `enforce_admins=false`, 0 approbation requise | API protection |
| Protection `dev` | PR requise, force push et suppression interdits, **aucun check requis** | API protection |
| Issues | 78 ouvertes, 0 fermée | `gh issue list` |
| ADR / RFC | 42 ADR (`docs/adr/ADR-*.md`), 2 RFC (`docs/rfc/`), tous `PROPOSED` | comptage + en-têtes |
| CI récente | produit `42529748229` et impact `42529748447` : **succès** sur `ea7200c` | `gh run list` |

### 1.2 Reproduction de la suite complète

Commande : `cargo test --workspace --offline -- --test-threads=1`, avec
`CARGO_BUILD_JOBS=2`, exécutée sur `ea7200c` le 2026-11-09 à 00:36.

| Résultat | Valeur |
|---|---|
| Code de sortie | **0** |
| Suites de tests | **54** |
| Tests réussis | **347** |
| Tests échoués | **0** |
| Tests ignorés | **0** |
| Sortie brute locale | `evidence/local/cargo-test-20261109-003644.txt` (répertoire ignoré par Git ; la présence de la sortie n'est pas une preuve versionnée) |

Conclusion L0 : la campagne 347/0 annoncée par le MVP-0b est **reproduite à
l'identique** sur l'état `ea7200c` ; aucun écart avec le dernier état connu
n'empêche de démarrer MVP-0c.

### 1.3 Outils et machine

| Élément | Valeur |
|---|---|
| `rustc` | 1.95.0 (`59807616e...`, 2026-04-14), host `x86_64-pc-windows-msvc`, LLVM 22.1.2 |
| `cargo` | 1.95.0 (`f2d3ce0bd`, 2026-03-21) |
| Python / Node | 3.13.7 / 26.13.0 |
| OS | Windows 11 Pro 26100, machine de référence 16 Go partagée avec d'autres agents |
| Sonde RAM/CPU/disque | **non concluante pendant l'audit** : les requêtes WMI/CIM ont dépassé 120 s sous charge. Aucune valeur n'est inventée ; les consommations seront mesurées par les lots L2/L4 avec le protocole du MVP-0b (RSS avant/après, compteur de jobs, `sysinfo`). |

### 1.4 Vault Obsidian `Rwanbt/obsidian-dev-brain`

| Élément | Valeur |
|---|---|
| Branche | `master`, propre, local = `origin/master` |
| HEAD observé | `e2b7929` (auto-sync 2026-11-09 00:35) |
| Contrat | `_system/VAULT-WRITING-CONTRACT.md` (schéma 4), `projects/ravum/AGENTS.md` |
| Sessions existantes | 8 : `2026-11-08-mvp0a-m0-m4-cycles.md` + 7 notes MVP-0b, dont la session complète `2026-11-08-ravum-mvp0b-session-complete.md` |
| Vérification | `python _system/tooling/vault.py check --format json` à rejouer avant chaque écriture |

La session complète MVP-0b est la référence de reprise : elle contient les
mesures, les décisions réservées et les non-claims du cycle précédent.

## 2. Audit des PR (L0)

### 2.1 Périmètre et dépendances

- **#78 → `dev`** : 186 fichiers, +25 528/−6, 22 commits. C'est la PR
  d'intégration de MVP-0a ; sa base `dev` ne contient encore ni `crates/` ni
  `.github/workflows/mvp*.yml`. La fusion de #78 apporte donc l'ensemble de la
  plateforme Rust et les workflows ; elle doit être vérifiée par un humain.
- **#80 → branche de #78** : 304 fichiers, +128 220/−304, 54 commits. Empilée :
  si #78 change de tête, #80 doit être rebasée (aucune réécriture automatique
  n'est autorisée). Le diff réel de #80 par rapport à `dev` n'est pas la somme
  des deux diffs : il faut le recalculer après intégration de #78.

### 2.2 Nature des fichiers (à confirmer par le worker d'audit)

Les 128 k lignes ajoutées sont dominées par le code Rust, les fixtures, les
preuves JSON, les documents et le harnais navigateur. Une revue structurée doit
classer chaque ajout en : code produit, test, fixture, preuve/evidence,
documentation, outil IA inclus, fichier généré. **Aucune preuve ne sera
supprimée** ; les gros fichiers de preuve restent traçables (Git LFS ou
artefacts de CI sont des options à décider humainement, pas un déplacement
unilatéral).

### 2.3 Stratégie d'intégration proposée (séquencée, aucune fusion par l'agent)

1. Vérification finale de #78 (checks verts, diff relu, aucun fichier utilisateur perdu).
2. Correction des problèmes bloquants éventuels sur une branche dédiée, sans forcer.
3. **Approbation humaine** de la fusion #78 → `dev`.
4. Après fusion autorisée, retarget de #80 → `dev`.
5. Recalcul et vérification du diff réel (`git diff origin/dev...origin/feat/mvp0b-hardening`).
6. Checks et tests par impact rejoués sur la nouvelle base.
7. **Approbation humaine** de #80.
8. Fusion éventuelle (humaine).
9. La branche MVP-0c repart de la base intégrée si elle existe ; sinon elle
   reste empilée sur `feat/mvp0b-hardening` (état actuel).

Tant que #78 et #80 sont ouvertes, la branche `feat/mvp0c-hardening` est créée
depuis `ea7200c` : elle n'écrase ni ne réécrit les travaux existants. Si `dev`
reçoit #78, un rebase explicite de MVP-0c sera nécessaire et sera annoncé.

## 3. Plan des six lots MVP-0c et gates

| Lot | Mission | Gate de sortie (falsifiable) | Propriétaire |
|---|---|---|---|
| **L0** | Audit, consolidation, stratégie d'intégration | ce document + suite 347/0 reproduite + état Git/PR/CI vérifié | Lead + Worker A |
| **L1** | Incrémentalité inter-crates fiable | un changement d'API de `core` force la revalidation réelle de `api`, `consumer` et `application` ; 16 tests négatifs conservés ; aucun artefact périmé activé ; fallback conservateur | Worker B |
| **L2** | Accélération des modifications réelles | gain mesuré sur fixtures réelles (p50/p95/p99, CPU/RAM/disque, nombre de compilations), correction inchangée, comparaison MVP-0a/0b/0c et Cargo natif | Worker C |
| **L3** | Collaboration multi-agents scalable | statuts sémantiques versionnés (`TEXTUALLY_APPLICABLE` … `REJECTED`/`MANUAL_REQUIRED`), publication transactionnelle testée sous crash/hook/écrivain externe, S8 comparatif honnête avec ou sans vrais agents | Worker D + Worker F |
| **L4** | Tests, CI, DSP, qualification | suite complète + impact + 10 E2E navigateur + stress DSP + preuves honnêtes ; CI par impact exercée sur une vraie PR vers `dev` **si** `dev` contient les crates | Worker E + Worker G |
| **L5** | Intégration, documentation, Obsidian | rapports versionnés, sessions Obsidian publiées et vérifiées, handoff reproductible | Worker H + Lead |

Lots parallélisables : L1 ∥ L2 (mesures seulement) ∥ L3 (coordinator) ∥ L4
(qualification) dès que les interfaces partagées sont stables. L2
(implémentation daemon) attend la stabilisation de l'interface externes de L1.

## 4. L1 — conception de l'incrémentalité inter-crates

### 4.1 Ce qui existe et ne suffit pas

COLLAB-4 (`crates/ravum-cli/src/collab_cycle.rs`) construit un graphe de crates
par un parseur `Cargo.toml` minimal limité aux dépendances `path`, analyse
chaque unité isolément avec `--emit=metadata` **sans `--extern`**, et n'utilise
que les hashs de fichiers sources. Conséquences vérifiées dans la doc : un
changement de signature de `core` peut casser `api` sans être détecté par la
seule analyse d'unité ; les dépendances indirectes ne sont pas propagées ; une
analyse `--emit=metadata` isolée ne démontre pas la validité du programme
complet. C'est la limitation bloquante L1.

### 4.2 Architecture retenue (conservatrice, sans second parseur Cargo)

Nouveau crate `ravum-unit-graph` (bibliothèque, aucune API privée `rustc`) :

1. **Modèle** obtenu via `cargo metadata --no-deps --format-version 1`
   (interface Cargo publique, pas de parseur TOML maison) : membres du
   workspace, cibles `lib`, éditions, features déclarées, `build.rs`,
   dépendances `path` internes, dépendances externes, `rename`.
   *Si `cargo` est absent ou que la commande échoue → `UNIT_GRAPH_UNAVAILABLE`,
   fallback Cargo complet, jamais une indépendance inventée.*
2. **Identités séparées** (INV-04) :
   - `content_id` : SHA-256 canonique du `Cargo.toml`, du `build.rs` et de tous
     les `.rs` du paquet (par chemin relatif, triés) ;
   - `context_id` : rustc `-vV` + target + édition + `--crate-name` + features
     activées + flags de compilation du harnais ;
   - `interface_id` : SHA-256 de l'artefact `.rmeta` produit par la compilation
     de l'unité — identité que rustc lit réellement pour vérifier les dépendants ;
   - `externs` : liste ordonnée `(nom, interface_id)` des dépendances directes ;
   - `verdict_id` : hash des diagnostics + statut (jamais confondu avec une preuve).
   Le cache de verdicts est indexé par `(content_id, context_id, externs,
   toolchain)` : **jamais** par le seul hash des sources de l'unité.
3. **Invalidation** : une unité est revalidée si son `content_id` change, si son
   `context_id` change, ou si l'`interface_id` d'une dépendance directe change
   (ou devient inconnue/périmée). Si rien ne change, le verdict est un HIT
   explicite. Un dépendant n'est jamais déclaré à jour sur la seule base de ses
   propres sources.
4. **Compilation réelle en chaîne** : ordre topologique ; pour chaque unité,
   `rustc --crate-type lib --emit=metadata --edition=… --crate-name=…
   --extern dep=<chemin du .rmeta fraîchement produit> [-L dependency=…]`.
   Le travail se fait dans un répertoire d'état dédié (`target/ravum/mvp0c/`) ;
   les artefacts sont adressés par contenu et les fichiers utilisateur ne sont
   jamais modifiés.
5. **Fallback conservateur** (obligatoire si non prouvable) :
   - dépendance externe non-`path` (registre, git), macro procédurale, `build.rs`
     exécuté, `include!`, features optionnelles/`cfg` non énumérables, target
     différent, ou graphe incohérent ⇒ **`cargo check -p <paquet>`** (Cargo est
     l'oracle) puis, si le moindre doute subsiste, `cargo check --workspace` ;
   - tout fallback est rapporté comme `FALLBACK_FULL`/`FALLBACK_PACKAGE`, jamais
     comme un succès incrémental ;
   - un artefact `.rmeta` produit sur un autre toolchain/contexte n'est jamais réutilisé.
6. **Arbitrage de statut** : `TARGETED_OK` (chaîne complète `--extern` vérifiée),
   `FALLBACK_PACKAGE`/`FALLBACK_FULL` (Cargo a vérifié), `STALE` (résultat
   abandonné), `ERROR` (diagnostics réels). Aucun `PROVED`.

### 4.3 Fixture multi-crates obligatoire

`fixtures/multicrate/` : `core` → `api` → `consumer` → `application`
(application = binaire, analysé via fallback Cargo), plus un paquet `features`
et un paquet `buildscript` pour les cas conservateurs.

Matrice de tests négatifs (16 cas du mandat) : corps privé inchangé d'interface,
signature publique, suppression de symbole, type partagé, implémentation de
trait, feature, build script, dépendance externe, contrat, configuration cible,
dépendance transitive, erreur uniquement dans un dépendant (doit être détectée
même si les sources du dépendant n'ont pas changé), modification pendant la
vérification (rejet STALE), réutilisation d'artefact périmé (refus), changement
de version de compilateur (invalidation du contexte), conflit entre deux
propositions d'agents (refus `MANUAL_REQUIRED` existant, conservé).

**Cas central** : modifier la signature publique de `core::scale` puis vérifier
que `api`, `consumer` et `application` sont revalidés réellement (traces rustc
non vides, pas de HIT), que le diagnostic de rupture apparaît, et qu'aucun
verdict antérieur n'est réutilisé.

### 4.4 Fichiers et propriété (aucune écriture simultanée)

| Zone | Propriétaire | Remarque |
|---|---|---|
| `crates/ravum-unit-graph/**` (nouveau) | Worker B | code + tests unitaires/intégration |
| `fixtures/multicrate/**` (nouveau) | Worker B | fixture réelle, minuscule |
| `crates/ravum-daemon/src/{snapshot.rs,config.rs,rustc_driver.rs,report.rs}` | Worker B | extension `externs` adressés par contenu, schéma `ravum-daemon` 1 → 2 |
| `crates/ravum-cli/src/collab_cycle.rs` + `crates/ravum-cli/src/main.rs` | Lead | intégration finale, après stabilisation B |
| `crates/ravum-daemon/src/{daemon.rs,gate.rs}` | Worker C (L2) | groupement/ordonnancement ; ne touche pas aux fichiers de B |
| `crates/ravum-workspace-coordinator/**` | Worker D (L3) | statuts, publication |
| `.github/workflows/**`, `tools/impact/**`, `scripts/**` | Worker E (L4) | CI/impact |
| `docs/mvp/MVP-0c-*.md`, `evidence/mvp0c-*` | Lead + chacun pour son lot | pas de collision : un fichier = un propriétaire |

Le daemon est le point de collision potentiel L1/L2 : l'interface `externs` est
stabilisée **avant** que Worker C ne modifie `daemon.rs`/`gate.rs`. En attendant,
Worker C travaille sur les mesures (harnais, fixtures, protocole) sans toucher au
daemon.

## 5. L2 — accélération des modifications réelles (conception)

Cibles mesurées, pas déclarées :

1. **Réduire le nombre de builds** : regroupement de N propositions compatibles
   en un snapshot cohérent → analyse ciblée → build groupé → patch atomique.
   Fenêtre de regroupement bornée (délai max configurable, petite valeur par
   défaut) et jamais de retard du feedback interactif : les diagnostics du
   snapshot courant partent dès qu'ils sont valides, indépendamment du build.
2. **Séparer diagnostics et activation** (schéma du mandat) : un résultat de
   diagnostic porte l'identité exacte du snapshot ; l'activation reste un acte
   séparé et fail-closed.
3. **Mesures** : fixtures petite unité, milliers de lignes, workspace
   multi-crates, full-stack, DSP, plusieurs agents (fichiers distincts et
   symboles distincts d'un même fichier) ; p50/p95/p99 par phase (analyse,
   codegen, lien, chargement, activation, cycle complet), CPU/RAM/disque,
   nombre de compilations ; comparaison MVP-0a, MVP-0b, MVP-0c, Cargo natif.
4. **Bonus déjà écarté à ne pas refaire sans mesure** : `-C incremental` sur
   cdylib, `lld-link`, cache `sccache` non mesuré, Cranelift (aucune mesure).
5. **Cible honnête** : réduction du travail inutile et du nombre de compilations ;
   pas de promesse de p95 < 50 ms pour le diagnostic local tant qu'il n'est pas
   mesuré sur du code réellement modifié.

## 6. L3 — collaboration multi-agents (conception)

- Conserver COLLAB-1→4 (snapshots, propositions, journal append-only, CAS Git
  unique, rejet des périmés, analyse ciblée).
- Versionner le protocole de statut (schéma) et distinguer
  `TEXTUALLY_APPLICABLE` / `SEMANTICALLY_CHECKED` / `TESTED` / `PROVED` borné /
  `REJECTED` / `MANUAL_REQUIRED`. `AUTO_SAFE` ne signifie jamais « programme
  correct » : il signifie au plus « texte applicable + vérification déclarée ».
- Relier la décision de fusion à l'analyse inter-crates L1 : un patch
  textuellement applicable qui change une interface utilisée ailleurs est au
  minimum revalidé (sinon refusé).
- Publication transactionnelle : index isolé, blobs, arbre, hooks, commit non
  publié, préconditions, `update-ref` CAS, journal, reprise ; tests crash,
  redémarrage, écrivain externe, permissions, fichiers générés.
- Granularité : patch textuel, ancres syntaxiques, symbole, diff d'AST ; CRDT/OT
  seulement si mesuré (une convergence n'est pas une compilation).
- S8 réel : comparer worktrees / workspace partagé / séquentiel avec des agents
  réels si des statistiques fiables existent ; sinon agents scriptés + rapport
  explicite « pas de tokens mesurés ». Ne jamais inventer de consommation.

  **Statistiques d'usage vérifiées dans l'environnement (2026-11-09)** : le harnais
  multi-agents local journalise, pour chaque message assistant, un objet
  `data.usage {inputTokens, outputTokens, totalTokens, cacheReadTokens}` dans
  `%DSH_HOME%/sessions/<workspace>/<session>/session.v4.jsonl.zstd` (zstd, lisible
  par Node 26 `zlib.zstdDecompressSync`). Chaque agent durable (Lead et teammates)
  possède son propre fichier de session, mis à jour pendant l'exécution. Les tokens
  et la durée par agent sont donc **mesurables sans clé API**, ce qui autorise une
  campagne S8 réelle à petite échelle avec les subagents du harnais. Limites : pas de
  ventilation par tâche, `cacheReadTokens` inclus dans les totaux, pas de coût
  monétaire exposé, et une campagne concurrente consomme réellement du budget LLM —
  elle doit rester bornée et pré-enregistrée. L'outil sans dépendance
  `tools/agent-usage/usage.mjs` agrège ces compteurs par session/agent (tokens
  d'entrée, de cache, de sortie, tokens traités, durée, profondeur de délégation) et
  sert de base de mesure pour S8 ; première exécution validée sur 10 sessions du
  workspace Ravum.

## 7. L4 — tests, CI, DSP, vérification (conception)

- Suite complète `cargo test --workspace`, démos M1–M4, E2E navigateur (10
  scénarios), CI produit + impact, job auto-GC : conservés sans affaiblissement.
- CI par impact : règles conservatrices inchangées ; exercice sur une vraie PR
  vers `dev` **uniquement après** que `dev` contient les crates (aujourd'hui ce
  n'est pas le cas : ne pas simuler). En attendant, valider l'impact sur les
  branches de travail et consigner le blocage.
- DSP : garde non possédante, destruction hors thread temps réel, crossfade,
  tests d'alloc/free, waveOut, WASAPI partagé/exclusif, MMCSS, XRUN proxy ;
  stress : swaps répétés, instances multiples, réinitialisation, buffers
  variables, destruction pendant fermeture, arrêt worker, saturation, races.
  Aucune qualification hard-RT.
- Preuves : ProofBroker/ledger conservés ; un prouveur externe (Kani) reste
  bloqué sur hôte Windows (Linux/WSL2 requis) ; documenter, ne pas simuler.
- Concurrence : tests pendant compilation, analyse, écriture Git, publication,
  migration, cache, GC, arrêt/redémarrage, journal. Un test réussi n'est pas
  une preuve générale d'absence de race.

## 8. Risques et non-claims de MVP-0c

| Risque | Traitement |
|---|---|
| `cargo metadata` non disponible / workspace non standard | fallback Cargo complet, rapporté, jamais une indépendance inventée |
| Déterminisme de `.rmeta` insuffisant pour l'identité d'interface | mesurer (double compilation, hash stable) ; sinon hasher un sous-ensemble d'interface ou utiliser `cargo check` par paquet comme oracle |
| Coût réel des vérifications inter-crates supérieur au gain | mesurer et publier ; ne pas déclarer un gain sans mesure |
| Collision d'écritures entre workers | propriété par fichier (tableau §4.4), tâches partagées, un seul écrivain Git par étape |
| Machine chargée par d'autres agents | mesures mono-exécution avec charge déclarée ; p50 exacts, comparaisons relatives |
| PR #78/#80 non fusionnées | aucune fusion par l'agent ; PR de travail empilée et documentée |

**Non-claims MVP-0c** : pas de compilateur Ravum, pas de preuve générale
`PROVED`, pas de prouveur externe exécuté, pas de hard-RT, pas de supériorité
multi-agents revendiquée sans mesure, pas de fusion de PR, pas de licence, pas
d'ADR accepté, pas de release/déploiement.

## 9. Séquence de démarrage

1. Branche `feat/mvp0c-hardening` depuis `ea7200c`, audit ci-dessus (fait).
2. Worker A : audit structuré des PR #78/#80 (read-only, rapport dédié).
3. Worker B : L1 (`ravum-unit-graph`, fixture multi-crates, 16 tests négatifs,
   extension daemon `externs`), avec gate central.
4. Worker C : mesures L2 sans modification du daemon tant que l'interface L1
   n'est pas stabilisée.
5. Worker D : L3 coordinator ; Worker E : L4 CI/tests ; Worker F : S8 ;
   Worker G : DSP ; Worker H : vault/documentation.
6. Chaque lot se termine par : tests reproductibles, rapport `docs/mvp/MVP-0c-*`,
   session Obsidian vérifiée, commit conventionnel, PR/branche poussée.
