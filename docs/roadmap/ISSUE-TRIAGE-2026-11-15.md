# Tri des issues ouvertes — inventaire du 2026-11-15 (E9, P-14)

> **Document de tri, pas une clôture.** P-14 impose un tri **par preuve** : une issue
> n'est fermée « completed » que si un rapport **mesuré** la couvre, « not planned » si le
> périmètre gelé l'exclut, sinon elle reste ouverte avec un jalon. Ce fichier inventorie et
> **propose** ; il ne ferme rien. Chaque clôture exige la lecture de l'issue et le renvoi à
> la preuve correspondante.

Total : **90 issues ouvertes** — 13 issues de gate,
9 épopées de fondation, 68 autres.

## 1. Issues de gate `RAV-LANG-T1` à `T12`

Le verdict est celui de [`CORE-1.0-GATE-MATRIX.md`](../roadmap/CORE-1.0-GATE-MATRIX.md) §2,
seule source de vérité. **Un verdict `PASS` ne suffit pas à fermer l'issue** : elle couvre
tout le périmètre du gate, pas seulement ce qui est mesuré.

| Issue | Gate | Verdict | Disposition proposée | Motif |
|---|---|---|---|---|
| #88 | T0 | `PASS` | fermable après vérification point par point | preuves du socle dans la matrice §2/§3 |
| #89 | T1 | `PARTIAL` | reste ouverte (périmètre non atteint) | voie C hors périmètre 1.0 (A3) ; décision humaine D2 |
| #98 | T10 | `PARTIAL` | reste ouverte (périmètre non atteint) | audit externe absent |
| #99 | T11 | `PARTIAL` | reste ouverte (périmètre non atteint) | signature : aucune clé (D9) |
| #100 | T12 | `BLOCKED` | **ne pas fermer** (humain) | Go/No-Go humain |
| #90 | T2 | `PARTIAL` | reste ouverte (périmètre non atteint) | dénominateur MUST : 344 non mappés (D10) |
| #91 | T3 | `PASS` | fermable après vérification point par point | frontend prouvé sur le périmètre mesuré |
| #92 | T4 | `BLOCKED` | **ne pas fermer** (humain) | audit externe humain (D4) |
| #93 | T5 | `PASS` | fermable après vérification point par point | binaires natifs Windows + Linux, WASI |
| #94 | T6 | `PARTIAL` | reste ouverte (périmètre non atteint) | threads/réseau/async explicitement UNSUPPORTED |
| #95 | T7 | `PASS` | fermable après vérification point par point | LSP réel + job CI vscode-e2e |
| #96 | T8 | `PARTIAL` | reste ouverte (périmètre non atteint) | pas de préservation d'état ; DSP reporté 1.1 (A2) |
| #97 | T9 | `PARTIAL` | reste ouverte (périmètre non atteint) | DSP hôte absent par périmètre (A2) |

## 2. Épopées de fondation `[EPIC][E0x][M*]`

Ces issues décrivent la vague MVP-0a/0b/0c de la **plateforme Rust**, hors promesse 1.0
(P-13). Elles ne se ferment pas au motif que le langage progresse : leurs livrables sont
distincts. Disposition proposée : **reste ouvertes**, avec un jalon « plateforme
expérimentale » pour les distinguer du chantier Core 1.0.

| Issue | Épopée |
|---|---|
| #1 | E01 — [EPIC][E01][M0] Charte, gouvernance, ADR et exigences |
| #2 | E02 — [EPIC][E02][M0] Corpus, baselines et expérimentation IA |
| #3 | E03 — [EPIC][E03][M1] Plateforme de développement continu et diagnostics |
| #4 | E04 — [EPIC][E04][M1] Hot swap natif responsable et preview |
| #5 | E05 — [EPIC][E05][M2] Vérification, preuves et invalidation |
| #6 | E06 — [EPIC][E06][M3] DSP audio et contraintes temps réel |
| #7 | E07 — [EPIC][E07][M4] Verticale full-stack et cohérence inter-cibles |
| #8 | E08 — [EPIC][E08][M5] Gate d'autonomie du langage Ravum |
| #9 | E09 — [EPIC][E09][M6+] Écosystème, portabilité et recherche avancée |

## 3. Autres issues

| Issue | Titre |
|---|---|
| #10 | [US-001][M1] Développeur : voir les erreurs pendant l'édition |
| #11 | [US-002][M1] Développeur : preview sans redémarrage systématique |
| #12 | [US-003][M1] Agent IA : recevoir des diagnostics structurés et stables |
| #13 | [US-004][M0] Responsable : protéger les spécifications contre les agents |
| #14 | [US-005][M1] Développeur sur 16 Go : garder la machine réactive |
| #15 | [US-006][M2] Développeur : savoir exactement ce qui est prouvé |
| #16 | [US-007][M3] Ingénieur audio : modifier le DSP sans compromettre le callback |
| #17 | [US-008][M4] Full-stack : partager les contrats frontend/API/SQL |
| #18 | [US-009][M2] Agent IA : reproduire un défaut déterministement |
| #19 | [US-010][M0] Mainteneur : mesurer le temps jusqu'à un livrable vérifié |
| #20 | [US-011][M5] Créateur de langage : vérifier l'intérêt d'une nouvelle syntaxe |
| #21 | [US-012][M0] Contributeur : comprendre l'état réel et les ADR |
| #22 | [US-013][M1] Développeur : rollback sûr après patch échoué |
| #23 | [US-014][M6+] Développeur systèmes : réutiliser un cœur portable et minimal |
| #24 | [RAV-001][M0] Valider la charte P0 et la matrice de traçabilité |
| #25 | [RAV-002][M0] Auditer les 42 ADR et dédupliquer les RFC multi-IA |
| #26 | [RAV-003][M0] Définir la machine de référence, pins et protocole de benchmark |
| #27 | [RAV-004][M0] Créer fixtures de référence et corpus négatif |
| #28 | [RAV-005][M0] Définir les cinq identités de snapshot et de preuve |
| #29 | [RAV-006][M0] Versionner les schémas diagnostics/preuves/jobs et tester les stat |
| #30 | [RAV-007][M0] Créer un registre d'autorité pour les spécifications normatives |
| #31 | [RAV-008][M0] Qualifier la CI documentaire et préparer gates de tests par impact |
| #32 | [RAV-009][M0] Établir baseline Rust S1-R jusqu'au linking et à l'activation |
| #33 | [RAV-010][M0] Pré-enregistrer S8-A/S8-B et exécuter pilote agent IA |
| #34 | [RAV-011][M0] Décider licence du code, gouvernance et inventaire tiers |
| #35 | [RAV-012][M0] Mettre au point la sélection des tests par impact sans masquer E2E |
| #36 | [RAV-013][M0] Formaliser le gate M5 et les critères de pivot avant les résultats |
| #37 | [RAV-014][M1] Implémenter SourceSnapshot et overlays immuables |
| #38 | [RAV-015][M1] Adapter rust-analyzer via LSP et cargo via CLI |
| #39 | [RAV-016][M1] Implémenter coordinator et scheduling prioritaire |
| #40 | [RAV-017][M1] Rendre l'annulation et STALE observables et résistants aux crashs |
| #41 | [RAV-018][M1] Exposer CLI check/watch/status et JSON versionné |
| #42 | [RAV-019][M1] Prototyper PatchHost par modules dynamiques en C ABI |
| #43 | [RAV-020][M1] Définir et vérifier manifeste ABI/layout de patch |
| #44 | [RAV-021][M1] Gérer appels en vol, quiescence et reclamation différée |
| #45 | [RAV-022][M1] Tester refus, rollback et compatibilité des patches |
| #46 | [RAV-023][M1] Mesurer pipeline complet frappe→preview |
| #47 | [RAV-024][M1] Qualifier la coexistence de DLL versionnées sous Windows et dylib  |
| #48 | [RAV-025][M2] Créer un ProofBroker avec adaptateur externe Kani ou équivalent |
| #49 | [RAV-026][M2] Implémenter statuts de preuve et politiques d'activation par profi |
| #50 | [RAV-027][M2] Construire EvidenceLedger append-only et clés de contexte de vérif |
| #51 | [RAV-028][M2] Exécuter S2 : mutation adversariale du cache de preuves |
| #52 | [RAV-029][M2] Exécuter S6 : comparer solveurs et qualité des contre-exemples |
| #53 | [RAV-030][M2] Créer S9 : simulateur déterministe de concurrence et pannes |
| #54 | [RAV-031][M3] Implémenter fixture audio gain puis filtre IIR avec état |
| #55 | [RAV-032][M3] Étudier analyse d'effets RT transitifs et disponibilité par platef |
| #56 | [RAV-033][M3] Préparer le patch DSP hors thread et libérer l'ancien état hors RT |
| #57 | [RAV-034][M3] Exécuter S4 : stress hot-swap multi-thread, état et ABI |
| #58 | [RAV-035][M3] Exécuter S5 : mesures callbacks, XRUN et discontinuités DSP |
| #59 | [RAV-036][M3] Qualifier un hôte audio CLAP/VST3 réel après harness |
| #60 | [RAV-037][M4] Créer une application témoin UI + API Rust + SQLite |
| #61 | [RAV-038][M4] Qualifier changements de schéma et migrations n/n-1 |
| #62 | [RAV-039][M4] Implémenter matrice E2E par impact + suite intégrale |
| #63 | [RAV-040][M4] Comparer WIT/WASI et une frontière native versionnée |
| #64 | [RAV-041][M4] Tester authentification, autorisation et exposition des secrets |
| #65 | [RAV-042][M5] Implémenter S1-T : mini frontend à requêtes par définition |
| #66 | [RAV-043][M5] Définir un oracle sémantique indépendant du comportement rustc |
| #67 | [RAV-044][M5] Exécuter S3 : backend Rust bridge vs Cranelift vs LLVM candidat |
| #68 | [RAV-045][M5] Exécuter S8-B : ergonomie et agent IA hors corpus du langage |
| #69 | [RAV-046][M5] Rédiger la décision Go/No-Go du langage autonome |
| #70 | [RAV-047][M5] Mettre en place parité comportementale dev/release |
| #71 | [RAV-048][M6+] Étudier portabilité mobile et intégration native Android/iOS |
| #72 | [RAV-049][M6+] Explorer GPU/WebGPU et accélérateurs multi-cibles |
| #73 | [RAV-050][M6+] Étudier hard-RT, WCET et exigences par plateforme |
| #74 | [RAV-051][M6+] Définir stratégie d'écosystème paquets, macros et plugins |
| #75 | [RAV-052][M6+] Explorer certificats de preuve indépendants et TCB minimal |
| #76 | [RAV-053][M6+] Mesurer empreinte énergétique et budgets par profil de cible |
| #79 | [RFC][M0/M1] Shared workspace multi-agents sans worktrees : propositions, confli |

## 4. Ce que ce tri ne fait pas

- Il ne ferme aucune issue : la clôture est un acte séparé, par issue, avec la preuve citée.
- Il ne transforme pas un `PASS` de gate en clôture automatique.
- Il ne touche pas aux issues `BLOCKED` par une décision humaine (T4, T12).
- Il ne remplace pas la lecture des issues : un titre n'est pas un périmètre.

## 5. Prochaine action

Pour chaque issue de gate `PASS` (T0, T3, T5, T7), lire l'issue, vérifier que chaque
livrable listé est couvert par une preuve mesurée de la matrice, puis fermer avec un
commentaire citant la commande, le SHA et le run. Les autres restent ouvertes.
