# V2-M1 — Revue des 80 MUST « déclaratifs » (`tools/must_declarative.txt`)

| Champ | Valeur |
|---|---|
| Statut | **ACCEPTED** — tri confirmé par le responsable le 2026-11-21 ; ABI-4.7 amendée (liste fermée) ; réécritures de MACH-9.1, CONC-3.1, ABI-5.1 **appliquées** le 2026-11-21 (§D) |
| Date | 2026-11-21 |
| SHA | `078ca20` |

Critère : une règle est **déclarative** si aucun programme, aucune sortie
d'outil ni aucun cas ne peut la faire échouer. Trois familles : **N**
négation de garantie (« aucun ABI », « pas de deadlock garanti ») ; **P**
procédure humaine (RFC, revue, journal) ; **D** définition de vocabulaire.

## A. Je propose de les **retirer** de la liste : testables (13)

| Règle | Pourquoi testable | Test possible |
|---|---|---|
| ABI-2.6 | le backend ne répare aucune erreur | `codegen/tests/backend_fail_closed.rs` existe : à rattacher |
| ABI-4.1 | `CheckedProgram` invalide = bug interne | même fichier (refus interne, pas d'émission) |
| EFF-5.2 | le checker refuse une déclaration trop faible | déjà couvert par `E-EFF-UNDECLARED` : à rattacher |
| MACH-2.2, MACH-6.1, MACH-6.4 | domaine de la machine | la machine refuse ces formes : cas négatifs du corpus machine |
| MOD-6.5 | aucune macro/attribut fourni par une dépendance | cas `packages/` : macro dans une dépendance → refus |
| STD-1.4 | signatures normatives | test croisé `stdlib.md` ↔ `builtins.rs` (signatures) |
| UNS-0.1 | absent ⇒ listé ou `E-RES-UNKNOWN` | cas `unsupported/` |
| UNS-9.3 | `UNSUPPORTED` devenu supporté ⇒ retiré de la table + FeatureId | contrôle d'outil (`unsupported.md` ∩ `features.md` = ∅) |
| FEAT-12.2 | fonctionnalité acceptée sans ligne = défaut | partiellement `features_coverage.py` |
| COMPAT-5.2 | consommateur tolère champs/codes inconnus | pour les consommateurs du dépôt (`tools/json_contract.py`) |
| FEAT-12.1 | « testée » ⇒ cas exécutable | `features_coverage.py --strict` le vérifie déjà |

## B. **Périmées ou violées** : à amender plutôt qu'à classer (4)

| Règle | Écart |
|---|---|
| MACH-9.1 | « tant que l'interpréteur n'existe pas » : la machine de référence existe (`ravum-core-machine`) |
| CONC-3.1 | « aucun callback audio en Core 1.0 » : l'ABI DSP 3 et les blocs `rt` existent désormais |
| ABI-5.1 | « aucun `abi_version` exposé » : l'ABI DSP (1 à 3) expose une version et une signature |
| ABI-4.7 | **violée** : « `#[allow]` interdit », mais `emit/mod.rs:639` émet un `#![allow(dead_code, unused_*, …, arithmetic_overflow, unconditional_panic)]` global. Il est nécessaire en l'état : les avertissements viendraient du source Ravum de l'utilisateur (variables non utilisées), et `arithmetic_overflow`/`unconditional_panic` sont des lints *deny* de `rustc` qui transformeraient un débordement Ravum (panique à l'exécution) en échec de compilation. Choix du responsable : amender ABI-4.7 (liste fermée de lints permis, justifiés) ou rendre le générateur sans avertissement |

## C. Déclaratives confirmées (63)

- **N** (ce qui n'est pas garanti) : ABI-1.1…1.4, 2.1, 2.9, 2.10, 4.8, 4.9,
  6.1…6.4 ; CONC-1.4, 2.4, 2.5, 2.7, 3.3 ; MACH-6.6, 7.4, 8.4, 8.5 ;
  EFF-4.8, 5.1, 6.1, 6.2, 6.3, 6.6 ; MOD-6.6, 8.6 ; STD-1.7, 3.1, 6.8 ;
  COMPAT-3.4.
- **P** (procédure, revue, décision) : ABI-5.2, 5.3 ; COMPAT-2.1…2.7,
  3.1…3.3, 3.5, 6.1…6.3, 6.5, 7.2, 7.3, 7.5 ; CONC-4.1, 4.2, 4.4 ;
  UNS-9.1, 9.2, 9.4, 9.5.
- **D** (définitions) : FEAT-1.1, 1.2.

Total : 13 + 4 + 63 = 80. Appliqué : `tools/must_declarative.txt` ne garde
que les 63 du §C ; les 17 autres sont comptées comme testables non
rattachées par `must_audit.py` (28 au total, travail à faire).

## D. Réécritures (appliquées sur décision du responsable, 2026-11-21)

| Règle | Texte proposé |
|---|---|
| MACH-9.1 | « Une sémantique Ravum n'est dite « vérifiée » que pour les programmes du corpus que la machine de référence (`ravum-core-machine`) exécute avec le résultat attendu : `TESTED` sur ce corpus, jamais `PROVED` (MACH-8.5). » |
| CONC-3.1 | « INV-05 s'applique aux blocs `rt` chargés par l'hôte DSP (ABI DSP 1 à 3) : aucun appel bloquant, allocation, compilation, migration ni destruction coûteuse dans `process` ; un programme Core 1.0 sans bloc DSP n'a pas de callback audio. » |
| ABI-5.1 | « Le seul ABI versionné est l'ABI DSP (`DSP_ABI_VERSION`, `DSP3_ABI_VERSION`, signature vérifiée au chargement, refus fail-closed) ; aucun autre `abi_version` n'est exposé ni promis. » |

ABI-4.7 : amendée dans `abi.md` (liste fermée de 17 lints au niveau du
fichier, plus `allow(unsafe_code)` local des enveloppes FFI/DSP) ; vérifiée
sur tout le corpus par `corpus_codegen.rs` (`abi_4_7_violation`, avec
contre-épreuve). Le seul `allow` local superflu du prélude
(`non_snake_case`, déjà dans la liste) est retiré.

## E. Défaut trouvé en testant STD-1.4 (à décider)

`crates/ravum-lang-sema/tests/stdlib_signatures.rs` compare chaque `pub fn`
de `stdlib.md` au checker : 62 lignes, **2 écarts**. `stdlib.md` (et
l'exemple de `effects.md` l. 47) donne `fs::read_to_string` →
`effects(fs, io, alloc)` et `fs::write_string` → `effects(fs, io)` ; le
checker ne leur donne pas `io`. Deux voies : retirer `io` de la doc (le
checker fait foi en pratique, aucun programme ne casse) ou l'ajouter au
checker (rupture COMPAT-3.5 : programmes `effects(fs)` refusés). Les deux
écarts sont nommés dans le test, qui échoue sur tout autre écart.
