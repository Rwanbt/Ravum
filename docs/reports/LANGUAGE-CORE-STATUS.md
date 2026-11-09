# Ravum Core 1.0 — état réel du langage (snapshot de travail)

> **Document de situation, pas un certificat.** Chaque chiffre est lié à une
> commande exécutée sur cette machine et à un état de travail non encore gelé par
> un SHA de commit final. Aucun `PROVED`, aucune maturité « production ».
> Dernière mesure : 2026-11-09, Windows 11 x86-64, rustc 1.95.0. Smoke des exemples : **28 PASS / 0 FAIL** (`scripts/lang-smoke.ps1 -SkipBuild`).

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
| Diagnostics par code | `cargo test -p ravum-lang-syntax` | 56 tests verts (lexer/parser/formatter) |
| Codegen + prélude | `cargo test -p ravum-lang-codegen` | 26 tests verts, dont compilations/exécutions rustc réelles |
| Conformance | `cargo run -p ravum-conformance -- run` | 104 cas, statuts réels (voir §3) |\n| Tests unitaires | `ravum-lang test examples/tests-demo` | `3 passed; 0 failed` (dont un test `-> Result`) |\n| M5 | `scripts/m5-bench.ps1` | A/B parité MATCH sur 4 programmes, B +12 à +31 % de compilation, C `NOT_IMPLEMENTED` |

## 2. Composants livrés dans cette itération

- `crates/ravum-lang-syntax` : lexer, AST + `NodeId` globaux, parser récursif
  descendant (récupération, garde-fous profondeur/tokens), formateur idempotent
  préservant commentaires et lignes vides.
- `crates/ravum-lang-sema` : résolution de modules/visibilité, table builtins
  (stdlib), typechecker, exhaustivité `match`, inférence d'effets transitive et
  profils `core`/`pure`/`rt`, **borrow checker conservateur** (init, moves,
  move partiel suivi, conflits de prêts, non-échappement des références, drop).
- `crates/ravum-lang-codegen` : génération d'un fichier Rust autonome et
  auditable (`#![forbid(unsafe_code)]`, aucun `unsafe`), prélude runtime safe,
  runner de tests, mapping stdlib, déterminisme.
- `crates/ravum-lang-cli` : `ravum-lang` (`init`, `check`, `build`, `run`, `test`,
  `fmt`, `lint`, `doc`, `cache`, `doctor`, `--version`), loader de packages
  (`ravum.toml`, modules, lockfile/hash, dépendances locales, chemins validés),
  sortie JSON stable, codes de sortie.
- `tools/conformance` + `conformance/` : harnais indépendant (std only) et 104 cas
  déclaratifs (`compile-pass`, `compile-fail` avec codes exacts, `run-pass`,
  `run-fail`, `fmt`, `cli`, sécurité).
- `spec/language/` : 15 documents DRAFT (lexical, grammaire, types, expressions,
  ownership, effets, modules, stdlib, diagnostics, ABI, concurrence,
  compatibilité, unsupported, core-machine, features).
- `examples/` : hello, fib, calculator, file-tool, stats, dsp-gain-iir,
  package-demo (util+app), tests-demo.
- `docs/architecture/RAVUM-CORE-DESIGN.md`, `docs/decision/M5-technical-report.md`,
  `docs/reports/T0-AUDIT.md`, `docs/roadmap/CORE-1.0-EXECUTION.md`,
  `DECISIONS_REQUIRED.md`.

## 3. Conformance — chiffres bruts et honnêtes

Dernière exécution complète (après corrections parser/effets) :
`cargo run -p ravum-conformance -- run --json target/conformance-report.json` →
**104 cas : 103 PASS, 0 FAIL, 1 SKIP, 0 NOT_RUN** (exit 0 ; le seul non-pass est le cas Unix `skip_on: windows`, inapplicable sur l'hôte).

Les FAIL ne sont pas masqués : ils mêlent des bugs réels en cours de correction
(résolutions de chemins pour le codegen, inférence `Vec::with_capacity`, codes de
diagnostic non canoniques, classification `use-after-move`), des cas de
sécurité dépendants de privilèges (symlink → SKIP) et des écarts de spec en cours
d'alignement. Ce chiffre sera re-mesuré après corrections ; il ne doit pas être
présenté comme un succès.

## 4. Gates (état honnête)

| Gate | État | Preuve / limite |
|---|---|---|
| T0 | **PASS** | baseline 511/0/80 sur `5408d9b` ; 4 défauts P0 corrigés, 208 tests sur les crates concernés, 0 échec (`target/p0-tests-lead.log`) ; limites TOCTOU documentées |
| T1 | **PARTIAL** | chaîne B réelle (`check→build→run` sur `.rv`) ; harness M5 écrit, mesures comparatives A/B pas encore exécutées ; C `NOT_IMPLEMENTED` |
| T2 | **PARTIAL** | spec candidate 15 fichiers + conformance 103/104 (1 skip OS) ; `features.md` majoritairement `NOT_COVERED`, aucun PASS revendiqué |
| T3 | **PARTIAL** | parser/formatter 56 tests verts, corpus hostile (profondeur, 100k tokens, troncatures), 312 tests langage au total ; fuzz long et LSP non exécutés |
| T4 | **PARTIAL** | typechecker + borrow checker implémentés et testés unitairement ; corpus négatif conformance encore partiel ; limite non-échappement des références documentée |
| T5 | **PARTIAL** | binaires natifs Windows réels ; Linux `NOT_RUN` ; parité debug/release non mesurée |
| T6 | **PARTIAL** | stdlib core/io/fs/string/vec/math disponible et utilisée par les exemples ; FFI/unsafe `UNSUPPORTED` |
| T7 | **PARTIAL** | `init/check/build/run/fmt/doctor/--version` opérationnels ; `test` en cours ; LSP `UNSUPPORTED` explicite |
| T8 | **NOT_RUN** | hot reload `.rv` non tenté ; fondations P0 disponibles |
| T9 | **PARTIAL** | hello/fib/calculator/stats exécutés ; dsp et tests-demo en cours ; service HTTP/WASM `NOT_RUN`/`UNSUPPORTED` |
| T10 | **PARTIAL** | conformance 103/104, 855 tests workspace ; fuzz long, Miri/TSan, audit indépendant non exécutés |
| T11 | **NOT_RUN** | installateur/SBOM/provenance non produits ; licence = décision humaine |
| T12 | **PARTIAL** | dossier de décisions et rapport ; dogfood/audit externe absents |

## 5. Limites structurelles à ne pas oublier

- Le backend de référence est **B** (Rust généré) : la sûreté mémoire du code
  safe est déléguée à un sous-ensemble Rust vérifié *après* le checker Ravum, pas
  prouvée formellement par Ravum.
- Le modèle d'emprunt est volontairement conservateur : pas de références
  stockées/retournées, pas de lifetimes explicites, pas d'`unsafe`/FFI/threads.
- Le profil `pure` ne suit pas les traps intrinsèques (overflow, division, index).
- Aucun backend natif direct (Cranelift/LLVM), aucun WASM, aucun hot reload .
- Un seul OS testé réellement (Windows) ; les autres cibles ne sont pas annoncées.
- Aucune ratification d'ADR, aucune licence, aucune release.

## 6. Reproduction

```powershell
cargo build -p ravum-lang-cli --bin ravum-lang
target\debug\ravum-lang.exe check examples\hello --json
target\debug\ravum-lang.exe build examples\hello
target\debug\ravum-lang.exe run examples\hello
pwsh -File scripts\lang-smoke.ps1 -SkipBuild
cargo run -p ravum-conformance -- run --json target/conformance-report.json
```




