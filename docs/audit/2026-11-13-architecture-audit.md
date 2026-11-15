# RAVUM — Audit architectural ciblé (2026-11-13)

> **Statut : rapport d'audit, non normatif.** Il ne ratifie aucun ADR et ne modifie aucune règle. Les décisions D1…D7 ci-dessous sont des recommandations `PROPOSED`, à soumettre au responsable.

Périmètre : 13 fichiers consultés (6 documents, 7 sources par extraits ou grep), branche `test/coverage-batch-1`. Aucun build, test ou benchmark lancé : tous les chiffres cités viennent des rapports du dépôt et ne sont pas re-mesurés.

## 1. Synthèse exécutive

Ravum contient deux produits réels qui ne se touchent pas :

- **La plateforme Rust** (`ravum`, environ 70 k lignes) : daemon d'analyse incrémentale, graphe d'unités, hot swap natif d'une fonction `f64 -> f64`, hôte DSP sans allocation au callback. Elle opère sur du code Rust.
- **Le langage** (`ravum-lang`, environ 67 k lignes) : lexer, parser, typechecker, borrow checker conservateur, effets, génération d'un fichier Rust unique compilé par `rustc`, LSP, 471 cas de conformance.

Le langage n'utilise aucun service de la plateforme : `ravum-lang-cli` ne dépend que des crates `ravum-lang-*`. Un programme `.rv` est donc recompilé en entier à chaque édition, sans `-C incremental`, et son « hot reload » est un redémarrage de processus. Les trois promesses centrales de la vision (incrémental rapide, mise à jour à chaud, temps réel) ne sont démontrées que pour Rust.

Le langage est sain sur son périmètre : sortie déterministe (aucun `HashMap` dans les crates du langage), quasi aucun `unwrap`/`panic` en code produit, Rust émis sous `#![forbid(unsafe_code)]`. La sûreté mémoire repose donc en dernier ressort sur `rustc`, pas sur le borrow checker Ravum. C'est une force, mais le design affirme l'inverse.

Trois risques dominent :

1. Les divergences `check` accepté / `build` refusé forment une classe structurelle, fermée par sondage et non par construction.
2. Les `NodeId` globaux attribués dans l'ordre de lecture sont hostiles à toute incrémentalité du frontend.
3. Le mandat des agents (`AGENTS.md`, `STATUS.md`) contredit l'état réel et la décision D2 (« production ready »).

Recommandation : ne lancer ni la voie C (Cranelift) ni d'extension de surface. Corriger d'abord la gouvernance, installer un oracle différentiel, mesurer la boucle `.rv`, puis relier les deux lignes par une verticale DSP.

## 2. État réel et niveau de confiance

| Domaine | État | Preuve | Confiance |
|---|---|---|---|
| Lexer, parser, AST, formateur | Implémenté, testé, fuzzé | `ravum-lang-syntax` (9,1 k lignes), gate T3 `PASS` | Élevée |
| Types, résolution, exhaustivité | Implémenté, testé | `typeck.rs` (6,2 k), `resolve.rs` (3,0 k), 471 cas | Élevée sur le sous-ensemble |
| Ownership et emprunts | Implémenté, conservateur, sur AST | `own.rs` (2,4 k lignes produit), T4 `BLOCKED` (audit humain absent) | Moyenne |
| Effets et profils `pure`/`rt` | Implémenté ; `rt` expérimental | `effects.rs`, design §3.6 | Moyenne |
| Backend | Transpilation vers Rust, un `rustc` par programme | `pipeline.rs:646-741`, `emit.rs` (7,7 k) | Élevée (fonctionne) |
| IR propre, Cranelift | Absent ; `--emit-ir` est un dump de faits | `pipeline.rs:860`, T1 `PARTIAL` | — |
| Incrémental du langage | Absent | pas de `-C incremental` ; le LSP recharge et revérifie tout le package (`analysis.rs:36-51`) | Élevée (constat) |
| Hot reload `.rv` | Redémarrage fail-closed, état perdu | T8 `PARTIAL`, design §6 | Élevée (constat) |
| Incrémental Rust, hot swap, DSP | Implémenté et mesuré, pour Rust | `STATUS.md` | Moyenne (mesures locales) |
| Temps réel | 0 allocation au callback mesuré ; aucune qualification hard-RT | `STATUS.md` L4-C | Faible au-delà du test |
| WASM | WASI preview1 seulement | design §5 | Moyenne |
| Mobile, embarqué, GPU, frontend web, async, threads, traits, closures | Conceptuel ou `UNSUPPORTED` | design §5 | — |
| Preuves formelles | Aucune ; Kani absent | `STATUS.md` | — |

Les rapports sont honnêtes sur leurs limites. Leur faiblesse est la lisibilité : `LANGUAGE-CORE-STATUS.md` empile onze blocs « mise à jour passe N » sur un corps daté du 9 novembre.

## 3. Architecture actuelle et écarts avec la vision

```
.rv → loader → parser (NodeId globaux) → resolve → typeck → own → effects
    → CheckedProgram{AST + Facts} → emit.rs → 1 fichier .rs → rustc → binaire
```

| Vision | Réalité | Écart |
|---|---|---|
| Compilation incrémentale très rapide | Programme entier, `debuginfo=2`, sans incrémental. Le banc M5 rapporte la voie B 12 à 31 % plus lente que Rust direct. | Fondamental : plus lent que Rust par construction |
| Mise à jour à chaud | Redémarrage de processus. Le hot swap de `ravum-patch-host` est inaccessible depuis `.rv` : pas de sortie `cdylib`, pas d'ABI. | Fondamental |
| Sûreté sans GC | Atteinte par restriction (aucune référence stockée ni retournée, sauf `&'static str`) et par `rustc` en dernier filet. | Faible aujourd'hui ; le modèle est très restrictif |
| Prévention des crashs | Débordement, index et division provoquent un panic contrôlé. Ces traps ne sont pas des effets. | Un programme `pure` ou `rt` peut encore s'arrêter |
| Diagnostics pour agents | JSON versionné, 108 codes au registre canonique, codes de sortie stables. | Faible : point fort |
| Temps réel DSP | Le profil `rt` interdit des effets à la granularité fonction, sans budget ni WCET. | Important |
| Développement parallèle par agents | Règle « un fichier = un propriétaire », mais trois fichiers de 5 à 8 k lignes. | Goulot de parallélisme |

## 4. Problèmes critiques

**C1 — RISQUE AVÉRÉ (P0). Le mandat des agents contredit la réalité.**
- Preuves : `AGENTS.md` dit « pas encore un compilateur fonctionnel » et que le MVP n'a pas à créer de frontend. `STATUS.md` (9 novembre) place le langage sur `feat/lang-core-1.0-rc`. `DECISIONS_REQUIRED.md` §0 indique la fusion dans `main` le 12 novembre et la cible D2 « production ready ».
- Impact : un agent délégué qui suit `AGENTS.md` à la lettre part d'un état faux. Ce fichier est sa première lecture obligatoire.
- Correction : mettre à jour ces deux documents, avec approbation du responsable pour `AGENTS.md`.

**C2 — RISQUE AVÉRÉ (P0). Les divergences `check` accepté / `build` refusé sont une classe ouverte.**
- Preuves : au moins 13 formes fermées en quatre passes (E0508, E0310, E0308), dont une régression introduite par un correctif. Le rapport dit « aucune forme ouverte *connue* ».
- Cause probable : le checker raisonne sur des places (`own.rs`, `Place`/`Step`), et un « planificateur » de déstructuration côté backend semble re-dériver les moves pour satisfaire `rustc`. Deux modèles évoluent séparément. La duplication exacte reste une HYPOTHÈSE : `emit.rs` n'a pas été lu.
- Impact : viole P0-A. Chaque extension du langage rouvre la classe. Aucun risque mémoire, car `rustc` refuse bruyamment.
- Correction : oracle différentiel génératif (D2), puis source unique des faits de move (D3).

**C3 — RISQUE AVÉRÉ (P0, documentaire). Le design nie le rôle réel de `rustc`.**
- Preuves : design §1 dit que `rustc` n'est « jamais vérificateur de sémantique ». En pratique il est le filet de sûreté mémoire grâce à `forbid(unsafe_code)`. INV-12 le reconnaît à moitié.
- Impact : aujourd'hui une erreur de `own.rs` coûte un build cassé. Sur une voie C, la même erreur deviendrait une faille mémoire silencieuse. Le document masque cette différence.
- Correction : écrire la base de confiance réelle (D1).

**C4 — RISQUE AVÉRÉ (P1). Aucune incrémentalité côté langage.**
- Preuves : `pipeline.rs:712-741`, `analysis.rs:36-51`.
- Impact : la cible « diagnostic p95 < 50 ms » est déjà déclarée non atteinte côté plateforme. Côté `.rv` elle est NON VÉRIFIÉE : aucune mesure par phase trouvée.
- Correction : mesurer d'abord, puis `-C incremental`, puis cache par fichier (D4).

**C5 — RISQUE AVÉRÉ (P1). `NodeId` globaux dans l'ordre de lecture.**
- Preuves : design §4 les déclare « interface gelée ». `Facts` est indexé par ces identifiants.
- Impact : éditer un fichier décale les identifiants de tous les fichiers chargés après lui. Aucun fait ne peut être mis en cache par fichier. Le coût de migration croît avec chaque consommateur ajouté.
- Réserve : l'allocation exacte dans `parser.rs` est NON VÉRIFIÉE.
- Correction : D5.

**C6 — RISQUE AVÉRÉ (P1). Le profil `rt` n'exclut pas les traps.**
- Preuves : design §3.6 (« limite assumée »). L'effet `alloc` est approché à la granularité fonction.
- Impact : une fonction `rt` peut paniquer dans un callback audio, ce qui contredit INV-05.
- Correction : D6.

**C7 — RISQUE AVÉRÉ (P2). Fichiers monolithiques.** `emit.rs` 7 737 lignes, `typeck.rs` 6 194, `parser.rs` 5 417 : conflits entre agents et revue humaine difficile, alors que T4 attend justement un audit humain.

**C8 — HYPOTHÈSE (P2). L'objectif de couverture 100 % lignes et régions (D10) détourne l'effort.** Pour un borrow checker, la qualité des assertions compte plus que les lignes atteintes ; des fichiers `mutants-*.out` montrent que le test de mutation existe déjà. C'est une décision du responsable, elle est seulement signalée ici.

**C9 — BUG CONFIRMÉ (mineur). Un répertoire `C:/Users/…` existe à la racine du dépôt.** C'est un chemin Windows absolu traité comme relatif par un outil. L'outil fautif est NON VÉRIFIÉ.

**État de l'arbre.** 11 fichiers modifiés non commités au moment de l'audit, dont six sources `ravum-lang-sema` et `tools/coverage_thresholds.json`. À intégrer ou isoler avant toute délégation.

## 5. Décisions architecturales recommandées

**D1 — Statut de `rustc`.**
- Options : (a) maintenir « jamais vérificateur » ; (b) déclarer la défense en profondeur.
- Choix : (b). Le checker Ravum reste l'autorité des diagnostics. `rustc` sous `forbid(unsafe_code)` est le filet mémoire officiel de la voie B. La base de confiance est `rustc`, le prélude (280 lignes) et les wrappers FFI.
- Conséquence : Ravum ne peut pas être plus permissif que Rust sur la voie B.
- Validation : un test échoue si un programme sans `extern` est émis sans `forbid(unsafe_code)`. L'existence de ce test dans `backend_audit.rs` est NON VÉRIFIÉE.

**D2 — Fermer la classe C2.**
- Options : (a) sondage manuel continu ; (b) fuzz différentiel génératif ; (c) IR de type MIR partagée par le checker et le backend.
- Choix : (b) maintenant, (c) différée.
- Justification : (c) est la bonne cible mais revient à une réécriture. (b) transforme « aucune forme connue » en taux mesuré, et sert de garde-fou à (c).
- Forme : un générateur déterministe à graine produit des programmes bien typés par construction, centrés sur les agrégats imbriqués, les moves par projection, `match`, le shadowing et les boucles. Propriété : `check` accepte ⇒ `rustc` accepte. Second axe : mêmes sorties en debug et en release.
- Validation : N programmes par graine en CI sans divergence ; chaque divergence devient un cas de conformance.

**D3 — Source unique des faits d'ownership.**
- Choix : `own.rs` écrit ses décisions dans `Facts` (pour chaque expression : move, copie, emprunt, place canonique). `emit.rs` les consomme et ne re-dérive plus rien.
- Compromis : `own.rs` cesse d'être un pur validateur, ce qui élargit légèrement l'interface gelée.
- Conséquence : le planificateur du backend se réduit à une traduction ; une IR future part de ces faits.
- Validation : plus aucune analyse de move dans `emit.rs` ; le fuzz D2 reste vert ; le Rust émis est identique octet pour octet sur le corpus, ou chaque différence est justifiée.
- Préalable : lire `emit.rs` pour confirmer la duplication.

**D4 — Trajectoire d'incrémentalité.**
- Options : (a) `-C incremental` sur le fichier unique ; (b) un crate Rust par module `.rv`, orchestré par `ravum-unit-graph` et le daemon ; (c) voie C Cranelift.
- Choix : mesurer par phase, puis (a). Passer à (b) seulement si `rustc` domine encore. Geler (c).
- Justification : (a) coûte un argument et un répertoire de cache. (b) réutilise l'existant mais impose de résoudre visibilité et monomorphisation entre crates. (c) fait de `own.rs` l'unique gardien mémoire, inacceptable tant que T4 est bloqué.
- Conséquence : les builds release restent non incrémentaux et bit-à-bit reproductibles.
- Validation : p50 et p95 par phase publiés pour chaque exemple, à froid et après édition d'une ligne.

**D5 — Identité des nœuds.**
- Options : (a) statu quo ; (b) plages de `u32` réservées par fichier ; (c) identifiant local au fichier, plus chemin de définition pour les références entre fichiers.
- Choix : (c), le modèle de rust-analyzer : `NodeId = (FileId, index local)`. Les références inter-fichiers passent par un chemin de définition (module et nom). Les caches sont indexés par hash de contenu, jamais par `FileId`.
- Compromis : migration transversale de `Facts`. (b) est fragile à l'ajout de fichiers.
- Validation : éditer un fichier ne change aucun identifiant ni aucun fait des autres fichiers ; sortie de codegen identique avant et après migration.
- Ordre : après D2, qui sert de filet de régression. Amender le design §4 par RFC.

**D6 — Sémantique des traps en profil `rt`.**
- Options : (a) statu quo documenté ; (b) en `rt`, indexation et division produisent l'effet `panic` sauf opérande littéral prouvable, et l'arithmétique entière exige des formes explicites (`wrapping_*`, `saturating_*`, `get`) ; (c) preuve d'intervalles.
- Choix : (b), soumis au responsable par RFC car c'est une règle de langage.
- Justification : (c) est un projet de recherche ; (a) vide le profil de son sens.
- Validation : cas de conformance négatifs pour `a[i]`, `x / y` et `i + 1` en `rt` ; `examples/dsp-gain-iir` compile sans trap implicite.

**D7 — Relier les deux lignes.**
- Choix : une seule verticale, `ravum-lang build --emit dsp-block`. Elle produit un `cdylib` dont le wrapper d'ABI de bloc (`ravum-patch-host/src/block.rs`) est généré par le codegen, pour une fonction de profil `rt`, avec état possédé par l'hôte.
- Compatibilité d'état : digest du schéma de la struct d'état (noms, types, ordre). Digest différent ⇒ swap refusé, ancienne version conservée (INV-03). Pas de migration d'état dans cette verticale.
- Compromis : introduit `unsafe` dans le wrapper généré, confiné et audité comme la FFI.
- Validation : édition d'un `.rv` pendant le rendu, swap sans allocation au callback, refus prouvé sur changement de schéma.

## 6. Plan de correction

Complexité : F faible, M moyenne, É élevée. « Délégable » désigne un modèle moins coûteux.

### P0

| ID | Problème | Modules | Solution | Dépend de | Tests | Acceptation | Cx | Délégable |
|---|---|---|---|---|---|---|---|---|
| P0-1 | Arbre non commité | 11 fichiers modifiés | Terminer et commiter le lot couverture, ou l'isoler | — | Suites `sema`, `tools/tests` | `git status` propre | F | Oui |
| P0-2 | Mandat périmé (C1) | `AGENTS.md`, `STATUS.md`, `START_HERE.md` | Décrire l'état réel et la cible D2 | Approbation humaine | `validate_repo_docs.py`, `report_consistency.py` | Aucune contradiction avec `DECISIONS_REQUIRED` §0 | F | Oui, PR non fusionnée |
| P0-3 | Base de confiance mal décrite (C3) | Design §1, `spec/language/ownership.md` | Appliquer D1 ; garantir le test `forbid` | — | Test backend, à créer s'il manque | Test rouge si `forbid` disparaît | F | Oui |
| P0-4 | Classe C2 ouverte | Nouveau `ravum-lang-sema/tests/diff_gen.rs` ou `tools/` | Générateur et oracle D2 | P0-1 | Lui-même, graines fixes | 10 000 programmes sur 3 graines sans divergence, job CI | É | Partiel : cadrage par modèle fort |
| P0-5 | Gel implicite absent | `BACKLOG.md` | Geler voie C et extensions jusqu'à P0-4 vert et T4 levé | P0-2 | — | Écrit et approuvé | F | Oui |

### P1

| ID | Problème | Modules | Solution | Dépend de | Tests | Acceptation | Cx | Délégable |
|---|---|---|---|---|---|---|---|---|
| P1-1 | Boucle `.rv` non mesurée (C4) | `pipeline.rs`, `json.rs` | `--timings` : durées de chaque phase en JSON | P0-1 | Test de contrat JSON | Rapport p50/p95 par exemple, avec SHA, OS et matériel | M | Oui |
| P1-2 | Pas d'incrémental `rustc` | `pipeline.rs`, `BuildIdentity` | `-C incremental=<cache>` en dev seulement | P1-1 | `build_isolation.rs`, test de reproductibilité release | Gain mesuré ; digest release inchangé | F | Oui |
| P1-3 | Double modèle de moves (C2) | `own.rs`, `facts.rs`, `emit.rs` | D3 | P0-4 | Conformance `--verify-build`, fuzz | Plus de planification de move dans `emit.rs` | É | Non |
| P1-4 | `NodeId` globaux (C5) | `ast.rs`, `parser.rs`, `facts.rs`, consommateurs | D5 après RFC | P0-4, P1-3 | Test de stabilité des identifiants | Identifiants des autres fichiers inchangés | É | Mécanique oui, conception non |
| P1-5 | Traps en `rt` (C6) | `effects.rs`, `typeck.rs`, spec | RFC D6 puis implémentation | Approbation | Cas négatifs | Voir D6 | M | Oui après RFC |
| P1-6 | Statut illisible | `LANGUAGE-CORE-STATUS.md` | Un état courant ; historique en annexe | P0-2 | `report_consistency.py` | Un seul jeu de chiffres courant | F | Oui |
| P1-7 | Répertoire `C:` (C9) | Racine, outil fautif | Trouver l'écrivain, corriger, supprimer après inspection | — | Test du chemin | Répertoire absent après la suite | F | Oui |

### P2

| ID | Problème | Solution | Dépend de | Acceptation | Cx | Délégable |
|---|---|---|---|---|---|---|
| P2-1 | Le LSP revérifie tout | Cache de parse et de résolution par hash de fichier | P1-4 | Diagnostic p95 < 50 ms sur l'exemple multi-module, mesuré | É | Partiel |
| P2-2 | Lignes disjointes | Verticale D7 | P1-5, P0-4 | Voir D7 | É | Non pour l'ABI |
| P2-3 | Monolithes (C7) | Découpage par déplacement pur | P1-3, P1-4 | Diff de déplacement seul ; tests et couverture identiques | M | Oui |
| P2-4 | `rustc` domine encore | Un crate par module via `ravum-unit-graph` | P1-2 mesuré insuffisant | Gain mesuré sur édition d'un module | É | Non |
| P2-5 | Voie C | Rouvrir seulement si T4 levé et fuzz stable sur 30 jours | P0-4, T4 | Décision humaine | — | Non |

## 7. Matrice des tests et validations

| Garantie | Validation existante | Manque | Tâche |
|---|---|---|---|
| `check` ⇒ `build` | Conformance `--verify-build` (202 cas, dont 81 validés seulement parce que le binaire s'est exécuté) | Génératif | P0-4 |
| Sûreté mémoire | `forbid(unsafe_code)`, Miri Linux, 106 cas adversariaux | Audit humain (T4) ; test de non-régression du `forbid` NON VÉRIFIÉ | P0-3 |
| Déterminisme | Reproductibilité bit-à-bit, identité de build, pas de `HashMap` | Garde contre `HashMap` dans `ravum-lang-*` ; repro avec incrémental | P1-2 |
| Parité debug/release | Smoke | Axe du fuzz | P0-4 |
| Activation sûre | `watch` fail-closed, `watch_continuity.rs` | Refus de swap sur schéma d'état | P2-2 |
| Latence | Mesurée pour Rust | Aucune mesure par phase pour `.rv` | P1-1 |
| Profil `rt` | Effets transitifs (EFF-4.7, EFF-2.9) | Traps, allocation fine | P1-5 |
| Stabilité des identifiants | Aucune | Test d'édition isolée | P1-4 |
| Diagnostics | `diagnostics_contract.rs`, registre | — | — |
| Qualité des tests | Mutation existante | Score de mutation sur `own.rs` comme gate | À proposer au responsable |

## 8. Risques résiduels et éléments non vérifiés

- **NON VÉRIFIÉ** : allocation exacte des `NodeId` ; contenu de `emit.rs` (donc la duplication C2) ; boucle `watch` ; contenu de `fuzz_check.rs` (un oracle différentiel partiel peut déjà exister) ; qualité des tests de la plateforme ; workflows CI ; `spec/language/` ; les 42 ADR (`NOT_REVIEWED`) ; origine du répertoire `C:`.
- **Limite de portée** : aucun programme n'a été exécuté pendant l'audit. « Fonctionne » signifie « rapporté par le dépôt avec commande et SHA ».
- **Expressivité** : sans références stockées, traits ni closures, le Core 1.0 ne permet pas d'écrire des structures de données ou des API non triviales. Chaque extension rouvre C2 et le modèle d'ownership.
- **Dépendance à `rustc`** : la voie B ne sera jamais plus rapide à compiler que Rust à froid. Le gain ne peut venir que de l'incrémental du frontend et d'un découpage fin.
- **Mesures** : une seule machine Windows. Aucune mesure de productivité (P0-I).
- **Audit humain** : T4 et T12 restent bloqués quoi que fassent les agents.

## 9. Prompt GOAL pour un agent moins coûteux

```
GOAL — Ravum : consolidation des fondations du langage (suite de l'audit du 2026-11-13)

CONTEXTE ÉTABLI (ne pas re-auditer)
- Dépôt D:\App\Ravum (github.com/Rwanbt/Ravum). Lire AGENTS.md, puis
  DECISIONS_REQUIRED.md §0, puis docs/architecture/RAVUM-CORE-DESIGN.md,
  puis docs/audit/2026-11-13-architecture-audit.md.
  Attention : AGENTS.md et docs/architecture/STATUS.md sont périmés sur un
  point. Le langage existe, il est fusionné dans main (2026-11-12) et la cible
  est « production ready » (D2).
- Deux lignes disjointes : la plateforme Rust (crates/ravum-* hors lang) et le
  langage (crates/ravum-lang-{syntax,sema,codegen,cli,lsp}). Chaîne du langage :
  .rv → parse → resolve → typeck → own → effects → CheckedProgram
  → codegen/emit.rs → un fichier Rust → rustc.
- Sûreté mémoire : le checker Ravum fait autorité pour les diagnostics ; rustc
  sous #![forbid(unsafe_code)] est le filet de sûreté officiel.
- Défaut structurel connu : des programmes acceptés par `check` puis refusés
  par rustc au `build`.

DÉCISIONS À RESPECTER
1. Ne pas démarrer la voie C (Cranelift), ni traits, closures, async, ni
   aucune extension de surface du langage.
2. Ne pas modifier NodeId, own.rs → Facts, ni la sémantique du profil rt sans
   RFC approuvée par le responsable. Ces tâches (P1-3, P1-4, P1-5) sont hors de
   ton périmètre.
3. Sortie déterministe : pas de HashMap/HashSet dans crates/ravum-lang-* ;
   builds release non incrémentaux et reproductibles bit à bit.
4. Tout Rust généré sans `extern` porte #![forbid(unsafe_code)].
5. Aucune nouvelle dépendance externe sans justification écrite et approbation.

TÂCHES, DANS L'ORDRE (une branche depuis dev et une PR vers dev par tâche)
T1 (P0-1) État de l'arbre. La branche test/coverage-batch-1 a 11 fichiers
   modifiés non commités. Ne rien écraser ni jeter. Lancer
   `cargo test -p ravum-lang-sema` et les tests de tools/. Si tout est vert,
   proposer des commits atomiques. Sinon rapporter et s'arrêter.
T2 (P0-2, P1-6) Documentation. Proposer la mise à jour de AGENTS.md, STATUS.md
   et START_HERE.md pour refléter l'état réel. Réduire
   LANGUAGE-CORE-STATUS.md à un état courant unique, historique en annexe.
   AGENTS.md exige l'approbation du responsable : PR non fusionnée. Valider
   avec tools/validate_repo_docs.py et tools/report_consistency.py.
T3 (P0-3) Vérifier qu'un test échoue si un programme sans `extern` est émis
   sans #![forbid(unsafe_code)]. Le créer dans
   crates/ravum-lang-codegen/tests/backend_audit.rs s'il manque. Corriger
   RAVUM-CORE-DESIGN.md §1 : rustc est le filet de sûreté mémoire de la voie B.
T4 (P0-4) Oracle différentiel. Lire d'abord
   crates/ravum-lang-sema/tests/fuzz_check.rs pour ne rien dupliquer. Écrire un
   générateur déterministe à graine de programmes .rv bien typés par
   construction : structs, enums, tuples, tableaux imbriqués, moves par
   projection, match, if/else, boucles, shadowing, String, Vec. Propriété 1 :
   `check` accepte ⇒ le Rust généré compile avec rustc. Propriété 2 : sorties
   identiques en debug et en release. Chaque divergence devient un cas minimal
   dans conformance/cases/ et est RAPPORTÉE. Ne pas corriger le compilateur
   dans cette tâche. Ne jamais restreindre le générateur pour obtenir du vert.
   Acceptation : 3 graines × 10 000 programmes, résultat chiffré, job CI.
T5 (P1-1) `--timings` sur `ravum-lang check` et `build` : durées de load,
   parse, resolve, typeck, own, effects, codegen, rustc en JSON
   (schema_version inchangé, champ additif). Test de contrat. Rapport p50/p95
   par exemple de examples/, à froid et après édition d'une ligne, avec SHA,
   rustc, OS et matériel.
T6 (P1-2) `-C incremental=<cache>` pour les builds dev uniquement
   (crates/ravum-lang-cli/src/pipeline.rs, compile_program). Le répertoire
   incrémental est hors de l'identité publiée. Re-mesurer avec T5. Prouver que
   le digest de reproductibilité release est inchangé.
T7 (P1-7) Un répertoire `C:` existe à la racine du dépôt. Trouver l'outil qui
   l'écrit, corriger la gestion du chemin, ajouter un test. Ne supprimer le
   répertoire qu'après avoir listé son contenu dans le rapport.

RÈGLES D'EXÉCUTION
- Chaque changement arrive avec ses tests positifs ET négatifs.
- Ne jamais supprimer, ignorer, affaiblir un test ou un seuil, ni marquer
  `#[ignore]` pour obtenir du vert. Un échec se rapporte avec commande, SHA et
  sortie brute.
- Préserver les contrats existants : codes de diagnostic
  (spec/language/diagnostics.md §3), schéma JSON, codes de sortie, corpus de
  conformance (`cargo run -p ravum-conformance -- run --verify-build` doit
  rester à 0 FAIL).
- Commits atomiques au format Conventional Commits, un sujet par commit. Pas
  de force-push, pas de fusion, pas de release, pas d'acceptation d'ADR.
- Toute décision nouvelle : RFC dans docs/rfc/ (identifiant RFC, jamais ADR),
  statut PROPOSED, avec options, choix et critères de validation.
- Ne déclarer une fonctionnalité validée qu'avec commande exécutée, SHA,
  versions et résultat chiffré. Statuts autorisés : TESTED, PARTIAL, NOT_RUN,
  UNKNOWN, BLOCKED. Jamais PROVED.
- Dette : aucune abstraction sans consommateur réel ; pas de refactoring hors
  périmètre de la tâche.
- Si une tâche exige de toucher own.rs, emit.rs, NodeId ou la spec normative
  au-delà d'un correctif local, s'arrêter et demander.

RAPPORT FINAL PAR TÂCHE
Objectif, périmètre, fichiers modifiés, tests ajoutés (positifs et négatifs),
commandes et résultats bruts, SHA, versions, limites, statut de preuve,
questions ouvertes pour le responsable.
```
