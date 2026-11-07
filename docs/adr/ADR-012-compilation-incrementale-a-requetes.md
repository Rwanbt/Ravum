# ADR-012 — Compilation incrémentale à requêtes

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-010, ADR-011.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Recompiler depuis zéro après chaque token est incompatible avec fluidité et budgets CPU/RAM.

## Décision proposée

Graphe de requêtes dépendantes façon Salsa ; granularité fonction/type/contrat et invalidation consciente des interfaces publiques.

## Alternatives à comparer / écarter

Build intégral à chaque sauvegarde ; suivi de timestamp fichiers seulement ; cache manuel sans dépendances.

## Conséquences et compromis

+ Réutilisation fine. - Bogues d’invalidation particulièrement dangereux.

## Preuves à produire avant acceptation

Oracle froid vs chaud bit-à-bit/semantic parity, invalidations adversariales, mutation de types et dépendances transversales, benchmarks RAM.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Le système ne se stabilise pas ou se révèle plus coûteux que des rebuilds simples.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
