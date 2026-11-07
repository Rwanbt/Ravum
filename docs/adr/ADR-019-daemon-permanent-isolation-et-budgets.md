# ADR-019 — Daemon permanent, isolation et budgets

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-012.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Une compilation permanente peut saturer le PC et affecter les IDE ou les agents.

## Décision proposée

Daemon par workspace/scope avec limites RAM/CPU/threads, annulation, priorité interaction, cache LRU borné et mode CLI batch sans service résidant.

## Alternatives à comparer / écarter

Daemon système global non isolé ; compilateur uniquement batch ; worker non interruptible.

## Conséquences et compromis

+ Réactivité maîtrisée, ressources prévisibles. - Priorités et gestion de concurrence complexe.

## Preuves à produire avant acceptation

Stress multi-projets, kill/restart, mémoire forcée, restauration cache, mesure CPU idle et de latence p95.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Le daemon ajoute plus de coût que de gain sur travaux fréquents.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
