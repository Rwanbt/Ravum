# Ravum Core 1.0 — rapport consolidé de fin de session

> **Rapport de situation, pas un certificat.** Tous les chiffres ci-dessous ont
> été produits par une commande exécutée pendant la session, sur le SHA indiqué.
> Aucun `PROVED`, aucune maturité « production », aucune release. Le registre
> canonique reste [`docs/roadmap/CORE-1.0-GATE-MATRIX.md`](../roadmap/CORE-1.0-GATE-MATRIX.md).

> **Portée de lecture.** Les sections 1 à 15 décrivent la session du **2026-11-09**
> (SHA `a277b86`). La **section 0 sexies** ci-dessous porte la passe du
> **2026-11-11 (passe 9, SHA `21cf7d4`)** et **prévaut pour les chiffres
> courants** ; la section 0 quinquies est la passe 8 (`7696e2a`), la section
> 0 quater la passe 7 (`c6b4537`), la section 0 ter la passe 6
> (`5d88ecb` → `9da9b12`).

## 0 sexies. Addendum passe 9 — les deux dernières divergences de move sont fermées (2026-11-11, SHA `21cf7d4`)

Cette section **prévaut sur 0 quinquies et sur toutes les suivantes** pour les
chiffres courants. Elle porte sur une seule chose : les deux formes
`check`-pass / `build`-fail qui restaient ouvertes après la passe 8 sont
fermées.

| Mesure | Résultat |
|---|---|
| `cargo test --workspace` | **113 suites, 1537 passés / 0 échec / 2 ignorés** (exit 0) |
| Conformance (`run --verify-build`) | **456 cas : 455 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** ; build **195/195** couverts, **120** vérifiés, **75** implicites, **0 échec**, `build_verified: true` |
| Formes `check`-pass / `build`-fail ouvertes connues | **0** (contre 2 à la passe 8) |
| Tests ajoutés | 1 test `rustc`-vérifié (compile + exécute + vérifie que le déstructurage est émis avant le `if`) et 3 cas de corpus (`run/array-move-branch-descents`, `-nested-descents`, `-read-order`) |
| Couverture | **portée élargie et re-mesurée** : **88,54 % lignes / 87,86 % régions** sur `ravum-lang-sema` **+** `ravum-lang-codegen` (`emit.rs` 87,02/86,02) ; trois seuils codegen ajoutés, `coverage.yml` mesure les deux crates ; aucun seuil abaissé |
| Reproductibilité, SBOM, provenance, installateur | **non re-mesurés** dans cette passe ; les chiffres de la passe 8 restent les derniers mesurés, et la CI les rejoue (`qualification-evidence.yml`) |
| Traçabilité FeatureId | **188/188** (inchangée) |
| Registre des codes | **108** codes enregistrés / **126** émis / 0 hors registre |
| CI | **14/14 verts sur `7665244`** (7 `push` + 7 `pull_request`), dont la CI Windows de qualification (`43087652812` : `cases=456`, `perimeter=195`, `verified=120`, `implicit=75`, `build_verified=True`) |
| Gates | **4 PASS / 7 PARTIAL / 2 BLOCKED** — inchangé ; `P0-A` est désormais vert **sur son périmètre mesuré** (aucune forme ouverte connue) et `T2` reste `PARTIAL` pour la raison indépendante du dénominateur MUST (D10) |

Ce qui a changé, et ce que cela ne prouve pas :

* **Le défaut était dans le planificateur, pas dans le langage** : il ne groupait
  que les descentes qui étaient des instructions *directes* du bloc du binder,
  donc ni deux branches d'un `if` ni un bloc imbriqué n'avaient d'ancre de
  déstructurage. L'ancre est maintenant la plus ancienne instruction du bloc du
  binder qui contient une descente, ce qui couvre les deux formes **et** garde le
  hiss partagé des instructions sœurs.
* **La suite complète a attrapé une régression que la CI n'aurait pas vue** : la
  première version groupait par instruction d'ancrage et rouvrait `E0508` sur une
  forme déjà fermée (`array-move-shared-descent`). Le test existant l'a rejetée
  immédiatement ; c'est l'argument pour les tests `rustc`-vérifiés.
* **Zéro forme connue ouverte n'est pas « aucune divergence »** : le corpus ne
  teste que ce qu'il contient. `P0-A` est vert sur son périmètre (456 cas,
  `perimeter 195` entièrement couvert) et l'invariant général reste une
  observation outillée, pas une preuve formelle.
* **Ce que cette passe ne prouve pas** : pas d'audit externe (D4, T4/T12 restent
  `BLOCKED`), pas de re-mesure de couverture, de reproductibilité ou de mutation
  — aucune n'est revendiquée —, et `T2` reste `PARTIAL` (D10).

## 0 quinquies. Addendum passe 8 — preuves continues et véracité documentaire (2026-11-11, SHA `7696e2a`)

Cette section **prévaut sur 0 quater et sur toutes les suivantes** pour les
chiffres courants. Elle porte sur trois choses : les dernières formes
`check`-pass / `build`-fail, le passage des preuves lourdes en CI, et un audit de
véracité de `spec/` et `docs/`.

| Mesure | Résultat |
|---|---|
| `cargo test --workspace` | **113 suites, 1536 passés / 0 échec / 2 ignorés** (exit 0) |
| Conformance (`run --verify-build`) | **453 cas : 452 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** ; build **192/192** couverts, **120** vérifiés, **72** implicites, **0 échec**, `build_verified: true` |
| Sondage round-3 (`check ⇒ build`) | **47 programmes × 11 axes : 47 build-pass, 0 divergence** (inchangé) |
| Reproductibilité bit-à-bit | **deux builds identiques**, `sha256 = 847DF0D1…59335CB96` (deux racines, deux `target/`) |
| Couverture sema (`llvm-cov`) | **88,94 % lignes / 88,46 % régions** ; `typeck.rs` 92,74 %, `resolve.rs` 93,13 %, `check.rs` 80,26 % (seuil 80) — **seuils tenus, aucun seuil modifié** |
| SBOM | SPDX 2.3 **déterministe** (deux runs, un digest), 64 paquets / 170 relations |
| Installateur Windows | contrat CI complet (installation, `--version`, manifeste, désinstallation idempotente, réinstallation) — **16/16** |
| Provenance | `emit` + `verify` + refus d'un binaire altéré ; **fail-closed** si un artefact manque (défaut corrigé) |
| Traçabilité FeatureId (inchangée) | **188/188** (187 cas + 1 exemption `F-META-006`) |
| Traçabilité MUST (inchangée) | **807 MUST ; 463/807 atteints (57,4 %) ; 344 non atteints** |
| Registre | **108 codes enregistrés / 126 émis / 0 hors registre / 0 jamais émis** |
| Binaire épinglé | `ravum-lang.exe` sha256 `333C08AF…A100777F` (Windows 11 x64, `rustc 1.95.0`) |
| Gates | **4 PASS / 7 PARTIAL / 2 BLOCKED** — inchangé ; `P0-A` reste `PARTIAL` (deux variantes bruyantes documentées), T2 reste `PARTIAL` (463/807) |

Ce qui a changé, en une phrase par point :

* **Six formes de déplacement d'élément sont fermées** : les quatre laissées
  ouvertes par la passe 7 et deux trouvées par la revue de cette passe. Chacune a
  un test qui compile réellement avec `rustc` et exécute le binaire.
* **Deux variantes restent ouvertes** (descentes dans deux branches distinctes ;
  deux descentes dans un même bloc imbriqué) : `check` exit 0 puis `build` exit 1
  avec un `E0508` bruyant, jamais de valeur fausse. Programmes minimaux et
  mécanisme dans `docs/qualification/CONFORMANCE-BUILD-CONTRACT.md` §9 ter.
  Aucun test ne verrouille le défaut.
* **Les cinq preuves lourdes sont devenues des jobs CI** sur `windows-latest`
  (`qualification-evidence.yml`), alors qu'elles n'existaient que dans des logs
  locaux recopiés à la main. Windows n'avait aucun job de conformance.
* **La CI a trouvé deux défauts dans ce nouveau workflow** : une étape qui prouve
  un refus héritait du code de sortie `1` malgré 8/8 assertions vertes, et un
  `$code:` dans un message est une référence à portée, donc un bloc entier
  n'était jamais analysé. Corrigés ; c'est l'argument même pour faire tourner les
  preuves en CI plutôt qu'à la main.
* **Une quarantaine d'affirmations fausses sont corrigées** dans `spec/` et
  `docs/` (LSP « UNSUPPORTED », WASM, `--emit-ir`, FFI/`unsafe`, licence,
  signature, reproductibilité, codes `E-TYP-RETURN` / `E-MOD-BOTH-ROOTS`,
  inventaires de tests). Motif commun : un document écrit à une passe antérieure
  et jamais revu.
* **La couverture a été re-mesurée** : 88,94 % / 88,46 % contre 89,2 % / 88,6 %
  publiés, `typeck.rs` 92,74 % contre 93,8 %. La baisse est rapportée, pas
  cachée ; elle vient du code ajouté depuis, et les seuils par fichier tiennent.

Ce que cette passe **ne prouve pas** : pas d'audit externe (D4), pas de signature
(D9), pas de release, pas de reproductibilité inter-machines, pas de fermeture des
deux variantes ci-dessus, et la campagne de mutation n'a pas été rejouée.



## 0 quater. Addendum passe 7 — CI réelle et places de tableau (2026-11-11, SHA `c6b4537`)

Cette section **prévaut sur 0 ter et sur toutes les suivantes** pour les chiffres
courants de la passe 7 (les chiffres de la passe 8 sont en 0 quinquies). Elle a
deux objets : la CI, qui n'avait jamais tourné sur ces commits, et la fermeture
des formes `check`-pass / `build`-fail restantes.

| Mesure | Résultat |
|---|---|
| `cargo test --workspace -- --test-threads=2` | **113 suites, 1530 passés / 0 échec / 2 ignorés** (exit 0) |
| Conformance (`run --verify-build`) | **448 cas : 447 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** ; build **187/187** couverts, **120** vérifiés, **67** implicites, **0 échec**, `build_verified: true` |
| Sondage round-3 de l'invariant `check ⇒ build` | **47 programmes × 11 axes : 47 check-pass, 47 build-pass, 0 divergence** (le script sortait `1` avec 4 divergences avant le correctif) |
| Smoke exemples | **43 PASS / 0 FAIL / 0 NOT_RUN** |
| Watch fail-closed | **PASS** |
| Traçabilité FeatureId (inchangée) | **188/188** (187 cas + 1 exemption `F-META-006`) |
| Traçabilité MUST (inchangée) | **807 MUST ; 463/807 atteints (57,4 %) ; 344 non atteints** |
| Registre | **108 codes enregistrés / 126 émis / 0 hors registre / 0 jamais émis** |
| Contrats Python | **33 tests OK** |
| **CI GitHub Actions** | **12 runs verts sur 12 sur `c6b4537`** (6 workflows × `push` + `pull_request`) : Linux qualification `43031394324` (7 jobs verts + TSan `NOT_APPLICABLE`), MVP product checks Windows `43031394660` (build, tests, démos M1/COLLAB-1/M2/DSP/M4, browser E2E), gate contracts `43031394278`, coverage `43031394299`, impact `43031394377`, VS Code `43031394291` |
| Gates | **4 PASS / 7 PARTIAL / 2 BLOCKED** — inchangé ; `P0-A` reste `PARTIAL` (4 formes de move encore ouvertes, listées) et T2 reste `PARTIAL` (463/807) |
| Binaires épinglés | `ravum-lang.exe` sha256 `5DDF8E5F…A705C499` ; `ravum-conformance.exe` sha256 `9F21CFF0…0600DF8` |

Deux défauts d'honnêteté de nos **propres tests** ont été révélés par Linux (jamais
par le local Windows) et corrigés : le contrat d'`exit code` était écrit depuis
l'observation Windows seule (POSIX tronque à 8 bits avant le parent), et le test de
build interrompu faisait la course entre `try_wait` et `kill`. Un troisième défaut,
visible celui-là, était l'absence de `cargo fmt` : le job bloquant de formatage
échouait. Les trois corrections sont dans `ab51b17`, `10459b8`, `f10a12f`,
`c67f17e`.

Cinq formes `check`-pass / `build`-fail sont **fermées** (`0f73738`) : chaîne
d'index imbriquée, tableau derrière un champ/tuple, liaison masquée par shadowing,
récepteur `self` par valeur sur un élément, payload de tableau dans un bras de
`match` — cette dernière étant une régression introduite par le correctif
lui-même, interceptée par le sondage systématique **avant** intégration. Quatre
formes restent ouvertes, bruyantes (E0508) et documentées : index dynamique dans un
tableau imbriqué, racine de chaîne qui n'est pas une place, plusieurs accès dans un
bras de `match` en corps d'expression, récepteur `by value` d'une méthode builtin.
Détail : [`T2-FINDINGS.md`](../qualification/T2-FINDINGS.md) §22.

## 0 ter. Addendum P0/P1/P2 — passe du 2026-11-11 (base `5d88ecb`)

Cette passe d'implémentation **prévaut sur toutes les sections ci-dessous** pour
les chiffres courants. Rapport détaillé (sections A–J) :
[`DELIVERY-REPORT-2026-11-11.md`](DELIVERY-REPORT-2026-11-11.md). Détail des
divergences : [`T2-FINDINGS.md`](../qualification/T2-FINDINGS.md) §21.

| Mesure | Résultat |
|---|---|
| `cargo test --workspace --all-targets` | **94 suites, 1525 passés / 0 échec / 2 ignorés** |
| Conformance (`run --verify-build`) | **424 cas : 423 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** ; build 163/163 couverts, 101 vérifiés, 62 implicites, **0 échec**, `build_verified: true` |
| Smoke exemples | **43 PASS / 0 FAIL / 0 NOT_RUN** |
| Traçabilité FeatureId (inchangée) | **188/188** (187 cas + 1 exemption `F-META-006`) |
| Traçabilité MUST (nouvelle métrique) | **807 MUST ; 463/807 atteints ; 344 non atteints** |
| Registre | **108 codes enregistrés / 126 émis / 0 hors registre / 0 jamais émis** |
| Contrats Python | **33 tests OK** |
| Gates | **4 PASS / 7 PARTIAL / 2 BLOCKED** — T2 repasse à `PARTIAL` : `188/188` porte sur les FeatureIds, pas sur les 807 MUST |
| CI | **NOT_RUN** sur ces commits (aucun push) |

Onze programmes acceptés par `check` mais refusés par `rustc` ont été corrigés
(références texte stockées/retournées, moves d'éléments de tableau, `mut` de
paramètre, `unwrap` consommateur, tableaux annotés, exhaustivité imbriquée,
`String → &str` implicite). Trois formes restent ouvertes et documentées
(chaîne d'index imbriquée, tableau derrière un champ/tuple, shadowing imbriqué) :
E0508 bruyant, hors corpus. Aucun test supprimé, aucune règle de spécification
modifiée, aucune revendication de `PASS` non mesurée.


## 0. Addendum P0 — passe de fiabilité des gates (2026-11-10, SHA `a5744ea`)

Deux vecteurs de faux `PASS` mesurés au début de la passe :

1. `python tools/features_coverage.py --strict` sortait **1** (`F-META-006`
   non couverte) alors que T2 était déclarée `PASS` par une exemption rédigée
   uniquement dans un rapport, et aucun job CI n'exécutait ce contrôle.
2. `python tools/registry_audit.py` sortait **1** : `W-TYP-UNREACHABLE` était émis
   par `typeck.rs` alors que EXPR-5.9, G-6.44 et DIAG-1.6 interdisent tout
   diagnostic pour un bras de match inatteignable.

Traitement (commit `16775b4`) : exemption **machine-lisible** et validée dans
`tools/coverage_exemptions.json` (raison, référence normative, approbateur,
condition de réexamen) ; outil qui refuse une exemption inconnue, mal formée,
obsolète ou masquant une règle non marquée `NOT_COVERED` ; émission
`W-TYP-UNREACHABLE` **retirée** conformément à la spécification et verrouillée par
un test Rust ; audit du registre bloquant dans les **deux** sens ; nouveau
workflow `gate-contracts.yml` ; 12 tests Python de régression.

Puis, en vérifiant les affirmations « vertes » du frontend : deux défauts réels
mesurés et corrigés — le type `!` était refusé dans **toute** position alors que
G-5.11/TYP-1.6 le rendent nommable pour une fonction divergente (ce qui faisait
passer un cas de refus pour la mauvaise raison), et les tuples hétérogènes
`(i32, bool)` étaient unifiés comme des tableaux, avec l'exhaustivité de motif
tuple cassée en conséquence. Une contre-expertise indépendante a ensuite rejeté
la première implémentation de `!` (elle typait `loop` comme `!`, contredisant
EXPR-5.11/6.7/G-6.19) et révélé trois trous : `as !` accepté, inférence
`Vec<!>`/`Option<!>`, `for` sur `!`. La version retenue garde `loop` de type `()`,
rejette ces trois cas (`E-TYP-CAST`, `E-TYP-AMBIGUOUS`, `E-TYP-MISMATCH`) et
aligne EXPR-11.3. Quinze cas de conformance neufs couvrent les deux
directions, dont `never-return-run` qui **compile et exécute** une fonction
`-> !`, et une exécution réelle d'un tuple déstructuré par `match` (`t.0` → `7`).
Détail : `docs/qualification/T2-FINDINGS.md` §14.

Deux défauts supplémentaires ont été mesurés et traités pendant la passe :

- **Tableaux hétérogènes acceptés** : `check_array` ne comparait les éléments
  que sous annotation ; `let a = [1, true];` était accepté alors que le codegen
  émet `[1, true]`, que `rustc` refuse. Correction : sans annotation, chaque
  élément est comparé au premier (`E-TYP-MISMATCH` sur l'élément fautif).
  Cas neufs `types/array-heterogeneous-fail` et
  `types/array-annotated-length-mismatch-fail`, plus 88 contrats dans
  `crates/ravum-lang-sema/tests/typeck_contracts.rs`. Détail : §15 de
  [`T2-FINDINGS.md`](../qualification/T2-FINDINGS.md).
- **Motifs littéraux flottants et négatifs** : G-6.36 et G-6.37 (MUST) les
  admettent, l'implémentation les refuse (`E-PARSE-EXPECTED`,
  `E-UNSUPPORTED-NEG-PAT`) et `diagnostics.md` documente ce refus : la
  spécification se contredit. Aucun changement unilatéral ; décision
  **D-PAT-1** demandée. Détail : §16 de `T2-FINDINGS.md`.

Mesures locales sur `67a2a2e` (Windows 11 x86-64, rustc 1.95.0) :
| Mesure | Résultat |
|---|---|
| `cargo test --workspace --all-targets` | **86 suites, 1320 passés / 0 échec / 2 ignorés** |
| Conformance | **384 cas : 383 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** |
| Traçabilité stricte | **188/188** règles satisfaites (187 par cas + 1 exemption structurellement vérifiée (revue humaine non faite)) |
| Registre des diagnostics | **111 codes** ; 129 émis ; **0 hors registre** ; **0 jamais émis** |
| Contrats de validation | **12 tests** PASS |
| Couverture sema (llvm-cov) | **88,9 % lignes / 88,3 % régions** ; `types.rs` 100 %, `own.rs` 90,9 %, `typeck.rs` **93,4 %**, `resolve.rs` **93,0 %** — **objectifs 90/85 atteints** sur les deux fichiers du gate, seuils bloquants en CI |
| Mutation testing ciblé (`types.rs`) | **53 mutants : 46 caught, 7 unviable, 0 missed** — 100 % des mutants viables tués |
| Mutation testing (`own.rs`, `resolve.rs`, `typeck.rs`) | **212 mutants : 153 tués, 50 survivants triés (14 inatteignables, 4 observables seulement via le codegen, 1 équivalent, 31 lacunes de test), 9 invivables** ; détail dans `MUTATION-EQUIVALENCE.md` §7 |
| Contrats du typechecker | **107 + 65 tests** (`typeck_contracts.rs`, `typeck_contracts2.rs`) ; `typeck.rs` 63,7 % → **93,4 %** de lignes |
| Contrats de résolution | **112 tests** (`resolve_contracts.rs`) ; `resolve.rs` 67,4 % → **93,0 %** |
| Audit du borrow checker | **93 tests** (`own_audit.rs`) |
| VS Code réel — au repos | **10/10 en 5 s** |
| VS Code réel — sous charge | **10/10 en 4 s** pendant `cargo test --workspace` concurrent |
| CI VS Code | workflow `vscode-e2e.yml` **vert** : push `42858553296` (49 s) et PR `42858866200` (1 m 04 s) sur `9972391` |
| Fuzz lexer/parser | 249 054 entrées, 0 échec, digest `0x338b9afe9926aa4f` |
| Fuzz sema | 100 000 itérations, 0 échec |
| Smoke exemples | 31 PASS / 0 FAIL |
| Balayages CLI / stdlib | 13/13 PASS ; 64/64 PASS |
| Reproductibilité | deux builds bit-à-bit identiques, `sha256 = EC3133FB…4865` |
| SBOM | 64 paquets, 170 relations, digest `7c532967…93ee3` à époque fixée |
| Performance | `check` 45,1 / 89,4 / 331,2 ms et `build` 329,9 / 647,1 / 2 151,7 ms pour 100 / 500 / 2 000 fonctions (Windows 11 x86-64, rustc 1.95.0, `dev`, 3 runs par taille) |
| Miri Windows | 95 tests PASS ; tests utilisant `std::env::temp_dir` **NOT_RUN** (`GetTempPathW` non supporté) — Miri complet reste lié à la CI Linux antérieure |
| CI sur `b7e5c5c` (code `63eda55`) | **succès** : 83 suites / 1019 passés Linux, conformance `340 cas / 338 PASS / 0 FAIL / 2 SKIP`, Miri Linux **159 tests / 0 échec**, gate-contracts succès |
| CI sur le HEAD `e05f3ff` (code `63619a5`) | **succès** : workspace Linux **1026 passés / 0 échec / 4 ignorés**, conformance `343 cas / 341 PASS / 0 FAIL / 2 SKIP`, Miri Linux **164 tests / 0 échec**, gate-contracts succès (runs `42837268985`, `42837285517`) |
| CI sur `a6e71e7` (docs `7e065d3`) | **succès** : workspace Linux **89 suites / 1292 passés / 0 échec / 5 ignorés**, conformance **346 cas / 344 PASS / 0 FAIL / 2 SKIP**, Miri, smoke, installateur, WASI et fuzz verts ; `vscode-e2e` vert sur le push (`42862021707`, 43 s) et la PR (`42862029061`, 45 s) ; gate contracts `42862021037` |
| **CI sur `29ef8be`** (code `67a2a2e`) | **succès** : Linux qualification `42882234365` — workspace **86 suites / 1317 passés / 0 échec / 4 ignorés**, conformance **384 cas / 382 PASS / 0 FAIL / 2 SKIP**, Miri, smoke, installateur, WASI et fuzz verts, TSan `NOT_APPLICABLE` ; **coverage ratchet** `42882234248`/`42882250559` vert (premier run rouge sur `dea7f43` : bug de normalisation de chemins corrigé dans `29ef8be` et verrouillé par 8 tests) ; gate contracts `42882234269`/`42882250616`, docs `42882250276`, VS Code `42882234259`/`42882250425`, MVP product `42882234370` |

Les statuts de gates évoluent sur deux points seulement : **T2 et T3 restent
`PASS`** (leur preuve devient exécutable et bloquante au lieu d'être
déclarative), et **T7 passe de `PASS` à `PARTIAL`** : le rejeu VS Code du
2026-11-10 donne 10/10 puis 3/10 sur le même arbre (timeouts du harnais), donc la
preuve E2E n'est pas reproductible de façon stable. Le harnais est depuis
**corrigé** (budgets 300 s/180 s, barrière de disponibilité LSP) et donne 10/10
au repos comme sous `cargo test --workspace` concurrent ; un job CI
`vscode-e2e.yml` est ajouté. **T7 passe à `PASS`** : le job CI `vscode-e2e` est vert sur le push
`42858553296` et la PR `42858866200` (SHA `9972391`), en plus des deux runs
locaux (au repos et sous charge). Bilan : **5 `PASS` · 0 `FAIL` · 6 `PARTIAL` ·
2 `BLOCKED`**. T4, T10 et T12 restent
bloqués par l'audit externe et le Go/No-Go humain ; T1, T6, T8, T9 et T11 restent
partiels pour les raisons structurelles déjà listées. Bilan courant :
**5 `PASS` · 0 `FAIL` · 6 `PARTIAL` · 2 `BLOCKED`**.

## 1. Dépôt GitHub — état initial et final

| Élément | Initial | Final |
|---|---|---|
| `main` | `ca675497` | **`ca675497` — intacte** |
| `dev` | `fbaa506` | **`fbaa506` — intacte** |
| Branche de travail | `feat/lang-core-1.0-rc` @ `16d4e5c` | **`feat/lang-core-1.0-rc` @ `a277b86`** |
| Commits ajoutés | — | **29** |
| PR | [#101](https://github.com/Rwanbt/Ravum/pull/101), draft | **draft, base `dev`, `MERGEABLE`** |
| Réécriture d'historique | — | **aucune** (pas de force-push, pas de `main`) |

## 2. Vault Obsidian

- Note de session : `projects/ravum/operations/sessions/2026-11-09-t2-traceability-and-vscode-session.md`
  dans le dépôt `Rwanbt/obsidian-dev-brain` (branche `master`). Le vault est un
  dépôt séparé : aucun lien relatif ne le traverse depuis ce dépôt-ci.
- Commit vérifié présent sur `origin/master` (`git cat-file -e`).
- `python _system/tooling/vault.py check --format json` : `lint`, `links`,
  `idempotence`, `root-cleanup` **verts** ; `migrate` signale **4 hashes
  préexistants** dans `projects/rimatrix` et `projects/seno-daw`, hors périmètre
  Ravum et non modifiés ici.

## 3. SHA exacts des preuves

| Preuve | SHA |
|---|---|
| Code et tests | **`a277b86`** |
| CI Linux (dernier run complet vert sauf conformance) | `a277b86` |
| Corpus de conformance | `a277b86` |
| Rejeu fuzz | `0dc93a1` (arbre identique côté compilateur) |
| Reproductibilité bit-à-bit | `a277b86` |
| Session VS Code | `a277b86` |

## 4. Matrice T0–T12

| Gate | État | Base |
|---|---|---|
| T0 | **PASS** | corpus sécurité du socle (overlays, Git, journal, symlinks) |
| T1 | PARTIAL | voies A/B réelles ; **voie C non implémentée** |
| T2 | **PASS** | 326 cas **325 PASS / 0 FAIL / 1 SKIP** ; **187/187 règles testables** au critère strict ; 112 codes enregistrés |
| T3 | **PASS** | frontend conforme : 5 défauts corrigés, fuzz + Miri rejoués |
| T4 | **BLOCKED** | **audit externe humain absent** |
| T5 | **PASS** | natifs Windows **et** Linux (CI) + WASI, smoke 31 PASS |
| T6 | PARTIAL | threads/réseau/async explicitement `UNSUPPORTED` |
| T7 | **PASS** | **session VS Code réelle 10/10** + LSP 11 tests |
| T8 | PARTIAL | oracle cold vs warm ; **pas de préservation d'état** |
| T9 | PARTIAL | 9 verticales ; pas de HTTP+SQLite, DSP hôte, E2E navigateur |
| T10 | PARTIAL | fuzz + Miri rejoués ; **audit externe absent** |
| T11 | PARTIAL | bit-à-bit + performance mesurés ; **signature sans clé (D9)** |
| T12 | **BLOCKED** | **Go/No-Go humain** |

**5 `PASS` · 0 `FAIL` · 6 `PARTIAL` · 2 `BLOCKED` (état de la session du 2026-11-09 ; voir §0 pour l'état courant).**

## 5. Tests — exécutés, ignorés, non exécutés

| Mesure | Résultat |
|---|---|
| `cargo test --workspace --all-targets` | **82 suites, 1020 passés, 0 échec, 2 ignorés** |
| Les 2 ignorés | `fuzz_*` de longue durée (`#[ignore]`), rejoués séparément |
| LSP (tubes réels) | **11 passés** |
| VS Code (session réelle) | **10 passés / 10** |
| Smoke exemples | **31 passés / 0** |
| `cargo fmt --all -- --check` | exit 0 |
| `python tools/validate_repo_docs.py` | exit 0 |

## 6. Conformance

**326 cas : 325 PASS / 0 FAIL / 1 SKIP** (Windows) · **321 PASS / 0 FAIL / 2 SKIP** (Linux).

Le `SKIP` est **par plateforme** : chaque cas tourne sur au moins un OS, aucun sur
les deux. Ce n'est pas un `PASS` universel.

Aucun échec. Les **13 codes non enregistrés** que le compilateur émettait ont été
remplacés par les codes canoniques de `diagnostics.md` §3. Le corpus passe, mais
le critère strict de traçabilité est satisfait par **187/187 règles testables** : il
reste **82 tests négatifs** à écrire. Détail :
[`docs/qualification/T2-FINDINGS.md`](../qualification/T2-FINDINGS.md).

## 7. Corrections appliquées (14)

LSP : navigation depuis la déclaration, `prepareRename`, `item_name_at`.
Compilateur : `effects` contextuel, `ref` identifiant, `pub test fn`,
`while`/`loop`/`for` en position de valeur, `int::abs`, zéros signés de
`math::min`/`max`, `Vec<T>::clone` exigeant `Copy`, méthode sans appel,
arité de motif tuple, chaînes raw/byte, plages hors `for`, `'a` sans cascade,
cible d'affectation devant être une place.

**Aucun `expect_codes` n'a été aligné sur le comportement observé** : ce sont les
comportements qui ont été alignés sur la spécification.

## 8. Fuzzing

| Campagne | Résultat |
|---|---|
| lexer/parser, 200 000 itérations (249 054 entrées) | **0 échec**, digest `0xcb1740aef51d2879` |
| sema, 100 000 itérations | **0 échec** |

Rejouées **après** les correctifs : les chiffres antérieurs ne valaient plus.

## 9. Miri et sanitizers

- **Miri** : 65 + 93 + 1 = **159 tests, 0 échec** (CI Linux, `a277b86`).
- **ThreadSanitizer** : `SKIP` explicite `NOT_APPLICABLE` — Core 1.0 n'expose ni
  threads ni async. Justifié, pas supprimé.

## 10. Benchmarks et ressources

| N fonctions | lignes | `check` (médiane) | pic | `build` (médiane) | pic |
|---:|---:|---:|---:|---:|---:|
| 100 | 1 106 | 38,7 ms | 9,1 Mo | 284,9 ms | 10,0 Mo |
| 500 | 5 506 | 86,4 ms | 20,1 Mo | 543,5 ms | 21,7 Mo |
| 2 000 | 22 006 | 296,6 ms | 60,4 Mo | 1 789,6 ms | 63,8 Mo |

`build` est dominé par `rustc` (~1,49 s sur 1,79 s à 2 000 fonctions). Détail et
limites : [`PERFORMANCE.md`](../qualification/PERFORMANCE.md).

## 11. Reproductibilité et chaîne de distribution

- **Bit-à-bit** : deux builds depuis deux racines et deux `target/` donnent
  `sha256 = 3B606F6B…9E9B` (`scripts/repro-check.ps1`). Sans drapeaux, trois
  builds donnaient trois binaires ; l'écart se limitait à 20 octets d'horodatage
  PE.
- **SBOM** : 64 paquets, 170 relations, digest `057f006e…492b` à époque fixée.
- **Licences** : 23/23 membres en `MIT OR Apache-2.0`.
- **Signature** : mécanisme qualifié (accepte l'intact, rejette l'altéré d'un
  octet) — **aucune clé de projet**, donc aucune release signée.

## 12. Applications `.rv` et plateformes

9 verticales exécutées : `hello`, `fib`, `calculator`, `file-tool`, `stats`,
`dsp-gain-iir`, `package-demo`, `tests-demo`, `ffi-abs`.
Plateformes qualifiées : Windows x86-64 (local), Linux x86-64 (CI), `wasm32-wasip1`.

## 13. Limites de sécurité

- `unsafe`/FFI : frontière documentée et testée, **non auditée par un humain**
  (D7).
- Aucune garantie ABI stable, aucune revendication macOS/ARM.
- Le corpus est fini : l'absence d'échec n'est pas une preuve d'absence de bug.

## 14. Décisions humaines

| ID | Sujet | État |
|---|---|---|
| D1 | Licence | **RÉSOLUE** |
| D2 | Ratification ADR / backend | ouverte |
| D3 | Fusion `main` + release | ouverte |
| D4 | **Audit externe** | ouverte — bloque T4/T12 |
| D5 | Périmètre Tier 1 | partielle |
| D6 | Périmètre universel | ouverte |
| D7 | Frontière `unsafe`/FFI | ouverte |
| **D8** | Identifiants de diagnostic | **RÉSOLUE** — le registre interdisait déjà ces codes |
| **D9** | **Custody de la clé de signature** | ouverte — bloque la signature |
| **D-PAT-1** | Motifs littéraux flottants/négatifs (`grammar.md` G-6.36/G-6.37 vs `diagnostics.md`) | ouverte — soit implémenter, soit amender deux MUST |

## 15. Verdict et reprise

**Verdict de maturité : `CORE_RC_CANDIDATE`.** Jamais `RELEASED` : il n'y a ni
audit externe, ni Go/No-Go humain, ni release signée.

```powershell
git fetch origin; git switch feat/lang-core-1.0-rc   # a277b86
cargo build -p ravum-lang-cli
cargo test --workspace --all-targets -- --test-threads=2   # 1020 pass
cargo run -p ravum-conformance -- run                      # 326 cas, 325/0, 1 skip
python tools/features_coverage.py --strict                 # 187/187 testables
pwsh -File scripts/lang-smoke.ps1 -SkipBuild               # 31 PASS
pwsh -File scripts/repro-check.ps1                         # bit-à-bit
pwsh -File scripts/lang-bench.ps1 -Runs 3                  # perf
cd tools/vscode-ravum; npm install; node test/runTest.js   # 10/10 VS Code
```

## 15 bis. Point de reprise vérifié depuis un clone neuf

Les commandes de §15 ont été **exécutées depuis un clone neuf** (`git clone
--depth 1 --branch feat/lang-core-1.0-rc`), dans un répertoire temporaire, sans
réutiliser le `target/` de la machine de développement.

| Commande | Résultat dans le clone neuf | Attendu (ce rapport) |
|---|---|---|
| `cargo build -p ravum-lang-cli` | succès | succès |
| `cargo run -p ravum-conformance -- run` | **326 cas, 325 PASS / 0 FAIL / 1 SKIP** | identique |
| `python tools/features_coverage.py --strict` | **187/187 testables** | identique |
| `python tools/registry_audit.py` | **112 codes, 1 hors registre** | identique |
| `pwsh scripts/lang-smoke.ps1 -SkipBuild` | **31 PASS / 0 FAIL** | identique |
| `cargo test --workspace --all-targets` | **82 suites, 1020 passés / 0 échec / 2 ignorés** | identique |

**Toutes les mesures se reproduisent à l'identique**, y compris la suite workspace
complète.

Un défaut de reproductibilité a été trouvé puis corrigé pendant cette
vérification : depuis un clone profond, `ravum-collab-oracle` échouait avec
`linking with link.exe failed: exit code 1104`. Cause mesurée : les répertoires
de campagne produisaient des chemins de **274 caractères**
(`<state>/cargo-target/debug/incremental/<hash>/<hash>/*.o`), au-delà de la limite
Windows de 260. Deux changements dans la campagne (`--target-dir` nommé `ct`,
`CARGO_INCREMENTAL=0`) ramènent les chemins sous la limite. Le message
fail-closed nomme désormais l'unité en échec et son premier diagnostic.

Le point de reprise est donc vérifié **de bout en bout** depuis un clone neuf :
build, suite workspace, conformance, traçabilité stricte, registre et smoke.
## 16. Ce que ce rapport ne prétend pas

- Ce n'est **pas** un audit de sécurité indépendant.
- Ce n'est **pas** une preuve de correction du langage.
- Les gates `PASS` sont bornés à leur périmètre écrit (plateformes, corpus,
  profils) et le disent dans la matrice.
- Les 82 règles sans test négatif sont comptées, pas dissimulées : c'est le
  travail restant de T2.


















## Addendum — passe 5 (2026-11-10, SHA `ea7df46`)

Les chiffres du corps ci-dessus décrivent `67a2a2e` (passe 4) et restent
historiques. Mesures de la passe 5, mêmes commandes :

| Mesure | Résultat |
|---|---|
| `cargo test --workspace --all-targets` | **89 suites, 1405 passés / 0 échec / 2 ignorés** |
| Conformance | **386 cas : 385 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** |
| Traçabilité stricte | **188/188** règles satisfaites (187 par cas + 1 exemption structurellement vérifiée (revue humaine non faite)) |
| Registre des diagnostics | **108 codes** ; 126 émis ; **0 hors registre** ; **0 jamais émis** |
| Couverture sema (llvm-cov) | **89,2 % lignes / 88,6 % régions** ; `types.rs` 100 %, `own.rs` 91,1 %, `typeck.rs` **93,8 %**, `resolve.rs` **93,1 %** — seuils bloquants tenus |
| Mutation ciblée (3 fichiers) | **195 mutants : 166 tués (85,1 %), 20 survivants triés (14 inatteignables, 6 équivalents), 9 invivables, 0 timeout, 0 lacune de test résiduelle** ; `MUTATION-EQUIVALENCE.md` §8 |
| Reproductibilité | deux builds bit-à-bit identiques, `sha256 = 83372F4F…D0546DD` |
| SBOM | 64 paquets, 170 relations, `sha256 = dc7b4e0d…c2134f1` |
| Smoke exemples | **31 PASS / 0 FAIL / 0 NOT_RUN** |
| Installateur | `sha256 = 28ba4b1c…f427dce680`, manifeste `locked = true` |
| Gates | **5 PASS / 6 PARTIAL / 2 BLOCKED** (inchangés : T4/T12 restent bloqués par l'audit externe et le Go/No-Go humains) |
| CI sur `ea7df46` | **tous les workflows verts** : Linux qualification [42933086911](https://github.com/Rwanbt/Ravum/actions/runs/42933086911) (workspace **89 suites / 1402 passés / 0 échec / 4 ignorés**, conformance **386 cas / 384 PASS / 0 FAIL / 2 SKIP**, Miri, smoke 31/0/0, stdlib 64/64, CLI 13/13, installateur, WASI, fuzz smoke), coverage ratchet (push/PR), gate contracts (push/PR), documentation, VS Code e2e (push/PR), MVP product et MVP impact |

Cette passe traite les quatorze points de `T2-FINDINGS.md` §19.2 (privacy du
résolveur, MOD-3.2, `test fn`, comparaisons mixtes, `Vec::new()+push`, famille
complète des affectations composées, codes inatteignables, harnais `W-*`,
phases JSON) et résout **D-CMP-1**, **D-UFCS-1** et **D-MOD-3.2**. Détail et
tri des survivants : `T2-FINDINGS.md` §20 et `MUTATION-EQUIVALENCE.md` §8.
