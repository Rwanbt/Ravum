# ADR-023 — Runtime minimal et profils sans runtime

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-000, ADR-002.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Binaire serveur, navigateur et microcontrôleur ne doivent pas partager une grosse VM obligatoire.

## Décision proposée

Runtime en couches opt-in : core no_std, alloc, async IO, patch-dev, OS, WASI ; fonctionnalités de développement exclues des releases sauf décision contraire.

## Alternatives à comparer / écarter

Runtime universel toujours lié ; VM obligatoire ; comportement caché par target.

## Conséquences et compromis

+ Petites tailles et embarqué. - Matrice de compatibilité plus large.

## Preuves à produire avant acceptation

Build `no_std` minimal, tailles de binaire, hooks d’allocation, tests absence de daemon en prod.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Le coût de multiplicité des runtimes excède les gains.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
