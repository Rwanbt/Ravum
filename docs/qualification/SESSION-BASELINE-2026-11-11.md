# Ravum — baseline de la session P0/P1/P2

> Mesures exécutées **avant** toute modification, sur un arbre propre.
> Aucun chiffre de ce fichier n'est estimé.

| Élément | Valeur |
|---|---|
| Date (Europe/Paris) | 2026-11-11 |
| Branche | `feat/lang-core-1.0-rc` |
| HEAD (`git rev-parse HEAD`) | `5d88ecbdb1839287aecd13fd6ba914b911b75f5b` |
| `git status` | arbre propre, aucun stash |
| `origin/feat/lang-core-1.0-rc` | identique à HEAD |
| Plateforme | Windows 11 x86-64, 16 threads logiques, ≈15,7 Go RAM |
| Outillage | `rustc 1.95.0`, `cargo 1.95.0`, `node v26.13.0`, `Python 3.13.7` |
| Binaires de mesure | copie figée dans `target/baseline-bin/` (les sources ont ensuite été éditées par plusieurs agents, la mesure ne devait pas dépendre de leur état) |

## 1. Tests workspace

```
cargo test --workspace --all-targets
```
→ **89 suites, 1405 passés / 0 échec / 2 ignorés** (exit 0). Journal : `target/baseline-workspace-tests.log`.

## 2. Conformance

```
target\baseline-bin\ravum-conformance.exe run --toolchain target\baseline-bin\ravum-lang.exe --json target\baseline-conformance.json
```
→ **386 cas : 385 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** (exit 0). Le SKIP est
`security/dependency-absolute-unix` (`skip_on: windows`).

## 3. Gates outillage (Python)

| Commande | Résultat |
|---|---|
| `python tools/registry_audit.py` | exit 0 — **108 codes enregistrés, 126 émis, 0 hors registre, 0 jamais émis**, 0 W hors DIAG-1.6 |
| `python tools/features_coverage.py --strict` | exit 0 — **188/188** règles satisfaites (187 par cas + 1 exemption `F-META-006` machine-lisible) |
| `python tools/report_consistency.py` | exit 0 — PASS, chiffres cohérents (strict 188/188, registre 108) |
| `python tools/validate_repo_docs.py` | exit 0 — 42 ADR proposés et fichiers requis présents |
| `python -m unittest discover -s tools/tests` | exit 0 — **22 tests PASS** |

## 4. Contrat `check ⇒ build` — divergence mesurée (P0-A)

```
pwsh -File scripts/p0a-check-build-sweep.ps1 -IncludeRunPass
```
→ **132 cas en mode accepté** (79 `compile-pass` + 53 `run-pass`) :
**132 check-pass**, **130 build-pass**, **2 DIVERGENCES** (rapport brut :
`target/p0a-sweep.json`).

| Cas | check | build | Cause racine | Contrat |
|---|---|---|---|---|
| `ownership/static-str-stored` | 0 | 1 | Rust `E0106 missing lifetime specifier` : le codegen émet `struct S { text: &str }` et `fn lit() -> &str` | OWN-6.2 (MUST) : `&'static str` est la seule référence stockable/retournable |
| `ownership/partial-move-tuple-ok` | 0 | 1 | Rust `E0508 cannot move out of [String; 2]` : `a[1]` en position de déplacement | OWN-5.4 (MUST) : le déplacement d'un élément de tableau est autorisé |

Les 37 cas signalés par la revue externe (Claude) **ne sont pas dans le dépôt** :
ils ne sont donc ni confirmés ni infirmés en tant que tels. La mesure ci-dessus
est celle du corpus courant ; les familles signalées sont reconstruites par cas
de régression dédiés (tâche P0-A).

## 5. Défauts confirmés par lecture du code (avant correction)

| ID | Fichier | Défaut | Invariant |
|---|---|---|---|
| P0-C | `crates/ravum-lang-cli/src/commands/watch.rs` (~l.88) | `stop_child` est appelé **avant** `build_once` : une modification qui ne compile pas interrompt la dernière version fonctionnelle | INV-03, INV-07 |
| P0-D | `crates/ravum-lang-cli/src/commands/run.rs` (l.130, l.156) | `code.clamp(0, 255)` : sur Windows un code d'exception négatif (ex. `-1073741819`) devient **0** = faux succès | INV-01 |
| P0-E | `crates/ravum-lang-cli/src/commands/test.rs` | aucun rapprochement entre tests déclarés et tests exécutés ; les lignes de verdict inconnues sont ignorées ; `--filter` est transmis à un runner qui l'ignore | INV-01 |
| P0-B | `crates/ravum-lang-cli/src/pipeline.rs` | chemins d'artefacts partagés (`.ravum/gen/main.rs`, `.ravum/bin/<nom>`) : deux builds simultanés se collisionnent, un build incomplet peut écraser un artefact valide | INV-03, INV-04, INV-07 |
