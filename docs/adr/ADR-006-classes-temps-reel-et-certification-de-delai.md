# ADR-006 — Classes temps réel et certification de délai

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-005.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Les besoins d’un audio callback et d’un contrôleur certifié ne sont pas les mêmes.

## Décision proposée

Trois classes : `rt_safe` (aucun effet interdit), `rt_bounded` (ressources/itérations bornées), `hard_rt` (analyse WCET + plateforme/configuration certifiée).

## Alternatives à comparer / écarter

Annotation `realtime` binaire unique ; imposer WCET sur tous les OS ; écarter le temps réel.

## Conséquences et compromis

+ Garanties honnêtes et montée progressive. - Complexité de profils et dépendances matérielles.

## Preuves à produire avant acceptation

Callbacks audio instrumentés ; rejets d’alloc/lock même transitifs ; rapport des hypothèses WCET et du scheduler.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Confusion persistante du public entre RT-safe et délais garantis.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
