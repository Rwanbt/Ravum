# Exigences vérifiables et traçabilité

Version 0.1, toutes les exigences sont **candidates**. La norme se construira par RFC/ADR acceptés.

## Convention

- `MUST` : obligatoire dans le domaine cible indiqué ; `SHOULD` : visé ; `RESEARCH` : non promis.
- Statuts de couverture : `SPECIFIED`, `IMPLEMENTED`, `TESTED`, `PROVED`, `UNSUPPORTED`. Aujourd'hui tout est `SPECIFIED` ou à discuter, **aucune implémentation n'est revendiquée**.
- Toute propriété dispose d'un **oracle de test**, d'une cible, d'hypothèses et d'une référence de preuve.

| ID | Exigence | Niveau et cible initiale | Oracle / démonstration |
|---|---|---|---|
| FR-001 | Syntaxe structurée, types et compilation native | MUST core | parser/compilation conformance |
| FR-002 | Ownership et emprunts contrôlés | MUST core | négatifs borrow/alias/lifetimes |
| FR-003 | `Option/Result` et erreurs non masquées | MUST core | tests de propagation et erreur |
| FR-004 | Contrats `requires/ensures/invariants` | MUST core | tests valid/invalid/vacuité |
| FR-005 | Effets et capacités par fonction/module | MUST core | lints transgressions et propagation |
| FR-006 | Frontend + backend typés et WASM | MUST fullstack | E2E navigateur + serveur |
| FR-007 | SQL validé et migrations versionnées | MUST data | PostgreSQL/SQLite CI réels |
| FR-008 | CLI `check/build/verify/test/dev/explain` | MUST core/DX | tests CLI interface et codes sortie |
| FR-009 | Language server/overlays non sauvegardés | MUST DX | LSP tests out-of-order |
| FR-010 | Hot swap module compatible + rollback | MUST dev | stress concurrence/reprise |
| FR-011 | Isolation modules/FFI unsafe | MUST security | fuzzing, audit, test violation |
| FR-012 | no_std / runtime optionnel | SHOULD systems | sample bare-metal/build |
| FR-013 | Domaines ARM/mobile/GPU | RESEARCH puis SHOULD | portages qualifiés spécifiques |
| NFR-001 | Temps de réponse diagnostics local p95 cible exploratoire <50 ms | RESEARCH DX | machine définie, edit fixture |
| NFR-002 | Preview UI simple p95 cible <150 ms | RESEARCH DX | mesure edit→visible |
| NFR-003 | Petite fonction hot compile p95 cible <500 ms | RESEARCH DX | benchmark cold/warm |
| NFR-004 | CPU/RAM idle et peak plafonnables | MUST daemon | limites imposées sous charge |
| NFR-005 | Cache invalidé uniquement si les dépendances pertinentes changent | MUST core | oracle cold vs warm + mutations |
| NFR-006 | Package release sans daemon dev | MUST release | sizes/dep audit |
| NFR-007 | Builds traçables, pin des toolchains, SHA exact | MUST CI | rebuild indépendant / evidence |
| SEC-001 | Aucun code d'édition invalide exécuté | MUST DX | fuzz/negative edit tests |
| SEC-002 | Aucune preuve `UNKNOWN/TIMEOUT/ASSUMED` considérée prouvée | MUST all | tests négatifs de statuts |
| SEC-003 | Toute traversée FFI expose une obligation de confiance | MUST all | audit manifest automatique |
| SEC-004 | Permissions minimales pour codegen/scripts/plugins | MUST build | sandbox adversarial |
| SEC-005 | Aucune exfiltration de code/secrets par défaut | MUST all | test mode réseau-off |
| SEC-006 | Une IA ne diminue pas seule les contrats normatifs | MUST AI | tests sécurité workflows |
| SEC-007 | Sources et artefacts identifiés par version/empreinte | MUST release | attestation et tampering |
| RT-001 | Callback `rt_safe` n'alloue pas sur heap | MUST audio | analyse transitive + instrumentation |
| RT-002 | Callback `rt_safe` n'attend pas lock/IO | MUST audio | analyse + stress callback |
| RT-003 | Pas de panic incontrôlé en RT strict | MUST audio | effets/contrats + test négatif |
| RT-004 | Activation patch à frontière sûre, sans travail lourd audio | MUST audio | E2E réel + traces |
| RT-005 | Budget pile/itérations déclaré en RT bounded | SHOULD RT | preuve + instrumentation |
| RT-006 | WCET réel uniquement sur plateforme modélisée | RESEARCH hard-RT | rapport matériel+analyse WCET |
| AI-001 | Diagnostics JSON schema versionné & source spans | MUST AI | schema tests |
| AI-002 | Contre-exemple minimal si disponible | MUST verify | replay fixture |
| AI-003 | Intention de patch et objectifs d'optimisation déclarés | SHOULD AI | diff + review |
| AI-004 | Statuts des preuves et hypothèses récupérables par API | MUST AI | contract tests |
| AI-005 | Corriger sans dégrader les invariants verrouillés | MUST AI | adversarial tests |
| TEST-001 | Differential testing contre oracle de référence | MUST core | fuzz corpus |
| TEST-002 | Simulation déterministe avec seed/replay/version | MUST distributed | replay determinism |
| TEST-003 | Fuzz parser/IR/ABI/patcher | MUST CI | corpus + crash minimization |
| TEST-004 | Aucun gate obligatoire désactivable par un agent | MUST governance | policy tests |

## Répertoire de preuve type

Chaque exigence devient un enregistrement `evidence/<REQ-ID>/<SHA>/<target>/` contenant : commande exacte, version du compilateur, build flags, seed/modèle/hypothèses, logs, junit/json/sarif, machine et kernel, verdict, artefacts hashés, date UTC. **Les résultats ne sont pas inventés et ce pack ne contient aucune preuve d'exécution.**

## Mapping exigences → ADR

- Mémoire/sémantique : FR-001..005 → ADR-002..010.
- Proving : SEC-002, TEST-001 → ADR-013..016, ADR-035.
- Temps réel : RT-001..006 → ADR-006, ADR-024.
- Expérience continue : NFR-001..006, SEC-001 → ADR-011..013, ADR-019..022, ADR-032.
- Full-stack : FR-006..007 → ADR-025..026.
- IA/permissions : AI-001..005, SEC-004..006 → ADR-027..029, ADR-033, ADR-040.
- Production : SEC-007, NFR-007, TEST-003..004 → ADR-030..031, ADR-041.
