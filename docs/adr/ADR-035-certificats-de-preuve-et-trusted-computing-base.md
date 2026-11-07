# ADR-035 — Certificats de preuve et Trusted Computing Base

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-014, ADR-018, ADR-030.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Un résultat `UNSAT` dépend du solveur ; génération binaire dépend du compilateur.

## Décision proposée

Manifeste TCB détaillant solveur, backend et hypothèses ; à moyen terme certificats vérifiables par petit checker pour propriétés compatibles, validation de traductions critiques.

## Alternatives à comparer / écarter

Se proclamer end-to-end formally verified ; aucun traçage de confiance ; réécrire CompCert entier dès le départ.

## Conséquences et compromis

+ Niveau de confiance explicite. - Certificats pas disponibles pour toutes obligations ; effort recherche.

## Preuves à produire avant acceptation

Checker indépendant sur preuve falsifiée et vraie, tests tampering, hash artefact, rapport de couverture TCB.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Aucun format de certificat adapté au moteur/à la propriété retenus.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
