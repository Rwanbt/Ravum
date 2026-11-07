# ADR-037 — Gestion des paquets, plugins, macros et écosystème

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-028, ADR-029.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

L’écosystème doit rester composable sans permettre des extensions de compilation arbitrairement privilégiées.

## Décision proposée

Paquets signables, lockfiles et politiques de capacités ; macros déclaratives pures d’abord, générateurs procéduraux sandboxés ensuite ; ABI composants stable/versionnée.

## Alternatives à comparer / écarter

Macros procédurales natives non restreintes ; tout code vendored ; registre central unique obligatoire.

## Conséquences et compromis

+ Supply-chain mesurable, extensions auditables. - Limitations de métaprogrammation.

## Preuves à produire avant acceptation

Tests dépendances hostiles, résolution déterministe, compatibilité semver, signature/refus et modes air-gap.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Les bibliothèques critiques ne fonctionnent pas sans escapes non auditables.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
