# ADR-039 — Validation de l’optimisation et compatibilité de versions

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-007, ADR-018, ADR-035.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Optimisations d’IA, LLVM et patch peuvent changer la sémantique.

## Décision proposée

Préservation des obligations contractuelles, differential testing et translation validation sur sous-ensembles critiques ; ne pas supposer équivalence totale de programmes arbitraires.

## Alternatives à comparer / écarter

Comparaison benchmark seule ; ignorer codegen ; exiger preuve complète de tout binaire instantanément.

## Conséquences et compromis

+ Optimisation sous contraintes. - Verification très coûteuse sur transformations complexes.

## Preuves à produire avant acceptation

Tester mutants d’optimizer, overflow/NaN/concurrence, refuser transform où validation échoue ou devient UNKNOWN en profil strict.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

L’approche n’évolue pas au-delà de micro-transformations locales.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
