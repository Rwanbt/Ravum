# ADR-029 — Security-by-default et sandbox du build

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-005, ADR-009.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Scripts de build, macros, dépendances et plugins peuvent accéder à des secrets et au système hôte.

## Décision proposée

Permission explicite de fichiers/réseau/process, sandbox build, dependencies lock, restrictions de plugins et manifeste d’effets ; analyse de flux secrets (taint).

## Alternatives à comparer / écarter

Exécution sans isolation ; confiance implicite crates ; sandbox complète impossible à désactiver.

## Conséquences et compromis

+ Réduction supply-chain/SSRF/exfil. - Compatibilité et limitations OS.

## Preuves à produire avant acceptation

Fixtures malveillantes de script, exfiltration simulée, path traversal, symlink/junction, secrets redaction.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Des outils nécessaires exigent privilèges non compatibles avec le modèle.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
