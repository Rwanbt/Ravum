# Ravum Core 1.0 — registre unique des gates T0–T12

> **Registre de suivi, pas un certificat.** Un seul tableau fait foi pour l'état
> des gates. Aucun `PASS` n'y est écrit sans preuve exécutée, liée à un SHA et
> reproductible par la commande citée.
>
> États autorisés : `PASS`, `PARTIAL`, `FAIL`, `BLOCKED`, `NOT_RUN`.
> `SKIP` et `NOT_RUN` ne sont **jamais** convertis en `PASS`. Un `BLOCKED`
> n'est jamais converti automatiquement en `PASS`.
>
> Dernière mise à jour : 2026-11-09 (session LSP + réconciliation licence).

## 0. Base de référence

| Élément | Valeur |
|---|---|
| Branche de travail | `feat/lang-core-1.0-rc` |
| Baseline au début de la session | `16d4e5c475a613b4b0142d2cc08c635ee63ed42c` |
| **SHA des preuves locales (§1)** | `8be7913d69dee77ba37d04b219919e894481f21e` |
| **SHA des preuves CI (§1 bis)** | `c43dafd` — 4 workflows verts sur ce même commit |
| `dev` fusionné dans la branche | `fbaa5068af39d5dfbcb27af4aaf378ff37ec2e7b` (PR #103, licence) |
| `main` (intacte) | `ca675497d1f8d74cbb7a93d12715a845149a8785` |
| PR du langage | [#101](https://github.com/Rwanbt/Ravum/pull/101) (draft, base `dev`) |
| Licence | `MIT OR Apache-2.0`, 23/23 membres du workspace |
| Toolchain | Windows 11 x86-64, `rustc 1.95.0`, `cargo 1.95.0` |

Les chiffres de la §1 ont été **réexécutés dans cette session** sur la baseline
ci-dessus (avant le commit de session). Ceux marqués « antérieur » proviennent
d'un run lié à un SHA plus ancien et doivent être rejoués avant tout `PASS`.

## 1. Mesures réexécutées dans cette session

| Mesure | Commande | Résultat exact |
|---|---|---|
| Tests workspace | `cargo test --workspace --all-targets -- --test-threads=2` | **1018 passés / 0 échec / 2 ignorés / 82 suites** (exit 0) |
| Conformance | `cargo run -p ravum-conformance -- run --json target/conformance-after-lsp.json` | **107 cas : 106 PASS / 0 FAIL / 1 SKIP** (exit 0) |
| Smoke exemples | `pwsh -File scripts/lang-smoke.ps1 -SkipBuild` | **31 PASS / 0 FAIL / 0 NOT_RUN** |
| SBOM | `pwsh -File scripts/sbom.ps1` | 64 paquets (23 workspace + 41 dépendances), 170 relations, `sha256=63f3faf0…e915f` |
| Licences déclarées | `cargo metadata --locked` + SBOM | **23/23 membres = `MIT OR Apache-2.0`** |
| Intégrité documentaire | `python tools/validate_repo_docs.py` | exit 0 après réconciliation |
| LSP (bout en bout) | `cargo test -p ravum-lang-cli --test lsp_client` | **9 PASS / 0 FAIL** (client LSP réel sur tubes) |

Le seul SKIP de conformance est `security/dependency-absolute-unix`, marqué
`skip_on: windows` : il est inapplicable sur l'hôte Windows et doit être exécuté
par le job Linux, pas coché depuis Windows.

## 1 bis. Mesures CI sur le même SHA (`c43dafd`)

Les quatre workflows sont verts sur ce commit : *MVP product checks*,
*MVP impact-selected checks*, *Documentation integrity*, *Linux qualification*.
Détail du run Linux
([42692673526](https://github.com/Rwanbt/Ravum/actions/runs/42692673526)) :

| Job (Linux, `ubuntu-latest`) | Résultat |
|---|---|
| Workspace tests (stable 1.95.0) | **82 suites, 1015 PASS / 0 FAIL / 4 ignorés** |
| Conformance corpus (Linux) | **total=107 pass=105 fail=0 skip=2 not_run=0** |
| Example smoke suite (Linux) | success (binaires natifs Linux réellement construits et exécutés) |
| Installer contract (temporary prefix, `--version`) | success |
| `wasm32-wasip1` (build + run via Node WASI) | success — `hello, ravum` observé |
| Fuzz smoke (tests `fuzz_*` ignorés, soft guard) | success |
| Miri (nightly, syntax + sema) | **65 + 93 + 1 PASS / 0 FAIL** |
| ThreadSanitizer | **SKIP explicite — `NOT_APPLICABLE`** (pas de threads/async en Core 1.0) |

**Lecture honnête des SKIP de conformance** : Windows saute
`dependency-absolute-unix` (1 SKIP) et Linux saute les deux cas spécifiques
Windows (2 SKIP). **Chaque cas du corpus est donc exécuté sur au moins une
plateforme**, mais un cas donné ne l'est pas sur les deux. Ce n'est ni un `PASS`
universel, ni une règle non couverte : c'est une couverture par plateforme.

**Ce que la CI ne prouve pas** : la correction sémantique de programmes
arbitraires, les performances, le temps réel, une ABI stable, macOS, ARM, ni la
reproductibilité bit-à-bit.

## 2. Matrice

| Gate | État | Ce qui est prouvé | Ce qui manque | Blocage |
|---|---|---|---|---|
| T0 | `PASS` | Corpus de sécurité du socle vert (overlays, publication Git, reprise après crash, journal, concurrence, symlinks, chemins hostiles) dans les 1018 tests | — | — |
| T1 | `PARTIAL` | Voie A et voie B réelles (`check→build→run`), parité A/B mesurée | **Voie C non implémentée** ; aucune mesure de parité A/B/C ; choix final non documenté/ratifié | Décision humaine de ratification (ADR) |
| T2 | `PARTIAL` | 15 documents de spécification + 107 cas de conformance traçables | `features.md` incomplet ; couverture traçable règle→test non exhaustive ; contradictions résiduelles | — |
| T3 | `PARTIAL` | Frontend réel, 1018 tests verts, corpus négatif, **interfaces LSP livrées et testées**, **Miri rejoué (159 tests)** sur ce SHA | Campagne **fuzz complète** non rejouée (seul un *fuzz smoke* a tourné) ; limites profondeur/mémoire non re-mesurées | — |
| T4 | `BLOCKED` | Typechecker + borrow checker + 85 cas d'audit adversarial interne ; finding CRITICAL `&"a"` couvert | **Audit externe humain absent** (exigé par le contrat) | Audit externe |
| T5 | `PASS` | Binaires natifs réels **Windows** (smoke 31 PASS) et **Linux** (CI verte : workspace + conformance + smoke) sur le **même SHA** ; WASM exécuté ; ABI stable non revendiquée | Sanitizers non exécutés (documenté) | — |
| T6 | `PARTIAL` | Stdlib core/io/fs/string/vec/math + FFI minimale `extern "C"`/`unsafe` | Threads, réseau, async, temps réel dur explicitement `UNSUPPORTED` | — |
| T7 | `PARTIAL` | CLI complète ; **LSP réel** (diagnostics, hover, definition, references, rename, completion, symboles, formatage) testé par client réel | **Aucune session VS Code réelle** ; pas de registre de paquets distant | — |
| T8 | `PARTIAL` | `watch` = rebuild + redémarrage ; overlays et snapshots du socle disponibles | **Pas de préservation d'état** ; compatibilité de patch non qualifiée pour le langage | — |
| T9 | `PARTIAL` | 9 verticales `.rv` exécutées (dont FFI et WASM) | HTTP + SQLite, DSP hôte réel, E2E navigateur absents | — |
| T10 | `PARTIAL` | 1018 tests, conformance, smoke, **Miri rejoué sur ce SHA (159 PASS)** ; TSan explicitement `NOT_APPLICABLE` ; 2 audits internes antérieurs | Campagne **fuzz complète** non rejouée sur ce SHA ; **audit externe absent** | Audit externe |
| T11 | `PARTIAL` | Installateur, SBOM déterministe, provenance, checksums, **licence réconciliée** | **Pas de signature/attestation**, **pas de build bit-à-bit** | Clé de signature |
| T12 | `BLOCKED` | Dossier de RC interne, PR, rapports, matrice | Dogfooding indépendant, Go/No-Go humain | Décision humaine |

## 3. Détail par gate

### T0 — Fondations et sécurité du socle — `PASS`

- **Exigences d'origine** : overlays, confinement de chemins, symlinks,
  propagation d'erreurs, snapshots, publication Git, restauration après crash,
  caches, invalidations, coordination concurrente.
- **Prérequis** : aucun.
- **Preuves (cette session)** : suites `overlay` (17), `git_publication` (16),
  `git_publication_crash` (15), `journal_durability` (3), `cycle_integration`
  (14), `merge_strategies` (2), `transient_verification` (3) — toutes vertes.
  Le scénario `publication → snapshot → même fichier → publication` est couvert
  par `overlay::publication_starts_a_new_cycle_and_the_same_path_is_no_longer_a_false_conflict`.
- **Tests négatifs** : chemins relatifs hostiles, symlinks, `git` metadata
  protégée, hook `pre-commit` refusant, journal non inscriptible.
- **Risques restants** : aucun défaut bloquant connu ; corpus fini.
- **Owner** : socle d'orchestration (agents 8/12).

### T1 — Architecture du compilateur et bootstrap — `PARTIAL`

- **Prouvé** : voie B (Rust généré + `rustc`) réellement utilisée par
  `build`/`run`/`test` ; voie A (outils Rust) opérationnelle ; parité A/B
  antérieure `MATCH` sur 4 programmes (`scripts/m5-bench.ps1`).
- **Absent** : **voie C** (backend natif direct, Cranelift ou alternative).
- **Conséquence** : le gate M5 ne peut pas être `PASS` : la comparaison A/B/C
  exigée n'existe pas. `NOT_IMPLEMENTED` n'est pas `PASS`.
- **Tests requis** : prototype C mesurable sur un périmètre explicite, parité
  des sorties, temps d'analyse/codegen/link, taille binaire, mémoire.
- **Décision humaine** : ratification architecturale du choix final (ADR).

### T2 — Spécification formelle — `PARTIAL`

- **Prouvé** : `spec/language/` (15 documents) ; 107 cas dont 106 PASS sur ce
  SHA ; codes de diagnostic stables réutilisés par le LSP.
- **Couverture mesurée** : `spec/language/features.md` contient **188 règles**,
  dont **99 référencent un cas de conformance** et **92 n'ont aucune référence de
  test**. C'est le déficit précis à combler pour `PASS`, pas une impression.
- **Absent** : couverture traçable exhaustive règle → test positif + tests
  négatifs + code + diagnostic.
- **Contradiction vérifiée comme déjà résolue** : `unsafe` et `extern "C"` ne
  sont **pas** exclus de la spécification. `unsupported.md` les liste comme
  supportés (bloc `unsafe`, `extern "C" { … }`, primitives FFI-sûres) et
  `features.md` trace `F-EFF-009`, `F-FFI-001`, `F-FFI-002` vers
  `cases/run/ffi-abs`, `cases/types/ffi-outside-unsafe`, `cases/types/ffi-bad-type`.
  La divergence signalée dans le mandat ne se reproduit donc pas sur ce SHA.
- **Incohérence spec↔implémentation vérifiée puis corrigée** : le cas
  `security/dependency-absolute-unix` portait une note affirmant que
  `diagnostics.md` §3.3 ne listait que `E-MOD-PATH`. Vérification faite, la
  spécification documente **les deux** codes (`E-MOD-PATH` : forme du chemin,
  lien symbolique ; `E-MOD-PATH-ESCAPE` : chemin absolu, `..`, profondeur), et
  `Q-DIAG-1` enregistre `E-MOD-PATH-ESCAPE` comme canonique. La note du cas
  était donc périmée et a été **corrigée** ; ce n'est pas un test modifié pour
  obtenir un vert, seulement une annotation alignée sur la spec et le code.
- **Décision humaine** : ratification des choix normatifs.

### T3 — Frontend — `PARTIAL`

- **Prouvé** : lexer/parser réels, récupération, garde-fous, formateur
  idempotent, corpus négatif exécuté, **interfaces LSP effectives** (le LSP
  consomme les mêmes diagnostics).
- **Rejoué sur ce SHA** : **Miri** (65 tests syntax + 93 sema + 1 fuzz court,
  0 échec) via le job Linux.
- **Absent** : la **campagne fuzz complète** (chiffres antérieurs : 249 054
  entrées, 0 invariant violé) n'a pas été rejouée ; seul un *fuzz smoke* a
  tourné. Les limites de profondeur/mémoire n'ont pas été re-mesurées.
- **Pour passer `PASS`** : rejouer la campagne fuzz complète sur le SHA final et
  joindre les sorties brutes.

### T4 — Sécurité mémoire, ownership, effets — `BLOCKED`

- **Prouvé** : typechecker, moves/moves partiels, prêts, non-échappement, drop,
  effets transitifs, profils ; 85 cas d'audit adversarial interne ; régression
  permanente du finding critique `&"a"`.
- **Manquant** : **audit externe humain**. Le contrat est explicite : un audit
  mené par un autre agent reste un audit interne. Sans auditeur réellement
  indépendant, ce gate ne peut pas être `PASS`, même si tous les tests locaux
  sont verts.
- **Dossier prêt pour l'auditeur** : `docs/audit/`, `docs/reports/`,
  `conformance/cases/ownership/`, `crates/ravum-lang-sema/tests/own_audit.rs`.

### T5 — Codegen, ABI, exécution native — `PASS`

- **Prouvé (Windows, cette session)** : 31 PASS sur `check/build/run` réels,
  binaires natifs exécutés, FFI `abs` exécutée.
- **Prouvé (Linux, CI sur le même SHA)** : workspace, conformance, smoke
  (binaires natifs Linux construits et exécutés), installateur et WASI verts.
- **Portée** : le `PASS` couvre **Windows x86-64 et Linux x86-64** sur un
  périmètre explicite (le corpus et les exemples), pas une garantie universelle.
- **Explicitement non revendiqué** : ABI stable, macOS, ARM, sanitizers.
- **Preuve** : `docs/reports/LANGUAGE-CORE-STATUS.md` +
  run [42692673526](https://github.com/Rwanbt/Ravum/actions/runs/42692673526).

### T6 — Runtime, stdlib, FFI — `PARTIAL`

- **Prouvé** : API core/io/fs/string/vec/math ; FFI minimale (`extern "C"`,
  `unsafe`, types primitifs, effets `ffi`/`unsafe`, refus hors `unsafe`).
- **Explicitement `UNSUPPORTED`** : threads, réseau, async, temps réel dur.
  Ces exclusions correspondent à la spécification : elles ne sont pas un
  périmètre réduit inventé pour obtenir un vert.
- **Manquant pour `PASS`** : les exigences T6 obligatoires non couvertes
  (réseau, concurrence, capacités système) restent absentes.

### T7 — Environnement de développement — `PARTIAL`

- **Prouvé** : `init`, `check`, `build`, `run`, `test`, `fmt`, `lint`, `doc`,
  `cache`, `doctor`, `watch`, **`lsp`** ; packages locaux, lockfile, validation
  de chemins.
- **Nouveau cette session** : serveur LSP réel, qualifié par un client LSP
  écrit à la main (9 tests) — voir
  [`docs/qualification/LSP.md`](../qualification/LSP.md).
- **Manquant** : session VS Code réelle ; registre de paquets distant
  (non annoncé, donc non requis).
- **Pour passer `PASS`** : démontrer le parcours complet dans un IDE réel et
  le documenter avec captures/commandes.

### T8 — Compilation continue et hot reload — `PARTIAL`

- **Prouvé** : `watch` recompile et redémarre ; overlays/snapshots du socle
  disponibles.
- **Absent** : préservation d'état, contrôle de compatibilité de patch,
  activation atomique avec lecteurs concurrents **pour le langage**.
- **Position honnête** : un redémarrage automatique ne satisfait pas un gate qui
  exige la préservation d'état. T8 reste `PARTIAL` jusqu'à implémentation réelle.

### T9 — Applications réelles `.rv` — `PARTIAL`

- **Prouvé** : 9 verticales exécutées (`hello`, `fib`, `calculator`,
  `file-tool`, `stats`, `dsp-gain-iir`, `package-demo`, `tests-demo`,
  `ffi-abs`).
- **Absent** : service HTTP + SQLite, DSP dans un hôte réel, E2E navigateur.
- **Point de vigilance** : l'application full-stack Rust du MVP-0b **n'est pas**
  une preuve que Ravum possède une base de données ou un serveur HTTP.

### T10 — Qualification intensive et audits — `PARTIAL`

- **Prouvé** : 1018 tests (Windows) / 1015 (Linux), conformance 107, smoke 31,
  **Miri rejoué sur ce SHA (159 tests PASS)**, audits internes antérieurs
  (borrow checker 85 cas, backend 21 cas).
- **Absent** : la **campagne fuzz complète** n'a pas été rejouée sur ce SHA
  (seul un *fuzz smoke* a tourné) ; **audit externe** absent.
- **TSan** : `NOT_APPLICABLE` **parce que** Core 1.0 n'expose ni threads ni
  async — la justification est écrite et le job est explicitement SKIP, pas
  supprimé.
- **Interdit** : marquer `NOT_APPLICABLE` un outil simplement difficile à
  exécuter.

### T11 — Performance, distribution, supply chain — `PARTIAL`

- **Prouvé** : SBOM SPDX-2.3 déterministe (64 paquets, 170 relations),
  checksums, manifeste de provenance, **licence `MIT OR Apache-2.0` réconciliée
  sur les 23 membres**, **installateur vert sur Linux (CI, ce SHA)** et antérieur
  sur Windows.
- **Absent** : signature/attestation (aucune clé disponible), reproductibilité
  bit-à-bit démontrée, comparaison inter-machines.
- **Interdit** : inventer une signature ou une provenance.

### T12 — Dogfood, audit final, RC — `BLOCKED`

- **Prouvé** : dossier de RC interne (PR, rapports, matrice, preuves),
  décisions humaines explicitées.
- **Absent** : dogfooding par des agents n'ayant pas développé le compilateur ;
  Go/No-Go humain.
- **Verdict de maturité** : `CORE_RC_CANDIDATE` au mieux, **jamais** `RELEASED`.

## 4. Décisions humaines restantes

| ID | Sujet | État |
|---|---|---|
| D1 | Licence | **RÉSOLUE** : `MIT OR Apache-2.0` |
| D2 | Ratification des ADR / choix du backend | **OUVERTE** |
| D3 | Fusion dans `main` et publication | **OUVERTE** |
| D4 | Audit externe (T4/T10/T12) | **OUVERTE** — bloque T4 et T12 |
| D5 | Périmètre Tier 1 (Linux/autres OS) | **PARTIELLE** : Windows testé, Linux sur CI |
| D6 | Périmètre des fonctionnalités universelles | **OUVERTE** |
| D7 | Ratification de la frontière `unsafe`/FFI | **OUVERTE** |

## 5. Règles de mise à jour

- Un changement d'état exige une commande, une sortie et un SHA.
- Un test ignoré ou non exécuté n'est jamais compté comme réussi.
- Un `SKIP` lié à l'OS est exécuté sur l'autre plateforme avant d'être coché.
- Aucun `PASS` obtenu en modifiant la définition du gate.
