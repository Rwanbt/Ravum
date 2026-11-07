# Plan directeur — Universal Language

Version 0.1 · 2026-10-23 · statut **DRAFT / architecture proposée**

## 1. Mission, succès, non-objectifs

**Mission** : créer un langage unique, léger et portable, qui rend vérifiables le comportement du programme, les capacités, les ressources et les conditions d'exécution, et qui compile en continu pendant l'édition humaine ou la génération IA. Les développements doivent rester fluides et les programmes performants, y compris en audio temps réel sous contraintes explicites.

**Succès observable** : (a) code sûr avec modèle mémoire précis ; (b) preuves/traces d'échec diagnostiquées honnêtement ; (c) latence éditer→prévisualiser mesurée ; (d) thread audio sans allocation/blocage via contrôles statiques + instrumentés ; (e) scénario full-stack de bout en bout ; (f) code IA corrigé sans affaiblissement de contrat ; (g) coût CPU/RAM reproductible ; (h) binaire de production qualifié, pas seulement une preview.

**Non-objectifs initiaux** : remplacer LLVM, Z3, OS et moteurs SQL ; inventer un compilateur certifié de bout en bout dès la première itération ; garantir zéro bug/crash ou WCET universel ; rendre tous les changements hot-swappables ; recréer immédiatement frameworks UI, pilotes GPU et mobile natif complets ; déployer des systèmes tiers avant autorisation.

**Lignes rouges** : pas de langage présenté comme « production-ready » sans suite de preuves exécutées ; pas de résultats de tests simulés ; jamais requalifier `UNKNOWN`, `TIMEOUT`, `UNSUPPORTED` ou `ASSUMED` en `PROVED` ; pas de baisse de contrat par un agent IA ; pas de patch actif sur un programme temps réel sans frontière sûre et rollback défini.

## 2. Séparer garanties, mécanismes et hypothèses

- **Garantie mémoire** : valable pour code dans le sous-ensemble sûr, selon le modèle de mémoire et les hypothèses de la chaîne de compilation ; les ponts `unsafe` / FFI portent des obligations supplémentaires.
- **Correction fonctionnelle** : exige un contrat suffisamment fort, un domaine défini et une preuve valide. Une fonction peut être « prouvée » relativement à une spécification erronée ; revue métier indispensable.
- **Tests déterministes** : rejouabilité des cas explorés, pas preuve universelle. Les limites de boucles, scheduling et modèle d'environnement doivent figurer dans le rapport.
- **Temps réel** : séparer *RT-safe* (effets interdits), *RT-bounded* (bornes statiques de ressources/itérations), *hard-RT certified* (modèle plateforme + WCET/ordonnancement démontrés).
- **Zéro crash** : non garanti face aux fautes matérielles, OS, adversaires, périphériques, FFI incorrectes, interruption électrique ou épuisement de ressources non couvert.
- **Rapidité** : la compilation interactive ne fait que le travail indispensable ; la preuve approfondie est planifiable et concurrente, mais le mode strict n'active pas le code sans obligations requises.

## 3. Architecture cible et frontière des composants

```text
 Éditeur | Agent IA | CLI | LSP
             │  edits / overlays / intents / diagnostics / proof-status
             ▼
  Service de compilation permanent (quotas, isolation, annulation)
             │
     CST incrémental et résilient ──> HIR typée
             │                         │
             └────────────────────> MIR ownership + effets + régions
                                       │
                           ┌───────────┴──────────┐
                           ▼                      ▼
                   IR de vérification       IR de codegen
                           │                      │
             SMT/contrats/model-check       Rust sûr (pont initial)
                 simulateur de fautes        Cranelift (spike)
                tests déterministes         LLVM (évaluation)
                           │                      │
                    preuve + statut       natif / WASM / modules
                           └──────────┬───────────┘
                                      ▼
                       Validateur de version / contrat
                         ┌────────────┴────────────┐
                         ▼                         ▼
                Dev hot-swap/preview         Build release
              état compatible/rollback    tests+preuves+attestation
```

**Point d'architecture critique** : ne pas confondre l'IR de vérification (sémantique contractuelle) avec l'IR bas niveau optimisée. Il faut une relation de raffinement ou une validation des transformations ; le premier backend Rust fournit un moyen pragmatique de limiter le risque initial, pas une preuve automatique de bonne traduction.

**Interop** : les ressources et imports/exports sont déclarés ; WASI/WIT lorsque disponible, FFI C de confiance restreinte, wrappers sûrs ; `unsafe` concentré et tracé. Le runtime n'est pas obligatoire sur les cibles `no_std`.

## 4. Workstreams (programmes cohérents)

| Code | Programme | Production livrable | Dépendances |
|---|---|---|---|
| WS-00 | Gouvernance, spécification, benchmark | ADR, modèle de menace, fixtures, preuve de reproductibilité | aucune |
| WS-01 | Parser, types, ownership | CST, HIR, diagnostics, modèles mémoire | WS-00 |
| WS-02 | Incrémentalité et compilation | daemon, invalidation fine, pipeline Rust sûr, cache | WS-01 |
| WS-03 | Vérification et contrats | obligations, solveurs, statuts et contre-exemples | WS-01/02 |
| WS-04 | Effets, capacités, realtime | effet/interdiction, budgets et DSP | WS-01/03 |
| WS-05 | Tests et simulation | scheduling contrôlé, fault injection et replay | WS-02/03 |
| WS-06 | Runtime patchable | modules versionnés, hot swap et rollback | WS-02/04 |
| WS-07 | Agent IA / DX | LSP, JSON/SARIF, API, permissions d'agent | WS-01/02/03 |
| WS-08 | Full-stack et données | WASM, client+API, SQL typé, migrations | WS-02/04/07 |
| WS-09 | Portabilité/prod/perf | Linux/Windows/macOS, ARM, no_std, audit, build release | Tous pertinents |
| WS-10 | Calcul spécialisé avancé | GPU, preuves numériques, WCET matériel | WS-03/04/09, recherche |

## 5. Phases ordonnées par preuves de faisabilité, pas par calendrier fictif

### F0 — Cadrage et décisions conditionnelles (T0)

Livrer et revoir la matrice d'exigences, la sémantique minimale, les domaines de confiance, les alternatives, les jeux d'essai et un environnement de mesure contrôlé. **Go** : chaque garantie est définie, des critères de falsification sont écrits, critères de confiance des démonstrateurs signés. **No-Go** : mots vagues (« zéro bug », « sûr partout ») utilisés comme critères.

### F1 — Expériences de risque majeur (T1, aucun produit promis)

Conduire en parallèle cinq spikes : (S1) analyse+cache fonctionnel ; (S2) preuves+invalidation ; (S3) codegen Rust vs Cranelift ; (S4) hot-swap/état ; (S5) DSP realtime. Capturer SHA, commandes, mesures, échecs, périmètres prouvés. **Go** : au moins une architecture candidate satisfait les contrats de sécurité et le budget de coût défini ; sinon réviser ADR et ne pas « construire quand même ».

### F2 — Langage autonome minimalement cohérent (T2)

Parser tolérant, types, valeurs, fonctions, tuples/structures/enums, ownership, emprunts, `Result`, contrats de base, effects ; pipeline Rust sûr ; CLI `check/build/verify` ; diagnostics identifiables ; tests positifs/négatifs et delta-debugging. **Go** : les programmes exécutés sont équivalents aux sémantiques de référence sur suites différentielles, et violations de propriété ne deviennent pas succès.

### F3 — Édition et vérification continue (T2)

Daemon multi-sessions avec isolation, quotas RAM/CPU et annulation, overlays non sauvegardés, AST invalide géré ; cache persisté par empreinte et dépendance de preuve ; preview gardant le dernier bon état ; protocole d'erreur IA ; benchmarks froid/chaud. **Go** : tests d'invalidation adversariaux verts, crash-recovery démontré et performance comparée à la baseline.

### F4 — Sécurité temps réel et hot swap (T3)

Contrats d'effets transitifs, allocations/locks/I-O interdits dans callbacks, budgets, migration d'état, code versionné, patch atomique, références anciennes conservées, test DSP réel avec instrumentation. **Go** : tests de stress, concurrence et E2E sur plateforme audio réelle, sans erreur détectée dans le domaine testé ; WCET non revendiqué sans analyse spécifique.

### F5 — Chaîne full-stack cohérente (T3)

Application référence avec frontend WASM, API serveur et PostgreSQL/SQLite : contrat d'API partagé, requêtes typées, migration versionnée, preview UI, tests transactionnels, authz et secrets. **Go** : E2E réel, changements de schéma incompatibles bloqués, perfs mesurées.

### F6 — Maturité système et qualification produit (T4)

Portages Windows/Linux/macOS, cross-arch, vulnérabilités, fuzzing continu, suites de conformité, release reproducible si possible, SBOM, provenance, test external/ABI, tests de comportement anti-régression, mesure qualité DX IA. **Go** : aucun défaut P0/P1 ouvert, CI requise au SHA exact, preuves auditables, périmètre produit publié précisément (pas « universel complet » sans couverture).

### F7 — Extensions avancées (programmes séparés)

Proof-carrying artifacts, validateurs de transformations, GPU, calcul numérique borné, mobile intégral, MCU no_std, hard-RT/WCET pour cibles identifiées et éventuel backend machine autonome. Chaque extension demande ADR, budgets et qualification cible propres.

## 6. Modèle de branchements et autorité

`main` protégée, branches par épic et PR cohérentes (recommandation initiale ≤ 400 LOC **de code** hors tests/docs pour faciliter la revue, exceptions documentées). Chaque PR inclut tests RED→GREEN et preuves exact SHA ; aucune fusion lorsque des checks requis manquent/échouent. Les interfaces de la spécification restent détenues par responsables humains : un agent ne change pas silencieusement invariants, hypothèses, critères CI ou statuts d'approbation.

Les prototypes ne sont pas « production ready ». Les décisions techniques restent `PROPOSED` tant que les expériences n'ont pas démontré les propriétés nécessaires et que les responsables n'ont pas accepté les ADR.

## 7. Programme prioritaire de 90 premiers jours **sans prétention de durée ferme**

L'ordre « jours 1 à 90 » est une trame de planification conditionnelle, pas une estimation que l'équipe doit tenir : **première séquence** : F0 + fixtures/bench ; **deuxième** : S1/S2/S3 ; **troisième** : S4/S5 ; **quatrième** : arbitrage et slice intégrée, puis CI. Une équipe d'une personne peut prendre beaucoup plus de temps : on suit les critères de sortie, pas les dates.

## 8. Conditions d'arrêt ou de pivot

- Cache de preuves non fiable → ne pas automatiser son réemploi avant correction.
- Compilateur vivant trop consommateur → mode léger sans daemon global, suppression sélective des caches.
- Rust bridge trop lent → Cranelift évalué avec oracle de sémantique, sans supprimer la chaîne fiable avant résultats.
- Hot patch trop risqué → recharge de module isolé ou redémarrage contrôlé ; ne pas forcer la migration.
- Preuves trop lourdes → garanties de base obligatoires, obligations approfondies planifiables, statuts explicites et release stricte par domaine.
- Preuves WCET non soutenues par la cible → catégorie RT-safe seulement ; ne jamais déclarer hard-RT.

## 9. Livrables de décision attendus

Après T1, une revue tranche : backend initial, représentation intermédiaire, solveurs, daemon, hot-swap, profil temps réel et critères de confiance. Les ADR doivent alors passer de PROPOSED à ACCEPTED/REJECTED avec preuves annexées. Sans cette revue, le dossier est une proposition et non une décision de réalisation.
