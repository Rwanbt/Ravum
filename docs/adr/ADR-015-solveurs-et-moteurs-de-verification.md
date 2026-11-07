# ADR-015 — Solveurs et moteurs de vérification

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-010, ADR-014.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Les propriétés diffèrent : logique SMT, exploration bornée, preuves manuelles et concurrence.

## Décision proposée

Abstraction de backends (Z3/cvc5 + Kani/Verus/Why3 en spikes) ; IR de vérification propre, adaptateurs avec capacités déclarées ; pas de dépendance à un outil unique.

## Alternatives à comparer / écarter

Implémenter un solveur maison ; se lier exclusivement à Kani ; admettre les preuves sans moteur indépendant.

## Conséquences et compromis

+ Flexibilité et comparaisons. - Risque de sémantiques divergentes entre adaptateurs.

## Preuves à produire avant acceptation

Mêmes propriétés sur au moins deux chemins quand possible ; corpus de faux positifs/négatifs et budget mémoire/temps.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Adaptateurs structurellement incapables de préserver le sens du langage.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
