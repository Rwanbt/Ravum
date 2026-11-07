# ADR-002 — Modèle mémoire : ownership, emprunts et régions

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-001.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Sans GC obligatoire, sûreté, faible latence et `no_std` imposent contrôle précis des durées de vie.

## Décision proposée

Valeurs possédées, emprunts exclusifs/partagés, restrictions d’aliasing, régions/arènes optionnelles ; API explicite pour références partagées.

## Alternatives à comparer / écarter

GC tracing obligatoire ; simple refcount sur toutes les valeurs ; gestion manuelle C non vérifiée.

## Conséquences et compromis

+ Sécurité et déterminisme de ressources. - Ergonomie et inférence potentiellement difficiles.

## Preuves à produire avant acceptation

Test compile-fail exhaustif des emprunts invalides, tests d’alias et génération différentielle vers code Rust sûr.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Une famille d’applications valides est impossible à modéliser sans `unsafe` disproportionné.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
