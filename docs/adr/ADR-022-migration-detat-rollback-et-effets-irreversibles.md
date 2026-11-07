# ADR-022 — Migration d’état, rollback et effets irréversibles

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-021.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Un changement de layout ou de données peut invalider les pointeurs et invariants existants.

## Décision proposée

Hot swap sans migration uniquement si contrats ABI/layout/état compatibles ; sinon migration typée préparée hors chemin critique, validation, commit/rollback. Effects externes ont compensation séparée, jamais rollback fictif.

## Alternatives à comparer / écarter

Réinterprétation de mémoire ; migration automatique supposée ; rollback de tout effet réseau/DB.

## Conséquences et compromis

+ Gestion réaliste de l’état. - Tests de migrations, mémoire temporaire nécessaire.

## Preuves à produire avant acceptation

Chaînes upgrade/downgrade, migration échouée en milieu de parcours, idempotence et compatibilité d’interface.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Le coût de migration domine ; privilégier versions de process et redémarrage contrôlé.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
