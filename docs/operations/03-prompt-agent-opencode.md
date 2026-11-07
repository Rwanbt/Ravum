# Mandat initial à donner à un agent OpenCode / Unifia

> Copier le prompt ci-dessous après avoir placé ce dossier dans le repo. Il lance **uniquement le cadrage et la première série de spikes**, non tout le compilateur ni une release. Aucune action d'écriture sur un dépôt externe n'a été effectuée lors de la création de ce pack.

```text
MISSION : préparer et expérimenter le futur langage universel vérifiable.
Lis entièrement README.md, docs/plan/00-plan-directeur.md, docs/specs/*,
docs/adr/README.md et les ADR pertinents, docs/plan/01-spikes-et-benchmarks.md,
docs/plan/03-gates-ci-qualification.md, docs/operations/*.

OBJECTIF IMMÉDIAT : rendre F0 auditable et réaliser les spikes S1 (CST +
queries incrémentales), S2 (cache de preuves contextuel et invalidation),
S3 (comparaison backend Rust sûr vs Cranelift) en branches isolées.

CONTRAINTES ABSOLUES :
- Les ADR sont PROPOSED ; ne les passer en ACCEPTED ni changer les contrats
  normatifs sans accord explicite du mainteneur.
- Aucune garantie « sans bug / hard-RT / production-ready » sans preuve.
- Aucune donnée de test fictive présentée comme un test exécuté.
- Main protégée ; pas de merge, release, publish ou déploiement sans autorisation
  et CI exacte au SHA. PR reviewable <= 400 LOC de code hors docs/tests en règle
  générale, avec justification pour toute exception.
- Ne jamais réduire des tests, des contrats, les permissions ou la portée d'un
  contrôle pour rendre un résultat vert.
- Écrire RED→GREEN, fixtures positives et négatives, vérifier erreurs de
  cache et de modèle, mesurer CPU/RAM et p50/p95/p99 sur la même machine.
- Garde les scénarios réseau/temps/accès fichiers sous sandbox ; aucun secret réel.
- Les équipes peuvent déléguer des chantiers indépendants, mais toute sortie
  est intégrée, revue et validée localement avant de la déclarer terminée.

LIVRABLES D'UNE ITÉRATION :
1) Matrice ADR dépendances + questions ouvertes (sans décision unilatérale).
2) Corpus initial programmes positifs/négatifs, scripts de reproduction.
3) POC S1, S2, S3 chacun avec source, tests réels, SHA, versions, commandes,
   sortie logs, métriques et limites.
4) Rapport comparatif : résultats, invalidations erronées, latences, RAM,
   risques, alternatives, recommandation falsifiable et Go/No-Go.
5) Backlog suite ordonné par dépendances, sans annoncer le projet en production.

SI UNE EXPÉRIENCE ÉCHOUE : documenter le cas précis, isoler le reproducer,
marquer NO-GO, proposer et tester une option alternative si faisable.

FIN DU TOUR : faire le plus de travail cohérent possible et rapporter uniquement
les preuves effectivement obtenues, les fichiers modifiés, SHA exact et blocages.
```
