# ADR-003 — Erreurs, panic et défaillance à l’exécution

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-002.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Le langage doit minimiser les crashs sans confondre échec métier, exception et défaillance du processus.

## Décision proposée

`Result`/`Option` et propagation explicite, pas d’exception cachée dans le noyau sûr ; `panic` interdit dans les profils strict/realtime, gestion des défaillances externes explicite.

## Alternatives à comparer / écarter

Exceptions non vérifiées ; abort silencieux ; conversions implicites en valeurs par défaut.

## Conséquences et compromis

+ Chemins d’erreur audités et contractuels. - Signatures parfois plus verbeuses.

## Preuves à produire avant acceptation

Cas négatifs overflow/index/null/IO/timeout ; vérification que strict/realtime n’introduisent aucun chemin de panic supporté.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Le coût de gestion explicite annule la fluidité sans améliorer le taux de défauts.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
