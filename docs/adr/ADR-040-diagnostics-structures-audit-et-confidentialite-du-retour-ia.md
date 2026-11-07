# ADR-040 — Diagnostics structurés, audit et confidentialité du retour IA

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-027, ADR-032.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Des rapports IA trop verbeux ou peu fiables peuvent augmenter les cycles de développement.

## Décision proposée

IDs stables, sorties typées JSON Schema versionné et SARIF, cause, trace, contre-exemple, scope preuve, coût, liens de source ; minimiser exposition secrets et répétitions.

## Alternatives à comparer / écarter

Chaînes de texte non structurées ; agent ayant accès à toute base de preuve ; IDs dépendant de position ligne.

## Conséquences et compromis

+ Débogage et audit automatiques. - Maintenance des schémas et source maps.

## Preuves à produire avant acceptation

Tests de parsing strict, triage automatique, diagnostics re-localisés après édition, secrets de test jamais exposés.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

La structure imposée empêche des diagnostics essentiels ; proposer un champ extension borné.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
