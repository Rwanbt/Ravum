# ADR-031 — CI en gates et qualification exacte au SHA

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-014, ADR-029.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Les garanties varient selon la cible et ne peuvent être inférées du simple succès d’un build.

## Décision proposée

Gates T0 spécification, T1 prototype, T2 safety+conformité, T3 E2E/RT/fullstack, T4 release auditée ; NO-GO si requis rouge, inconnu non justifié ou SHA divergent.

## Alternatives à comparer / écarter

Une seule CI verte ; tests désactivés pour merger ; déploiement à la validation textuelle.

## Conséquences et compromis

+ Dossiers de preuves fiables. - Coût CI et maintenance.

## Preuves à produire avant acceptation

Tester CI négative, jobs requis réels, matrice cibles, SHA des artefacts, contrôle policy-as-code.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Coûts CI non tenables : réduire les variantes exploratoires, non les gates critiques.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
