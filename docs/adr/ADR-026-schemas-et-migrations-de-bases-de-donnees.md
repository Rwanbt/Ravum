# ADR-026 — Schémas et migrations de bases de données

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-025.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

La validation SQL sur une base de dev ne prouve pas l’état de la base prod.

## Décision proposée

Schémas versionnés, SQL statiquement vérifié contre snapshot attesté, migration expand/contract avec compatibilité n/n-1, contrôle runtime de version et transactions.

## Alternatives à comparer / écarter

ORM masquant SQL ; migration automatique destructive ; requêtes non typées.

## Conséquences et compromis

+ Contrats de données détectables avant déploiement. - Spécificités SQL par backend.

## Preuves à produire avant acceptation

PostgreSQL+SQLite réels, migrations avant/après, rollback contrôlé, tests deadlock/isolation et requêtes paramétrées.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Incompatibilités SQL dialectes invalidant abstractions communes.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
