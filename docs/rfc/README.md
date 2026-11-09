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

## Relation avec les autres documents

- Décisions d'architecture existantes : [`../adr/README.md`](../adr/README.md)
- Invariants projet (P0, INV) : [`../architecture/PROJECT_INVARIANTS.md`](../architecture/PROJECT_INVARIANTS.md)
- Réalité actuelle : [`../architecture/STATUS.md`](../architecture/STATUS.md)
- Revue multi-IA non ratifiée : [`../reviews/2026-11-08-consolidation.md`](../reviews/2026-11-08-consolidation.md)

Une RFC ne modifie ni les ADR, ni les invariants, ni la roadmap. Elle propose ; elle n'engage rien.
