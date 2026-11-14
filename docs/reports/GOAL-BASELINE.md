# GOAL « production ready » — baseline de référence (phase 0)

> **Document de mesure, pas un certificat.** Tous les chiffres ci-dessous ont été
> produits par les commandes citées, dans la session du 2026-11-13, sur cette
> machine, à ce SHA. Statuts autorisés : `TESTED`, `PARTIAL`, `NOT_RUN`,
> `UNKNOWN`, `BLOCKED`, `PENDING_HUMAN_APPROVAL`. Aucun `PROVED` n'est écrit.
> Ce fichier est la référence de non-régression des phases 1 à 9 : aucun chiffre
> ci-dessous ne doit diminuer.

## 1. Identités

| Élément | Valeur |
|---|---|
| Dépôt | `D:\App\Ravum` (`github.com/Rwanbt/Ravum`) |
| Branche de travail | `goal/production-ready` |
| Créée depuis | `test/coverage-batch-1` @ `c931fdfe346474b838ba1856f3a84bf6798bba87` |
| SHA de cette baseline | `c931fdfe346474b838ba1856f3a84bf6798bba87` |
| `dev` local | `38278e1222498f734155b1d9bc7d63bfb5f71e91` |
| `origin/main` (fetché) | `8a58357` (contient la fusion `94f3ac4`) |
| `main` local | `16b1ff2` — **périmé** par rapport à `origin/main` ; non utilisé comme base |
| Commit d'audit ajouté | `c931fdf docs(audit): add the independent 2026-11-13 architecture audit` |
| Date de session | 2026-11-13T17:05 +01:00 (Europe/Paris) |

**Pourquoi la branche part de `test/coverage-batch-1` et non de `dev`.** L'énoncé
demandait une branche depuis `dev`, avec repli documenté sur
`test/coverage-batch-1` si le lot couverture n'était pas dans `dev`. Le lot
couverture **est commité et poussé** (`60d4db1`, `1868286`, `170d37e`), mais
`dev` est à `38278e1` et ne contient pas ces trois commits : `git rev-list
--count dev..test/coverage-batch-1` vaut `3`. La branche de travail part donc de
`test/coverage-batch-1` @ `c931fdf` pour ne rien perdre. Ce choix est répété dans
le journal et devra figurer dans la description de la PR.

## 2. Environnement de référence

| Élément | Valeur |
|---|---|
| OS | Microsoft Windows 11 Professionnel, 10.0.26100 build 26100, x86-64 |
| CPU | 13th Gen Intel(R) Core(TM) i7-13620H, 10 cœurs / 16 threads logiques |
| RAM | 16 868 962 304 octets (≈ 15,7 Gio) |
| Rust | `rustc 1.95.0 (59807616e 2026-04-14)`, `cargo 1.95.0 (f2d3ce0bd 2026-03-21)`, épinglé par `rust-toolchain.toml` |
| PowerShell | 7.7.0 |
| Python | 3.13.7 |
| Git | 2.51.1.windows.1 |

## 3. Commandes et résultats bruts

Toutes les commandes ont été exécutées à `c931fdf`, dans l'ordre ci-dessous,
sans autre build lourd concurrent (R12). Les journaux complets sont dans
`target/goal-baseline-*.log` (non versionnés).

| # | Commande | Sortie / exit | Statut |
|---|---|---|---|
| 1 | `git status --short` | arbre propre après le commit d'audit | `TESTED` |
| 2 | `cargo test -p ravum-lang-sema -- --test-threads=2` | **683 passés / 0 échec**, 13 groupes `test result`, exit 0 | `TESTED` |
| 3 | `python -m unittest discover -s tools/tests` | **33 tests OK**, exit 0 | `TESTED` |
| 4 | `cargo fmt --all -- --check` | exit 0 | `TESTED` |
| 5 | `cargo test --workspace --all-targets -- --test-threads=2` | **94 groupes `test result` : 1574 passés / 0 échec / 2 ignorés**, exit 0 | `TESTED` |
| 6 | `cargo run -p ravum-conformance -- run --verify-build --json target/goal-baseline-conformance.json` | **471 cas : 470 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** ; `build: perimeter=202 covered=202 verified=121 implicit=81 failed=0` ; `build_verified: true` ; exit 0 | `TESTED` |
| 7 | `pwsh -NoProfile -File scripts/lang-smoke.ps1 -SkipBuild` | **43 PASS / 0 FAIL / 0 NOT_RUN**, exit 0 | `TESTED` |
| 8 | `python tools/validate_repo_docs.py` | `PASS`, 42 ADR proposés, exit 0 | `TESTED` |
| 9 | `python tools/report_consistency.py` | `PASS`, 6 chiffres contrôlés, exit 0 | `TESTED` |
| 10 | `python tools/registry_audit.py` | **108 codes enregistrés / 126 émis / 0 hors registre / 0 jamais émis / 0 W hors DIAG-1.6**, exit 0 | `TESTED` |
| 11 | `python tools/features_coverage.py --strict` | **188/188 règles satisfaites**, 1 exemption justifiée (`F-META-006`), exit 0 | `TESTED` |

Détail du run 5 : le journal contient 94 lignes `test result:` (une par binaire
de test) et 0 ligne `Doc-tests`. Les 2 tests ignorés sont les `#[ignore]` longs
documentés (fuzz long) ; ils ne comptent jamais comme réussis.

## 4. Écarts entre l'énoncé du GOAL et l'état observé

Ces écarts sont consignés tels quels : ils changent la lettre de l'énoncé, pas
son but.

1. **« 11 fichiers modifiés non commités » (phase 0.2) : état périmé.**
   L'arbre était propre ; le lot couverture est commité et poussé sur
   `test/coverage-batch-1` en trois commits (`60d4db1`, `1868286`, `170d37e`).
   Le seul élément non suivi était `docs/audit/2026-11-13-architecture-audit.md`,
   désormais commité (`c931fdf`). Aucun fichier n'a été jeté.
2. **« 113 suites » (documents) : non reproduit.** La mesure donne **94**
   groupes `test result` ; le nombre de tests passés (1574) et d'ignorés (2)
   correspond exactement aux documents. La méthode de comptage des « suites »
   utilisée par les rapports n'est pas reproduite par
   `cargo test --workspace --all-targets` sur ce SHA : `UNKNOWN`, à trancher en
   phase 1.3 (ne pas recopier « 113 suites » sans la commande qui le produit).
3. **Répertoire `C:` : le nom réel est `C` + U+F03A**, pas `C:` (un deux-points
   est interdit dans un nom de fichier Win32). Contenu : uniquement des
   répertoires vides `Users\barat\AppData\Local\Temp\ravum-sh-test-4425bb`,
   datés du 2026-11-09 17:26. Aucun écrivain `ravum-sh-test` dans l'arbre
   courant ni dans l'historique (`git grep` sur toutes les révisions) ; Git ne
   l'affiche pas parce qu'un arbre de répertoires vides est invisible pour Git.
   Traitement en phase 1.7 (inspection, suppression si prouvé régénérable, garde).
4. **`main` local périmé.** `main` local = `16b1ff2` (ancien commit d'outillage
   local), `origin/main` = `8a58357` (contient `94f3ac4`). Les documents qui
   citent `main` = `94f3ac4` parlent d'`origin/main`, pas de la branche locale.
   Aucune écriture n'est faite sur `main`.

## 5. Seuils de non-régression (phase 0 → phase 9)

| Mesure | Plancher de la session |
|---|---|
| `cargo test --workspace --all-targets` | ≥ 1574 passés, 0 échec ; les 2 ignorés restent identifiés |
| `ravum-lang-sema` | ≥ 683 passés, 0 échec |
| Conformance `--verify-build` | ≥ 471 cas, **0 FAIL**, 0 NOT_RUN ; `build_verified: true` ; `perimeter` couvert à 100 % |
| Smoke exemples | ≥ 43 PASS, 0 FAIL |
| Registre des codes | 108 enregistrés, 126 émis, 0 hors registre, 0 jamais émis |
| Traçabilité stricte | 188/188, 1 exemption justifiée |
| Contrats Python | ≥ 33 OK |
| `cargo fmt --all -- --check` | exit 0 |
| Docs | `validate_repo_docs.py` et `report_consistency.py` exit 0 |

Tout chiffre inférieur est une régression à traiter avant la phase suivante.

## 6. Ce que cette baseline ne prouve pas

- Elle ne prouve pas l'absence de divergence `check`/`build` hors du corpus :
  elle mesure un corpus fini de 471 cas (phase 2 pour l'oracle génératif).
- Elle ne prouve pas la reproductibilité inter-machines, la signature, l'audit
  externe ni le Go/No-Go : `NOT_RUN` / `BLOCKED` / `PENDING_HUMAN_APPROVAL`.
- Elle ne remplace pas la couverture, la mutation ni les campagnes fuzz longues,
  rejouées en phase 9 (ou marquées `NOT_RUN` avec raison).
- Les chiffres de couverture cités par les documents (89,08 % / 88,39 %) ne sont
  **pas** re-mesurés dans cette phase : `NOT_RUN` jusqu'à la phase 9.
