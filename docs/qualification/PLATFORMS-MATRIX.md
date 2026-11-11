# Matrice des plateformes — Ravum Core 1.0

> **Registre de mesure, pas un certificat.** Date : 2026-11-11. Chaque cellule
> porte un état réel et la preuve associée : `PASS` (mesuré), `PARTIAL`
> (mesuré sur un sous-ensemble), `NOT_RUN` (aucune exécution), `UNSUPPORTED`
> (inexprimable : le CLI refuse la cible ou la cible n'existe pas). Aucune
> compilation croisée n'est présentée comme un support qualifié.

## 0. Règles de lecture

- **Une cible n'est « supportée » que si elle a été *exécutée***, pas seulement
  compilée. `cargo build --target` qui réussit ne prouve pas qu'un binaire
  s'exécute.
- **La compilation croisée n'est pas un support.** `rustc --target` installé sur
  une machine ne qualifie pas la plateforme cible : il n'y a ni exécution, ni
  conformance, ni test d'installation.
- Les états `NOT_RUN` et `UNSUPPORTED` ne se convertissent jamais en `PASS` par
  absence d'échec.
- Les mesures locales ont été faites sur un binaire construit depuis un export
  propre de `HEAD` **`5d88ecb`** (voir §1), pas sur `target/debug/` partagé avec
  d'autres agents. Les mesures CI sont des exécutions tierces sur
  `ubuntu-latest`, vérifiées via l'API GitHub (voir §3).

## 1. Environnement de mesure local

| Élément | Valeur |
|---|---|
| SHA | `5d88ecb` (`feat/lang-core-1.0-rc`), export `git archive HEAD` |
| Compilateur mesuré | `ravum-lang 0.1.0 (edition core-1.0-draft.1)`, construit par `cargo build -p ravum-lang-cli --bin ravum-lang` dans la copie exportée ; SHA-256 `4D5ED2EB…278686E5` |
| Machine | Windows 11 x86-64, i7-13620H (10 cœurs / 16 threads), 16 Go |
| Toolchain | `rustc 1.95.0`, `node v26.13.0`, cibles rustup installées : `x86_64-pc-windows-msvc`, `wasm32-wasip1`, `wasm32-unknown-unknown` |
| Charge | 79–100 % CPU (autres agents en compilation) pendant les mesures ; les temps de paroi sont des majorants, voir RFC-D2 |

## 2. Matrice

| Plateforme | Compilation | Exécution | Conformance | Installation | FFI | CI | État |
|---|---|---|---|---|---|---|---|
| **Windows x86-64** | **PASS** — `run examples/hello` etc. (§2.1) | **PASS** — 4 exemples exécutés, exit 0 | **PASS local** — 386 cas, 385 PASS / 0 FAIL / 1 SKIP (`security/dependency-absolute-unix`, `skip_on: windows`) | **NOT_RUN** cette passe (mesure antérieure : `scripts/install.ps1`, manifeste, hash) | **PASS** — `ffi-abs` → `42` | **AUCUN job CI Windows** — preuve locale uniquement | **PASS (plateforme primaire, CI absente)** |
| **Linux x86-64** | **PASS (CI)** — `cargo test --workspace --all-targets` vert | **PASS (CI)** — smoke Linux + conformance + installateur | **PASS (CI)** — corpus exécuté sur `ubuntu-latest` | **PASS (CI)** — `scripts/install.sh` dans un préfixe temporaire + `--version` | **PARTIAL (CI)** — `run/ffi-abs` dans le corpus ; pas de test FFI dédié Linux | `linux-qual.yml`, 7 jobs verts + TSan `SKIPPED` (run 42936163958) | **PASS sur une seule image**, non mesuré localement |
| **WASI (`wasm32-wasip1`)** | **PASS** — `run --target wasm32-wasip1 examples/hello` | **PASS** — stdout `hello, ravum` via Node WASI (§2.2) | **NOT_RUN** — le corpus n'est pas exécuté sous WASI | **NOT_RUN** | **NOT_RUN** — aucun `extern` sous WASI | **PASS (CI)** — job `wasm32-wasip1 (build + run via Node WASI)` vert | **PARTIAL** (qualifié séparément, périmètre étroit) |
| **macOS x86-64** | **UNSUPPORTED** — `--target x86_64-apple-darwin` refusé (exit 2) | **NOT_RUN** | **NOT_RUN** | **NOT_RUN** — `install.sh` a des branches `darwin` mais n'a jamais tourné | **NOT_RUN** | **AUCUN** | **NOT_RUN** |
| **macOS ARM64** | **UNSUPPORTED** — aucune cible CLI | **NOT_RUN** | **NOT_RUN** | **NOT_RUN** | **NOT_RUN** | **AUCUN** | **NOT_RUN** |
| **Linux ARM64** | **UNSUPPORTED** — `--target aarch64-unknown-linux-gnu` refusé (exit 2) | **NOT_RUN** | **NOT_RUN** | **NOT_RUN** | **NOT_RUN** | **AUCUN** | **NOT_RUN** |
| **Android** | **UNSUPPORTED** — aucune cible, aucun NDK, aucune doc | **NOT_RUN** | **NOT_RUN** | **NOT_RUN** | **NOT_RUN** | **AUCUN** | **UNSUPPORTED** |
| **iOS** | **UNSUPPORTED** — aucune cible, pas d'outillage Apple | **NOT_RUN** | **NOT_RUN** | **NOT_RUN** | **NOT_RUN** | **AUCUN** | **UNSUPPORTED** |
| **Navigateur (WASM)** | **UNSUPPORTED** — `--target wasm32-unknown-unknown` refusé (exit 2) ; aucune liaison JS | **NOT_RUN** | **NOT_RUN** | **NOT_RUN** | **NOT_RUN** | **AUCUN** | **UNSUPPORTED** (ADR-025 `PROPOSED`, non implémenté) |

### 2.1 Windows — commandes et sorties exactes

Exécutées sur des copies temporaires des exemples (le dépôt n'est pas modifié) :

```
ravum-lang run <tmp>/hello        → exit 0, "hello, ravum"
ravum-lang run <tmp>/fib          → exit 0, "0 1 1 2 3 5 8 13 21 34 55"
ravum-lang run <tmp>/ffi-abs      → exit 0, "42"          (extern "C" abs + unsafe)
ravum-lang run <tmp>/dsp-gain-iir → exit 0, "2 1 -0.5 0 --- 0.2"
ravum-lang check <pkg> --json     → exit 0, JSON schema_version 1
ravum-lang doctor --json          → exit 0, rustc ok, parser/checker/codegen ready, os=windows, arch=x86_64
```

Conformance Windows, mesure de cette passe :

```
cargo run -p ravum-conformance -- run --json target/conformance-head.json
→ total=386 pass=385 fail=0 skip=1 not_run=0   (SKIP = security/dependency-absolute-unix)
```

### 2.2 WASI — commande et sortie exactes

```
ravum-lang run --target wasm32-wasip1 <tmp>/hello
→ "running (wasm32-wasip1 via node): …/hello.wasm"
→ "hello, ravum"   (exit 0 ; Node 26.13.0, avertissement WASI expérimental)
```

Limites mesurées : la cible CLI est unique (`wasm32-wasip1`), l'exécution passe
par Node, et le corpus de conformance n'est **pas** rejoué sous WASI. Le cas
`run/ffi-abs` n'a pas d'équivalent WASI.

### 2.3 Refus de cibles non supportées (mesuré)

Toute cible autre que native et `wasm32-wasip1` est refusée **avant** toute
compilation, avec le code de sortie `2` (usage invalide) :

```
ravum-lang build --target wasm32-unknown-unknown …  → exit 2, "supported target: `wasm32-wasip1`"
ravum-lang build --target x86_64-apple-darwin …     → exit 2
ravum-lang build --target aarch64-unknown-linux-gnu … → exit 2
ravum-lang build --target x86_64-unknown-linux-gnu …  → exit 2
```

C'est un comportement **fail-closed** correct : le CLI ne prétend pas
cross-compiler. Il ne faut pas en déduire un support ARM/macOS/Linux : seule une
exécution sur la plateforme le donnerait.

## 3. Preuve CI Linux — vérifiée via l'API GitHub

Le run `linux-qual.yml` sur le SHA complet
`5d88ecbdb1839287aecd13fd6ba914b911b75f5b` a été interrogé le 2026-11-11 :

```
GET https://api.github.com/repos/Rwanbt/Ravum/actions/runs/42936163958/jobs?per_page=20
→ total_count = 8, toutes sur le même head_sha
```

| Job | Conclusion |
|---|---|
| Workspace tests (Linux, stable 1.95.0) | `success` |
| Conformance corpus (Linux) | `success` |
| Example smoke suite (Linux) (dont stdlib codegen sweep, CLI surface, JSON contract) | `success` |
| Installer contract (temporary prefix, then `--version`) | `success` |
| wasm32-wasip1 (build + run via Node WASI) | `success` |
| Miri (nightly, syntax + sema) | `success` |
| Fuzz smoke (ignored `fuzz_*` tests) | `success` |
| ThreadSanitizer | `skipped` — `NOT_APPLICABLE` (aucun thread/async en Core 1.0) |

Ces conclusions sont des **données tierces** (GitHub), pas une exécution sur
cette machine : elles qualifient Linux sur **cette image** (`ubuntu-latest`) à
**ce SHA**, et rien de plus. Aucune reproduction multi-images, aucun runner
auto-hébergé, aucune version de noyau épinglée.

**Trou structurel :** il n'existe **aucun job CI Windows**, alors que Windows est
la plateforme primaire de développement. Toute la preuve Windows de ce document
est locale et liée à une machine ; elle n'est pas rejouée en intégration
continue. C'est le principal risque de la matrice.

## 4. Ce qui n'est pas qualifié, et ce qu'il faudrait pour le devenir

| Cible | Ce qui manque | Commande/événement qui changerait l'état |
|---|---|---|
| macOS x86-64 / ARM64 | une cible `--target`, un runner macOS, une exécution du corpus | ajouter la cible au CLI, job `runs-on: macos-*`, `lang-smoke.sh` + conformance verts |
| Linux ARM64 | idem + un runner ARM | `runs-on: ubuntu-24.04-arm` (ou self-hosted), corpus exécuté |
| WASI « qualifié » | exécution du **corpus** sous WASI, pas seulement `hello` ; gestion des arguments/préopens ; FFI WASI absente | un harnais conformance sous Node WASI avec les mêmes `case.json` |
| Navigateur WASM | cible `wasm32-unknown-unknown` + liaisons JS + tests navigateur | décision de périmètre (ADR-025) puis implémentation |
| Android / iOS | toolchains, cibles, exécution sur appareil/émulateur | hors périmètre Core 1.0 ; aucune décision en cours |
| Windows CI | un runner Windows et les mêmes jobs que Linux | `runs-on: windows-latest` + smoke/conformance/install |

## 5. Ce que cette matrice ne fait pas

- Elle ne convertit aucune compilation croisée en support : les refus exit 2
  sont enregistrés comme des refus, pas comme des preuves d'absence de bug.
- Elle ne présente pas Linux comme « universel » : une image, un run, un SHA.
- Elle ne rejoue ni l'installateur Windows ni le SBOM cette passe ; ces mesures
  restent celles publiées antérieurement (`docs/release/INSTALL.md`,
  `docs/qualification/REPRODUCIBILITY.md`) et sont marquées `NOT_RUN` ici.
- Elle ne modifie aucun workflow CI ni aucun script d'installation.
