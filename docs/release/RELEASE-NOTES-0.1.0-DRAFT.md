# Ravum Core 1.0 — notes de version `ravum-lang 0.1.0` — **UNRELEASED (DRAFT)**

> **UNRELEASED — DRAFT — AUCUN TAG, AUCUNE RELEASE.**
>
> **Statut : `UNRELEASED` · DRAFT · 2026-11-14 · `PENDING_HUMAN_APPROVAL`.**
> Ces notes décrivent le **sous-ensemble Core 1.0 DRAFT** du compilateur
> `ravum-lang`, au commit **`214bf5e`** de la branche `goal/consolidation-2`
> (HEAD au moment des mesures ; il a pu avancer depuis).
>
> **Il n'y a aucun tag, aucune release, aucun artefact signé, aucune
> publication.** Vérifié le 2026-11-14 : `git tag` ne renvoie **rien** et
> `git describe --tags --always` renvoie `214bf5e`. La version de crate
> (`0.1.0`) et l'édition (`core-1.0-draft.1`) ne sont donc **pas** une version
> publiée.
>
> Ces notes **ne déclarent aucune disponibilité**. Elles ne revendiquent ni
> stabilité d'API/ABI, ni « production ready », ni audit externe. « Production
> ready » est une **cible décidée** (D2), pas un état atteint :
> [R14](../roadmap/PRODUCTION-READINESS.md) est `PARTIAL`, T4 (audit externe) et
> T12 (Go/No-Go) sont `BLOCKED` par des décisions humaines.

## 1. Identité du binaire

| Élément | Valeur constatée le 2026-11-14 |
|---|---|
| `ravum-lang --version` | `ravum-lang 0.1.0 (edition core-1.0-draft.1)` |
| Crate | `ravum-lang-cli` (`0.1.0`, version d'espace de travail) |
| Toolchain épinglée | `rustc 1.95.0 (59807616e 2026-04-14)`, [`rust-toolchain.toml`](../../rust-toolchain.toml) |
| Commit de référence | `214bf5e` (branche `goal/consolidation-2`) ; arbre non propre (`git_dirty: true`) lors de l'installation testée |
| Licence | `MIT OR Apache-2.0` au choix du preneur |
| Plateforme mesurée | Windows 11 x86-64 ; Linux x86-64 **en CI uniquement** |
| Binaires | `target/debug/ravum-lang.exe` et `target/release/ravum-lang.exe` |

Un binaire construit depuis un arbre modifié **n'est pas** un artefact de
release ([`PROVENANCE.md`](PROVENANCE.md)). Aucune signature n'existe : le
mécanisme OpenSSH est qualifié, mais D9 (custody de clé) est **différée** et
**aucune clé de projet** n'a été générée ([`../qualification/SIGNATURES.md`](../qualification/SIGNATURES.md)).

## 2. Fonctions disponibles

### 2.1 Ligne de commande

Les commandes de l'entête `ravum-lang --help` exécutent toutes le pipeline réel ;
la liste « NOT IMPLEMENTED » de l'entête est vide. Exécuté le 2026-11-14 (sauf
mention `NOT_RUN`) :

| Commande | Rôle | Vérifié cette passe |
|---|---|---|
| `init <dir> [--lib] [--force]` | crée `ravum.toml` + `src/main.rv` (ou `src/lib.rv`) + `.gitignore` | oui (bin et lib) |
| `check [PATH] [--json] [--profile] [--rt-entry] [--locked] [--timings]` | charge, résout et vérifie | oui |
| `build [PATH] [--release] [--target] [--emit-rust] [--emit-ir] [--rt-entry] [--timings]` | vérifie, génère du Rust, appelle `rustc` | oui (natif) |
| `run [PATH] [--target] [-- ARGS…]` | build puis exécute le binaire | oui (`hello`, `fib`, `calculator`, `stats`, `file-tool`, `file-store`, `ffi-abs`, `dsp-gain-iir`, `package-demo`) |
| `watch [PATH] [--run] [--release]` (alias `dev`) | reconstruit à chaque changement | `NOT_RUN` |
| `test [PATH] [--filter] [--json]` | construit et exécute les `test fn` | oui (`tests-demo` : 3/3) |
| `fmt [PATH] [--check]` | formate, ou vérifie sans écrire | oui |
| `lint [PATH] [--deny CODE]…` | avertissements ; `--deny` les promeut en erreur | oui |
| `doc [PATH] [--out DIR]` | documentation Markdown des items publics | oui |
| `cache status\|clean [PATH]` | inspecte ou supprime le cache de paquet | oui (`status`) |
| `lsp` | serveur de langage LSP 3.17 sur stdio | oui (cycle `initialize`/`shutdown`/`exit`) |
| `doctor [--json]` | diagnostic `rustc`, chemins, cache, stades du frontend | oui (`verdict: ready`) |
| `help [COMMAND]`, `--version` | aide et version | oui |

Cibles : natif (défaut) et `wasm32-wasip1` (via `rustc --target` puis Node WASI).
La cible WASM est `NOT_RUN` sur la machine de cette passe (`E0463`, cible
`wasm32-wasip1` non installée) ; l'état `PASS` publié exige
`rustup target add wasm32-wasip1` et Node
([`../qualification/PLATFORMS-MATRIX.md`](../qualification/PLATFORMS-MATRIX.md)
§2).

Codes de sortie : `0` succès, `1` échec de compilation/diagnostic/test ou refus
opérationnel sûr, `2` ligne de commande invalide, `3` outil externe manquant
(`rustc`).

### 2.2 Langage (sous-ensemble Core 1.0)

Surface **réellement outillée** (lexer, parser, résolution, typechecker, borrow
checker, codegen Rust, conformance) :

- fonctions, paramètres, retours explicites, génériques **sans bornes**
  (monomorphisation), `!` coercible ;
- `struct`, `enum`, `impl` inhérent, `match` avec exhaustivité, tuples
  hétérogènes, tableaux et slices `&[T]`, `for` sur `a..b` et sur `Vec<T>` ;
- `Option`/`Result` + `?`, `test fn` (y compris `-> Result<(), Error>`) ;
- ownership conservateur : initialisation, moves et move partiel suivis, prêts,
  non-échappement, `drop`, provenance `&'static str` interprocédurale ;
- inférence d'effets transitive et profils `core` / `pure` / `rt` ;
- `.rv` généré en Rust sûr : `#![forbid(unsafe_code)]` sans `extern`,
  `#![deny(unsafe_code)]` + `allow` local des wrappers FFI ;
- stdlib `core`/`io`/`fs`/`string`/`vec`/`math` (périmètre T6, [`../../spec/language/stdlib.md`](../../spec/language/stdlib.md)) ;
- modules et paquets locaux (`path = "../util"`, un seul niveau de racine,
  `ravum.lock`), **aucun** registre ni version distante ;
- FFI **scalaire** expérimentale (`extern "C"` + `unsafe`, primitives seulement) ;
- dix verticales `.rv` exécutables dans [`../../examples/`](../../examples/).

Références normatives : [`../../spec/language/features.md`](../../spec/language/features.md)
(règles tracées) et [`../../spec/language/unsupported.md`](../../spec/language/unsupported.md)
(liste de fermeture des refus). La spécification est **DRAFT**, non ratifiée.

## 3. Refus connus — explicites, jamais silencieux

Aucune fonctionnalité n'est refusée en silence : chaque refus produit un code de
diagnostic stable, et un backend n'accepte jamais une forme « parce que `rustc`
l'accepte » ([`unsupported.md`](../../spec/language/unsupported.md) UNS-0.1 à
UNS-0.3).

| Famille | Codes (extraits) | Phase |
|---|---|---|
| Traits, bornes, `where`, `impl Trait`, GAT, `Drop` | `E-UNSUPPORTED-TRAIT`, `E-UNSUPPORTED-BOUNDS`, `E-UNSUPPORTED-WHERE`, `E-UNSUPPORTED-IMPL-TRAIT`, `E-UNSUPPORTED-DROP-IMPL` | parse |
| Attributs, `derive`, macros | `E-UNSUPPORTED-ATTR`, `E-UNSUPPORTED-MACRO`, `E-PARSE-UNEXPECTED` | parse |
| Closures, `move`, `async`/`await`, `unsafe fn`, pointeurs bruts | `E-UNSUPPORTED-CLOSURE`, `E-UNSUPPORTED-MOVE`, `E-UNSUPPORTED-ASYNC`, `E-UNSUPPORTED-UNSAFE` | parse |
| `static`, alias de type, `union`, const generics, variadiques | `E-UNSUPPORTED-STATIC`, `E-UNSUPPORTED-TYPE-ALIAS`, `E-PARSE-EXPECTED` | parse |
| Ranges hors `for`, étiquettes, turbofish, patterns de struct/plage | `E-UNSUPPORTED-RANGE`, `E-UNSUPPORTED-TOKEN`, `E-PARSE-UNEXPECTED`, `E-UNSUPPORTED-RANGE-PAT` | parse |
| Threads, atomiques, synchronisation, canaux | `E-RES-UNKNOWN` | resolve |
| Réseau (`net`), `async` d'effet | `E-UNSUPPORTED-EFFECT` | effects |
| FFI non scalaire (`char`, `String`, `Vec`, `&T`, `&str`, `&mut T`, tuples, tableaux, structs/enums) | `E-FFI-TYPE` | resolve |
| Appel `extern` hors `unsafe` | `E-UNSAFE-CONTEXT` | types |
| ABI non `"C"`, `extern "C" fn` avec corps, `extern` dans un `impl` | `E-UNSUPPORTED-ABI`, `E-UNSUPPORTED-EXTERN` | parse |
| FFI sous profil `pure` ou `rt` | `E-EFF-PROFILE` | effects |
| Profil `hard_rt`, WCET, hot reload du langage, GPU, mobile, `no_std`, WASM navigateur | hors surface : `UNSUPPORTED`/cible refusée (exit 2) | CLI |

Vérifié le 2026-11-14 sur de petits programmes : `thread::spawn` →
`E-RES-UNKNOWN` + `E-TYP-CALL` ; `async fn` → `E-UNSUPPORTED-ASYNC` ;
`trait` → `E-UNSUPPORTED-TRAIT` ; `io` sous profil `rt` → `E-EFF-PROFILE`.

## 4. Qualité mesurée — chiffres liés à un SHA

> **Aucun chiffre n'est un `PROVED`.** Tous sont liés à une commande, une machine
> et un SHA ; ils ne sont **pas** re-mesurés dans cette passe de rédaction. Un
> `SKIP`/`NOT_RUN`/`PARTIAL` n'est jamais converti en `PASS`.

| Mesure | Commande | Résultat publié | Source / date |
|---|---|---|---|
| Workspace | `cargo test --workspace --all-targets -- --test-threads=2` | **102 groupes : 1 660 passés / 0 échec / 7 ignorés**, exit 0 | [LANGUAGE-CORE-STATUS](../reports/LANGUAGE-CORE-STATUS.md) §1.1, 2026-11-14, `b172248` |
| Conformance + build | `cargo run -p ravum-conformance -- run --verify-build` | **484 cas : 483 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** ; `perimeter=207/207`, `verified=124`, `failed=0`, `build_verified: true` | idem |
| Smoke des exemples | `pwsh -File scripts/lang-smoke.ps1 -SkipBuild` | **43 PASS / 0 FAIL / 0 NOT_RUN** | idem |
| Registre des codes | `python tools/registry_audit.py` | **108 enregistrés / 126 émis / 0 hors registre** | idem |
| Traçabilité FeatureId | `python tools/features_coverage.py --strict` | **188/188**, 1 exemption justifiée (`F-META-006`) | idem |
| Traçabilité MUST | `python tools/must_audit.py` | **807 énoncés, 463 atteignables, 344 sans FeatureId** → décision D10 | [matrice](../roadmap/CORE-1.0-GATE-MATRIX.md) §2 |
| Couverture sema+codegen | `cargo llvm-cov …` puis `coverage_report.py --check` | **90,44 % lignes / 89,88 % régions** (19 237/21 271) ; produit **88,55 %** ; objectif 100 % **non atteint** (10 fichiers) | [COVERAGE-100-OBJECTIVE](../qualification/COVERAGE-100-OBJECTIVE.md) §6 |
| Oracle différentiel | `cargo test -p ravum-lang-codegen --test differential_oracle` | **46 200 exécutions, 0 divergence** (dont le nominal **3 × 2 000 = 6 000**, 0 rejet) | [campagne](../reports/GOAL-CLOSING-CAMPAIGN-2026-11-14.md) §2 |
| Fuzz long | `cargo test … fuzz_lex_parse` / `fuzz_check` | **249 054 entrées, 0 échec** ; **100 000 itérations, 0 échec** | idem |
| Client LSP | `cargo test -p ravum-lang-cli --test lsp_client` | **11 / 0** | idem |
| LSP bout en bout (petit paquet) | `… end_to_end_diagnostic_latency` | p50 2,061 ms / p95 **2,826 ms** (338 lignes) | [LSP.md](../qualification/LSP.md) §7 |
| LSP bout en bout (échelle) | `… end_to_end_diagnostic_latency_on_the_scale_corpus` | p50 322,4 / p95 **337,6 ms** en **debug** ; **116,7 / 163,5 ms en release** à **5 110 lignes** — **cible 50 ms non tenue** | [LANG-TIMINGS](../reports/LANG-TIMINGS.md) §9 |
| Reproductibilité | `pwsh -File scripts/repro-check.ps1` ; `reproducibility.yml` | **PASS**, deux racines et deux `target/` : `9FC5E3F2…757E77` ; **deux runners distincts** : `9b7f16cc…cdf` puis `c1ad5ca9…ecc` (par run, identiques a/b) | [REPRODUCIBILITY](../qualification/REPRODUCIBILITY.md) §« deux runners » |
| SBOM | `SOURCE_DATE_EPOCH=0 pwsh -File scripts/sbom.ps1` | SPDX-2.3, **64 paquets / 170 relations**, déterministe | idem |
| Installateur Windows | `scripts/install.ps1 -Prefix <tmp> -SkipBuild`, puis deux `-Uninstall` | installation exit 0, `--version` correct, `locked=true`, manifeste cohérent ; désinstallations **0 / 0** (« rien à désinstaller ») | **exécuté le 2026-11-14** pour ces notes ; CI : 16/16 |
| Installateur Linux | `scripts/install-contract.sh --skip-build` | **exécuté en CI le 2026-11-14** (run 43625955751, job 132541685225) : **25 vérifications shell + 30 invariants** ; **`NOT_RUN` en local** | [INSTALL-LINUX](../release/INSTALL-LINUX.md) §1 |
| Miri | CI Linux | vert en CI sur `e58ed35` (syntax + sema, 9 min 23) ; **local `NOT_RUN`** (`cargo-miri` absent) | [matrice](../roadmap/CORE-1.0-GATE-MATRIX.md) §0 nonies |
| Mutation | `mutation.yml` | **219 mutants / 180 tués / 30 survivants triés / 9 non viables** (portée ciblée, dont `emit.rs` 36/30/6) ; **local `NOT_RUN`** | [MUTATION-EQUIVALENCE](../qualification/MUTATION-EQUIVALENCE.md) §12 |
| CI | PR [#111](https://github.com/Rwanbt/Ravum/pull/111) | tous les jobs verts sur `b172248` après correction du YAML de `gate-contracts.yml` (le run `b172248` n'avait que ce job en échec de configuration, pas de test) ; TSan `NOT_APPLICABLE` | [campagne](../reports/GOAL-CLOSING-CAMPAIGN-2026-11-14.md) §2 |

Constat défavorable publié sans note de bas de page :
`check` n'est **pas linéaire** à l'échelle — de 5 110 à 50 110 lignes (×9,8),
il passe de **365 ms à 26 424 ms** (×72), la phase `own` portant **97,5 %** du
temps ([LANG-TIMINGS](../reports/LANG-TIMINGS.md) §8). La cible P0-A
« diagnostic p95 < 50 ms » est tenue sur les petits paquets, **pas** à cette
échelle.

## 5. Gates — état du registre (2026-11-13)

Registre canonique : [`../roadmap/CORE-1.0-GATE-MATRIX.md`](../roadmap/CORE-1.0-GATE-MATRIX.md)
**§2** — **4 `PASS` / 7 `PARTIAL` / 2 `BLOCKED`**. Les états n'ont **pas** été
re-mesurés sur la branche de ces notes.

| Gate | État | Ce qui manque |
|---|---|---|
| T0 — fondations et sécurité du socle | `PASS` | — |
| T1 — architecture et bootstrap | `PARTIAL` | voie C non implémentée ; choix A/B/C non ratifié (D2) |
| T2 — spécification formelle | `PARTIAL` | **807 énoncés MUST, 463 atteignables** ; dénominateur non tranché (D10) |
| T3 — frontend | `PASS` | — |
| T4 — typechecker / borrow checker | **`BLOCKED`** | **audit externe humain absent** |
| T5 — backend et exécution | `PASS` | ABI stable non revendiquée ; sanitizers non exécutés |
| T6 — bibliothèque standard | `PARTIAL` | threads, réseau, async `UNSUPPORTED` |
| T7 — outillage (CLI, LSP, VS Code) | `PASS` | flakiness E2E ordinaire ; Windows/macOS non couverts par le job `vscode-e2e` |
| T8 — incrémentalité / rechargement | `PARTIAL` | pas de préservation d'état, pas d'ABI de patch |
| T9 — verticales applicatives | `PARTIAL` | HTTP + SQLite, DSP hôte, E2E navigateur absents |
| T10 — qualité et preuves | `PARTIAL` | audit externe absent ; couverture loin de 100 % ; fuzz complet non rejoué |
| T11 — distribution | `PARTIAL` | **aucune clé ni ancre de confiance** (D9) ; reproductibilité inter-machines non testée |
| T12 — dossier de RC / Go-No-Go | **`BLOCKED`** | **Go/No-Go humain** |

## 6. Limites

### 6.1 Limites de capacités

- **Concurrence, réseau, asynchrone** : refusés (`UNSUPPORTED`), mono-thread par
  construction (`CONC-1.1`).
- **DSP** : **aucune verticale DSP `.rv` → hot swap** — constat daté « non faite,
  RFC seule » ([`../audit/2026-11-14-architecture-audit.md`](../audit/2026-11-14-architecture-audit.md)) ;
  `examples/dsp-gain-iir` est un calcul **hors-ligne** (gain + IIR sur buffers
  fixes), sans périphérique audio. Le profil `rt` est **expérimental** et ne
  certifie ni latence ni WCET ; le gate de traps est
  `RFC-RT-TRAPS-1, PENDING_HUMAN_APPROVAL`.
- **Rechargement** : reconstruction + redémarrage, **aucune préservation
  d'état**.
- **La plateforme Rust historique** (`ravum-*` : hot patch `f64 -> f64`,
  crossfade DSP, MVP-0a/0b) est une **autre ligne** ; elle ne compile pas de
  `.rv` et ne prouve rien sur ce compilateur.

### 6.2 FFI : scalaire, et base de confiance

La FFI se limite à des **appels scalaires** à la CRT/libm déjà liée par défaut :
pas de pointeur, pas de buffer, pas de callback, pas de `#[link]`, pas d'ABI
stable. **Phrase de la TCB, sans adoucissement** : *appeler une fonction C place
cette bibliothèque dans la base de confiance du processus* ; un programme Ravum
peut donc être corrompu par du code que Ravum n'a pas analysé. La garantie est
une **sûreté de forme** (un seul `unsafe`, types autorisés fermés), pas une
sûreté mémoire de la bibliothèque C
([`../ffi/SAFETY.md`](../ffi/SAFETY.md) §1, C-FFI-10). **D7 est ouvert**
(`PROPOSED` / `PENDING_HUMAN_APPROVAL`).

### 6.3 Limites de mesure

- **LSP à l'échelle** : p95 **337,6 ms** à 5 110 lignes en profil de
  développement (transport inclus), cible 50 ms **non tenue**. **Aucune mesure
  en profil release n'est publiée dans le dépôt** (recherche `163,5`/`163.5` le
  2026-11-14 : 0 occurrence) — état `NOT_RUN` / `À VÉRIFIER`, aucun chiffre
  release n'est repris ici.
- **`check` non linéaire** à l'échelle (voir §4), cause non établie par un
  profileur.
- **Une seule machine** pour ces notes (Windows 11 x64) ; Linux dépend de la CI,
  macOS ARM64 est Tier 2, les autres cibles sont `NOT_RUN` ou `UNSUPPORTED`.
- Aucune mesure de latence, de couverture ou de débit n'est une **garantie**.

### 6.4 Limites de distribution

- **Aucun tag, aucune release, aucun artefact signé** : `git tag` vide,
  [checklist de release](RELEASE-CHECKLIST.md) **candidate non ratifiée**,
  D9 différée.
- **Pas de contrat d'installateur Linux ratifié** équivalent à Windows (R11
  `PARTIAL`), et `docs/release/INSTALL-LINUX.md` **n'existe pas** au
  2026-11-14 (vérifié) : la référence Linux est [`INSTALL.md`](INSTALL.md) §3.
- **Politique de dépréciation non ratifiée**, registre de codes non gelé (R10
  `PARTIAL`) : un document `PROPOSED` ne vaut pas ratification.
- **Aucune mise à jour automatique, aucun rollback** après installation.

### 6.5 Documentation

- R14 (documentation sans fausse affirmation) reste **`PARTIAL`** : ce document
  et le [guide de démarrage](../guide/GETTING-STARTED.md) réduisent un manque
  identifié (« guide utilisateur absent »), mais **l'audit de véracité n'est pas
  rejoué** et les chiffres antérieurs ne sont pas revalidés.
- Une incohérence documentaire connue est signalée dans le guide §7.2
  (extension VS Code « non fournie » vs `tools/vscode-ravum` existant) :
  `À VÉRIFIER`, non corrigée ici.

## 7. Décisions humaines ouvertes

Aucune de ces décisions n'est prise par un agent
([`DECISIONS_REQUIRED.md`](../../DECISIONS_REQUIRED.md)) :

| Décision | Objet | État |
|---|---|---|
| **D2** | ratification du backend et du socle normatif ; cible « production ready » | ouverte |
| **D4 / R13** | audit humain indépendant (borrow checker, codegen, prélude) — gate **T4** | `BLOCKED` |
| **D7** | ratification de la frontière `unsafe`/FFI | `PENDING_HUMAN_APPROVAL` |
| **D9 / R12** | custody de clé, `allowed_signers`, signature de l'artefact | **différée** |
| **D10** | dénominateur de traçabilité (FeatureIds vs MUST) — gate **T2** | ouverte |
| **T12** | Go/No-Go humain | `BLOCKED` |

## 8. Où lire l'état réel

- [Guide de démarrage](../guide/GETTING-STARTED.md) — installation, premier
  paquet, boucle `check`/`build`/`run`/`test`, limites.
- [Matrice des gates T0–T12](../roadmap/CORE-1.0-GATE-MATRIX.md) §2 — **registre
  canonique**.
- [Rapport de la seconde vague de consolidation](../reports/GOAL-CONSOLIDATION-2-REPORT.md)
  — ce qui a été exécuté, `NOT_RUN` et `BLOCKED` consolidés.
- [État du langage](../reports/LANGUAGE-CORE-STATUS.md) §1 — mesures du
  2026-11-14 et limites structurelles.
- [Chemin vers la revue de production](../roadmap/PRODUCTION-READINESS.md) —
  R1–R14.
- [Temps mesurés](../reports/LANG-TIMINGS.md) et
  [performance de `check`/`build`](../qualification/PERFORMANCE.md).
- [LSP](../qualification/LSP.md), [VS Code](../qualification/VSCODE.md),
  [plateformes](../qualification/PLATFORMS-MATRIX.md).
- [FFI / `unsafe`](../ffi/SAFETY.md), [installation](INSTALL.md),
  [provenance](PROVENANCE.md), [checklist de release](RELEASE-CHECKLIST.md).
- [Décisions qui attendent un humain](../../DECISIONS_REQUIRED.md).

## 9. Ce que ces notes ne sont pas

- Ce ne sont **pas** les notes d'une version livrée : aucun tag, aucune release,
  aucun artefact signé, aucune date de disponibilité.
- Ce n'est **pas** un `CHANGELOG` de diffusion : le dépôt ne porte pas de tag de
  référence pour un diff de release.
- Ce n'est **pas** une revendication de maturité, de stabilité d'ABI/API, de
  performance garantie, ni de sûreté mémoire générale.
- Ce n'est **pas** un audit : T4 et T12 restent humains, D7/D9/D10 restent
  ouvertes ou différées.
- Aucun chiffre de durée, de coût ou de délai n'y est estimé : le dépôt
  n'en publie pas.
