<!-- Bannière du dépôt Ravum -->
![Ravum — langage universel pour le développement continu](https://raw.githubusercontent.com/Rwanbt/Ravum/main/assets/ravum-banner.jpg)

# Ravum

<p align="center">
  <a href="README.md"><img src="https://img.shields.io/badge/Language-English-2f81f7?style=for-the-badge" alt="Read in English"></a>
</p>

> **Projet de recherche et d'ingénierie — Core 1.0 DRAFT, non publié.**
> Un compilateur Ravum expérimental existe et exécute un sous-ensemble Core 1.0
> documenté : lexer/parser, résolution de noms, vérification de types, profils
> d'effets, vérification d'ownership et d'emprunts, contrôles de traps, génération
> de Rust, CLI, LSP, corpus de conformance, cibles natives Windows/Linux et
> `wasm32-wasip1`. Il n'est **ni stable, ni audité à l'extérieur, ni
> « production ready »**. Les ADR restent **PROPOSED**, et il n'existe aucun tag,
> aucune release, aucune signature. Le registre canonique des gates est
> [`docs/roadmap/CORE-1.0-GATE-MATRIX.md`](docs/roadmap/CORE-1.0-GATE-MATRIX.md).

**Ravum** est un projet exploratoire de **langage de programmation universel** et
d'**environnement de développement continu**. L'objectif à long terme est de
raccourcir le temps entre une édition — humaine ou générée par une IA — et un
livrable vérifié et exécutable, du frontend au backend, jusqu'au DSP temps réel,
aux applications natives et aux systèmes embarqués.

Le projet explore une boucle fail-closed : une édition déclenche une analyse
ciblée, une vérification, une compilation focalisée et une prévisualisation
actualisée. Si une modification est incomplète ou incorrecte, la dernière version
valide reste active. Une affirmation doit être adossée à une commande, un SHA,
une toolchain et une plateforme ; elle n'est jamais déduite d'une démo verte.

## Principes fondateurs (P0)

| ID | Objectif non négociable |
|---|---|
| **P0-A** | **Fluidité** — feedback rapide pendant la frappe ou la génération par IA ; compilation et preview incrémentales. |
| **P0-B** | **Performances** — natif, faible overhead, code compact, SIMD, multicœur, sans GC obligatoire. |
| **P0-C** | **Sécurité mémoire** — ownership et emprunts, frontières `unsafe` explicites. |
| **P0-D** | **Vérification honnête** — contrats, preuves ciblées, tests déterministes et statuts explicites. |
| **P0-E** | **IA native** — diagnostics structurés, contre-exemples, conservation des contrats et correction automatisable. |
| **P0-F** | **Temps réel** — profils vérifiables d'effets/ressources, notamment pour l'audio DSP et les plugins VST3/CLAP. |
| **P0-G** | **Universalité** — web, serveur, base de données, desktop, mobile, embarqué, outils système, calcul/GPU. |
| **P0-H** | **Sobriété** — compilateur permanent avec budgets CPU/RAM ; machine de référence initiale de 16 Go. |
| **P0-I** | **Productivité globale** — mesurer le temps jusqu'à un **livrable vérifié**, pas seulement les microbenchmarks de compilation. |

« Universel » ne signifie **pas** qu'on peut prouver automatiquement tout
programme, garantir zéro bug ou crash, migrer arbitrairement toutes les
structures en hot swap, ou garantir une échéance hard real-time sur tout OS/CPU.
Les garanties doivent mentionner leurs hypothèses, versions, plateformes et
frontières de confiance. Les statuts `TESTED`, `ASSUMED`, `UNKNOWN`, `PENDING`,
`STALE`, `TIMEOUT` et `UNSUPPORTED` ne sont **jamais** présentés comme `PROVED`.

## Stratégie : apprendre avant de figer

**F — stratégie expérimentale :** construire une petite plateforme utilisable sur
Rust existant : snapshots immuables, diagnostics, orchestration, patch ABI
contrôlé et métrologie. Elle doit apporter une valeur autonome.

**D — architecture cible conditionnelle :** si les expériences le justifient,
développer la sémantique propre à Ravum, des IR vérifiables et des backends
natifs. Le projet Rust outillé reste la baseline et une alternative légitime.

La décision de construire un frontend autonome appartient au gate **M5**, sur
résultats reproductibles : compilation incrémentale de définition, temps
éditer→preview, coûts CPU/RAM, correction IA sur syntaxe nouvelle, garanties
réellement imposées et coût de maintenance.

## État en un coup d'œil

Le **2026-11-15**, le dépôt ne conserve que deux branches : `main` et `dev`.
Elles sont synchronisées après la fusion de la vague 3 (PR #111 à #113), et aucune
PR de travail n'est laissée ouverte.

| Domaine | État actuel |
|---|---|
| Langage | Sous-ensemble Core 1.0 **DRAFT** ; compilateur et CLI implémentés dans `crates/ravum-lang-*` |
| Pipeline | `.rv` → loader/parser → résolution → type check → effets → ownership → traps → `CheckedProgram` → `emit.rs` → Rust → `rustc 1.95.0` |
| CLI | `check`, `build`, `run`, `watch`, `test`, `fmt`, `lint`, `doc`, `cache`, `lsp`, `doctor` |
| Cibles | Natives Windows/Linux ; `wasm32-wasip1` via Node.js WASI |
| Conformance | **487 cas : 485 PASS / 1 FAIL / 1 SKIP** sur la machine de référence ; l'échec est un cas connu, volontairement rouge ; `build_verified: false` |
| Tests du workspace | **1 732 passés / 0 échec / 8 ignorés** |
| Couverture | **91,65 % lignes / 91,32 % régions** (sema + codegen) ; **90,37 % de code produit** (1 305 non couvertes / 13 548) ; la cible S5 de 100 % n'est **pas atteinte** |
| Performance | Exposant de la phase `own` **1,9803 → 0,8372** ; `check` à 50 110 lignes **602,6 ms** (debug) ; LSP p95 **19,551 ms** à 1 025 lignes et **80,283 ms** à 5 110 lignes (release, machine calme) |
| Gates | `PASS` : T0, T3, T5, T7 — `PARTIAL` : T1, T2, T6, T8, T9, T10, T11 — `BLOCKED` : T4, T12 |
| Release | Aucun tag, release, artefact signé, audit externe ni décision Go/No-Go |

> Chiffres de référence : Windows 11 x64, `rustc 1.95.0`, commit de code
> `5fd54b8` (2026-11-14), fusionné en `400336b` dans `dev` et `20e65d2` dans
> `main` le 2026-11-15. Le premier run CI Linux après la fusion de la vague 3 a
> mesuré 484 PASS / 1 FAIL / 2 SKIP ; le FAIL est le même cas connu et le skip
> supplémentaire dépend de l'environnement. Ces chiffres sont locaux et liés à
> la CI ; ce ne sont pas des garanties portables. Voir le
> [rapport de la vague 3](docs/reports/GOAL-WAVE-3-REPORT.md).

## Ce qui fonctionne réellement

- **Compiler et exécuter de vrais programmes.** Paquets `.rv` avec `ravum.toml`,
  exemples multi-fichiers, `check`, `build`, `run`, `watch`, `test`, `fmt`,
  `lint`, `doc`, `cache` et `doctor` exécutent le vrai pipeline. Une étape du
  frontend encore incomplète émet un diagnostic stable et sort en erreur ; elle
  ne rapporte jamais un faux succès. La CLI le documente explicitement.
- **Un vrai serveur de langage.** LSP 3.17 sur stdio : diagnostics, hover,
  définition, références, renommage, complétion, symboles et formatage. Il est
  exercé par un test client intégré et par une vraie session VS Code via
  l'extension `tools/vscode-ravum`.
- **Une frontière FFI contrainte.** Seule une ABI C scalaire documentée est
  acceptée ; `i128` et `u128` sont refusés par **le vérificateur et le backend**.
  Les fonctions qui déroulent, `longjmp`, lèvent un signal ou terminent le
  processus sont hors contrat (C-FFI-4). `unsafe` est confiné et audité.
- **Un outillage de preuve reproductible.** Le runner de conformance vérifie le
  contrat `check → build`, la suite smoke exerce les exemples, et les scripts de
  `tools/` auditent la couverture, les résultats de mutation, les codes de
  registre, la traçabilité des features (`188/188`), la couverture MUST
  (`809 / 465 / 344`) et `unsafe`.
- **La ligne plateforme Rust.** Les crates `ravum-*` séparées fournissent la
  plateforme expérimentale pour du code Rust : analyse incrémentale, daemon
  persistant, publication Git transactionnelle, ABI de hot patch, traitement DSP
  par blocs et application de référence full-stack `ravum-todo`. MVP-0a/0b/0c
  sont intégrés ; MVP-0d est partiel.

## Cas de conformance rouge connu

> **Le gate de conformance n'est pas vert, et c'est délibéré.**
>
> `conformance/cases/run/power-array-move-write-region-nested` enregistre une
> vraie divergence `check → build`. `check` sort à 0 ; `build` sort à 1 avec
> `error[E0508]: cannot move out of type [String; 2]`. Le cas reste rouge
> jusqu'au correctif du planificateur de moves de tableau. C'est un
> **résultat**, pas un incident de gate, et il n'est ni ignoré ni supprimé.
>
> Sur l'arbre de la fusion de la vague 3 (`400336b`), le check requis `mvp`, la
> couverture, les contrats de gate, la documentation et VS Code passent. Les
> jobs de conformance Linux, Windows et macOS échouent sur ce seul cas, et
> `Qualification evidence` échoue par conséquence. Aucune assertion, aucun cas
> de test ni aucun seuil n'a été affaibli pour obtenir du vert.

## Démarrage rapide

Prérequis : Rust `1.95.0` (épinglé par `rust-toolchain.toml`), `rustc` sur le
`PATH`, et Node.js uniquement pour la cible WASI ou les harnais VS Code/navigateur.

```bash
cargo build -p ravum-lang-cli --bin ravum-lang
./target/debug/ravum-lang doctor
./target/debug/ravum-lang run examples/hello
```

Sous Windows, le binaire est `target\debug\ravum-lang.exe`.

Autres commandes utiles :

```bash
./target/debug/ravum-lang check examples/calculator
./target/debug/ravum-lang test examples/tests-demo
./target/debug/ravum-lang lsp

cargo test --workspace --all-targets -- --test-threads=2
cargo run -p ravum-conformance -- run --verify-build
```

La commande de conformance sort à 1 tant que le cas rouge connu existe : le
résultat de référence est 487 cas, 485 PASS / 1 FAIL / 1 SKIP, avec
`build_verified: false`. Le FAIL est attendu et documenté ci-dessus.

## Organisation du dépôt

| Chemin | Contenu |
|---|---|
| `crates/ravum-lang-*` | Compilateur Core 1.0 : syntaxe, analyse sémantique, effets, ownership, codegen, LSP et CLI |
| `crates/ravum-*` | Ligne plateforme Rust : daemon, coordinateur, patch host, snapshots, harnais DSP, ledger de preuves, cœur full-stack |
| `conformance/` | Cas `.rv` positifs et négatifs, dont la divergence volontairement rouge |
| `spec/language/` | Core 1.0 DRAFT normatif : grammaire, ABI, features, core machine |
| `tools/` | Gates de couverture, mutation, registre, MUST, timings et evidence |
| `docs/` | Décisions, matrice des gates, production readiness, RFC, architecture, rapports de qualification |
| `examples/` | Programmes et paquets `.rv` compilables |
| `scripts/` | Scripts smoke, démo, timings et evidence |
| `apps/ravum-todo` | Application de référence full-stack utilisée par MVP-0a/0b/0c |
| `.github/workflows/` | CI Linux/macOS, MVP, couverture, performance, reproductibilité, VS Code et conformance |

## Gates, readiness et preuves

La matrice des gates est la source unique pour T0–T12. Le document de production
readiness suit R1–R14 par rapport à la cible D2 (« production ready »). Un `PASS`
n'est jamais écrit sans commande exécutée, SHA et contexte reproductible ; `SKIP`
et `NOT_RUN` ne sont jamais convertis en `PASS`, et aucun `PROVED` général
n'existe.

| Statut | Gates |
|---|---|
| `PASS` | T0, T3, T5, T7 |
| `PARTIAL` | T1, T2, T6, T8, T9, T10, T11 |
| `BLOCKED` | T4 (audit externe), T12 (Go/No-Go) |

La vague 3 n'a changé aucun verdict de gate. R2 est `PARTIAL` car la divergence
ci-dessus est un cas ouvert connu ; R4 et R13 sont `BLOCKED` par l'audit externe ;
R12 est `DIFFÉRÉ` par la décision sur la clé de signature ; les autres critères
sont `PARTIAL` ou `TESTED`.

Preuves principales :

- [Matrice des gates T0–T12](docs/roadmap/CORE-1.0-GATE-MATRIX.md)
- [Production readiness R1–R14](docs/roadmap/PRODUCTION-READINESS.md)
- [Rapport et chiffres de la vague 3](docs/reports/GOAL-WAVE-3-REPORT.md)
- [Décisions du responsable du 2026-11-14](docs/decision/DECISIONS-2026-11-14.md)
- [Décisions humaines encore requises](docs/reports/HUMAN-DECISIONS-REQUIRED.md)
- [Résultats du test de puissance T2](docs/qualification/T2-FINDINGS.md)
- [Qualification LSP](docs/qualification/LSP.md)
- [Objectif de couverture et écarts](docs/qualification/COVERAGE-100-OBJECTIVE.md)

## Feuille de route

| Piste | Maintenant | Prochaine décision ou preuve |
|---|---|---|
| Langage Core 1.0 | Compilateur DRAFT, LSP, conformance et backend Rust dans `main`/`dev` | Fermer T4 (audit indépendant) et T12 (Go/No-Go) ; corriger la divergence connue des moves de tableau |
| Ligne plateforme Rust | MVP-0a/0b/0c intégrés ; MVP-0d partiel | Corpus de projets externes pour MVP-0d ; migration d'état et ABI de patch formelle pour le DSP |
| Qualité | Couverture 90,37 % du code produit ; campagne de mutation partielle | Ratifier ou rejeter les 14 exemptions de couverture ; terminer la campagne de mutation `emit.rs` |
| Confiance | Compilations reproductibles sur deux runners ; contrat d'installateur Linux | Artefacts signés (D9) ; audit externe (D4) |

Aucune date de livraison n'est promise. La cible du langage est un sous-ensemble
Core 1.0 documenté et fail-closed — pas une affirmation que des programmes
arbitraires sont sûrs ou vérifiés.

## En attente de décisions humaines

Décisions ouvertes : **D2** (ratifier le socle normatif et RFC-D2), **D4** (audit
indépendant), **D9** (clé de signature), **T12** (Go/No-Go),
**D-2026-11-14-6** et **D-2026-11-14-7**.

La vague 3 laisse aussi ces décisions : garder le cas rouge connu dans le gate ou
le tenir hors gate jusqu'au correctif ; ratifier ou rejeter les 14 exemptions de
couverture (140 lignes) ; autoriser une extension du schéma `OwnFacts` pour les
cinq familles bloquées ; décider la campagne complète de mutation `emit.rs` en
CI ; décider la capacité du runner LSP ; mettre à jour les documents aux chiffres
périmés. La dette MUST préexistante est de 344 énoncés non mappés (deux lignes contenant
un `|` non échappé subsistent dans la spec).

## Branches et contributions

- **`main`** est la branche par défaut et la référence documentaire, pas une
  affirmation de maturité. **`dev`** est la branche d'intégration. Le travail part
  de `dev`, passe par une PR vers `dev`, et les branches courtes sont supprimées
  après fusion.
- `mvp` est un check requis sur les deux branches. Les force pushes et la
  suppression de branches sont désactivés sur `main` et `dev`.
- Ne supprimez, n'ignorez ni n'affaiblissez jamais des tests, cas de conformance,
  assertions ou seuils pour obtenir du vert. Rapporter les échecs honnêtement.
- Les propositions d'IA sont bienvenues, mais aucun agent n'accepte un ADR, ne
  modifie un contrat normatif ou n'affaiblit un gate sans décision humaine
  explicite.
- Voir [CONTRIBUTING.md](CONTRIBUTING.md) et [AGENTS.md](AGENTS.md).

## Documentation

Les pages de statut sont datées. En cas de chiffre dupliqué, la matrice des gates
et le rapport de la vague 3 prévalent sur les pages de statut plus anciennes.

- [Démarrage avec Core 1.0 DRAFT](docs/guide/GETTING-STARTED.md)
- [Statut architectural — deux lignes séparées](docs/architecture/STATUS.md)
- [Matrice des gates Core 1.0](docs/roadmap/CORE-1.0-GATE-MATRIX.md)
- [Production readiness et plan d'écart](docs/roadmap/PRODUCTION-READINESS.md)
- [Rapport de campagne de la vague 3](docs/reports/GOAL-WAVE-3-REPORT.md)
- [Décisions du responsable du 2026-11-14](docs/decision/DECISIONS-2026-11-14.md)
- [Décisions humaines requises](docs/reports/HUMAN-DECISIONS-REQUIRED.md)
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
