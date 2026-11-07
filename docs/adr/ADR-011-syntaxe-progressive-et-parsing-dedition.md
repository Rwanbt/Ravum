# ADR-011 — Syntaxe progressive et parsing d’édition

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-010.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Une IA et un éditeur produisent des fragments temporairement invalides.

## Décision proposée

Parser tolérant, arbres partiels et overlays non sauvegardés ; jamais générer du code exécutable issu d’un AST invalide.

## Alternatives à comparer / écarter

Parser batch strict uniquement ; exécuter des fragments au fil des tokens ; parser via outil externe opaque.

## Conséquences et compromis

+ Diagnostics pendant frappe et stabilité de preview. - Sémantique des nœuds de récupération à formaliser.

## Preuves à produire avant acceptation

Fuzz éditions à chaque position, interruptions d’écriture, tests de synchronisation source/buffer et résilience parser.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Dérives permanentes AST/source ou complexité mémoire disproportionnée.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
