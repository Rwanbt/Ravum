# ADR-008 — Concurrence, synchronisation et annulation

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-002, ADR-005.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Les serveurs et moteurs média ont besoin de tâches concurrentes sans courses mémoire et de charge maîtrisée.

## Décision proposée

Tâches structurées, canaux typés bornés, possession des messages, annulation explicite, backpressure ; primitives atomiques de bas niveau auditées.

## Alternatives à comparer / écarter

Fibres non structurées ; verrous implicites omniprésents ; actor model exclusif.

## Conséquences et compromis

+ Traçabilité des durées de vie et scénarios simulables. - Adaptateurs aux runtimes existants.

## Preuves à produire avant acceptation

Tests Loom/Shuttle et simulateur, leak/cancel storms, timeouts, canaux saturés, deadlock detection partielle correctement signalée.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Incompatibilité majeure avec environnements async réels.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
