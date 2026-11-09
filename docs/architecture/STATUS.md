# Statut architectural — Ravum (mise à jour MVP-0b, 8 novembre 2026)

> **Document de situation, non un certificat de réalisation.** Ce qui est marqué
> « mesuré » l'a été sur la machine de référence locale, avec les limites publiées dans
> les rapports liés. Aucun ADR n'est accepté, aucune release n'existe, aucune garantie
> hard-RT ni preuve générale n'est revendiquée.

| Élément | État |
|---|---|
| Charte P0-A…P0-I | Documentée, validation humaine en attente |
| ADR-000 à ADR-041 | 42 propositions originales ; **toutes PROPOSED** |
| Consolidation v0.3 | Revue multi-IA ; **REVIEW CANDIDATE**, non ratifiée |
| Compilateur Ravum / frontend / runtime | Non implémentés |
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
| MVP-0c / L2 — modifications réelles | **Gate franchi (bench), collecteur non branché au produit** : analyse 6 éditions → 1 (1 159 → 204–240 ms, −74 à −83 %), seuils `p*` 27–68 %, mixte p≈50 % non rentable ; **de bout en bout avec vrai `cargo test`** : 8 960 → 2 765 ms (**−69,1 %**), 12/12 tests identiques + digest identique, collecteur `ravum-batch` réel (w=0/50), repli N+1 sans perte, point d'équilibre 6 195 ms ; un lot invalide coûte ≈ le séquentiel ; pas encore câblé au daemon/CLI — [MVP-0c-build-performance.md](../mvp/MVP-0c-build-performance.md), [collecteur](../mvp/MVP-0c-batch-collector.md) |
| MVP-0c / L3 — collaboration | **Partiel avancé** : protocole v2 versionné (plus d'`AUTO_SAFE` textuel), 10/10 cas, adaptateur `ravum-collab-oracle` sur le graphe L1, publication Git robuste (15 tests de crash réels, aucun défaut de sûreté), **campagne S8 réelle exécutée** (27 subagents DSH, 9 campagnes, tokens mesurés : A 111 s < B 238 s < C 528 s, B **pas** plus rapide, 0 perte) — [MVP-0c-collaboration.md](../mvp/MVP-0c-collaboration.md), [oracle](../mvp/MVP-0c-collab-oracle.md), [publication](../mvp/MVP-0c-publication-robustness.md), [S8](../mvp/MVP-0c-s8.md) |
| MVP-0c / L4 — qualification | **Partiel** : **491 tests / 0 échec / 77 suites**, CI par impact (59 cas / 655 assertions, E2E navigateur déclenchés par impact et **vérifiés sur CI réelle** : runs + dispatch, +26–29 s), 10/10 E2E local+CI, 9 tests de stress DSP, reclassification des échecs transitoires, flake auto-GC corrigé, preuves de test opt-in ; CI produit+impact verte sur `24d92b1` ; CI par impact sur une vraie PR vers `dev` bloquée (`dev` sans crate) ; aucun prouveur externe (Kani bloqué, WSL indisponible) — [rapport](../mvp/MVP-0c-REPORT.md), [qualification](../mvp/MVP-0c-qualification.md) |
| MVP-0c / L5 — intégration | **Avancé** : CLI `ravum unit-check` + `collab-cycle --protocol v2 --oracle unit-graph` (47 tests), **audit final `READY_FOR_HUMAN_REVIEW`** (0 bloquant, 0 secret, 0 conflit simulé), **gate `fmt` rustfmt 1.95 + toolchain épinglée**, **preuves anonymisées** (63 JSON, 0 chemin utilisateur), simulation de fusion rejouée (0 conflit, arbres documentés), rapport consolidé, sessions Obsidian, PR #82 draft ; aucune fusion, aucun ADR accepté — [checklist](../mvp/MVP-0c-integration-checklist.md) |
| Vérification bornée (M2) | `Proved` limité au domaine énuméré, ledger append-only, cache invalidé par mutation ; prouveur externe **absent** (`Unsupported`) |
| DSP de bloc (M3) | Démonstrateur : ABI de bloc, état possédé par l'hôte, swap sans verrou, crossfade optionnel ; **pas d'hôte audio réel** |
| Verticale full-stack (M4) | Application témoin : UI statique, API HTTP, SQLite, contrat v1, migrations additives, E2E API + **E2E navigateur** ; pas de TLS |
| Prouveur externe (Kani…) | **Non installé** : l'adaptateur renvoie `Unsupported`, jamais un succès |
| Protection de branches | **Appliquée** sur `main` et `dev` : PR requises (0 approbation), force push et suppression interdits, check `mvp` requis sur `main` (#77) |
| Licence | **Analyse publiée, aucune licence choisie** ; audit automatique qui refuse une décision implicite — [MVP-0b-licence-analysis.md](../mvp/MVP-0b-licence-analysis.md) |
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

- Aucune garantie hard-RT, aucun XRUN mesuré, aucun hôte audio réel.
- Cible P0-A « diagnostic p95 < 50 ms sur code modifié » **non atteinte** ; cible
  « petit patch p95 < 500 ms » non re-mesurée de bout en bout.
- Aucune preuve que le workspace partagé est plus productif que les worktrees avec de
  vrais agents LLM (S8 utilise des éditeurs scriptés ; tokens non mesurés).
- Le groupement de patches (L2) n'est **pas implémenté** ; sa rentabilité mesurée est
  conditionnelle (seuil `p*` = 27–68 % selon N) et le régime de conflits réel en
  production n'est pas mesuré.
- La campagne S8 avec de vrais agents et statistiques d'usage reste à exécuter ; la
  procédure est prête mais aucun token réel n'a encore été mesuré.
- Le CLI ne branche pas encore l'analyse inter-crates ni l'oracle sémantique, et la CI
  par impact n'a pas été exercée sur une vraie PR vers `dev` (`dev` sans crate).
- `cargo fmt --check` n'est pas un gate : ~305 fichiers ne sont pas au format
  rustfmt 1.95 (dérive de version), sans impact fonctionnel démontré.
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
