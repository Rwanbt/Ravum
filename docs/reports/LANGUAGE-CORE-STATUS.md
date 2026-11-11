# Ravum Core 1.0 — état réel du langage (snapshot de travail)

> **Mise à jour passe 6 — 2026-11-11, base `5d88ecb`.** Cette passe est une passe
> d'**implémentation** de fiabilité, pas une revue : contrat `check → build`
> mesuré et bloquant, **onze programmes acceptés par `check` mais refusés par
> `rustc` corrigés** (références stockées/retournées, moves après `unwrap`,
> tableaux annotés, exhaustivité imbriquée, moves d'éléments de tableau,
> `String → &str` implicite), identité de build déterministe et publication
> atomique, `watch` fail-closed, codes de sortie honnêtes, runner de tests
> complet, seconde métrique de traçabilité. Mesures : **94 suites, 1525 passés /
> 0 échec / 2 ignorés** ; conformance **424 cas, 423 PASS / 0 FAIL / 1 SKIP**,
> build vérifié **163/163** (101 vérifiés, 62 implicites, 0 échec),
> `build_verified: true` ; smoke **43/0/0** ; registre **108 codes / 126 émis /
> 0 écart** ; FeatureIds **188/188** ; **MUST 463/807** (nouvelle métrique —
> T2 repasse à `PARTIAL`). Gates : **4 PASS / 7 PARTIAL / 2 BLOCKED**
> (T4/T12 exigent l'audit externe et le Go/No-Go humains). CI **NOT_RUN** sur
> ces commits (aucun push). Le corps et le tableau de gates ci-dessous restent
> l'instantané du **2026-11-09** (`1b7b4f1`) : le registre canonique est
> [`CORE-1.0-GATE-MATRIX.md`](../roadmap/CORE-1.0-GATE-MATRIX.md).

> **Document de situation, pas un certificat.** Chaque chiffre est lié à une
> commande exécutée sur cette machine. Le registre canonique des gates est
> [`docs/roadmap/CORE-1.0-GATE-MATRIX.md`](../roadmap/CORE-1.0-GATE-MATRIX.md).
> Aucun `PROVED`, aucune maturité « production ».
> Dernière mesure : 2026-11-09 (passe T2 de traçabilité + session VS Code),
> Windows 11 x86-64, rustc 1.95.0, branche `feat/lang-core-1.0-rc` @ `1b7b4f1`.
> Tests workspace : **1020 passés / 0 échec / 2 ignorés / 82 suites**. Smoke des
> exemples : **31 PASS / 0 FAIL** (`scripts/lang-smoke.ps1 -SkipBuild`, rejoué après les correctifs).
> Conformance : **269 cas, 252 PASS / 16 FAIL / 1 SKIP** — les 16 échecs restants
> sont des divergences spec ↔ implémentation, documentées et non assouplies
> ([`docs/qualification/T2-FINDINGS.md`](../qualification/T2-FINDINGS.md)) ; six
> défauts frontend ont été corrigés dans le compilateur pendant la passe, et un cas neuf verrouille `min(-0.0, +0.0) = -0.0`.
> **T2 reste `FAIL`** (arbitrage humain D8 requis) et **T3 repasse à `PARTIAL`**.
> Licence : **`MIT OR Apache-2.0`** sur les 23 membres (SBOM reproductible à
> époque fixée : digest `21ee4d00…526db` sur deux runs identiques).
> **Serveur LSP réel** : 10 tests client bout en bout
> ([`docs/qualification/LSP.md`](../qualification/LSP.md)), et **session VS Code
> réelle 10/10** ([`docs/qualification/VSCODE.md`](../qualification/VSCODE.md)).
> **Linux rejoué sur le SHA final** (`c43dafd`, run
> [42692673526](https://github.com/Rwanbt/Ravum/actions/runs/42692673526)) :
> workspace **82 suites / 1015 PASS / 0 FAIL / 4 ignorés**, conformance
> **107 cas / 105 PASS / 0 FAIL / 2 SKIP** (cas Windows), smoke, installateur,
> WASI et **Miri (159 tests)** verts ; TSan explicitement `NOT_APPLICABLE`.

## 1. Ce qui fonctionne réellement (vérifié)

| Capacité | Commande | Résultat observé |
|---|---|---|
| Version/édition | `ravum-lang --version` | `ravum-lang 0.1.0 (edition core-1.0-draft.1)` |
| Diagnostic outillage | `ravum-lang doctor` | rustc 1.95.0 détecté, `parser=ready, checker=ready, codegen=ready`, verdict `ready` |
| Création de projet | `ravum-lang init <dir>` | `ravum.toml`, `src/main.rv`, `.gitignore` |
| Analyse | `ravum-lang check examples/hello --json` | exit 0, JSON `schema_version:1`, effets inférés (`main: io`) |
| Compilation native | `ravum-lang build examples/hello` | `.ravum/gen/main.rs` + `.ravum/bin/hello.exe` |
| Exécution | `ravum-lang run examples/hello` | `hello, ravum`, exit 0 |
| Récursion/casts | `ravum-lang build/run examples/fib` | `0 1 1 2 3 5 8 13 21 34 55` |
| Args + f64 + Result + `?` | `calculator 5 + 3` | `8` |
| Fichier + lignes + parse | `stats sample.txt` | `2.5` |
| Formatter | `ravum-lang fmt --check examples/hello` | 0 fichier à reformater, exit 0 |
| Échec typé JSON | `check --json` sur type mismatch | exit 1 + `E-TYP-MISMATCH` |
| Tests unitaires | `ravum-lang test examples/tests-demo` | `3 passed; 0 failed` (dont un test `-> Result`) |
| Conformance | `cargo run -p ravum-conformance -- run` | 107 cas, 106 PASS / 0 FAIL / 1 SKIP (voir §3) |
| FFI minimale | `ravum-lang run examples/ffi-abs` | `extern "C"` + `unsafe { }`, `abs(-42)` → `42` |
| Hot reload | `ravum-lang watch --run` | rebuild + redémarrage sur modification (v1→v2), échec non fatal |
| Cible WASM | `ravum-lang run --target wasm32-wasip1 examples/hello` | `hello, ravum` via Node WASI preview1 |
| **Serveur LSP** | `ravum-lang lsp` | JSON-RPC sur stdio ; diagnostics, hover, definition, references, rename, completion, symboles, formatage (**9 tests client réels**) |
| M5 | `scripts/m5-bench.ps1` | A/B parité MATCH sur 4 programmes, B +12 à +31 % de compilation, C `NOT_IMPLEMENTED` |

## 2. Composants livrés dans cette itération

- `crates/ravum-lang-syntax` : lexer, AST + `NodeId` globaux, parser récursif
  descendant (récupération, garde-fous profondeur/tokens), formateur idempotent
  préservant commentaires et lignes vides. **69 tests** ; fuzzing déterministe
  (249 054 entrées, 0 invariant violé après correctifs FZ-1/FZ-2).
- `crates/ravum-lang-sema` : résolution de modules/visibilité, table builtins
  (stdlib), typechecker, exhaustivité `match`, inférence d'effets transitive,
  profils `core`/`pure`/`rt`, **borrow checker conservateur** (init, moves,
  move partiel suivi, conflits de prêts, non-échappement, drop, provenance
  `&'static str` interprocédurale). **133 tests unitaires + 85 tests d'audit
  adversarial actifs + 5 tests FFI**.
- `crates/ravum-lang-codegen` : fichier Rust autonome et auditable
  (`#![forbid(unsafe_code)]` sans `extern`, `#![deny(unsafe_code)]` + allow
  local des wrappers avec FFI), prélude safe, runner de tests, déterminisme.
  **29 tests unitaires + 21 tests d'audit backend** (dont exécutions rustc
  réelles).
- `crates/ravum-lang-lsp` : **serveur de langage réel** (JSON-RPC 2.0 sur
  stdio, base protocol LSP, synchronisation incrémentale, diagnostics sur buffers
  non sauvegardés, hover, definition, references, rename, completion, document
  et workspace symbols, formatage). Le loader accepte des **overlays en mémoire**
  (`LoadOptions::overrides`) : le buffer est analysé sans jamais écrire le
  fichier de l'utilisateur. **9 tests client bout en bout** + tests unitaires de
  positions UTF-16 / URI / changements incrémentaux.
- `crates/ravum-lang-cli` : `ravum-lang` (`init`, `check`, `build`, `run`,
  `watch`/`dev`, `test`, `fmt`, `lint`, `doc`, `cache`, `doctor`, `lsp`,
  `--version`), cible `wasm32-wasip1`, loader de packages (`ravum.toml`,
  modules, lockfile/hash, dépendances locales, chemins validés), JSON stable.
  **57 tests** (14 unitaires + 43 intégration).
- `tools/conformance` + `conformance/` : harnais indépendant (std only) et
  **107 cas** déclaratifs (`compile-pass`, `compile-fail` à codes exacts,
  `run-pass`, `run-fail`, `fmt`, `cli`, sécurité, FFI, ownership).
- `spec/language/` : 15 documents DRAFT, mis à jour pour la frontière FFI
  (`extern "C"`, `unsafe`, effets `unsafe`/`ffi`, codes `E-FFI-TYPE`,
  `E-UNSAFE-CONTEXT`, `E-UNSUPPORTED-ABI`).
- `examples/` : hello, fib, calculator, file-tool, stats, dsp-gain-iir,
  package-demo (util+app), tests-demo, **ffi-abs**.
- Outillage de qualification : `scripts/install.{ps1,sh}` + manifeste SHA-256,
  `tools/sbom/sbom.mjs` (SPDX-2.3, 63 composants, déterministe),
  `scripts/lang-smoke.{ps1,sh}`, `.github/workflows/linux-qual.yml`,
  `docs/qualification/`, `docs/audit/`, `docs/release/`.

## 3. Conformance — chiffres bruts et honnêtes

Dernière exécution complète :
`cargo run -p ravum-conformance -- run --json target/conformance-final.json` →
**107 cas : 106 PASS, 0 FAIL, 1 SKIP, 0 NOT_RUN** (exit 0 ; le seul non-pass
est le cas Unix `skip_on: windows`, inapplicable sur l'hôte Windows).

Aucun FAIL n'est masqué. Le SKIP est un cas de sécurité dépendant des
privilèges de symlink, explicitement marqué `skip_on: windows` ; il sera
exécuté par le job Linux.

## 4. Tests, fuzz, Miri, audits (preuves liées à cet état)

| Campagne | Commande | Résultat exact |
|---|---|---|
| Workspace complet | `cargo test --workspace --all-targets -- --test-threads=2` | **1018 passés / 0 échec / 2 ignorés / 82 suites** (2026-11-09) |
| Client LSP bout en bout | `cargo test -p ravum-lang-cli --test lsp_client` | **9 passés / 0 échec** (initialize, diagnostics buffer non sauvegardé, édition incrémentale, hover, definition, references, rename, symboles/completion/formatage, méthode inconnue) |
| Conformance | `cargo run -p ravum-conformance -- run --json target/conformance-after-lsp.json` | **107 cas : 106 PASS, 0 FAIL, 1 SKIP** |
| Smoke | `pwsh -File scripts/lang-smoke.ps1 -SkipBuild` | **31 PASS / 0 FAIL / 0 NOT_RUN** |
| SBOM | `pwsh -File scripts/sbom.ps1` | 64 paquets, 170 relations, 23/23 membres `MIT OR Apache-2.0` |
| Fuzz lexer/parser (**antérieur**) | `RAVUM_FUZZ_COLLECT=1 RAVUM_FUZZ_ITERS=200000 cargo test -p ravum-lang-syntax --test fuzz_lex_parse -- --ignored` | 249 054 entrées, 5 216 641 tokens, **0 échec** — à rejouer sur le SHA final |
| Fuzz frontend sema (**antérieur**) | `RAVUM_FUZZ_ITERS=100000 cargo test -p ravum-lang-sema --test fuzz_check -- --ignored` | 120 300 itérations cumulées, **0 panic / 0 diagnostic vide / 0 non-déterminisme** — à rejouer |
| Miri (**antérieur**) | `MIRIFLAGS=-Zmiri-disable-isolation cargo +nightly miri test -p ravum-lang-syntax -p ravum-lang-sema --lib --tests` | **153 tests PASS** — à rejouer |
| Audit borrow checker (**antérieur**) | `cargo test -p ravum-lang-sema --test own_audit` | **85 passés / 0 échec** |
| Audit backend (**antérieur**) | `cargo test -p ravum-lang-codegen --test backend_audit` | **21 passés** |
| Installateur (**antérieur**) | `scripts/install.ps1 -Prefix <tmp>` | install/uninstall réels, hash manifeste = hash binaire |
| SBOM (**antérieur**) | `scripts/sbom.ps1` / `sbom.sh` | SPDX-2.3, déterministe, SHA-256 identique entre runs |

Les lignes marquées **antérieur** ne sont pas des preuves de cette session : elles
restent liées à un SHA plus ancien et doivent être rejouées avant tout `PASS`
(voir la [matrice](../roadmap/CORE-1.0-GATE-MATRIX.md)).

## 5. Gates (état honnête)

| Gate | État | Preuve / limite |
|---|---|---|
| T0 | **PASS** | baseline 511/0/80 sur `5408d9b` ; 4 défauts P0 corrigés, 208 tests ; workspace final 993/0 |
| T1 | **PARTIAL** | chaîne B réelle (`check→build→run`) ; M5 A/B mesuré et parité MATCH ; **C `NOT_IMPLEMENTED`**, décision non ratifiée |
| T2 | **PARTIAL** | spec DRAFT 15 fichiers + conformance 106/107 (1 skip OS) ; `features.md` : 188 règles dont 99 tracées vers un cas, **92 sans référence de test** ; aucun PASS revendiqué |
| T3 | **PARTIAL** | 69 tests syntaxe, corpus négatif ; interfaces LSP livrées et testées ; fuzz/Miri à rejouer sur le SHA final |
| T4 | **PARTIAL** | typechecker + borrow checker testés (133 + 85 cas), finding CRITICAL corrigé ; **pas d'audit externe**, pas de preuve formelle |
| T5 | **PASS** | binaires natifs réels **Windows** (smoke 31 PASS) et **Linux** (CI verte sur le même SHA) ; FFI `abs` exécutée ; WASI vert ; pas d'ABI stable revendiquée |
| T6 | **PARTIAL** | stdlib core/io/fs/string/vec/math + **FFI minimale** (`extern "C"`, primitives, `unsafe`) ; threads/net/async `UNSUPPORTED` |
| T7 | **PARTIAL** | CLI complète (`watch` inclus), WASM WASI preview1, **serveur LSP réel** (9 tests client) ; manque une session VS Code réelle |
| T8 | **PARTIAL** | hot reload = rebuild + redémarrage (pas de préservation d'état) ; collaboration/overlays non qualifiés ici |
| T9 | **PARTIAL** | 9 verticales `.rv` exécutées (dont FFI et WASM) ; HTTP/SQLite/GPU `NOT_RUN`/`UNSUPPORTED` |
| T10 | **PARTIAL** | fuzz + Miri + audits internes exécutés sur Windows **et** Linux ; **TSan `NOT_RUN`/`NOT_APPLICABLE`**, audit externe absent |
| T11 | **PARTIAL** | installateur, SBOM régénéré, provenance, checksums, **licence `MIT OR Apache-2.0` sur 23/23 membres** ; **pas de signature**, **pas de build bit-à-bit** |
| T12 | **PARTIAL** | PR/rapports/décisions prêts ; dogfood indépendant et Go/No-Go humain absents |

## 6. Limites structurelles à ne pas oublier

- Le backend de référence est **B** (Rust généré) : la sûreté mémoire du code
  safe est déléguée à un sous-ensemble Rust vérifié *après* le checker Ravum, pas
  prouvée formellement par Ravum.
- Le modèle d'emprunt est volontairement conservateur : pas de références
  stockées/retournées en général, pas de lifetimes explicites.
- Le **LSP est fonctionnel mais partiel** : pas de code actions, pas de
  semantic tokens, pas de signature help ; `references`/`rename` sur une locale
  refusés en cas de shadowing ; **aucune session VS Code réelle** exécutée
  (preuve = client LSP sur tubes). Voir
  [`docs/qualification/LSP.md`](../qualification/LSP.md).
- La FFI est **expérimentale** : primitives seulement, `unsafe` obligatoire,
  aucune garantie sur la bibliothèque C appelée ; le profil `pure` la refuse.
- Le profil `pure` ne suit pas les traps intrinsèques (overflow, division, index).
- Aucun backend natif direct (Cranelift/LLVM) ; WASM limité à WASI preview1
  sans preopen ; hot reload sans préservation d'état.
- Un seul OS testé localement (Windows) ; Linux dépend du run CI, les autres
  cibles ne sont pas annoncées.
- Aucune ratification d'ADR, aucune release, aucun audit externe. La licence est
  en revanche tranchée (`MIT OR Apache-2.0`).

## 7. Reproduction

```powershell
cargo build -p ravum-lang-cli --bin ravum-lang
target\debug\ravum-lang.exe check examples\hello --json
target\debug\ravum-lang.exe build examples\hello
target\debug\ravum-lang.exe run examples\hello
target\debug\ravum-lang.exe run examples\ffi-abs
target\debug\ravum-lang.exe run --target wasm32-wasip1 examples\hello
target\debug\ravum-lang.exe lsp
pwsh -File scripts\lang-smoke.ps1 -SkipBuild
cargo run -p ravum-conformance -- run --json target/conformance-report.json
cargo test --workspace --all-targets -- --test-threads=2
cargo test -p ravum-lang-cli --test lsp_client
pwsh -File scripts\sbom.ps1
python tools\validate_repo_docs.py
```







