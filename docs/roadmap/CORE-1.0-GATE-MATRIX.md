# Ravum Core 1.0 — registre unique des gates T0–T12

> **Registre de suivi, pas un certificat.** Un seul tableau fait foi pour l'état
> des gates. Aucun `PASS` n'y est écrit sans preuve exécutée, liée à un SHA et
> reproductible par la commande citée.
>
> États autorisés : `PASS`, `PARTIAL`, `FAIL`, `BLOCKED`, `NOT_RUN`.
> `SKIP` et `NOT_RUN` ne sont **jamais** convertis en `PASS`. Un `BLOCKED`
> n'est jamais converti automatiquement en `PASS`.
>
> Dernière mise à jour : 2026-11-13 (passe 11 « cas de conformance FFI » + lot 1
> couverture : huit cas ajoutés pour EFF-4.7 (`rt` + FFI), EFF-2.9 (propagation
> transitive), `char`/`&i32` refusés, `bool` admis, `i64` traversant réellement,
> `extern` dans `impl` et pointeur brut dans `unsafe` ; puis déblayage du code
> mort et fermeture de `facts.rs`/`builtins.rs` en couverture ; **aucun
> changement du compilateur** — T2 reste `PARTIAL` pour la raison indépendante
> D10, D7 reste `OPEN`).
>
> **Chiffres courants (passe 11 + lot 1, `dev`, mesurés le 2026-11-13)** :
> **471 cas : 470 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** (conformance
> `--verify-build`, `build_verified: true`, `perimeter 202`) ;
> **113 suites, 1574 passés / 0 échec / 2 ignorés** (workspace, `cargo test --workspace`) ;
> **188/188** règles tracées, **188/188** satisfaites ;
> **108** codes enregistrés, **126** émis ; couverture **89,08 %** lignes /
> **88,39 %** régions sur **`ravum-lang-sema` + `ravum-lang-codegen`** (mesure
> Windows 11 x64 ; **89,12 / 88,44** sur le runner Linux de `coverage.yml`), et
> **86,41 %** du seul code produit (1 715 lignes non couvertes hors modules
> `#[cfg(test)]`), seuils ratchet par fichier tenus, cinq relevés
> (`facts.rs` 100/100, `builtins.rs` 100/98, `typeck.rs` 93/92,
> `effects.rs` 92/94, `resolve.rs` 93/93 ; **aucun abaissé**).
> Ces chiffres sont ceux que `tools/report_consistency.py` confronte au rapport
> consolidé.
>
> **Sections datées = archives.** Les sections `§0 bis` (passe 6), `§0 ter`
> (passe 7), `§0 quater` (passe 8), `§0 quinquies` (passe 9), `§0 sexies`
> (passe 10) et `§1.x` portent les chiffres **de leur passe**, pas les chiffres
> courants : deux relecteurs externes ont pris ces tableaux pour l'état du HEAD.
> Les chiffres qui font foi sont ceux du bloc ci-dessus, et le tableau de la §2.

## 0. Base de référence

| Élément | Valeur |
|---|---|
| Branche de travail | `feat/lang-core-1.0-rc` |
| Baseline au début de la session | `16d4e5c475a613b4b0142d2cc08c635ee63ed42c` |
| **SHA des preuves de cette passe** | **`c6b4537`** (code abaissement des places de tableau `0f73738` ; sondage round-3 `aac9743` ; tests inter-OS `c67f17e`) |
| SHA des preuves CI antérieures | `c43dafd` / `01fc148` — **4 workflows verts** (état antérieur, ne couvre pas cette passe) |
| **CI sur ce code** | **12 runs verts sur 12** sur `c6b4537` (commit final, 6 workflows × `push` + `pull_request`) : Linux qualification [43031394324](https://github.com/Rwanbt/Ravum/actions/runs/43031394324), MVP product checks Windows [43031394660](https://github.com/Rwanbt/Ravum/actions/runs/43031394660), gate contracts [43031394278](https://github.com/Rwanbt/Ravum/actions/runs/43031394278), coverage [43031394299](https://github.com/Rwanbt/Ravum/actions/runs/43031394299), impact [43031394377](https://github.com/Rwanbt/Ravum/actions/runs/43031394377), VS Code [43031394291](https://github.com/Rwanbt/Ravum/actions/runs/43031394291) ; même verdict sur `c67f17e` (runs `43026622427`/`43026622466`) |
| `dev` fusionné dans la branche | `fbaa5068af39d5dfbcb27af4aaf378ff37ec2e7b` (PR #103, licence) |
| `main` (intacte) | `ca675497d1f8d74cbb7a93d12715a845149a8785` |
| PR du langage | [#101](https://github.com/Rwanbt/Ravum/pull/101) (draft, base `dev`) |
| Licence | `MIT OR Apache-2.0`, 23/23 membres du workspace |

## 0 bis. Passe 6 — fiabilité P0/P1/P2 (2026-11-11)

| Élément | Valeur |
|---|---|
| Base de cette passe | `5d88ecbdb1839287aecd13fd6ba914b911b75f5b` |
| Commits de code | `8c6434b` (sema/codegen), `5daeb40` (contrat de conformance), `e0af6b6` (CLI), `9314d2f` (build), `9c163de` (P2), `a3a43b2` (P1) |
| Binaires épinglés | `ravum-lang.exe` sha256 `5816ACDDCE0A81A768F9CE5620CCE0C786B342058BA8D7398D77725C157F661D` ; `ravum-conformance.exe` sha256 `7DF6345C68284731E3D72BFF44B31639BE88EC4325C4548535ED88AA4BE4CCCF` |
| Tests workspace | **94 suites, 1525 passés / 0 échec / 2 ignorés** (exit 0) — `cargo test --workspace --all-targets` |
| Conformance + build | **424 cas : 423 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** ; `build: perimeter=163 covered=163 verified=101 implicit=62 failed=0 environment=0 not_measured=0` ; `build_verified: true` — `ravum-conformance run --verify-build` |
| Smoke exemples | **43 PASS / 0 FAIL / 0 NOT_RUN** (dont 13 assertions `file-store`) — `scripts/lang-smoke.ps1 -SkipBuild` |
| Watch fail-closed | **PASS** — `scripts/watch-fail-closed-check.ps1` : l'ancienne version survit à une édition invalide, la nouvelle s'active, un seul enfant, aucun orphelin, `.ravum/watch/<pid>/` supprimé |
| Registre des codes | **108 enregistrés / 126 émis / 0 hors registre / 0 jamais émis / 0 W hors DIAG-1.6** — `python tools/registry_audit.py` (exit 0) |
| Traçabilité FeatureId | **188/188** (187 cas + 1 exemption `F-META-006`) — `python tools/features_coverage.py --strict` (inchangé) |
| **Traçabilité MUST** | **807 énoncés MUST ; 463/807 atteignables par un FeatureId ; 344/807 non atteints** — `python tools/must_audit.py` (nouvelle métrique, seuil inchangé) |
| Contrats Python | **33 tests OK** — `python -m unittest discover -s tools/tests` |
| Docs / cohérence | `validate_repo_docs.py` PASS (42 ADR proposés) ; `report_consistency.py` PASS ; `json_contract.py` PASS |
| **CI sur ces commits** | **NOT_RUN** : aucun push n'a été effectué dans cette passe. Le job `linux-qual` exécute désormais `--verify-build` et une étape bloquante qui exige `build_verified: true`, mais aucun run n'a encore été observé sur ces SHA. Windows reste la plateforme primaire et n'a **aucun** job CI. |
| Portée | Aucune ratification d'ADR, aucune release, aucun tag, aucun merge. `main` intacte. |

Cette passe corrige **onze programmes acceptés par `check` mais refusés par
`rustc`** (familles : références stockées/retournées, moves après `unwrap`,
tableaux annotés, exhaustivité imbriquée, moves d'éléments de tableau,
`String → &str` implicite) et rend le contrat `check → build` **mesuré et
bloquant** sur le corpus accepté. Trois formes restent ouvertes et déclarées
(chaîne d'index imbriquée, tableau derrière un champ/tuple, liaison masquée par
un shadowing imbriqué) : elles échouent avec un E0508 bruyant, ne sont pas dans
le corpus et sont documentées dans `CONFORMANCE-BUILD-CONTRACT.md`. Détail :
`docs/qualification/T2-FINDINGS.md` §21.

## 0 ter. Passe 7 — CI réelle, invariants inter-OS, places de tableau (2026-11-11)

| Élément | Valeur |
|---|---|
| Base de cette passe | `9da9b12` (passe 6, 7 commits locaux non poussés) |
| Commits | `ab51b17` (rustfmt), `10459b8`/`f10a12f`/`e84e070`/`c67f17e` (contrats de test inter-OS), `aac9743` (sondage round-3 + 19 cas), `0efe84d` (D13), `5f1cb12` (message sema), `0f73738` (abaissement des places de tableau), `c6b4537` (commentaire codegen) |
| Binaires épinglés | `ravum-lang.exe` sha256 `5DDF8E5F602D6B657C3644BBE22584B67A655799188E5511491B8202A705C499` ; `ravum-conformance.exe` sha256 `9F21CFF01629458776E4F66ED0E0587E3A7166F184F197FC5F2014100600DF58` (Windows 11 x64, `rustc 1.95.0`) |
| Tests workspace | **113 suites, 1530 passés / 0 échec / 2 ignorés** (exit 0) — `cargo test --workspace -- --test-threads=2` |
| Conformance + build | **448 cas : 447 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** ; `build: perimeter=187 covered=187 verified=120 implicit=67 failed=0 environment=0 not_measured=0` ; `build_verified: true` — `ravum-conformance run --verify-build` |
| Sondage round-3 | **47 programmes × 11 axes : 47 check-pass, 47 build-pass, 0 divergence** — `scripts/p0a-probe-round3.ps1` (exit 0 ; il sortait 1 avec 4 divergences avant cette passe) |
| Smoke exemples | **43 PASS / 0 FAIL / 0 NOT_RUN** — `scripts/lang-smoke.ps1 -SkipBuild` |
| Watch fail-closed | **PASS** — `scripts/watch-fail-closed-check.ps1` |
| Registre des codes | **126 émis / 0 hors registre / 0 jamais émis / 0 W hors DIAG-1.6** — `python tools/registry_audit.py` |
| Traçabilité FeatureId | **188/188** — `python tools/features_coverage.py --strict` |
| **Traçabilité MUST** | **807 énoncés ; 463 atteints (57,4 %) / 344 non atteints (42,6 %)** — `python tools/must_audit.py` (métrique inchangée, seuil inchangé) |
| Contrats Python | **33 tests OK** — `python -m unittest discover -s tools/tests` |
| **CI sur ces commits** | **12/12 verts sur `c6b4537`** (commit final de la passe) : Linux qualification [43031394324](https://github.com/Rwanbt/Ravum/actions/runs/43031394324) (7 jobs verts + TSan `NOT_APPLICABLE`), MVP product checks Windows [43031394660](https://github.com/Rwanbt/Ravum/actions/runs/43031394660), gate contracts [43031394278](https://github.com/Rwanbt/Ravum/actions/runs/43031394278), coverage [43031394299](https://github.com/Rwanbt/Ravum/actions/runs/43031394299), impact [43031394377](https://github.com/Rwanbt/Ravum/actions/runs/43031394377), VS Code [43031394291](https://github.com/Rwanbt/Ravum/actions/runs/43031394291) ; verdict identique sur `c67f17e` (`43026622427`, `43026622466`) |
| Portée | Aucune ratification d'ADR, aucun merge, aucun tag, aucune release, aucun changement de protection de branche. `main` intacte. |

Ce que cette passe a réellement changé, et ce qu'elle ne prouve pas :

* **La CI n'avait jamais tourné sur ces commits.** Elle a trouvé trois défauts
  d'honnêteté que le local Windows ne pouvait pas voir : absence de `cargo fmt`
  (job bloquant), contrat d'`exit code` écrit depuis l'observation Windows seule
  (POSIX tronque à 8 bits avant le parent), et une course dans le test de build
  interrompu (`try_wait` puis `kill`). Les trois sont corrigés ; Linux est vert.
* **Cinq formes `check`-pass / `build`-fail sont fermées** : `a[0][1]` (chaîne
  d'index imbriquée), `h.items[0]` / `t.0[0]` (projection), shadowing de liaison,
  récepteur de méthode `self` par valeur sur élément (`a[0].take()`, famille
  trouvée par le sondage round-3), et payload de tableau dans un bras de `match`.
  Chacune a un test qui compile réellement avec `rustc` et exécute le binaire.
* **Quatre formes restent ouvertes, bruyantes (E0508) et documentées** : index
  dynamique dans un tableau imbriqué (`a[0][i]`), racine de chaîne qui n'est pas
  une place (`make()[0]`), plusieurs accès dans un bras de `match` en corps
  d'expression, et récepteur `by value` d'une méthode **builtin**
  (`a[0].unwrap()`). Elles ne sont pas dans le corpus et `P0-A` reste donc
  `PARTIAL`, pas `PASS`.
* **La traçabilité MUST n'a pas bougé** : 463/807. T2 reste `PARTIAL`. Aucun
  seuil n'a été modifié pour obtenir du vert.


## 0 quater. Passe 8 — formes restantes, preuves continues, véracité documentaire (2026-11-11)

| Élément | Valeur |
|---|---|
| Base de cette passe | `7e0c77a` (passe 7 poussée, CI 12/12 verte) |
| Commits de code | `5268838` (quatre formes d'abaissement), `d693162` (deux formes de descente partagée) |
| Commits de doc | `566ca7d`, `a82cf08`, `2037cbc`, `e48a461`, `66c8a78`, `afba4a0`, `a77333f` (audit de véracité documentaire) |
| Commits de CI | `c06a882` (nouveau workflow de preuves), `7696e2a` et `abe7d88` (deux défauts de ce workflow trouvés **par la CI elle-même**) |
| Tests workspace | **113 suites, 1536 passés / 0 échec / 2 ignorés** (exit 0) — `cargo test --workspace` |
| Conformance + build | **453 cas : 452 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** ; `build: perimeter=192 covered=192 verified=120 implicit=72 failed=0 environment=0 not_measured=0` ; `build_verified: true` — `ravum-conformance run --verify-build` |
| **Couverture re-mesurée** | **88,94 % lignes / 88,46 % régions** sur sema (`11923/13406` lignes) ; `types.rs` 100 %, `own.rs` 91,41 %, `check.rs` 80,26 % (seuil 80), `typeck.rs` 92,74 % (régions 92,23 %), `resolve.rs` 93,13 % (régions 93,36 %) — **tous les seuils déclarés tenus, aucun seuil modifié** |
| **Build bit-à-bit** | **deux binaires identiques**, `sha256 = 847DF0D11C1E2D00ABD049E8035E371D531D86DF90C6EA30184880759335CB96` (`scripts/repro-check.ps1`, deux racines et deux `target/`) |
| SBOM | SPDX 2.3 **déterministe** (deux runs, un digest, horodatage fixe) ; 64 paquets / 170 relations ; le digest intègre le commit dans `documentNamespace`, il change donc à chaque commit par construction |
| Installateur Windows | contrat exécuté en CI : installation, `--version`, champs du manifeste (`locked`, `git_commit`, `sha256`, `rustc`), désinstallation deux fois (idempotente), réinstallation `-SkipBuild` — **16/16** |
| Provenance | `emit` + `verify` + **refus d'un binaire altéré** (exit 1, motif vérifié) ; **fail-closed** si le binaire ou le SBOM manque (défaut corrigé cette passe) |
| Sondage round-3 | **47 programmes × 11 axes : 47 build-pass, 0 divergence** (inchangé) |
| Traçabilité | FeatureId **188/188** ; MUST **807 / 463 atteignables (57,4 %)** — métriques et seuils inchangés |
| Registre | **126 émis / 0 hors registre / 0 jamais émis** |
| **CI sur ce code** | **14/14 verts sur `abe7d88`** (7 `push` + 7 `pull_request`) : Linux qualification [43064500949](https://github.com/Rwanbt/Ravum/actions/runs/43064500949), MVP product checks Windows [43064500628](https://github.com/Rwanbt/Ravum/actions/runs/43064500628), **qualification evidence Windows** [43064500916](https://github.com/Rwanbt/Ravum/actions/runs/43064500916) / PR [43064513146](https://github.com/Rwanbt/Ravum/actions/runs/43064513146) — le nouveau workflow, avec conformance 453 cas, repro, SBOM, provenance et installateur —, gate contracts `43064501003`, coverage `43064501032`, impact `43064500697`, VS Code `43064500883`, documentation integrity `43064513246` |
| Portée | Aucune ratification d'ADR, aucun merge, aucun tag, aucune release, aucun changement de protection de branche. `main` intacte. |

Ce que cette passe a réellement changé, et ce qu'elle ne prouve pas :

* **Six formes `check`-pass / `build`-fail sont fermées**, dont les quatre laissées
  ouvertes par la passe 7 (index dynamique imbriqué, racine non-place, plusieurs
  accès dans un bras de `match` en corps d'expression, récepteur builtin *by
  value*) et les deux trouvées par la revue de cette passe (plusieurs descentes
  de projection sur des éléments différents, base relue après la descente).
  Chacune a un test `rustc`-vérifié (compilation + exécution + valeur) et un cas
  de corpus `conformance/cases/run/array-move-*`. Le corpus passe de 448 à 453
  cas et de `perimeter 187` à `192`.
* **Deux formes restent ouvertes, bruyantes (E0508) et documentées** : descentes
  dans deux branches distinctes (`if`/`else`) et deux descentes dans un même bloc
  imbriqué. `P0-A` reste donc **`PARTIAL`** à l'issue de cette passe, jamais
  `PASS`, et aucun test ne verrouille le défaut. Voir
  `docs/qualification/CONFORMANCE-BUILD-CONTRACT.md` §9 ter pour les programmes
  minimaux et le mécanisme exact. **Ces deux formes sont fermées par la passe 9**
  (`21cf7d4`, §0 quinquies) : cette phrase décrit l'état au commit `7696e2a`, pas
  l'état courant.
* **Les preuves lourdes sont devenues continues.** Un nouveau workflow
  `qualification-evidence.yml` exécute sur `windows-latest` la conformance
  `check ⇒ build`, la reproductibilité bit-à-bit, le SBOM, la provenance et le
  contrat d'installateur. C'était le trou principal du dossier : ces cinq
  preuves n'existaient que dans des logs locaux recopiés à la main dans des
  tableaux, donc elles ne pouvaient pas vieillir honnêtement. Windows n'avait
  **aucun** job de conformance.
* **La CI a trouvé deux défauts dans ce workflow, pas dans le langage** : une
  étape qui prouve un refus (`exit 1`) héritait de ce code de sortie et
  apparaissait rouge avec 8/8 assertions vertes ; et un `$code:` dans un message
  est une référence de variable à portée, donc un bloc entier n'était **jamais
  analysé**. Les deux sont corrigés ; le second example vaut d'être retenu : un
  job qu'on n'a jamais lancé est une affirmation, pas une preuve.
* **La couverture a été re-mesurée** après les changements de sema de la passe 7 :
  88,94 % / 88,46 % contre 89,2 % / 88,6 % publiés, `typeck.rs` 92,74 % contre
  93,8 %. La baisse est **rapportée, pas cachée** ; elle vient de code ajouté
  depuis, les seuils par fichier restent tenus et **aucun seuil n'a été
  abaissé**. La marge la plus mince est `check.rs` (80,26 % pour un seuil à 80).
* **Véracité documentaire** : sept commits corrigent une quarantaine
  d'affirmations fausses dans `spec/` et `docs/` (LSP « UNSUPPORTED », WASM,
  `--emit-ir`, FFI/`unsafe`, licence « à décider », signature « non
  implémentée », repro « non revendiquée », `E-TYP-RETURN`, `E-MOD-BOTH-ROOTS`,
  codes de sortie, inventaires de tests). Le motif commun : un document écrit à
  une passe antérieure et jamais revu quand le code avançait.
* **Ce que cette passe ne prouve pas** : pas d'audit externe (D4), pas de
  signature (D9), pas de release, pas de reproductibilité inter-machines (deux
  builds du même runner), pas de fermeture des deux formes ci-dessus, et la
  campagne de mutation n'a pas été rejouée.



### Passe 5 — mesures courantes

| Mesure | Commande | Résultat exact |
|---|---|---|
| Tests workspace | `cargo test --workspace --all-targets` | **89 suites, 1405 passés / 0 échec / 2 ignorés** (exit 0) |
| Conformance | `cargo run -p ravum-conformance -- run` | **386 cas : 385 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** (exit 0) |
| Contrats typechecker | `cargo test -p ravum-lang-sema --test typeck_contracts --test typeck_contracts2 --test typeck_contracts3` | **107 + 65 + 25 PASS / 0 FAIL** |
| Contrats résolution | `cargo test -p ravum-lang-sema --test resolve_contracts --test resolve_contracts2` | **112 + 31 PASS / 0 FAIL** |
| Audit borrow checker | `cargo test -p ravum-lang-sema --test own_audit` | **94 PASS / 0 FAIL** (dont la preuve que `E-OWN-LIMIT` est atteignable) |
| **Couverture (llvm-cov)** | `cargo llvm-cov -p ravum-lang-sema --json` puis `python tools/coverage_report.py --check` | **89,2 % lignes / 88,6 % régions** sur sema ; `types.rs` 100 %, `own.rs` 91,1 %, `typeck.rs` **93,8 %** (régions 93,0 %), `resolve.rs` **93,1 %** (régions 93,4 %) — **tous les seuils déclarés sont tenus**, `coverage.yml` bloquant |
| **Mutation (3 fichiers, ciblée)** | `cargo mutants --file … -F … --no-shuffle -j 8` | **195 mutants : 166 tués, 20 survivants triés (14 inatteignables par construction, 6 équivalents, 0 lacune de test), 9 invivables, 0 timeout** — 72,2 % → **85,1 %** de tués sur le périmètre ciblé ; voir `MUTATION-EQUIVALENCE.md` §8 |
| **Registre des codes** | `python tools/registry_audit.py` | **108 codes enregistrés ; 126 émis ; 0 hors registre ; 0 déclaré-jamais-émis** (exit 0) |
| Traçabilité des règles | `python tools/features_coverage.py --strict` | **188/188** règles tracées et satisfaites, 1 exemption machine-lisible (revue humaine non faite) |
| **Build bit-à-bit** | `pwsh -File scripts/repro-check.ps1` | **deux binaires identiques**, `sha256 = 83372F4F…D0546DD` (racines et `target/` différents) |
| SBOM | `$env:SOURCE_DATE_EPOCH='0'; pwsh -File scripts/sbom.ps1` | 64 paquets, 170 relations, `sha256 = dc7b4e0d…c2134f1` |
| Smoke exemples | `pwsh -File scripts/lang-smoke.ps1` | **31 PASS / 0 FAIL / 0 NOT_RUN** |
| Installateur | `pwsh -File scripts/install.ps1 -Prefix <tmp>` | manifeste de provenance, `sha256 = 28ba4b1c…f427dce680`, `locked = true` |
| Cohérence des documents | `python tools/report_consistency.py` | PASS — 6 contrôles, chiffres identiques |

Cette passe **traite les quatorze points de `T2-FINDINGS.md` §19.2** : les
défauts de confidentialité du résolveur, la famille complète des affectations
composées (D-CMP-1), le repli `use` de MOD-3.2, les codes erronés du typage,
l'inférence `Vec::new()`/`push`, les codes inatteignables, le harnais `W-*` et
les phases JSON. Détail : `T2-FINDINGS.md` §20. Aucun test existant n'a été
supprimé ; deux cas de corpus ont été mis à jour (code de `test fn`, cas
`unsupported/compound-assign` devenu sans objet).

### Passe 4 — mesures (SHA `67a2a2e`, historique)

| Mesure | Commande | Résultat exact |
|---|---|---|
| Tests workspace | `cargo test --workspace --all-targets` | **86 suites, 1320 passés / 0 échec / 2 ignorés** (exit 0) |
| Conformance | `cargo run -p ravum-conformance -- run` | **384 cas : 383 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** (exit 0) |
| Contrats typechecker | `cargo test -p ravum-lang-sema --test typeck_contracts --test typeck_contracts2` | **107 + 65 PASS / 0 FAIL** |
| Contrats résolution | `cargo test -p ravum-lang-sema --test resolve_contracts` | **112 PASS / 0 FAIL** |
| Audit borrow checker | `cargo test -p ravum-lang-sema --test own_audit` | **93 PASS / 0 FAIL** |
| **Couverture (llvm-cov)** | `cargo llvm-cov -p ravum-lang-sema --json` puis `python tools/coverage_report.py --check` | **88,9 % lignes / 88,3 % régions** sur sema ; `types.rs` 100 %, `own.rs` 90,9 %, `typeck.rs` **93,4 %** (régions 92,3 %), `resolve.rs` **93,0 %** (régions 93,5 %) — **objectifs 90/85 atteints sur les deux fichiers du gate**, et **seuils bloquants en CI** (`coverage.yml`, `tools/coverage_thresholds.json`) |
| **Mutation (3 fichiers, ciblée)** | `cargo mutants --file … -F … --no-shuffle -j 8` | **212 mutants : 153 tués, 50 survivants triés (14 inatteignables par construction, 4 observables seulement via le codegen, 1 équivalent, 31 lacunes de test), 9 invivables** — voir `MUTATION-EQUIVALENCE.md` §7 |
| **Registre des codes** | `python tools/registry_audit.py` | **111 codes enregistrés ; 129 émis ; 0 hors registre ; 0 déclaré-jamais-émis** (exit 0) |
| Traçabilité des règles | `python tools/features_coverage.py --strict` | **188/188** règles tracées et satisfaites, 1 exemption machine-lisible (revue humaine non faite) |
| **Build bit-à-bit** | `pwsh -File scripts/repro-check.ps1` | **deux binaires identiques**, `sha256 = 67F2934B…A2FA7D3` (racines et `target/` différents) |
| SBOM | `='0'; pwsh -File scripts/sbom.ps1` | 64 paquets, 170 relations, `sha256 = b637b03e…b24acd6` |
| Smoke exemples | `pwsh -File scripts/lang-smoke.ps1` | **31 PASS / 0 FAIL / 0 NOT_RUN** |
| Cohérence des documents | `python tools/report_consistency.py` | PASS — 6 contrôles, chiffres identiques |

Cette passe corrige **sept défauts mesurés** (dont deux de soundness : `None` nu
en motif, `None(payload)`), applique la décision **D-PAT-1** (G-6.36/G-6.37
implémentées, `E-UNSUPPORTED-NEG-PAT` retiré) et rend la couverture **bloquante
en CI**. Détail : `T2-FINDINGS.md` §18-19.
### Passe 5 — CI observée sur `fb2a0f5`

| Workflow | Run | Résultat exact |
|---|---|---|
| Linux qualification | [42925805059](https://github.com/Rwanbt/Ravum/actions/runs/42925805059) | **succès** : workspace **89 suites / 1402 passés / 0 échec / 4 ignorés** ; conformance **386 cas / 384 PASS / 0 FAIL / 2 SKIP** ; Miri, smoke **31/0/0**, stdlib **64/64**, CLI **13/13**, installateur et WASI verts ; fuzz smoke vert ; TSan `NOT_APPLICABLE` (aucun chemin concurrent) |
| Coverage ratchet | 42925804947 (push), 42925813721 (PR) | succès — seuils tenus (`typeck.rs` 93,8 %, `resolve.rs` 93,1 %, sema 89,2 %) |
| Gate contracts | 42925804881 (push), 42925813460 (PR) | succès |
| Documentation integrity | 42925813445 | succès |
| VS Code e2e | 42925805242 (push), 42925813541 (PR) | succès |
| MVP product / impact | 42925804854 et 42925813455 / 42925804853 et 42925813468 | succès |

Différence Windows/Linux assumée et documentée : Windows **89 suites / 1405 passés / 2 ignorés**,
conformance **385 PASS / 1 SKIP** ; Linux **89 suites / 1402 passés / 4 ignorés**,
conformance **384 PASS / 2 SKIP** — aucun échec de chaque côté, écarts liés à l'OS
(trois tests Windows-only deviennent ignorés sous Linux, dont deux cas FS).

Clôture `ea7df46` (correctif `const_expr` → `E-TYP-CONST` et assertion de
mutation) : mêmes workflows verts — Linux qualification
[42933086911](https://github.com/Rwanbt/Ravum/actions/runs/42933086911)
(workspace **89 suites / 1402 passés / 0 échec / 4 ignorés**, conformance
**386 cas / 384 PASS / 0 FAIL / 2 SKIP**, Miri, smoke, installateur, WASI, fuzz
smoke), coverage ratchet 42933086890 / 42933093863, gate contracts 42933086889 /
42933093852, documentation integrity 42933093820, VS Code e2e 42933086917 /
42933094041, MVP product 42933086896 / 42933093790, MVP impact 42933086891 /
42933093985.

### Passe 4 — CI observée sur `29ef8be` (historique)

| Workflow | Run | Résultat exact |
|---|---|---|
| Linux qualification | [42882234365](https://github.com/Rwanbt/Ravum/actions/runs/42882234365) | **succès** : workspace **86 suites / 1317 passés / 0 échec / 4 ignorés** ; conformance **384 cas / 382 PASS / 0 FAIL / 2 SKIP** ; smoke, installateur, WASI, fuzz smoke et **Miri** verts ; TSan `NOT_APPLICABLE` (aucun chemin concurrent) |
| **Coverage ratchet** (nouveau) | 42882234248 (push), 42882250559 (PR) | **succès**. Premier run sur `dea7f43` **rouge** pour une raison réelle : `tools/coverage_report.py` ne retirait que `/Ravum/` alors que llvm-cov rapporte `Ravum/crates/...` sur le runner, donc tous les seuils semblaient absents. Corrigé dans `29ef8be` (ancrage sur le premier répertoire racine connu) et verrouillé par 8 tests `tools/tests/test_coverage_report.py` |
| Gate contracts | 42882234269 (push), 42882250616 (PR) | succès |
| Documentation integrity | 42882250276 | succès |
| VS Code e2e | 42882234259 (push), 42882250425 (PR) | succès |
| MVP product checks | 42882234370 | succès |

Différence Windows/Linux assumée et documentée : Windows 1320 passés / 2 ignorés et
conformance 383 PASS / 1 SKIP ; Linux 1317 / 4 ignorés et 382 PASS / 2 SKIP (tests
dépendants de l'OS, aucun échec de chaque côté).


### Passe 3 — mesures antérieures (SHA `a6e71e7`)

| Mesure | Commande | Résultat exact |
|---|---|---|
| Tests workspace | `cargo test --workspace --all-targets -- --test-threads=2` | **84 suites, 1129 passés / 0 échec / 2 ignorés** (exit 0) |
| **VS Code réel — au repos** | `node tools/vscode-ravum/test/runTest.js` | **10/10 en 5 s** |
| **VS Code réel — sous charge** | idem, pendant `cargo test --workspace` concurrent | **10/10 en 4 s** ; harnais corrigé (timeout Mocha 300 s, attente 180 s, barrière de disponibilité LSP) |
| **CI VS Code** | workflow `vscode-e2e.yml` | **créé** (build CLI + `npm ci` + `xvfb-run` + suite réelle) ; premier run à confirmer |
| Conformance | `cargo run -p ravum-conformance -- run` | **346 cas : 345 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** (exit 0) |
| **Contrats typechecker** | `cargo test -p ravum-lang-sema --test typeck_contracts` | **95 PASS / 0 FAIL** |
| **Audit borrow checker** | `cargo test -p ravum-lang-sema --test own_audit` | **92 PASS / 0 FAIL** |
| **Couverture (llvm-cov)** | `cargo llvm-cov -p ravum-lang-sema --summary-only` | **79,4 % lignes / 79,7 % régions** sur sema ; `types.rs` 100 %, `own.rs` 90,1 %, `typeck.rs` 76,9 %, `resolve.rs` 67,4 % — **objectifs 90/85 non atteints** ; mesure locale non câblée en CI |
| **Mutation `own.rs`** | `cargo mutants ... --regex 'eval_loop\|eval_for\|eval_call'` | **22 mutants : 12 caught, 10 équivalents, 0 non expliqué** ; `eval_method_call` **4/4 caught** — voir [`MUTATION-EQUIVALENCE.md`](../qualification/MUTATION-EQUIVALENCE.md) |
| **Mutation `typeck.rs` (passe 1)** | `cargo mutants ... --file crates/ravum-lang-sema/src/typeck.rs --regex 'check_lit_pattern\|check_path_pattern\|cover_pattern\|check_array'` | **49 mutants : 38 tués, 7 survivants expliqués, 4 unviables** (dont 4 morts par construction tant que D-PAT-1 refuse les motifs négatifs) |
| **CI sur `a6e71e7` (docs `7e065d3`)** | runs `42862021582` (Linux), `42862021707` (VS Code push), `42862029061` (VS Code PR), `42862021037` (gate contracts) | **succès** : workspace Linux **89 suites / 1292 passés / 0 échec / 5 ignorés** ; conformance **346 cas / 344 PASS / 0 FAIL / 2 SKIP** ; Miri, smoke, installateur, WASI et fuzz smoke verts ; VS Code e2e **43–45 s** |

### Passe 2 — mesures antérieures (SHA `a5744ea`)

| Mesure | Commande | Résultat exact |
|---|---|---|
| Tests workspace | `cargo test --workspace --all-targets -- --test-threads=2` | **83 suites, 1029 passés / 0 échec / 2 ignorés** (exit 0) |
| LSP (bout en bout, tubes réels) | `cargo test -p ravum-lang-cli --test lsp_client` | **11 PASS / 0 FAIL** (dont l'oracle cold vs warm) |
| LSP (unitaire) | `cargo test -p ravum-lang-lsp` | 13 PASS / 0 FAIL (mesure antérieure, code inchangé) |
| **VS Code réel** | `node tools/vscode-ravum/test/runTest.js` | **10/10 observé** sur `63eda55`, mais **rejeu non stable** (10/10 puis 3/10, timeouts du harnais) — voir T7, désormais `PARTIAL` ; non rejoué sur `a5744ea` |
| Conformance | `cargo run -p ravum-conformance -- run --json target/p3-conformance.json` | **343 cas : 342 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** (exit 0) |
| Traçabilité des règles | `python tools/features_coverage.py --strict` | **188/188** règles tracées, **188/188 satisfaites** (187 par cas + 1 exemption structurellement vérifiée (revue humaine non faite)), exit 0 |
| **Contrats de validation** | `python -m unittest discover -s tools/tests -v` | **12 tests PASS** (vecteurs de faux PASS T2 et registre) |
| **Couverture (llvm-cov)** | `cargo llvm-cov -p ravum-lang-sema --tests --summary-only` | **75,0 % lignes / 75,3 % régions** sur sema ; `types.rs` 100 %, `own.rs` 89,4 %, `typeck.rs` 63,7 %, `resolve.rs` 67,2 % — **objectifs 90/85 non atteints** sur les composants critiques, mesure locale non câblée en CI |
| **Mutation testing (ciblé)** | `cargo mutants -p ravum-lang-sema -f crates/ravum-lang-sema/src/types.rs -j 4 --timeout 120` | **53 mutants : 46 caught, 7 unviable, 0 missed** — 100 % des mutants viables tués sur `types.rs` (tests ajoutés pour `is_*`, `int_bits`, `display`, `contains_never`) ; les fichiers critiques `typeck.rs`/`own.rs` ne sont pas encore couverts par une campagne complète |
| **Mutation testing `own.rs` (sous-ensemble)** | `cargo mutants ... --re 'eval_loop\|eval_for\|eval_call\|eval_method_call\|read_place\|mark_used'` | **27 mutants : 16 caught, 11 missed** ; 2 tests d'audit ajoutés (partial-move/disjoint-use, array-repeat après move). Les survivants restants sont des fixpoints de boucle et filtres de receiver, à instruire |
| Fuzz lexer/parser | `RAVUM_FUZZ_COLLECT=1 RAVUM_FUZZ_ITERS=200000 cargo test -p ravum-lang-syntax --test fuzz_lex_parse -- --ignored --nocapture` | **249 054 entrées, 0 échec**, digest `0x338b9afe9926aa4f`, 26,2 s |
| Fuzz sema | `RAVUM_FUZZ_ITERS=100000 cargo test -p ravum-lang-sema --test fuzz_check fuzz_check_long -- --ignored --nocapture` | **100 000 itérations, 0 échec**, 4 132 it/s, 24,2 s |
| Smoke exemples | `pwsh -File scripts/lang-smoke.ps1 -SkipBuild` | **31 PASS / 0 FAIL / 0 NOT_RUN** |
| SBOM | `$env:SOURCE_DATE_EPOCH='0'; pwsh -File scripts/sbom.ps1` | 64 paquets, 170 relations, `sha256 = 7c532967…93ee3` à époque fixée |
| **Registre des codes** | `python tools/registry_audit.py` | **111 codes enregistrés ; 129 émis ; 0 hors registre ; 0 déclaré-jamais-émis** (exit 0) |
| **Build bit-à-bit** | `pwsh -File scripts/repro-check.ps1` | **deux binaires identiques**, `sha256 = EC3133FB…4865` (racines et `target/` différents) |
| Formatage | `cargo fmt --all -- --check` | exit 0 |
| **Cohérence des documents** | `python tools/report_consistency.py` | le rapport consolidé porte **les mêmes chiffres que ce registre** (6 contrôles) |
| **Contrat JSON** | `python tools/json_contract.py` | **6 scénarios**, 0 violation, **1 tension documentée** (`doctor` expose des chemins absolus, interdits par DIAG-7.5) |
| **Audit des drapeaux CLI** | `pwsh -File scripts/cli-flag-audit.ps1` | **18 drapeaux documentés, 18 acceptés** |
| **Balayage CLI** | `pwsh -File scripts/cli-sweep.ps1` | **13 cas, 13 PASS / 0 FAIL** |
| **Balayage stdlib** | `pwsh -File scripts/stdlib-codegen-sweep.ps1` | **64 PASS / 0 FAIL** |
| **Miri (Windows)** | `cargo +nightly miri test -p ravum-lang-syntax -p ravum-lang-sema --lib --tests -- --skip loader::tests` | **93 + 2 PASS**, mais **`tests/ffi` NOT_RUN** : Miri Windows refuse `GetTempPathW`. Pas un défaut du programme ; **Miri complet a été rejoué en CI Linux sur `b7e5c5c`** (159 tests) |
| Clone neuf | `git clone --depth 1` puis toutes les commandes ci-dessus | mesure antérieure `1b7b4f1` ; **non rejouée** sur `a5744ea` |
| Performance | `pwsh -File scripts/lang-bench.ps1 -Runs 3 -Sizes 100,500,2000` (invoqué par taille) | `check` **45,1 / 89,4 / 331,2 ms** (pic **9,1 / 20,2 / 60,8 Mo**) ; `build` **329,9 / 647,1 / 2 151,7 ms** (pic **10,0 / 21,7 / 64,2 Mo**) pour 100 / 500 / 2 000 fonctions (Windows 11 x86-64, rustc 1.95.0, profil `dev`, 3 runs par taille) |
| **Outil de benchmark** | `scripts/lang-bench.ps1` | liste `-Sizes` validée (entiers 1..20 000, séparés par virgules) : l’ancienne conversion `[int[]]` transformait `100,500` en **100 500** et pouvait générer un programme géant ; corrigé et vérifié (liste combinée + rejet immédiat de `1005002000`) |

Les deux SKIP de conformance sont **par plateforme** : Windows saute
`security/dependency-absolute-unix`, Linux saute les deux cas spécifiques
Windows. Chaque cas est exécuté sur au moins un OS ; seuls trois SKIP sont liés à la plateforme (deux cas Windows sur Linux, un cas Unix sur Windows), aucun cas n'est sauté sur les deux.

## 0 quinquies. Passe 9 — les deux dernières divergences de move sont fermées (2026-11-11)

| Élément | Valeur |
|---|---|
| Base de cette passe | `000708f` (passe 8 poussée, CI 14/14 verte) |
| Commit de code, de test et de corpus | `21cf7d4` |
| Défaut corrigé | le planificateur ne groupait que les descentes qui étaient des instructions **directes** du bloc du binder : ni deux branches d'un `if`, ni un bloc imbriqué n'avaient d'ancre. L'ancre est maintenant la **plus ancienne instruction du bloc du binder** qui contient une descente, donc deux branches, un bloc imbriqué et des instructions sœurs partagent un seul déstructurage de niveau 0 |
| Mesure avant / après | les deux formes étaient `check` exit 0 / `build` exit 1 (`E0508`) à `000708f` ; elles construisent, s'exécutent et rendent la valeur attendue à `21cf7d4` |
| Tests workspace | **113 suites, 1537 passés / 0 échec / 2 ignorés** (exit 0) — `cargo test --workspace` |
| Conformance + build | **456 cas : 455 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** ; `build: perimeter=195 covered=195 verified=120 implicit=75 failed=0 environment=0 not_measured=0` ; `build_verified: true` — `ravum-conformance run --verify-build` |
| **Formes ouvertes connues** | **0** — aucune forme `check`-pass / `build`-fail connue, dans ou hors corpus |
| Couverture | **portée élargie et re-mesurée** : **88,54 % lignes / 87,86 % régions** sur `ravum-lang-sema` **+** `ravum-lang-codegen` (`emit.rs` 87,02/86,02, lib.rs 76,92/66,67, prelude.rs 100/100) ; trois seuils codegen ajoutés au ratchet, coverage.yml mesure désormais les deux crates ; **aucun seuil abaissé** (deux seuils sema relevés : own.rs 89/86 → 91/89, `facts.rs` 78/72 → 81/76) |
| CI | **14/14 verts sur `7665244`** (7 `push` + 7 `pull_request`) : Linux qualification [43087652725](https://github.com/Rwanbt/Ravum/actions/runs/43087652725), **qualification evidence Windows** [43087652812](https://github.com/Rwanbt/Ravum/actions/runs/43087652812) / PR [43087661007](https://github.com/Rwanbt/Ravum/actions/runs/43087661007) — conformance `cases=456`, `perimeter=195`, `verified=120`, `implicit=75`, `build_verified=True`, « 9 checks hold » —, MVP product checks [43087652789](https://github.com/Rwanbt/Ravum/actions/runs/43087652789), gate contracts [43087652704](https://github.com/Rwanbt/Ravum/actions/runs/43087652704), coverage [43087652635](https://github.com/Rwanbt/Ravum/actions/runs/43087652635), impact [43087652699](https://github.com/Rwanbt/Ravum/actions/runs/43087652699), VS Code [43087652655](https://github.com/Rwanbt/Ravum/actions/runs/43087652655), documentation integrity [43087660908](https://github.com/Rwanbt/Ravum/actions/runs/43087660908). Le code mesuré est `21cf7d4` ; `de6f0de` et `7665244` ne modifient que `docs/` |
| Livraison | **Fusion exécutée le 2026-11-12** (D3) : PR #101 → `dev` (`a2b8db5`), puis PR #104 → `main` (`94f3ac4`), commits de fusion sans squash et sans réécriture. **Aucun tag, aucune release, aucune signature** ; `main` est la référence documentaire, jamais une preuve de maturité. Rollback = `git revert -m 1 94f3ac4`. |

Ce que cette passe change, et ce qu'elle ne prouve pas :

* **Les deux formes fermées** sont exactement celles de la §9 ter du contrat :
  descentes de projection dans deux branches distinctes d'un `if`/`else`, et deux
  descentes dans un même bloc imbriqué. Elles passent `check` **et** `build`,
  avec deux cas de corpus (`run/array-move-branch-descents`,
  `run/array-move-nested-descents`) et un test qui compile réellement les deux
  formes avec `rustc` et vérifie que le déstructurage est émis **avant** le `if`.
* **Une régression a été trouvée par la suite complète, pas par la CI** : la
  première version du correctif groupait les descentes par instruction d'ancrage,
  ce qui séparait deux descentes en instructions **sœurs** d'un même bloc et
  rouvrait `E0508` sur `array-move-shared-descent`. Le test existant l'a attrapée
  immédiatement ; le regroupement se fait désormais par place, avec l'ancre la
  plus ancienne. C'est l'argument pour garder les tests `rustc`-vérifiés : le
  correctif « évident » cassait une forme déjà fermée.
* **Zéro forme connue ouverte ≠ aucune divergence.** Le corpus ne teste que ce
  qu'il contient. `P0-A` est donc vert **sur son périmètre mesuré** (456 cas,
  `perimeter 195` entièrement couvert, aucun échec, aucune forme connue ouverte),
  et l'invariant général reste une observation outillée, pas une preuve formelle.
* **Ce que cette passe ne prouve pas** : pas d'audit externe (D4, T4/T12 restent
  `BLOCKED`), pas de re-mesure de couverture, de reproductibilité ni de mutation
  (aucune n'est affectée par ce changement, aucune n'est revendiquée), et
  `T2` reste `PARTIAL` pour la raison indépendante du dénominateur MUST (D10).

## 0 sexies. Passe 10 — D13 : la staticité est prouvée, deux divergences backend fermées (2026-11-12)

| Élément | Valeur |
|---|---|
| Base de cette passe | `c6f2afc` (docs de la passe 9, code mesuré `21cf7d4`), branche `feat/lang-core-1.0-rc` |
| Commit de code, de test et de corpus | `2b31d63` ; documentation `8d8bf32` |
| Décisions du responsable | **D13** : « 2 - ok » → ni A (hypothèse) ni B (refus), **la preuve** (`docs/rfc/RFC-D13-STATICITY.md` §6). **`F-META-006`** : « 1 - ok » → exemption approuvée, date `2026-11-12`, réexamen au plus tard le `2027-02-12` |
| Défaut sémantique fermé | `return a[1]`, `let s = a[1]`, `c.text`, payload de `match`, `Option::unwrap`, `str::as_str` sur un texte statique et les **copies locales à travers une frontière de fonction** sont désormais **prouvés statiques** (règle d'origine + règle de stockage) et acceptés ; `String::as_str()` reste refusé |
| Divergence backend fermée (1) | `emit.rs` : un `&str` **imbriqué** dans un paramètre était émis `[&str; 2]` / `Option<&str>`. Dès que `return a[1]` est devenu accepté, `rustc` refusait la sortie (`E0310`, durée de vie) : forme `check`-pass / `build`-fail. Les types imbriqués sont maintenant `&'static str` ; seul le `&str` propre au paramètre reste élidé |
| Divergence backend fermée (2) | `str::as_str` n'avait aucun abaissement : le vérificateur l'acceptait, le backend répondait « unsupported builtin method ». Abaissé en identité |
| Tests | **12 tests d'audit** `own_audit.rs`, **10 tests unitaires** `own.rs`, **3 tests backend** `backend_audit.rs` (compilation et exécution réelles), **7 cas de conformance** (5 pass, 2 fail) |
| Tests workspace | **113 suites, 1562 passés / 0 échec / 2 ignorés** (exit 0) — `cargo test --workspace` |
| Conformance + build | **463 cas : 462 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** ; `build: perimeter=200 covered=200 verified=120 implicit=80 failed=0 environment=0 not_measured=0` ; `build_verified: true` |
| **Formes ouvertes connues** | **0** — aucune forme `check`-pass / `build`-fail connue, dans ou hors corpus ; la forme trouvée par cette passe est fermée et verrouillée par deux tests backend |
| Couverture | non re-mesurée dans cette passe : elle est mesurée par `coverage.yml` sur `sema + codegen` avec l'objectif 100 % (`tools/coverage_thresholds.json`) ; les lignes ajoutées sont exercées par 25 tests et 7 cas |
| **CI sur ce code** | **8/8 verts sur `8d8bf32` (`push`)** : Linux qualification [43309750021](https://github.com/Rwanbt/Ravum/actions/runs/43309750021) (workspace, conformance, Miri, smoke, installateur, WASI verts ; TSan `NOT_APPLICABLE`), MVP product checks [43309750384](https://github.com/Rwanbt/Ravum/actions/runs/43309750384) (build, tests, démos M1/COLLAB-1/M2/DSP/M4, **E2E navigateur réel en 19 s**), qualification evidence Windows [43309749722](https://github.com/Rwanbt/Ravum/actions/runs/43309749722) (conformance 463 cas, reproductibilité, SBOM, provenance, installateur), gate contracts [43309749969](https://github.com/Rwanbt/Ravum/actions/runs/43309749969), coverage [43309749806](https://github.com/Rwanbt/Ravum/actions/runs/43309749806) (ratchet tenu), impact [43309750069](https://github.com/Rwanbt/Ravum/actions/runs/43309750069), VS Code [43309750459](https://github.com/Rwanbt/Ravum/actions/runs/43309750459), macOS [43309750149](https://github.com/Rwanbt/Ravum/actions/runs/43309750149). Côté PR [#106](https://github.com/Rwanbt/Ravum/pull/106) : **7/8 verts** ; le doublon du job `mvp` (même SHA, même workflow) est resté **bloqué dans « Browser E2E » sur le runner** alors que le même pas a pris 19 s sur le push — flake E2E déjà documenté (T7), **pas un échec du code**. |
| Re-vérification locale (2026-11-12) | `cargo test --workspace` : **113 suites, 1562 passés / 0 échec / 2 ignorés** (exit 0) ; `ravum-conformance run --verify-build` : **463 cas, 462 PASS / 0 FAIL / 1 SKIP**, `perimeter=200 covered=200 verified=120 implicit=80 failed=0`, `build_verified: true` — même machine, `rustc 1.95.0`, code `2b31d63` |
| Livraison | **PR [#106](https://github.com/Rwanbt/Ravum/pull/106) ouverte en draft (base `dev`)** ; aucun tag, aucune release, aucune signature ; `main` inchangée |

Ce que cette passe change, et ce qu'elle ne prouve pas :

* **La preuve est locale** : elle établit I-STR (« tout `&str` qui sort d'une
  instruction est statique ») pour le sous-ensemble Core 1.0, pas la sûreté
  générale du langage. `T2` reste `PARTIAL` pour la raison indépendante du
  dénominateur MUST (D10) ; `T4`/`T12` restent `BLOCKED` (audit externe, Go/No-Go).
* **La forme `check`-pass / `build`-fail a été trouvée par le contrôle de build,
  pas par la suite de tests** : c'est l'argument pour garder `--verify-build`
  bloquant. Elle est fermée avant tout push et couverte par un test qui compare
  le texte généré.
* **Zéro forme connue ouverte ≠ aucune divergence** : le corpus ne teste que ce
  qu'il contient ; `P0-A` est vert **sur son périmètre mesuré** (463 cas,
  `perimeter 200` entièrement couvert).
* **Ce que cette passe ne prouve pas** : pas d'audit externe, pas de re-mesure
  de couverture, de mutation ni de performance ; la couverture sera mesurée par
  la CI sur le commit poussé.

## 0 septies. Passe 11 — huit cas de conformance FFI (2026-11-13)

| Élément | Valeur |
|---|---|
| Base de cette passe | `e6ca298` (fusion de la PR #106 dans `dev`), branche `feat/ffi-conformance-cases` |
| Cas ajoutés (**8**) | `effects/ffi-rt-profile` et `effects/ffi-transitive-wrapper` (`compile-fail`, `--profile rt` / `pure`, `E-EFF-PROFILE`) ; `types/ffi-char-rejected` et `types/ffi-reference-rejected` (`E-FFI-TYPE`) ; `types/ffi-bool-boundary` (`compile-pass`) ; `run/ffi-width-boundary` (`run-pass`, `llabs` en `i64`) ; `types/ffi-extern-in-impl` (`E-UNSUPPORTED-EXTERN`) ; `types/unsafe-does-not-unlock-pointers` (`E-UNSUPPORTED-UNSAFE`, `unsupported: true`) |
| Conformance + build | **471 cas : 470 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** ; `build: perimeter=202 covered=202 verified=121 implicit=81 failed=0 environment=0 not_measured=0` ; `build_verified: true` |
| Ce que ces cas ferment | **EFF-4.7** (`rt` + FFI) et **EFF-2.9** (propagation `unsafe`/`ffi` à travers deux niveaux de wrappers) n'avaient **aucun** test ; `char` et `&i32` refusés à la frontière ; `bool` admis et wrapper réellement compilé ; un `i64` traverse vraiment (`llabs(-9000000000) = 9000000000`) ; `extern` dans un `impl` refusé ; un pointeur brut reste refusé **à l'intérieur** d'un bloc `unsafe` |
| Écarts assumés vs RFC-D7 §5 | `types/ffi-extern-in-impl` attend le code **observé** `E-UNSUPPORTED-EXTERN`, pas `E-PARSE-EXPECTED` (le cas enregistre le comportement réel, il n'amende pas la spec) ; `bool` n'est que `compile-pass` et `usize`/`u64`/`isize` ne sont pas exercés à l'exécution, faute de symbole CRT portable — c'est écrit dans les `notes` des cas ; l'extension `doctor` n'est pas un cas de conformance |
| Portée | **aucun changement du compilateur, d'une règle ou d'un ADR** ; D7 reste `OPEN`, la FFI reste scalaire et expérimentale ; `T2` reste `PARTIAL` (dénominateur MUST, D10) |

Ce que cette passe ne prouve pas : elle outille des refus et deux chemins d'effets,
pas une sûreté FFI ; « 0 FAIL » vaut pour le corpus mesuré ; le cas `bool` ne prouve
pas l'ABI à l'exécution ; les callbacks, pointeurs, unwinding et ABI multi-plateforme
restent hors surface ([`docs/ffi/SAFETY.md`](../ffi/SAFETY.md) §6).

## 1 bis. CI sur le SHA `9ca29de` (Linux, `ubuntu-latest`) — **tous les jobs verts**

Run [42703719255](https://github.com/Rwanbt/Ravum/actions/runs/42703719255).

| Job | Résultat |
|---|---|
| Workspace tests (stable 1.95.0) | **succès** |
| Example smoke suite | **succès** |
| Installer contract | **succès** |
| `wasm32-wasip1` | **succès** |
| Fuzz smoke | **succès** |
| **Miri (nightly)** | **succès — 65 + 93 + 1 = 159 tests, 0 échec** |
| ThreadSanitizer | **SKIP explicite** `NOT_APPLICABLE` |
| **Conformance corpus (Linux)** | **SUCCÈS — `total=269 pass=267 fail=0 skip=2`** |

Les **30 mêmes cas** échouaient alors sur Linux et sur Windows : les divergences
spec ↔ implémentation ne sont pas spécifiques à une plateforme. Sept d'entre eux
ont depuis été corrigés dans le compilateur (mesure locale postérieure :
**268 PASS / 0 FAIL / 1 SKIP**) ; la CI de conformance reste rouge **volontairement** tant que le travail de cas de T2 n'est pas terminé. Les autres workflows (*Documentation integrity*, *MVP*,
*MVP impact-selected*) sont verts sur ce SHA.

## 1 ter. CI sur le SHA poussé `b7e5c5c` (code `a5744ea`, Linux) — **tous les jobs verts**

Runs [42809676443](https://github.com/Rwanbt/Ravum/actions/runs/42809676443)
(qualification Linux) et
[42809676594](https://github.com/Rwanbt/Ravum/actions/runs/42809676594)
(gate contracts). Le push couvre `a5744ea` (code) et `b7e5c5c` (docs) ; les deux
ont le même arbre de code.

| Job | Résultat exact |
|---|---|
| Workspace tests (stable 1.95.0) | **succès — 83 suites, 1019 passés / 0 échec / 4 ignorés** |
| **Conformance corpus (Linux)** | **succès — `total=340 pass=338 fail=0 skip=2 not_run=0`** (2 SKIP Windows) |
| **Miri (nightly, syntax + sema)** | **succès — 65 + 93 + 1 = 159 tests, 0 échec** (rejoué sur ce SHA) |
| Example smoke suite | **succès** |
| Installer contract | **succès** |
| `wasm32-wasip1` (build + run Node) | **succès** |
| Fuzz smoke | **succès** |
| ThreadSanitizer | **SKIP explicite** `NOT_APPLICABLE` |
| **Gate contracts (coverage/registry/tests)** | **succès** |

La CI ne prouve pas l'absence de bug : elle exécute le corpus et les contrôles
de gates sur un SHA précis, avec les limites listées au §3.

Sur le HEAD documentaire `043e36b` (même arbre de code que `63eda55`), les cinq
workflows de la branche sont également `success` : Documentation integrity,
Gate contracts, MVP, MVP impact-selected et Linux qualification
(run [42811025492](https://github.com/Rwanbt/Ravum/actions/runs/42811025492)).

## 1 quater. CI sur `1fb0568` (code `a5744ea`, Linux) — **tous les jobs verts après un re-run**

Runs [42815650401](https://github.com/Rwanbt/Ravum/actions/runs/42815650401)
(Linux) et [42815661183](https://github.com/Rwanbt/Ravum/actions/runs/42815661183)
(gate contracts).

| Job | Résultat exact |
|---|---|
| Workspace tests | **succès — 83 suites, 1019 passés / 0 échec / 4 ignorés** |
| Conformance corpus | **succès — `total=342 pass=340 fail=0 skip=2 not_run=0`** |
| Miri (nightly) | **succès — 65 + 93 + 1 = 159 tests, 0 échec** |
| Smoke, installateur, `wasm32-wasip1`, fuzz smoke | **succès** |
| Documentation integrity | **succès** |
| Gate contracts | **succès** |
| ThreadSanitizer | `NOT_APPLICABLE` |

**Flakiness identifiée (transparente)** : au premier passage, le job workspace a
échoué sur `crash_in_commit_msg_hook_publishes_nothing_and_restarts_cleanly`
(`ravum-workspace-coordinator`, assertion « the child should have been killed,
but it exited successfully »). Le re-run du seul job en échec est **vert**, sans
aucune modification de cette crate entre `b7e5c5c` (vert) et `1fb0568`
(`git diff` ne touche que sema/registry/conformance/docs). C'est un **test
flaky**, pas une régression démontrée ; il reste à instrumenter (fenêtre de
kill du harnais) et ne doit pas être compté comme stable.

**Correctif de fiabilité appliqué (2026-11-10)** : la fenêtre
« installation des hooks → spawn → attente de la mort de l'enfant » est
désormais sérialisée par un mutex, car les tests de crash partagent le helper
de kill et `target/ravum-tests`. Le diagnostic de l'assertion inclut aussi le
contenu du journal de hooks. Vérification locale : **5 exécutions consécutives,
15/15 tests vertes** (13–14 s chacune) ; la confirmation Linux reste à observer
en CI.

## 1 quinquies. CI sur `39ebea2` (fiabilité crash/bench, code `a5744ea`) — **vert**

Runs [42825171502](https://github.com/Rwanbt/Ravum/actions/runs/42825171502)
(Linux) et [42825184740](https://github.com/Rwanbt/Ravum/actions/runs/42825184740)
(gate contracts).

| Job | Résultat exact |
|---|---|
| Workspace tests (Linux) | **succès — 83 suites, 1019 passés / 0 échec / 4 ignorés**, dont `git_publication_crash` 15/15 |
| Conformance corpus | **succès — `total=342 pass=340 fail=0 skip=2 not_run=0`** |
| Miri (nightly) | **succès — 159 tests, 0 échec** |
| Gate contracts, smoke, installateur, WASI, fuzz smoke | **succès** |

Le flake `crash_in_commit_msg_hook...` ne s'est **pas reproduit** sur ce run
après sérialisation ; une seule exécution ne prouve pas l'absence du flake, mais
elle est cohérente avec 5/5 runs locaux verts.

## 1 sexies. CI sur `e05f3ff` (position de valeur des boucles, code `63619a5`) — **vert**

Runs [42837268985](https://github.com/Rwanbt/Ravum/actions/runs/42837268985)
(Linux) et [42837285517](https://github.com/Rwanbt/Ravum/actions/runs/42837285517)
(gate contracts).

| Job | Résultat exact |
|---|---|
| Workspace tests (Linux) | **succès — 83 suites, 1026 passés / 0 échec / 4 ignorés** |
| Conformance corpus | **succès — `total=343 pass=341 fail=0 skip=2 not_run=0`** |
| Miri (nightly) | **succès — 65 + 98 + 1 = 164 tests, 0 échec** |
| Smoke, installateur, WASI, fuzz smoke, gate contracts | **succès** |

Le nouveau cas `types/control-as-value-fail` passe sur les deux plateformes
(342 PASS / 1 SKIP sur Windows, 341 PASS / 2 SKIP sur Linux).

## 2. Matrice

| Gate | État | Ce qui est prouvé | Ce qui manque / échoue | Blocage |
|---|---|---|---|---|
| T0 | `PASS` | Corpus de sécurité du socle vert (overlays, publication Git, reprise après crash, journal, concurrence, symlinks, chemins hostiles) | — | — |
| T1 | `PARTIAL` | Voies A et B réelles (`check→build→run`), parité A/B mesurée | **Voie C non implémentée** ; pas de parité A/B/C ; choix final non ratifié | Décision humaine (ADR) |
| T2 | `PARTIAL` | **FeatureIds : 386 → 463 cas, 462 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** ; **188/188** FeatureIds tracés et satisfaits au critère strict (187 par cas + 1 exemption vérifiée) ; 108 codes enregistrés / 126 émis / 0 hors registre ; contrat `check → build` **mesuré et bloquant** (`--verify-build` : périmètre **200**, couverts 200, vérifiés 120, implicites 80, échoués 0), **aucune forme ouverte connue** (les familles trouvées pendant P0-A sont toutes fermées, `CONFORMANCE-BUILD-CONTRACT.md` §9-§11) et désormais **rejoué en CI sur Windows** (`qualification-evidence.yml`) | **La seconde métrique change le verdict** : 807 énoncés MUST, **463/807 seulement** atteignables par un FeatureId, **344** sans FeatureId (`must_audit.py`). Le critère `188/188` ne mesure donc pas la traçabilité des règles MUST ; l'exemption `F-META-006` est **approuvée** par le responsable (2026-11-12, réexamen 2027-02-12) mais reste sans oracle | Décision humaine sur le dénominateur MUST (D10) |
| T3 | `PASS` | Lexer/parser/AST réels, **1129 tests**, **fuzz complet rejoué (2 campagnes, 0 échec)**, **Miri Linux rejoué en CI sur `e05f3ff` (164 tests, 0 échec)** + **Miri Windows partiel 95 tests, tests FS `NOT_RUN`**, formateur idempotent, **8 défauts frontend corrigés** (`effects`, `pub test fn`, boucles-valeurs, cascade `lifetime`, place d'affectation, **type `!` nameable et divergent**, **tuples hétérogènes + exhaustivité tuple**, **boucles en position de valeur**) | — (les divergences de codes restantes sont traitées sous T2/D8 ; plus aucun code hors registre) | — |
| T4 | `BLOCKED` | Typechecker, borrow checker, 106 cas adversariaux internes | **Audit externe humain absent** | Audit externe |
| T5 | `PASS` | Binaires natifs réels Windows (smoke **43 PASS**) et Linux (CI antérieure `42809676443` sur `b7e5c5c`) ; WASI exécuté ; `build` publie une identité déterministe et un artefact atomique (`.ravum/build/<id16>/` → `.ravum/bin/<name>` + manifeste) | ABI stable non revendiquée ; sanitizers non exécutés ; CI **NOT_RUN** sur les commits de la passe 6 | — |
| T6 | `PARTIAL` | Stdlib core/io/fs/string/vec/math + FFI minimale | Threads, réseau, async explicitement `UNSUPPORTED` | — |
| T7 | `PASS` | CLI complète ; **LSP réel** (11 tests client) ; **harnais VS Code stabilisé** (timeouts 300 s, attente 180 s, barrière de disponibilité LSP) ; **10/10 au repos, 10/10 sous `cargo test --workspace` concurrent**, et **job CI `vscode-e2e` vert** sur `9972391` (push `42858553296`, 49 s ; PR `42858866200`, 1 m 04 s) avec build CLI + `xvfb` + VS Code réel | Risque résiduel : flakiness E2E ordinaire d'une suite Electron, sans échec observé depuis le correctif ; les plateformes Windows/macOS ne sont pas couvertes par ce job | — |
| T8 | `PARTIAL` | `watch` **fail-closed** : compilation avant arrêt de la version active, rejet des résultats obsolètes (digest de publication + re-scan), copie privée par processus, repli sur la dernière version valide, aucun orphelin (preuve `scripts/watch-fail-closed-check.ps1`) ; overlays/snapshots ; oracle cold vs warm (36 éditions) | **Pas de préservation d'état** ; pas d'ABI de patch ni de safepoint ; « continuité de service » n'est pas « hot reload » | Implémentation |
| T9 | `PARTIAL` | **10 verticales `.rv` exécutées**, dont `examples/file-store` (magasin clé/valeur versionné en Ravum pur : put/get/list/history/delete, erreurs typées, refus de traversée de chemin), avec 13 assertions de smoke et 3 cas de conformance | HTTP + SQLite, DSP hôte, E2E navigateur absents ; `file-store` n'est **pas** une base de données ni un service | Implémentation + décision D6/D7 |
| T10 | `PARTIAL` | **1574 tests** (113 suites, 2 ignorés), conformance **471 cas** avec **build vérifié bloquant** (`perimeter 202`, `verified 121`, `implicit 81`, `failed 0`), smoke **43/0/0**, contrats Python **33**, **Miri Linux vert en CI sur `c67f17e`**, **fuzz smoke vert en CI** ; couverture **sema + codegen** **89,08 % lignes / 88,39 % régions** (mesure du 2026-11-13, lot 1 ; objectif 100 % inscrit ; seuils ratchet par fichier tenus, cinq relevés, aucun abaissé ; codegen/emit.rs 87,00/86,02 ; `facts.rs` et `builtins.rs` à 100 % de lignes) ; **86,41 % du code produit mesuré** est couvert (1 715 lignes restantes sur 12 621, dont 1 145 dans `emit.rs` + `loader.rs`) ; mutation ciblée **rejouée en CI** (`mutation.yml`, run [43329512746](https://github.com/Rwanbt/Ravum/actions/runs/43329512746) : 183 mutants / 150 tués / 24 survivants triés équivalents / 9 non viables) ; **conformance Windows, reproductibilité, SBOM, provenance et installateur désormais rejoués par la CI** (`qualification-evidence.yml`) | **Audit externe absent** ; couverture **loin de 100 %** sur `emit.rs` (78,84 % du code produit) et `loader.rs` (69,71 %) ; fuzz **complet** non rejoué (seul le garde-fou `fuzz_*` tourne) ; **aucune variante de move ouverte** (fermées en passe 8-9) ; aucune mesure de performance au repos (machine chargée) | Audit externe |
| T11 | `PARTIAL` | Installateur **rejoué en CI** (`qualification-evidence.yml` : installation, `--version`, manifeste `locked=true`/`git_commit`/`sha256`/`rustc`, désinstallation idempotente, réinstallation `-SkipBuild` — **16/16**, binaire CI `6437db9d…ab6adf66`), SBOM **déterministe** (64 paquets / 170 relations, deux runs un digest, CI `63060489…4fa0dc8c`), provenance émise/vérifiée en CI avec **refus d'un binaire altéré** et fail-closed, licence `MIT OR Apache-2.0`, **build bit-à-bit identique entre deux racines et deux `target/`** (local `847DF0D1…59335CB96` ; CI `C281A035…588C2F70` — le digest est propre à l'environnement, c'est pourquoi la revendication reste « même toolchain, même linker »), **mécanisme de signature OpenSSH qualifié (accepte l'intact, rejette l'altéré)** ; procédure de custody **proposée** (`docs/release/KEY-CUSTODY.md`) | **Aucune clé de projet ni ancre de confiance** ; reproductibilité **inter-machines** non testée | Décision humaine **D9** (custody de clé) |
| T12 | `BLOCKED` | Dossier de RC interne, PR, rapports, matrice | Dogfooding indépendant, Go/No-Go humain | Décision humaine |

## 3. Détail par gate

### T0 — Fondations et sécurité du socle — `PASS`

Inchangé. Suites `overlay` (17), `git_publication` (16),
`git_publication_crash` (15), `journal_durability` (3), `cycle_integration`
(14), `merge_strategies` (2), `transient_verification` (3) vertes, plus le
scénario `publication → snapshot → même fichier → publication`.

### T1 — Architecture du compilateur et bootstrap — `PARTIAL`

Voie B réellement utilisée par `build`/`run`/`test` ; voie A opérationnelle ;
parité A/B antérieure `MATCH` sur 4 programmes. **La voie C (backend natif
direct) n'existe pas** : `NOT_IMPLEMENTED` n'est pas `PASS`, donc M5 ne peut pas
être vert. Décision humaine D2.

### T2 — Spécification formelle — `PARTIAL` (traçabilité FeatureId acquise, dénominateur MUST ouvert)

Le tableau de la §2 fait foi : `PARTIAL`, parce que le critère `188/188`
FeatureIds ne mesure pas les **807 énoncés MUST** (463 atteignables, 57,4 %) —
c'est la décision humaine **D10**, pas un défaut de tests. Le détail ci-dessous
reste l'historique de la passe de traçabilité.

Passe de traçabilité complète dans
[`docs/qualification/T2-FINDINGS.md`](../qualification/T2-FINDINGS.md).

- **Avant** : 99/188 règles citées par un cas, 107 cas, couverture non mesurée.
- **Après (2026-11-10)** : **346 cas**, **188/188** règles tracées vers un cas
  existant ou vers une exemption déclarée et structurellement vérifiée, **188/188 satisfaites** au critère
  strict (cas d'acceptation + cas de refus portant le code exigé) : **187 par
  cas + `F-META-006` exemptée**.
- **Aucun cas n'échoue** : les codes émis hors registre ont été alignés sur
  `diagnostics.md` §3 (« tout code absent est interdit »), les codes légitimes ont
  été **enregistrés** plutôt que remplacés par des codes plus génériques, et 3
  contradictions internes de la table ont été corrigées (F-SYN-021, F-SYN-024,
  F-STD-020) ainsi qu'une référence morte (F-LEX-001).
- **1 règle non testable, déclarée par la spécification elle-même** :
  `F-META-006` — colonne `Pass / Fail` = « aucun (oracle absent) », et MACH-1.4
  interdit de citer un résultat machine comme preuve. L'exemption n'est plus une
  prose : elle vit dans [`tools/coverage_exemptions.json`](../../tools/coverage_exemptions.json)
  avec raison, référence normative, approbateur, date et condition de réexamen ;
  `features_coverage.py --strict` échoue si elle est inconnue, mal formée,
  obsolète, ou si la règle n'est pas littéralement `NOT_COVERED`.
- **Régression verrouillée** : `tools/tests/test_validation_contracts.py`
  reproduit le vecteur de faux PASS (règle non couverte + exemption de
  complaisance) et exige l'échec ; la CI l'exécute.
- **Aucun cas n'a été assoupli** : `expect_codes` vient de la spec, jamais de la
  sortie du compilateur.

### T3 — Frontend — `PASS`

- **Prouvé** : lexer/parser/AST réels, récupération, garde-fous, formateur
  idempotent, corpus négatif, interfaces LSP effectives.
- **Fuzz complet rejoué sur le SHA de cette passe** : 249 054 entrées lexer/parser
  (**0 échec**, digest `0x338b9afe9926aa4f`, 26,2 s) et 100 000 itérations `sema`
  (**0 échec**, 4 132 it/s, 24,2 s). Le digest a changé par rapport à la campagne
  précédente, ce qui est attendu : `!` est désormais un type syntaxique et les
  tuples hétérogènes sont acceptés, donc l'ensemble des programmes acceptés
  change. L'empreinte reste donc liée au compilateur, pas une constante.
- **Trois défauts frontend reproduits puis corrigés** : `let effects = 1;` refusé
  alors que `effects` est contextuel (LEX-4.5), `pub test fn` accepté au lieu de
  `E-PARSE-EXPECTED`, et `while`/`loop`/`for` acceptés comme valeurs
  (`E-TYP-STATEMENT-VALUE`). Les cas correspondants sont passés au vert.
- **Corrigé en plus** : le token `lifetime` (`'a`) produisait une cascade de
  quatre diagnostics. Le lexer émet maintenant `E-UNSUPPORTED-TOKEN` et le
  parseur consomme le jeton (listes de génériques, derrière `&`) : un diagnostic
  par occurrence, l'item continue de se parser.
- **Corrigé en plus** : `(a+b).0 = 1;` était accepté par le parseur puis refusé
  par le checker (`E-TYP-FIELD`), alors que la spécification en fait une erreur
  de **parse**. Le parseur vérifie maintenant que la cible est une *place* (nom,
  champ, index, déréférence) et émet `E-PARSE-EXPECTED`.
- **Corrigé en plus — type `!` (G-5.11 / TYP-1.6)** : le parseur refusait `!`
  dans **toute** position, ce qui rendait impossible une fonction divergente
  `fn stop() -> !` et faisait passer le cas de refus `never-return-body` pour la
  mauvaise raison. `!` est désormais un type syntaxique ; il est accepté comme
  type de retour **entier** d'une fonction dont le corps diverge par `panic`,
  `return` ou un appel divergent — `loop`/`while`/`for` restent de type `()`
  (EXPR-5.11/6.7/G-6.19), donc un corps `loop {}` est refusé (`E-TYP-RETURN`). Il
  est refusé en paramètre, champ, argument générique, annotation locale et
  constante (`E-TYP-MISMATCH`), comme cible de `as` (`E-TYP-CAST`), et une
  inférence qui produirait `Option<!>`/`Vec<!>`/`(!, T)`/`W<!>` est
  `E-TYP-AMBIGUOUS` ; `for x in panic(…)` est `E-TYP-MISMATCH`. La phrase
  EXPR-11.3 « non nommable », contredite par G-5.11/TYP-1.6/Q-GRAM-10, a été
  alignée. **Douze cas** couvrent les deux directions, dont `never-return-run`
  qui **compile et exécute** réellement une fonction `-> !`.
  La première version de ce correctif typait `loop` sans `break` comme `!` :
  elle a été **retirée après contre-expertise indépendante** (trois MUST
  contredits, acceptation relâchée).
- **Corrigé en plus — tuples hétérogènes et exhaustivité** : le checker
  unifiait les éléments d'un tuple comme ceux d'un tableau, donc `(i32, bool)`
  était refusé (`E-TYP-MISMATCH`). Chaque élément est maintenant vérifié contre
  l'élément attendu correspondant. Un motif tuple irréfutable (`(a, b)`) couvre
  désormais tout le tuple, donc `match t { (a, b) => a }` n'est plus déclaré
  non exhaustif. Deux cas de conformance et une exécution réelle (`t.0` → `7`)
  le verrouillent.
- **Corrigé en plus — boucles en position de valeur** : EXPR-5.11/6.7/G-6.19
  interdisent `while`/`loop`/`for` comme expressions de valeur, mais seuls
  `let`, les éléments de tuple et les éléments de tableau étaient contrôlés. Un
  compteur de position de valeur couvre maintenant les arguments d'appel (dont
  `Some`/`Ok`/`Err`), l'affectation, les champs de struct, les bras et la
  scrutinee de `match`, et la valeur répétée d'un tableau →
  `E-TYP-STATEMENT-VALUE`. `fn main() { loop { } }` reste accepté. Cas
  `types/control-as-value-fail` (6 positions).
- **Pourquoi T3 est `PASS` (mesure du 2026-11-10)** : les 343 cas de conformance
  passent (342 PASS, 1 SKIP par plateforme, 0 FAIL) et **aucun code émis hors
  registre** ne subsiste. L'ancien écart résiduel — des identifiants de diagnostic
  différents sur 16 cas, dont 13 non enregistrés — est clos : les codes ont été
  alignés ou enregistrés, et le dernier code interdit (`W-TYP-UNREACHABLE`) a été
  retiré parce que EXPR-5.9/G-6.44 imposent qu'un bras inatteignable ne produise
  **aucun** diagnostic. Aucun programme n'est accepté à tort ni refusé à tort
  sur les cas mesurés.

### T4 — Sécurité mémoire, ownership, effets — `BLOCKED`

Inchangé : le contrat exige un **auditeur humain indépendant**. Un audit mené
par un autre agent reste un audit interne.

**Dossier prêt pour l'auditeur** : [`docs/audit/AUDIT-BRIEF.md`](../audit/AUDIT-BRIEF.md)
— périmètre exact, ce qui est déjà prouvé (et ne demande pas d'audit), **où
attaquer** d'après les défauts réellement trouvés pendant la session, les cinq
questions à trancher, et le format de rendu attendu. Complété par les deux audits
internes antérieurs (`docs/audit/BACKEND-AUDIT.md`,
`docs/audit/BORROW-CHECKER-AUDIT.md`), `conformance/cases/ownership/` et
`crates/ravum-lang-sema/tests/own_audit.rs`.

### T5 — Codegen, ABI, exécution native — `PASS`

31 PASS Windows sur `check/build/run` réels ; Linux qualifié par la CI
antérieure (workspace, conformance, smoke, installateur, WASI). Portée :
Windows x86-64 + Linux x86-64 sur le corpus et les exemples. Non revendiqué :
ABI stable, macOS, ARM, sanitizers.

### T6 — Runtime, stdlib, FFI — `PARTIAL`

`core/io/fs/string/vec/math`, FFI minimale (`extern "C"`, `unsafe`, types
primitifs, effets `ffi`/`unsafe`, refus hors `unsafe`). Threads, réseau, async,
temps réel dur explicitement `UNSUPPORTED` — conformément à la spec, donc pas un
périmètre réduit inventé. Les exigences T6 obligatoires non couvertes restent
absentes.

### T7 — Environnement de développement — `PASS` (harnais stabilisé et rejoué en CI)

Preuve historique : [`docs/qualification/VSCODE.md`](../qualification/VSCODE.md).

- Extension réelle `tools/vscode-ravum` (grammaire TextMate, serveur `ravum-lang
  lsp` en stdio), **10 tests d'intégration exécutés dans un VS Code réel**
  (1.145.2), **10 PASS / 0 FAIL** sur `1b7b4f1`.
- **Rejeu du 2026-11-10 sur `63eda55` non concluant** : une exécution donne
  **10/10**, une seconde **3/10** avec des timeouts (symboles, complétion,
  survol, définition, références) et l'erreur « `…/src/main.rv` is not part of
  the analysed package ». Le LSP en session directe répond correctement aux deux
  formes d'URI ; aucune régression fonctionnelle n'était démontrée, mais la
  preuve E2E n'était pas reproductible de façon stable.
- **Correction du harnais (2026-11-10, SHA `29cc79e`)** : le budget Mocha passe
  de 180 s à **300 s**, l'attente des fonctionnalités de 90 s à **180 s**, et
  une **barrière de disponibilité** interroge `executeDocumentSymbolProvider`
  (budget 240 s) avant le premier test : une machine chargée ne peut plus faire
  échouer un test fonctionnel sur la latence de démarrage du serveur.
- **Preuves après correction** : **10/10 en 5 s au repos**, puis **10/10 en
  4 s** pendant que `cargo test --workspace` tournait en parallèle — le scénario
  exact qui produisait 3/10.
- **Rejeu en CI (2026-11-10, SHA `9972391`, code `29cc79e`)** : le workflow
  `vscode-e2e.yml` (build CLI + `npm ci` + `xvfb-run` + VS Code réel + LSP réel)
  est **vert** sur le push (`42858553296`, 49 s) **et** sur la PR
  (`42858866200`, 1 m 04 s). C'est la preuve qui manquait : la suite s'exécute
  désormais dans un job CI, pas seulement sur la machine de l'auteur.
- **Risque résiduel** : flakiness E2E ordinaire d'une suite Electron sur un
  runner partagé, et aucune couverture Windows/macOS de ce job. Aucun échec
  observé depuis le correctif (2 runs locaux sous charge, 2 runs CI).

### T8 — Compilation continue et hot reload — `PARTIAL`

`watch` recompile et redémarre. **La préservation d'état n'existe pas** : un
redémarrage automatique ne satisfait pas une exigence de préservation d'état.
Le contrat d'invalidation demandé par
[`docs/specs/03-compilation-continue-hot-swap.md`](../specs/03-compilation-continue-hot-swap.md)
est en revanche mesuré : `warm_incremental_edits_match_a_cold_analysis` applique
36 éditions incrémentales à un buffer et vérifie qu'après chaque lot l'arbre de
symboles servi est **identique** à celui d'un serveur neuf analysant le même
texte, avec une assertion de non-vacuité (au moins un lot doit changer l'arbre).
Une régression d'invalidation devient donc un échec de test, pas une erreur
silencieuse dans l'éditeur.

### T9 — Applications réelles `.rv` — `PARTIAL`

9 verticales exécutées (`hello`, `fib`, `calculator`, `file-tool`, `stats`,
`dsp-gain-iir`, `package-demo`, `tests-demo`, `ffi-abs`). Absents : service HTTP
+ SQLite, DSP dans un hôte réel, E2E navigateur.

### T10 — Qualification intensive et audits — `PARTIAL`

1019 tests, 269 cas de conformance, 31 smoke, **fuzz complet rejoué après les
correctifs (0 échec sur les deux campagnes)**, et **Miri rejoué en CI** : 65 + 93 + 1 = **159 tests,
0 échec** (respectivement 225,8 s / 142,4 s / 18,5 s). TSan `NOT_APPLICABLE`
justifié (aucun thread/async exposé). **Audit externe absent** — c'est ce qui
empêche `PASS`.

### T11 — Performance, distribution, supply chain — `PARTIAL`

SBOM SPDX-2.3 reproductible (64 paquets, 170 relations, digest identique sur
deux runs à époque fixée — le digest dépend sinon de l'horodatage, ce qui est le
comportement SPDX attendu et non un défaut), checksums, manifeste de provenance,
licence réconciliée.

**Reproductibilité bit-à-bit : mesurée et verte** —
[`docs/qualification/REPRODUCIBILITY.md`](../qualification/REPRODUCIBILITY.md).
Deux builds depuis **deux racines différentes** et **deux répertoires de build
différents** produisent le même `sha256` (`AA6C1AF1…870B0F`, valeur liée à cet arbre) avec
`CARGO_INCREMENTAL=0`, `SOURCE_DATE_EPOCH=0`, `--remap-path-prefix` et
`-C link-arg=/Brepro`. Sans ces drapeaux, trois builds donnaient trois binaires
différents ; l'écart se limitait à 20 octets, tous des horodatages dans l'en-tête
PE écrits par `link.exe`.

**Signature : mécanisme qualifié, ancre absente** —
[`docs/qualification/SIGNATURES.md`](../qualification/SIGNATURES.md).
`scripts/attest-release.ps1` signe et vérifie des artefacts avec `ssh-keygen -Y
sign/verify` (Ed25519, namespace `ravum-release`) : un artefact intact est
accepté, un artefact modifié d'un octet est rejeté avec le code de sortie 1.
**Aucune clé de projet n'existe** : générer une clé laissée dans le dépôt
produirait une signature décorative. La custody est **D9**.

### T12 — Dogfood, audit final, RC — `BLOCKED`

Dossier de RC interne complet. Absents : dogfooding par des agents n'ayant pas
développé le compilateur, et Go/No-Go humain. Verdict plafonné à
`CORE_RC_CANDIDATE`, jamais `RELEASED`.

## 4. Décisions humaines restantes

| ID | Sujet | État |
|---|---|---|
| D1 | Licence | **RÉSOLUE** : `MIT OR Apache-2.0` |
| D2 | Ratification des ADR / choix du backend | **PARTIELLE** : cible **production ready** (pas MVP) actée par le responsable le 2026-11-11 ; les ADR restent `PROPOSED` (aucune ratification par un agent) |
| D3 | Fusion dans `main` et publication | **RÉSOLUE (2026-11-12)** : fusion sans release — PR #101 → `dev` (`a2b8db5`), PR #104 → `main` (`94f3ac4`), enregistrement `8a58357` ; **aucun tag, aucune release, aucune signature** ; rollback = `git revert -m 1 94f3ac4` |
| D4 | Audit externe (T4/T10/T12) | **OUVERTE** — bloque T4 et T12 ; cadre de mission prêt : [`AUDIT-BRIEF.md`](../audit/AUDIT-BRIEF.md) |
| D5 | Périmètre Tier 1 | **PARTIELLE** : Windows local + Linux CI ; **macOS CI Tier 2** ajouté et vert (2026-11-11, run 43132064512) |
| D6 | Périmètre des fonctionnalités universelles | **RÉSOLUE (2026-11-11, périmètre gelé)** : toute extension est une version 1.x, jamais une modification de 1.0 |
| D7 | Ratification de la frontière `unsafe`/FFI | **OUVERTE** — programme en 4 étages écrit (`PRODUCTION-READINESS.md` §5) ; rien n'est ratifié |

| **D8** | Arbitrage des codes de diagnostic | **RÉSOLUE** — le registre (`diagnostics.md` §3) déclare « tout code absent est interdit » ; les codes fautifs ont été remplacés. Le dernier restant, `W-TYP-UNREACHABLE`, n'a **pas** été enregistré : EXPR-5.9, G-6.44 et DIAG-1.6 imposent qu'un bras inatteignable ne produise aucun diagnostic, donc l'émission a été retirée et un test la verrouille |
| **D9** | **Custody de la clé de signature et publication de `allowed_signers`** | **REPORTÉE (2026-11-11)** : le responsable ne veut pas de clé tant que le langage n'est pas prêt pour la production. Procédure proposée : [`KEY-CUSTODY.md`](../release/KEY-CUSTODY.md) |
| **D-PAT-1** | Motifs littéraux flottants/négatifs (`grammar.md` G-6.36/G-6.37 vs `diagnostics.md`) | **RÉSOLUE (2026-11-10, option A)** : les deux règles MUST sont **implémentées**, `E-UNSUPPORTED-NEG-PAT` est retiré du registre (0 jamais émis). Preuves : `T2-FINDINGS.md` §18, 8 contrats `typeck_contracts.rs`, 5 cas de corpus dont 2 `run-pass` |
| **D-CMP-1** | Famille d'affectations composées (`grammar.md` G-6.8 / `expressions.md` EXPR-9.4 **MUST acceptées** vs `diagnostics.md` `E-UNSUPPORTED-COMPOUND-ASSIGN` et implémentation limitée à `+=`) | **RÉSOLUE (2026-11-10, passe 5)** : les **deux MUST** gagnent (Q-GRAM-3 disait déjà « famille complète acceptée, aucun `E-*` spécifique »). La famille est **implémentée** (sema + codegen `place op= e`), la ligne §7 de `grammar.md`, la ligne §12 de `expressions.md` et le code `E-UNSUPPORTED-COMPOUND-ASSIGN` sont retirés. Preuves : `T2-FINDINGS.md` §20, 9 contrats `typeck_contracts3.rs`, cas `run/compound-assign` **exécuté** (les 10 opérateurs) + `types/compound-assign-mismatch` |
| **D-UFCS-1** | UFCS : `unsupported.md` (`E-UNSUPPORTED-UFCS`) vs EXPR-7.8 (MUST : `E-RES-UNKNOWN`) | **RÉSOLUE (2026-11-10, passe 5)** : la règle MUST gagne ; le code dédié, jamais atteignable, est retiré du registre. `P::len(&p)` donne `E-RES-UNKNOWN`. Même précédent que `E-PARSE-BAD-SUFFIX` (cf. `diagnostics.md` Q-DIAG-8/Q-DIAG-11) |
| **D-MOD-3.2** | `use` relatif nu : repli module courant → racine (MUST) non implémenté ; double résolution jamais signalée | **RÉSOLUE (2026-11-10, passe 5)** : repli implémenté, double résolution → `E-RES-CONFLICT`. Preuves : 9 contrats `resolve_contracts2.rs` (dont non-faux-conflit et cas 1 segment) |
| **D-CONST-1** | `const_expr` interdit : G-5.11/G-5.13 (MUST, `E-TYP-CONST`) vs code émis `E-RES-CONST-NOT-CONST`, réservé par `diagnostics.md` §3.1 aux valeurs non représentables | **RÉSOLUE (2026-11-10, clôture de la passe 5)** : bloc, appel, variable, champ, index, `?` et référence à un autre `const` émettent `E-TYP-CONST` ; la constante flottante garde `E-RES-CONST-NOT-CONST`. Deux tests qui figeaient l'ancien code corrigés ; `mutation_gaps::const_block_is_refused_by_name` resserré. Preuve : `T2-FINDINGS.md` §20.5 |
| **D-COERCE-1** | Coercition implicite `String → &str` en position d'argument (`types.md` TYP-13.1 n'admet que `&String → &str`, TYP-8.5 rend la conversion explicite) | **RÉSOLUE par conformité (2026-11-11)** : TYP-8.5 (MUST) rend `String → &str` explicite et TYP-13.1 (MUST) n'admet que `&String → &str` ; l'implémentation acceptait le `String` possédé, donc `check` passait et `build` échouait (E0308). La coercition a été retirée ; `examples/stats` et `run/stats` migrent vers `.as_str()`. Aucune règle modifiée, aucun test supprimé. Texte normatif proposé : `docs/rfc/RFC-D-COERCE-1-MATRIX.md`. |

| **D10** | **Dénominateur de la traçabilité** : `188/188` FeatureIds vs **807 énoncés MUST**, dont **344** sans FeatureId (`tools/must_audit.py`) | **OUVERTE**. Elle bloque le `PASS` de T2. L'objectif **100 % de couverture** (D10-couverture) est acté séparément et inscrit dans `tools/coverage_thresholds.json`. Options : (a) redéfinir le critère T2 en deux métriques et exiger un plan de traçabilité des 344 MUST ; (b) déclarer les 344 MUST hors périmètre du gate avec justification écrite ; (c) tracer les 344. Aucune option prise par un agent. |
| **D11** | **Contradictions internes de spécification** : `effects.md` EFF-1.1 dit « 9 groupes » alors que son tableau et `facts.rs` en comptent **10** ; `features.md` FEAT-12.5 énumère **6** lignes `experimental` alors que **8** lignes du tableau le sont (`F-STD-031`, `F-META-006`) ; deux `rule_id` sont dupliqués (**G-5.11**, **G-6.40**) | **RÉSOLUE pour les comptes (2026-11-11)** : `effects.md` dit 10, `features.md` dit 8. Les collisions `G-5.11`/`G-6.40` sont **documentées** (Q-GRAM-11) mais **pas renumérotées** : un `rule_id` est un identifiant contractuel (Q-DIAG-4), la renumérotation reste une décision humaine. |
| **D12** | **Disposition des divergences `check → build` sur les moves de tableau** | **RÉSOLUE (option (a), 2026-11-11)** : abaissement général par place, sans amendement de règle. Les 11 formes trouvées sont toutes fermées — passe 7 (`0f73738`), passe 8 (`5268838`, `d693162`), passe 9 (`21cf7d4`) — et le corpus est passé de 443 à **463 cas** (périmètre build 182 → 200). Aucune forme ouverte connue ; `P0-A` vert sur son périmètre mesuré, jamais revendiqué comme preuve universelle |
| **D13** | **Provenance de `&'static str` à travers les agrégats** : `let s = a[1];` et `fn f(a: [&str; 2]) -> &str` étaient refusés (`E-OWN-REF-ESCAPE`) alors que TYP-6.3 (MUST) fait de `&str` une valeur `Copy`, stockable et retournable ; OWN-6.2 ne nommait que le littéral | **RÉSOLUE et IMPLÉMENTÉE (2026-11-12)** : ni A (hypothèse) ni B (refus) — **la preuve** (règle d'origine + règle de stockage, `docs/rfc/RFC-D13-STATICITY.md` §6). `return a[1]`, `let s = a[1]`, `c.text`, payload de `match`, `Option::unwrap`, `str::as_str` statique et les copies locales à travers un appel sont acceptés ; `String::as_str()` reste refusé. Preuves : 12 tests d'audit, 10 unitaires, 3 backend, 7 cas de conformance ; corpus **463 cas / 462 PASS / 0 FAIL**, `build_verified: true`. La passe a aussi fermé deux divergences backend (positions de type `&str` imbriquées, `str::as_str` sans abaissement) |
## 5. Règles de mise à jour

- Un changement d'état exige une commande, une sortie et un SHA.
- Un test ignoré ou non exécuté n'est jamais compté comme réussi.
- Un `SKIP` lié à l'OS est exécuté sur l'autre plateforme avant d'être coché.
- Aucun `PASS` obtenu en modifiant la définition du gate.
- **Une exemption de couverture n'est valide que si elle est machine-lisible,
  justifiée, normativement référencée et révisable** (`tools/coverage_exemptions.json`,
  validé par `features_coverage.py --strict` et la CI). Une phrase dans un rapport
  n'exempte rien.
- **Un gate peut régresser** : `T3` est passé de `PARTIAL` à `FAIL` parce que la
  passe T2 a produit des contre-exemples exécutables. Une information nouvelle
  qui dégrade un état est un progrès de mesure, pas une régression de qualité.

## 0 octies. Passe 12 — GOAL « prêt pour la revue de production » (2026-11-13)

> Section **datée** : elle ajoute des preuves mesurées dans la session du GOAL,
> sur la branche `goal/production-ready`. **Aucun état de gate n'est changé par
> cette section** : les preuves ci-dessous renforcent T2/T10/T11 mais ne ferment
> ni l'audit externe (T4/T12), ni D10, ni D9. Le détail et les limites sont dans
> [`GOAL-PRODUCTION-READINESS-REPORT.md`](../reports/GOAL-PRODUCTION-READINESS-REPORT.md).

| Mesure | Commande | Résultat exact |
|---|---|---|
| Workspace | `cargo test --workspace --all-targets -- --test-threads=2` | **100 groupes : 1 605 passés / 0 échec / 5 ignorés**, exit 0 |
| Workspace `--release` | `cargo test --workspace --all-targets --release -- --test-threads=2` | **100 groupes : 1 603 passés / 0 échec / 5 ignorés**, exit 0 ; 2 tests debug-only exclus (garde Copy/Move, garde de place) |
| `OwnFacts` (RFC-OWN-FACTS-1) | `cargo test -p ravum-lang-sema --test own_facts` + `cargo test -p ravum-lang-codegen --lib` | étape A **2/2** (move, copie, emprunt, move projeté `p.b`, déterminisme) ; étape B partielle : garde de cohérence, **600 programmes + 476 cas, 0 désaccord**, test négatif vert |
| Conformance + build | `cargo run -p ravum-conformance -- run --verify-build` | **476 cas : 475 PASS / 0 FAIL / 1 SKIP** ; `perimeter=203/203`, `verified=122`, `implicit=81`, `failed=0`, `build_verified: true` |
| Traps `rt` (RFC-RT-TRAPS-1) | `cargo test -p ravum-lang-sema --lib traps` + 5 cas `effects/rt-trap-*` | **10/10** tests unitaires (index, division, arithmétique binaire **et composée**, négation, décalages, agrégats/flux, flottants, formes explicites) ; 4 compile-fail `E-EFF-PROFILE`, 1 compile-pass ; `core`/`pure` inchangés |
| Smoke exemples | `pwsh -File scripts/lang-smoke.ps1 -SkipBuild` | **43 PASS / 0 FAIL / 0 NOT_RUN** |
| Registre | `python tools/registry_audit.py` | **108 enregistrés / 126 émis / 0 hors registre / 0 jamais émis** |
| Traçabilité | `python tools/features_coverage.py --strict` | **188/188**, 1 exemption |
| MUST | `python tools/must_audit.py` | **807 énoncés, 344 non mappés** — D10 ouverte, inchangée |
| Couverture | `cargo llvm-cov -p ravum-lang-sema -p ravum-lang-codegen --json --output-path target/cov-round16.json` puis `coverage_report.py --check` | **89,47 % lignes / 88,87 % régions**, **tous les seuils tenus**, 10 fichiers sous l'objectif 100 %, **2 126** lignes non couvertes (20 182 − 18 056) ; `codegen/lib.rs` **100/100**, `traps.rs` 98,66/96,84, `emit.rs` 87,75/87,09 ; limite des régions `fmt::Error` documentée — [`COVERAGE-100-OBJECTIVE.md`](../qualification/COVERAGE-100-OBJECTIVE.md) |
| Oracle différentiel | `cargo test -p ravum-lang-codegen --test differential_oracle` | générateur v2 **42 gabarits** : rapide **600** (0 divergence), long local **3 000** (0 divergence), **nominal 3 × 10 000 = 30 000 programmes locaux, 0 divergence, 8 602 s**, **3 × 1 000 rejoué après le garde de places : 3 000, 0 divergence** ; cumul **40 200 programmes, 0 divergence**, 100 % acceptés |
| Timings | `pwsh -File scripts/lang-timings.ps1 -Runs 20` | `check` p95 froid **1,3-8,1 ms**, édition **11,0-49,8 ms** ; `build` p50 **179-331 ms**, `rustc` **97,1-99,3 %** — [`LANG-TIMINGS.md`](../reports/LANG-TIMINGS.md) |
| Incrémental | A/B `RAVUM_INCREMENTAL=1` | gain **nul ou négatif** (jusqu'à +44 %) ; opt-in, **désactivé par défaut** |
| Cache de parse LSP | `cargo test -p ravum-lang-lsp --test parse_cache` et `cargo test -p ravum-lang-cli --test lsp_client` | 2 tests INV-07/réutilisation verts ; analyse p95 **1,362 ms** avec cache vs **1,836 ms** sans ; **bout en bout transport inclus p50 2,061 ms / p95 2,826 ms** ; client LSP **11/0** — [`LSP.md`](../qualification/LSP.md) §7 |
| `NodeId` | `cargo test -p ravum-lang-sema --test node_id_stability` | **2/2** : ids de `util.rv` inchangés après édition de `main.rv` — migration **sans objet** |
| Fuzz long | `RAVUM_FUZZ_ITERS=200000 … fuzz_lex_parse -- --ignored` et `RAVUM_FUZZ_ITERS=100000 … fuzz_check` | **249 054 entrées, 0 échec**, digest `0x9b09fc61e761b87e` ; **100 000 itérations, 0 échec**, 4 660/s ; **rejoué après traps `rt`** : 100 000, 0 échec, 5 275/s (≈ 33 000 en `rt`) |
| Reproductibilité | `pwsh -File scripts/repro-check.ps1` | **PASS**, deux racines et deux `target/` : sha256 `988521151EFE827EA62448343C08128370CA3801A3EAEF312C04B298C43FDE8A` (rejoué au round 13 sur `662f63d`) |
| SBOM | `SOURCE_DATE_EPOCH=0 pwsh -File scripts/sbom.ps1` | SPDX-2.3, **64 paquets (23 workspace + 41 dépendances) / 170 relations**, `created=1970-01-01`, sha256 `46b65d9fe941123e4ea7ac8fb82cfeb1e4ccb457d7cf29947ec5cd0251647721` |
| Installateur | `pwsh -File scripts/install.ps1 -Prefix target/install-round13 -SkipBuild` puis deux `-Uninstall` | install exit 0, manifeste commit `662f63d`, binaire sha256 `bf55aab9…`, rustc 1.95.0, `locked=true` ; désinstallations **0 / 0** (la seconde : « rien à désinstaller ») |
| LSP client | `cargo test -p ravum-lang-cli --test lsp_client` | **11 / 0** |
| Miri, mutation | — | **`NOT_RUN`** : raisons dans le rapport §6 (composant Miri absent, cargo-mutants absent) |

**Ce que cette passe ne change pas** : T4 et T12 restent `BLOCKED` (humains) ;
T2 reste `PARTIAL` (D10) ; T11 reste `PARTIAL` (D9) ; aucune ratification d'ADR,
aucun tag, aucune release. L'étape A d'`OwnFacts` est additive (aucun
consommateur) ; l'étape B est un garde-fou de debug ; l'étape C n'est pas faite.




























