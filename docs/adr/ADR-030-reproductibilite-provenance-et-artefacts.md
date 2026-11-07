# ADR-030 — Reproductibilité, provenance et artefacts

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-029.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Une preuve sans lien fiable avec le code machine et les dépendances est impossible à auditer.

## Décision proposée

Lockfiles complets, SHA exact, toolchain pin, artefacts identifiés, SBOM, provenance SLSA-compatible quand applicable et manifestes de preuve ; source date epoch contrôlé.

## Alternatives à comparer / écarter

`latest` implicite ; releases manuelles non traçables ; supposer bit-identique sur toutes machines.

## Conséquences et compromis

+ Traçabilité et audit. - Rebuild reproductible multi-platform demande travail.

## Preuves à produire avant acceptation

Double build indépendant sur machines contrôlées, comparaison binaire/diffoscope et rapport d’écarts documentés.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Les plateformes ne permettent aucune reproducibilité acceptable.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
