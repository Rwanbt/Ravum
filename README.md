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
| Outil Rust MVP-0a (M1) | **Implémenté et testé localement** : boucle `check → build → verify → hot patch`, voir [docs/mvp/architecture.md](docs/mvp/architecture.md) |
| Hot swap natif | **Démontré** sur une entrée `f64 -> f64` à ABI vérifiée ; migration d'état **non** implémentée |
| Moteur de preuve Ravum / qualification RT | **Non implémentés** ; vérification bornée M2 présente, aucun PROVED général, aucun prouveur externe installé |
| Mesures de la boucle M1 | **Mesurées** sur la machine de référence ; locales et non portables, voir [evidence/](evidence/README.md) |
| Vérification bornée (M2) | **Implémentée** : obligations sur domaine fini explicite, ledger append-only, invalidation par mutation ; `PROVED` **uniquement** pour le domaine déclaré |
| DSP à chaud (M3) | **Démonstrateur de bloc** : état possédé par l'hôte, publication sans verrou, garde temps réel, **0 allocation et 0 désallocation** mesurées sur le thread audio ; crossfade de remplacement optionnel ; aucun périphérique audio |
| Verticale full-stack (M4) | **Application témoin** : UI statique + API HTTP + SQLite, contrat versionné, validation serveur, migrations additives, 10 tests E2E ; aucun navigateur piloté, aucun TLS |
| Prouveur externe (Kani…) | **Non installé** : l'adaptateur renvoie `Unsupported`, jamais un succès |
| MVP-0b : daemon persistant (L2) | **Implémenté et mesuré** : cache de verdicts (contenu identique ≈ 0 ms), rejet des résultats périmés, annulation réelle ; gain incrémental seulement au-delà de ~1 000 lignes |
| MVP-0b : Git transactionnel (L1) | **Implémenté** : index isolé, `commit-tree`, `update-ref` CAS, hooks honorés, journal durable ; 16 tests négatifs |
| CI produit | **Ajoutée** (build + tests + démonstrations) ; elle ne certifie ni preuve, ni temps réel, ni stabilité d'ABI |

Les fichiers `.ul` des exemples initiaux illustrent seulement des **idées de syntaxe**, pas du code compilable aujourd'hui.

## MVP-0a : ce qui fonctionne réellement

Une modification de source déclenche l'analyse ciblée, la compilation d'une unité de
patch, une vérification faible mais honnête (`tested`), puis l'activation à chaud
dans un **processus déjà en cours d'exécution**, sans redémarrage. Toute étape qui
échoue laisse la version précédente active.

```text
ravum check  <fichier.rs>    # analyse seule ; --daemon = moteur persistant (cache de verdicts)
ravum build  <fichier.rs>    # cdylib après une analyse propre
ravum patch  <artefact>      # validation ABI / manifeste / symboles
ravum run    <fichier.rs>    # application de démonstration
ravum watch  <fichier.rs>    # boucle complète : édition -> activation
ravum status                 # dernier état enregistré
ravum bench  <a.rs> <b.rs>   # mesures par étape, min/p50/p95/p99 + calibration de charge
ravum prove  <fichier.rs>    # vérifie une obligation bornée et l'enregistre dans le ledger
ravum artifacts [--prune]    # liste ou élague les artefacts (jamais celui qui est actif)
ravum dsp    <fichier.rs>    # session DSP mesurée, remplacement à chaud optionnel (--next) et crossfade (--crossfade-frames)
```

Démonstrations exécutables : `scripts/demo-m1.ps1` (hot patch d'un processus vivant)
et `scripts/demo-collab.ps1` (propositions multi-agents, conflit explicite, écrivain
Git unique).

**Limites assumées** : un seul point d'entrée `f64 -> f64` ; une panique dans un
patch avorte le processus hôte (`panic=abort`) ; aucune migration d'état, aucune
fusion AST/CRDT, aucune garantie temps réel. Un statut `PROVED` n'est émis que pour
un **domaine fini explicitement énuméré** (jamais pour une propriété générale), et
l'absence de prouveur externe produit `Unsupported`, pas un succès.

**Cibles de latence : partiellement atteintes en MVP-0b, sans fausse promesse.** Le
daemon persistant ramène l'analyse d'un contenu **inchangé** de ~145 ms à **< 1 ms**,
mais une **modification réelle** coûte encore 165–208 ms (p50) : la cible diagnostic
< 50 ms n'est donc **pas** atteinte pour du code modifié. Le gain de `-C incremental`
n'apparaît qu'au-delà de ~1 000 lignes (−12/−24 %) et atteint −30/−36 % vers 3 200+
lignes. Le patch complet (< 500 ms) n'est pas re-mesuré de bout en bout. Mesures et
verdict : [docs/mvp/MVP-0b-incremental.md](docs/mvp/MVP-0b-incremental.md),
[docs/mvp/M2-evidence.md](docs/mvp/M2-evidence.md).

**DSP** : le chemin audio est sans verrou, sans allocation **et sans désallocation**
(mesuré sur 199 517 blocs), avec un crossfade de remplacement optionnel qui réduit les
clics mesurés en M3 (0,108 → 0,029 pour un coefficient IIR ; 0,657 → 0,058 pour un
doublement de gain). Il n'y a **aucun périphérique audio, aucune priorité temps réel** :
une mesure d'ordonnanceur OS sur machine chargée montre déjà 0,4–2,6 ms de gigue pour
une période de 5,33 ms, donc aucune revendication hard-RT. Détail :
[docs/mvp/MVP-0b-rt-lifecycle.md](docs/mvp/MVP-0b-rt-lifecycle.md),
[docs/mvp/MVP-0b-dsp-crossfade.md](docs/mvp/MVP-0b-dsp-crossfade.md),
[docs/mvp/MVP-0b-audio-host.md](docs/mvp/MVP-0b-audio-host.md).

### Application témoin full-stack (M4)

```text
cargo run -p ravum-todo -- --db target/ravum/todo.db --port 8080 --token dev-token
```

UI statique (aucune étape de build), API HTTP, SQLite avec migrations versionnées,
types partagés exposés à `/api/schema`, validation et autorisation côté serveur.
Les tests E2E démarrent le serveur sur une vraie socket et une vraie base. Limites :
aucun navigateur piloté, aucun TLS, une seule connexion SQLite. Détail :
[docs/mvp/M4-evidence.md](docs/mvp/M4-evidence.md).

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

### MVP-0b — rapports de ce cycle

- [Audit L0 et baseline reproductible](docs/mvp/MVP-0b-audit.md)
- [L1-A — cycle de vie temps réel des modules DSP](docs/mvp/MVP-0b-rt-lifecycle.md)
- [L4-B — crossfade de remplacement DSP](docs/mvp/MVP-0b-dsp-crossfade.md)
- [L2 — daemon persistant d'analyse incrémentale](docs/mvp/MVP-0b-incremental.md)
- [L4-C — chemin audio réel : ce qui est mesuré et ce qui ne l'est pas](docs/mvp/MVP-0b-audio-host.md)
- [L4-D — vérification bornée et prouveur externe](docs/mvp/MVP-0b-verification.md)
- [L5.3 — analyse de licence et inventaire tiers](docs/mvp/MVP-0b-licence-analysis.md)

## Branches et contributions

- **`main`** : branche par défaut, documentation et états de référence revus ; pas de faux label « stable ».
- **`dev`** : branche d'intégration du développement ; les changements arrivent par PR et validations.
- **`feat/<issue>-description`**, **`fix/<issue>-description`**, **`research/<issue>-description`** : branches courtes issues de `dev`.

L'usage des PR, des tests pertinents ciblés et des tests E2E complets périodiques vise une CI rapide **sans masquer les véritables échecs**. Les critères de promotion vers `main` doivent être explicitement définis puis appliqués.

Les propositions IA sont les bienvenues, mais **aucun agent n'accepte un ADR ou n'affaiblit un contrat normatif sans décision humaine**. Les revues doivent citer les paragraphes réels des ADR.

## Licence, contributions et état des outils

La **licence du futur code** doit être choisie avant une contribution externe de code ; **aucune licence n'est actuellement déclarée**, donc n'assumez pas que le contenu soit librement réutilisable. Les analyses de licences font partie de M0. Les outils tiers étudiés ne sont pas vendus comme composants déjà intégrés.

Le projet est ouvert à la discussion sur les [issues](https://github.com/Rwanbt/Ravum/issues). Consultez la roadmap avant de commencer un ticket.
