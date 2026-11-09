# Ravum Core 1.0 — état réel du langage (snapshot de travail)

> **Document de situation, pas un certificat.** Chaque chiffre est lié à une
> commande exécutée sur cette machine et à un état de travail non encore gelé par
> un SHA de commit final. Aucun `PROVED`, aucune maturité « production ».
> Dernière mesure : 2026-11-09, Windows 11 x86-64, rustc 1.95.0. Tests
> workspace : **993 passés / 0 échec / 2 ignorés / 80 suites**. Smoke des
> exemples : **31 PASS / 0 FAIL** (`scripts/lang-smoke.ps1 -SkipBuild`).
> Conformance : **107 cas, 106 PASS / 0 FAIL / 1 SKIP**. **Linux qualifié** sur
> runner GitHub `ubuntu-latest` (run
> [42675213985](https://github.com/Rwanbt/Ravum/actions/runs/42675213985), SHA
> `78c8f72`) : workspace, conformance, smoke, install, Miri, fuzz et WASM verts ;
> TSan explicitement `NOT_APPLICABLE`.

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
- `crates/ravum-lang-cli` : `ravum-lang` (`init`, `check`, `build`, `run`,
  `watch`/`dev`, `test`, `fmt`, `lint`, `doc`, `cache`, `doctor`,
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
| Workspace complet | `cargo test --workspace --all-targets -- --test-threads=2` | **993 passés / 0 échec / 2 ignorés / 80 suites** |
| Fuzz lexer/parser | `RAVUM_FUZZ_COLLECT=1 RAVUM_FUZZ_ITERS=200000 cargo test -p ravum-lang-syntax --test fuzz_lex_parse -- --ignored` | 249 054 entrées, 5 216 641 tokens, **0 échec**, digest stable |
| Fuzz frontend sema | `RAVUM_FUZZ_ITERS=100000 cargo test -p ravum-lang-sema --test fuzz_check -- --ignored` | 120 300 itérations cumulées, **0 panic / 0 diagnostic vide / 0 non-déterminisme** |
| Miri | `MIRIFLAGS=-Zmiri-disable-isolation cargo +nightly miri test -p ravum-lang-syntax -p ravum-lang-sema --lib --tests` | **153 tests PASS** (65 syntax + 87 sema + 1 fuzz), ≈ 5 min 53 s ; exclusions fs Windows documentées |
| Audit borrow checker | `cargo test -p ravum-lang-sema --test own_audit` | **85 passés / 0 échec** ; finding F2 (référence à un temporaire) corrigé + régression de conformance |
| Audit backend | `cargo test -p ravum-lang-codegen --test backend_audit` | **21 passés** ; F0/F0b/F2 corrigés, F1/F3–F6 documentés |
| Installateur | `scripts/install.ps1 -Prefix <tmp>` | install/uninstall réels, hash manifeste = hash binaire, PATH idempotent |
| SBOM | `scripts/sbom.ps1` / `sbom.sh` | SPDX-2.3, 63 composants, 165 relations, SHA-256 identique entre runs |

## 5. Gates (état honnête)

| Gate | État | Preuve / limite |
|---|---|---|
| T0 | **PASS** | baseline 511/0/80 sur `5408d9b` ; 4 défauts P0 corrigés, 208 tests ; workspace final 993/0 |
| T1 | **PARTIAL** | chaîne B réelle (`check→build→run`) ; M5 A/B mesuré et parité MATCH ; **C `NOT_IMPLEMENTED`**, décision non ratifiée |
| T2 | **PARTIAL** | spec DRAFT 15 fichiers + conformance 106/107 (1 skip OS) ; `features.md` encore majoritairement `NOT_COVERED`, aucun PASS revendiqué |
| T3 | **PARTIAL** | 69 tests syntaxe, fuzz 249 k entrées, Miri syntax PASS ; LSP absent (outil, pas frontend) |
| T4 | **PARTIAL** | typechecker + borrow checker testés (133 + 85 cas), finding CRITICAL corrigé ; **pas d'audit externe**, pas de preuve formelle |
| T5 | **PASS** | binaires natifs réels sur **Windows et Linux** (CI) ; FFI `abs` exécutée sur les deux ; pas d'ABI stable revendiquée |
| T6 | **PARTIAL** | stdlib core/io/fs/string/vec/math + **FFI minimale** (`extern "C"`, primitives, `unsafe`) ; threads/net/async `UNSUPPORTED` |
| T7 | **PARTIAL** | CLI complète (`watch` inclus), WASM WASI preview1 ; **LSP `UNSUPPORTED` explicite** ; binaire nommé `ravum-lang` |
| T8 | **PARTIAL** | hot reload = rebuild + redémarrage (pas de préservation d'état) ; collaboration/overlays non qualifiés ici |
| T9 | **PARTIAL** | 9 verticales `.rv` exécutées (dont FFI et WASM) ; HTTP/SQLite/GPU `NOT_RUN`/`UNSUPPORTED` |
| T10 | **PARTIAL** | fuzz + Miri + audits internes exécutés sur Windows **et** Linux ; **TSan `NOT_RUN`/`NOT_APPLICABLE`**, audit externe absent |
| T11 | **PARTIAL** | installateur, SBOM, provenance, checksums ; **pas de signature**, **pas de build bit-à-bit**, licence `PENDING_HUMAN` |
| T12 | **PARTIAL** | PR/rapports/décisions prêts ; dogfood indépendant et Go/No-Go humain absents |

## 6. Limites structurelles à ne pas oublier

- Le backend de référence est **B** (Rust généré) : la sûreté mémoire du code
  safe est déléguée à un sous-ensemble Rust vérifié *après* le checker Ravum, pas
  prouvée formellement par Ravum.
- Le modèle d'emprunt est volontairement conservateur : pas de références
  stockées/retournées en général, pas de lifetimes explicites.
- La FFI est **expérimentale** : primitives seulement, `unsafe` obligatoire,
  aucune garantie sur la bibliothèque C appelée ; le profil `pure` la refuse.
- Le profil `pure` ne suit pas les traps intrinsèques (overflow, division, index).
- Aucun backend natif direct (Cranelift/LLVM) ; WASM limité à WASI preview1
  sans preopen ; hot reload sans préservation d'état.
- Un seul OS testé localement (Windows) ; Linux dépend du run CI, les autres
  cibles ne sont pas annoncées.
- Aucune ratification d'ADR, aucune licence, aucune release, aucun audit externe.

## 7. Reproduction

```powershell
cargo build -p ravum-lang-cli --bin ravum-lang
target\debug\ravum-lang.exe check examples\hello --json
target\debug\ravum-lang.exe build examples\hello
target\debug\ravum-lang.exe run examples\hello
target\debug\ravum-lang.exe run examples\ffi-abs
target\debug\ravum-lang.exe run --target wasm32-wasip1 examples\hello
pwsh -File scripts\lang-smoke.ps1 -SkipBuild
cargo run -p ravum-conformance -- run --json target/conformance-report.json
cargo test --workspace --all-targets -- --test-threads=2
```
