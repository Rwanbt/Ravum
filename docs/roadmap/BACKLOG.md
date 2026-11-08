# Backlog GitHub — Ravum

> **Snapshot documentaire du 8 novembre 2026.** Les issues réelles sont la source de vérité pour leurs états et leurs discussions. Les 42 ADR restent **PROPOSED**. Aucun prototype ni test du compilateur n'a encore été exécuté.

**Résumé initial :** 9 epics, 14 user stories, 53 tâches techniques ; 76 issues au total. Les identifiants `RAV-###` ne sont **pas** les numéros GitHub ; les liens ci-dessous pointent vers les vrais numéros.

## Epics (jalons et responsabilités)

- [#1](https://github.com/Rwanbt/Ravum/issues/1) — [EPIC][E01][M0] Charte, gouvernance, ADR et exigences
- [#2](https://github.com/Rwanbt/Ravum/issues/2) — [EPIC][E02][M0] Corpus, baselines et expérimentation IA
- [#3](https://github.com/Rwanbt/Ravum/issues/3) — [EPIC][E03][M1] Plateforme de développement continu et diagnostics
- [#4](https://github.com/Rwanbt/Ravum/issues/4) — [EPIC][E04][M1] Hot swap natif responsable et preview
- [#5](https://github.com/Rwanbt/Ravum/issues/5) — [EPIC][E05][M2] Vérification, preuves et invalidation
- [#6](https://github.com/Rwanbt/Ravum/issues/6) — [EPIC][E06][M3] DSP audio et contraintes temps réel
- [#7](https://github.com/Rwanbt/Ravum/issues/7) — [EPIC][E07][M4] Verticale full-stack et cohérence inter-cibles
- [#8](https://github.com/Rwanbt/Ravum/issues/8) — [EPIC][E08][M5] Gate d'autonomie du langage Ravum
- [#9](https://github.com/Rwanbt/Ravum/issues/9) — [EPIC][E09][M6+] Écosystème, portabilité et recherche avancée

## User stories

- [#10](https://github.com/Rwanbt/Ravum/issues/10) — [US-001][M1] Développeur : voir les erreurs pendant l'édition
- [#11](https://github.com/Rwanbt/Ravum/issues/11) — [US-002][M1] Développeur : preview sans redémarrage systématique
- [#12](https://github.com/Rwanbt/Ravum/issues/12) — [US-003][M1] Agent IA : recevoir des diagnostics structurés et stables
- [#13](https://github.com/Rwanbt/Ravum/issues/13) — [US-004][M0] Responsable : protéger les spécifications contre les agents
- [#14](https://github.com/Rwanbt/Ravum/issues/14) — [US-005][M1] Développeur sur 16 Go : garder la machine réactive
- [#15](https://github.com/Rwanbt/Ravum/issues/15) — [US-006][M2] Développeur : savoir exactement ce qui est prouvé
- [#16](https://github.com/Rwanbt/Ravum/issues/16) — [US-007][M3] Ingénieur audio : modifier le DSP sans compromettre le callback
- [#17](https://github.com/Rwanbt/Ravum/issues/17) — [US-008][M4] Full-stack : partager les contrats frontend/API/SQL
- [#18](https://github.com/Rwanbt/Ravum/issues/18) — [US-009][M2] Agent IA : reproduire un défaut déterministement
- [#19](https://github.com/Rwanbt/Ravum/issues/19) — [US-010][M0] Mainteneur : mesurer le temps jusqu'à un livrable vérifié
- [#20](https://github.com/Rwanbt/Ravum/issues/20) — [US-011][M5] Créateur de langage : vérifier l'intérêt d'une nouvelle syntaxe
- [#21](https://github.com/Rwanbt/Ravum/issues/21) — [US-012][M0] Contributeur : comprendre l'état réel et les ADR
- [#22](https://github.com/Rwanbt/Ravum/issues/22) — [US-013][M1] Développeur : rollback sûr après patch échoué
- [#23](https://github.com/Rwanbt/Ravum/issues/23) — [US-014][M6+] Développeur systèmes : réutiliser un cœur portable et minimal

## Tickets techniques par phase

### M0

- [#24](https://github.com/Rwanbt/Ravum/issues/24) — [RAV-001][M0] Valider la charte P0 et la matrice de traçabilité
- [#25](https://github.com/Rwanbt/Ravum/issues/25) — [RAV-002][M0] Auditer les 42 ADR et dédupliquer les RFC multi-IA
- [#26](https://github.com/Rwanbt/Ravum/issues/26) — [RAV-003][M0] Définir la machine de référence, pins et protocole de benchmark
- [#27](https://github.com/Rwanbt/Ravum/issues/27) — [RAV-004][M0] Créer fixtures de référence et corpus négatif
- [#28](https://github.com/Rwanbt/Ravum/issues/28) — [RAV-005][M0] Définir les cinq identités de snapshot et de preuve
- [#29](https://github.com/Rwanbt/Ravum/issues/29) — [RAV-006][M0] Versionner les schémas diagnostics/preuves/jobs et tester les statuts
- [#30](https://github.com/Rwanbt/Ravum/issues/30) — [RAV-007][M0] Créer un registre d'autorité pour les spécifications normatives
- [#31](https://github.com/Rwanbt/Ravum/issues/31) — [RAV-008][M0] Qualifier la CI documentaire et préparer gates de tests par impact
- [#32](https://github.com/Rwanbt/Ravum/issues/32) — [RAV-009][M0] Établir baseline Rust S1-R jusqu'au linking et à l'activation
- [#33](https://github.com/Rwanbt/Ravum/issues/33) — [RAV-010][M0] Pré-enregistrer S8-A/S8-B et exécuter pilote agent IA
- [#34](https://github.com/Rwanbt/Ravum/issues/34) — [RAV-011][M0] Décider licence du code, gouvernance et inventaire tiers
- [#35](https://github.com/Rwanbt/Ravum/issues/35) — [RAV-012][M0] Mettre au point la sélection des tests par impact sans masquer E2E
- [#36](https://github.com/Rwanbt/Ravum/issues/36) — [RAV-013][M0] Formaliser le gate M5 et les critères de pivot avant les résultats

### M1

- [#37](https://github.com/Rwanbt/Ravum/issues/37) — [RAV-014][M1] Implémenter SourceSnapshot et overlays immuables
- [#38](https://github.com/Rwanbt/Ravum/issues/38) — [RAV-015][M1] Adapter rust-analyzer via LSP et cargo via CLI
- [#39](https://github.com/Rwanbt/Ravum/issues/39) — [RAV-016][M1] Implémenter coordinator et scheduling prioritaire
- [#40](https://github.com/Rwanbt/Ravum/issues/40) — [RAV-017][M1] Rendre l'annulation et STALE observables et résistants aux crashs
- [#41](https://github.com/Rwanbt/Ravum/issues/41) — [RAV-018][M1] Exposer CLI check/watch/status et JSON versionné
- [#42](https://github.com/Rwanbt/Ravum/issues/42) — [RAV-019][M1] Prototyper PatchHost par modules dynamiques en C ABI
- [#43](https://github.com/Rwanbt/Ravum/issues/43) — [RAV-020][M1] Définir et vérifier manifeste ABI/layout de patch
- [#44](https://github.com/Rwanbt/Ravum/issues/44) — [RAV-021][M1] Gérer appels en vol, quiescence et reclamation différée
- [#45](https://github.com/Rwanbt/Ravum/issues/45) — [RAV-022][M1] Tester refus, rollback et compatibilité des patches
- [#46](https://github.com/Rwanbt/Ravum/issues/46) — [RAV-023][M1] Mesurer pipeline complet frappe→preview
- [#47](https://github.com/Rwanbt/Ravum/issues/47) — [RAV-024][M1] Qualifier la coexistence de DLL versionnées sous Windows et dylib Linux

### M2

- [#48](https://github.com/Rwanbt/Ravum/issues/48) — [RAV-025][M2] Créer un ProofBroker avec adaptateur externe Kani ou équivalent
- [#49](https://github.com/Rwanbt/Ravum/issues/49) — [RAV-026][M2] Implémenter statuts de preuve et politiques d'activation par profil
- [#50](https://github.com/Rwanbt/Ravum/issues/50) — [RAV-027][M2] Construire EvidenceLedger append-only et clés de contexte de vérification
- [#51](https://github.com/Rwanbt/Ravum/issues/51) — [RAV-028][M2] Exécuter S2 : mutation adversariale du cache de preuves
- [#52](https://github.com/Rwanbt/Ravum/issues/52) — [RAV-029][M2] Exécuter S6 : comparer solveurs et qualité des contre-exemples
- [#53](https://github.com/Rwanbt/Ravum/issues/53) — [RAV-030][M2] Créer S9 : simulateur déterministe de concurrence et pannes

### M3

- [#54](https://github.com/Rwanbt/Ravum/issues/54) — [RAV-031][M3] Implémenter fixture audio gain puis filtre IIR avec état
- [#55](https://github.com/Rwanbt/Ravum/issues/55) — [RAV-032][M3] Étudier analyse d'effets RT transitifs et disponibilité par plateforme
- [#56](https://github.com/Rwanbt/Ravum/issues/56) — [RAV-033][M3] Préparer le patch DSP hors thread et libérer l'ancien état hors RT
- [#57](https://github.com/Rwanbt/Ravum/issues/57) — [RAV-034][M3] Exécuter S4 : stress hot-swap multi-thread, état et ABI
- [#58](https://github.com/Rwanbt/Ravum/issues/58) — [RAV-035][M3] Exécuter S5 : mesures callbacks, XRUN et discontinuités DSP
- [#59](https://github.com/Rwanbt/Ravum/issues/59) — [RAV-036][M3] Qualifier un hôte audio CLAP/VST3 réel après harness

### M4

- [#60](https://github.com/Rwanbt/Ravum/issues/60) — [RAV-037][M4] Créer une application témoin UI + API Rust + SQLite
- [#61](https://github.com/Rwanbt/Ravum/issues/61) — [RAV-038][M4] Qualifier changements de schéma et migrations n/n-1
- [#62](https://github.com/Rwanbt/Ravum/issues/62) — [RAV-039][M4] Implémenter matrice E2E par impact + suite intégrale
- [#63](https://github.com/Rwanbt/Ravum/issues/63) — [RAV-040][M4] Comparer WIT/WASI et une frontière native versionnée
- [#64](https://github.com/Rwanbt/Ravum/issues/64) — [RAV-041][M4] Tester authentification, autorisation et exposition des secrets

### M5

- [#65](https://github.com/Rwanbt/Ravum/issues/65) — [RAV-042][M5] Implémenter S1-T : mini frontend à requêtes par définition
- [#66](https://github.com/Rwanbt/Ravum/issues/66) — [RAV-043][M5] Définir un oracle sémantique indépendant du comportement rustc
- [#67](https://github.com/Rwanbt/Ravum/issues/67) — [RAV-044][M5] Exécuter S3 : backend Rust bridge vs Cranelift vs LLVM candidat
- [#68](https://github.com/Rwanbt/Ravum/issues/68) — [RAV-045][M5] Exécuter S8-B : ergonomie et agent IA hors corpus du langage
- [#69](https://github.com/Rwanbt/Ravum/issues/69) — [RAV-046][M5] Rédiger la décision Go/No-Go du langage autonome
- [#70](https://github.com/Rwanbt/Ravum/issues/70) — [RAV-047][M5] Mettre en place parité comportementale dev/release

### M6+

- [#71](https://github.com/Rwanbt/Ravum/issues/71) — [RAV-048][M6+] Étudier portabilité mobile et intégration native Android/iOS
- [#72](https://github.com/Rwanbt/Ravum/issues/72) — [RAV-049][M6+] Explorer GPU/WebGPU et accélérateurs multi-cibles
- [#73](https://github.com/Rwanbt/Ravum/issues/73) — [RAV-050][M6+] Étudier hard-RT, WCET et exigences par plateforme
- [#74](https://github.com/Rwanbt/Ravum/issues/74) — [RAV-051][M6+] Définir stratégie d'écosystème paquets, macros et plugins
- [#75](https://github.com/Rwanbt/Ravum/issues/75) — [RAV-052][M6+] Explorer certificats de preuve indépendants et TCB minimal
- [#76](https://github.com/Rwanbt/Ravum/issues/76) — [RAV-053][M6+] Mesurer empreinte énergétique et budgets par profil de cible

## Politique d'exécution

1. Commencer par M0, sans affaiblir P0-A…P0-I. Utiliser `dev` pour intégrer les PR de travail.
2. Produire une preuve de test négatif et la trace SHA/toolchain/cible pour chaque ticket.
3. Prioriser la boucle continue M1, puis S2, S4, S5 et S8 ; l'architecture finale du langage n'est pas préjugée.
4. Ne jamais « résoudre » une CI rouge en supprimant les E2E en échec ; sélectionner les tests par impact conservateur et utiliser une suite intégrale périodique.
5. Aucun ADR n'est accepté par issue close sans décision humaine explicite.

Voir [MVP](MVP.md), [protocole des spikes](../plan/01-spikes-et-benchmarks.md), [invariants](../architecture/PROJECT_INVARIANTS.md) et [état actuel](../architecture/STATUS.md).
