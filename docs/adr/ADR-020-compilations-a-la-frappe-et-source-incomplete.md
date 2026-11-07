# ADR-020 — Compilations à la frappe et source incomplete

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-011, ADR-019.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Les préviews doivent suivre les buffers d’éditeur et pas seulement les fichiers sauvegardés.

## Décision proposée

Protocoles overlays versionnés ; syntaxe invalide analysable mais jamais activée ; compilation dès unités closes/valides, debouncing adaptatif et annulation du travail obsolète.

## Alternatives à comparer / écarter

Exécuter code partiel ; n’activer que sauvegarde disque ; interpréteur séparé sans sémantique commune.

## Conséquences et compromis

+ Preview réellement continue et sûre. - Granularité de compilation et reconciling des offsets.

## Preuves à produire avant acceptation

Éditions concurrentes human+AI, commandes out-of-order, versions périmées, tests non activation d’AST invalide.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Overlays provoquent désynchronisation fréquente et bugs de compilation.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
