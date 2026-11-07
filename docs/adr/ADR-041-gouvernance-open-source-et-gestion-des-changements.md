# ADR-041 — Gouvernance open source et gestion des changements

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-030.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Le compilateur sera une infrastructure de confiance avec contributions humaines et IA.

## Décision proposée

Décisions via ADR, RFC pour grande syntaxe, PR petites et auditées, CODEOWNERS selon périmètre, branche protégée, licences évaluées avant ajout d’outils.

## Alternatives à comparer / écarter

Merge automatique sans revue ; ADR réécrits sans historique ; adoption aveugle de code GPL/propriétaire.

## Conséquences et compromis

+ Pérennité, révision facile. - Revue formelle et coût administratif.

## Preuves à produire avant acceptation

Test de protection main, reviewers indépendants sur code critique, politique licence/SBOM et audit des décisions.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Administration trop lourde pour maintenir un noyau ouvert viable.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
