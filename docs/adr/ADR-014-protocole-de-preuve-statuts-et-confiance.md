# ADR-014 — Protocole de preuve, statuts et confiance

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-004, ADR-013.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

UNSAT solver, test qui passe, compilation et certificat de preuve ne portent pas la même certitude.

## Décision proposée

Status `PROVED/DISPROVED/UNKNOWN/TIMEOUT/UNSUPPORTED/ASSUMED/TESTED` ; chaque résultat mentionne formule, portée, hypothèses, moteur, version, budget, certificat ou base de confiance.

## Alternatives à comparer / écarter

Booléen pass/fail unique ; `UNKNOWN` assimilé au succès ; certificat inventé par un agent.

## Conséquences et compromis

+ Audit honnête et CI stricte. - UX plus riche, qualification des moteurs nécessaire.

## Preuves à produire avant acceptation

Injecter timeouts, modèle partiel, invariants faux, précondition contradictoire, solver crash ; vérifier qu’aucun n’est marqué PROVED.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Un moteur de preuve fiable et plus simple devient disponible.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
