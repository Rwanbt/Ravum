# ADR-034 — Ergonomie, LSP, debug et navigation des preuves

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-011, ADR-027.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

La vérification profonde ne doit pas envahir l’éditeur avec des erreurs peu utiles.

## Décision proposée

LSP standard, diagnostics agrégés/priorisés, code actions et statuts `PROVED`/unknown ; debugger et inspecteur de contrats indépendants de l’IDE.

## Alternatives à comparer / écarter

IDE propriétaire obligatoire ; console seule ; affichage de toutes les obligations à chaque frappe.

## Conséquences et compromis

+ Adoption par humains et agents. - UI complexe et versionnement LSP.

## Preuves à produire avant acceptation

Évaluations tâches concrètes, accessibilité clavier, événements out-of-order, temps à comprendre un contre-exemple.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

La couche UI devient un goulot d’étranglement même avec daemon rapide.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
