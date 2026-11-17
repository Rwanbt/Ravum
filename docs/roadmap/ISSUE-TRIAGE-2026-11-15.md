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

## 3 bis. Première vérification par issue : T0 (#88)

Vérification faite selon la règle de la §1. **Conclusion : ne pas fermer.**

- l'issue exige un rapport `docs/reports/T0-AUDIT.md` lié au SHA ;
- ce rapport existe, mais son en-tête dit lui-même **« Statut : PARTIAL en cours »** ;
- il ne peut donc pas servir de preuve de clôture « completed » au sens de P-14.

Correction faite au passage : le rapport citait **43** fichiers ADR alors que le
dépôt en compte **42** (vérifié par `validate_repo_docs.py`).

C'est l'argument contre le tri par lot : la proposition « fermable après
vérification » était une hypothèse, et la vérification l'a infirmée pour T0.
T3, T5 et T7 doivent subir la même lecture avant toute clôture.

## 3 ter. Vérification par issue : T3 (#91) — **fermée « completed »**

Les quatre critères de PASS de l'issue sont couverts, un par un :

| Critère | Preuve mesurée |
|---|---|
| corpus hostile sans panique ni boucle | corpus de conformance : **717 fichiers `.rv`** au total, dont **240** dans les familles de refus et de sécurité (`security`, `types`, `effects`, `ownership`), **503 cas / 502 PASS / 0 FAIL / 1 SKIP**, `build_verified: true` |
| aller-retour du formateur | `crates/ravum-lang-syntax/src/fmt.rs::assert_idempotent` (idempotence + préservation du sens parsé) |
| tests du parseur | **72** tests `ravum-lang-syntax` verts |
| fuzz smoke | job CI bloquant « Fuzz smoke », vert sur `dev` |

Le « Statut : PARTIAL » du corps de l'issue était **périmé** : il datait de la passe
où le parseur et le formateur étaient en cours. Le verdict du gate est `PASS` dans
la matrice §2 (T3), sur son périmètre mesuré — pas une preuve d'absence de bug sur
tout programme `.rv`.

Fermée avec un commentaire citant les commandes et les chiffres. **T5 (#93) et
T7 (#95) restent à vérifier** de la même façon, et T0 (#88) reste ouverte : son
rapport est lui-même marqué PARTIAL.

## 3 quater. Vérification par issue : T5 (#93) — **maintenue ouverte**

Vérification point par point des quatre exigences de l'issue :

| Exigence | Preuve | Verdict |
|---|---|---|
| backend B (Rust auditable) qualifié | `--emit-rust` (`args.rs`), oracle différentiel, S7 | couvert |
| binaires natifs Windows et Linux | smoke 43 PASS (Windows), CI Linux | couvert |
| parité debug/release | oracle `RAVUM_DIFF_RELEASE_EVERY`, **échantillonnée** | **partiel** |
| ABI et limites documentées | `SAFETY.md` C-FFI-2, `spec/language/abi.md` ABI-2.5/3.3 | **partiel** : contradictions ouvertes |

Conclusion : l'issue reste **ouverte**, son `Statut : PARTIAL` est juste. La matrice
donne `T5 = PASS`, ce qui paraît trop généreux au regard de ces deux points. Reclasser
T5 relève d'une décision de gate, donc humaine. Aucune clôture de T5 avant résolution
de ABI-2.5/3.3 et de la parité debug/release exhaustive.

## 3 quinquies. Vérification par issue : T7 (#95) — **maintenue ouverte**

Rejeu du 2026-11-17 sur `dev` = `bbb2ddf`, Linux x86_64, CLI construite par
`cargo build -p ravum-lang-cli --bin ravum-lang` (rustc 1.95.0).

| Exigence de l'issue | Preuve mesurée | Verdict |
|---|---|---|
| commandes `init/check/build/run/test/fmt/lint/doc/cache/doctor` | chacune exécutée sur un paquet créé par `ravum-lang init app` dans un répertoire vide, code de sortie 0 ; `run` imprime `hello, ravum` ; `doctor` conclut `verdict: ready` | couvert |
| manifeste, lockfile, paquets locaux | `check` crée `ravum.lock` puis `check --locked` répond `lockfile: current` ; sur une copie de `examples/package-demo` (dépendance `util = { path = "../util" }`), `check --locked`, `build`, `run` (sorties `42` et `40`), `test` et `fmt --check` sortent à 0 | couvert |
| LSP livré | `cargo test -p ravum-lang-cli` : `lsp_client` 12 réussites, 0 échec, **3 ignorés** (mesures de latence marquées `#[ignore]`, donc non exécutées) ; job CI `vscode-e2e` | couvert, hors mesures de latence |
| sorties stables | deux `build` du même paquet, cache `.ravum/` supprimé entre les deux, donnent la même ligne `identity: sha256:6e199fc3…` | couvert sur une machine |
| parcours `init → fmt` **sur machine propre** | le job `Installer contract` de `linux-qual.yml` installe dans un préfixe temporaire et n'exécute que `--version` sur le binaire installé ; le parcours ci-dessus part d'un binaire du workspace | **non couvert** |

Suite complète `cargo test -p ravum-lang-cli` sur ce même arbre : 133 réussites,
0 échec, 3 ignorés, en 8 binaires de tests.

Conclusion : l'issue reste **ouverte**. Le critère de PASS de l'issue demande le
parcours sur une machine propre, et aucune preuve ne fait tourner ce parcours depuis
un binaire installé. Proposition (non appliquée ici) : étendre
`scripts/install-contract.sh` pour dérouler `init`, `check`, `build`, `run`, `test`
et `fmt --check` avec le binaire du préfixe. Le `Statut : PARTIAL` du corps de
l'issue est juste sur ce point ; le reclassement du gate `T7 = PASS` de la matrice
relève du responsable.

## 3 sexies. Issues de gate `PARTIAL` : lecture point par point

Aucune de ces issues n'est fermée. La colonne « manque » dit ce qui empêche la
clôture au sens de P-14.

| Issue | Exigences lues dans le corps | Constat sur `bbb2ddf` | Manque |
|---|---|---|---|
| #89 (T1) | comparaison A/B/C, `docs/decision/M5-technical-report.md`, `scripts/m5-bench.ps1`, `evidence/m5-results.json`, décision provisoire | les trois fichiers existent ; le rapport se déclare `DRAFT / EXPERIMENTAL` et écrit que C est `NOT_IMPLEMENTED` | voie C sans mesure, donc pas de comparaison A/B/C ; choix de bootstrap non ratifié (D2) |
| #94 (T6) | stdlib Core, erreurs/panic/exit spécifiés, programmes fichier/texte/heap/collection | `bash scripts/lang-smoke.sh --skip-build` : 31 PASS, 0 FAIL, 0 NOT_RUN (dont `stats` et `ffi-abs`) ; `file-tool` et `file-store` ne sont pas dans ce smoke : `check` et `build` sortent à 0, leur exécution n'a pas été rejouée ici | le corps dit « FFI et unsafe UNSUPPORTED », la matrice dit FFI scalaire livrée (C-FFI-1 à 10) : corps périmé à corriger par le responsable ; threads, réseau, async `UNSUPPORTED` |
| #96 (T8) | hot reload `.rv` démontré **ou** repli honnête documenté, 0 perte en concurrence, API versionnée | `watch_continuity` : 3 réussites ; `watch` reconstruit et redémarre, sans préservation d'état | pas de preuve « 0 perte en concurrence » pour du code `.rv` ; pas d'ABI de patch ; bloc DSP reporté à la 1.1 (A2) |
| #97 (T9) | hello, calculator, file-tool, paquet multi-module, data, DSP offline ; HTTP/SQLite et WASM seulement si profils supportés | les six exemples nommés existent ; cinq passent le smoke Linux (`check`, `build`, `run`), `file-tool` n'y figure pas (`check` et `build` à 0, `run` non rejoué) ; `git ls-files examples` : 11 fichiers `.rv`, 575 lignes, 0 fichier `.rs` suivi | E2E « sur cibles annoncées » : le smoke WASI et Windows n'ont pas été rejoués dans cette vérification ; verdict matrice `PARTIAL` (HTTP + SQLite, DSP hôte, D6/D7) |
| #98 (T10) | conformance, fuzz, tests adversariaux, Miri/sanitizers, **revue indépendante** | conformance et mutation mesurées en CI (voir matrice T10) | revue indépendante absente (D4) ; sanitizers non exécutés ; couverture produit sous 100 % (E6 en cours, PR #148) |
| #99 (T11) | benchmarks gelés, budgets, SBOM/provenance, archives et installateurs Windows/Linux, checksums, politique de compatibilité | job CI `Release evidence (repro, SBOM, provenance, installer)` vert sur la PR #149 | pas d'artefact installable publié et lié à un SHA (aucune release, P-10) ; aucune clé de signature (D9) ; licence = #34 |

#90 (T2) n'est pas relue ici : les inverses S6 sont en cours dans la PR #149 et le
verdict reste `PARTIAL` (D10). #92 (T4) et #100 (T12) ne sont pas touchées.

## 4. Ce que ce tri ne fait pas

- Il ne ferme aucune issue : la clôture est un acte séparé, par issue, avec la preuve citée.
- Il ne transforme pas un `PASS` de gate en clôture automatique.
- Il ne touche pas aux issues `BLOCKED` par une décision humaine (T4, T12).
- Il ne remplace pas la lecture des issues : un titre n'est pas un périmètre.

## 5. Prochaine action

Les quatre issues de gate `PASS` ont été lues : #91 (T3) est fermée, #88 (T0),
#93 (T5) et #95 (T7) restent ouvertes pour les motifs des sections 3 bis à
3 quinquies. Restent à faire : relire #90 (T2) une fois les inverses S6 consignés,
et soumettre au responsable les trois écarts entre la matrice et les issues
(T0, T5, T7 notés `PASS` alors qu'un critère de l'issue n'est pas couvert).
