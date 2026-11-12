# RFC — propositions non ratifiées

Ce répertoire contient les **propositions d'ingénierie** qui ne sont ni des ADR, ni des décisions. Une RFC décrit un problème, des invariants, des alternatives et des critères de falsification ; elle ne vaut pas autorisation d'implémenter.

## Règles

1. **Aucun numéro ADR n'est attribué ici.** Les identifiants `ADR-000…ADR-041` restent réservés aux textes existants (`docs/adr/`). Une RFC ratifiée devra être portée par un ADR distinct, référençant les ADR concernés.
2. **Statut par défaut : `PROPOSED`.** Seul le mainteneur peut passer une RFC à `ACCEPTED`, `REJECTED`, `SUPERSEDED` ou `DEFERRED`. Un agent ne peut jamais clore une RFC.
3. **Aucune mesure inventée.** Une RFC qui cite des chiffres doit déclarer s'il s'agit de cibles de conception ou de résultats mesurés, avec SHA, toolchain et environnement.
4. **Un prototype n'est pas une preuve.** L'existence d'un code expérimental ne ratifie rien.
5. **Nommage** : `RFC-<SUJET-EN-MAJUSCULES>.md` ; pas de numérotation séquentielle tant qu'un registre stable n'est pas décidé.

## Index

| RFC | Sujet | Statut | Date | Prototype | ADR liés |
|---|---|---|---|---|---|
| [RFC-SHARED-WORKSPACE-MULTIAGENT](RFC-SHARED-WORKSPACE-MULTIAGENT.md) | Coordination multi-agents sur un espace partagé : snapshots immuables, overlays, ChangeProposals, niveaux de fusion, écrivain Git unique | `PROPOSED` | 2026-11-08 | COLLAB-1 partiel (`ravum-workspace-coordinator`) | 008, 012, 021, 022, 027, 041 |
| [RFC-BACKEND-NATIF-VOIE-C](RFC-BACKEND-NATIF-VOIE-C.md) | Backend natif (voie C) : options, coût, oracle de parité A/B/C, critère de réouverture | `PROPOSED` | 2026-11-10 | aucun (voie C absente) | 000, 001 |
| [RFC-PERIMETRE-UNIVERSEL-D6](RFC-PERIMETRE-UNIVERSEL-D6.md) | Périmètre « universel » : réseau bloquant, async déterministe, threads, hot-state | `PROPOSED` | 2026-11-10 | aucun (surface refusée par le langage) | 000, 002, 041 |
| [RFC-D2-REFERENCE-BACKEND-CONTRACT](RFC-D2-REFERENCE-BACKEND-CONTRACT.md) | D2 : contrat `source → frontend → CheckedProgram → émission → binaire`, voie B pour Core 1.0, voie C future, pas d'IR ; métriques mesurées | `PROPOSED` · `PENDING_HUMAN_APPROVAL` | 2026-11-11 | voie B en place (contrat de lowering à pinner) | 001, 010, 017 |
| [RFC-D-COERCE-1-MATRIX](RFC-D-COERCE-1-MATRIX.md) | D-COERCE-1 : coercition `String → &str`, matrice accepté/refusé, texte normatif et migration `examples/stats` | `PROPOSED` · `PENDING_HUMAN_APPROVAL` | 2026-11-11 | aucun prototype ; le texte normatif proposé est TYP-8.5/TYP-13.1 | 002 |
| [RFC-D7-FFI-EFFECTS](RFC-D7-FFI-EFFECTS.md) | D7 : frontière `unsafe`/FFI, inventaire réel, effets, contrats manquants (ABI, ressources, unwinding, callbacks), tests négatifs | `PROPOSED` · `PENDING_HUMAN_APPROVAL` | 2026-11-11 | FFI scalaire→scalaire seulement | 005, 009 |
| [RFC-P2-RUNTIME-CAPABILITIES](RFC-P2-RUNTIME-CAPABILITIES.md) | P2-A : runtime modulaire, capacités optionnelles, effets typés, profils ; matrice d'état réelle par capacité | `PROPOSED` | 2026-11-11 | aucun (prélude généré existant) | 005, 023 |
| [RFC-P2-VERTICALS-BLOCKERS](RFC-P2-VERTICALS-BLOCKERS.md) | P2-B/C/D/E : blocages exacts TCP, HTTP, SQLite, verticale full-stack ; preuve de liaison native, tests hermétiques | `PROPOSED` | 2026-11-11 | `examples/file-store` (magasin fichier, PAS SQL) | 023, 025, 026 |
| [RFC-P2-EXPERIMENTAL-SURFACE](RFC-P2-EXPERIMENTAL-SURFACE.md) | P2-GATE : contrat d'isolation d'une surface expérimentale non activée par défaut (15 MUST, fail-closed) | `PROPOSED` — rien n'est activé | 2026-11-11 | aucun | 023, 029 |
| [RFC-P2-CONTINUOUS-COMPILATION](RFC-P2-CONTINUOUS-COMPILATION.md) | P2-F : compilation continue — snapshots, cache valide, invalidation, annulation, rejet des résultats obsolètes, activation/rollback | `PROPOSED` — conception seulement | 2026-11-11 | P0-B (identité/ publication atomique) et P0-C (watch fail-closed) en place | 012, 020, 021, 022 |
| [RFC-D13-STATICITY](RFC-D13-STATICITY.md) | D13 : preuve déterministe de la staticité de `&str` (règle d'origine + règle de stockage) ; rend `a[1]` retournable sans hypothèse | `PROPOSED` — décision du responsable attendue | 2026-11-11 | conformance 456 cas, `E-OWN-REF-ESCAPE` outillé | — |

## Relation avec les autres documents

- Décisions d'architecture existantes : [`../adr/README.md`](../adr/README.md)
- Invariants projet (P0, INV) : [`../architecture/PROJECT_INVARIANTS.md`](../architecture/PROJECT_INVARIANTS.md)
- Réalité actuelle : [`../architecture/STATUS.md`](../architecture/STATUS.md)
- Revue multi-IA non ratifiée : [`../reviews/2026-11-08-consolidation.md`](../reviews/2026-11-08-consolidation.md)

Une RFC ne modifie ni les ADR, ni les invariants, ni la roadmap. Elle propose ; elle n'engage rien.
