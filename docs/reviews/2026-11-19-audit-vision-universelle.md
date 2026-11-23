# Audit stratégique et proposition — « Ravum, langage universel » (2026-11-19)

> **Statut : RFC-VISION-1 — PROPOSITION, non ratifiée.** Aucune décision ADR
> n'est acceptée par ce document. Sources lues : `AGENTS.md`, `STATUS.md`,
> `PROJECT_INVARIANTS.md`, `LANGUAGE-CORE-STATUS.md`, `spec/language/unsupported.md`,
> exemples `examples/*`. ADR individuels : `NOT_REVIEWED` en texte intégral pour
> cet audit (seuls les titres ont été vus). La veille langages est issue de
> connaissances générales (pas de mesure nouvelle).

## 1. Verdict court

**La direction est bonne sur la méthode, à revoir sur la stratégie produit.**

- ✅ **Très bon** : l'honnêteté des preuves (INV-01), le fail-closed (INV-03),
  le refus des résultats périmés, les effets/profils (`pure`/`rt`), la boucle
  incrémentale mesurée, le hot swap DSP sans allocation, la conformance
  (503 cas), le fuzz/Miri/mutation. Peu de projets de langage ont cette rigueur
  aussi tôt. C'est un **vrai différenciateur** à garder.
- ⚠️ **À revoir** :
  1. **Deux lignes qui ne se rejoignent pas.** Le hot swap, le daemon et
     l'incrémentalité vivent dans la plateforme Rust ; le langage Core 1.0 n'en
     profite pas encore (il émet du Rust compilé par `rustc`). Le cœur de la
     promesse (« édition → exécution à chaud en temps réel » *dans Ravum*) n'est
     donc démontré **sur aucun programme Ravum**.
  2. **Core 1.0 est un sous-ensemble de Rust, en plus restreint** : pas de
     traits, closures, `async`, threads, `Box`/`Rc`, macros, patterns riches.
     Aujourd'hui un développeur n'a **aucune raison** de le préférer à Rust.
     Cibler D2 « production ready » sur cette surface risque de produire un
     langage solide mais sans usage.
  3. **« Tous les avantages sans les inconvénients » n'est pas atteignable** :
     plusieurs avantages sont mutuellement exclusifs (voir §3). Il faut des
     **choix assumés + des profils**, pas une addition de fonctionnalités.
  4. **Énorme surface documentaire** (42 ADR, 811 MUST) par rapport au code
     langage (~17 k lignes). Risque de bureaucratie qui ralentit P0-I.

## 2. Veille : ce qu'il faut prendre à chaque langage

| Langage | À prendre | À éviter |
|---|---|---|
| **Rust** | ownership/borrow, pas de GC, `Result`, enums algébriques, Cargo, zéro-coût | temps de compilation, complexité des lifetimes/traits, async « coloré », macros opaques |
| **Zig** | `comptime` à la place des macros, allocateurs explicites, cross-compilation triviale, interop C sans glue | pas de sûreté mémoire |
| **Go** | compilation ultra-rapide, outillage unique (`fmt`, `test`), simplicité, goroutines | GC, `nil`, erreurs verbeuses |
| **Swift / Hylo (Val)** | **sémantique de valeur mutable** + ARC/CoW : sûreté sans lifetimes visibles | ARC coûteux non contrôlé |
| **Mojo** | syntaxe Python + performance native, SIMD/GPU de première classe | écosystème fermé |
| **Koka / Effekt / OCaml 5** | **effets algébriques** typés (async, erreurs, générateurs, DI = un seul mécanisme) | adoption |
| **Erlang/Elixir** | supervision, « let it crash » isolé, **hot code reload** en prod, acteurs | perf brute, typage faible |
| **Unison** | code adressé par contenu → hot swap et cache incrémental naturels, pas de conflits de dépendances | modèle de stockage déroutant |
| **TypeScript** | typage graduel, inférence, DX éditeur | runtime JS, unsoundness |
| **Kotlin / Dart (Flutter)** | multiplateforme, null-safety, **hot reload UI** sous la seconde | VM/GC |
| **Lean / F\* / Dafny / Ada SPARK** | contrats vérifiés, raffinements, SPARK pour le temps réel certifié | coût d'écriture des preuves |
| **Roc / Elm** | zéro exception runtime, messages d'erreur exemplaires, plateformes séparées du langage | domaine restreint |
| **Gleam / Pony** | capacités de référence (Pony) : concurrence sans data race | niche |
| **Faust / Csound** | DSP déclaratif, compilation de graphes audio | mono-domaine |
| **Python** | lisibilité, REPL, notebooks — **la langue que les IA écrivent le mieux** | lenteur, typage dynamique |
| **WebAssembly (cible)** | sandbox, portabilité, Component Model | — |

**Synthèse** : la combinaison gagnante et réaliste est
*Rust (sûreté) + Swift/Hylo (simplicité de la mémoire) + Zig (comptime) +
Koka (effets) + Erlang/Unison (hot reload/adressage contenu) + Go (vitesse
d'outillage) + SPARK (contrats RT)*. Ravum a déjà les briques effets, contrats
et hot swap ; il manque la **simplicité** et la **jonction**.

## 3. Les compromis inévitables (et comment les traiter)

| Tension | Proposition Ravum |
|---|---|
| Pas de GC **vs** simplicité | Valeurs mutables + CoW par défaut (style Hylo) ; références empruntées seulement en paramètre, sans lifetimes écrites ; `Rc`/arènes opt-in. |
| Compilation rapide **vs** code optimal | **Deux backends** : interpréteur/JIT (Cranelift, ADR-018) en dev pour le hot swap < 100 ms, LLVM/`rustc` en release. Même sémantique, oracle différentiel entre les deux (déjà en place). |
| Hot swap **vs** déterminisme/sûreté | Swap uniquement aux **points de quiescence** typés (frontière de bloc DSP, fin de requête, frame UI) + migration d'état déclarée ; sinon refus (fail-closed déjà conforme INV-03). |
| Universalité **vs** runtime minimal | **Profils** (`rt`, `pure`, `core`, + à créer `web`, `server`, `embedded`, `gpu`) : le profil choisit runtime et effets autorisés ; le langage reste unique. |
| Zéro crash **vs** réalité | « Pas de crash mémoire » = garanti par types. « Pas de crash runtime » = panics confinés (supervision par tâche/acteur, `rt` interdit les traps — déjà RFC-RT-TRAPS-1). On ne promet pas l'absence d'erreurs logiques. |
| Expressivité **vs** facilité IA | Syntaxe régulière, une seule façon de faire, pas de macros textuelles (comptime typé), diagnostics JSON (ADR-040 déjà prévu). |

## 4. Ce qu'il faut ajouter (par priorité)

### P1 — Fermer la boucle « Ravum + hot swap » (la promesse centrale)
1. Brancher `ravum-lang-*` sur le daemon/coordinator : `ravum watch` sur un
   projet `.rv` → analyse incrémentale par requêtes → **swap de fonction** dans
   un processus vivant.
2. Backend de développement rapide (interpréteur sur IR typée ou Cranelift),
   pour ne plus payer `rustc` à chaque frappe. Mesurer P0-I sur des `.rv`.
3. Une démo de bout en bout : le DSP `examples/dsp-gain-iir` modifié pendant la
   lecture audio, sans clic, sans allocation callback.

### P2 — Rendre le langage *utilisable* (sinon personne ne l'adopte)
1. **Traits/interfaces** (ou « protocoles » plus simples, sans HRTB/GAT au début).
2. **Closures** et itérateurs.
3. **Gestion d'erreurs** ergonomique (déjà `?`) + effets algébriques comme
   mécanisme unique pour async/générateurs/erreurs.
4. **Concurrence structurée** sans data race (acteurs ou tâches à portée,
   capacités style Pony), annulation intégrée.
5. **Comptime** typé à la place des macros.
6. **Interop** : importer des crates Rust et des en-têtes C automatiquement
   (l'écosystème Rust devient l'écosystème Ravum dès le jour 1).

### P3 — Universalité par profils et cibles
- `wasm32` + Component Model (web/front et plugins sandboxés) — job CI wasm32 déjà présent, à étendre.
- Profil `server` (async, HTTP), `ui` (hot reload d'UI déclarative, état préservé),
  `embedded` (`no_std`, allocateur explicite), `gpu` (noyaux SIMD/compute, ADR-036).
- Bibliothèque standard « batteries incluses mais modulaire » : fs complet
  (`create_dir`, `remove_file` manquent), réseau, JSON, temps, tests, bench.

### P4 — Différenciateurs IA (unique sur le marché)
- **API machine** stable (ADR-027) : l'agent interroge types, effets, contrats,
  impacts d'une modification, en JSON.
- **Code adressé par contenu** (style Unison) au niveau fonction : cache
  incrémental exact, hot swap granulaire, et fusion multi-agents sans conflit.
- **Contrats = spécification exécutable** : l'IA génère, Ravum vérifie (tests +
  preuve bornée + fuzz), statut honnête. C'est la boucle P0-E déjà amorcée.
- **Déterminisme** : mode simulation déterministe (seed, horloge virtuelle,
  ordonnanceur rejouable — ADR-016) pour rejouer exactement un bug trouvé par
  un agent.

## 5. Ce qu'il faut revoir / alléger

1. **Recentrer D2** : « production ready » d'un sous-ensemble sans traits ni
   closures n'a pas de marché. Proposer : D2 = *production ready pour un
   domaine pilote* (DSP temps réel + outils CLI), avec hot swap Ravum natif.
2. **Choisir un domaine phare** au lieu de P0-G tout de suite : le **temps réel
   audio/DSP avec hot swap** est l'endroit où Ravum est déjà le meilleur et où
   aucun concurrent n'offre sûreté + RT + reload. L'universalité viendra par
   profils ensuite.
3. **Décider le modèle mémoire utilisateur** maintenant (ownership Rust visible
   vs valeurs mutables à la Hylo). C'est la décision qui conditionne la
   simplicité et l'adoption ; ADR-002 à re-examiner (`NOT_REVIEWED` ici).
4. **Réduire le coût de gouvernance** : regrouper les 42 ADR en ~10 décisions
   structurantes à ratifier ; les autres restent des notes. Les 344 MUST non
   mappés : soit testés, soit retirés.
5. **Syntaxe** : rester proche de Rust aide l'IA (corpus énorme) mais garder
   l'option de simplifications (pas de `::` vs `.` en double, pas de lifetimes
   écrites).

## 6. Feuille de route proposée (indicative)

| Étape | Livrable vérifiable | Critère de sortie |
|---|---|---|
| R1 (≈1–2 mois) | `ravum watch` sur `.rv` + swap de fonction à chaud | p95 édition→activation < 200 ms sur 5 k lignes, 0 alloc callback |
| R2 | traits simples + closures + itérateurs | 3 exemples réels réécrits plus courts qu'en Rust |
| R3 | interop crates Rust / C | appeler une crate réelle sans glue manuelle |
| R4 | concurrence structurée + effets algébriques | 0 data race par construction, tests négatifs |
| R5 | profils `wasm`, `server`, `ui` | une app web + une app native du même code métier |
| R6 | API agent + simulation déterministe | campagne S8 avec LLM réels : temps jusqu'au correctif vérifié mesuré |

## 7. Décisions à prendre par le responsable

- **Q1** Domaine pilote unique pour D2 (recommandé : DSP/RT + CLI) ?
- **Q2** Modèle mémoire visible : ownership Rust ou valeurs mutables + CoW ?
- **Q3** Backend dev rapide : interpréteur IR ou Cranelift ?
- **Q4** Priorité P1 (jonction hot swap/langage) avant toute extension de surface ?
- **Q5** Consolidation des ADR en un noyau ratifiable ?

Aucune de ces décisions n'est prise ici (INV-02).
