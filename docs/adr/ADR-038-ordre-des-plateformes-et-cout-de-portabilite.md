# ADR-038 — Ordre des plateformes et coût de portabilité

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-000, ADR-023.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Toutes plateformes à la fois multiplient exponentiellement la matrice de tests.

## Décision proposée

Targets prioritaires : x86_64 Linux + Windows et WASM ; puis macOS/ARM, Android/iOS et no_std ; GPU/hard-RT par cible séparée. Décision à confirmer par bench.

## Alternatives à comparer / écarter

Tout cibler avant T1 ; Windows uniquement ; WASM comme runtime obligatoire.

## Conséquences et compromis

+ Focus des tests et expérience réelle. - Universalisme retardé.

## Preuves à produire avant acceptation

Matrice par cible / fonctionnalités, cross-compile, smoke sur machines réelles, package reproductible.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Demande matérielle prioritaire change les contraintes initiales.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
