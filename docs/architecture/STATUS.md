# Statut architectural — Ravum (mise à jour 2026-11-13)

> **Deux lignes de travail, deux statuts — ne pas mélanger leurs chiffres.**
>
> 1. **Plateforme Rust** (`ravum`, `dev`, MVP-0a/b/c/d) : boucle analyse →
>    build → vérification → hot patch sur du code Rust. C'est l'objet principal
>    des lignes ci-dessous.
> 2. **Langage Core 1.0** (`crates/ravum-lang-*`) : compilateur réel, **fusionné
>    dans `main` le 2026-11-12** (PR #101 → `dev` `a2b8db5`, puis PR #104 →
>    `main` `94f3ac4`), cible décidée **D2 « production ready »**. Il ne vit plus
>    sur une branche séparée. État mesuré le 2026-11-13 (phase 0 du GOAL,
>    `c931fdf`) : conformance **471 cas : 470 PASS / 0 FAIL / 1 SKIP**,
>    `build_verified: true`, workspace **1574 passés / 0 échec / 2 ignorés**.
>    C'est un sous-ensemble Core 1.0 **DRAFT** : aucun ADR ratifié, aucun tag,
>    aucune release, et l'audit externe (T4) comme le Go/No-Go (T12) restent
>    humains. Registre canonique :
>    [CORE-1.0-GATE-MATRIX.md](../roadmap/CORE-1.0-GATE-MATRIX.md) ; chiffres de
>    référence et seuils de non-régression :
>    [GOAL-BASELINE.md](../reports/GOAL-BASELINE.md).

> **Document de situation, non un certificat de réalisation.** Ce qui est marqué
> « mesuré » l'a été sur la machine de référence locale, avec les limites publiées dans
> les rapports liés. Aucun ADR n'est accepté, aucune release n'existe, aucune garantie
> hard-RT ni preuve générale n'est revendiquée.

| Élément | État |
|---|---|
| Charte P0-A…P0-I | Documentée, validation humaine en attente |
| ADR-000 à ADR-041 | 42 propositions originales ; **toutes PROPOSED** |
| Consolidation v0.3 | Revue multi-IA ; **REVIEW CANDIDATE**, non ratifiée |
| Intégration `dev` | **MVP-0a, 0b et 0c présents sur `dev`** le 9 novembre 2026 (`6b0ac7b`) via une chaîne d'intégration propre : 114 commits produit rejoués, outillage d'agents local exclu, arbres vérifiés identiques hors outillage — [intégration](../mvp/INTEGRATION-0abc.md) |
| MVP-0d / Lot A — incrémentalité | **Implémenté et testé** : activation Cargo résolue (`cargo metadata`), `context_id` v2 (features, crate types, build script, kind), `--cfg feature` transmis à rustc, chemins normalisés (déterminisme du `.rmeta`) ; **replis conservateurs maintenus** pour build scripts et dépendances optionnelles — [MVP-0d-incremental](../mvp/MVP-0d-incremental.md) |
| MVP-0d / Lot B — overlays | **Prototype testé, partiel** : `BaseSnapshot` content-adressée, overlays privés, intégration tout ou rien, CAS de base, conflits de chemin refusés, 9 tests dont concurrence 8 threads ; stockage mesuré −92,0 % vs 8 copies complètes ; **publication Git non branchée**, campagne A/B/C/D non exécutée — [MVP-0d-overlays](../mvp/MVP-0d-overlays.md) |
| MVP-0d / Lot C — groupement adaptatif | **Implémenté et mesuré** (trace synthétique) : fenêtre bornée 10–100 ms, croissance sur densité, rétrécissement sur rareté ; fixe 0/50/100 = 12/4/4 lots, attente 200/360/510 ms ; adaptative = 4 lots / 450 ms — [MVP-0d-batch-window](../mvp/MVP-0d-batch-window.md) |
| MVP-0d / Lot D — projets représentatifs | **`NOT_RUN`** : aucun corpus externe exécuté pendant ce cycle — [rapport](../mvp/MVP-0d-REPORT.md) |
| Langage Core 1.0 (`ravum-lang`) | **Intégré à `main` depuis le 2026-11-12** : lexer/parser, résolution, typage, ownership, effets, codegen Rust, CLI, LSP, 471 cas de conformance. Mesures du 2026-11-13 (`c931fdf`) : 470 PASS / 0 FAIL / 1 SKIP, `build_verified: true` (périmètre 202/202), smoke 43/0/0. Sous-ensemble **DRAFT** : ADR non ratifiés, audit externe et Go/No-Go humains absents, aucun `PROVED`, aucune release. Détail : [LANGUAGE-CORE-STATUS](../reports/LANGUAGE-CORE-STATUS.md), [GOAL-BASELINE](../reports/GOAL-BASELINE.md) |
| Outil Rust MVP-0a (M1) | **Implémenté, expérimental** : boucle analyse → build → vérification → hot patch, voir [docs/mvp/architecture.md](../mvp/architecture.md) |
| Hot swap natif (une entrée `f64 -> f64`) | **Démontré** par tests et démonstration ; migration d'état **non** implémentée |
| MVP-0b / L0 — audit et baseline | **Fait** : 139/139 tests reproduits, démonstrations M1–M4 reproduites, risques R1–R10 publiés — [MVP-0b-audit.md](../mvp/MVP-0b-audit.md) |
| MVP-0b / L1-A — cycle de vie temps réel DSP | **Corrigé et mesuré** : plus aucune destruction de module sur le callback (199 517 blocs, 0 allocation/0 désallocation) — [MVP-0b-rt-lifecycle.md](../mvp/MVP-0b-rt-lifecycle.md) |
| MVP-0b / L1-B — publication Git transactionnelle | **Implémentée** : index isolé, `commit-tree`, `update-ref` CAS, hooks honorés, journal durable, reprise — [MVP-0b-collab-2-3.md](../mvp/MVP-0b-collab-2-3.md) (voir aussi le rapport de cycle) |
| MVP-0b / L2 — daemon persistant d'analyse | **Implémenté et mesuré** : contenu identique ≈ 0 ms, modifications réelles 165–208 ms (p50), gain `-C incremental` −30/−36 % au-delà de ~3 200 lignes, rejet des résultats périmés, annulation réelle ; **GC de rétention borné** mesuré (politique explicite, protections, −66 % de disque sur 40 éditions) et **GC automatique opt-in** (12 collectes, disque borné à ~70 Ko contre 210 Ko sans collecte) — [MVP-0b-incremental.md](../mvp/MVP-0b-incremental.md) |
| MVP-0b / P0-A & P0-I — boucle de bout en bout | **Mesurée** : contenu inchangé, cycle complet 85–177 ms → **8–13 ms** (×10) ; édition réelle dominée par le build à froid ; **scheduling chevauché analyse ∥ build** : 390 → 303 ms (petite unité) et 593 → 412 ms (2 313 lignes), **p95 < 500 ms atteint sur les deux tailles**, scheduling chevauché productisé dans `watch`/`run` (`--concurrent`, opt-in) — [MVP-0b-cycle.md](../mvp/MVP-0b-cycle.md) |
| MVP-0b / L4-B — crossfade DSP | **Implémenté et mesuré** : clics 0,108 → 0,029 (IIR) et 0,657 → 0,058 (gain), 0 allocation/désallocation callback — [MVP-0b-dsp-crossfade.md](../mvp/MVP-0b-dsp-crossfade.md) |
| MVP-0b / L4-C — audio réel | **Exécuté sur périphérique `waveOut` (silence uniquement)** : 512 frames tient le temps réel (ratio 1,001, 0 alloc/0 free sur le thread de rendu, 11/11 swaps à chaud, crossfade 0,0291 vs 0,1260 sans fondu) ; l'ouverture en 256 frames sur la voie MOTU par défaut ne tient pas (ratio 0,53) ; sous charge CPU 100 %, lateness max ≈208 ms **sans allocation** ; **MMCSS « Pro Audio » + critical mesuré sous charge contrôlée** : 0 tampon en retard, max 2,5–2,7 ms, marge +29 ms (sans MMCSS : jusqu'à 255 tampons en retard, max 134 ms, marge −112 ms) ; **WASAPI mode partagé réel** mesuré (MOTU, float32 48 kHz, buffer 22 ms, ratio 1,001, 0 événement en retard, 0 alloc/free, 11/11 swaps) et **mode exclusif** mesuré/refusé par endpoint (MOTU refuse `AUDCLNT_E_UNSUPPORTED_FORMAT`, campagne NVIDIA HDMI PCM16 48 kHz 10 ms, ratio 1,0000) ; **aucune qualification hard-RT**, XRUN non observables — [MVP-0b-audio-host.md](../mvp/MVP-0b-audio-host.md) |
| MVP-0b / L4-D — vérification et prouveur externe | **Bornée conservée** ; **aucun prouveur externe installé** (`kani` absent → `Unsupported`) ; un test instable préexistant du ledger de preuves a été identifié et corrigé — [MVP-0b-verification.md](../mvp/MVP-0b-verification.md) |
| MVP-0b / L3 — collaboration multi-agents | COLLAB-1 conservé ; COLLAB-2/3 implémentés avec vérification du fusionné ; **benchmark S8 exécuté** (agents scriptés, pas des LLM) — [MVP-0b-collab-2-3.md](../mvp/MVP-0b-collab-2-3.md), [MVP-0b-s8.md](../mvp/MVP-0b-s8.md) |
| MVP-0b / L3.4 — collaboration ↔ daemon | **Implémenté et mesuré** : unités affectées + dépendants path-deps seulement, unité indépendante jamais analysée, identités vérifiées fichier par fichier, rejeu 0 `rustc` (cache mémoire/disque) — [MVP-0b-collab-4.md](../mvp/MVP-0b-collab-4.md) |
| MVP-0b / L4-A — E2E navigateur | Harnais CDP sans dépendance npm, scénarios réels sur l'UI — [MVP-0b-browser-e2e.md](../mvp/MVP-0b-browser-e2e.md) |
| MVP-0b / L4-E — CI sélective | Graphe d'impact conservateur et fail-closed ; suite complète conservée — [MVP-0b-ci.md](../mvp/MVP-0b-ci.md) |
| MVP-0c / L0 — audit et plan | **Fait** : suite 347/0 reproduite, audit structuré des PR #78/#80 (0 secret, 0 binaire, 0 fichier supprimé), plan des six lots — [MVP-0c-plan.md](../mvp/MVP-0c-plan.md) |
| MVP-0c / L1 — incrémentalité inter-crates | **Implémenté et testé** : crate `ravum-unit-graph` (`cargo metadata`, identités séparées, chaîne `--extern`, fallback Cargo, 21 tests dont cas central et fix root-package), daemon schéma 2 avec externs contenu-adressés **et protocole JSON-lines** (115 tests) ; rupture d'API de `core` détectée sur `api` **sans changement de ses sources** — [MVP-0c-cross-crate.md](../mvp/MVP-0c-cross-crate.md), [daemon](../mvp/MVP-0c-cross-crate-daemon.md) |
| MVP-0c / L2 — modifications réelles | **Gate franchi et collecteur câblé au CLI** : analyse 6 éditions → 1 (1 159 → 204–240 ms, −74 à −83 %), seuils `p*` 27–68 % ; **de bout en bout avec vrai `cargo test`** : 8 960 → 2 765 ms (**−69,1 %**), 12/12 tests + digest identiques ; `ravum batch-check` (collecteur + vrai `cargo test`, repli N+1, 4 tests) ; un lot invalide coûte ≈ le séquentiel ; intégration daemon non faite (non requise) — [MVP-0c-build-performance.md](../mvp/MVP-0c-build-performance.md), [CLI](../mvp/MVP-0c-cli.md) |
| MVP-0c / L3 — collaboration | **Partiel avancé** : protocole v2 versionné (plus d'`AUTO_SAFE` textuel), 10/10 cas, adaptateur `ravum-collab-oracle` sur le graphe L1, publication Git robuste (15 tests de crash réels, aucun défaut de sûreté), **campagne S8 réelle exécutée** (27 subagents DSH, 9 campagnes, tokens mesurés : A 111 s < B 238 s < C 528 s, B **pas** plus rapide, 0 perte) — [MVP-0c-collaboration.md](../mvp/MVP-0c-collaboration.md), [oracle](../mvp/MVP-0c-collab-oracle.md), [publication](../mvp/MVP-0c-publication-robustness.md), [S8](../mvp/MVP-0c-s8.md) |
| MVP-0c / L4 — qualification | **Partiel** : **495 tests / 0 échec / 78 suites**, CI par impact (59 cas / 655 assertions, E2E navigateur déclenchés par impact et **vérifiés sur CI réelle** : runs + dispatch, +26–29 s), 10/10 E2E local+CI, 9 tests de stress DSP, reclassification des échecs transitoires, flake auto-GC corrigé, preuves de test opt-in et anonymisées, gate `fmt` ; CI produit+impact verte sur `8fc1345` ; CI par impact sur une vraie PR vers `dev` bloquée (`dev` sans crate) ; aucun prouveur externe (Kani bloqué, WSL indisponible) — [rapport](../mvp/MVP-0c-REPORT.md), [qualification](../mvp/MVP-0c-qualification.md) |
| MVP-0c / L5 — intégration | **Avancé** : CLI `ravum unit-check` + `collab-cycle --protocol v2 --oracle unit-graph` (47 tests), **audit final `READY_FOR_HUMAN_REVIEW`** (0 bloquant, 0 secret, 0 conflit simulé), **gate `fmt` rustfmt 1.95 + toolchain épinglée**, **preuves anonymisées** (63 JSON, 0 chemin utilisateur), simulation de fusion rejouée (0 conflit, arbres documentés), rapport consolidé, sessions Obsidian, PR #82 draft ; **fusionné dans `dev`** le 9 novembre 2026 via une chaîne d'intégration propre (PR #83/#84/#85, `dev` = `6b0ac7b`), sans le commit d'outillage local `16b1ff2` ; aucun ADR accepté — [checklist](../mvp/MVP-0c-integration-checklist.md), [intégration](../mvp/INTEGRATION-0abc.md) |
| Vérification bornée (M2) | `Proved` limité au domaine énuméré, ledger append-only, cache invalidé par mutation ; prouveur externe **absent** (`Unsupported`) |
| DSP de bloc (M3) | Démonstrateur : ABI de bloc, état possédé par l'hôte, swap sans verrou, crossfade optionnel ; **pas d'hôte audio réel dans ce démonstrateur** (l'hôte L4-C, distinct, est décrit plus haut) |
| Verticale full-stack (M4) | Application témoin : UI statique, API HTTP, SQLite, contrat v1, migrations additives, E2E API + **E2E navigateur** ; pas de TLS |
| Prouveur externe (Kani…) | **Non installé** : l'adaptateur renvoie `Unsupported`, jamais un succès |
| Protection de branches | **Appliquée** sur `main` et `dev` : PR requises (0 approbation), force push et suppression interdits, check `mvp` requis sur `main` (#77) |
| Licence | **`MIT OR Apache-2.0`** sur les 23 membres du workspace (D1 résolue ; PR de licence fusionnée) — [décision](../adr/README.md) |
| Gate M5 pour un nouveau langage | Proposed, **pas accepté** |
| Code de release | Aucun |

## Séparation des pouvoirs

1. Les exigences utilisateur P0 ont priorité sur les propositions des modèles et les ADR.
2. Les ADR originaux sont historiques. Une modification exige un nouvel amendement
   traçable, une revue et l'approbation humaine.
3. Les nouvelles propositions multi-IA sont des RFC ; elles ne reçoivent pas de numéro ADR
   collisionnant sans validation.
4. Un `PROVED` n'existe qu'avec des hypothèses, un périmètre et une base de confiance
   déclarés. Aujourd'hui aucun `PROVED` Ravum général n'a été émis ; les seuls `proved`
   sont bornés à un domaine fini explicitement énuméré.
5. Les prétentions des outils tiers ne sont pas des preuves de fonctionnement de Ravum.

## Ce qui reste non démontré (extrait)

- Aucune garantie hard-RT, aucun XRUN mesuré ; l'hôte audio de démonstration L4-C existe
  (`waveOut`/WASAPI, MMCSS mesurés) mais n'est pas qualifié temps réel
  ([audio réel](../mvp/MVP-0b-audio-host.md)).
- Cible P0-A « diagnostic p95 < 50 ms sur code modifié » **non atteinte** ; cible
  « petit patch p95 < 500 ms » non re-mesurée de bout en bout.
- Côté **langage** (`ravum-lang`), aucune mesure de latence par phase n'existe
  aujourd'hui (ni `--timings`, ni p50/p95) : la cible P0-A n'est donc **ni
  atteinte ni mesurée** pour un programme `.rv`. La mesure est planifiée par la
  phase 3 du chantier `goal/production-ready`.
- Aucune preuve que le workspace partagé est plus productif que les worktrees avec de
  vrais agents LLM (S8 utilise des éditeurs scriptés ; tokens non mesurés).
- Le regroupement adaptatif de patches est **implémenté et mesuré** (MVP-0d, Lot C) ; le
  Lot D (projets représentatifs) reste `NOT_RUN`. Sa rentabilité mesurée est conditionnelle
  (seuil `p*` = 27–68 % selon N) et le régime de conflits réel en production n'est pas mesuré.
- La campagne S8 avec de vrais agents et statistiques d'usage reste à exécuter ; la
  procédure est prête mais aucun token réel n'a encore été mesuré.
- Le CLI expose `ravum unit-check` et `collab-cycle --oracle unit-graph`
  (MVP-0c / L5) ; l'oracle sémantique de contrats n'est pas branché. La CI par impact a
  été exercée sur une vraie PR vers `dev` (PR #101, check `impact` vert) ; il manque une
  campagne sur plusieurs PR réelles.
- `cargo fmt --check` est un **gate bloquant** dans `mvp.yml` ; la dérive de formatage a été
  corrigée (`ab51b17`, rustfmt 1.95 épinglé). Le nombre de fichiers concernés avant
  correction (~305, chiffre du 9 novembre) n'est pas re-mesuré ici.
- Aucun prouveur externe, aucune propriété générale, aucun certificat indépendant.
- Aucune mesure de productivité humaine/agent jusqu'au livraison vérifié (P0-I).

## Références

- [42 ADR d'origine](../adr/README.md)
- [Consolidation des retours multi-IA](../reviews/2026-11-08-consolidation.md)
- [Feuille de route MVP](../roadmap/MVP.md)
- [Invariants proposés](PROJECT_INVARIANTS.md)
- Rapports MVP-0b : [audit](../mvp/MVP-0b-audit.md), [temps réel](../mvp/MVP-0b-rt-lifecycle.md),
  [crossfade](../mvp/MVP-0b-dsp-crossfade.md), [incrémental](../mvp/MVP-0b-incremental.md),
  [audio réel](../mvp/MVP-0b-audio-host.md), [vérification](../mvp/MVP-0b-verification.md),
  [licence](../mvp/MVP-0b-licence-analysis.md), [S8](../mvp/MVP-0b-s8.md),
  [E2E navigateur](../mvp/MVP-0b-browser-e2e.md), [CI impact](../mvp/MVP-0b-ci.md),
  [COLLAB-2/3](../mvp/MVP-0b-collab-2-3.md).
- Rapports MVP-0c : [rapport consolidé](../mvp/MVP-0c-REPORT.md),
  [plan](../mvp/MVP-0c-plan.md), [audit PR](../mvp/MVP-0c-pr-audit.md),
  [cross-crate](../mvp/MVP-0c-cross-crate.md), [daemon externs](../mvp/MVP-0c-cross-crate-daemon.md),
  [performance](../mvp/MVP-0c-build-performance.md), [collaboration](../mvp/MVP-0c-collaboration.md),
  [oracle L1](../mvp/MVP-0c-collab-oracle.md), [publication](../mvp/MVP-0c-publication-robustness.md),
  [vérification](../mvp/MVP-0c-verification.md), [S8](../mvp/MVP-0c-s8.md),
  [E2E](../mvp/MVP-0c-e2e.md), [stress DSP](../mvp/MVP-0c-dsp-stress.md),
  [qualification](../mvp/MVP-0c-qualification.md).
