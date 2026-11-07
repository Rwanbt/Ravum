# ADR-010 — Frontend et IR sémantique multi-niveaux

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-002, ADR-003, ADR-004.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Syntaxe incomplète, ownership, preuves et backends ne doivent pas être forcés dans une IR monolithique.

## Décision proposée

CST lossless → HIR typée → MIR ownership/effets → IR de vérification et IR de génération liées par contrats de raffinement.

## Alternatives à comparer / écarter

AST unique mutable ; LLVM IR pour toutes les étapes ; compilateur par regexp/macros.

## Conséquences et compromis

+ Meilleure séparation sémantique/optimisation. - Complexité de mapping diagnostic et vérification de traduction.

## Preuves à produire avant acceptation

Tests de préservation de sémantique, round-trip parsing, source maps précis et differential testing avant/après lowering.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

La duplication des IR nuit fortement aux builds et au contrôle des preuves.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
