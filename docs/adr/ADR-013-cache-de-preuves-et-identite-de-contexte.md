# ADR-013 — Cache de preuves et identité de contexte

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-012.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Les preuves conservées doivent dépendre de toutes les hypothèses pertinentes, pas seulement du texte source.

## Décision proposée

Empreinte content-addressed sur IR canonique, contrats, axiomes, solveur, options, modèles, FFI, cibles, version compilateur et dépendances ; cache invalidé par défaut en cas d’incertitude.

## Alternatives à comparer / écarter

Cache par chemin/n° ligne ; cache partagé sans version de solveur ; pas de cache de preuves.

## Conséquences et compromis

+ Preuves rapides à chaud sans régression sournoise. - Stockage, canonicalisation et coûts de hash.

## Preuves à produire avant acceptation

Mutation testing du cache : changer chaque composant du contexte et vérifier invalidation ; comparer preuves fresh/cached.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Une seule fausse réutilisation confirmée : blocage immédiat du cache de preuve.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
