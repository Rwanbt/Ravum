# ADR-021 — Hot swap atomique et versionnement du code

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-019, ADR-020.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Le code actif ne peut être remplacé en retirant des fonctions encore utilisées.

## Décision proposée

Points d’entrée indirects versionnés, activation atomique aux safepoints admissibles, conservation des anciens segments jusqu’à quiescence (epochs/refcount adapté), échec fermé.

## Alternatives à comparer / écarter

Patcher octets arbitraires en place ; reconstruire tout l’exécutable pour chaque modification ; supprimer immédiatement l’ancienne version.

## Conséquences et compromis

+ Évolution active sans redémarrer. - Overhead indirection, collecte des versions, limites ABI.

## Preuves à produire avant acceptation

Stress multi-thread, appels in-flight, partage état, échecs patch, tests ASan/TSan/Miri où applicables.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Une classe fréquente de patchs valide ne peut préserver la sécurité ; opter pour redémarrage/sandbox.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
