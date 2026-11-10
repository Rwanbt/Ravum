# Ravum Core 1.0 — registre unique des gates T0–T12

> **Registre de suivi, pas un certificat.** Un seul tableau fait foi pour l'état
> des gates. Aucun `PASS` n'y est écrit sans preuve exécutée, liée à un SHA et
> reproductible par la commande citée.
>
> États autorisés : `PASS`, `PARTIAL`, `FAIL`, `BLOCKED`, `NOT_RUN`.
> `SKIP` et `NOT_RUN` ne sont **jamais** convertis en `PASS`. Un `BLOCKED`
> n'est jamais converti automatiquement en `PASS`.
>
> Dernière mise à jour : 2026-11-10 (passe P0 de fiabilité des gates : exemption
> de couverture machine-vérifiée, registre des diagnostics bloquant, retrait de
> `W-TYP-UNREACHABLE`).

## 0. Base de référence

| Élément | Valeur |
|---|---|
| Branche de travail | `feat/lang-core-1.0-rc` |
| Baseline au début de la session | `16d4e5c475a613b4b0142d2cc08c635ee63ed42c` |
| **SHA des preuves de cette passe** | **`a5744ea`** (sema `!`/tuples ; durcissement gates `3ddf481` ; P0 `16775b4`) — 83 suites, conformance, fuzz, reproductibilité ; **fiabilité/tests `03774d6`/`e981f2f` ; position de valeur et audit borrow `63619a5`** (mutex crash + validation `-Sizes`) |
| SHA des preuves CI antérieures | `c43dafd` / `01fc148` — **4 workflows verts** (état antérieur, ne couvre pas cette passe) |
| **CI sur ce code** | **succès** sur `e05f3ff` (code `63619a5`) : runs [42837268985](https://github.com/Rwanbt/Ravum/actions/runs/42837268985) (Linux) et [42837285517](https://github.com/Rwanbt/Ravum/actions/runs/42837285517) (gate contracts) |
| `dev` fusionné dans la branche | `fbaa5068af39d5dfbcb27af4aaf378ff37ec2e7b` (PR #103, licence) |
| `main` (intacte) | `ca675497d1f8d74cbb7a93d12715a845149a8785` |
| PR du langage | [#101](https://github.com/Rwanbt/Ravum/pull/101) (draft, base `dev`) |
| Licence | `MIT OR Apache-2.0`, 23/23 membres du workspace |

## 1. Mesures (passe 4, SHA `67a2a2e`, 2026-11-10)

### Passe 4 — mesures courantes

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
| T2 | `PASS` | **384 cas : 383 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** ; **188/188** règles tracées, **188/188 satisfaites** au critère strict (187 par cas + 1 exemption justifiée) ; **111 codes enregistrés, 0 hors registre, 0 jamais émis** (`E-UNSUPPORTED-NEG-PAT` retiré par D-PAT-1) ; 3 contradictions internes corrigées ; **D-PAT-1 RÉSOLUE** : G-6.36/G-6.37 implémentées et prouvées par 2 cas `run-pass` (`NaN` ne correspond jamais, `-0.0` correspond à `0.0`) | **`F-META-006`** reste sans forme exécutable (pass : « aucun (oracle absent) » ; MACH-1.4 interdit de citer un résultat machine) — exemption **machine-lisible** dans `tools/coverage_exemptions.json`, structurellement vérifiée par l'outil (revue humaine non faite) et bloquante en CI | Exemption déclarée, structurellement vérifiée, révocable |
| T3 | `PASS` | Lexer/parser/AST réels, **1129 tests**, **fuzz complet rejoué (2 campagnes, 0 échec)**, **Miri Linux rejoué en CI sur `e05f3ff` (164 tests, 0 échec)** + **Miri Windows partiel 95 tests, tests FS `NOT_RUN`**, formateur idempotent, **8 défauts frontend corrigés** (`effects`, `pub test fn`, boucles-valeurs, cascade `lifetime`, place d'affectation, **type `!` nameable et divergent**, **tuples hétérogènes + exhaustivité tuple**, **boucles en position de valeur**) | — (les divergences de codes restantes sont traitées sous T2/D8 ; plus aucun code hors registre) | — |
| T4 | `BLOCKED` | Typechecker, borrow checker, 85 cas adversariaux internes | **Audit externe humain absent** | Audit externe |
| T5 | `PASS` | Binaires natifs réels Windows (smoke 31 PASS) et Linux (CI `42809676443` sur `b7e5c5c`) ; WASI exécuté | ABI stable non revendiquée ; sanitizers non exécutés | — |
| T6 | `PARTIAL` | Stdlib core/io/fs/string/vec/math + FFI minimale | Threads, réseau, async explicitement `UNSUPPORTED` | — |
| T7 | `PASS` | CLI complète ; **LSP réel** (11 tests client) ; **harnais VS Code stabilisé** (timeouts 300 s, attente 180 s, barrière de disponibilité LSP) ; **10/10 au repos, 10/10 sous `cargo test --workspace` concurrent**, et **job CI `vscode-e2e` vert** sur `9972391` (push `42858553296`, 49 s ; PR `42858866200`, 1 m 04 s) avec build CLI + `xvfb` + VS Code réel | Risque résiduel : flakiness E2E ordinaire d'une suite Electron, sans échec observé depuis le correctif ; les plateformes Windows/macOS ne sont pas couvertes par ce job | — |
| T8 | `PARTIAL` | `watch` = rebuild + redémarrage ; overlays/snapshots ; **oracle cold vs warm** : 36 éditions incrémentales comparées à une analyse froide, arbre de symboles identique à chaque lot | **Pas de préservation d'état** ; pas d'ABI de patch ni de safepoint | Implémentation |
| T9 | `PARTIAL` | 9 verticales `.rv` exécutées | HTTP + SQLite, DSP hôte, E2E navigateur absents | Implémentation |
| T10 | `PARTIAL` | **1320 tests** (86 suites), conformance **384 cas**, smoke, **fuzz complet rejoué (2 campagnes)**, **Miri Linux en CI sur `e05f3ff` (164 tests, 0 échec)** ; Miri Windows : **95 PASS, tests FS `NOT_RUN`** ; mutation ciblée `types.rs` (53 mutants, 0 survivant viable), `own.rs` (32 mutants, 1 survivant équivalent), `resolve.rs` (74 mutants, 21 survivants triés) et `typeck.rs` (106 mutants, 28 survivants triés) — **212 mutants, 153 tués** ; **couverture sema 88,9 % lignes / 88,3 % régions**, `typeck.rs` **93,4 %** et `resolve.rs` **93,0 %** : **objectifs 90/85 atteints** et **seuils bloquants en CI** (`coverage.yml`) | **Audit externe absent** ; **31 lacunes de test** de mutation identifiées et documentées (`MUTATION-EQUIVALENCE.md` §7.4) ; pas de score de mutation global | Audit externe |
| T11 | `PARTIAL` | Installateur (`sha256 5257d20d…`, manifeste, `--version`), SBOM **reproductible** (64 paquets, `b637b03e…`), provenance, checksums, licence, **build bit-à-bit identique entre deux racines et deux `target/`** (`67F2934B…`), **mécanisme de signature OpenSSH qualifié (accepte l'intact, rejette l'altéré)** ; procédure de custody **proposée** (`docs/release/KEY-CUSTODY.md`) | **Aucune clé de projet ni ancre de confiance** | Décision humaine **D9** (custody de clé) |
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

### T2 — Spécification formelle — `PASS` (une exemption déclarée)

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
| D2 | Ratification des ADR / choix du backend | **OUVERTE** |
| D3 | Fusion dans `main` et publication | **OUVERTE** |
| D4 | Audit externe (T4/T10/T12) | **OUVERTE** — bloque T4 et T12 ; cadre de mission prêt : [`AUDIT-BRIEF.md`](../audit/AUDIT-BRIEF.md) |
| D5 | Périmètre Tier 1 | **PARTIELLE** : Windows local, Linux sur CI |
| D6 | Périmètre des fonctionnalités universelles | **OUVERTE** |
| D7 | Ratification de la frontière `unsafe`/FFI | **OUVERTE** |
| **D8** | Arbitrage des codes de diagnostic | **RÉSOLUE** — le registre (`diagnostics.md` §3) déclare « tout code absent est interdit » ; les codes fautifs ont été remplacés. Le dernier restant, `W-TYP-UNREACHABLE`, n'a **pas** été enregistré : EXPR-5.9, G-6.44 et DIAG-1.6 imposent qu'un bras inatteignable ne produise aucun diagnostic, donc l'émission a été retirée et un test la verrouille |
| **D9** | **Custody de la clé de signature et publication de `allowed_signers`** | **NOUVELLE — bloque la signature de release (T11)** ; procédure proposée : [`KEY-CUSTODY.md`](../release/KEY-CUSTODY.md) |
| **D-PAT-1** | Motifs littéraux flottants/négatifs (`grammar.md` G-6.36/G-6.37 vs `diagnostics.md`) | **RÉSOLUE (2026-11-10, option A)** : les deux règles MUST sont **implémentées**, `E-UNSUPPORTED-NEG-PAT` est retiré du registre (111 codes, 0 jamais émis). Preuves : `T2-FINDINGS.md` §18, 8 contrats `typeck_contracts.rs`, 5 cas de corpus dont 2 `run-pass` |
| **D-CMP-1** | Famille d'affectations composées (`grammar.md` G-6.8 / `expressions.md` EXPR-9.4 **MUST acceptées** vs `diagnostics.md` `E-UNSUPPORTED-COMPOUND-ASSIGN` et implémentation limitée à `+=`) | **NOUVELLE — OUVERTE** : contradiction de spécification de même forme que D-PAT-1. Soit implémenter la famille (`-= *= /= %= &= \|= ^= <<= >>=`), soit amender deux MUST. Détail : `T2-FINDINGS.md` §19.2 |
| **D-COERCE-1** | Coercition implicite `String → &str` en position d'argument (`types.md` TYP-13.1 n'admet que `&String → &str`, TYP-8.5 rend la conversion explicite) | **NOUVELLE — OUVERTE** : comportement actuel accepté et **conservé** cette passe car `line.trim()` rend un `String` (STD-9.1) et que `examples/stats` + `conformance/cases/run/stats` (cité par F-STD-017) en dépendent. Détail : `T2-FINDINGS.md` §19.2 |

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



























