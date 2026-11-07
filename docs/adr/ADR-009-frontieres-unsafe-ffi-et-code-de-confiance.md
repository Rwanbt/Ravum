# ADR-009 — Frontières unsafe, FFI et code de confiance

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-001, ADR-004.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Universalité systèmes/pilotes/GPU implique interopérabilité avec des ABI externes.

## Décision proposée

`unsafe` isolé dans modules spécialisés avec justification, invariants publics, lints, revue obligatoire ; capacités et manifestes de confiance explicites.

## Alternatives à comparer / écarter

Bannir `unsafe` ; autoriser `unsafe` arbitraire ; garantir tacitement que les FFI sont sûres.

## Conséquences et compromis

+ Compatibilité système sans vider les promesses de sûreté. - Coût d’audit et maintenance des wrappers.

## Preuves à produire avant acceptation

Inventaire machine de tous les sites, tests Miri/sanitizers/fuzzing, adversarial ABI/mémoire et revues externes.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Le périmètre de confiance devient trop vaste pour être auditable.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
