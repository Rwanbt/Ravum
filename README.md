<!-- Ravum repository banner -->
![Ravum — Universal language for continuous development](https://raw.githubusercontent.com/Rwanbt/Ravum/main/assets/ravum-banner.jpg)

# Ravum

> **Projet de recherche et d'ingénierie — état : cadrage architectural / pré-MVP (8 novembre 2026).**  
> **Aucun compilateur Ravum, runtime, moteur de preuves ni chaîne de hot reload Ravum n'est encore implémenté ou qualifié.** Les ADR sont des **propositions**, les benchmarks sont des **objectifs à mesurer**, et aucun logiciel n'est actuellement « production-ready ».

**Ravum** est un projet de **langage de programmation universel** et d'**environnement de développement continu**, conçu pour raccourcir le temps nécessaire à la création d'applications sûres, performantes et maintenables — du frontend et du backend jusqu'au DSP temps réel, aux applications natives et aux systèmes embarqués.

Le projet explore une boucle où l'édition humaine ou la génération par agent IA déclenche l'analyse incrémentale, la vérification, la compilation ciblée et une **prévisualisation actualisée à chaud**. La dernière version valide reste active si une modification est incomplète ou incorrecte. Le code de production doit rester performant et les garanties ne doivent jamais être inventées.

**English:** Ravum is an exploratory universal programming language and continuous development platform. It aims for safe, high-performance native/full-stack code, explicit verifiable guarantees and fast incremental edit-to-preview cycles. **There is no working compiler yet.**

## Principes fondateurs (P0)

| ID | Objectif non négociable |
|---|---|
| **P0-A** | **Fluidité** : feedback rapide pendant la frappe et la génération par IA ; compilation et preview incrémentales. |
| **P0-B** | **Performances** : natif, faible overhead, code compact, SIMD, multicœur, sans GC obligatoire. |
| **P0-C** | **Sécurité mémoire** : ownership/emprunts, frontières `unsafe` explicites. |
| **P0-D** | **Vérification honnête** : contrats, preuves ciblées, tests déterministes et statuts explicites. |
| **P0-E** | **IA native** : diagnostics structurés, contre-exemples, conservation des contrats et correction automatisable. |
| **P0-F** | **Temps réel** : profils vérifiables d'effets/ressources, notamment pour l'audio DSP et les plugins VST3/CLAP. |
| **P0-G** | **Universalité** : web, serveur, base de données, desktop, mobile, embarqué, outils système, calcul/GPU. |
| **P0-H** | **Sobriété** : compilateur permanent avec budget CPU/RAM ; machine de référence initiale de 16 Go. |
| **P0-I** | **Productivité globale** : mesurer le temps jusqu'à un **livrable vérifié**, pas uniquement les microbenchmarks de compilation. |

### Limites assumées

« Universel » ne signifie **pas** qu'on peut prouver automatiquement tout programme, garantir zéro bug/crash, migrer arbitrairement toutes les structures en hot swap, ou garantir une échéance hard real-time sur tout OS/CPU. Les garanties doivent mentionner les **hypothèses, versions, plateformes et frontières de confiance**.

Les statuts `TESTED`, `ASSUMED`, `UNKNOWN`, `PENDING`, `STALE`, `TIMEOUT`, `UNSUPPORTED` ne sont **jamais** présentés comme `PROVED`. Un `PROVED` futur doit déclarer sa base de confiance et sa portée.

## Où en est Ravum ?

| Domaine | État réel |
|---|---|
| Vision et exigences | Documentées, non ratifiées globalement |
| ADR-000 à ADR-041 | **PROPOSED** ; aucun accepté automatiquement |
| Revue multi-IA | Réalisée sur documents, résultats contradictoires conservés |
| Architecture F → D | **Hypothèse candidate**, à comparer |
| Compilateur Ravum / langage exécutable | **Non implémenté** |
| Daemon / hot swap / moteur de preuve Ravum | **Non implémentés** |
| Benchmarks Ravum / qualifications RT | **Aucun résultat mesuré** |
| MVP-0a | **Planifié, non commencé** |
| CI produit | **Non qualifiée** ; la CI documentaire éventuelle ne certifie pas du code exécutable |

Les fichiers `.ul` des exemples initiaux illustrent seulement des **idées de syntaxe**, pas du code compilable aujourd'hui.

## Stratégie d'implémentation : apprendre avant de figer

**F — stratégie expérimentale :** construire une petite plateforme utilisable sur Rust existant : snapshots immuables, diagnostics, orchestration, patch ABI contrôlé et métrologie. Elle doit apporter une valeur autonome.

**D — architecture cible conditionnelle :** si les expériences le justifient, développer la sémantique, les IR vérifiables et les backends propres du langage Ravum. Un projet Rust outillé reste la **baseline** et une alternative légitime.

La décision de construire un frontend autonome est réservée au **gate M5**, sur résultats reproductibles : compilation incrémentale de définition, temps éditer→preview, coûts CPU/RAM, correction IA sur syntaxe nouvelle, garanties réellement imposées et coût de maintenance.

## Feuille de route

| Jalons | Résultats attendus (non réalisés à ce jour) |
|---|---|
| **M0** Cadrage + baseline | Exigences traçables, corpus gelé, machine de référence, protocole S8, mesures Rust de référence |
| **M1** Boucle continue MVP-0a | Snapshot → analyse → diagnostic structuré → build → patch compatible → activation / refus |
| **M2** Preuves et invalidation | Vérificateur isolé, mutations S2, statuts et cache **non autoritatif** avant validation |
| **M3** DSP temps réel | Bascule préparée hors callback, aucune allocation/lock/destruction coûteuse sur thread audio |
| **M4** Full-stack | Mini-app web + API + SQLite, types partagés et E2E réels |
| **M5** Décision Ravum autonome | Comparer (A) Rust outillé, (B) mini-Ravum→Rust, (C) mini-Ravum→Cranelift |
| **M6+** Déploiement évolutif | Langage, multi-backends, portabilité, certificats avancés selon preuves et ressources |

**Aucune date de livraison n'est garantie.** Les jalons sont conditionnés aux mesures et aux critères Go/No-Go.

## Architecture de travail proposée

```text
Éditeur / agent IA / CLI
          │ overlays de source et intentions de modification
          ▼
Coordinateur incrémental (snapshots, priorités, annulation, quotas)
          ├── Adaptateurs d'analyse / diagnostics
          ├── Tests, modèles et preuves (avec contexte et confiance)
          ├── Codegen + linker (comparés par benchmark)
          └── PatchHost versionné / preview, fail-closed
                         │
                Application en cours
```

Interfaces propres au projet, adaptateurs externes remplaçables, aucun composant abstrait sans consommateur effectif. Une C ABI contrôlée est **candidate pour le démonstrateur**, mais n'est pas une ABI Ravum publiée ni une garantie de sécurité à elle seule.

## Documentation

- [Registre des 42 ADR originaux](docs/adr/README.md) — tous proposés.
- [Charte, exigences et traçabilité](docs/specs/01-exigences-tracabilite.md)
- [Plan directeur](docs/plan/00-plan-directeur.md)
- [Plan MVP et jalons](docs/roadmap/MVP.md)
- [Expériences S1–S10](docs/plan/01-spikes-et-benchmarks.md)
- [Politique CI et tests](docs/plan/03-gates-ci-qualification.md)
- [Consolidation de la revue multi-IA](docs/reviews/2026-11-08-consolidation.md)
- [Architecture et statut des décisions](docs/architecture/STATUS.md)
- [Backlog et user stories](docs/roadmap/BACKLOG.md)
- [Contribuer](CONTRIBUTING.md)
- [Instructions pour les agents IA](AGENTS.md)
- [Premières tâches à réaliser](docs/roadmap/START_HERE.md)

## Branches et contributions

- **`main`** : branche par défaut, documentation et états de référence revus ; pas de faux label « stable ».
- **`dev`** : branche d'intégration du développement ; les changements arrivent par PR et validations.
- **`feat/<issue>-description`**, **`fix/<issue>-description`**, **`research/<issue>-description`** : branches courtes issues de `dev`.

L'usage des PR, des tests pertinents ciblés et des tests E2E complets périodiques vise une CI rapide **sans masquer les véritables échecs**. Les critères de promotion vers `main` doivent être explicitement définis puis appliqués.

Les propositions IA sont les bienvenues, mais **aucun agent n'accepte un ADR ou n'affaiblit un contrat normatif sans décision humaine**. Les revues doivent citer les paragraphes réels des ADR.

## Licence, contributions et état des outils

**Licence : `MIT OR Apache-2.0` (au choix du réutilisateur).** Consultez [LICENSE-MIT](LICENSE-MIT) et [LICENSE-APACHE](LICENSE-APACHE). Cette double licence concerne les contributions originales au projet dont les auteurs détiennent les droits ; les composants, textes ou ressources tiers restent soumis à leurs propres licences et avis. Les programmes créés avec Ravum n'héritent pas automatiquement de cette licence du seul fait de l'utilisation du langage ou du compilateur. Les outils tiers étudiés ne sont pas vendus comme composants déjà intégrés.

Le projet est ouvert à la discussion sur les [issues](https://github.com/Rwanbt/Ravum/issues). Consultez la roadmap avant de commencer un ticket.
