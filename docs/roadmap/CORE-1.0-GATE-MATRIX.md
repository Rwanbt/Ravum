# Ravum Core 1.0 — registre unique des gates T0–T12

> **Registre de suivi, pas un certificat.** Un seul tableau fait foi pour l'état
> des gates. Aucun `PASS` n'y est écrit sans preuve exécutée, liée à un SHA et
> reproductible par la commande citée.
>
> États autorisés : `PASS`, `PARTIAL`, `FAIL`, `BLOCKED`, `NOT_RUN`.
> `SKIP` et `NOT_RUN` ne sont **jamais** convertis en `PASS`. Un `BLOCKED`
> n'est jamais converti automatiquement en `PASS`.
>
> Dernière mise à jour : 2026-11-09 (passe T2 de traçabilité + session VS Code).

## 0. Base de référence

| Élément | Valeur |
|---|---|
| Branche de travail | `feat/lang-core-1.0-rc` |
| Baseline au début de la session | `16d4e5c475a613b4b0142d2cc08c635ee63ed42c` |
| **SHA des preuves de cette passe** | `1b7b4f1` (navigation LSP, `prepareRename`, extension VS Code, 161 cas de conformance) |
| SHA des preuves CI antérieures | `c43dafd` / `01fc148` — **4 workflows verts** (état antérieur, ne couvre pas cette passe) |
| `dev` fusionné dans la branche | `fbaa5068af39d5dfbcb27af4aaf378ff37ec2e7b` (PR #103, licence) |
| `main` (intacte) | `ca675497d1f8d74cbb7a93d12715a845149a8785` |
| PR du langage | [#101](https://github.com/Rwanbt/Ravum/pull/101) (draft, base `dev`) |
| Licence | `MIT OR Apache-2.0`, 23/23 membres du workspace |

## 1. Mesures de cette passe (SHA `1b7b4f1`)

| Mesure | Commande | Résultat exact |
|---|---|---|
| Tests workspace | `cargo test --workspace --all-targets -- --test-threads=2` | **82 suites, 1020 passés / 0 échec / 2 ignorés** (exit 0) |
| LSP (bout en bout, tubes réels) | `cargo test -p ravum-lang-cli --test lsp_client` | **11 PASS / 0 FAIL** (dont l'oracle cold vs warm) |
| LSP (unitaire) | `cargo test -p ravum-lang-lsp` | **13 PASS / 0 FAIL** |
| **VS Code réel** | `node tools/vscode-ravum/test/runTest.js` | **10 tests / 10 PASS**, VS Code **1.145.2** (rejoué après les correctifs) |
| Conformance | `cargo run -p ravum-conformance -- run` | **321 cas : 320 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** |
| Traçabilité des règles | `python tools/features_coverage.py --strict` | **187/188** règles tracées, **179/188** au critère strict |
| Fuzz lexer/parser | `RAVUM_FUZZ_COLLECT=1 RAVUM_FUZZ_ITERS=200000 cargo test -p ravum-lang-syntax --test fuzz_lex_parse -- --ignored` | **249 054 entrées, 0 échec**, digest `0xcb1740aef51d2879`, 61,4 s (**rejoué après les 12 correctifs**) |
| Fuzz sema | `RAVUM_FUZZ_ITERS=100000 cargo test -p ravum-lang-sema --test fuzz_check fuzz_check_long -- --ignored` | **100 000 itérations, 0 échec**, 1 591 it/s, 62,9 s (**rejoué après les correctifs**) |
| Smoke exemples | `pwsh -File scripts/lang-smoke.ps1 -SkipBuild` | **31 PASS / 0 FAIL / 0 NOT_RUN** (rejoué après les 14 correctifs) |
| SBOM | `$env:SOURCE_DATE_EPOCH='0'; pwsh -File scripts/sbom.ps1` | 64 paquets, 170 relations, **digest stable pour un arbre donné : `057f006e…492b`** (deux runs identiques) |
| **Registre des codes** | `python tools/registry_audit.py` | **112 codes enregistrés ; 1 conflit documenté** (`W-TYP-UNREACHABLE`, interdit par DIAG-1.6) |
| **Build bit-à-bit** | `pwsh -File scripts/repro-check.ps1` | **deux binaires identiques**, `sha256 = 3B606F6B…9E9B` (racines et `target/` différents, après les 14 correctifs) |
| Formatage | `cargo fmt --all -- --check` | exit 0 |
| **Performance** | `pwsh -File scripts/lang-bench.ps1 -Runs 3 -Sizes 100,500,2000` | `check` **38,7 / 86,4 / 296,6 ms** et **9,1 / 20,1 / 60,4 Mo** pour 100 / 500 / 2 000 fonctions (profil `dev`) |

Les deux SKIP de conformance sont **par plateforme** : Windows saute
`security/dependency-absolute-unix`, Linux saute les deux cas spécifiques
Windows. Chaque cas tourne sur au moins un OS, aucun sur les deux.

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

## 2. Matrice

| Gate | État | Ce qui est prouvé | Ce qui manque / échoue | Blocage |
|---|---|---|---|---|
| T0 | `PASS` | Corpus de sécurité du socle vert (overlays, publication Git, reprise après crash, journal, concurrence, symlinks, chemins hostiles) | — | — |
| T1 | `PARTIAL` | Voies A et B réelles (`check→build→run`), parité A/B mesurée | **Voie C non implémentée** ; pas de parité A/B/C ; choix final non ratifié | Décision humaine (ADR) |
| T2 | `PARTIAL` | 313 cas **312 PASS / 0 FAIL / 1 SKIP** ; **179/188** règles au critère strict ; 17 codes interdits éliminés (13 + 4) | **27 codes de langage émis hors du registre §3** ; 9 règles non satisfaites (5 par travail de cas, 2 par défaut compilateur, 1 incohérence de spec, 1 impossible) | Travail de cas et de code — aucune décision requise |
| T3 | `PASS` | Lexer/parser/AST réels, **1020 tests**, **fuzz complet rejoué (2 campagnes, 0 échec)**, **Miri rejoué (159 tests)**, formateur idempotent, **5 défauts frontend corrigés** (`effects`, `pub test fn`, boucles-valeurs, cascade `lifetime`, place d'affectation) | — (les 16 divergences restantes sont des **identifiants de diagnostic**, pas des comportements : traitées sous T2/D8) | — |
| T4 | `BLOCKED` | Typechecker, borrow checker, 85 cas adversariaux internes | **Audit externe humain absent** | Audit externe |
| T5 | `PASS` | Binaires natifs réels Windows (smoke 31 PASS) et Linux (CI antérieure) ; WASI exécuté | ABI stable non revendiquée ; sanitizers non exécutés | — |
| T6 | `PARTIAL` | Stdlib core/io/fs/string/vec/math + FFI minimale | Threads, réseau, async explicitement `UNSUPPORTED` | — |
| T7 | `PASS` | CLI complète ; **LSP réel** (10 tests tubes) ; **session VS Code réelle 10/10** (diagnostics, survol, définition, références, renommage, complétion, symboles, formatage) | — | — |
| T8 | `PARTIAL` | `watch` = rebuild + redémarrage ; overlays/snapshots ; **oracle cold vs warm** : 36 éditions incrémentales comparées à une analyse froide, arbre de symboles identique à chaque lot | **Pas de préservation d'état** ; pas d'ABI de patch ni de safepoint | Implémentation |
| T9 | `PARTIAL` | 9 verticales `.rv` exécutées | HTTP + SQLite, DSP hôte, E2E navigateur absents | Implémentation |
| T10 | `PARTIAL` | 1019 tests, conformance, smoke, **fuzz complet rejoué**, **Miri rejoué en CI sur ce SHA (159 tests, 0 échec)** | **Audit externe absent** | Audit externe |
| T11 | `PARTIAL` | Installateur, SBOM **reproductible**, provenance, checksums, licence, **build bit-à-bit identique entre deux racines et deux `target/`**, **mécanisme de signature OpenSSH qualifié (accepte l'intact, rejette l'altéré)** | **Aucune clé de projet ni ancre de confiance** | Décision humaine **D9** (custody de clé) |
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

### T2 — Spécification formelle — `PARTIAL`

Passe de traçabilité complète dans
[`docs/qualification/T2-FINDINGS.md`](../qualification/T2-FINDINGS.md).

- **Avant** : 99/188 règles citées par un cas, 107 cas, couverture non mesurée.
- **Après** : **268 cas**, **187/188** règles tracées vers un cas existant,
  **87/188** satisfaisant le critère strict (cas d'acceptation + cas de refus
  portant le code exigé).
- **Aucun cas n'échoue** après l'alignement des codes sur le registre (§9 de
  `T2-FINDINGS.md`), chacun documenté : 13 codes sont **émis par le
  compilateur sans être documentés nulle part** (`E-OWN-MOVE-INDEXED`,
  `E-UNSUPPORTED-TURBOFISH`, …), 4 sont des divergences de nom sur des codes
  pourtant documentés, 2 sont des défauts de comportement.
- **1 règle non couvrable** : `F-META-006` n'a ni forme acceptée ni refus
  exécutable (MACH-1.4 fixe l'interpréteur à `NOT_IMPLEMENTED`).
- **Aucun cas n'a été assoupli** : `expect_codes` vient de la spec, jamais de la
  sortie du compilateur.
- **Codes** : les 13 codes émis hors registre ont été remplacés par les codes
  canoniques (`diagnostics.md` §3 : « tout code absent est interdit »). Le
  corpus est vert ; il reste à écrire les tests négatifs manquants (§9).

### T3 — Frontend — `FAIL`

- **Prouvé** : lexer/parser/AST réels, récupération, garde-fous, formateur
  idempotent, corpus négatif, interfaces LSP effectives.
- **Fuzz complet rejoué après les correctifs** : 249 054 entrées lexer/parser
  (**0 échec**, digest `0xcb1740aef51d2879`) et 100 000 itérations `sema`
  (**0 échec**). Le digest a changé par rapport à la campagne précédente
  (`0x9ca81d63…`), ce qui est attendu — les correctifs modifient quels programmes
  sont acceptés — et confirme que l'empreinte est bien liée au compilateur, pas
  une constante. Les deux bugs de formateur trouvés par la campagne initiale
  restent couverts par des tests de régression.
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
- **Pourquoi T3 passe à `PASS`** : les 16 cas de conformance qui échouent encore
  refusent **exactement** les programmes que la spécification refuse ; seul
  l'identifiant du diagnostic diffère (et 13 de ces identifiants ne sont pas
  enregistrés). C'est un écart de *contrat de diagnostic*, mesuré et suivi par T2
  et le travail de cas de T2, pas un défaut de frontend. Aucun programme n'est accepté à
  tort ni refusé à tort.

### T4 — Sécurité mémoire, ownership, effets — `BLOCKED`

Inchangé : le contrat exige un **auditeur humain indépendant**. Un audit mené
par un autre agent reste un audit interne. Dossier prêt pour l'auditeur :
`docs/audit/`, `docs/reports/`, `conformance/cases/ownership/`,
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

### T7 — Environnement de développement — `PASS`

Preuve : [`docs/qualification/VSCODE.md`](../qualification/VSCODE.md).

- Extension réelle `tools/vscode-ravum` (grammaire TextMate, serveur `ravum-lang
  lsp` en stdio), **10 tests d'intégration exécutés dans un VS Code réel**
  (1.145.2), **10 PASS / 0 FAIL** — **rejoué après les correctifs du compilateur**.
- Assertions via l'API publique `vscode` : langue, absence de faux positif,
  symboles, complétion, survol, définition, références, formatage, renommage
  multi-occurrences, diagnostic `E-*` publié.
- Deux défauts réels trouvés par ce test et corrigés côté serveur : navigation
  impossible depuis une déclaration, et `textDocument/prepareRename` absent (ce
  qui faisait échouer le renommage VS Code même avec une édition valide).

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
différents** produisent le même `sha256` (`3B606F6B…9E9B`, valeur liée à cet arbre) avec
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
| D4 | Audit externe (T4/T10/T12) | **OUVERTE** — bloque T4 et T12 |
| D5 | Périmètre Tier 1 | **PARTIELLE** : Windows local, Linux sur CI |
| D6 | Périmètre des fonctionnalités universelles | **OUVERTE** |
| D7 | Ratification de la frontière `unsafe`/FFI | **OUVERTE** |
| **D8** | Arbitrage des codes de diagnostic | **RÉSOLUE** — le registre (`diagnostics.md` §3) déclare « tout code absent est interdit » ; les 13 codes fautifs ont été remplacés |
| **D9** | **Custody de la clé de signature et publication de `allowed_signers`** | **NOUVELLE — bloque la signature de release (T11)** |

## 5. Règles de mise à jour

- Un changement d'état exige une commande, une sortie et un SHA.
- Un test ignoré ou non exécuté n'est jamais compté comme réussi.
- Un `SKIP` lié à l'OS est exécuté sur l'autre plateforme avant d'être coché.
- Aucun `PASS` obtenu en modifiant la définition du gate.
- **Un gate peut régresser** : `T3` est passé de `PARTIAL` à `FAIL` parce que la
  passe T2 a produit des contre-exemples exécutables. Une information nouvelle
  qui dégrade un état est un progrès de mesure, pas une régression de qualité.
