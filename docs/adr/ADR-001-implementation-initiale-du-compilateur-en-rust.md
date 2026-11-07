# ADR-001 — Implémentation initiale du compilateur en Rust

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-000.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Le compilateur est un composant critique, exposé à du code adversarial, et doit rester maintenable.

## Décision proposée

Implémenter le frontend, service, runtime de développement et outils en Rust sûr, avec `unsafe` exceptionnel, audité et instrumenté.

## Alternatives à comparer / écarter

C++ ; OCaml ; Zig ; code compiler généré majoritairement par IA sans revue.

## Conséquences et compromis

+ Écosystème, sûreté et portabilité. - Compilation du compilateur parfois lente ; borrows complexes.

## Preuves à produire avant acceptation

Parser fuzzé avec corpus hostile, Miri sur noyau, profiling pic RAM/CPU, cross-platform CI.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Rust constitue un blocage mesuré de productivité/performance impossible à contourner.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
