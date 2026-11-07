# ADR-016 — Model checking, fuzzing et simulation déterministe

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-008, ADR-015.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Tests unitaires classiques échouent souvent à explorer concurrence et fautes.

## Décision proposée

Fixture runtime virtualisable (horloge, random, scheduling, réseau, stockage), seed et replay versionnés ; fuzz/property-based et model checking ciblés.

## Alternatives à comparer / écarter

Tests système non reproductibles seulement ; prétendre que le simulateur couvre tout ; RNG non enregistré.

## Conséquences et compromis

+ Reproduction et triage automatisé. - Interfaces environnementales et couverture partielle.

## Preuves à produire avant acceptation

100 replays identiques sur système figé, fault matrix, crash traces identiques ; marquer les appels non virtualisés comme hors modèle.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Test réel et simulé divergent systématiquement sans explication.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
