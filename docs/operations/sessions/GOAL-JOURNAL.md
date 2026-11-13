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


