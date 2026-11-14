# Journal de session — GOAL « prêt pour la revue de production » (Core 1.0)

> Journal de reprise. Mis à jour après chaque tâche. Il dit ce qui a été
> **exécuté** (commande, SHA, résultat chiffré), ce qui a été **décidé** et
> pourquoi, et ce qui **reste**. Il ne prouve rien par lui-même : la preuve est la
> commande rejouable et son journal brut dans `target/`.
>
> Statuts : `TESTED`, `PARTIAL`, `NOT_RUN`, `UNKNOWN`, `BLOCKED`,
> `PENDING_HUMAN_APPROVAL`. Jamais `PROVED`.

## Entrée 2026-11-13 — Phase 0 : état initial et base de référence

| Champ | Valeur |
|---|---|
| Tâches | 0.1, 0.2, 0.3, 0.4 |
| Statut | `TESTED` (baseline établie) |
| SHA | `c931fdfe346474b838ba1856f3a84bf6798bba87` |
| Branche | `goal/production-ready`, créée depuis `test/coverage-batch-1` |

### Ce qui a été observé et fait

- **0.1** `git status --short` : seul élément non suivi =
  `docs/audit/2026-11-13-architecture-audit.md`. Branche de départ
  `test/coverage-batch-1`, HEAD `170d37e`, 5 derniers commits listés dans le
  journal brut de la session.
- **0.2** L'énoncé annonçait 11 fichiers modifiés non commités : **état périmé**.
  Le lot couverture est commité (`60d4db1`, `1868286`, `170d37e`) et poussé.
  `cargo test -p ravum-lang-sema -- --test-threads=2` → **683 passés / 0 échec** ;
  `python -m unittest discover -s tools/tests` → **33 OK**. Le rapport d'audit a
  été commité seul : `c931fdf docs(audit): add the independent 2026-11-13
  architecture audit`. Rien n'a été jeté.
- **0.3** `git fetch --all --prune` fait. `dev` = `38278e1` ne contient pas les
  3 commits du lot couverture (`dev..test/coverage-batch-1` = 3), donc la
  branche `goal/production-ready` part de `test/coverage-batch-1` @ `c931fdf`.
  Décision conforme au repli prévu par l'énoncé ; à répéter dans la PR.
- **0.4** Vérification complète exécutée à `c931fdf` :
  `fmt` 0 ; workspace **94 groupes, 1574 passés / 0 échec / 2 ignorés** ;
  conformance **471 cas, 470 PASS / 0 FAIL / 1 SKIP**, `perimeter=202/202`,
  `verified=121`, `implicit=81`, `build_verified: true` ; smoke **43/0/0** ;
  `validate_repo_docs` PASS ; `report_consistency` PASS ; registre
  **108/126/0/0** ; `features_coverage --strict` **188/188**. Détail et seuils
  de non-régression : `docs/reports/GOAL-BASELINE.md`.

### Décisions prises

1. Partir de `test/coverage-batch-1` (et non `dev`) pour conserver le lot
   couverture ; documenté dans la baseline et à mettre dans la PR.
2. Consigner les écarts énoncé/réalité au lieu de les lisser : « 11 fichiers »
   périmé, « 113 suites » non reproduit (94 groupes mesurés, pass/ignorés
   identiques), répertoire `C:` en réalité `C`+U+F03A.
3. Ne pas re-mesurer la couverture (89,08/88,39) en phase 0 : `NOT_RUN` jusqu'à
   la phase 9, conformément à la règle R3 (aucun chiffre recopié).

### Ce qui reste

- Phase 1 (gouvernance, base de confiance, garde `forbid`, garde `HashMap`,
  répertoire `C`), puis phases 2 à 9 dans l'ordre.
- La PR brouillon vers `dev` n'est pas encore ouverte (phase 9.6).

## Entrée 2026-11-13 — Phase 1 : gouvernance et base de confiance

| Champ | Valeur |
|---|---|
| Tâches | 1.1 → 1.7 |
| Statut | `TESTED` (7/7 traitées ; 1.2 `PENDING_HUMAN_APPROVAL`, 1.7 cause `UNKNOWN`) |
| Commits | `41851db` (STATUS + START_HERE), `fac8a8c` (AGENTS, pending), `c04f33e` (statut du langage), `dddfd32` (design §1), `b129024` (gardes codegen), `85e993e` (garde HashMap), `0ea05b7` (hygiène racine) |
| SHA de vérification | arbre à `0ea05b7` + journal |

### Ce qui a été fait

- **1.1** `STATUS.md` et `START_HERE.md` réécrits : deux lignes réelles, langage
  fusionné dans `main` le 2026-11-12, cible D2, chiffres de la phase 0 et
  pointeurs vers le registre et la baseline. L'ancienne phrase « aucune
  implémentation commencée » a disparu.
- **1.2** `AGENTS.md` corrigé (mandat périmé) dans un **commit séparé**, en-tête
  `PENDING_HUMAN_APPROVAL` : il ne prend effet qu'après approbation humaine.
- **1.3** `LANGUAGE-CORE-STATUS.md` : une seule section « état courant » (chiffres
  mesurés à `c931fdf`, note sur l'écart 94 vs 113 « suites »), puis l'historique
  des passes **déplacé tel quel** en annexe. Aucune information supprimée.
- **1.4** `RAVUM-CORE-DESIGN.md` §1 : la base de confiance est nommée —
  `rustc` sous `forbid(unsafe_code)` + prélude + wrappers FFI ; le checker Ravum
  reste l'autorité des diagnostics ; conséquence : Ravum ne peut pas accepter un
  programme que `rustc` refuse sur la voie B.
- **1.5** Deux gardes ajoutés dans `backend_audit.rs` :
  (a) programme **sans** `extern` : `#![forbid(unsafe_code)]` conservé et zéro
  construction `unsafe` émise, même si la source Ravum contient `unsafe` ;
  (b) programme **avec** `extern` : `#![deny(unsafe_code)]`, exactement **un**
  `unsafe` généré par wrapper, **aucun** hors wrapper (scanner structurel).
  **Preuves de rouge** (une fois, puis rétablies) : retirer `forbid` fait échouer
  (a) ; émettre un `unsafe { }` au niveau crate fait échouer (b) et (a).
  `emit.rs` est restauré à l'identique (`git diff` vide).
- **1.6** `determinism_guard.rs` : échoue si `HashMap`/`HashSet` apparaît dans
  `crates/ravum-lang-*/src` (non-vacuité : ≥5 crates, ≥20 fichiers). **Preuve de
  rouge** : fichier sonde `guard_probe_tmp.rs` contenant `HashMap` → test rouge,
  fichier supprimé → vert.
- **1.7** Répertoire `C`+U+F03A inspecté : **0 fichier**, 6 répertoires vides
  (`Users\barat\AppData\Local\Temp\ravum-sh-test-4425bb`), datés du 2026-11-09.
  Aucun écrivain `ravum-sh-test` dans l'arbre ni dans l'historique (`git grep`
  sur toutes les révisions) : cause **`UNKNOWN`**. Supprimé après preuve qu'il ne
  contenait aucun fichier ; garde ajouté dans `tools/tests/test_repo_hygiene.py`
  (détecte `C:`, `C`+U+F03A, `C`+U+FF1A à la racine).

### Vérification de fin de phase (arbre courant)

- `cargo test --workspace --all-targets -- --test-threads=2` :
  **95 groupes : 1577 passés / 0 échec / 2 ignorés**, exit 0.
- Conformance : **471 cas, 470 PASS / 0 FAIL / 1 SKIP**, `perimeter 202/202`,
  `build_verified: true`, exit 0.
- Smoke **43/0/0** ; `validate_repo_docs` PASS ; `report_consistency` PASS ;
  registre 108/126/0/0 ; `features_coverage --strict` 188/188 ; contrats Python
  **38 OK** (33 + 5 nouveaux) ; `cargo fmt --check` exit 0.

### Décisions et points ouverts

1. `emit.rs` n'a **pas** été modifié au final : les preuves de rouge étaient des
   mutations temporaires, retirées.
2. La cause du répertoire `C` reste inconnue (écrivain absent du dépôt) : la
   garde couvre la récidive, pas une correction de code impossible à localiser.
3. `AGENTS.md` reste `PENDING_HUMAN_APPROVAL` : à signaler dans la PR, ne pas
   fusionner sans accord.
4. `main` local périmé (`16b1ff2` vs `origin/main` `8a58357`) : non touché.

### Ce qui reste

Phase 2 (oracle différentiel `check` ⇒ `build`), objet de la suite de la session.

## Entrée 2026-11-13 — Phase 2 : oracle différentiel check ⇒ build

| Champ | Valeur |
|---|---|
| Tâches | 2.1 → 2.7 |
| Statut | **PARTIAL** : générateur, propriétés 1/3, réducteur et modes livrés ; **0 divergence sur 3 600 programmes** ; mode long nominal (30 000) `NOT_RUN` localement |
| Commit | `2539d5c test(codegen): add a deterministic differential oracle (check => build)` (+ job CI `differential-oracle.yml`) |
| Fichiers | `crates/ravum-lang-codegen/tests/differential_oracle.rs`, `.github/workflows/differential-oracle.yml`, `docs/qualification/T2-FINDINGS.md` §25 |

### Ce qui a été fait

- **2.1** `fuzz_check.rs` et `fuzz_lex_parse.rs` lus : ils testent la **totalité**
  et le **déterminisme** du frontend (mutations de templates), pas la propriété
  `check` accepte ⇒ `rustc` compile. Aucun doublon : nouveau harnais, même style
  de générateur xorshift64*.
- **2.2** Générateur déterministe **bien typé par construction** : 34 gabarits
  d'instructions typés (structs, enums à payload, tuples, tableaux imbriqués,
  `Vec`, `String`, `Option`, `Result`/`?`, moves par projection, `match` avec
  liaisons, `if`/`else`, `while`/`loop`/`for`, shadowing, méthodes
  `&self`/`&mut self`/`self`, arguments `&`/`&mut`, coercition slice, flottants).
  Chaque valeur non-`Copy` est consommée au plus une fois.
- **2.3** Propriétés : (1) `check` accepte ⇒ `rustc` compile ; (2) debug == release
  sur un échantillon 1/10 ; (3) même graine ⇒ mêmes octets et deux passes
  `check`/codegen identiques. Aucune n'a échoué.
- **2.4** Réducteur implémenté (suppression d'instructions tant que la divergence
  persiste, sortie `target/diff-divergences/`) : **non exercé** faute de
  divergence ; efficacité `UNKNOWN`, deux tests de non-vacuité ajoutés.
- **2.5–2.6** Aucune divergence trouvée : aucun cas de conformance ni correctif
  ajoutés. Rien n'a été « corrigé » pour faire du vert.
- **2.7** Deux modes : rapide `3 × 200` dans `cargo test` ; long `#[ignore]`
  `3 × 10 000` + job CI dédié (hebdomadaire et `workflow_dispatch`, artefact des
  divergences réduites).

### Mesures (cette session, `rustc 1.95.0`, Windows 11 x64)

| Campagne | Programmes | Acceptés | Divergences | Durée |
|---|---|---|---|---|
| Rapide (défaut) | 3 × 200 = **600** | 600 (100 %) | **0** | 177 s |
| Longue (locale, budget réduit) | 3 × 1 000 = **3 000** | 3 000 (100 %) | **0** | 1 104 s |
| Longue nominale | 3 × 10 000 = 30 000 | — | — | **`NOT_RUN`** (≈ 3 h estimées ; job CI fourni) |

L'écart avec l'énoncé est explicite : le mode long **nominal** n'a pas été
exécuté localement, faute de budget de temps sur cette machine ; 3 000
programmes l'ont été, et le job CI exécute la configuration nominale. Le statut
de la phase reste donc `PARTIAL`, pas `TESTED`.

### Ce qui reste

- Phase 3 (`--timings`, mesure par phase, `-C incremental`), puis 4 à 9.
- Si le job CI de l'oracle est lancé un jour : remplacer la ligne `NOT_RUN` par
  le résultat du run, avec le SHA.

## Entrée 2026-11-13 — Phase 3 : mesure puis incrémental rustc

| Champ | Valeur |
|---|---|
| Tâches | 3.1 → 3.5 |
| Statut | **TESTED** : `--timings` livré, campagne 20+20 publiée, incrémental mesuré puis laissé **désactivé**, reproductibilité release prouvée |
| Commits | `4262569` (`--timings`), `84eaa08` (incrémental opt-in), `3b6eb75` (script de mesure) |
| SHA de mesure | `3b6eb755e8dfe0255bcdb1b8ffa83e0716abb455` |
| Rapport | [`docs/reports/LANG-TIMINGS.md`](../../reports/LANG-TIMINGS.md) |

### Ce qui a été fait

- **3.1** `--timings` sur `check` et `build` : `load`, `parse`, `resolve`,
  `typeck`, `effects`, `own` (check) + `codegen`, `rustc`, `total` (build).
  `check --json` ajoute un objet **additif** `timings` (schéma inchangé,
  `schema_version: 1`) ; `build` imprime une ligne `timings:` (il n'a pas de
  contrat JSON — aucun format JSON non enregistré n'a été inventé).
  `parse` est accumulé par le loader (tous fichiers, dépendances incluses) ;
  `load` l'exclut, donc `total` n'est pas doublé. Tests :
  `crates/ravum-lang-cli/tests/timings_contract.rs`, **3/3**.
- **3.2** `scripts/lang-timings.ps1` : 20 exécutions à froid + 20 après une
  ligne modifiée, par exemple et par commande, p50/p95 par phase, JSON avec SHA,
  rustc, OS, CPU, RAM. Campagne complète exécutée deux fois (avant puis après
  les commits ; la seconde seule est publiée).
- **3.3** `-C incremental=<cache>/incr/<clé de session>` pour les builds debug
  seulement. Clé = nom/type de crate, cible, opt-level, rustc, générateur —
  **pas** le digest des sources (sinon le répertoire serait froid à chaque
  édition). Répertoire hors identité/digest d'artefact ; `cache clean` le
  supprime.
- **3.4** Mesure avant/après : le gain est **nul ou négatif** (tableau du
  rapport ; jusqu'à +44 % sur `file-store`, +10 % sur 800 fonctions). Décision :
  option **désactivée par défaut**, activable par `RAVUM_INCREMENTAL=1` ;
  release/test jamais concernés.
- **3.5** `scripts/repro-check.ps1` sur l'arbre : deux builds release
  bit-à-bit identiques, sha256
  `0C7B63C66C27B0874AF5679FB0576C63A3DFC7B4B76C0E32A7AD8337A6AB64A8`.

### Mesures principales

- `check` froid p95 : **1,3 → 8,1 ms** selon l'exemple ; édition p95 :
  **11,0 → 49,8 ms** (le coût est la relecture du fichier fraîchement écrit,
  pas une phase du compilateur).
- `build` p50 : **179 → 331 ms** ; `rustc` représente **97,1-99,3 %** du total ;
  deux p95 aberrants (613,9 et 646,0 ms) sont publiés sans être filtrés.
- Repro release : **PASS**, digest identique sur deux racines.
- P0-A (diagnostic LSP p95 < 50 ms) **non mesuré ici** : `NOT_RUN`, phase 7.

### Ce qui reste

- Phase 4 (source unique des faits d'ownership) : étape A puis B, étape C
  seulement si ≤ 20 désaccords.

## Entrée 2026-11-13 — Phase 6 : identité des nœuds (sans objet, prouvé)

| Champ | Valeur |
|---|---|
| Tâches | 6.1 → 6.3 |
| Statut | **TESTED** — la migration est **sans objet** : les `NodeId` sont déjà par fichier |
| Commits | `0595b9f` + `7916840` (test de stabilité), `91d3621` (RFC-NODE-ID) |
| Preuve | `cargo test -p ravum-lang-sema --test node_id_stability` → **2/2** |

- Le parser alloue `NodeId = (FileId << 24) | compteur_local` ; ajouter une
  instruction dans `main.rv` laisse l'instantané AST complet de `util.rv`
  **identique** (ids et spans), mesuré via les overrides du loader.
- La phase 6.3 (migration) n'est donc **pas exécutée** : elle serait un
  refactoring sans consommateur (R11).
- Ce qui reste vrai et est écrit dans `docs/rfc/RFC-NODE-ID.md` : `FileId` dépend
  de l'ordre de chargement, donc **aucun cache ne doit être indexé par
  `FileId`** ; les caches se indexent par hash de contenu, les références
  inter-fichiers par chemin de définition.

## Entrée 2026-11-13 — Phase 7 : cache de parse dans le LSP

| Champ | Valeur |
|---|---|
| Tâches | 7.1 → 7.3 |
| Statut | **TESTED** (analyse en processus) / `NOT_RUN` pour la latence de bout en bout |
| Fichiers | `sema/loader.rs` (`ParseCache`), `lsp/src/analysis.rs`, `lsp/src/server.rs`, `lsp/tests/parse_cache.rs` |
| Mesure | p95 **1,362 ms** avec cache contre **1,836 ms** sans, sur 30 éditions d'une ligne de `package-demo` |

- **7.1** Cache borné (512) indexé par `(hash FNV-1a 64, FileId)` ; le parser
  seul est court-circuité, `resolve`/`typeck`/`own`/`effects` restent globaux.
  `Analysis::run` garde son comportement (cache neuf) ; le `Server` en tient un.
- **7.2** Mesure en processus, transport JSON-RPC exclu (écrit tel quel dans
  `docs/qualification/LSP.md` §7) : cible P0-A p95 < 50 ms **atteinte au niveau
  de l'analyse** (1,4 ms), **pas** revendiquée pour le bout en bout.
- **7.3** `stale_diagnostics_are_never_reused_for_the_new_buffer` : un
  diagnostic calculé pour l'ancien buffer n'est jamais republié (INV-07) ;
  `unchanged_file_is_not_reparsed` vérifie hits/misses exacts.

### Ce qui reste

- Phase 5 (traps du profil `rt`) et phase 8 (bloc DSP), puis phase 9
  (qualification, rapport, PR).

## Entrée 2026-11-13 — Phases 4, 5, 8 : état honnête (non implémentées)

Ces trois phases demandent des changements de fond (moteur d'ownership partagé,
règle de langage, ABI générée) qui n'ont **pas** été menés à bien dans la
session. Elles sont consignées ici avec ce qui existe (RFC, lecture du code) et
ce qui manque, plutôt que maquillées en `TESTED`.

### Phase 4 — source unique des faits d'ownership : `PARTIAL`

**4.1 (fait) — inventaire des décisions prises par `emit.rs`** (lecture de
`crates/ravum-lang-codegen/src/emit.rs` à `6257b84`) :

| Lieu (ligne) | Décision re-dérivée par le backend |
|---|---|
| `is_copy_type` (1594-1638) | copie vs move par type ; « miroir » explicite de `sema::own::is_copy_type` (commentaire 1596) |
| `is_owned_position` (1640) | position possédée d'une expression |
| `plan_array_moves` (1649) / `MovePlanner` (3574-3592) | quelles places de tableau sont déplacées, par un **walk syntaxique indépendant** (scopes, binders, `whole_uses`, `descents`) |
| `single_move_expr`, `chain_move_expr`, `build_destructure`, `plan_prefix_slots` (1668, 1703, 4165, 4259) | forme du déstructurage et où le hoister |
| `builtin_method_moves_receiver` (927) | un builtin consomme-t-il son récepteur |
| `Access`/`WholeUse`/`Descent` dans `MovePlanner` | usages complets, descentes, accès par bras de `match` |

Conclusion : la duplication de C2 est **confirmée** (deux modèles : `own.rs` et
`MovePlanner`), et c'est bien la classe P0-A.

**4.2 (fait)** `docs/rfc/RFC-OWN-FACTS.md` : structure `OwnFacts`, production par
`own.rs`, consommation en trois étapes, critère d'arrêt (> 20 désaccords), diff
byte-à-byte exigé pour C.

**4.3–4.5 : `NOT_RUN`.** Raison : l'étape A exige de threader `NodeId` dans
toutes les décisions d'accès d'`own.rs` (`consume_place`, `read_place`,
`write_place`, `borrow_place` et leurs ~15 sites d'appel) et de remplacer le
`MovePlanner` en C ; à l'échelle de la session, le risque de casser la classe
P0-A (que l'oracle différentiel surveille) dépasse le bénéfice d'une étape A
seule. Aucun demi-changement n'a été laissé. Statut : `PARTIAL` (4.1-4.2),
`NOT_RUN` (4.3-4.5).

### Phase 5 — traps du profil `rt` : `NOT_RUN` (RFC écrite)

`docs/rfc/RFC-RT-TRAPS.md` (PROPOSED, `PENDING_HUMAN_APPROVAL`) définit la règle,
les alternatives, les formes explicites (`wrapping_*`/`saturating_*`/`checked_*`)
et les cas de conformance attendus. **Aucune ligne de compilateur, aucun cas de
corpus, aucun changement d'`examples/dsp-gain-iir`** : non implémenté, donc
`NOT_RUN`. Décision humaine requise avant toute implémentation (règle de
langage, et la RFC inclut deux points non tranchés : décalages/négation, sort de
`pure`).

### Phase 8 — bloc DSP `.rv` : `NOT_RUN` (RFC écrite, ABI lue)

**8.1 (fait)** ABI résumée : `ravum_process_block(*const f32, *mut f32, u32,
*mut DspState)`, `DspState { z1: f32, z2: f32 }`, `cdylib` chargé par
`ravum-patch-host`, crossfade sans allocation, state possédé par l'hôte.
**8.2 (fait)** `docs/rfc/RFC-RV-DSP-BLOCK.md` : wrapper généré, `unsafe`
local, digest FNV-1a du schéma, refus de swap sur schéma différent (INV-03),
pas de migration.
**8.3–8.4 : `NOT_RUN`** : implémentation (flag CLI, wrapper codegen, protocole,
tests de swap) non commencée. `PENDING_HUMAN_APPROVAL` sur l'ABI et le digest.

## Entrée 2026-11-13 — Phase 9 : qualification et rapport final

| Champ | Valeur |
|---|---|
| Tâches | 9.1 → 9.7 (9.7 `NOT_RUN`) |
| Statut | **PARTIAL** : toutes les preuves rejouables l'ont été ; Miri, mutation, 30 000 programmes, LSP bout en bout, découpage : `NOT_RUN` |
| Rapport | [`docs/reports/GOAL-PRODUCTION-READINESS-REPORT.md`](../../reports/GOAL-PRODUCTION-READINESS-REPORT.md) |
| Registres mis à jour | `CORE-1.0-GATE-MATRIX.md` §0 octies, `PRODUCTION-READINESS.md` §9 (sections datées, **aucun gate changé**) |

### Preuves rejouées (SHA final, arbre `faf65cb`)

- Workspace **99 groupes / 1 587 passés / 0 échec / 4 ignorés** ;
  conformance **471 / 470 PASS / 0 FAIL / 1 SKIP**, `perimeter 202/202`,
  `verified 121`, `implicit 81`, `build_verified: true` ; smoke **43/0/0**.
- Registre **108/126/0/0** ; FeatureIds **188/188** ; MUST **807 / 344 non
  mappés** (D10) ; docs PASS ; fmt 0.
- Couverture **89,21 % lignes / 88,61 % régions**, **tous les seuils tenus**,
  10 fichiers sous l'objectif 100 % (2 103 lignes non couvertes, vue
  instanciations) ; aucun test « pour la ligne ».
- Fuzz long : syntaxe **249 054 entrées / 0 échec** (digest
  `0x9b09fc61e761b87e`), sema **100 000 itérations / 0 échec** (4 660/s).
- LSP client **11/0** ; cache de parse LSP p95 **1,362 ms** (analyse).
- Reproductibilité release **PASS**, sha256
  `19743DC29F3937E09E3765A485FFAB545B412D4DF22E2B8FBB2A2CD0BECD10B6`.
- SBOM **64 paquets / 170 relations**, digest `5119e194…` ; installateur Windows
  rejoué (installation, désinstallation deux fois idempotente).

### Ce qui n'a pas été exécuté

Miri (nightly non garantie), mutation (`cargo-mutants` absent), mode long
nominal de l'oracle (30 000, ≈ 3 h), latence LSP de bout en bout, découpage des
monolithes (9.7). Phases 4.3-4.5, 5 et 8 : `NOT_RUN` avec RFC écrites.

### Suite

- **9.6** PR brouillon ouverte vers `dev` :
  [PR #110](https://github.com/Rwanbt/Ravum/pull/110), sans auto-merge.
  Base `dev`, tête `goal/production-ready` ;
  la PR inclut les 3 commits du lot couverture absents de `dev` (expliqué dans
  sa description).
- CI déclenchée sur le SHA de la PR (11 checks). Résultats consignés dans
  l'entrée de clôture ci-dessous ; tout rouge doit être corrigé ou rapporté.

### Ce qui reste après cette session

- Phases 4.3-4.5, 5, 8 : à reprendre après ratification des RFC.
- Mode long nominal de l'oracle, Miri, mutation, LSP bout en bout.
- T4, T12, D9, D10, D7 : humains.

## Entrée 2026-11-13 (round 2) — Phase 4 : étapes A et B partielles

| Champ | Valeur |
|---|---|
| Tâches | 4.3 `TESTED`, 4.4 `PARTIAL`, 4.5 `NOT_RUN` |
| Commits | `afb5783` (étape A), `fbb0aeb` (étape B + test de non-vacuité) |
| Preuve | `cargo test -p ravum-lang-sema --test own_facts` : **2/2** ; `cargo test -p ravum-lang-codegen --lib` : **30/30** ; conformance **471 / 470 PASS / 0 FAIL** ; oracle rapide **600 programmes, 0 divergence** |

- **4.3 (A)** `own.rs` enregistre désormais, pour chaque expression à usage de
  valeur, `UseKind::{Move,Copy,Borrow,BorrowMut}` et la place
  (`binding` = span de déclaration + projections `Field/Tuple/Index/Deref`).
  `Facts.own` est additif, `BTreeMap`, `Default`. Les tests vérifient un move
  (`take(text)`), une copie (`x`, `s`), un emprunt (récepteur `&self`), un move
  projeté (`p.b` avec `Projection::Field("b")`) et le déterminisme. Aucun
  consommateur ne lit encore les faits.
- **4.4 (B, partiel)** `emit.rs::verify_ownership_facts` compare, en build
  debug, la classification `Copy`/`Move` de chaque usage à celle du backend
  (`emit::is_copy_type`). Un désaccord est un `CodegenError`. Mesure : **600
  programmes générés + 471 cas de conformance, 0 désaccord**. Le test
  `ownership_facts_disagreement_is_rejected_in_debug_builds` prouve que le
  garde peut échouer (décision `Copy` injectée sur un `String`). Limite : les
  **places** ne sont pas comparées, car `MovePlanner` ne mappe pas ses décisions
  sur les `NodeId`.
- **4.5 (C)** `NOT_RUN` : suppression du `MovePlanner` non tentée ; conditionnée
  au diff byte-à-byte et à la comparaison des places.

Note : `emit::is_copy_type` classe `Slice` et `Error` comme mobiles alors que
`own::is_copy_type` les classe `Copy` ; cet écart est **inatteignable** en Core
1.0 (une valeur de type slice/`Error` ne peut pas être consommée : les
références ne sont ni stockées ni retournées, et `CheckedProgram` n'existe pas
avec des types `Error`). Aucun test du corpus ne l'exerce ; il est consigné ici
plutôt que corrigé sans consommateur.

**CI du round 2** : sur `3a35b49`, les **8/8 workflows sont verts** (documentation,
contrats, VS Code, **coverage ratchet**, qualification Windows, macOS, MVP
produit, impact). Aucun correctif nécessaire.

**Défaut trouvé par la CI (round 2, docs-only `e2a8abc`)** : le job macOS a
échoué sur `own_facts_are_deterministic` avec `E-MOD-LOCK`. Cause réelle : les
helpers `TempDir` de trois tests d'intégration (`own_facts`,
`node_id_stability`, `parse_cache`) construisaient leur chemin avec `pid` +
horloge en nanosecondes ; la résolution d'horloge de macOS est assez grossière
pour que deux tests parallèles choisissent le **même répertoire**, l'un
supprimant le `ravum.lock` de l'autre. Ce n'est pas un défaut du compilateur.
Correctif : compteur atomique par processus dans les trois helpers
(`50a1154`), vérifié localement avec `--test-threads=4`. CI rejouée sur ce SHA.

## Entrée 2026-11-13 (round 3) — LSP de bout en bout ; Miri non installé

| Champ | Valeur |
|---|---|
| Tâche | 7.2 complétée (latence de bout en bout), 9.5 partielle (Miri) |
| Commit | `a980717` |
| Mesure | **p50 2,061 ms / p95 2,826 ms**, 20 éditions d'une ligne de `examples/package-demo/app`, transport JSON-RPC inclus |

- Nouveau test `#[ignore]` dans `crates/ravum-lang-cli/tests/lsp_client.rs` :
  le client LSP réel envoie 20 `didChange` (le buffer grandit d'une ligne
  inutilisée à chaque fois, donc un diagnostic change et un `publishDiagnostics`
  est garanti), et mesure l'aller-retour complet. Le fichier est édité en
  mémoire, jamais écrit.
- Piège corrigé pendant la mise au point : `fs::canonicalize` renvoie un chemin
  Windows verbatim (`\\?\`) qui produisait une URI `file:///\\?\...` invalide et
  empêchait le serveur de rattacher le document (timeout). Le préfixe est retiré
  dans le test.
- **P0-A** : cible p95 < 50 ms **atteinte** sur cette mesure locale (2,8 ms),
  avec la réserve « pipes locaux, une machine, pas de rendu éditeur ».
- **Miri** : `cargo +nightly miri` répond que `cargo-miri.exe` n'est pas
  installé pour la toolchain nightly locale. L'installer serait un outil
  supplémentaire (R5) ; reste `NOT_RUN` localement, la preuve Miri Linux de la CI
  n'est pas rejouée ici.
- **CI** : sur `a980717`, **8/8 workflows verts** (documentation, contrats,
  VS Code, coverage ratchet, qualification Windows, macOS, MVP produit, impact).

## Entrée 2026-11-13 (round 4) — Phase 5 : traps `rt` implémentés

| Champ | Valeur |
|---|---|
| Tâches | 5.1-5.3 `TESTED`, 5.4 `NOT_RUN` |
| Statut de phase | **`PARTIAL`** (`PENDING_HUMAN_APPROVAL` : règle de langage) |
| Fichiers | `sema/src/traps.rs` (neuf), `check.rs`, `lib.rs`, `builtins.rs`, `codegen/emit.rs`, 4 cas `conformance/cases/effects/rt-trap-*` |
| Vérification | sema **695/0** ; codegen lib **30/30** ; conformance **475 cas, 474 PASS / 0 FAIL / 1 SKIP**, build **203/203** ; workspace **100 groupes, 1 597 / 0 / 5** |

- **5.2** Nouvelle passe `traps.rs`, appelée **seulement** pour `Profile::Rt` :
  indexation (sauf littéral dans les bornes d'un `[T; N]`), division/reste
  (sauf diviseur littéral positif), `+ - *` entiers nus, négation unaire,
  décalages dynamiques ⇒ `E-EFF-PROFILE`. Flottants et `as` exemptés.
  Méthodes `wrapping_*`, `saturating_*`, `checked_*` ajoutées à la table des
  builtins et au passage direct du codegen (Rust std). **7 tests unitaires**
  couvrant négatifs et positifs. `core`/`pure` inchangés : le cas
  `effects/intrinsic-panic-not-tracked` reste accepté en core.
- **5.3** 4 cas de conformance : 3 compile-fail (`E-EFF-PROFILE`) et 1
  compile-pass avec formes explicites, indice littéral et flottants. Le corpus
  passe de 471 à **475** cas ; le périmètre build passe de 202 à **203**
  (1 vérifié de plus).
- **5.4 `NOT_RUN`** : `examples/dsp-gain-iir` lit/écrit des slices avec des
  indices dynamiques et alloue (`push`), deux choses interdites en `rt`. Aucun
  accesseur sûr (`get`/`get_mut`/`set`) n'existe dans la stdlib Core 1.0 ;
  l'adapter exigerait une nouvelle surface stdlib + codegen, hors RFC. L'exemple
  est inchangé ; aucune revendication temps réel.

### Ce qui reste

- Phase 8 (bloc DSP) : RFC seulement.
- Phase 4.5 (étape C), 30 000 programmes, Miri, mutation, couverture 100 %.
- T4, T12, D9, D10, D7 et ratification de `RFC-RT-TRAPS-1`.

**CI du round 4** : sur `5403734`, **8/8 workflows verts** (documentation,
contrats, VS Code, coverage ratchet, qualification Windows, macOS, MVP produit,
impact), coverage ratchet incluse malgré `traps.rs` et les 4 nouveaux cas.

## Entrée 2026-11-13 (round 5) — Oracle étendu (42 gabarits) et couverture re-mesurée

| Champ | Valeur |
|---|---|
| Tâches | phase 2 (renforcement), 9.2 (mesure) |
| Statut | **TESTED** pour la campagne ; couverture « baisse totale assumée » |
| Fichiers | `crates/ravum-lang-codegen/tests/differential_oracle.rs`, `T2-FINDINGS.md` §26, rapport, registres |

- **Générateur v2 (34 → 42 gabarits)** : ajout de deux moves d'éléments du même
  tableau, move de payload `Option<String>` / enum `String` via `match`, move de
  champ de struct imbriquée puis lecture d'un autre champ, move d'élément d'un
  tableau de structs non-`Copy`, `if`/`else` produisant une `String`, méthode
  `&self` sur un temporaire, shadowing de `String` en bloc imbriqué.
- **Campagnes** : rapide **3 × 200 = 600** (600 acceptés, 0 divergence, 194 s) ;
  longue locale **3 × 1 000 = 3 000** (3 000 acceptés, 0 divergence, 928 s) ;
  **100 % des programmes acceptés par `check`**, donc l'oracle n'est pas devenu
  vide en s'étendant. Cumul v1+v2 : **7 200 programmes, 0 divergence**.
  Mode nominal 3 × 10 000 : toujours `NOT_RUN` localement (≈ 3 h), job CI fourni.
- **Couverture re-mesurée** (9.2) : `cargo llvm-cov -p sema -p codegen` →
  **89,07 % lignes / 88,43 % régions**, **tous les seuils par fichier tenus** ;
  10 fichiers sous l'objectif 100 % ; **2 169** lignes non couvertes
  (19 840 − 17 671). Les totaux **baissent** de 0,14/0,18 pt par rapport au
  round 3 (89,21/88,61) parce que 354 lignes de code produit ont été ajoutées
  (`traps.rs` 78,15/71,33, `OwnFacts`/`check.rs`) ; aucun seuil n'a été abaissé.
  C'est consigné en tête de la comparaison du rapport, pas en bas.
- Aucun changement du compilateur dans ce round : uniquement le générateur de
  test et les documents.
- **CI** : sur `36ae70b`, **8/8 workflows verts**, coverage ratchet incluse.

## Entrée 2026-11-14 (round 6) — Affectation composée, couverture résorbée

| Champ | Valeur |
|---|---|
| Tâches | phase 5 (complément : affectation composée), 9.2 (couverture) |
| Statut | 5.2-5.3 renforcés ; couverture du round 5 **résorbée** |
| Fichiers | `sema/src/traps.rs` (+3 tests, règle `AssignOp`), 1 cas `rt-trap-compound-assignment` |

- **Trou trouvé en écrivant les tests** : `x += 1` n'était pas refusé en `rt`
  alors que `x + 1` l'était. `traps.rs` partage désormais une règle
  `check_binary` entre `Binary` et `AssignOp` ; le cas de conformance
  `effects/rt-trap-compound-assignment` le prouve (compile-fail
  `E-EFF-PROFILE`).
- **Tests** : 7 → **10** tests unitaires (affectation composée, largeurs de
  décalage u8/u16/u128, agrégats, `if`/`match`/`while`, littéral `&str`, cast).
- **Preuves** : conformance **476 cas, 475 PASS / 0 FAIL / 1 SKIP**, build
  **203/203** (`verified=122`, `implicit=81`), `build_verified: true` ;
  workspace **100 groupes, 1 600 / 0 / 5** ; sema **698/0**.
- **Couverture** (re-mesurée) : **89,29 / 88,67** (lignes/régions),
  **tous seuils tenus**, 2 133 lignes non couvertes (19 917 − 17 784) ;
  `traps.rs` passe de 78,15/71,33 à **93,37/89,39**. La baisse du round 5 est
  donc résorbée et le total repasse au-dessus du round 3 (89,21/88,61).
- Incident de mesure : un run de conformance a produit un timeout transitoire de
  `types/match-expression` (571 s) ; rejoué seul puis en corpus complet, il est
  **PASS** — c'était une contention d'environnement, pas une régression.
- **CI** : sur `28ad5da`, **8/8 workflows verts**, coverage ratchet incluse.

## Entrée 2026-11-14 (round 8) — Campagne nominale de l'oracle : 30 000 programmes, 0 divergence

| Champ | Valeur |
|---|---|
| Tâche | 2.4 (mode long nominal) — **fait** |
| Commande | `RAVUM_DIFF_PROGRAMS=10000 cargo test -p ravum-lang-codegen --test differential_oracle differential_oracle_long -- --ignored --nocapture` |
| Résultat | **3 × 10 000 = 30 000 programmes, 30 000 acceptés, 0 rejet, 0 divergence**, 8 602 s (2 h 23), 3 000 comparaisons debug == release (`release_every=10`) |
| Cumul | v1 + v2 + nominale = **37 200 programmes, 0 divergence** |

- Graines : `…0001` 10 000/10 000 acceptés en 3 022 s ; `…0002` 10 000/10 000
  en 2 797 s ; `…0003` 10 000/10 000 en 2 783 s.
- Le journal de la campagne est conservé dans `target/goal-nominal-oracle.log`
  (artefact local non versionné).
- Conséquence : le mode long nominal n'est plus `NOT_RUN` localement ; la phase 2
  passe à **`TESTED`**. La gate **T2 reste `PARTIAL`** pour D10 (344 MUST non
  mappés) — ce sont deux états différents.
- Limites inchangées : une machine, une toolchain, 8 instructions par programme,
  sous-ensemble documenté de Core 1.0 ; aucune preuve d'absence de divergence sur
  toutes les formes possibles.
- Aucun changement du compilateur ; mise à jour des documents seulement.

## Entrée 2026-11-14 (round 9) — `codegen/lib.rs` clos à 100 % de couverture

| Champ | Valeur |
|---|---|
| Tâche | 9.2 / D10 (couverture par fichier) |
| Fichier | `crates/ravum-lang-codegen/src/lib.rs`, `tools/coverage_thresholds.json` |
| Résultat | **100,00 % lignes / 100,00 % régions** sur ce fichier ; plancher relevé de 75/65 à **100/100** |

- Le seul code non couvert de `codegen/lib.rs` était
  `CodegenError::Display::fmt` ; le test
  `tests::codegen_error_displays_its_message` l'exerce (y compris via
  `&dyn std::error::Error`).
- Mesure après ajout : workspace `sema+codegen` **89,31 / 88,69** (contre
  89,29/88,67), **2 130** lignes non couvertes (19 925 − 17 795), **10 fichiers**
  sous l'objectif 100 % (contre 11) ; `traps.rs` inchangé à 93,37/89,39.
- Le plancher de `codegen/lib.rs` passe à 100/100 dans
  `tools/coverage_thresholds.json` ; `--check` passe. C'est le premier fichier
  du périmètre mesuré qui atteint l'objectif ratifié D10 par des tests, sans
  exclusion.
- Workspace : **1 601 tests** passés (1 ajouté).
- **CI** : sur `17ab64e`, **8/8 workflows verts**, coverage ratchet incluse avec
  le nouveau plancher 100/100 sur `codegen/lib.rs`.

## Entrée 2026-11-14 (round 10) — `traps.rs` à 98,66 % et limite D10 documentée

| Champ | Valeur |
|---|---|
| Tâche | 9.2 / D10 |
| Fichiers | `sema/src/traps.rs` (+3 tests), `tools/coverage_thresholds.json`, `docs/qualification/COVERAGE-100-OBJECTIVE.md` (neuf) |
| Mesure | workspace **1 604 tests** ; couverture **89,41/88,81** ; `traps.rs` **98,66/96,84** |

- **`traps.rs` 93,37/89,39 → 98,66/96,84** avec 3 tests supplémentaires
  (`?`/`Try`, bloc-expression + `[0; 3]` + `for i in 0..2`, largeurs i64/u64/
  isize/usize) et un test `#[should_panic]` qui exerce le rendu des diagnostics
  de parse. Le fichier entre au cliquet à **97/95**.
- **Limite structurelle trouvée et documentée** : `types.rs` (100 % lignes)
  plafonne à **98,10 % de régions** et `builtins.rs` à 98,42 %, parce que
  `llvm-cov` compte comme régions les chemins d'erreur `?` des `write!` dans
  `impl Display` ; ces régions ne sont atteignables que si le `Formatter`
  échoue. Le document `docs/qualification/COVERAGE-100-OBJECTIVE.md` fournit les
  segments mesurés (`count=0` sur les seuls `?`) et propose trois options
  humaines : exclusion ciblée, changement d'objectif, ou statu quo.
- Aucun seuil abaissé ; `codegen/lib.rs` reste à 100/100. Totaux :
  **2 112** lignes non couvertes (19 950 − 17 838), **10 fichiers** sous
  l'objectif.
- **CI** : sur `7977210`, **8/8 workflows verts**, coverage ratchet incluse avec
  les planchers `codegen/lib.rs` 100/100 et `traps.rs` 97/95.

## Entrée 2026-11-14 (round 11) — Suite complète en profil `--release`

| Champ | Valeur |
|---|---|
| Commande | `cargo test --workspace --all-targets --release -- --test-threads=2` |
| Résultat | **100 groupes, 1 603 passés / 0 échec / 5 ignorés**, exit 0 |
| Écart debug | 1 test de moins : `ownership_facts_disagreement_is_rejected_in_debug_builds` est `#[cfg(debug_assertions)]` (le garde de l'étape B est compilé hors release) |

- Le profil livré (release) passe la même suite que le debug, à l'exclusion du
  test qui vérifie le garde de debug. Aucun test ne dépend d'un comportement
  debug-only pour réussir.
- Aucun changement de code ; preuve ajoutée au rapport (§3), à la matrice des
  gates (T10) et à `PRODUCTION-READINESS.md` §9.

## Entrée 2026-11-14 (round 12) — Fuzz sema rejoué après les traps `rt`

| Champ | Valeur |
|---|---|
| Commande | `RAVUM_FUZZ_ITERS=100000 cargo test -p ravum-lang-sema --test fuzz_check fuzz_check_long -- --ignored --nocapture` |
| Résultat | **100 000 itérations, 0 échec, 18 957 ms, 5 275/s** |
| Profil | le générateur choisit `rt` 1 fois sur 3 ⇒ **≈ 33 000 exécutions de la passe `traps.rs`** |

- Le run de 100 000 itérations datait d'**avant** la phase 5 ; ce rejeu exerce
  `traps.rs` (y compris `AssignOp`, `Try`, ranges, agrégats) sous le fuzz.
- Aucun panic, aucun diagnostic non déterministe, aucun span invalide.
  `docs/qualification/FUZZ-MIRI-SEMA.md` §3.1 et §7 consignent la commande et
  le débit ; le débit plus élevé reflète une machine moins chargée.
- Aucun changement de code ; documents seulement.

## Entrée 2026-11-14 (round 13) — Reproductibilité, SBOM et installateur rejoués au SHA courant

| Preuve | Résultat (HEAD `662f63d`) |
|---|---|
| Reproductibilité | `repro-check.ps1` **PASS** : deux racines, deux `target/`, sha256 `988521151EFE827EA62448343C08128370CA3801A3EAEF312C04B298C43FDE8A` |
| SBOM | SPDX-2.3, **64 paquets (23 workspace + 41 dépendances) / 170 relations**, `created=1970-01-01`, sha256 `46b65d9fe941123e4ea7ac8fb82cfeb1e4ccb457d7cf29947ec5cd0251647721` |
| Installateur | install `-SkipBuild` exit 0, manifeste commit `662f63d`, binaire sha256 `bf55aab9…`, `locked=true`, rustc 1.95.0 ; désinstallation **0**, seconde désinstallation **0** (« rien à désinstaller ») |

- Ces trois preuves dataient du SHA `faf65cb`/`303da6a` ; elles sont rejouées sur
  l'arbre courant (code des phases 4-5 inclus) sans changement de procédure.
- Le sha256 du binaire change par rapport au run précédent (`19743DC2…`) : c'est
  attendu, le code a changé ; la propriété prouvée est l'**identité entre deux
  racines**, pas un hash stable entre versions.
- Documents mis à jour : rapport §3/T11, matrice des gates (lignes
  reproductibilité/SBOM/installateur), `PRODUCTION-READINESS.md` R7.

## Entrée 2026-11-14 (round 14) — Revalidation du corpus et des audits

| Contrôle | Résultat (HEAD `2ae9e9f`) |
|---|---|
| Conformance `--verify-build` | **476 cas : 475 PASS / 0 FAIL / 1 SKIP** ; `perimeter=203/203`, `verified=122`, `implicit=81`, `build_verified: true` |
| Smoke exemples (`-SkipBuild`) | **43 PASS / 0 FAIL / 0 NOT_RUN** |
| Registre | **126 codes émis**, 0 hors registre, 0 jamais émis |
| Traçabilité | **188/188** (1 exemption) |
| MUST | **807 énoncés, 344 non mappés** (D10 inchangée) |

- Le code est identique à `28ad5da` depuis la phase 5 ; seuls des tests et des
  documents ont changé. Les chiffres du rapport sont donc reproductibles sur
  l'arbre courant, sans écart.
- `must_audit.py` signale deux lignes de `grammar.md` avec un `|` non échappé
  (comptage inchangé, texte potentiellement tronqué) ; c'est noté dans le
  rapport §4.4, pas corrigé ici (hors périmètre de la session, document de spec).
- Aucun changement de code.

## Entrée 2026-11-14 (round 15) — Étape B étendue : comparaison des places

| Champ | Valeur |
|---|---|
| Fichiers | `crates/ravum-lang-codegen/src/emit.rs`, `docs/rfc/RFC-OWN-FACTS.md` |
| Vérification | conformance **476 / 475 / 0 / 1**, build **203/203** ; oracle rapide **600, 0 divergence** ; codegen lib **31/31** |
| Statut | 4.4 reste `PARTIAL` ; 4.5 `NOT_RUN` |

- `verify_ownership_facts` (debug uniquement) compare désormais, pour chaque
  expression ayant une place enregistrée, les **projections** (`Field`, `Tuple`,
  `Index(Lit/Opaque)`, `Deref`) à une place recalculée depuis la **syntaxe** —
  indépendamment de `MovePlanner`. Les places enregistrées qui n'ont pas de
  contrepartie syntaxique sont rejetées.
- Deux ajustements exigés par la sémantique du langage, découverts par le garde
  lui-même sur des cas réels :
  1. une projection sur une **référence** (`self.a` avec `&self`, `xs[0]` avec
     `&[T]`) auto-déréférence la base : un `Deref` est attendu avant la
     projection (un par couche de référence) ;
  2. un **récepteur de méthode** (`text.len()`) et un **argument de type
     référence** passé à un paramètre référence (`compte(v)`) sont réborrowés avec
     auto-déréférence par l'appel ; ces deux contextes sont **exclus** de la
     comparaison (limitation écrite).
- L'identité de la liaison (span de déclaration) n'est pas comparable depuis
  l'AST ; elle reste hors du garde. C'est la limite qui maintient 4.4 en
  `PARTIAL`.
- Aucune régression : les 4 échecs initiaux de conformance étaient tous des
  auto-déréférences contextuelles, pas des bugs du checker ; le garde a été
  précisé (types des bases, exclusions) sans toucher au compilateur.
- Couverture re-mesurée : **89,38/88,79** (contre 89,41/88,81 au round 10),
  `emit.rs` **87,43/86,85** (planchers 86/85 tenus), 2 141 lignes non couvertes
  (20 153 − 18 012), 10 fichiers sous l'objectif. Le reflux de 0,03/0,02 pt est
  dû aux ~240 lignes du garde, exercées par la conformance/l'oracle mais hors
  mesure llvm-cov.
- **CI** : sur `95e9b8b`, **8/8 workflows verts**, coverage ratchet incluse.

## Entrée 2026-11-14 (round 16) — Non-vacuité du garde de places

| Champ | Valeur |
|---|---|
| Fichier | `crates/ravum-lang-codegen/src/emit.rs` (test) |
| Test | `place_facts_disagreement_is_rejected_in_debug_builds` |
| Résultat | codegen lib **32/32** ; workspace **1 605** ; couverture **89,47/88,87** |

- Le test parse et check un vrai programme (`let a = [1, 2, 3]; let x = a[1];`),
  sélectionne la place enregistrée de `a[1]`, y **injecte une projection `Deref`
  erronée**, et exige que `crate::generate` échoue avec un message de place
  syntaxique. Il prouve que le garde de l'étape B n'est pas vide : une place
  corrompue est bien rejetée.
- Couverture : `emit.rs` **87,75/87,09** (planchers 86/85), totaux **89,47/88,87**
  (contre 89,38/88,79 au round 15), 2 126 lignes non couvertes
  (20 182 − 18 056), 10 fichiers sous l'objectif.
- Le test est `#[cfg(debug_assertions)]` : avec le test négatif Copy/Move, il
  porte à **2** le nombre de tests exclus en profil release (la mesure release du
  round 11 reste 1 603/0/5).

## Entrée 2026-11-14 (round 17) — Smoke et suite release rejoués après le garde de places

| Contrôle | Résultat (arbre `ed60f72`) |
|---|---|
| Smoke exemples | **43 PASS / 0 FAIL / 0 NOT_RUN** |
| Workspace `--release` | **100 groupes, 1 603 passés / 0 échec / 5 ignorés**, exit 0 (2 tests debug-only exclus) |

- Le smoke a été rejoué **après** l'extension du garde d'ownership aux places :
  aucun des 43 exemples n'est rejeté par la nouvelle comparaison (ils passent
  tous par `check` **et** `build`, donc par le garde en debug).
- La suite release confirme que le profil livré passe la même suite que le debug
  à l'exclusion des deux tests `#[cfg(debug_assertions)]` ; c'est la mesure
  courant de l'arbre, pas une extrapolation du round 11.
- Aucun changement de code ; documents seulement.

## Entrée 2026-11-14 (round 18) — Oracle 3 × 1 000 après le garde de places

| Champ | Valeur |
|---|---|
| Commande | `RAVUM_DIFF_PROGRAMS=1000 cargo test -p ravum-lang-codegen --test differential_oracle differential_oracle_long -- --ignored --nocapture` |
| Résultat | **3 × 1 000 = 3 000 programmes, 3 000 acceptés, 0 divergence**, 830 s, 300 comparaisons debug == release |
| Cumul oracle | **40 200 programmes, 0 divergence** (v1, v2, nominale, post-gardes+places) |

- Chaque programme passe par le **garde de cohérence de l'étape B** en debug
  (`Copy`/`Move` + projections de place) : aucun faux refus, aucune divergence.
- Ce rejeu valide à l'échelle l'extension des rounds 15-16, sur 3 000 programmes
  supplémentaires, sans attendre une nouvelle campagne nominale de 2 h 23.
- Documents : `T2-FINDINGS.md` §26.5, rapport (§1, §2, §3, R2, T2, §9), matrice
  des gates, `PRODUCTION-READINESS.md` R2.

## Entrée 2026-11-14 (round 19) — Qualification Linux + Miri sur l'arbre courant

| Champ | Valeur |
|---|---|
| Déclenchement | `gh workflow run linux-qual.yml --ref goal/production-ready` (dispatch manuel, pas la PR) |
| Run | [43562818570](https://github.com/Rwanbt/Ravum/actions/runs/43562818570), SHA `878e212` |
| Résultat | **7 jobs verts** ; TSan `skipped`/`NOT_APPLICABLE` |

- Workspace Linux : **100 groupes, 1 601 passés / 0 échec / 7 ignorés**.
- Conformance Linux : total **476**, **474 PASS / 0 FAIL / 2 SKIP**,
  `build_verified: true` (un SKIP de plus que Windows : cas spécifique plateforme).
- Smoke Linux : **31/0/0** ; stdlib 64/64 ; CLI 13/13 ; flags 20/20.
- **Miri** : syntax **65** tests, sema **134** tests (dont les 13 de `traps.rs`),
  fuzz court **2** tests — job vert. C'est la première exécution Miri sur le code
  des phases 4-5. Miri local reste `NOT_RUN` (composant absent, R5).
- wasm32-wasip1 : build + exécution via Node WASI, vert ; installateur : contrat
  vert ; fuzz smoke : vert.
- Le workflow ne se déclenche pas sur cette branche (triggers `main`/`dev`/
  `feat/**`/`fix/**`) ; le dispatch manuel est la seule façon de qualifier Linux
  sans merge. Ce n'est pas un gate de la PR.

## Entrée 2026-11-14 (round 20) — Mutation rejouée en CI sur l'arbre courant

| Champ | Valeur |
|---|---|
| Déclenchement | `gh workflow run mutation.yml --ref goal/production-ready` |
| Run | [43565189293](https://github.com/Rwanbt/Ravum/actions/runs/43565189293), SHA `b5298db` |
| Outil | `cargo-mutants 27.1.0`, `ubuntu-latest`, `rustc 1.95.0`, `--no-shuffle -j 8` |
| Résultat | **183 mutants / 150 tués / 24 survivants / 9 non viables** (artefact `mutants-targeted`) |

- Portée identique à §9/§10 (`own.rs`, `resolve.rs`, `typeck.rs`, filtres `-F`) ;
  totaux **exactement identiques** au run de référence de 2026-11-13, malgré les
  phases 4-5. Aucun nouveau survivant ; le tri E1–E6 reste valable.
- `cargo-mutants` reste absent localement (R5) : la preuve est CI, ciblée, et ne
  constitue pas un score de crate.
- Documents : `MUTATION-EQUIVALENCE.md` §11, rapport (§1, §3, R8, T10, §6),
  matrice des gates, `PRODUCTION-READINESS.md`.

## Entrée 2026-11-14 (round 21) — Corps de la PR #110 remis à jour

| Champ | Valeur |
|---|---|
| Action | `gh pr edit 110 --title … --body-file target/pr-110-body.md` |
| État | PR **toujours brouillon**, base `dev`, tête `7dd076a` ; aucun merge/tag/release |

- Le corps de la PR datait de `8abf258` : il annonçait encore 99 groupes /
  1 587 tests, 471 cas, 3 600 programmes, phases 4-5 `NOT_RUN`, Miri et mutation
  `NOT_RUN`. Il est réécrit avec les chiffres courants : workspace
  **1 605/0/5** (release 1 603), conformance **476 (475 PASS)**, oracle
  **40 200 programmes**, phases 4-5 partielles documentées, Miri et mutation
  **rejoués en CI** (runs et liens), couverture **89,47/88,87**.
- Le titre ne prétend plus « 4/5/8 not run » mais « phases 4-5 partial, 8 not
  run » — plus exact.
- Le corps rappelle la base de branche (`test/coverage-batch-1`, 3 commits absents
  de `dev`), les décisions humaines bloquantes, et le fait qu'aucun gate n'est
  fermé par la PR.
- Aucun changement de code ni de dépôt hormis cette entrée de journal.

## Entrée 2026-11-14 (round 22) — RFC-DEV-TOOLS-1 : débloquer les deux NOT_RUN locaux

| Champ | Valeur |
|---|---|
| Fichier | `docs/rfc/RFC-DEV-TOOLS.md` (neuf) |
| Statut | `PROPOSED` — `PENDING_HUMAN_APPROVAL` |
| Objet | autoriser l'installation **locale** de Miri (composant rustup) et de `cargo-mutants 27.1.0`, outils déjà utilisés par la CI |

- Les deux derniers `NOT_RUN` **locaux** (Miri, mutation) ne sont pas bloqués par
  le code : ils le sont par R5, qui interdit d'ajouter un outil sans RFC. La RFC
  expose le périmètre exact, les versions épinglées, les commandes prévues (avec
  les mêmes exclusions `--skip` et filtres `-F` que la CI), les risques et les
  options (statu quo / installation).
- **Aucune installation n'a été faite** : la décision appartient au responsable.
  En attendant, Miri et mutation restent `NOT_RUN` localement, avec les preuves
  CI datées (runs 43562818570 et 43565189293).
- Rapport : §5 (RFC ajoutée), §6 (les deux lignes renvoient à la RFC), §7
  (ratification). Rien d'autre n'est modifié.

## Entrée 2026-11-14 (round 23) — Index des RFC complété, SKIP Linux nommés

| Champ | Valeur |
|---|---|
| Fichiers | `docs/rfc/README.md`, rapport §4.5 |
| Nature | traçabilité / transparence de la revue, aucun changement de code |

- L'index `docs/rfc/README.md` s'arrêtait à `RFC-D13-STATICITY` : les cinq RFC de
  cette session (`OWN-FACTS`, `RT-TRAPS`, `NODE-ID`, `RV-DSP-BLOCK`,
  `DEV-TOOLS`) n'y figuraient pas. Elles sont ajoutées avec statut
  `PROPOSED · PENDING_HUMAN_APPROVAL`, date, état du prototype et lien.
- Le rapport §4.5 nomme désormais les **2 SKIP Linux** :
  `security/dependency-absolute-windows` et `security/reserved-windows-name`,
  tous deux spécifiques à Windows — au lieu de dire seulement « 2 SKIP ».
- Aucune conclusion, aucun gate, aucune preuve modifiés.

## Entrée 2026-11-14 (round 24) — Les têtes de registre ne mentent plus par omission

| Champ | Valeur |
|---|---|
| Fichiers | `docs/roadmap/CORE-1.0-GATE-MATRIX.md`, `docs/roadmap/PRODUCTION-READINESS.md` |
| Nature | note datée pointant vers l'état courant ; aucun chiffre « confronté » modifié |

- La matrice des gates affichait en tête « chiffres courants » de la passe 11
  (471 cas, 1 574 tests, couverture 89,08/88,39) sans dire qu'ils avaient évolué.
  Une note ajoutée donne l'état au 2026-11-14 (476 cas / 475 PASS, 1 605 tests,
  89,47/88,87, oracle 40 200, Miri/mutation CI) et renvoie à §0 octies, au §9 de
  `PRODUCTION-READINESS.md` et au rapport final.
- `PRODUCTION-READINESS.md` : même note en tête, pointant vers §9.
- Le bloc « chiffres courants » **reste inchangé** parce que
  `tools/report_consistency.py` le confronte au rapport consolidé ; la note ne
  s'y substitue pas. `report_consistency.py` passe après édition.
- `STATUS.md` n'est pas modifié : ses chiffres sont explicitement datés
  (`c931fdf`, 2026-11-13) et restent vrais pour cette date.

## Entrée 2026-11-14 (round 25) — `LANGUAGE-CORE-STATUS.md` remis à l'état courant

| Champ | Valeur |
|---|---|
| Fichier | `docs/reports/LANGUAGE-CORE-STATUS.md` |
| Mesures rejouées | sema **16 groupes / 701 passés / 0 échec / 1 ignoré** ; contrats Python **38 tests OK** |
| Nature | mise à jour de l'état courant ; l'annexe historique n'est pas réécrite |

- Ce document, lié depuis `STATUS.md`, annonçait « État courant » avec les
  chiffres de la **phase 0** (`c931fdf` : 94 groupes, 1 574 tests, 471 cas).
  Il porte désormais les chiffres du 2026-11-14 : workspace **100 groupes,
  1 605/0/5** (release 1 603), sema **701/0/1**, conformance **476 (475 PASS,
  build 203/203)**, smoke 43/0/0, registre 108/126/0/0, traçabilité 188/188,
  contrats Python **38**.
- Nouvelle §1.3 « Preuves ajoutées depuis la phase 0 » : oracle 40 200
  programmes, couverture 89,47/88,87 (`codegen/lib.rs` 100/100), fuzz long,
  LSP de bout en bout, Miri et mutation en CI, qualification Linux — avec liens
  et rappel qu'aucun n'est un `PROVED`.
- Les chiffres de la phase 0 restent dans `GOAL-BASELINE.md` ; l'annexe (§2)
  n'est pas réécrite. `report_consistency.py` passe.

## Entrée 2026-11-14 (round 26) — ABI de bloc DSP : contrat réel et découpage minimal

| Champ | Valeur |
|---|---|
| Fichier | `docs/rfc/RFC-RV-DSP-BLOCK.md` §7 (neuf) |
| Source | `fixtures/dsp/gain_v1.rs`, `crates/ravum-patch-host/src/{block,host}.rs`, tests `block_host.rs` |
| Nature | analyse de code + découpage ; **aucune implémentation** |

- L'ABI réellement attendue par l'hôte est consignée : exports
  `ravum_abi_version` (=2), `ravum_manifest_v1` (JSON versionné avec `patch_id`,
  `export`, `signature`), `ravum_process_block`, `DspState { z1, z2 }` en
  `repr(C)`.
- **Écart corrigé** : l'ABI v2 n'a **pas** de digest de schéma d'état ; le digest
  proposé au §6.2 serait une extension de protocole, pas une condition du
  contrat actuel. Le découpage minimal (codegen wrapper, CLI `--emit dsp-block`,
  tests, scan `unsafe`) est écrit, avec le point de couplage langage/hôte à
  trancher.
- Rien n'est implémenté : 8.3/8.4 restent `NOT_RUN`. Ce travail réduit le risque
  d'une future implémentation et évite de confondre l'ABI existante avec la
  proposition de digest.

## Entrée 2026-11-14 (round 27) — Campagne nominale relancée sur l'arbre courant

| Champ | Valeur |
|---|---|
| Commande | `RAVUM_DIFF_PROGRAMS=10000 cargo test -p ravum-lang-codegen --test differential_oracle differential_oracle_long -- --ignored --nocapture` |
| Objet | rejouer les **3 × 10 000** avec le garde de places actif (le nominal précédent, `aa88479`/8 602 s, datait d'avant les rounds 15-16) |
| État à la clôture | **en cours** — aucun résultat n'est avancé avant la sortie complète |

- Pourquoi : le rapport cite un nominal de 30 000 programmes exécuté **avant**
  l'extension du garde d'ownership aux places. La campagne 3 × 1 000 du round 18
  a validé le garde à petite échelle ; rejouer le nominal rend cette preuve
  homogène avec le code courant.
- Aucun autre build local n'est lancé pendant la campagne (R12).
- `RFC-RT-TRAPS.md` §7 : la ligne 5.4 est précisée — trois obstacles indépendants
  (indexation dynamique, `alloc`, **`io`**), et non un seul. L'exemple affiche
  son résultat, donc l'adapter en `rt` modifierait ce que le smoke vérifie :
  décision humaine, pas un contournement d'agent.

## Entrée 2026-11-14 (round 28) — Reproductibilité : mesure courante consignée

| Champ | Valeur |
|---|---|
| Fichier | `docs/qualification/REPRODUCIBILITY.md` |
| Mesure | `scripts/repro-check.ps1` au SHA `662f63d` : deux racines, **`988521151EFE…`** identique, PASS |
| Nature | documentation d'une mesure déjà exécutée (round 13) ; aucun build relancé pendant la campagne nominale |

- Le document de qualification s'arrêtait à la passe 8 ; il porte maintenant la
  mesure du code des phases 4-5, avec l'explication du **changement de digest**
  (le code a changé ; ce qui est prouvé est l'égalité entre racines, pas la
  stabilité entre versions) et le rappel que la CI garde son propre digest.
- Le SBOM (`46b65d9f…`) et l'installateur Windows (`bf55aab9…`, deux
  désinstallations idempotentes) du même rejeu sont référencés.
- Aucun build local pendant la campagne nominale (R12).

## Entrée 2026-11-14 (round 29) — `LINUX-QUALIFICATION.md` pointe le rejeu courant

| Champ | Valeur |
|---|---|
| Fichier | `docs/qualification/LINUX-QUALIFICATION.md` |
| Ajout | §12 : run [43562818570](https://github.com/Rwanbt/Ravum/actions/runs/43562818570), SHA `878e212` |
| Nature | documentation d'un run déjà exécuté (round 19) ; aucun build local |

- Le statut global du document pointait encore le run `c67f17e`
  (2026-11-11), antérieur à `OwnFacts`, `traps.rs` et au garde de places. Il
  pointe désormais le rejeu `878e212`, tout en conservant les runs antérieurs
  comme historique (§10/§11).
- §12 détaille les 7 jobs verts (workspace 1 601, conformance 474 PASS / 2 SKIP,
  smoke 31, Miri 65+134, fuzz, installateur, wasm32), les **deux SKIP nommés**
  (Windows-only) et les limites (une image, un SHA, pas de TSan, Miri borné).





































### Clôture 2026-11-13 — CI de la PR (SHA `303da6a`)

Les **8 workflows exécutés sont verts** sur le SHA de la PR : documentation
integrity, validation gate contracts, VS Code integration, coverage ratchet,
qualification evidence Windows (conformance, repro, SBOM, provenance,
installateur), macOS Tier 2, MVP product checks, MVP impact-selected checks.
Un run `impact` du SHA précédent a été **annulé par le push** (supersession, pas
échec). Le présent commit de journal est **docs-only** et ne change pas le code
mesuré. Aucun correctif CI n'a été nécessaire.









---

### Seconde vague de consolidation — 2026-11-14 (branche `goal/consolidation-2`)

Base : `dev` à `49cdc83` (PR #109 et #110 fusionnées). Branche créée depuis
`dev` ; les tâches T1-T4, T9 et T10 ont été menées, T5-T8 ne l'ont pas été.
Détail complet : `docs/reports/GOAL-CONSOLIDATION-2-REPORT.md`.

| Tâche | Statut | Commits | Résultat mesuré |
|---|---|---|---|
| T1 — affirmations | `TESTED` | `383cf5a` | R2 `PARTIAL` ; chaque « 40 200 » porte « 42 gabarits indépendants, sans composition » ; `report_consistency.py` PASS |
| T2 — vrai générateur | `PARTIAL` | `40f1737`, `1121308`, `b7e6c7a` | 120 programmes : 110/110 acceptés, 10/10 fautifs refusés, 0 divergence, 53 règles, profondeur 0,96 ; **2 divergences réelles trouvées et corrigées** ; test de puissance **NOT_RUN** (revert en conflit) |
| T3 — corpus à l'échelle | `PARTIAL` | `7f645d9` | 1 025 / 5 110 / 20 110 / 50 110 lignes ; `check` p50 34 ms → 26 424 ms ; `own` 97,5 % à 50 k ; `rustc` 5,6 % ; incrémental jamais plus rapide ; RSS 151/236 Mo ; LSP à l'échelle `NOT_RUN` |
| T4 — plafond de fichiers | `PARTIAL` | `c61cb92` | `NodeId` u64 32/32 ; `MAX_SOURCE_FILES=65 536` ; paquet de 1 000 modules accepté ; refus testé ; Rust généré identique après normalisation des noms dérivés d'ids (15/232 fichiers différents littéralement) |
| T5 — `FileId` stable LSP | `NOT_RUN` | — | — |
| T6 — `rt` par point d'entrée | `NOT_RUN` | — | — |
| T7 — garde OwnFacts | `NOT_RUN` | — | — |
| T8 — mutants survivants | `NOT_RUN` | — | — |
| T9 — source unique des chiffres | `PARTIAL` | `77c3c0f` | 7 contrôles PASS, `evidence/current.json`, 19 déclarations d'état courant vérifiées (3 documents) |
| T10 — CI `goal/**` | `TESTED` (config) | `1da5544` | déclencheur ajouté ; run CI non observé sur cette branche |

Fin de session : `cargo test --workspace --all-targets -- --test-threads=2` →
**101 suites, 1 610 passés / 0 échec / 6 ignorés**, exit 0 ; conformance
**478 cas : 477 PASS / 0 FAIL / 1 SKIP**, `perimeter=206/206`, `verified=123`.
Aucun cas, test ni seuil supprimé. Aucune fusion, aucun tag, aucun push.

### Suite du 2026-11-14 — T5 exécutée (branche `goal/consolidation-2`)

T5 (`FileId` stable en session LSP) a été menée **après** la rédaction du
rapport : commit `e3823cb`, tests positifs et négatifs (3 fichiers inchangés +
4e module déclaré avant ⇒ 3 hits de cache et `NodeId` inchangés ; même édition
sans table ⇒ 0 hit), refus fail-closed d'une table incohérente. Conformance et
workspace rejoués : **101 groupes, 1 618 passés / 0 échec / 6 ignorés**, exit 0 ;
conformance inchangée (**478 / 477 / 0 / 1**, `perimeter=206/206`,
`verified=123`). L'entrée ci-dessus garde ses chiffres de fin de session ; le
détail T5 est dans `GOAL-CONSOLIDATION-2-REPORT.md` §5.

### Suite du 2026-11-14 — T6 exécutée (branche `goal/consolidation-2`)

T6 (profil `rt` par point d'entrée + indexation bornée) : commit `986f542`,
RFC `RFC-RT-TRAPS.md` amendée (§2 et §7). `--rt-entry` sur `check` (avec
`--profile rt`) et `build` ; atteignabilité sur le graphe d'appels ; deux formes
d'indexation bornée (`0..Kusize` avec `K <=` taille du tableau, `0..a.len()`
sur tableau/tranche non réaffecté) ; entrée inconnue refusée (`E-TOOL`).
`examples/dsp-gain-iir` vérifie en `rt` avec une sortie identique octet pour
octet (smoke 43/0/0). Conformance **483 / 482 / 0 / 1**, `perimeter=207/207`,
`verified=124` ; workspace **101 groupes, 1 624 passés / 0 échec / 6 ignorés** ;
`evidence/current.json` rafraîchi (7 contrôles PASS). Règle :
`PENDING_HUMAN_APPROVAL`.

### Suite du 2026-11-14 — T7 exécutée (branche `goal/consolidation-2`)

T7 (garde OwnFacts étendu), un commit par exclusion : `ad62f32` (récepteurs de
méthode), `7f7f431` (arguments de référence), `0d04106` (identité de liaison —
`Resolution::Local` porte le span de déclaration, même identité qu'`OwnFacts`).
Le garde de l'étape B ne saute plus ces catégories, il les vérifie ; trois tests
négatifs injectent un fait faux et attendent `CodegenError`. Générateur T2 avec
le garde actif : **120 programmes, 0 divergence, 0 désaccord**, 110/110 acceptés,
53 règles. Workspace **101 groupes, 1 627 passés / 0 échec / 6 ignorés** ;
conformance inchangée (**483 / 482 / 0 / 1**, `perimeter=207/207`,
`verified=124`) ; `evidence/current.json` rafraîchi (7 contrôles PASS).
Garde debug-only ; campagne rapide seulement.

### Suite du 2026-11-14 — T8 documentée (branche `goal/consolidation-2`)

T8 (mutants survivants) : la liste des 24 et leurs justifications existent déjà
(`MUTATION-EQUIVALENCE.md` §9.1–§9.2, 24/24, familles E1–E6) et ont été
**revérifiées à HEAD `0d04106`** (sites et conditions présents ; turbofish
toujours refusé, `type_is_copy` inchangé, `pattern_contains_float` à quatre
bras). Rejeu de la campagne **`NOT_RUN`** : `cargo mutants --version` → exit 101
(outil absent ; installation = `RFC-DEV-TOOLS-1` `PENDING_HUMAN_APPROVAL`),
workflow `mutation.yml` non déclenchable sans push. Dernier rejeu CI connu :
2026-11-13, 183 mutants / 150 tués / 24 survivants / 9 non viables, identique.
Statut T8 : `PARTIAL`.
