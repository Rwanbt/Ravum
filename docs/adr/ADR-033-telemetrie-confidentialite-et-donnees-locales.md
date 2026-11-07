# ADR-033 — Télémétrie, confidentialité et données locales

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-027, ADR-029.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Agents IA et preuves peuvent contenir du code et données confidentielles.

## Décision proposée

Off-line et local-first par défaut ; aucune télémétrie source ou compilation envoyée sans consentement ; redaction, journaux éphémères configurables, rétention limitée.

## Alternatives à comparer / écarter

Télémétrie obligatoire ; stockage cloud source par défaut ; aucune mesure locale possible.

## Conséquences et compromis

+ Souveraineté et confidentialité. - Moins de données de diagnostic distant.

## Preuves à produire avant acceptation

Tests trafic réseau nul en mode offline, secrets dans logs, effacement caches et politique d’opt-in claire.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Un cas d’usage explicitement connecté nécessite partage contrôlé.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
