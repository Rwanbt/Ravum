<!-- Bannière du dépôt Ravum -->
![Ravum — langage universel pour le développement continu](https://raw.githubusercontent.com/Rwanbt/Ravum/main/assets/ravum-banner.jpg)

# Ravum

<p align="center">
  <a href="README.md"><img src="https://img.shields.io/badge/English-2f81f7?style=for-the-badge" alt="Read in English"></a>
</p>

**Ravum** est un langage de programmation universel expérimental et un
environnement de développement continu. Il vise à raccourcir le chemin entre une
édition de source — humaine ou générée par une IA — et un programme vérifié et
exécutable, grâce à l'analyse incrémentale, la compilation ciblée et une
prévisualisation fail-closed.

> **État du projet — Core 1.0 DRAFT.** Un compilateur et un serveur de langage
> existent et prennent en charge un sous-ensemble documenté. Il n'existe ni
> release stable, ni audit externe, ni revendication de maturité « production
> ready ». Les ADR restent des propositions.

## Aperçu

Ravum s'articule autour de neuf invariants de travail : fluidité, performances,
sécurité mémoire, vérification honnête, feedback IA natif, profils temps réel,
universalité, sobriété des ressources et productivité de bout en bout. Ce sont
des exigences d'ingénierie, pas des arguments marketing. L'ensemble est maintenu
dans
[`docs/architecture/PROJECT_INVARIANTS.md`](docs/architecture/PROJECT_INVARIANTS.md).

Le dépôt contient actuellement une implémentation expérimentale du langage
Core 1.0 et une plateforme Rust séparée. Le langage est la cible à long terme ;
la plateforme est une stratégie expérimentale pour appliquer les mêmes idées de
développement continu à du code Rust existant.

## État

| Composant | État actuel |
|---|---|
| Langage | Sous-ensemble Core 1.0 **DRAFT** ; compilateur et CLI implémentés dans `crates/ravum-lang-*` |
| Outillage | `check`, `build`, `run`, `watch`, `test`, `fmt`, `lint`, `doc`, `cache`, `lsp`, `doctor` |
| Cibles | Natives Windows/Linux ; `wasm32-wasip1` via Node.js WASI |
| Conformance | **503 cas : 502 PASS / 0 FAIL / 1 SKIP** sur la machine de référence ; `build_verified: true` |
| Tests du workspace | **1 740 passés / 0 échec / 8 ignorés** |
| Couverture | **91,65 % lignes / 91,32 % régions** (sema + codegen) ; **90,37 % de code produit** (1 305 non couvertes / 13 548) ; la cible de 100 % n'est **pas atteinte** |
| Performance | Exposant de la phase `own` **1,9803 → 0,8372** ; `check` à 50 110 lignes **602,6 ms** (debug) ; LSP p95 **19,551 ms** à 1 025 lignes et **80,283 ms** à 5 110 lignes (release, machine calme) |
| Release | Aucun tag, aucune release, aucun artefact signé, aucun audit externe |

> Environnement de référence : Windows 11 x64, `rustc 1.95.0`, état du code du
> 2026-11-15 (étape E1 du chantier production readiness). Ces chiffres sont liés
> aux commandes et à la révision citées ; ce ne sont pas des garanties portables.
> Ces chiffres se reproduisent avec les commandes de la section
> [Développement](#développement) ; le protocole détaillé est dans
> [`T2-FINDINGS.md` §29](docs/qualification/T2-FINDINGS.md#295-vérification-e1-2026-11-15).

## Démarrage rapide

Prérequis : Rust `1.95.0` (épinglé par `rust-toolchain.toml`), `rustc` sur le
`PATH`, et Node.js uniquement pour la cible WASI ou les harnais éditeur/navigateur.

```bash
cargo build -p ravum-lang-cli --bin ravum-lang
./target/debug/ravum-lang doctor
./target/debug/ravum-lang run examples/hello
```

Sous Windows, le binaire est `target\debug\ravum-lang.exe`. Une exécution
réussie affiche `hello, ravum`.

## Outillage

### Pipeline du compilateur

```text
.rv → loader/parser → résolution de noms → vérification de types → effets
    → ownership et emprunts → contrôles de traps → CheckedProgram
    → emit.rs → Rust → rustc 1.95.0
```

Le compilateur émet du Rust et délègue la génération d'objets à `rustc`. Il n'y a
ni interpréteur caché ni repli qui rapporte un succès lorsqu'une étape est
indisponible.

### Interface en ligne de commande

```text
ravum-lang init <dir> [--lib] [--force]
ravum-lang check [PATH] [--json] [--profile core|pure|rt] [--rt-entry FN]
ravum-lang build [PATH] [--release] [--target TRIPLE] [--emit-rust FILE]
ravum-lang run   [PATH] [--target TRIPLE] [-- ARGS...]
ravum-lang watch [PATH] [--run] [--release]
ravum-lang test  [PATH] [--filter TEXT] [--json]
ravum-lang fmt   [PATH] [--check]
ravum-lang lint  [PATH] [--deny CODE]...
ravum-lang doc   [PATH] [--out DIR]
ravum-lang cache status|clean [PATH]
ravum-lang lsp
ravum-lang doctor [--json]
```

Les chemins sont relatifs au répertoire courant par défaut. Un répertoire doit
contenir `ravum.toml` ; un fichier `.rv` seul est vérifié comme programme
autonome. Une sortie JSON versionnée est disponible pour `check`, `test` et
`doctor`.

### Serveur de langage

`ravum-lang lsp` implémente LSP 3.17 sur stdio : diagnostics, hover, définition,
références, renommage, complétion, symboles de document et formatage. Le client
est couvert par des tests d'intégration et par l'extension VS Code
[`tools/vscode-ravum`](tools/vscode-ravum).

### Cibles et FFI

Les binaires natifs sont la cible par défaut ; les builds `wasm32-wasip1`
s'exécutent via Node.js WASI. La FFI C est volontairement réduite : seule une ABI
scalaire documentée est acceptée. `i128` et `u128` sont refusés par le
vérificateur et par le backend. Les fonctions qui déroulent, appellent
`longjmp`, lèvent un signal ou terminent le processus sont hors contrat et
restent de la responsabilité de l'appelant.

## État de la conformance

`conformance/cases/run/power-array-move-write-region-nested` était une vraie
divergence `check → build` : `check` acceptait le programme, mais le backend
émettait de simples accès `a[0]`/`a[1]` et `rustc` les rejetait avec
`error[E0508]: cannot move out of type [String; 2]`. Le défaut est corrigé dans
le compilateur : les moves d'éléments de tableau sont abaissés à l'accès au lieu
d'être hissés par un planificateur, et le cas reste dans le corpus de régression.
La mesure de référence donne **503 cas : 502 PASS / 0 FAIL / 1 SKIP** et
`build_verified: true`.

`build_verified` ne couvre que la dimension build. Le corpus est borné et ne
prouve pas l'absence de forme inconnue ; ce n'est pas une preuve formelle. La
commande `--verify-build`, son périmètre et ses limites sont documentés dans
[`CONFORMANCE-BUILD-CONTRACT.md`](docs/qualification/CONFORMANCE-BUILD-CONTRACT.md).

## Limites actuelles

- **Couverture.** Le code produit est à 90,37 % contre un objectif de 100 % ;
  1 305 lignes restent non couvertes.
- **Aucune garantie de release.** Il n'existe ni contrat stable pour la CLI, le
  JSON ou le LSP, ni artefact signé, ni audit externe, ni décision Go/No-Go.
- **Vérification bornée.** Le ledger de preuves couvre des domaines finis
  explicitement énumérés ; il n'existe ni moteur de preuve Ravum général ni
  `PROVED` général.
- **Capacités runtime partielles.** Il n'y a ni hot reload préservant l'état ni
  ABI de patch stable. MVP-0d (incrémentalité et collaboration) est partiel.
- **Couverture du test de puissance partielle.** La campagne T2 a exercé 9 des
  13 formes historiques ; 4 formes inverses n'ont pas été exécutées.
- **Performances liées à la machine.** Les temps publiés proviennent d'une seule
  machine de référence Windows. Aucune garantie hard real-time n'est revendiquée.

## Organisation du dépôt

| Chemin | Contenu |
|---|---|
| `crates/ravum-lang-*` | Compilateur Core 1.0 : syntaxe, analyse sémantique, effets, ownership, codegen, LSP et CLI |
| `crates/ravum-*` | Plateforme Rust : daemon, coordinateur, patch host, snapshots, harnais DSP, ledger de preuves, cœur full-stack |
| `conformance/` | Cas `.rv` positifs et négatifs, dont le cas actuellement en échec |
| `spec/language/` | Core 1.0 DRAFT normatif : grammaire, ABI, features, core machine |
| `tools/` | Gates de couverture, mutation, registre, MUST, timings et evidence |
| `docs/` | Architecture, matrice des gates, production readiness, RFC, rapports de qualification |
| `examples/` | Programmes et paquets `.rv` compilables |
| `scripts/` | Scripts smoke, démo, timings et evidence |
| `apps/ravum-todo` | Application de référence full-stack utilisée par la plateforme Rust |
| `.github/workflows/` | CI Linux/macOS, MVP, couverture, performance, reproductibilité, VS Code et conformance |

## Architecture

Ravum comporte deux lignes d'implémentation séparées :

- **Langage Core 1.0** (`crates/ravum-lang-*`) : parser, résolveur, vérificateur
  de types, profils d'effets, analyse d'ownership, contrôles de traps, génération
  de Rust, CLI et LSP. C'est le chemin vers un outillage Ravum natif.
- **Plateforme Rust** (`crates/ravum-*`) : snapshots, analyse incrémentale,
  daemon persistant, publication Git transactionnelle, ABI de patch et traitement
  DSP par blocs pour du code Rust. Elle a une valeur autonome et sert de baseline
  au travail sur le langage.

La plateforme est la stratégie expérimentale **F**. Un backend Ravum natif est la
cible conditionnelle **D** ; la décision appartient au gate M5 et dépend de
mesures reproductibles du temps éditer→preview, de l'usage des ressources, des
garanties réellement imposées et du coût de maintenance.

## Développement

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --jobs 2 -- -D warnings
cargo deny check
cargo test --workspace --all-targets -- --test-threads=2
cargo run -p ravum-conformance -- run --verify-build
```

La commande de conformance sort à 0 lorsque le corpus et la dimension
`check → build` sont tous deux verts ; un `SKIP` est rapporté séparément et n'est
jamais compté comme un PASS. Des vérifications ciblées sont aussi disponibles par
crate, par exemple :

```bash
cargo test -p ravum-lang-sema
cargo test -p ravum-lang-codegen
```

Le job CI `mvp` vérifie le formatage, exécute Clippy avec les avertissements
refusés, valide la politique de chaîne d'approvisionnement avec `cargo-deny`,
construit le workspace, exécute la suite de tests, les démos M1–M4 et la suite
E2E navigateur. Le job de conformance exécute le corpus `check → build`.
Dependabot met à jour les dépendances et les GitHub Actions chaque semaine ;
`CODEOWNERS` définit la responsabilité de revue.

## Contribution

Le développement suit un workflow de branches courtes :

1. Créer une branche depuis `dev`.
2. Garder le changement ciblé et ajouter des tests pour le nouveau comportement
   et les refus.
3. Exécuter le formatage, Clippy avec les avertissements refusés,
   `cargo deny check`, les tests pertinents et, le cas échéant, le corpus de
   conformance.
4. Ajouter le changement visible dans [`CHANGELOG.md`](CHANGELOG.md).
5. Ouvrir une pull request vers `dev`. Le check requis `mvp` doit passer.

`main` est la branche par défaut et la référence documentaire. `dev` est la
branche d'intégration. Les branches durables sont `main` et `dev` ; les branches
de travail sont supprimées après fusion.

Les tests, cas de conformance, assertions et seuils font partie du contrat. Ne
les supprimez pas, ne les ignorez pas et ne les affaiblissez pas pour obtenir du
vert. Rapportez les échecs avec la commande, le SHA, la toolchain et la plateforme
qui les ont produits. Les contributions automatisées sont bienvenues selon les
mêmes règles de revue.

Voir [CONTRIBUTING.md](CONTRIBUTING.md) pour le processus complet et
[AGENTS.md](AGENTS.md) pour les règles applicables aux agents IA.

## Gates de qualité

| Statut | Gates |
|---|---|
| `PASS` | T0, T3, T5, T7 |
| `PARTIAL` | T1, T2, T6, T8, T9, T10, T11 |
| `BLOCKED` | T4 (audit externe), T12 (Go/No-Go) |

Les critères R1–R14 sont suivis séparément. R4 et R13 sont bloqués par l'audit
externe, R12 est différé, et les autres critères sont `PARTIAL` ou `TESTED`.
Aucun gate n'est promu sans commande, SHA et contexte enregistré. Le registre
canonique est la [matrice des gates](docs/roadmap/CORE-1.0-GATE-MATRIX.md) ; le
plan d'écart est [production readiness](docs/roadmap/PRODUCTION-READINESS.md).

## Feuille de route

| Piste | Prochain jalon |
|---|---|
| Langage Core 1.0 | Corriger la divergence des moves de tableau ; fermer T4 (audit externe) et T12 (Go/No-Go) |
| Plateforme Rust | Terminer le corpus externe MVP-0d ; ajouter la migration d'état et une ABI de patch stable pour le DSP |
| Qualité | Remonter la couverture produit vers 100 % ; terminer la campagne de mutation `emit.rs` |
| Distribution | Produire des artefacts signés et vérifier la provenance (R12) |

Aucune date de livraison n'est promise. La cible est un sous-ensemble Core 1.0
documenté et fail-closed, pas une affirmation que des programmes arbitraires sont
sûrs ou vérifiés.

## Documentation

- [Démarrage avec Core 1.0 DRAFT](docs/guide/GETTING-STARTED.md)
- [Invariants du projet](docs/architecture/PROJECT_INVARIANTS.md)
- [Matrice des gates Core 1.0](docs/roadmap/CORE-1.0-GATE-MATRIX.md)
- [Production readiness et plan d'écart](docs/roadmap/PRODUCTION-READINESS.md)
- [Dernier rapport de qualification](docs/reports/GOAL-WAVE-3-REPORT.md)
- [Index des RFC](docs/rfc/README.md)
- [Registre des ADR originaux](docs/adr/README.md) — tous proposés
- [Intégration MVP-0a/0b/0c](docs/mvp/INTEGRATION-0abc.md)
- [Rapport MVP-0d](docs/mvp/MVP-0d-REPORT.md)
- [Contribuer](CONTRIBUTING.md)
- [Mandat des agents IA](AGENTS.md)

## Licence

**MIT OR Apache-2.0**, au choix du réutilisateur. Voir
[LICENSE-MIT](LICENSE-MIT) et [LICENSE-APACHE](LICENSE-APACHE). Cette double
licence couvre les contributions originales dont les auteurs détiennent les
droits ; les composants, textes et ressources tiers restent soumis à leurs
propres licences. Les programmes écrits avec Ravum n'héritent pas automatiquement
de la licence du compilateur du seul fait de l'utilisation du langage ou de la
toolchain.
