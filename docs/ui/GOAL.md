# GOAL d'exécution — Ravum UI

Document de **reprise** : un agent sans mémoire de session doit pouvoir
continuer à partir d'ici. Il ne remplace ni [`AGENTS.md`](../../AGENTS.md),
qui prime, ni [`PLAN.md`](PLAN.md) (plan directeur).

## Avant toute modification

1. Lire `AGENTS.md`, puis [`DECISIONS-2026-11-24-UI.md`](../decision/DECISIONS-2026-11-24-UI.md), [`PLAN.md`](PLAN.md) et [`SESSION-LOG.md`](SESSION-LOG.md) (dernière entrée).
2. Relever `git rev-parse HEAD`, la branche, `git status --short` et l'état
   de la CI. Comparer avec le dernier SHA du journal ; ne jamais traiter un
   état ancien comme courant.
3. Vérifier qu'aucune décision du responsable n'est plus récente que ces documents.

## Mandat

- Réaliser Ravum UI selon `PLAN.md`, étape après étape, chacune complète :
  implémentation réelle, tests positifs et négatifs, spec, rapport lié au SHA.
- Ce qui reste interdit sans décision explicite du responsable :
  - ratifier une RFC ou un ADR ;
  - adopter une dépendance ;
  - fusionner, publier, taguer, pousser sur `main` ;
  - affaiblir un test, un seuil ou un contrat.
- En cas de blocage sur une décision humaine : la consigner **une fois**,
  puis poursuivre les travaux qui ne la préjugent pas.
- Vérification : aucune IA dans Verify, déterminisme, fail-closed, statuts honnêtes.

## Prochaine action selon l'état

| État | Action |
|---|---|
| RFC-UI-LANG-1 non ratifiée | U1-B (banc d'essai) et U0 (scripts de mesure) ; ne pas toucher au langage |
| RFC-UI-LANG-1 ratifiée | U1 dans l'ordre de la RFC (§7), une PR par étape |
| Banc U1-B rendu, pile non choisie | attendre la décision UI-D4 ; continuer U1 |
| U1 et pile choisis | U2, puis la suite de `PLAN.md` |

## Fin de chaque session

Ajouter une entrée à [`SESSION-LOG.md`](SESSION-LOG.md) :
- date ;
- HEAD et branche ;
- objectif, fichiers modifiés ;
- commandes et leurs codes de sortie ;
- résultats `PASS`, `FAIL` ou `SKIP` ;
- décisions attendues ;
- prochaine action.
